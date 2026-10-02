//! ORBIT's own cells: what a page edits that is no block's param.

/// A cell bound to ORBIT's UI state rather than a param (`SlotBinding::Orbit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitCell {
    /// CHRD: the selected segment's degree.
    Degree,
    /// CUSTOM: the note under the cursor, round the 12.
    Cursor,
    /// CUSTOM: the cursor's note in the scale or out of it.
    Set,
}
