//! MIDI learn (MIDI map spec § 4, phase 1 § 6.3): MIX+SEQ arms on the
//! focused cell; the first control on the Part's channel maps to it, and
//! learn senses its kind from what it sends till MIX+SEQ, a nav key, a page
//! change or a turn ends it. What learn says, it says in the focus band
//! (ADR 0017), under the cell's own label. Bank 1 keeps the state and the
//! band; hearing, sensing and the words are bank 2's (`ui::midi::learn`).

use core::fmt::Write;

use chimera_hal::{ALL_ENCODERS, Controls};

use super::view::{self, View};
use super::{PrimeStatus, UiState};
use crate::MidiChannel;
use crate::midi_in::Cc;
use crate::midi_map::Mappable;
use crate::project::PartId;
use crate::ui::fmt::FmtBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Learn {
    #[default]
    Off,
    /// On Part `part`'s focused cell; `note` the last refusal, if any.
    Armed {
        part: PartId,
        note: Option<LearnNote>,
    },
    /// Part `part`'s map entry `entry` is the control heard, its kind
    /// sensed from what it sends (the samples bank 2's, in AXI).
    Sensing { part: PartId, entry: u8 },
}

/// What a learn heard or found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LearnNote {
    /// The focused cell isn't `Mappable`: a CC is ignored.
    NotMappable,
    WrongChannel {
        cc: Cc,
        got: MidiChannel,
        part: MidiChannel,
    },
    Reserved(Cc),
    /// Sensing the control heard: `TURN IT RIGHT, THEN LEFT`, then the guess.
    Sensing(Sensed),
    /// Sixteen entries, the cell's param not among them.
    Full,
}

impl LearnNote {
    /// The band's words, `label` the cell's.
    pub fn text(&self, label: &str, out: &mut FmtBuf) {
        let _ = match *self {
            Self::NotMappable => write!(out, "{label} NOT MAPPABLE"),
            Self::WrongChannel { cc, got, part } => write!(
                out,
                "CC {} ON CH {}, PART ON CH {}",
                cc.get(),
                got.get() + 1,
                part.get() + 1
            ),
            Self::Reserved(cc) => write!(out, "CC {} RESERVED", cc.get()),
            Self::Sensing(s) => {
                super::cc::learn_words(s, label, Line::Words, out);
                Ok(())
            }
            Self::Full => out.write_str("MAP FULL"),
        };
    }
}

/// What sensing has made of the control: the entry's input as guessed and
/// whether the guess is sure, packed in three bytes so the band's status
/// stays as small as learn's other notes. Bank 2 packs and reads it
/// (`Sensed::new`, `input`, in `ui::midi::learn`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sensed(pub(super) [u8; 3]);

impl Sensed {
    #[inline(always)]
    pub fn sure(self) -> bool {
        self.0[0] & 1 != 0
    }
}

/// Which of sensing's lines `learn::words` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// `CC 21 → CUTOFF`.
    Label,
    /// `TURN IT RIGHT, THEN LEFT`, or the guess in words.
    Words,
}

/// MIX + SEQ ends sensing, once the guess is sure: the hint line's.
#[cfg(feature = "midi-map")]
pub const DONE: super::hint::Hint = &[super::hint::Gesture("MIX + SEQ", "DONE")];

/// Sensing's label line (spec § 8.1), `CC 21 → CUTOFF` on the cell
/// labelled `label`, into `out`.
#[cfg(feature = "midi-map")]
#[inline(always)]
pub fn label_line(s: Sensed, label: &str, out: &mut FmtBuf) {
    super::cc::learn_words(s, label, Line::Label, out);
}

/// Armed, nothing heard yet.
pub fn armed_text(label: &str, out: &mut FmtBuf) {
    let _ = write!(out, "MOVE A KNOB · {label}");
}

impl UiState {
    pub fn learn(&self) -> Learn {
        self.learn
    }

    /// The focused cell's param, if a CC may drive it.
    pub(super) fn learn_target(&self) -> Option<Mappable> {
        view::view(self.page_def(), self.focused_slot(), &self.ctx())
            .addr()
            .and_then(Mappable::new)
    }

    /// What the focus band says in the value's place: learn's while armed
    /// or sensing.
    pub(super) fn band_status(&self) -> Option<PrimeStatus> {
        #[cfg(feature = "midi-map")]
        match self.learn {
            Learn::Armed { note, .. } => {
                let not = || self.learn_target().is_none();
                return Some(PrimeStatus::Learn(
                    note.or_else(|| not().then_some(LearnNote::NotMappable)),
                ));
            }
            Learn::Sensing { .. } => {
                return self
                    .sensed()
                    .map(|s| PrimeStatus::Learn(Some(LearnNote::Sensing(s))));
            }
            Learn::Off => {}
        }
        self.prime_status
    }

    /// The band's words as drawn: the tests' read.
    #[cfg(any(test, feature = "test-support"))]
    pub fn status_text(&self) -> Option<FmtBuf> {
        let v = view::view(self.page_def(), self.focused_slot(), &self.ctx());
        self.band_status().map(|s| s.text(v.label()))
    }

    /// Sensing's label line, the words in the value's place, and whether
    /// `MIX + SEQ  DONE` follows: the tests' read.
    #[cfg(all(feature = "midi-map", any(test, feature = "test-support")))]
    pub fn learn_text(&self) -> Option<(FmtBuf, FmtBuf, bool)> {
        let Some(PrimeStatus::Learn(Some(LearnNote::Sensing(s)))) = self.band_status() else {
            return None;
        };
        let v = view::view(self.page_def(), self.focused_slot(), &self.ctx());
        let (mut top, mut words) = (FmtBuf::new(), FmtBuf::new());
        super::cc::learn_words(s, v.label(), Line::Label, &mut top);
        super::cc::learn_words(s, v.label(), Line::Words, &mut words);
        Some((top, words, s.sure()))
    }

    /// MIX + SEQ: arms on a Part's page, never in SETTINGS; cancels when
    /// armed.
    #[cfg(feature = "midi-map")]
    #[cfg_attr(feature = "midi-map", inline(never))]
    pub(super) fn toggle_learn(&mut self) {
        let on_page = self.loc.settings().is_none() && self.loc.page(&self.cx()).is_some();
        self.learn = match self.learn {
            Learn::Off if on_page => Learn::Armed {
                part: self.active_part,
                note: None,
            },
            _ => Learn::Off,
        };
    }

    /// Armed, a turn moves the focus and never a value; sensing, a turn
    /// ends it with its guess and moves nothing. `true` while learning.
    /// Out of line: `input` is big enough.
    #[cfg_attr(feature = "midi-map", inline(never))]
    pub(super) fn learn_turns(&mut self, c: &impl Controls) -> bool {
        let part = match self.learn {
            _ if !cfg!(feature = "midi-map") => return false,
            Learn::Off => return false,
            Learn::Armed { part, .. } => part,
            Learn::Sensing { .. } => {
                if ALL_ENCODERS.iter().any(|&e| c.encoder_delta(e) != 0) {
                    self.learn = Learn::Off;
                }
                return true;
            }
        };
        let (def, ctx) = (self.page_def(), self.ctx());
        for (i, &enc) in ALL_ENCODERS.iter().enumerate() {
            if c.encoder_delta(enc) != 0 && view::view(def, i, &ctx) != View::Empty {
                self.focus.touch(def.id, i);
                self.learn = Learn::Armed { part, note: None };
            }
        }
        true
    }
}
