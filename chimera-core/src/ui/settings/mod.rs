//! The SETTINGS menu: its tree is one static table (ADR 0066).

pub mod leaves;
pub mod tree;
pub mod view;

pub use tree::{Act, Issue, Kind, MANAGE_COMMANDS, ROOT, Row, Screen, issue, row_at, rows};
