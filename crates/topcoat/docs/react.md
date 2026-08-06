# React

Topcoat can mount typed [React](https://react.dev) components inside a server-rendered view. Your application bundles React and its component modules with a JavaScript bundler such as esbuild. Topcoat streams the module, serialized props, and optional [SWR](https://swr.vercel.app) fallback data to the browser.

Enable the `react` feature, then declare a handle that pairs one client registration name with one Rust props type and one bundled module:

```rust
use serde::Serialize;
use topcoat::{
    asset::{Asset, asset},
    react::ReactComponent,
};

#[derive(Serialize)]
struct SearchProps {
    placeholder: String,
}

const SEARCH_JS: Asset = asset!("assets/search.js");
const SEARCH: ReactComponent<SearchProps> = ReactComponent::new("search", SEARCH_JS);
```

The component handle accepts only `SearchProps`. Render it from an async Topcoat component or route:

```rust
# use serde::Serialize;
# use topcoat::{Result, asset::{Asset, asset}, context::Cx, react::ReactComponent, view::View};
# #[derive(Serialize)] struct SearchProps { placeholder: String }
# const SEARCH_JS: Asset = asset!("README.md");
# const SEARCH: ReactComponent<SearchProps> = ReactComponent::new("search", SEARCH_JS);
async fn search(cx: &Cx) -> Result<View> {
    SEARCH
        .props(SearchProps {
            placeholder: "Search products".to_owned(),
        })
        .render(cx)
        .await
}
```

The client module registers the same name. The mount function receives the DOM element, decoded props, an SWR fallback object, and a `serverRendered` boolean. Return a cleanup function so Topcoat can unmount the component when htmx or another client removes its island:

```js
import React from "react";
import { createRoot } from "react-dom/client";

function Search(props) {
  return React.createElement("input", { placeholder: props.placeholder });
}

globalThis.topcoat.react.register("search", ({ element, props }) => {
  const root = createRoot(element);
  root.render(React.createElement(Search, props));
  return () => root.unmount();
});
```

Place [`defer_script`](crate::view::defer_script) in the document head. The helper handles modules that register before or after their island reaches the DOM.

## SWR preloads

Call `preload` on the island builder to send fallback data under an SWR key:

```rust
# use serde::Serialize;
# use topcoat::{Result, asset::{Asset, asset}, context::Cx, react::ReactComponent, view::View};
# #[derive(Serialize)] struct SearchProps { placeholder: String }
# #[derive(Serialize)] struct Product { name: String }
# const SEARCH_JS: Asset = asset!("README.md");
# const SEARCH: ReactComponent<SearchProps> = ReactComponent::new("search", SEARCH_JS);
async fn search(cx: &Cx, products: &[Product]) -> Result<View> {
    SEARCH
        .props(SearchProps {
            placeholder: "Search products".to_owned(),
        })
        .preload(cx, "/api/products", products)?
        .render(cx)
        .await
}
```

Wrap the client component in `SWRConfig` with the supplied fallback:

```js
import React from "react";
import { createRoot } from "react-dom/client";
import { SWRConfig } from "swr";

globalThis.topcoat.react.register(
  "search",
  ({ element, props, fallback }) => {
    const root = createRoot(element);
    root.render(
      React.createElement(
        SWRConfig,
        { value: { fallback } },
        React.createElement(Search, props),
      ),
    );
    return () => root.unmount();
  },
);
```

Each SWR key is deduplicated within the response. Sending two values for the same key is an error. The browser waits for component registration, props, and all listed preloads before mounting, regardless of their network or stream arrival order.

## Server rendering

Enable the `react-ssr` feature to render an island with [QuickJS](https://bellard.org/quickjs/) before sending it to the browser. Bundle a separate server entrypoint as an IIFE. It must assign a function to `globalThis.topcoatReactRender`; the function receives the same props and SWR fallback object as the client mount:

```js
import React from "react";
import { renderToString } from "react-dom/server.browser";
import { SWRConfig } from "swr";

globalThis.topcoatReactRender = (props, fallback) =>
  renderToString(
    <SWRConfig value={{ fallback }}>
      <Search {...props} />
    </SWRConfig>,
  );
```

The server bundle is compiled outside Topcoat and included in the application binary. Attach it to the typed component handle:

```rust
# use serde::Serialize;
# use topcoat::{asset::{Asset, asset}, react::{ReactComponent, ReactServerRenderer}};
# #[derive(Serialize)] struct SearchProps { placeholder: String }
# const SEARCH_JS: Asset = asset!("README.md");
const SEARCH_SERVER_JS: &str = "globalThis.topcoatReactRender = () => '<input>'";
const SEARCH: ReactComponent<SearchProps> = ReactComponent::new("search", SEARCH_JS)
    .server_renderer(ReactServerRenderer::new(SEARCH_SERVER_JS));
```

Use `include_str!` for the generated server bundle in an application. Server rendering runs on a blocking worker with a one-second execution limit and a 64 MiB `QuickJS` memory limit. A JavaScript exception or invalid render result returns an error from `render`.

The client registration must hydrate server-rendered markup instead of replacing it:

```js
import { createRoot, hydrateRoot } from "react-dom/client";

globalThis.topcoat.react.register(
  "search",
  ({ element, props, serverRendered }) => {
    const app = <Search {...props} />;
    const root = serverRendered
      ? hydrateRoot(element, app)
      : createRoot(element);
    if (!serverRendered) root.render(app);
    return () => root.unmount();
  },
);
```

The result is an ordinary `View`, so server-rendered islands also work inside the deferred-view pipeline:

```rust
# use serde::Serialize;
# use topcoat::{Result, asset::{Asset, asset}, context::Cx, react::{ReactComponent, ReactServerRenderer}, view::{View, view}};
# #[derive(Serialize)] struct SearchProps { placeholder: String }
# const SEARCH_JS: Asset = asset!("README.md");
# const SEARCH: ReactComponent<SearchProps> = ReactComponent::new("search", SEARCH_JS).server_renderer(ReactServerRenderer::new("globalThis.topcoatReactRender = () => '<input>'"));
async fn deferred_search(props: SearchProps) -> Result<View> {
    let placeholder = view! { <div>"Loading search..."</div> }?;
    Ok(placeholder.defer(move |cx| async move {
        SEARCH.props(props).render(&cx).await
    }))
}
```

The placeholder is sent with the shell. When `QuickJS` finishes, Topcoat streams a patch containing the server-rendered island; the browser then hydrates it after its module, props, and preloads are ready.
