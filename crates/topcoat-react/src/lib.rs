#![cfg_attr(docsrs, feature(doc_cfg))]
//! Typed React islands for Topcoat.

#[cfg(feature = "ssr")]
mod server;

use core::marker::PhantomData;

use serde::Serialize;
#[cfg(feature = "ssr")]
use serde::ser::{SerializeMap, Serializer};
#[cfg(feature = "ssr")]
pub use server::*;
use topcoat_asset::{Asset, CxAssetExt};
use topcoat_core::{
    context::{Cx, JsonKey},
    error::Result,
};
use topcoat_view::{Unescaped, View};
use topcoat_view_macro::view;

const SWR_JSON_PREFIX: &str = "@topcoat/swr/";

/// A client React component with one Rust props type and one bundled module.
#[derive(Debug)]
pub struct ReactComponent<Props> {
    name: &'static str,
    module: Asset,
    #[cfg(feature = "ssr")]
    server_renderer: Option<ReactServerRenderer>,
    props: PhantomData<fn() -> Props>,
}

impl<Props> Clone for ReactComponent<Props> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Props> Copy for ReactComponent<Props> {}

impl<Props> ReactComponent<Props> {
    /// Declares the registered client name and JavaScript module for a component.
    #[must_use]
    pub const fn new(name: &'static str, module: Asset) -> Self {
        Self {
            name,
            module,
            #[cfg(feature = "ssr")]
            server_renderer: None,
            props: PhantomData,
        }
    }

    /// Enables server rendering with a bundled `QuickJS` script.
    #[cfg(feature = "ssr")]
    #[cfg_attr(docsrs, doc(cfg(feature = "ssr")))]
    #[must_use]
    pub const fn server_renderer(mut self, renderer: ReactServerRenderer) -> Self {
        self.server_renderer = Some(renderer);
        self
    }

    /// Starts an island builder with this component's typed props.
    #[must_use]
    pub fn props(self, props: Props) -> ReactIsland<Props> {
        ReactIsland {
            component: self,
            props,
            preloads: Vec::new(),
        }
    }
}

/// A React component invocation and the SWR data it needs before mounting.
#[derive(Debug)]
pub struct ReactIsland<Props> {
    component: ReactComponent<Props>,
    props: Props,
    preloads: Vec<SWRPreload>,
}

impl<Props> ReactIsland<Props> {
    /// Streams data into SWR's fallback cache before this island mounts.
    ///
    /// Reusing a key with the same value is deduplicated for the response.
    /// Reusing it with a different value returns an error.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails or the key already has a
    /// different value in this response.
    pub fn preload<T>(mut self, cx: &Cx, key: impl Into<String>, value: &T) -> Result<Self>
    where
        T: Serialize + ?Sized,
    {
        let key = key.into();
        #[cfg(feature = "ssr")]
        let value = serde_json::to_value(value)?;
        #[cfg(feature = "ssr")]
        let json_key = cx.send_json_internal(format!("{SWR_JSON_PREFIX}{key}"), &value)?;
        #[cfg(not(feature = "ssr"))]
        let json_key = cx.send_json_internal(format!("{SWR_JSON_PREFIX}{key}"), value)?;
        if !self.preloads.iter().any(|preload| preload.key == key) {
            self.preloads.push(SWRPreload {
                key,
                json_key,
                #[cfg(feature = "ssr")]
                value,
            });
        }
        Ok(self)
    }
}

impl<Props> ReactIsland<Props>
where
    Props: Serialize,
{
    /// Requires the component bundle and renders its mount point.
    ///
    /// The browser waits for the module registration, typed props, and every
    /// SWR preload before it calls the registered mount function.
    ///
    /// # Errors
    ///
    /// Returns an error if an asset requirement conflicts or the payload
    /// cannot be serialized. With server rendering enabled, it also returns
    /// an error if `QuickJS` cannot evaluate the bundle or render the component.
    pub async fn render(self, cx: &Cx) -> Result<View> {
        cx.require_asset(self.component.module.module())?;

        #[cfg(feature = "ssr")]
        let server_html = match self.component.server_renderer {
            Some(renderer) => {
                let props = serde_json::to_string(&self.props)?;
                let fallback = serde_json::to_string(&SWRFallback(&self.preloads))?;
                Some(
                    tokio::task::spawn_blocking(move || renderer.render(&props, &fallback))
                        .await??,
                )
            }
            None => None,
        };
        #[cfg(not(feature = "ssr"))]
        let server_html: Option<String> = None;
        let payload = IslandPayload {
            props: &self.props,
            preloads: self.preloads.iter().map(SWRPreload::payload).collect(),
        };
        let payload_key = cx.send_json(&payload)?;

        let server_rendered = server_html.is_some();
        let server_html = server_html.map(Unescaped::new_unchecked);

        view! {
            cx =>
            <div
                data-topcoat-react=(self.component.name)
                data-topcoat-react-payload=(payload_key.as_str())
                data-topcoat-react-ssr=(server_rendered)
            >
                (server_html)
            </div>
        }
    }
}

#[derive(Debug)]
struct SWRPreload {
    key: String,
    json_key: JsonKey,
    #[cfg(feature = "ssr")]
    value: serde_json::Value,
}

#[cfg(feature = "ssr")]
struct SWRFallback<'a>(&'a [SWRPreload]);

#[cfg(feature = "ssr")]
impl Serialize for SWRFallback<'_> {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for preload in self.0 {
            map.serialize_entry(&preload.key, &preload.value)?;
        }
        map.end()
    }
}

