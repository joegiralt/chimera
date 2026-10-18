//! A WAVE or ALG A/B choice the formant budget refuses (ADR 0151): the
//! turn lands on it, the cell shows it dimmed with the reason in the focus
//! band, and nothing is written.

use crate::addr::{BlockRef, ParamAddr};
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::kinds::{Linked, OpKind, Why, why_kind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::preset::Sound;
use crate::project::Line;

/// The choice a turn stopped on: `addr`'s enum code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    pub addr: ParamAddr,
    pub code: u8,
}

/// A held choice and why it is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub held: Held,
    pub why: Why,
}

/// Whether `a` is a cell this applies to.
pub fn applies(a: ParamAddr, shift: bool) -> bool {
    match a.block {
        BlockRef::AlgoOp(_) => a.param == AlgoOpParams::WAVE,
        BlockRef::Algo => !shift && matches!(a.param, AlgoParams::ALG_A | AlgoParams::ALG_B),
        _ => false,
    }
}

impl Held {
    /// Why `sound` refuses it, or `None` where it would land.
    pub fn why(self, sound: &Sound) -> Option<Why> {
        let p = &sound.params.algo;
        match self.addr.block {
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
        }
    }
}

/// The focus band's reason: 216 px at the house tracking.
pub fn reason(w: Why) -> Line {
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
