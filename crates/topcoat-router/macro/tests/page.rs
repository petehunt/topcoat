use http_body_util::BodyExt;
use serde::Deserialize;
use topcoat::{
    Result,
    context::Cx,
    router::{Body, Router, content::Form, error::redirect, page, request::uri, to_bytes},
    view::{Deferred, boundary, defer, view},
};

mod common;
use common::send;

/// Like [`send`], but with an explicit request method.
async fn send_as(router: &Router, method: &str, path: &str) -> (u16, String) {
    let request = http::Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .unwrap();
    let response = router.handle(request).await;
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[page("/")]
async fn home() -> Result {
    view! { <h1>"home"</h1> }
}

#[derive(Deserialize)]
struct Search {
    q: String,
}

#[derive(Debug, Clone)]
struct LateError;

impl std::fmt::Display for LateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("late failure")
    }
}

impl std::error::Error for LateError {}

// A page that reads a request body through a destructuring pattern.
#[page("/search")]
async fn search(Form(input): Form<Search>) -> Result {
    view! {
        <p>
            "searching for "
            (input.q)
        </p>
    }
}

#[page("/whoami")]
async fn whoami(cx: &Cx) -> Result {
    view! { <p>(uri(cx).path())</p> }
}

// A page whose body binds its own name: the binding must shadow the
// generated marker.
#[page("/shadowed")]
async fn shadowed() -> Result {
    let shadowed = "shadowed";
    view! { <p>(shadowed)</p> }
}

// Pages used as components: called like any component inside `view!`, with a
// request body passed as the already-parsed `body` prop.
#[page("/composed")]
async fn composed() -> Result {
    let query = Search {
        q: String::from("topcoat"),
    };
    view! {
        home()
        search(body: Form(query))
        whoami()
    }
}

// A page serving a method other than the default `GET`.
#[page(POST "/submit")]
async fn submit() -> Result {
    view! { <p>"submitted"</p> }
}

// A page serving several methods at one path.
#[page([GET, POST] "/either")]
async fn either() -> Result {
    view! { <p>"either"</p> }
}

// A page serving every method.
#[page(* "/anything")]
async fn anything() -> Result {
    view! { <p>"anything"</p> }
}

#[page("/stream")]
async fn stream(cx: &Cx) -> Result {
    let content = match defer(cx, async { 42_u8 }) {
        Deferred::Pending => view! { <p>"loading"</p> },
        Deferred::Ready(value) => view! { <p>(value)</p> },
    }?;
    view! { (boundary(content)) }
}

#[page("/stream-redirect")]
async fn stream_redirect(cx: &Cx) -> Result {
    match defer(cx, async { redirect("/target") }) {
        Deferred::Pending => view! { <p>"waiting"</p> },
        Deferred::Ready(error) => Err(error.into()),
    }
}

#[page("/stream-pending")]
async fn stream_pending(cx: &Cx) -> Result {
    match defer(cx, std::future::pending::<u8>()) {
        Deferred::Pending => view! { <p>"first"</p> },
        Deferred::Ready(value) => view! { <p>(value)</p> },
    }
}

#[page("/stream-error")]
async fn stream_error(cx: &Cx) -> Result {
    match defer(cx, async { LateError }) {
        Deferred::Pending => view! { <p>"waiting"</p> },
        Deferred::Ready(error) => Err(error.into()),
    }
}

#[page("/stream-chain")]
async fn stream_chain(cx: &Cx) -> Result {
    let content = match defer(cx, async { 1_u8 }) {
        Deferred::Pending => view! { <p>"first pending"</p> },
        Deferred::Ready(_) => match defer(cx, async { 2_u8 }) {
            Deferred::Pending => view! { <p>"second pending"</p> },
            Deferred::Ready(value) => view! {
                <p>
                    "done "
                    (value)
                </p>
            },
        },
    }?;
    view! { (boundary(content)) }
}

#[tokio::test]
async fn renders_a_page_registered_by_name() {
    let router = Router::builder().page(home).build();
    let (status, body) = send(&router, "/").await;
    assert_eq!(status, 200);
    assert_eq!(body, "<h1>home</h1>");
}

#[tokio::test]
async fn a_page_serves_get_by_default() {
    let router = Router::builder().page(home).build();
    assert_eq!(send_as(&router, "POST", "/").await.0, 405);
}

#[tokio::test]
async fn a_page_can_declare_another_method() {
    let router = Router::builder().page(submit).build();
    let (status, body) = send_as(&router, "POST", "/submit").await;
    assert_eq!(status, 200);
    assert_eq!(body, "<p>submitted</p>");
    assert_eq!(send_as(&router, "GET", "/submit").await.0, 405);
}

#[tokio::test]
async fn a_page_can_declare_a_method_list() {
    let router = Router::builder().page(either).build();
    for method in ["GET", "POST"] {
        let (status, body) = send_as(&router, method, "/either").await;
        assert_eq!(status, 200);
        assert_eq!(body, "<p>either</p>");
    }
}

