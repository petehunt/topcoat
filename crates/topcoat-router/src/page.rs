use std::{borrow::Cow, pin::Pin};

use bytes::Bytes;
use futures_util::{
    StreamExt,
    stream::{self, FuturesUnordered},
};
use http_body::Frame;
use http_body_util::StreamBody;
use topcoat_core::{
    context::Cx,
    error::{Error, Result},
};
use topcoat_view::{DeferredFuture, DeferredState, View, deferred_state};

use crate::{
    Body, BoxError, IntoPath, Methods, OwnedMethods, Path, Route, RouteFuture,
    content::Html,
    error::{RedirectError, respond},
    reconcile::{SWAP_SCRIPT, Snapshot, redirect_template, swap_template},
    response::{IntoResponse, Response},
};

/// The async render function backing a [`PageFn`].
pub type PageRenderFn = for<'cx> fn(
    cx: &'cx Cx,
    body: Body,
) -> Pin<Box<dyn Future<Output = Result<View>> + Send + 'cx>>;

/// A page handler, backed by a plain render function, that renders a [`View`]
/// for a specific URL path.
///
/// Created either manually via `#[page("/path")]` or by the module router
/// (which derives the path from the module tree). Registered into a
/// [`RouterBuilder`](crate::RouterBuilder) alongside [`LayoutFn`]s, which wrap
/// it when their path is a prefix of the page's.
///
/// A page serves `GET` unless it declares other methods, either in the macro
/// (`#[page(POST "/path")]`) or through [`PageFn::new`].
#[derive(Debug, Clone)]
pub struct PageFn {
    /// The HTTP methods this page responds to.
    methods: OwnedMethods,
    /// The URL path this page handles.
    path: Cow<'static, Path>,
    /// The async render function that produces the page [`View`].
    render: PageRenderFn,
}

impl PageFn {
    /// Creates a new page with explicit methods, path, and render function.
    ///
    /// The methods are anything convertible into [`OwnedMethods`]: a single
    /// [`Method`](crate::Method), a `&'static [Method]`, a `Vec<Method>`, or
    /// [`Methods::Any`] to respond to every method.
    ///
    /// # Panics
    ///
    /// Panics if `path` is a string that is not a well-formed route path.
    #[track_caller]
    pub fn new(
        methods: impl Into<OwnedMethods>,
        path: impl IntoPath,
        render: PageRenderFn,
    ) -> Self {
        Self::const_new(methods.into(), path.into_path(), render)
    }

    /// Const-context constructor used by macro-generated code.
    pub const fn const_new(
        methods: OwnedMethods,
        path: Cow<'static, Path>,
        render: PageRenderFn,
    ) -> Self {
        Self {
            methods,
            path,
            render,
        }
    }

    /// Returns the HTTP methods this page responds to.
    #[must_use]
    pub fn methods(&self) -> Methods<'_> {
        self.methods.as_methods()
    }

    /// Returns the URL path this page handles.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Renders the page, returning a [`Result`].
    #[must_use]
    pub fn render<'cx>(
        &self,
        cx: &'cx Cx,
        body: Body,
    ) -> Pin<Box<dyn Future<Output = Result<View>> + Send + 'cx>> {
        (self.render)(cx, body)
    }
}

#[cfg(feature = "discover")]
inventory::collect!(PageFn);

/// The async render function backing a [`LayoutFn`], receiving the rendered child content as a
/// [`Result`]`<`[`View`]`>`.
pub type LayoutRenderFn = for<'cx> fn(
    cx: &'cx Cx,
    slot: Result<View>,
) -> Pin<Box<dyn Future<Output = Result<View>> + Send + 'cx>>;

/// A layout handler, backed by a plain render function, that wraps pages whose
/// path starts with the layout's path prefix.
///
/// When multiple layouts match a page, they nest from most-specific (innermost)
/// to least-specific (outermost). For example, layouts at `/` and `/settings`
/// both match `/settings/profile`, rendering as: root -> settings -> page.
#[derive(Debug, Clone)]
pub struct LayoutFn {
    /// The path prefix this layout applies to.
    path: Cow<'static, Path>,
    /// The async render function that wraps the child content [`Result`]`<`[`View`]`>`.
    render: LayoutRenderFn,
}

impl LayoutFn {
    /// Creates a new layout with an explicit path and render function.
    pub const fn new(path: Cow<'static, Path>, render: LayoutRenderFn) -> Self {
        Self { path, render }
    }

