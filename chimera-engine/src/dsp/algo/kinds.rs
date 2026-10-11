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

const fn with_tail(
    w: &[&'static str; WAVE_COUNT],
    noise: &'static str,
) -> [&'static str; OP_KIND_COUNT] {
    let mut a = ["FRMT"; OP_KIND_COUNT];
    let mut i = 0;
    while i < WAVE_COUNT {
        a[i] = w[i];
        i += 1;
    }
    a[WAVE_COUNT + 1] = noise;
    a
}

/// FORMANT is wider than a cell's 62 px, so code 16 reads FRMT; NOISE fits.
pub static OP_KIND_NAMES: [&str; OP_KIND_COUNT] = with_tail(WAVE_NAMES, "NOISE");
pub static OP_KIND_IDENTS: [&str; OP_KIND_COUNT] = with_tail(&WAVE_IDENTS, "NOIS");

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

    /// A formant or noise band: not a table (spec § 1).
    pub const fn is_formant(self) -> bool {
        !matches!(self, OpKind::Table(_))
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

/// The six operators' kinds, within the formant budget by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kinds([OpKind; OPS]);

/// A kind refused: it would overspend the formant budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverBudget;

impl Kinds {
    pub const TABLE: Kinds = Kinds([OpKind::Table(WaveId::W1); OPS]);

    /// Panics unless `k` fits, so a `const` is checked at compile time:
    ///
    /// ```compile_fail
    /// use chimera_engine::dsp::algo::kinds::{Kinds, OpKind};
    /// const K: Kinds = Kinds::new([OpKind::Formant; 6]);
    /// let _ = K;
    /// ```
    pub const fn new(k: [OpKind; OPS]) -> Kinds {
        assert!(fits(&k), "over the formant budget");
        Kinds(k)
    }

    /// Table kinds charge nothing, so they always fit.
    pub const fn tables(w: [WaveId; OPS]) -> Kinds {
        let (mut k, mut i) = ([OpKind::Noise; OPS], 0);
        while i < OPS {
            k[i] = OpKind::Table(w[i]);
            i += 1;
        }
        Kinds(k)
    }

    pub const fn get(&self, op: usize) -> OpKind {
        self.0[op]
    }

    pub const fn all(&self) -> [OpKind; OPS] {
        self.0
    }

    pub fn with(self, op: usize, kind: OpKind) -> Result<Kinds, OverBudget> {
        let mut k = self.0;
        k[op] = kind;
        if fits(&k) {
            Ok(Kinds(k))
        } else {
            Err(OverBudget)
        }
    }

    pub fn any_formant(&self) -> bool {
        self.0.iter().any(|k| k.is_formant())
    }
}

/// Where an encoder turn of WAVE lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindStep {
    pub to: OpKind,
    /// A refused kind was met on the way: the cell toasts.
    pub skipped: bool,
}

/// `|delta|` allowed kinds on from operator `op`'s, in `delta`'s direction,
/// stopping at codes 0 and 17.
pub fn step_kind(k: Kinds, op: usize, delta: i8) -> KindStep {
    let mut s = KindStep {
        to: k.get(op),
        skipped: false,
    };
    let (mut code, mut left) = (i16::from(s.to.code()), delta.unsigned_abs());
    while left > 0 {
        code += i16::from(delta.signum());
        let Some(next) = u8::try_from(code).ok().and_then(OpKind::from_code) else {
            break;
        };
        if k.with(op, next).is_ok() {
            (s.to, left) = (next, left - 1);
        } else {
            s.skipped = true;
        }
    }
    s
}
