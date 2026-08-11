use std::{
    collections::HashMap,
    future::Future,
    hash::{Hash, Hasher},
    panic::Location,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

use futures_util::task::noop_waker_ref;
use topcoat_core::{context::Cx, error::Result};

use crate::{
    Component, View,
    identity::{Identity, IdentityFuture, SiteKey},
};

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
pub type DeferredFuture = Pin<Box<dyn Future<Output = DeferredKey> + Send + 'static>>;

enum DeferredEntry {
    Pending,
    Ready,
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
    pub fn resolve(&self, key: DeferredKey) {
        self.entries
            .lock()
            .expect("deferred state lock poisoned")
            .insert(key, DeferredEntry::Ready);
    }
}

/// Renders a component that may complete after the first response chunk.
///
/// Each call is identified by its component identity and source location. The
/// component is polled once on each render pass. A component that completes
/// without yielding returns [`Deferred::Ready`] immediately. A component that
/// yields is registered and returns [`Deferred::Pending`]. Once it completes,
/// the page renders again and reconstructs the component. Data loaded by the
/// component should be memoized so the new render completes immediately.
///
/// # Panics
///
/// Panics if the enclosing component identity is ambiguous, if the same call
/// is poisoned by another panic.
#[track_caller]
pub fn defer<C>(cx: &Cx, component: C, props: C::Props) -> Deferred<Result<View>>
where
    C: Component + 'static,
    C::Props: 'static,
{
    let location = Location::caller();
    let site = SiteKey::new(location.file(), location.line(), location.column(), 0);
    let owned_cx = cx.detach();
    defer_future(
        cx,
        IdentityFuture::new(site, async move {
            Component::render(component, &owned_cx, props).await
        }),
        location,
    )
}

fn defer_future<T, F>(cx: &Cx, future: F, location: &'static Location<'static>) -> Deferred<T>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let key = deferred_key(location);
    let state = deferred_state(cx);
    let entries = state.entries.lock().expect("deferred state lock poisoned");
    match entries.get(&key) {
        Some(DeferredEntry::Pending) => Deferred::Pending,
        Some(DeferredEntry::Ready) | None => {
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
                        .insert(key, DeferredEntry::Ready);
                    Deferred::Ready(value)
                }
                Poll::Pending => {
                    state
                        .entries
                        .lock()
                        .expect("deferred state lock poisoned")
                        .insert(key, DeferredEntry::Pending);
                    state
                        .futures
                        .lock()
                        .expect("deferred future lock poisoned")
                        .push(Box::pin(async move {
                            let _ = future.await;
                            key
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
        defer_future(cx, async { 42 }, Location::caller())
    }

    fn never(cx: &Cx) -> Deferred<()> {
        defer_future(cx, std::future::pending(), Location::caller())
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