impl SWRPreload {
    fn payload(&self) -> SWRPreloadPayload<'_> {
        SWRPreloadPayload {
            key: &self.key,
            json_key: self.json_key.as_str(),
        }
    }
}

#[derive(Serialize)]
struct IslandPayload<'a, Props> {
    props: &'a Props,
    preloads: Vec<SWRPreloadPayload<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SWRPreloadPayload<'a> {
    key: &'a str,
    json_key: &'a str,
}

#[cfg(test)]
mod tests {
    use serde::Serialize;
    use topcoat_asset::{AssetConfig, Manifest, asset};
    use topcoat_core::{context::CxTestBuilder, response_event::ResponseEvent};

    use super::*;

    const MODULE: Asset = asset!("tests/fixtures/component.js");
    #[cfg(feature = "ssr")]
    const SERVER_RENDERER: ReactServerRenderer = ReactServerRenderer::new(
        r#"
globalThis.topcoatReactRender = (props, fallback) =>
  `<label>${props.label}:${fallback["/api/products"].join(",")}</label>`;
"#,
    );

    #[derive(Debug, Serialize)]
    struct Props {
        label: &'static str,
    }

    fn cx() -> Cx {
        let manifest = Manifest::parse(&format!(
            r#"
version = 1

[[assets]]
id = {}
file = "component.js"
hash = "0"
content_type = "text/javascript"
"#,
            MODULE.id().as_u64()
        ))
        .unwrap();
        CxTestBuilder::new()
            .app_context(AssetConfig::hosted_at("https://example.com", manifest))
            .build()
    }

    #[tokio::test]
    async fn renders_typed_props_and_preloads_before_the_island_payload() {
        let cx = cx();
        let component = ReactComponent::<Props>::new("search", MODULE);
        let view = component
            .props(Props { label: "Products" })
            .preload(&cx, "/api/products", &[1, 2])
            .unwrap()
            .render(&cx)
            .await
            .unwrap();
        let html = view.render(&cx);
        let mut events = cx.take_response_event_receiver();

        assert!(html.contains("data-topcoat-react=\"search\""), "{html}");
        assert!(html.contains("data-topcoat-react-payload="), "{html}");
        assert!(matches!(
            events.try_next(),
            Some(ResponseEvent::Json { key, json })
                if key == "@topcoat/swr//api/products" && json == "[1,2]"
        ));
        assert!(matches!(
            events.try_next(),
            Some(ResponseEvent::Resource(_))
        ));
        assert!(matches!(
            events.try_next(),
            Some(ResponseEvent::Json { json, .. })
                if json.contains("\"label\":\"Products\"")
                    && json.contains("\"key\":\"/api/products\"")
                    && json.contains("\"jsonKey\":\"@topcoat/swr//api/products\"")
        ));
    }

    #[tokio::test]
    async fn deduplicates_equal_swr_keys_and_rejects_conflicts() {
        let cx = cx();
        let component = ReactComponent::<Props>::new("search", MODULE);
        let island = component
            .props(Props { label: "Products" })
            .preload(&cx, "/api/products", &[1, 2])
            .unwrap()
            .preload(&cx, "/api/products", &[1, 2])
            .unwrap();
        let error = component
            .props(Props { label: "Other" })
            .preload(&cx, "/api/other", &[1, 2])
            .unwrap()
            .preload(&cx, "/api/other", &[3])
            .unwrap_err();
        let response_error = component
            .props(Props { label: "Other" })
            .preload(&cx, "/api/products", &[3])
            .unwrap_err();

        island.render(&cx).await.unwrap();
        assert!(error.to_string().contains("two different values"));
        assert!(response_error.to_string().contains("two different values"));
    }

    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn server_renders_props_and_swr_fallback() {
        let cx = cx();
        let component =
            ReactComponent::<Props>::new("search", MODULE).server_renderer(SERVER_RENDERER);
        let view = component
            .props(Props { label: "Products" })
            .preload(&cx, "/api/products", &[1, 2])
            .unwrap()
            .render(&cx)
            .await
            .unwrap();
        let html = view.render(&cx);

        assert!(html.contains("data-topcoat-react-ssr"), "{html}");
        assert!(html.contains("<label>Products:1,2</label>"), "{html}");
    }

    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn server_rendered_islands_can_resolve_as_deferred_views() {
        let cx = cx();
        let component =
            ReactComponent::<Props>::new("search", MODULE).server_renderer(SERVER_RENDERER);
        let deferred = View::empty().defer(move |cx| async move {
            component
                .props(Props { label: "Deferred" })
                .preload(&cx, "/api/products", &[3, 4])?
                .render(&cx)
                .await
        });
        let mut rendered = deferred.render_response(&cx);

        assert!(rendered.html.contains("data-topcoat-defer-start"));
        assert_eq!(rendered.deferred.len(), 1);
        let completed = rendered
            .deferred
            .pop()
            .unwrap()
            .resolve(cx.handle())
            .await
            .unwrap();
        let html = completed.render(&cx);
        assert!(html.contains("data-topcoat-react-ssr"), "{html}");
        assert!(html.contains("<label>Deferred:3,4</label>"), "{html}");
    }

    #[cfg(feature = "ssr")]
    #[tokio::test]
    async fn server_render_errors_are_returned() {
        let cx = cx();
        let renderer = ReactServerRenderer::new(
            "globalThis.topcoatReactRender = () => { throw new Error('render failed') }",
        );
        let component = ReactComponent::<Props>::new("search", MODULE).server_renderer(renderer);
        let error = component
            .props(Props { label: "Products" })
            .render(&cx)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("render failed"), "{error}");
    }
}
