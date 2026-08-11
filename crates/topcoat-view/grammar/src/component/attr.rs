use syn::{
    Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// Arguments passed to the `#[component]` attribute itself.
pub struct ComponentAttr {
    boxed: bool,
    rerender: bool,
}

impl ComponentAttr {
    /// Whether the generated `render` returns a boxed future.
    #[must_use]
    pub fn boxed(&self) -> bool {
        self.boxed
    }

    /// Whether async setup is split from a reusable final render expression.
    #[must_use]
    pub fn rerender(&self) -> bool {
        self.rerender
    }
}

impl Parse for ComponentAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let args = Punctuated::<syn::Ident, Token![,]>::parse_terminated(input)?;
        let mut attr = Self {
            boxed: false,
            rerender: false,
        };
        for arg in args {
            if arg == "boxed" {
                if attr.boxed {
                    return Err(syn::Error::new(arg.span(), "duplicate `boxed` argument"));
                }
                attr.boxed = true;
            } else if arg == "rerender" {
                if attr.rerender {
                    return Err(syn::Error::new(arg.span(), "duplicate `rerender` argument"));
                }
                attr.rerender = true;
            } else {
                return Err(syn::Error::new(
                    arg.span(),
                    "expected `boxed` or `rerender`",
                ));
            }
        }
        Ok(attr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_err(source: &str) -> String {
        match syn::parse_str::<ComponentAttr>(source) {
            Ok(_) => panic!("expected parse error for `{source}`"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn parses_empty_arguments() {
        let attr: ComponentAttr = syn::parse_str("").unwrap();
        assert!(!attr.boxed());
        assert!(!attr.rerender());
    }

    #[test]
    fn parses_boxed() {
        let attr: ComponentAttr = syn::parse_str("boxed").unwrap();
        assert!(attr.boxed());
    }

    #[test]
    fn rejects_unknown_argument() {
        assert!(parse_err("pinned").contains("expected `boxed` or `rerender`"));
    }

    #[test]
    fn rejects_trailing_tokens() {
        assert!(parse_err("boxed, extra").contains("expected `boxed` or `rerender`"));
    }

    #[test]
    fn parses_rerender_with_boxed() {
        let attr: ComponentAttr = syn::parse_str("rerender, boxed").unwrap();
        assert!(attr.rerender());
        assert!(attr.boxed());
    }
}
