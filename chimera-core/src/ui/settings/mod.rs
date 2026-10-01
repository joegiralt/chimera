//! The SETTINGS menu: its tree is one static table (ADR 0066).

pub mod leaves;
pub mod tree;

pub use tree::{Act, Kind, ROOT, Row, Screen, Status, row_at, rows};
