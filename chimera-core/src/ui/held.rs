//! A WAVE or ALG A/B choice the formant budget refuses (ADR 0151), a
//! TAPE machine or WALK DIRECTION a streamed source can't play, or a pad's
//! CHOKE that makes a 9th voice group (ADR 0280): the turn lands on it, the
//! cell shows it dimmed with the reason in the focus band, and nothing is
//! written.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::kinds::{Linked, OpKind, Why, why_kind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::drum::groups::VoiceGroups;
use crate::dsp::drum::params::PadParams;
use crate::dsp::drum::types::ChokeGroup;
use crate::dsp::tape_engine::{NeedsRam, TapeEngineParams};
use crate::preset::Sound;
use crate::project::Line;

/// The choice a turn stopped on, on cell `addr`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub addr: ParamAddr,
    pub choice: Choice,
}

/// What a held turn points at, typed by its cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// An operator's WAVE.
    Kind(OpKind),
    /// ALG A or B.
    Alg(AlgoId),
    /// TAPE's MODEL or DIRECTION.
    Tape(TapeCode),
    /// A pad's CHOKE, held on the project at revision `rev`: a kit changed
    /// since (a load, a CC) drops it (ADR 0280).
    Choke { to: ChokeGroup, rev: u32 },
}

impl Choice {
    /// The cell's enum code: what its text shows.
    pub fn code(self) -> u8 {
        match self {
            Choice::Kind(k) => k.code(),
            Choice::Alg(a) => a.get(),
            Choice::Tape(c) => c.get(),
            Choice::Choke { to, .. } => to.index() as u8,
        }
    }
}

impl Held {
    /// `choice`'s enum code.
    pub fn code(self) -> u8 {
        self.choice.code()
    }
}

/// A TAPE MODEL or DIRECTION code: each lists four, 0..=3 by
/// construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TapeCode(u8);

const _: () = assert!(
    crate::dsp::tape_engine::TapeMachine::ALL.len() == TapeCode::LEN
        && crate::dsp::tape_engine::Direction::ALL.len() == TapeCode::LEN
);

impl TapeCode {
    const LEN: usize = 4;

    /// `c`, clamped into the list.
    pub const fn clamped(c: u8) -> TapeCode {
        TapeCode(if c < Self::LEN as u8 {
            c
        } else {
            Self::LEN as u8 - 1
        })
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Turned `delta` along the list, saturating; with `shift`, to its end
    /// that way.
    pub fn step(self, delta: i8, shift: bool) -> TapeCode {
        let last = Self::LEN as u8 - 1;
        TapeCode(match (shift, delta > 0) {
            (true, true) => last,
            (true, false) => 0,
            _ => self.0.saturating_add_signed(delta).min(last),
        })
    }
}

/// Why a choice is refused: the formant budget, a streamed source, or
/// the kit's voice groups (the count the choke would make).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Formant(Why),
    NeedsRam(NeedsRam),
    Groups(VoiceGroups),
}

impl From<Why> for Reason {
    fn from(w: Why) -> Reason {
        Reason::Formant(w)
    }
}

/// A held choice and why it is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub held: Held,
    pub why: Reason,
}

/// TAPE's MODEL: on a streamed source a turn holds a machine it refuses.
pub const MODEL: ParamAddr = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL);
/// WALK's DIRECTION: likewise REVERSE and PING-PONG (ADR 0203).
pub const DIRECTION: ParamAddr = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODE);

/// A pad's CHOKE: a turn holds a 9th voice group (ADR 0280).
pub const CHOKE: ParamAddr = ParamAddr::new(BlockRef::Pad, PadParams::CHOKE);

/// Whether `a` is a cell this applies to.
pub fn applies(a: ParamAddr, shift: bool) -> bool {
    match a.block {
        BlockRef::AlgoOp(_) => a.param == AlgoOpParams::WAVE,
        BlockRef::Algo => !shift && matches!(a.param, AlgoParams::ALG_A | AlgoParams::ALG_B),
        _ => false,
    }
}

impl Held {
    /// Why `sound` refuses it, or `None` where it would land. `streamed`:
    /// the Sound's source streams. A CHOKE is the kit's (`kit_groups`).
    pub fn why(self, sound: &Sound, streamed: bool) -> Option<Reason> {
        let p = &sound.params.algo;
        let why = match (self.addr.block, self.choice) {
            (_, Choice::Tape(c)) => {
                if !streamed {
                    return None;
                }
                // Bank 2's (`ui::walk`): closed, nothing is held.
                let t = &sound.params.tape;
                return crate::bank2::ui_door(|u| u.tape_held(t, self.addr.param, c.get()))
                    .flatten()
                    .map(Reason::NeedsRam);
            }
            (BlockRef::AlgoOp(op), Choice::Kind(k)) => {
                why_kind(p.kinds(), p.linked(), op.index(), k)
            }
            (BlockRef::Algo, Choice::Alg(to)) => {
                let (a, b) = p.algs();
                let l = if self.addr.param == AlgoParams::ALG_A {
                    Linked::of(to, b)
                } else {
                    Linked::of(a, to)
                };
                (!p.kinds().fit(l)).then_some(Why::Links)
            }
            _ => None,
        };
        why.map(Reason::Formant)
    }
}

/// The focus band's reason: 216 px at the house tracking.
pub fn reason(w: impl Into<Reason>) -> Line {
    let w = match w.into() {
        Reason::Formant(w) => w,
        Reason::NeedsRam(n) => {
            let mut l = Line::new(crate::ui::source_cell::needs_ram_words(n));
            let _ = core::fmt::Write::write_str(&mut l, " · STREAMED");
            return l;
        }
        Reason::Groups(n) => {
            return match crate::bank2::groups(super::kit_groups::Ask::Why(n)) {
                Some(super::kit_groups::Said::Line(l)) => l,
                _ => Line::new(""),
            };
        }
    };
    match w {
        Why::Modulated(op) => {
            let mut l = Line::new("OP ");
            let _ = core::fmt::Write::write_fmt(
                &mut l,
                format_args!("{} MODULATED · OVER BUDGET", op + 1),
            );
            l
        }
        Why::Full => Line::new("BUDGET FULL · OTHER FORMANTS"),
        Why::Links => Line::new("LINKS A FORMANT · OVER BUDGET"),
    }
}
