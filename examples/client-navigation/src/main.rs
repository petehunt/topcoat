use std::time::Duration;

use topcoat::{
    Result,
    context::Cx,
    router::{Router, RouterBuilderDiscoverExt, layout, page, path_param, request::uri},
    view::{Deferred, boundary, component, defer, view},
};

const SECTIONS: [(&str, &str); 3] = [
    ("products", "Products"),
    ("company", "Company"),
    ("support", "Support"),
];
const PAGES: [(&str, &str); 3] = [
    ("overview", "Overview"),
    ("details", "Details"),
    ("activity", "Activity"),
];

path_param!(section);
path_param!(content);

#[tokio::main]
async fn main() {
    topcoat::start(Router::builder().discover().build())
        .await
        .unwrap();
}

#[layout("/")]
async fn root_layout(cx: &Cx, slot: Result) -> Result {
    let navigation = match defer(cx, top_navigation, TopNavigationProps {}) {
        Deferred::Pending => view! {
            <header class="shell loading">
                "LOADING TOP-LEVEL LAYOUT - NO NAV YET"
            </header>
        },
        Deferred::Ready(navigation) => navigation,
    }?;

    view! {
        <!DOCTYPE html>
        <html lang="en" data-topcoat-navigation="">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Topcoat server navigation"</title>
                <style>
                    r#"
                    :root { color-scheme: light; font: 16px/1.5 system-ui, sans-serif; }
                    body { margin: 0; background: #f5f7fb; color: #172033; }
                    .shell, main { max-width: 860px; margin: auto; padding: 24px; }
                    .shell > div, nav { display: flex; align-items: center; gap: 12px; }
                    nav { margin-top: 18px; flex-wrap: wrap; }
                    a { color: #46536b; padding: 8px 12px; border-radius: 8px; text-decoration: none; }
                    a:hover { background: #e8edf7; }
                    a.active { color: white; background: #3157d5; }
                    .nested { background: white; border: 1px solid #dfe5f0; border-radius: 14px; padding: 20px; }
                    .content { min-height: 180px; margin-top: 20px; padding: 24px; background: white; border-radius: 14px; box-shadow: 0 8px 30px #25365b12; }
                    .loading { margin-block: 20px; border: 6px dashed #d11; background: #ff0; color: #900; font: bold 20px monospace; text-align: center; }
                    [data-topcoat-navigating] { cursor: progress; }
                    "#
                </style>
                topcoat::dev::script()
            </head>
            <body>
                (boundary(navigation))
                <main>(slot?)</main>
            </body>
        </html>
    }
}

#[component]
async fn top_navigation(cx: &Cx) -> Result {
    topcoat::memoize_global!("top-level-layout", async {
        tokio::time::sleep(Duration::from_millis(180)).await;
    })
    .await;
    let current = uri(cx).path();

    view! {
        <header class="shell">
            <strong>"Topcoat navigation"</strong>
            <nav aria-label="Top-level">
                for (slug, label) in SECTIONS {
                    <a
                        href=(format!("/{slug}/overview"))
                        class=(current
                            .starts_with(&format!("/{slug}/"))
                            .then_some("active"))
                        aria-current=(current
                            .starts_with(&format!("/{slug}/"))
                            .then_some("page"))
                    >
                        (label)
                    </a>
                }
            </nav>
        </header>
    }
}

#[layout("/{section}")]
async fn section_layout(cx: &Cx, slot: Result) -> Result {
    let section = path_param::<Section>(cx);
    valid_section(section)?;
    let navigation = match defer(cx, section_navigation, SectionNavigationProps {}) {
        Deferred::Pending => view! {
            <section class="nested loading">
                "LOADING NESTED LAYOUT - NO NAV YET"
            </section>
        },
        Deferred::Ready(navigation) => navigation,
    }?;

    view! {
        (boundary(navigation))
        (slot?)
    }
}

#[component]
async fn section_navigation(cx: &Cx) -> Result {
    let section = path_param::<Section>(cx);
    let current = path_param::<Content>(cx);
    topcoat::memoize_global!(section.to_owned(), async {
        tokio::time::sleep(Duration::from_millis(260)).await;
    })
    .await;

    view! {
        <section class="nested">
            <strong>(section_name(section))</strong>
            " section"
            <nav aria-label="Nested">
                for (slug, label) in PAGES {
                    <a
                        href=(format!("/{section}/{slug}"))
                        class=((current == slug).then_some("active"))
                        aria-current=((current == slug).then_some("page"))
                    >
                        (label)
                    </a>
                }
            </nav>
        </section>
    }
}

#[page("/{section}/{content}")]
async fn content_page(cx: &Cx) -> Result {
    let section = path_param::<Section>(cx);
    let content = path_param::<Content>(cx);
    valid_section(section)?;
    valid_content(content)?;
    let content_view = match defer(cx, page_content, PageContentProps {}) {
        Deferred::Pending => view! {
            <article class="content loading">
                "LOADING CONTENT PAGE - NOTHING TO USE YET"
            </article>
        },
        Deferred::Ready(content) => content,
    }?;

    view! { (boundary(content_view)) }
}

#[component]
async fn page_content(cx: &Cx) -> Result {
    let section = path_param::<Section>(cx);
    let content = path_param::<Content>(cx);
    topcoat::memoize_global!((section.to_owned(), content.to_owned()), async {
        tokio::time::sleep(Duration::from_millis(340)).await;
    })
    .await;

    view! {
        <article class="content">
            <h1>
                (section_name(section))
                " / "
                (content_name(content))
            </h1>
            <p>
                "This page, both navigation bars, and their active states came from the server. "
                "The browser only reconciled the returned boundaries."
            </p>
        </article>
    }
}

fn valid_section(section: &str) -> Result<()> {
    Ok(SECTIONS
        .iter()
        .any(|(slug, _)| slug == &section)
        .then_some(())
        .ok_or_else(topcoat::router::error::not_found)?)
}

fn valid_content(content: &str) -> Result<()> {
    Ok(PAGES
        .iter()
        .any(|(slug, _)| slug == &content)
        .then_some(())
        .ok_or_else(topcoat::router::error::not_found)?)
}

fn section_name(section: &str) -> &'static str {
    SECTIONS
        .iter()
        .find_map(|(slug, name)| (slug == &section).then_some(*name))
        .unwrap_or("Unknown")
}

fn content_name(content: &str) -> &'static str {
    PAGES
        .iter()
        .find_map(|(slug, name)| (slug == &content).then_some(*name))
        .unwrap_or("Unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_model_has_nine_valid_pages() {
        let pages = SECTIONS
            .iter()
            .flat_map(|(section, _)| PAGES.iter().map(move |(page, _)| (*section, *page)))
            .collect::<Vec<_>>();

        assert_eq!(pages.len(), 9);
        for (section, page) in pages {
            assert!(valid_section(section).is_ok());
            assert!(valid_content(page).is_ok());
        }
    }
}
