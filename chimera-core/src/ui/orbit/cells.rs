//! ORBIT's own cells: what a page edits that is no block's param.

use super::OrbitUi;
use crate::block::{OFF_ON, ValFmt};
use crate::orbit::channel::ArmTarget;
use crate::orbit::chords::ChordRing;
use crate::orbit::runner::Capture;
use crate::orbit::scene::{OrbitSong, SCENES, SceneIdx, SceneRing};
use crate::orbit::{RINGS, RingIdx};
use crate::ui::nav::{step_clamped, step_wrapped};
use chimera_theory::{Degree, Interval, Note};

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
    /// NOTES: the selected chip's note, a semitone a detent; with MIX,
    /// which chip.
    Note,
    /// NOTES: up arms a capture into the ring from the keys, down
    /// disarms it.
    Learn,
    /// NOTES, with MIX held: up leaves one note at OCTAVE's C, down rolls
    /// random notes of the key in the register. A plain turn does nothing:
    /// either way loses notes set by hand.
    Clear,
    /// CHRD: up arms a capture of the next chord held into the segment;
    /// down disarms it, and with MIX drops the played chord for the degree.
    Play,
    /// SCN: the scene playing, or queued; a turn queues another.
    Scene,
    /// SCN: the slot EDIT saves `live` to, the playing one at first.
    Save,
    /// SCN: when a switch lands, the loop's end. Read-only.
    Switch,
}

const LOOP: [&str; 1] = ["LOOP"];

