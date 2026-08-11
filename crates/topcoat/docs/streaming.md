# Streaming rendering

Streaming rendering sends a page's initial HTML while selected work is still running. Later render passes append small swap instructions that replace only changed regions.

Use [`defer`] to render a component outside the first response chunk. Topcoat polls the component once during each pass. If it completes immediately, that pass receives [`Deferred::Ready`] with the component's result. If it yields, the pass receives [`Deferred::Pending`]. Once the component finishes, Topcoat renders the page again and constructs the component again. Memoize the component's data loads so this new render completes immediately instead of repeating the work.

```rust
use topcoat::{
    Result,
    context::{Cx, memoize},
    router::page,
    view::{Deferred, boundary, component, defer, view},
};

#[memoize]
async fn load_report(cx: &Cx) -> String {
    let _ = cx;
    String::from("Ready")
}

#[component]
async fn report_content(cx: &Cx) -> Result {
    let loaded_report = load_report(cx).await;
    view! { <article>(loaded_report)</article> }
}

#[page("/report")]
async fn report(cx: &Cx) -> Result {
    let content = match defer(cx, report_content, ReportContentProps {}) {
        Deferred::Pending => view! { <p>"Loading report..."</p> },
        Deferred::Ready(content) => content,
    }?;

    view! { (boundary(content)) }
}
```

The initial response contains the pending branch and a small swap script. Topcoat keeps the HTTP response open, waits for deferred components, and renders the page and its layouts again in the same request context. Components that now finish during their eager poll return `Ready`; unfinished components remain `Pending`. New deferred components discovered by a later pass are started at that point.

[`boundary`] gives a region a stable identity and marker comments. Topcoat compares boundary hashes between passes and streams `<template data-topcoat-swap>` instructions for changed regions. Nested boundary contents are represented by their identities when the parent is hashed, so a change in a child does not replace its parent. A page without boundaries still works; the document is treated as one root region.

The `Ready` value is the component's normal `Result<View>`, so `?` uses normal Rust control flow. Errors can bubble through the page and layouts on the later pass. The first chunk has already fixed the HTTP status and headers, so an uncaught late error becomes a root swap instead of changing the status. A late [`RedirectError`](crate::router::error::RedirectError) becomes a browser navigation instruction.

`defer` does not retain the component's output. It retains only the fact that the pending component finished, then asks the next render pass to construct it again. Request-scoped [`#[memoize]`](crate::context::memoize) works because all passes share the same [`Cx`]. Process-wide caches can make the same data immediately available across requests.

The connection remains open until all deferred work reachable from the page completes. Response middleware must preserve streaming body frames; middleware that buffers the complete body also delays the first paint.

## Server-driven navigation

Add `data-topcoat-navigation` to the document's `<html>` element to make same-origin links use reconciliation instead of a full page load. The browser reads the server-issued hash from each boundary marker and sends the current set in the `X-Topcoat-Boundaries` request header. The server renders the destination route, compares its boundaries with those hashes, and returns only changed `<template>` instructions. Active-link state and all other route decisions still come from the server. The browser only applies the returned boundaries and updates history.

Navigation responses vary on `X-Topcoat-Boundaries`, so shared caches do not mix a full document with a reconciliation response. A missing boundary or a changed boundary structure falls back to a root swap.

[`Cx`]: crate::context::Cx
[`defer`]: crate::view::defer
[`Deferred::Pending`]: crate::view::Deferred::Pending
[`Deferred::Ready`]: crate::view::Deferred::Ready
[`boundary`]: crate::view::boundary
