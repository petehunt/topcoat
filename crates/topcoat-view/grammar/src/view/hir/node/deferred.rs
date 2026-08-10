use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use topcoat_core_grammar::paths::{topcoat_error, topcoat_view};

use super::{Component, Emit, Emitter};
use crate::view::hir::Scope;

/// A deferred component invocation and the placeholder shown until it resolves.
pub(crate) struct Deferred {
    pub component: Component,
    pub placeholder: Scope,
    pub span: Span,
}

impl Deferred {
    fn emit_future(&self) -> TokenStream {
        let component = self.component.emit_render_future();
        let placeholder = self.placeholder.emit_future();
        let span = self.span;

        quote_spanned! {span=>
            async {
                let __placeholder = (#placeholder).await?;
                ::core::result::Result::<
                    #topcoat_view::View,
                    #topcoat_error::Error,
                >::Ok(#topcoat_view::defer(__placeholder, move |__cx| async move {
                    let __cx = &__cx;
                    (#component).await
                }))
            }
        }
    }
}

impl Emit for Deferred {
    fn emit(&self, emitter: &mut Emitter) {
        let ident = emitter.fresh_ident();
        let span = self.span;
        let future = self.emit_future();
        emitter.hoist_future(span, &ident, &future);
        emitter.burst(quote_spanned! {span=> __b.view(#ident); });
    }
}
