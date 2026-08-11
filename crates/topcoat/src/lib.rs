#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc = include_str!("../README.md")]

extern crate self as topcoat;

/// Caches one asynchronous load across requests, keyed by an expression.
///
/// Each invocation has an independent process-wide cache. Completed values
/// are reused and concurrent misses for the same key share one in-flight
/// future. The cache has no expiration or eviction.
#[macro_export]
macro_rules! memoize_global {
    ($key:expr, $load:expr $(,)?) => {{
        static CACHE: $crate::internal::GlobalMemoizeCache =
            $crate::internal::GlobalMemoizeCache::new();
        $crate::internal::global_memoize(&CACHE, $key, $load)
    }};
}

#[cfg(feature = "router")]
pub mod dev;

#[cfg(feature = "serve")]
mod serve;

pub use topcoat_core::error::Error;

#[cfg(feature = "view")]
pub type Result<T = view::View, E = topcoat_core::error::Error> = topcoat_core::error::Result<T, E>;
#[cfg(not(feature = "view"))]
pub type Result<T, E = topcoat_core::error::Error> = topcoat_core::error::Result<T, E>;

#[cfg(feature = "alpine-ajax")]
pub mod alpine_ajax;

#[cfg(feature = "asset")]
pub mod asset;

#[cfg(feature = "cookie")]
pub mod cookie;

pub mod context;

#[cfg(feature = "datastar")]
pub mod datastar;

#[cfg(feature = "font")]
pub mod font;

#[cfg(feature = "htmx")]
pub mod htmx;

#[cfg(feature = "icon")]
pub mod icon;

#[cfg(feature = "mail")]
pub mod mail;

#[cfg(feature = "router")]
pub mod router;

#[cfg(feature = "view")]
pub mod view;

#[cfg(feature = "serve")]
pub use serve::{serve, serve_until, start};

#[cfg(feature = "runtime")]
pub mod runtime;

#[cfg(feature = "session")]
pub mod session;

#[cfg(feature = "tailwind")]
pub mod tailwind;

#[doc(hidden)]
pub mod internal;