#[tokio::test]
async fn a_star_page_serves_every_method() {
    let router = Router::builder().page(anything).build();
    for method in ["GET", "POST", "PUT", "DELETE", "PATCH"] {
        assert_eq!(
            send_as(&router, method, "/anything").await,
            (200, "<p>anything</p>".into())
        );
    }
}

#[tokio::test]
async fn parses_the_request_body_of_a_registered_page() {
    let router = Router::builder().page(search).build();
    let (status, body) = send(&router, "/search?q=topcoat").await;
    assert_eq!(status, 200);
    assert_eq!(body, "<p>searching for topcoat</p>");
}

#[tokio::test]
async fn a_binding_shadows_the_page_marker() {
    let router = Router::builder().page(shadowed).build();
    let (status, body) = send(&router, "/shadowed").await;
    assert_eq!(status, 200);
    assert_eq!(body, "<p>shadowed</p>");
}

#[tokio::test]
async fn renders_pages_as_components() {
    let router = Router::builder().page(composed).build();
    let (status, body) = send(&router, "/composed").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        "<h1>home</h1><p>searching for topcoat</p><p>/composed</p>"
    );
}

#[tokio::test]
async fn deferred_pages_stream_a_boundary_swap() {
    let router = Router::builder().page(stream).build();
    let (status, body) = send(&router, "/stream").await;

    assert_eq!(status, 200);
    assert!(body.contains("<p>loading</p>"));
    assert!(body.contains("data-topcoat-stream"));
    assert!(body.contains("<template data-topcoat-swap="));
    assert!(body.contains("<p>42</p></template>"));
}

#[tokio::test]
async fn compressed_deferred_pages_finish_the_body_stream() {
    let router = Router::builder().page(stream).build();
    let request = http::Request::builder()
        .uri("/stream")
        .header(http::header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .unwrap();
    let response = router.handle(request).await;

    assert_eq!(response.headers()[http::header::CONTENT_ENCODING], "gzip");
    assert!(
        !to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn navigation_requests_receive_only_server_reconciliation_chunks() {
    let router = Router::builder().page(stream).build();
    let (_, initial) = send(&router, "/stream").await;
    let marker = initial
        .split_once("<!--topcoat-boundary ")
        .unwrap()
        .1
        .split_once("-->")
        .unwrap()
        .0;
    let id = marker.split_whitespace().next().unwrap();
    let request = http::Request::builder()
        .uri("/stream")
        .header("x-topcoat-boundaries", format!("{id}=0000000000000000"))
        .body(Body::empty())
        .unwrap();
    let response = router.handle(request).await;
    assert!(
        response
            .headers()
            .get_all(http::header::VARY)
            .iter()
            .any(|value| value == "x-topcoat-boundaries")
    );
    let body = String::from_utf8(
        to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();

    assert!(body.contains("data-topcoat-navigation"));
    assert!(body.contains("data-topcoat-swap"));
    assert!(body.contains("data-topcoat-hash"));
    assert!(!body.contains("data-topcoat-stream"));
    assert!(!body.contains("<!DOCTYPE"));
}

#[tokio::test]
async fn redirects_after_streaming_become_navigation_instructions() {
    let router = Router::builder().page(stream_redirect).build();
    let (status, body) = send(&router, "/stream-redirect").await;

    assert_eq!(status, 200);
    assert!(body.contains("<p>waiting</p>"));
    assert!(body.contains("<template data-topcoat-redirect=\"/target\"></template>"));
}

#[tokio::test]
async fn the_initial_html_is_available_before_deferred_work_finishes() {
    let router = Router::builder().page(stream_pending).build();
    let request = http::Request::builder()
        .uri("/stream-pending")
        .body(Body::empty())
        .unwrap();
    let mut response = router.handle(request).await;
    let frame = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        response.body_mut().frame(),
    )
    .await
    .expect("the initial frame should not wait for deferred work")
    .unwrap()
    .unwrap();
    let body = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
    assert!(body.contains("<p>first</p>"));
    assert!(body.contains("data-topcoat-stream"));
}

#[tokio::test]
async fn errors_after_streaming_become_root_swaps() {
    let router = Router::builder().page(stream_error).build();
    let (status, body) = send(&router, "/stream-error").await;

    assert_eq!(status, 200);
    assert!(body.contains("<p>waiting</p>"));
    assert!(body.contains("<template data-topcoat-swap=\"root\">internal server error</template>"));
}

#[tokio::test]
async fn later_passes_can_discover_more_deferred_work() {
    let router = Router::builder().page(stream_chain).build();
    let (status, body) = send(&router, "/stream-chain").await;

    assert_eq!(status, 200);
    assert!(body.contains("<p>first pending</p>"));
    assert!(body.contains("<p>second pending</p></template>"));
    assert!(body.contains("<p>done 2</p></template>"));
}
