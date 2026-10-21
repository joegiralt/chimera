//! A WAVE or ALG A/B choice the formant budget refuses (ADR 0151), or a
//! TAPE machine or WALK DIRECTION a streamed source can't play: the turn lands on it, the
//! cell shows it dimmed with the reason in the focus band, and nothing is
//! written.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::kinds::{Linked, OpKind, Why, why_kind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::tape_engine::{NeedsRam, TapeEngineParams, needs_ram};
use crate::preset::Sound;
use crate::project::Line;

/// The choice a turn stopped on: `addr`'s enum code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub addr: ParamAddr,
    pub code: u8,
}

/// Why a choice is refused: the formant budget, or a streamed source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Formant(Why),
    NeedsRam(NeedsRam),
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
/// WALK's DIRECTION: likewise REVERSE and PING-PONG (ADR 0202).
pub const DIRECTION: ParamAddr = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODE);

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
    /// the Sound's source streams.
    pub fn why(self, sound: &Sound, streamed: bool) -> Option<Reason> {
        if self.addr == MODEL || self.addr == DIRECTION {
            let mut t = sound.params.tape;
            crate::block::Block::set(&mut t, self.addr.param, f32::from(self.code));
            return needs_ram(&t).filter(|_| streamed).map(Reason::NeedsRam);
        }
        let p = &sound.params.algo;
        let why = match self.addr.block {
            BlockRef::AlgoOp(op) if self.addr.param == AlgoOpParams::WAVE => why_kind(
                p.kinds(),
                p.linked(),
                op.index(),
                OpKind::clamped(self.code),
            ),
            BlockRef::Algo => {
                let (a, b) = p.algs();
                let to = AlgoId::clamped(self.code);
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
