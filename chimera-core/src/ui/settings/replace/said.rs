//! The prompt's yes, as a value. No imports: `project::guard` takes it, and
//! only `replace` makes one (ADR 0066).

/// A SEQ tap on a prompt's confirming pill.
pub struct Said(());

impl Said {
    pub(super) fn new() -> Self {
        Said(())
    }
}
