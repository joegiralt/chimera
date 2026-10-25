//! MIDI learn (MIDI map spec § 4): MIX+SEQ arms on the focused cell; the
//! first CC on the Part's channel maps to it. What learn says, it says in
//! the focus band (ADR 0017), under the cell's own label.

use core::fmt::Write;

use chimera_hal::{ALL_ENCODERS, Controls};

use super::view::{self, View};
use super::{PrimeStatus, UiState};
use crate::MidiChannel;
use crate::midi_in::Cc;
#[cfg(feature = "midi-map")]
use crate::midi_in::CcEvent;
#[cfg(feature = "midi-map")]
use crate::midi_map::{Input, Mappable};
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
}

impl Learn {
    /// Armed: on which Part, and the last refusal. Never without
    /// `midi-map`, so every path past the arm folds away.
    pub fn armed(self) -> Option<(PartId, Option<LearnNote>)> {
        match self {
            Learn::Armed { part, note } if cfg!(feature = "midi-map") => Some((part, note)),
            _ => None,
        }
    }
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
    Mapped(Cc),
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
            Self::Mapped(cc) => write!(out, "CC {} → {label}", cc.get()),
            Self::Full => out.write_str("MAP FULL"),
        };
    }
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
    #[cfg(feature = "midi-map")]
    fn learn_target(&self) -> Option<Mappable> {
        view::view(self.page_def(), self.focused_slot(), &self.ctx())
            .addr()
            .and_then(Mappable::new)
    }

    /// What the focus band says in the value's place: learn's while armed.
    pub(super) fn band_status(&self) -> Option<PrimeStatus> {
        #[cfg(feature = "midi-map")]
        if let Some((_, note)) = self.learn.armed() {
            let not = || self.learn_target().is_none();
            return Some(PrimeStatus::Learn(
                note.or_else(|| not().then_some(LearnNote::NotMappable)),
            ));
        }
        self.prime_status
    }

    /// The band's words as drawn: the tests' read.
    #[cfg(any(test, feature = "test-support"))]
    pub fn status_text(&self) -> Option<FmtBuf> {
        let v = view::view(self.page_def(), self.focused_slot(), &self.ctx());
        self.band_status().map(|s| s.text(v.label()))
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

    /// Armed, a turn moves the focus and never a value. `true` when armed.
    /// Out of line: `input` is big enough.
    #[cfg_attr(feature = "midi-map", inline(never))]
    pub(super) fn learn_turns(&mut self, c: &impl Controls) -> bool {
        let Some((part, _)) = self.learn.armed() else {
            return false;
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

    /// A CC heard while armed. On the Part's channel and free, it maps the
    /// focused cell and is consumed (`true`): its value is never applied.
    /// A refusal is said and the CC heard on as usual.
    #[cfg(feature = "midi-map")]
    pub(super) fn learn_cc(&mut self, ev: CcEvent) -> bool {
        let Some((part, _)) = self.learn.armed() else {
            return false;
        };
        let ch = self.project.part(part).mix.channel;
        let note = if ev.channel != ch {
            LearnNote::WrongChannel {
                cc: ev.cc,
                got: ev.channel,
                part: ch,
            }
        } else if ev.cc.is_reserved() {
            LearnNote::Reserved(ev.cc)
        } else if let Some((input, t)) = Input::cc(ev.cc.get()).zip(self.learn_target()) {
            let mut map = self.project.part(part).sound.map;
            let note = match map.learn(input, t) {
                Ok(()) => {
                    // A Sound edit, so `*` shows; entries may have moved.
                    self.project.edit_part(part).sound.map = map;
                    self.cc.reset_part(part);
                    LearnNote::Mapped(ev.cc)
                }
                Err(_) => LearnNote::Full,
            };
            self.learn = Learn::Off;
            self.prime_status = Some(PrimeStatus::Learn(Some(note)));
            return true;
        } else {
            // NOT MAPPABLE stands.
            return false;
        };
        self.learn = Learn::Armed {
            part,
            note: Some(note),
        };
        false
    }
}
