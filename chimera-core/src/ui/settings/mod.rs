//! The SETTINGS menu: its tree is one static table (ADR 0066).

pub mod leaves;
pub mod naming;
pub mod prompt;
pub mod tree;
pub mod view;

pub use tree::{
    Act, Issue, Kind, MANAGE_COMMANDS, PART_ROW, ROOT, Row, Screen, issue, row_at, rows,
};

use crate::project::{PartFrom, PartId, PartSource, Pending};
use prompt::Wording;

/// An open prompt and what it holds until answered; dropping it is CANCEL.
/// One variant per prompt as Tasks 11–13 wire them.
#[derive(Debug)]
#[cfg_attr(
    not(any(test, feature = "test-support")),
    expect(dead_code, reason = "opened from the Sound rung in Task 13")
)]
pub(crate) enum Ask {
    /// SAVE PART FIRST / REPLACE / CANCEL.
    ReplacePart(Pending<PartSource>),
}

impl Ask {
    pub(crate) fn wording(&self) -> Wording {
        match self {
            Ask::ReplacePart(p) => {
                let PartSource { part, from } = p.source();
                Wording::Replace {
                    part,
                    to_init: matches!(from, PartFrom::Init(_)),
                }
            }
        }
    }
}

/// Whose name NAMING edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, feature = "test-support")),
    expect(dead_code, reason = "opened from RENAME in Tasks 11–13")
)]
pub(crate) enum NamingFor {
    RenameLoaded,
    RenamePart(PartId),
}

impl NamingFor {
    pub(crate) fn title(self) -> &'static str {
        match self {
            NamingFor::RenameLoaded => "PROJECT NAME",
            NamingFor::RenamePart(_) => "SOUND NAME",
        }
    }
}
