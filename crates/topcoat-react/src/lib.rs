#![cfg_attr(docsrs, feature(doc_cfg))]
//! Typed React islands for Topcoat.

use core::marker::PhantomData;

use serde::Serialize;
use topcoat_asset::{Asset, CxAssetExt};
use topcoat_core::{
    context::{Cx, JsonKey},
    error::Result,
};
use topcoat_view::View;
use topcoat_view_macro::view;

const SWR_JSON_PREFIX: &str = "@topcoat/swr/";

/// A client React component with one Rust props type and one bundled module.
#[derive(Debug)]
pub struct ReactComponent<Props> {
    name: &'static str,
    module: Asset,
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
            props: PhantomData,
        }
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
        let json_key = cx.send_json_internal(format!("{SWR_JSON_PREFIX}{key}"), value)?;
        if !self.preloads.iter().any(|preload| preload.key == key) {
            self.preloads.push(SWRPreload { key, json_key });
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
    /// cannot be serialized.
    pub async fn render(self, cx: &Cx) -> Result<View> {
        cx.require_asset(self.component.module.module())?;
        let payload = IslandPayload {
            props: &self.props,
            preloads: self.preloads.iter().map(SWRPreload::payload).collect(),
        };
        let payload_key = cx.send_json(&payload)?;

        view! {
            cx =>
            <div
                data-topcoat-react=(self.component.name)
                data-topcoat-react-payload=(payload_key.as_str())
            ></div>
        }
    }
}

#[derive(Debug)]
struct SWRPreload {
    key: String,
    json_key: JsonKey,
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
}
