use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Function, Runtime};
use topcoat_core::error::{Error, Result};

const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const RENDER_TIMEOUT: Duration = Duration::from_secs(1);
const RENDER_FUNCTION: &str = "topcoatReactRender";

/// A `QuickJS` bundle that renders one React component to HTML.
#[derive(Clone, Copy, Debug)]
pub struct ReactServerRenderer {
    source: &'static str,
}

impl ReactServerRenderer {
    /// Declares a trusted bundled script that assigns a render function to
    /// `globalThis.topcoatReactRender`.
    #[must_use]
    pub const fn new(source: &'static str) -> Self {
        Self { source }
    }

    pub(crate) fn render(self, props: &str, fallback: &str) -> Result<String> {
        let runtime = Runtime::new()?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        let deadline = Instant::now() + RENDER_TIMEOUT;
        runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));
        let context = Context::full(&runtime)?;

        context.with(|context| {
            self.evaluate(&context, props, fallback)
                .map_err(|error| Error::from(std::io::Error::other(error.to_string())))
        })
    }

    fn evaluate<'js>(
        self,
        context: &rquickjs::Ctx<'js>,
        props: &str,
        fallback: &str,
    ) -> rquickjs::CaughtResult<'js, String> {
        context.eval::<(), _>(self.source).catch(context)?;
        let render = context
            .globals()
            .get::<_, Function<'js>>(RENDER_FUNCTION)
            .catch(context)?;
        let props = context.json_parse(props).catch(context)?;
        let fallback = context.json_parse(fallback).catch(context)?;
        render.call((props, fallback)).catch(context)
    }
}