    /// Returns the path prefix this layout applies to.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Renders the layout, embedding the given child content [`Result`]`<`[`View`]`>` as its slot.
    #[must_use]
    pub fn render<'cx>(
        &self,
        cx: &'cx Cx,
        slot: Result<View>,
    ) -> Pin<Box<dyn Future<Output = Result<View>> + Send + 'cx>> {
        (self.render)(cx, slot)
    }
}

#[cfg(feature = "discover")]
inventory::collect!(LayoutFn);

/// A [`PageFn`] paired with the [`LayoutFn`]s that wrap it.
#[derive(Clone)]
pub struct PageWithLayouts {
    page: PageFn,
    /// The matching layouts, ordered by ascending path length (outermost first).
    layouts: Vec<LayoutFn>,
}

impl PageWithLayouts {
    /// Pairs `page` with the `layouts` that wrap it.
    ///
    /// `layouts` must be ordered from least- to most-specific (ascending path
    /// length); they are applied from the innermost (most specific) outward.
    #[must_use]
    pub fn new(page: PageFn, layouts: Vec<LayoutFn>) -> Self {
        Self { page, layouts }
    }

    async fn render_view(&self, cx: &Cx, body: Bytes) -> Result<View> {
        let mut slot = self.page.render(cx, Body::from(body)).await;
        for layout in self.layouts.iter().rev() {
            slot = layout.render(cx, slot).await;
        }
        slot
    }
}

impl Route for PageWithLayouts {
    fn methods(&self) -> Methods<'_> {
        self.page.methods()
    }

    fn path(&self) -> &Path {
        &self.page.path
    }

    fn handle<'cx>(&'cx self, cx: &'cx Cx, body: Body) -> RouteFuture<'cx> {
        Box::pin(async move {
            let body = crate::body::to_bytes(body, crate::body_limit(cx)).await?;
            let view = self.render_view(cx, body.clone()).await?;
            let deferred = deferred_state(cx);
            if !deferred.has_pending() {
                return view.into_response(cx);
            }

            let rendered = view.render_response(cx);
            let snapshot = Snapshot::parse(rendered.html.clone());
            let mut first = rendered.html;
            first.push_str(SWAP_SCRIPT);
            let stream = StreamingPage {
                page: self.clone(),
                cx: cx.detach(),
                body,
                deferred,
                pending: FuturesUnordered::new(),
                snapshot,
            };
            stream.response(first, rendered.status_code, rendered.headers, cx)
        })
    }
}

struct StreamingPage {
    page: PageWithLayouts,
    cx: Cx,
    body: Bytes,
    deferred: std::sync::Arc<DeferredState>,
    pending: FuturesUnordered<DeferredFuture>,
    snapshot: Snapshot,
}

impl StreamingPage {
    fn response(
        mut self,
        first: String,
        status: Option<http::StatusCode>,
        headers: http::HeaderMap,
        cx: &Cx,
    ) -> Result<Response> {
        self.pending.extend(self.deferred.take_futures());
        let first = stream::once(async move { Ok::<_, BoxError>(Frame::data(Bytes::from(first))) });
        let rest = stream::unfold(self, |mut state| async move {
            state
                .next_chunk()
                .await
                .map(|chunk| (Ok::<_, BoxError>(Frame::data(Bytes::from(chunk))), state))
        });
        let mut response = Html(String::new()).into_response(cx)?;
        *response.body_mut() = Body::new(StreamBody::new(first.chain(rest).fuse()));
        if let Some(status) = status {
            *response.status_mut() = status;
        }
        response.headers_mut().extend(headers);
        Ok(response)
    }

    async fn next_chunk(&mut self) -> Option<String> {
        loop {
            let (key, value) = self.pending.next().await?;
            self.deferred.resolve(key, value);
            let chunk = match self.page.render_view(&self.cx, self.body.clone()).await {
                Ok(view) => {
                    let next = Snapshot::parse(view.render_response(&self.cx).html);
                    let (snapshot, chunk) = self.snapshot.reconcile(next);
                    self.snapshot = snapshot;
                    chunk
                }
                Err(error) => self.error_chunk(error).await,
            };
            self.pending.extend(self.deferred.take_futures());
            if !chunk.is_empty() {
                return Some(chunk);
            }
        }
    }

    async fn error_chunk(&mut self, error: Error) -> String {
        match error.downcast::<RedirectError>() {
            Ok(redirect) => redirect.location().to_str().map_or_else(
                |_| swap_template("root", "internal server error"),
                redirect_template,
            ),
            Err(error) => {
                let response = respond(&self.cx, error);
                let body = crate::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap_or_else(|_| Bytes::from_static(b"internal server error"));
                swap_template("root", &String::from_utf8_lossy(&body))
            }
        }
    }
}
