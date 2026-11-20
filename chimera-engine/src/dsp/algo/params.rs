//! The Algo Sound's parameters, stored as bytes (spec § Voice model).

use crate::addr::Op;
use crate::block::{Block, Change, Motion, ParamId, ParamSpec, Smoothing, ValFmt};
use crate::dsp::algo::algorithms::{ALGO_COUNT, ALGO_IDENTS, ALGO_NAMES, AlgoId};
use crate::dsp::algo::env::EnvRates;
use crate::dsp::algo::kinds::{
    Kinds, Linked, OP_KIND_COUNT, OP_KIND_IDENTS, OP_KIND_NAMES, OpKind, OverBudget, step_kind,
};
use crate::dsp::algo::math::exp2;
use crate::dsp::algo::plan::OPS;
use crate::dsp::algo::tx::COARSE_NAMES;
use crate::dsp::algo::vowel::{VSLOT_IDENTS, VSLOT_NAMES, VowelSlot};
use crate::dsp::algo::waves::WaveId;

/// One operator's parameters but its kind, which `AlgoParams` holds
/// (`Kinds`), so WAVE is read and written through `OpView` and `OpViewMut`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoOpParams {
    pub coarse: u8,
    pub fine: u8,
    pub detune: i8,
    pub level: u8,
    pub ar: u8,
    pub d1r: u8,
    pub d1l: u8,
    pub d2r: u8,
    pub rr: u8,
    pub rate_scale: u8,
    pub feedback: Fdbk,
    pub velocity: u8,
    /// FRMT's formant frequency and NOIS's centre: `frmt_hz`.
    pub frmt: u8,
    /// FRMT's bandwidth and NOIS's width: `bw_hz`.
    pub bw: u8,
    pub skirt: u8,
    pub key: u8,
    pub res: u8,
    pub vslot: u8,
}

/// An operator's FDBK (ADR 0299): the TX81Z's steps 0–7 in tenths of a
/// step, 0..=70. A whole step is the TX81Z's (`tx::FEEDBACK_CYCLES`); the
/// tenths between run straight from one to the next (`tx::feedback_cycles`).
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Fdbk(u8);

/// Its tenths, bare, as the byte printed before it was typed: the neutral
/// Sound's printed form is a pinned contract (`neutral_v1.txt`, ADR 0087).
impl core::fmt::Debug for Fdbk {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self.0, f)
    }
}

impl Fdbk {
    pub const OFF: Fdbk = Fdbk(0);
    /// The TX81Z's top step.
    pub const TX_STEPS: u8 = 7;
    /// Tenths to a step.
    pub const PER_STEP: u8 = 10;
    pub const MAX: Fdbk = Fdbk(Self::TX_STEPS * Self::PER_STEP);
    /// A tenth, in steps: the spec's tick.
    pub const STEP: f32 = 0.1;

    /// The TX81Z's step `n`; past 7, 7.
    pub const fn tx(n: u8) -> Fdbk {
        Fdbk::tenths(n.saturating_mul(Self::PER_STEP))
    }

    /// `t` tenths of a step; past the top, the top.
    pub const fn tenths(t: u8) -> Fdbk {
        Fdbk(if t > Self::MAX.0 { Self::MAX.0 } else { t })
    }

    /// In tenths, 0..=70.
    pub const fn get(self) -> u8 {
        self.0
    }

    pub const fn is_off(self) -> bool {
        self.0 == 0
    }

    /// In the TX81Z's steps, as the spec, the card and the kernel read it:
    /// the `f32` nearest `tenths / 10`, so `n` exactly at whole step `n`
    /// and `0.9` as a text file writes it.
    pub const fn steps(self) -> f32 {
        self.0 as f32 / Self::PER_STEP as f32
    }