impl OrbitCell {
    pub const fn label(self) -> &'static str {
        match self {
            OrbitCell::Ring => "RING",
            OrbitCell::Degree => "DEGREE",
            OrbitCell::Cursor | OrbitCell::Note => "NOTE",
            OrbitCell::Set => "IN SCALE",
            OrbitCell::Learn => "LEARN",
            OrbitCell::Clear => "CLEAR",
            OrbitCell::Play => "PLAY",
            OrbitCell::Scene => "SCENE",
            OrbitCell::Save => "SAVE",
            OrbitCell::Switch => "SWITCH",
        }
    }

    /// Discrete, so a cell draws no bar; its text is its own (`draw::cell_text`).
    pub const fn fmt(self) -> ValFmt {
        match self {
            OrbitCell::Ring => ValFmt::OneBased(RINGS as u8 - 1),
            OrbitCell::Degree | OrbitCell::Cursor => ValFmt::Int(11),
            OrbitCell::Note => ValFmt::Int(127),
            OrbitCell::Scene | OrbitCell::Save => ValFmt::OneBased(SCENES as u8 - 1),
            OrbitCell::Switch => ValFmt::Names(&LOOP),
            OrbitCell::Set | OrbitCell::Learn | OrbitCell::Clear | OrbitCell::Play => {
                ValFmt::Names(&OFF_ON)
            }
        }
    }

    /// The cell's value, 0..1, on ring `ring`: what it eases toward.
    pub fn value(self, song: &OrbitSong, ui: &OrbitUi, ring: RingIdx) -> f32 {
        let s = &song.live;
        let c = &s.chords;
        let slot = |n: SceneIdx| n.get() as f32 / (SCENES - 1) as f32;
        match self {
            OrbitCell::Scene => slot(song.queued().unwrap_or(song.current())),
            OrbitCell::Save => slot(ui.save_slot(song)),
            OrbitCell::Switch => 0.0,
            OrbitCell::Ring => ring.get() as f32 / (RINGS - 1) as f32,
            OrbitCell::Degree => {
                let last = c.key().degree_count().saturating_sub(1).max(1);
                c.degrees[ui.seg_of(c)].index().min(last) as f32 / last as f32
            }
            OrbitCell::Cursor => ui.cursor as f32 / 11.0,
            OrbitCell::Set => f32::from(c.custom.contains(ui.interval())),
            OrbitCell::Note => {
                let r = &s.rings[ring.get() as usize];
                r.chip(ui.chip_of(r)).get() as f32 / 127.0
            }
            OrbitCell::Learn => f32::from(ui.capture == Capture::Ring(ring)),
            OrbitCell::Clear => 0.0,
            // Armed, or which segment's chord: a cell to redraw either way.
            OrbitCell::Play => {
                let seg = ui.seg_of(c);
                match (ui.capture == Capture::Segment(seg as u8), c.played[seg]) {
                    (true, _) => 1.0,
                    (false, Some(_)) => 0.25 + seg as f32 / 32.0,
                    (false, None) => 0.0,
                }
            }
        }
    }

    /// One turn by `delta`, `mix` held or not, on ring `ring`: RING moves
    /// it, and the caller goes there.
    pub fn turn(
        self,
        delta: i8,
        mix: bool,
        song: &mut OrbitSong,
        ui: &mut OrbitUi,
        ring: &mut RingIdx,
    ) {
        let by = |at: usize, len: usize| step_clamped(at, delta as i32, len);
        match self {
            // Past the ends it stays: no switch to what already plays or waits.
            OrbitCell::Scene => {
                let at = song.queued().unwrap_or(song.current());
                let to = SceneIdx::ALL[by(at.get() as usize, SCENES)];
                if to != at {
                    ui.queue(to, song);
                }
                return;
            }
            OrbitCell::Save => {
                let to = by(ui.save_slot(song).get() as usize, SCENES);
                ui.save = Some(SceneIdx::ALL[to]);
                return;
            }
            OrbitCell::Switch => return,
            _ => {}
        }
        // What the turn reads, from `live`; what it changes, through
        // `edit`, into the slot too (ADR 0083).
        let s = &song.live;
        let c = &s.chords;
        let ri = ring.get() as usize;
        let r = &s.rings[ri];
        match self {
            OrbitCell::Ring => *ring = RingIdx::ALL[by(ri, RINGS)],
            // MIX picks the segment DEGREE edits.
            OrbitCell::Degree if mix => ui.seg = by(ui.seg_of(c), c.segs.get() as usize) as u8,
            OrbitCell::Degree => {
                let seg = ui.seg_of(c);
                let n = c.key().degree_count();
                // A degree past a smaller set turns from the set's last.
                let to = Degree::ALL[by(c.degrees[seg].index().min(n - 1), n)];
                song.edit(&mut |s| s.chords.degrees[seg] = to);
            }
            OrbitCell::Cursor => ui.cursor = step_wrapped(ui.cursor as i32, delta as i32, 12) as u8,
            // Right sets, left clears, however far; `toggle` keeps the root.
            OrbitCell::Set => {
                let i = ui.interval();
                if (delta > 0) != c.custom.contains(i) {
                    let to = c.custom.toggle(i);
                    song.edit(&mut |s| s.chords.custom = to);
                }
            }
            // MIX picks the chip NOTE edits.
            OrbitCell::Note if mix => ui.chip = by(ui.chip_of(r), r.chips()) as u8,
            // The chip follows its note as the pool re-sorts.
            OrbitCell::Note => {
                let i = ui.chip_of(r);
                let to = (r.chip(i).get() as i32 + delta as i32).clamp(0, 127) as u8;
                if let Some(to) = Note::new(to) {
                    let mut at = i;
                    song.edit(&mut |s| at = s.rings[ri].set_chip(i, to));
                    ui.chip = at as u8;
                }
            }
            OrbitCell::Learn => arm(ui, ArmTarget::Ring(*ring), delta),
            // Down disarms; with MIX it drops the played chord too, for the
            // degree: a flick past disarming loses no chord.
            OrbitCell::Play => {
                let seg = ui.seg_of(c);
                if delta < 0 && mix {
                    song.edit(&mut |s| s.chords.played[seg] = None);
                }
                arm(ui, ArmTarget::Segment(seg as u8), delta);
            }
            // MIX held, or nothing: a slip on its way to NOTE loses no notes.
            OrbitCell::Clear if !mix => {}
            OrbitCell::Clear if delta > 0 => song.edit(&mut |s| s.rings[ri].clear()),
            OrbitCell::Clear => {
                let salt = ui.clear_salt();
                song.edit(&mut |s| s.scatter(*ring, salt));
            }
            OrbitCell::Scene | OrbitCell::Save | OrbitCell::Switch => {}
        }
    }
}

/// Up arms `to` (dropping any other), down disarms it.
fn arm(ui: &mut OrbitUi, to: ArmTarget, delta: i8) {
    if delta > 0 {
        ui.arm(Some(to));
    } else if ui.capture == to.into() {
        ui.disarm();
    }
}

impl OrbitUi {
    /// CHRD's segment, within the progression's.
    pub fn seg_of(&self, c: &ChordRing) -> usize {
        (self.seg as usize).min(c.segs.get() as usize - 1)
    }

    /// NOTES' chip, within the ring's.
    pub fn chip_of(&self, r: &SceneRing) -> usize {
        (self.chip as usize).min(r.chips() - 1)
    }

    /// The note under CUSTOM's cursor, above the tonic.
    pub fn interval(&self) -> Interval {
        Interval::ALL[self.cursor as usize % 12]
    }
}
