use core::ops::AsyncFn;

use topcoat_core::{context::Cx, error::Error};

use crate::{Props, View};

pub trait Component: Sized + Send {
    type Props: Props + Send;

    #[must_use]
    fn props_builder() -> <Self::Props as Props>::Builder {
        Self::Props::builder()
    }

    /// Runs the component's async setup and returns its reusable render body.
    fn prepare<'cx>(
        self,
        cx: &'cx Cx,
        props: Self::Props,
    ) -> impl Future<Output = Result<impl AsyncFn() -> Result<View, Error> + Send + 'cx, Error>> + Send
    where
        Self: 'cx,
        Self::Props: 'cx;

    /// Prepares the component and renders it once.
    fn render<'cx>(
        self,
        cx: &'cx Cx,
        props: Self::Props,
    ) -> impl Future<Output = Result<View, Error>> + Send
    where
        Self: 'cx,
        Self::Props: 'cx;
}