    /// The nearest tenth to `v` steps, held to the range: the cast
    /// saturates (under 0 and NaN to 0, past 255 to 255), `tenths` holds
    /// the top, and a half added before truncating rounds what is left.
    pub fn from_steps(v: f32) -> Fdbk {
        Fdbk::tenths((v * Self::PER_STEP as f32 + 0.5) as u8)
    }
}

// Every whole step reads back exactly: an old Sound plays bit for bit.
const _: () = {
    let mut n = 0;
    while n <= Fdbk::TX_STEPS {
        assert!(Fdbk::tx(n).steps() == n as f32);
        n += 1;
    }
};

/// `100 · 2^(v / 24)` Hz.
pub fn frmt_hz(v: f32) -> f32 {
    100.0 * exp2(v / 24.0)
}

/// `20 · 2^(v / 24)` Hz.
pub fn bw_hz(v: f32) -> f32 {
    20.0 * exp2(v / 24.0)
}

impl Default for AlgoOpParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl AlgoOpParams {
    pub const DEFAULT: Self = Self {
        coarse: 4,
        fine: 0,
        detune: 0,
        level: 0,
        ar: 31,
        d1r: 0,
        d1l: 15,
        d2r: 0,
        // RR 5: INIT's release, T60 about 1.1 s (ADR 0049's note).
        rr: 5,
        rate_scale: 0,
        feedback: Fdbk::OFF,
        velocity: 0,
        frmt: 72,
        bw: 72,
        skirt: 1,
        key: 0,
        res: 0,
        vslot: 0,
    };

    pub const WAVE: ParamId = ParamId(0);
    pub const COARSE: ParamId = ParamId(1);
    pub const FINE: ParamId = ParamId(2);
    pub const DETUNE: ParamId = ParamId(3);
    pub const LEVEL: ParamId = ParamId(4);
    pub const AR: ParamId = ParamId(5);
    pub const D1R: ParamId = ParamId(6);
    pub const D1L: ParamId = ParamId(7);
    pub const D2R: ParamId = ParamId(8);
    pub const RR: ParamId = ParamId(9);
    pub const RATE_SCALE: ParamId = ParamId(10);
    pub const FEEDBACK: ParamId = ParamId(11);
    pub const VELOCITY: ParamId = ParamId(12);
    pub const FRMT: ParamId = ParamId(13);
    pub const BW: ParamId = ParamId(14);
    pub const SKIRT: ParamId = ParamId(15);
    pub const KEY: ParamId = ParamId(16);
    pub const RES: ParamId = ParamId(17);
    pub const VSLOT: ParamId = ParamId(18);

    pub fn rates(&self) -> EnvRates {
        EnvRates {
            ar: self.ar,
            d1r: self.d1r,
            d1l: self.d1l,
            d2r: self.d2r,
            rr: self.rr,
            rs: self.rate_scale,
        }
    }
}

