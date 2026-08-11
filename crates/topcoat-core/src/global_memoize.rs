use std::{
    any::Any,
    future::Future,
    sync::{Arc, Mutex},
};

use futures_util::future::{BoxFuture, FutureExt, Shared};

type ErasedValue = Arc<dyn Any + Send + Sync>;
type SharedLoad = Shared<BoxFuture<'static, ErasedValue>>;

trait ErasedKey: Send + Sync {
    fn as_any(&self) -> &dyn Any;
}

impl<T: Send + Sync + 'static> ErasedKey for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct Entry {
    key: Box<dyn ErasedKey>,
    state: EntryState,
}

enum EntryState {
    Loading(SharedLoad),
    Ready(ErasedValue),
}

/// A process-wide cache backing one `memoize_global!` call site.
#[doc(hidden)]
pub struct GlobalMemoizeCache {
    entries: Mutex<Vec<Entry>>,
}

impl GlobalMemoizeCache {
    /// Creates an empty cache.
    #[doc(hidden)]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }
}

/// Returns a cached value or shares one in-flight load across callers.
#[doc(hidden)]
pub async fn global_memoize<K, T, F>(cache: &'static GlobalMemoizeCache, key: K, load: F) -> T
where
    K: Clone + Eq + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let shared = {
        let mut entries = cache
            .entries
            .lock()
            .expect("global memoize cache lock poisoned");
        if let Some(entry) = find_entry(&entries, &key) {
            match &entry.state {
                EntryState::Ready(value) => return clone_value(value),
                EntryState::Loading(shared) => shared.clone(),
            }
        } else {
            let shared = load
                .map(|value| Arc::new(value) as ErasedValue)
                .boxed()
                .shared();
            entries.push(Entry {
                key: Box::new(key.clone()),
                state: EntryState::Loading(shared.clone()),
            });
            shared
        }
    };

    let value = shared.await;
    let mut entries = cache
        .entries
        .lock()
        .expect("global memoize cache lock poisoned");
    let entry =
        find_entry_mut(&mut entries, &key).expect("global memoize entry disappeared while loading");
    entry.state = EntryState::Ready(value.clone());
    clone_value(&value)
}

fn find_entry<'a, K>(entries: &'a [Entry], key: &K) -> Option<&'a Entry>
where
    K: Eq + 'static,
{
    entries.iter().find(|entry| {
        entry
            .key
            .as_ref()
            .as_any()
            .downcast_ref::<K>()
            .is_some_and(|stored| stored == key)
    })
}

fn find_entry_mut<'a, K>(entries: &'a mut [Entry], key: &K) -> Option<&'a mut Entry>
where
    K: Eq + 'static,
{
    entries.iter_mut().find(|entry| {
        entry
            .key
            .as_ref()
            .as_any()
            .downcast_ref::<K>()
            .is_some_and(|stored| stored == key)
    })
}

fn clone_value<T>(value: &ErasedValue) -> T
where
    T: Clone + 'static,
{
    value
        .downcast_ref::<T>()
        .expect("one global memoize call site returned different types")
        .clone()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static CACHE: GlobalMemoizeCache = GlobalMemoizeCache::new();
    static LOADS: AtomicUsize = AtomicUsize::new(0);
    static SHARED_CACHE: GlobalMemoizeCache = GlobalMemoizeCache::new();
    static SHARED_LOADS: AtomicUsize = AtomicUsize::new(0);

    #[tokio::test]
    async fn caches_completed_values_across_calls() {
        let first = global_memoize(&CACHE, "one", async {
            LOADS.fetch_add(1, Ordering::Relaxed);
            42
        })
        .await;
        let second = global_memoize(&CACHE, "one", async {
            LOADS.fetch_add(1, Ordering::Relaxed);
            99
        })
        .await;

        assert_eq!((first, second), (42, 42));
        assert_eq!(LOADS.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn keys_cache_independently() {
        assert_eq!(global_memoize(&CACHE, "two", async { 2 }).await, 2);
        assert_eq!(global_memoize(&CACHE, "three", async { 3 }).await, 3);
    }

    #[tokio::test]
    async fn concurrent_misses_share_the_in_flight_load() {
        let first = global_memoize(&SHARED_CACHE, (), async {
            SHARED_LOADS.fetch_add(1, Ordering::Relaxed);
            tokio::task::yield_now().await;
            7
        });
        let second = global_memoize(&SHARED_CACHE, (), async {
            SHARED_LOADS.fetch_add(1, Ordering::Relaxed);
            9
        });
        let (first, second) = tokio::join!(first, second);

        assert_eq!((first, second), (7, 7));
        assert_eq!(SHARED_LOADS.load(Ordering::Relaxed), 1);
    }
}
