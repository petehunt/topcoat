pub use crate::global_memoize::{GlobalMemoizeCache, global_memoize};

pub trait ResultExt {
    type T;
    type E;
}

impl<T, E> ResultExt for Result<T, E> {
    type T = T;
    type E = E;
}
