//! What an operator is: one of the wave tables, a formant or a noise band.
//! WAVE's stored code is the kind's code (ADR 0045).

use crate::budget::FORMANT_BUDGET;
use crate::dsp::algo::engine::AlgoEngine;
use crate::dsp::algo::plan::OPS;
use crate::dsp::algo::waves::{WAVE_COUNT, WAVE_IDENTS, WAVE_NAMES, WaveId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Table(WaveId),
    Formant,
    Noise,
}

pub const OP_KIND_COUNT: usize = WAVE_COUNT + 2;
const _: () = assert!(OP_KIND_COUNT == 18);

const fn with_tail(w: &[&'static str; WAVE_COUNT]) -> [&'static str; OP_KIND_COUNT] {
    let mut a = ["FRMT"; OP_KIND_COUNT];
    let mut i = 0;
    while i < WAVE_COUNT {
        a[i] = w[i];
        i += 1;
    }
    a[WAVE_COUNT + 1] = "NOIS";
    a
}

pub static OP_KIND_NAMES: [&str; OP_KIND_COUNT] = with_tail(WAVE_NAMES);
pub static OP_KIND_IDENTS: [&str; OP_KIND_COUNT] = with_tail(&WAVE_IDENTS);

impl OpKind {
    pub const fn from_code(c: u8) -> Option<OpKind> {
        match c as usize {
            16 => Some(OpKind::Formant),
            17 => Some(OpKind::Noise),
            _ => match WaveId::from_index(c) {
                Some(w) => Some(OpKind::Table(w)),
                None => None,
            },
        }
    }

    pub const fn clamped(c: u8) -> OpKind {
        let c = if c > 17 { 17 } else { c };
        match Self::from_code(c) {
            Some(k) => k,
            None => OpKind::Noise,
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            OpKind::Table(w) => w.get(),
            OpKind::Formant => 16,
            OpKind::Noise => 17,
        }
    }

    pub const fn is_formant(self) -> bool {
        matches!(self, OpKind::Formant)
    }

    /// What the kind draws from the formant budget: its cost over a table's.
    pub const fn charge(self) -> u32 {
        match self {
            OpKind::Table(_) => 0,
            OpKind::Formant => AlgoEngine::COST_FRMT.0 - AlgoEngine::COST_OP.0,
            OpKind::Noise => AlgoEngine::COST_NOIS.0 - AlgoEngine::COST_OP.0,
        }
    }
}

pub const fn spend(k: &[OpKind; OPS]) -> u32 {
    let (mut s, mut i) = (0, 0);
    while i < OPS {
        s += k[i].charge();
        i += 1;
    }
    s
}

pub const fn fits(k: &[OpKind; OPS]) -> bool {
    spend(k) <= FORMANT_BUDGET.0
}
