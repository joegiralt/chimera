//! What an operator is: one of the wave tables, a formant or a noise band.
//! WAVE's stored code is the kind's code (ADR 0045).

use crate::block::ParamId;
use crate::budget::{FORMANT_BUDGET, FORMANT_BUDGET_FRMT};
use crate::dsp::algo::algorithms::{ALGORITHMS, AlgoId};
use crate::dsp::algo::engine::AlgoEngine;
use crate::dsp::algo::params::AlgoOpParams as P;
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

    /// What the kind draws from the formant budget: its cost over a
    /// table's at its worst SKIRT, with a link into it or without.
    pub const fn charge(self, linked: bool) -> u32 {
        match self {
            OpKind::Table(_) => 0,
            OpKind::Formant => AlgoEngine::frmt_cost(1, linked) - AlgoEngine::COST_OP.0,
            OpKind::Noise => AlgoEngine::COST_NOIS.0 - AlgoEngine::COST_OP.0,
        }
    }
}

/// The operators an algorithm pair links into: bit `i`, operator `i + 1`
/// has a modulator. FDBK isn't a link: it drives only the carrier's phase,
/// never a FRMT's grain delay (spec § 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Linked(u8);

impl Linked {
    pub const NONE: Linked = Linked(0);

    /// Into either of ALG A and B: the plan runs both algorithms' edges at
    /// every MORPH, so each operator bills at the heavier of the two.
    pub const fn of(a: AlgoId, b: AlgoId) -> Linked {
        let (a, b) = (&ALGORITHMS[a.get() as usize], &ALGORITHMS[b.get() as usize]);
        let (mut m, mut i) = (0u8, 0);
        while i < OPS {
            m |= a.mods[i] | b.mods[i];
            i += 1;
        }
        Linked(m)
    }

    pub const fn has(self, op: usize) -> bool {
        self.0 & (1 << op) != 0
    }
}

pub const fn spend(k: &[OpKind; OPS], l: Linked) -> u32 {
    let (mut s, mut i) = (0, 0);
    while i < OPS {
        s += k[i].charge(l.has(i));
        i += 1;
    }
    s
}

/// Within the budget of its floor under `l`: three voices with a FRMT,
/// else four.
pub const fn fits(k: &[OpKind; OPS], l: Linked) -> bool {
    let (mut frmt, mut i) = (false, 0);
    while i < OPS {
        frmt |= matches!(k[i], OpKind::Formant);
        i += 1;
    }
    spend(k, l)
        <= if frmt {
            FORMANT_BUDGET_FRMT.0
        } else {
            FORMANT_BUDGET.0
        }
}

/// The six operators' kinds, within the formant budget on some algorithm
/// (one with no links into them) by construction; `AlgoParams` holds them
/// within it on its own ALG A and B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kinds([OpKind; OPS]);

/// A kind refused: it would overspend the formant budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverBudget;

impl Kinds {
    pub const TABLE: Kinds = Kinds([OpKind::Table(WaveId::W1); OPS]);

    /// Panics unless `k` fits unlinked, so a `const` is checked at compile
    /// time:
    ///
    /// ```compile_fail
    /// use chimera_engine::dsp::algo::kinds::{Kinds, OpKind};
    /// const K: Kinds = Kinds::new([OpKind::Formant; 6]);
    /// let _ = K;
    /// ```
    pub const fn new(k: [OpKind; OPS]) -> Kinds {
        assert!(fits(&k, Linked::NONE), "over the formant budget");
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

    pub fn with(self, op: usize, kind: OpKind, l: Linked) -> Result<Kinds, OverBudget> {
        let mut k = self.0;
        k[op] = kind;
        if fits(&k, l) {
            Ok(Kinds(k))
        } else {
            Err(OverBudget)
        }
    }

    pub fn any_formant(&self) -> bool {
        self.0.iter().any(|k| k.is_formant())
    }

    /// Tables alone charge nothing, so they fit without reading the
    /// budget, which the costliest table Sound defines.
    pub const fn fit(&self, l: Linked) -> bool {
        let mut i = 0;
        while i < OPS {
            if self.0[i].is_formant() {
                return fits(&self.0, l);
            }
            i += 1;
        }
        true
    }
}

/// Whether an operator of `kind` reads `id`: the one table for dimming.
/// A table ignores the formant params; a band ignores pitch (its centre is
/// FRMT) and, as noise, the grain's SKIRT and KEY; a formant has no RES.
pub const fn reads(kind: OpKind, id: ParamId) -> bool {
    !matches!(
        (kind, id),
        (
            OpKind::Table(_),
            P::FRMT | P::BW | P::SKIRT | P::KEY | P::RES | P::VSLOT
        ) | (OpKind::Formant, P::RES)
            | (
                OpKind::Noise,
                P::SKIRT | P::KEY | P::COARSE | P::FINE | P::DETUNE | P::FEEDBACK
            )
    )
}

/// Where an encoder turn of WAVE lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindStep {
    pub to: OpKind,
    /// A refused kind was met on the way: the cell toasts.
    pub skipped: bool,
}

/// `|delta|` allowed kinds on from operator `op`'s under `l`, in `delta`'s
/// direction, stopping at codes 0 and 17.
pub fn step_kind(k: Kinds, l: Linked, op: usize, delta: i8) -> KindStep {
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
        if k.with(op, next, l).is_ok() {
            (s.to, left) = (next, left - 1);
        } else {
            s.skipped = true;
        }
    }
    s
}
