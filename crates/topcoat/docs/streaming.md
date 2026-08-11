# Streaming rendering

Streaming rendering sends a page's initial HTML while selected work is still running. Later render passes append small swap instructions that replace only changed regions.

Use [`defer`] to register a future. Topcoat polls a new future once during the first pass. If it completes immediately, that pass receives [`Deferred::Ready`]. If it yields, the pass receives [`Deferred::Pending`] and a new pass receives [`Deferred::Ready`] after completion. The future and its output must be owned, `Send`, and `'static` because they may outlive the page handler. The output must also be `Clone` so every later pass can observe the same completed value.

```rust
use topcoat::{
    Result,
    context::Cx,
    router::page,
    view::{Deferred, boundary, defer, view},
};

#[page("/report")]
async fn report(cx: &Cx) -> Result {
    let content = match defer(cx, async { load_report().await }) {
        Deferred::Pending => view! { <p>"Loading report..."</p> },
        Deferred::Ready(report) => view! { <article>(report)</article> },
    }?;

    view! { (boundary(content)) }
}

# async fn load_report() -> String { String::from("Ready") }
```

The initial response contains the pending branch and a small swap script. Topcoat keeps the HTTP response open, waits for deferred work, and renders the page and its layouts again in the same request context. Completed calls return `Ready`; unfinished calls remain `Pending`. New deferred calls discovered by a later pass are started at that point.

[`boundary`] gives a region a stable identity and marker comments. Topcoat compares boundary hashes between passes and streams `<template data-topcoat-swap>` instructions for changed regions. Nested boundary contents are represented by their identities when the parent is hashed, so a change in a child does not replace its parent. A page without boundaries still works; the document is treated as one root region.

Errors observed in a `Ready` branch use normal Rust control flow. They can bubble through the page and layouts on the later pass. The first chunk has already fixed the HTTP status and headers, so an uncaught late error becomes a root swap instead of changing the status. A late [`RedirectError`](crate::router::error::RedirectError) becomes a browser navigation instruction.

The deferred output must be cloneable. For fallible work, use an error type that implements `Clone`, then apply `?` in the `Ready` branch. Request-scoped memoized functions remain useful because all passes share the same [`Cx`], but a deferred future must own any context handle it uses. [`Cx::detach`](crate::context::Cx::detach) creates that owned handle.

The connection remains open until all deferred work reachable from the page completes. Response middleware must preserve streaming body frames; middleware that buffers the complete body also delays the first paint.

## Server-driven navigation

Add `data-topcoat-navigation` to the document's `<html>` element to make same-origin links use reconciliation instead of a full page load. The browser reads the server-issued hash from each boundary marker and sends the current set in the `X-Topcoat-Boundaries` request header. The server renders the destination route, compares its boundaries with those hashes, and returns only changed `<template>` instructions. Active-link state and all other route decisions still come from the server. The browser only applies the returned boundaries and updates history.

Navigation responses vary on `X-Topcoat-Boundaries`, so shared caches do not mix a full document with a reconciliation response. A missing boundary or a changed boundary structure falls back to a root swap.

[`Cx`]: crate::context::Cx
[`defer`]: crate::view::defer
[`Deferred::Pending`]: crate::view::Deferred::Pending
[`Deferred::Ready`]: crate::view::Deferred::Ready
[`boundary`]: crate::view::boundary