/// LEVEL is read every block and applied to the gain after conversion, so
/// it is the one operator destination (ADR 0010). FINE and FEEDBACK are
/// not: FINE's 104-cent steps would zipper.
pub static ALGO_OP_SPECS: [ParamSpec; 19] = [
    ParamSpec::choice(
        0,
        "WAVE",
        ValFmt::Names(&OP_KIND_NAMES),
        17.0,
        0.0,
        Change::Crossfade,
    )
    .ident("WAVE")
    .glyph(crate::glyph::FocusGlyph::Wave),
    ParamSpec::stepped(
        1,
        "CRSE",
        ValFmt::Names(&COARSE_NAMES),
        0.0,
        63.0,
        4.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("CRSE"),
    ParamSpec::stepped(
        2,
        "FINE",
        ValFmt::Int(15),
        0.0,
        15.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("FINE"),
    ParamSpec::stepped(
        3,
        "DETUN",
        ValFmt::Signed(3),
        -3.0,
        3.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("DETUN"),
    ParamSpec::stepped(
        4,
        "LEVEL",
        ValFmt::Int(99),
        0.0,
        99.0,
        0.0,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("LEVEL")
    .glyph(crate::glyph::FocusGlyph::LevelBar),
    ParamSpec::stepped(
        5,
        "AR",
        ValFmt::Int(31),
        0.0,
        31.0,
        31.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("AR"),
    ParamSpec::stepped(
        6,
        "D1R",
        ValFmt::Int(31),
        0.0,
        31.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("D1R"),
    ParamSpec::stepped(
        7,
        "D1L",
        ValFmt::Int(15),
        0.0,
        15.0,
        15.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("D1L"),
    ParamSpec::stepped(
        8,
        "D2R",
        ValFmt::Int(31),
        0.0,
        31.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("D2R"),
    ParamSpec::stepped(
        9,
        "RR",
        ValFmt::OneBased(14),
        1.0,
        15.0,
        5.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("RR"),
    ParamSpec::stepped(
        10,
        "RS",
        ValFmt::Int(3),
        0.0,
        3.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::Own),
    )
    .ident("RS"),
    // The TX81Z's steps 0–7, in tenths (ADR 0299): a file from before
    // holds a whole step, which plays as it did.
    ParamSpec::stepped(
        11,
        "FDBK",
        ValFmt::Tenths(Fdbk::MAX.get()),
        0.0,
        Fdbk::TX_STEPS as f32,
        0.0,
        false,
        Motion::Continuous(Smoothing::KNOB),
    )
    .stepping(Fdbk::STEP)
    .ident("FDBK")
    .glyph(crate::glyph::FocusGlyph::LevelBar),
    ParamSpec::stepped(
        12,
        "VEL",
        ValFmt::Int(7),
        0.0,
        7.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("VEL"),
    ParamSpec::stepped(
        13,
        "FORMANT",
        ValFmt::LogHz {
            base: 100,
            per_octave: 24,
        },
        0.0,
        168.0,
        72.0,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("FRMT")
    .short("FRMT"),
    ParamSpec::stepped(
        14,
        "BW",
        ValFmt::LogHz {
            base: 20,
            per_octave: 24,
        },
        0.0,
        216.0,
        72.0,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("BW"),
    ParamSpec::stepped(
        15,
        "SKIRT",
        ValFmt::Int(3),
        0.0,
        3.0,
        1.0,
        false,
        Motion::Stepped(Change::NoteOn),
    )
    .ident("SKIRT")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::stepped(
        16,
        "KEY",
        ValFmt::Int(100),
        0.0,
        100.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::NoteOn),
    )
    .ident("KEY"),
    ParamSpec::stepped(
        17,
        "RES",
        ValFmt::Int(100),
        0.0,
        100.0,
        0.0,
        false,
        Motion::Continuous(Smoothing::NoteOn),
    )
    .ident("RES"),
    ParamSpec::choice(
        18,
        "SLOT",
        ValFmt::Names(&VSLOT_NAMES),
        4.0,
        0.0,
        Change::NoteOn,
    )
    .ident("VSLOT")
    .glyph(crate::glyph::FocusGlyph::None),
];

// An Enum's `max` is its last code: a longer table needs the spec to grow too.
const _: () = assert!(ALGO_OP_SPECS[0].max as usize == OP_KIND_COUNT - 1);

impl AlgoOpParams {
    /// Every id but WAVE, which `OpView` reads.
    #[inline(never)]
    pub fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::COARSE => self.coarse as f32,
            Self::FINE => self.fine as f32,
            Self::DETUNE => self.detune as f32,
            Self::LEVEL => self.level as f32,
            Self::AR => self.ar as f32,
            Self::D1R => self.d1r as f32,
            Self::D1L => self.d1l as f32,
            Self::D2R => self.d2r as f32,
            Self::RR => self.rr as f32,
            Self::RATE_SCALE => self.rate_scale as f32,
            Self::FEEDBACK => self.feedback.steps(),
            Self::VELOCITY => self.velocity as f32,
            Self::FRMT => self.frmt as f32,
            Self::BW => self.bw as f32,
            Self::SKIRT => self.skirt as f32,
            Self::KEY => self.key as f32,
            Self::RES => self.res as f32,
            Self::VSLOT => self.vslot as f32,
            _ => 0.0,
        }
    }

    /// Every id but WAVE, which `OpViewMut` writes.
    #[inline(never)]
    pub fn write(&mut self, id: ParamId, v: f32) {
        // One conversion, not one per arm.
        let b = v as u8;
        match id {
            Self::COARSE => self.coarse = b,
            Self::FINE => self.fine = b,
            Self::DETUNE => self.detune = v as i8,
            Self::LEVEL => self.level = b,
            Self::AR => self.ar = b,
            Self::D1R => self.d1r = b,
            Self::D1L => self.d1l = b,
            Self::D2R => self.d2r = b,
            Self::RR => self.rr = b,
            Self::RATE_SCALE => self.rate_scale = b,
            Self::FEEDBACK => self.feedback = Fdbk::from_steps(v),
            Self::VELOCITY => self.velocity = b,
            Self::FRMT => self.frmt = b,
            Self::BW => self.bw = b,
            Self::SKIRT => self.skirt = b,
            Self::KEY => self.key = b,
            Self::RES => self.res = b,
            Self::VSLOT => self.vslot = b,
            _ => {}
        }
    }
}

/// Operator `op` of `p` as a block: WAVE is its kind's code (ADR 0045).
/// Out of line, as every block's `get` is (`Block::spec`).
#[inline(never)]
fn op_get(p: &AlgoParams, op: usize, id: ParamId) -> f32 {
    if id == AlgoOpParams::WAVE {
        f32::from(p.kinds.get(op).code())
    } else {
        p.ops[op].get(id)
    }
}

#[inline(never)]
fn op_code(p: &AlgoParams, op: usize, id: ParamId) -> Option<u8> {
    match id {
        AlgoOpParams::WAVE => Some(p.kinds.get(op).code()),
        AlgoOpParams::VSLOT => Some(p.ops[op].vslot),
        _ => None,
    }
}

/// A kind's ident is its own literal (`Recipe::ident`), not its label.
fn op_ident(p: &AlgoParams, op: usize, id: ParamId) -> Option<&'static str> {
    let c = usize::from(op_code(p, op, id)?);
    let idents: &[&str] = if id == AlgoOpParams::WAVE {
        &OP_KIND_IDENTS
    } else {
        &VSLOT_IDENTS
    };
    idents.get(c).copied()
}

/// Shared by both views, so each is one call.
#[inline(never)]
fn op_normalized(p: &AlgoParams, op: usize, id: ParamId) -> f32 {
    crate::block::find_spec(&ALGO_OP_SPECS, id).map_or(0.0, |s| s.normalize(op_get(p, op, id)))
}

/// One operator, read: `BlockRef::AlgoOp`'s block.
pub struct OpView<'a> {
    p: &'a AlgoParams,
    op: usize,
}

/// One operator, written: WAVE only through `AlgoParams::try_set_kind`, so
/// a write that would overspend the formant budget changes nothing.
pub struct OpViewMut<'a> {
    p: &'a mut AlgoParams,
    op: usize,
}

impl Block for OpView<'_> {
    fn specs(&self) -> &'static [ParamSpec] {
        &ALGO_OP_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        op_get(self.p, self.op, id)
    }

    fn normalized(&self, id: ParamId) -> f32 {
        op_normalized(self.p, self.op, id)
    }

    /// A shared view writes nothing; `BlockView` lends it only as `&`.
    fn write(&mut self, _: ParamId, _: f32) {}

    fn set(&mut self, _: ParamId, _: f32) {}

    fn nudge(&mut self, _: ParamId, _: i8) {}

    fn snap(&mut self, _: ParamId, _: i8) {}

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        op_code(self.p, self.op, id)
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        op_ident(self.p, self.op, id)
    }
}

impl Block for OpViewMut<'_> {
    fn specs(&self) -> &'static [ParamSpec] {
        &ALGO_OP_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        op_get(self.p, self.op, id)
    }

    fn normalized(&self, id: ParamId) -> f32 {
        op_normalized(self.p, self.op, id)
    }

    /// Out of line, so `snap` calls it.
    #[inline(never)]
    fn set(&mut self, id: ParamId, v: f32) {
        if let Some(s) = self.spec(id) {
            self.write(id, s.quantized(v));
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        if id == AlgoOpParams::WAVE {
            let _ = self.p.try_set_kind(self.op, OpKind::clamped(v as u8));
        } else {
            self.p.ops[self.op].write(id, v);
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        op_code(self.p, self.op, id)
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        op_ident(self.p, self.op, id)
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            AlgoOpParams::WAVE => {
                OpKind::from_code(code).is_some_and(|k| self.p.try_set_kind(self.op, k).is_ok())
            }
            AlgoOpParams::VSLOT => VowelSlot::from_code(code)
                .map(|v| self.p.ops[self.op].vslot = v.code())
                .is_some(),
            _ => false,
        }
    }

    /// WAVE lands on the kind `delta` on; the budget refuses it (`step_kind`).
    fn nudge(&mut self, id: ParamId, delta: i8) {
        if id == AlgoOpParams::WAVE {
            let from = self.p.kinds.get(self.op);
            let to = step_kind(self.p.kinds, self.p.linked(), self.op, from, delta).to;
            let _ = self.p.try_set_kind(self.op, to);
        } else if let Some(s) = self.spec(id) {
            self.write(id, s.nudged(self.get(id), delta));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoParams {
    /// Private, as `kinds`: an algorithm that would link into a FRMT past
    /// the budget is refused (`try_set_algs`).
    alg_a: u8,
    alg_b: u8,
    pub morph: u8,
    pub transpose: i8,
    /// 0–100 over U O A E I (`ValFmt::Vowel`).
    pub vowel: u8,
    pub ops: [AlgoOpParams; OPS],
    /// Private: every write goes through `try_set_kind`.
    kinds: Kinds,
}

/// INIT's operators 2–4 (ADR 0049). At 72 T1's stack keeps a strong
/// fundamental (0.60, beside 0.40 0.34 0.66 for harmonics 2–4); 66–70 all
/// but cancel it and INIT reads an octave up.
const INIT_LEVEL: u8 = 72;
/// Operators routed on INIT: four, so eight INIT voices fit rev V (ADR 0040).
const INIT_OPS: usize = 4;
/// INIT's formants, the grid points nearest A's F1–F3 and a steady fourth
/// (spec § 2): part of the frozen decode base (ADR 0087).
pub const INIT_FRMT: [u8; OPS] = [69, 83, 111, 123, 123, 123];
/// Vowel A, the table's reference.
pub const INIT_VOWEL: u8 = 50;
const INIT_A: AlgoId = AlgoId::T1;
const INIT_B: AlgoId = AlgoId::A1;

impl Default for AlgoParams {
    /// INIT (ADR 0049): operators 1–4 sounding at ratio 1, so each
    /// algorithm is its own timbre, and ALG B is A1, so MORPH is live: from
    /// T1's four-deep stack down to its clean additive twin.
    fn default() -> Self {
        let mut p = Self::single(WaveId::W1);
        for o in &mut p.ops[1..INIT_OPS] {
            o.level = INIT_LEVEL;
        }
        (p.alg_a, p.alg_b) = (INIT_A.get(), INIT_B.get());
        p
    }
}

impl AlgoParams {
    pub const ALG_A: ParamId = ParamId(0);
    pub const ALG_B: ParamId = ParamId(1);
    pub const MORPH: ParamId = ParamId(2);
    pub const TRANSPOSE: ParamId = ParamId(3);
    pub const VOWEL: ParamId = ParamId(4);

    /// Operator 1 alone at full level on `wave` (the one-operator Sounds).
    pub const fn single(wave: WaveId) -> Self {
        let mut ops = [AlgoOpParams::DEFAULT; OPS];
        let mut i = 0;
        while i < OPS {
            ops[i].frmt = INIT_FRMT[i];
            i += 1;
        }
        ops[0].level = 99;
        let mut w = [WaveId::W1; OPS];
        w[0] = wave;
        Self::new(AlgoId::T1, AlgoId::T1, 0, Kinds::tables(w), ops)
    }

    /// TRNSP 0. Panics unless `kinds` fit on `alg_a` and `alg_b`, so a
    /// `const` recipe is checked at compile time.
    pub const fn new(
        alg_a: AlgoId,
        alg_b: AlgoId,
        morph: u8,
        kinds: Kinds,
        ops: [AlgoOpParams; OPS],
    ) -> Self {
        assert!(
            kinds.fit(Linked::of(alg_a, alg_b)),
            "over the formant budget"
        );
        Self {
            alg_a: alg_a.get(),
            alg_b: alg_b.get(),
            morph,
            transpose: 0,
            vowel: INIT_VOWEL,
            ops,
            kinds,
        }
    }

    pub const fn kinds(&self) -> Kinds {
        self.kinds
    }

    /// ALG A and B, an out-of-range code read as the last algorithm.
    pub const fn algs(&self) -> (AlgoId, AlgoId) {
        (AlgoId::clamped(self.alg_a), AlgoId::clamped(self.alg_b))
    }

    /// The operators ALG A or B links into.
    pub const fn linked(&self) -> Linked {
        let (a, b) = self.algs();
        Linked::of(a, b)
    }

    /// The one write of an operator's kind: refused, it changes nothing.
    pub fn try_set_kind(&mut self, op: usize, kind: OpKind) -> Result<(), OverBudget> {
        self.kinds = self.kinds.with(op, kind, self.linked())?;
        Ok(())
    }

    /// The one write of ALG A and B: refused when the links it adds would
    /// overspend the formant budget, and then it changes nothing.
    pub fn try_set_algs(&mut self, a: AlgoId, b: AlgoId) -> Result<(), OverBudget> {
        self.set_codes(a.get(), b.get())
    }

    /// Raw codes, as a block write stores them (the engine clamps).
    fn set_codes(&mut self, a: u8, b: u8) -> Result<(), OverBudget> {
        let l = Linked::of(AlgoId::clamped(a), AlgoId::clamped(b));
        if !self.kinds.fit(l) {
            return Err(OverBudget);
        }
        (self.alg_a, self.alg_b) = (a, b);
        Ok(())
    }

    fn set_side(&mut self, id: ParamId, code: u8) -> Result<(), OverBudget> {
        match id {
            Self::ALG_A => self.set_codes(code, self.alg_b),
            _ => self.set_codes(self.alg_a, code),
        }
    }

    /// ALG A's or B's (`id`) algorithm.
    pub fn side(&self, id: ParamId) -> AlgoId {
        let (a, b) = self.algs();
        if id == Self::ALG_A { a } else { b }
    }

    pub fn op(&self, op: Op) -> OpView<'_> {
        OpView {
            p: self,
            op: op.index(),
        }
    }

    pub fn op_mut(&mut self, op: Op) -> OpViewMut<'_> {
        OpViewMut {
            p: self,
            op: op.index(),
        }
    }
}

pub static ALGO_SPECS: [ParamSpec; 5] = [
    ParamSpec::choice(
        0,
        "ALG A",
        ValFmt::Names(ALGO_NAMES),
        31.0,
        INIT_A.get() as f32,
        Change::Crossfade,
    )
    .ident("ALG_A")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::choice(
        1,
        "ALG B",
        ValFmt::Names(ALGO_NAMES),
        31.0,
        INIT_B.get() as f32,
        Change::Crossfade,
    )
    .ident("ALG_B")
    .glyph(crate::glyph::FocusGlyph::None),
    ParamSpec::stepped(
        2,
        "MORPH",
        ValFmt::Uni,
        0.0,
        127.0,
        0.0,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("MORPH")
    .glyph(crate::glyph::FocusGlyph::Crossfader)
    .short("MRPH"),
    ParamSpec::stepped(
        3,
        "TRNSP",
        ValFmt::Signed(24),
        -24.0,
        24.0,
        0.0,
        false,
        Motion::Stepped(Change::Snap),
    )
    .ident("TRNSP")
    .glyph(crate::glyph::FocusGlyph::Staff),
    ParamSpec::stepped(
        4,
        "VOWEL",
        ValFmt::Vowel,
        0.0,
        100.0,
        INIT_VOWEL as f32,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident("VOWEL")
    .short("VOWL"),
];

const _: () = assert!(ALGO_SPECS[0].max as usize == ALGO_COUNT - 1);
const _: () = assert!(ALGO_SPECS[1].max as usize == ALGO_COUNT - 1);
const _: () = assert!(ALGO_IDENTS.len() == ALGO_COUNT && ALGO_NAMES.len() == ALGO_COUNT);

impl Block for AlgoParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ALGO_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ALG_A => self.alg_a as f32,
            Self::ALG_B => self.alg_b as f32,
            Self::MORPH => self.morph as f32,
            Self::TRANSPOSE => self.transpose as f32,
            Self::VOWEL => self.vowel as f32,
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::ALG_A | Self::ALG_B => {
                let _ = self.set_side(id, v as u8);
            }
            Self::MORPH => self.morph = v as u8,
            Self::TRANSPOSE => self.transpose = v as i8,
            Self::VOWEL => self.vowel = v as u8,
            _ => {}
        }
    }

    /// ALG A and B's codes are their `AlgoId` indices.
    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::ALG_A => Some(self.alg_a),
            Self::ALG_B => Some(self.alg_b),
            _ => None,
        }
    }

    /// An algorithm's ident is its own literal (`Algorithm::ident`), not its label.
    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        let i = match id {
            Self::ALG_A => self.alg_a,
            Self::ALG_B => self.alg_b,
            _ => return None,
        };
        ALGO_IDENTS.get(usize::from(i)).copied()
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        AlgoId::from_index(code).is_some()
            && matches!(id, Self::ALG_A | Self::ALG_B)
            && self.set_side(id, code).is_ok()
    }

    /// ALG A and B land on the algorithm `delta` on; the budget refuses it
    /// (`step_alg`).
    fn nudge(&mut self, id: ParamId, delta: i8) {
        if matches!(id, Self::ALG_A | Self::ALG_B) {
            let to = step_alg(self, id, self.side(id), delta).to;
            let _ = self.set_side(id, to.get());
        } else if let Some(s) = self.spec(id) {
            self.write(id, s.nudged(self.get(id), delta));
        }
    }
}

/// Where an encoder turn of ALG A or B lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgStep {
    pub to: AlgoId,
    /// `to` would overspend the budget: shown dimmed, never written.
    pub refused: bool,
}

/// The algorithm `delta` on from `from` for ALG A or B (`id`), clamped to
/// the list, whether `p`'s kinds allow it or not.
pub fn step_alg(p: &AlgoParams, id: ParamId, from: AlgoId, delta: i8) -> AlgStep {
    let (a, b) = p.algs();
    let code = (i16::from(from.get()) + i16::from(delta)).clamp(0, ALGO_COUNT as i16 - 1);
    let to = AlgoId::clamped(code as u8);
    let l = if id == AlgoParams::ALG_A {
        Linked::of(to, b)
    } else {
        Linked::of(a, to)
    };
    AlgStep {
        to,
        refused: !p.kinds.fit(l),
    }
}
