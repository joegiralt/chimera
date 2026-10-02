//! ORBIT's own cells: what a page edits that is no block's param.

use super::OrbitUi;
use crate::block::ValFmt;
use crate::orbit::chords::ChordRing;
use crate::orbit::{RINGS, RingIdx};
use chimera_theory::{Degree, Interval};

/// A cell bound to ORBIT's UI state rather than a param (`SlotBinding::Orbit`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitCell {
    /// ORB: the selected ring, one to five.
    Ring,
    /// CHRD: the selected segment's degree; with MIX, which segment.
    Degree,
    /// CUSTOM: the note under the cursor, round the 12.
    Cursor,
    /// CUSTOM: the cursor's note in the scale or out of it.
    Set,
}

const OFF_ON: [&str; 2] = ["OFF", "ON"];

impl OrbitCell {
    pub const fn label(self) -> &'static str {
        match self {
            OrbitCell::Ring => "RING",
            OrbitCell::Degree => "DEGREE",
            OrbitCell::Cursor => "NOTE",
            OrbitCell::Set => "IN SCALE",
        }
    }

    /// Discrete, so a cell draws no bar; its text is its own (`draw::cell_text`).
    pub const fn fmt(self) -> ValFmt {
        match self {
            OrbitCell::Ring => ValFmt::OneBased(RINGS as u8 - 1),
            OrbitCell::Degree | OrbitCell::Cursor => ValFmt::Int(11),
            OrbitCell::Set => ValFmt::Names(&OFF_ON),
        }
    }

    /// The cell's value, 0..1, on ring `ring`: what it eases toward.
    pub fn value(self, c: &ChordRing, ui: &OrbitUi, ring: RingIdx) -> f32 {
        match self {
            OrbitCell::Ring => ring.get() as f32 / (RINGS - 1) as f32,
            OrbitCell::Degree => {
                let last = c.key().degree_count().saturating_sub(1).max(1);
                c.degrees[ui.seg_of(c)].index().min(last) as f32 / last as f32
            }
            OrbitCell::Cursor => ui.cursor as f32 / 11.0,
            OrbitCell::Set => f32::from(c.custom.contains(ui.interval())),
        }
    }

    /// One turn by `delta`, `mix` held or not, on ring `ring`: RING moves
    /// it, and the caller goes there.
    pub fn turn(
        self,
        delta: i8,
        mix: bool,
        c: &mut ChordRing,
        ui: &mut OrbitUi,
        ring: &mut RingIdx,
    ) {
        let by =
            |at: usize, len: usize| (at as i32 + delta as i32).clamp(0, len as i32 - 1) as usize;
        match self {
            OrbitCell::Ring => *ring = RingIdx::ALL[by(ring.get() as usize, RINGS)],
            // MIX picks the segment DEGREE edits.
            OrbitCell::Degree if mix => ui.seg = by(ui.seg_of(c), c.segs.get() as usize) as u8,
            OrbitCell::Degree => {
                let seg = ui.seg_of(c);
                let n = c.key().degree_count();
                // A degree past a smaller set turns from the set's last.
                c.degrees[seg] = Degree::ALL[by(c.degrees[seg].index().min(n - 1), n)];
            }
            OrbitCell::Cursor => ui.cursor = (ui.cursor as i32 + delta as i32).rem_euclid(12) as u8,
            // Right sets, left clears, however far; `toggle` keeps the root.
            OrbitCell::Set => {
                let i = ui.interval();
                if (delta > 0) != c.custom.contains(i) {
                    c.custom = c.custom.toggle(i);
                }
            }
        }
    }
}

impl OrbitUi {
    /// CHRD's segment, within the progression's.
    pub fn seg_of(&self, c: &ChordRing) -> usize {
        (self.seg as usize).min(c.segs.get() as usize - 1)
    }

    /// The note under CUSTOM's cursor, above the tonic.
    pub fn interval(&self) -> Interval {
        Interval::ALL[self.cursor as usize % 12]
    }
}
