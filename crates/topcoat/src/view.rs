#![doc = include_str!("../docs/view.md")]

pub use topcoat_view::*;
pub use topcoat_view_macro::*;

/// Streaming page rendering with deferred work and replaceable boundaries.
#[cfg(feature = "router")]
pub mod streaming {
    #![doc = include_str!("../docs/streaming.md")]

    pub use topcoat_view::{Boundary, Deferred, boundary, defer, keyed_boundary};
}
