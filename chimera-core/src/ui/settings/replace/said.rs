//! The prompt's yes, as a value. No crate imports: `project::guard` takes
//! it, and only `replace` makes one (ADR 0066).

use core::marker::PhantomData;

/// A SEQ tap on a prompt's confirming pill, for a `T` target only.
pub struct Said<T>(PhantomData<T>);

impl<T> Said<T> {
    pub(super) fn new() -> Self {
        Said(PhantomData)
    }
}
