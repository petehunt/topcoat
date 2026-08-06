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

The client module registers the same name. The mount function receives the DOM element, decoded props, and an SWR fallback object. Return a cleanup function so Topcoat can unmount the component when htmx or another client removes its island:

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
