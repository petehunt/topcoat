use topcoat_core::context::Cx;

use crate::{
    NodeViewParts, PartsWriter, View,
    identity::{Identity, IdentityKey, SiteKey},
};

/// A region that can be replaced independently by a streaming render pass.
#[derive(Debug, Clone)]
pub struct Boundary {
    identity: Identity,
    child: View,
}

impl Boundary {
    /// Wraps `child` in stable marker comments used by the reconciliation
    /// protocol.
    #[must_use]
    pub fn new(identity: Identity, child: View) -> Self {
        Self { identity, child }
    }
}

impl NodeViewParts for Boundary {
    fn into_view_parts(self, _cx: &Cx, parts: &mut PartsWriter<'_>) {
        let id = format!("{:032x}", self.identity.hash());
        parts.push_string_unescaped(format!("<!--topcoat-boundary {id}-->"));
        parts.push_view(self.child);
        parts.push_string_unescaped(format!("<!--/topcoat-boundary {id}-->"));
    }
}

/// Marks a view as an independently replaceable streaming region.
///
/// # Panics
///
/// Panics if the enclosing component identity is ambiguous.
#[must_use]
#[track_caller]
pub fn boundary(child: View) -> Boundary {
    let location = std::panic::Location::caller();
    let site = SiteKey::new(location.file(), location.line(), location.column(), 0);
    Boundary::new(Identity::current().child(site), child)
}

/// Marks one repeated view as an independently replaceable streaming region.
///
/// # Panics
///
/// Panics if the enclosing component identity is ambiguous.
#[must_use]
#[track_caller]
pub fn keyed_boundary(key: impl IdentityKey, child: View) -> Boundary {
    let location = std::panic::Location::caller();
    let site = SiteKey::new(location.file(), location.line(), location.column(), 0);
    Boundary::new(Identity::current().keyed_child(site, key), child)
}
