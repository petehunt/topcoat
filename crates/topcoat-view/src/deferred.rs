use std::{
    any::Any,
    collections::HashMap,
    future::Future,
    hash::{Hash, Hasher},
    panic::Location,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use futures_util::task::noop_waker_ref;
use topcoat_core::context::Cx;

use crate::identity::Identity;

/// The current value of deferred work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Deferred<T> {
    /// The work is still running. Render placeholder content for this pass.
    Pending,
    /// The work completed and its value is available to this render pass.
    Ready(T),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[doc(hidden)]
pub struct DeferredKey(u128);

#[doc(hidden)]
pub type DeferredFuture =
    Pin<Box<dyn Future<Output = (DeferredKey, Box<dyn Any + Send + Sync>)> + Send + 'static>>;

enum DeferredEntry {
    Pending,
    Ready(Box<dyn Any + Send + Sync>),
}

#[derive(Default)]
#[doc(hidden)]
pub struct DeferredState {
    entries: Mutex<HashMap<DeferredKey, DeferredEntry>>,
    futures: Mutex<Vec<DeferredFuture>>,
}

impl DeferredState {
    #[doc(hidden)]
    #[must_use]
    pub fn has_pending(&self) -> bool {
        self.entries
            .lock()
            .expect("deferred state lock poisoned")
            .values()
            .any(|entry| matches!(entry, DeferredEntry::Pending))
    }

    #[doc(hidden)]
    pub fn take_futures(&self) -> Vec<DeferredFuture> {
        std::mem::take(&mut *self.futures.lock().expect("deferred future lock poisoned"))
    }

    #[doc(hidden)]
    pub fn resolve(&self, key: DeferredKey, value: Box<dyn Any + Send + Sync>) {
        self.entries
            .lock()
            .expect("deferred state lock poisoned")
            .insert(key, DeferredEntry::Ready(value));
    }
}

/// Registers work that may complete after the first response chunk.
///
/// Each call is identified by its component identity and source location. The
/// first render polls `future` once. A future that completes without yielding
/// returns [`Deferred::Ready`] immediately. A future that yields is registered
/// and returns [`Deferred::Pending`]; later render passes return
/// [`Deferred::Ready`] with its completed value.
///
/// # Panics
///
/// Panics if the enclosing component identity is ambiguous, if the same call
/// returns different output types across passes, or if deferred state was
/// poisoned by another panic.
#[track_caller]
pub fn defer<T, F>(cx: &Cx, future: F) -> Deferred<T>
where
    T: Clone + Send + Sync + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let key = deferred_key(Location::caller());
    let state = deferred_state(cx);
    let mut entries = state.entries.lock().expect("deferred state lock poisoned");
    match entries.get(&key) {
        Some(DeferredEntry::Ready(value)) => Deferred::Ready(
            value
                .downcast_ref::<T>()
                .expect("one deferred call returned different types across render passes")
                .clone(),
        ),
        Some(DeferredEntry::Pending) => Deferred::Pending,
        None => {
            entries.insert(key, DeferredEntry::Pending);
            drop(entries);
            let mut future = Box::pin(future);
            match future
                .as_mut()
                .poll(&mut Context::from_waker(noop_waker_ref()))
            {
                Poll::Ready(value) => {
                    state
                        .entries
                        .lock()
                        .expect("deferred state lock poisoned")
                        .insert(key, DeferredEntry::Ready(Box::new(value.clone())));
                    Deferred::Ready(value)
                }
                Poll::Pending => {
                    state
                        .futures
                        .lock()
                        .expect("deferred future lock poisoned")
                        .push(Box::pin(async move {
                            let value = future.await;
                            let value: Box<dyn Any + Send + Sync> = Box::new(value);
                            (key, value)
                        }));
                    Deferred::Pending
                }
            }
        }
    }
}

#[doc(hidden)]
#[must_use]
pub fn deferred_state(cx: &Cx) -> std::sync::Arc<DeferredState> {
    topcoat_core::context::shared_context(cx)
}

fn deferred_key(location: &'static Location<'static>) -> DeferredKey {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    location.file().hash(&mut hasher);
    location.line().hash(&mut hasher);
    location.column().hash(&mut hasher);
    let site = u128::from(hasher.finish());
    DeferredKey(Identity::current().hash() ^ (site << 64) ^ site)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn immediate(cx: &Cx) -> Deferred<u32> {
        defer(cx, async { 42 })
    }

    fn never(cx: &Cx) -> Deferred<()> {
        defer(cx, std::future::pending())
    }

    #[test]
    fn immediately_ready_futures_never_enter_pending() {
        let cx = Cx::default();

        assert_eq!(immediate(&cx), Deferred::Ready(42));
        assert_eq!(immediate(&cx), Deferred::Ready(42));
        assert!(!deferred_state(&cx).has_pending());
        assert!(deferred_state(&cx).take_futures().is_empty());
    }

    #[test]
    fn yielding_futures_are_registered_as_pending() {
        let cx = Cx::default();

        assert_eq!(never(&cx), Deferred::Pending);
        assert!(deferred_state(&cx).has_pending());
        assert_eq!(deferred_state(&cx).take_futures().len(), 1);
    }
}
