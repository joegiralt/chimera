//! The Algo Sound's parameters, stored as bytes (spec § Voice model).

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::algo::algorithms::{ALGO_COUNT, ALGO_IDENTS, ALGO_NAMES, AlgoId};
use crate::dsp::algo::env::EnvRates;
use crate::dsp::algo::plan::OPS;
use crate::dsp::algo::tx::COARSE_NAMES;
use crate::dsp::algo::waves::{WAVE_COUNT, WAVE_IDENTS, WAVE_NAMES, WaveId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoOpParams {
    pub wave: u8,
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
    pub feedback: u8,
    pub velocity: u8,
}

impl Default for AlgoOpParams {
    fn default() -> Self {
        Self {
            wave: 0,
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
            feedback: 0,
            velocity: 0,
        }
    }
}

impl AlgoOpParams {
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
pub static ALGO_OP_SPECS: [ParamSpec; 13] = [
    ParamSpec::choice(0, "WAVE", ValFmt::Names(&WAVE_NAMES), 15.0, 0.0)
        .ident("WAVE")
        .glyph(crate::ui::glyph::FocusGlyph::Wave),
    ParamSpec::stepped(
        1,
        "CRSE",
        ValFmt::Names(&COARSE_NAMES),
        0.0,
        63.0,
        4.0,
        false,
    )
    .ident("CRSE"),
    ParamSpec::stepped(2, "FINE", ValFmt::Int(15), 0.0, 15.0, 0.0, false).ident("FINE"),
    ParamSpec::stepped(3, "DETUN", ValFmt::Signed(3), -3.0, 3.0, 0.0, false).ident("DETUN"),
    ParamSpec::stepped(4, "LEVEL", ValFmt::Int(99), 0.0, 99.0, 0.0, true)
        .ident("LEVEL")
        .glyph(crate::ui::glyph::FocusGlyph::LevelBar),
    ParamSpec::stepped(5, "AR", ValFmt::Int(31), 0.0, 31.0, 31.0, false).ident("AR"),
    ParamSpec::stepped(6, "D1R", ValFmt::Int(31), 0.0, 31.0, 0.0, false).ident("D1R"),
    ParamSpec::stepped(7, "D1L", ValFmt::Int(15), 0.0, 15.0, 15.0, false).ident("D1L"),
    ParamSpec::stepped(8, "D2R", ValFmt::Int(31), 0.0, 31.0, 0.0, false).ident("D2R"),
    ParamSpec::stepped(9, "RR", ValFmt::OneBased(14), 1.0, 15.0, 5.0, false).ident("RR"),
    ParamSpec::stepped(10, "RS", ValFmt::Int(3), 0.0, 3.0, 0.0, false).ident("RS"),
    ParamSpec::stepped(11, "FDBK", ValFmt::Int(7), 0.0, 7.0, 0.0, false)
        .ident("FDBK")
        .glyph(crate::ui::glyph::FocusGlyph::LevelBar),
    ParamSpec::stepped(12, "VEL", ValFmt::Int(7), 0.0, 7.0, 0.0, false).ident("VEL"),
];

// An Enum's `max` is its last code: a longer table needs the spec to grow too.
const _: () = assert!(ALGO_OP_SPECS[0].max as usize == WAVE_COUNT - 1);
const _: () = assert!(WAVE_IDENTS.len() == WAVE_COUNT && WAVE_NAMES.len() == WAVE_COUNT);

impl Block for AlgoOpParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ALGO_OP_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::WAVE => self.wave as f32,
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
            Self::FEEDBACK => self.feedback as f32,
            Self::VELOCITY => self.velocity as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::WAVE => self.wave = v as u8,
            Self::COARSE => self.coarse = v as u8,
            Self::FINE => self.fine = v as u8,
            Self::DETUNE => self.detune = v as i8,
            Self::LEVEL => self.level = v as u8,
            Self::AR => self.ar = v as u8,
            Self::D1R => self.d1r = v as u8,
            Self::D1L => self.d1l = v as u8,
            Self::D2R => self.d2r = v as u8,
            Self::RR => self.rr = v as u8,
            Self::RATE_SCALE => self.rate_scale = v as u8,
            Self::FEEDBACK => self.feedback = v as u8,
            Self::VELOCITY => self.velocity = v as u8,
            _ => {}
        }
    }

    /// WAVE's code is its `WaveId` index.
    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::WAVE).then_some(self.wave)
    }

    /// A wave's ident is its own literal (`Recipe::ident`), not its label.
    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        (id == Self::WAVE)
            .then(|| WAVE_IDENTS.get(usize::from(self.wave)).copied())
            .flatten()
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        id == Self::WAVE
            && WaveId::from_index(code)
                .map(|w| self.wave = w.get())
                .is_some()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoParams {
    pub alg_a: u8,
    pub alg_b: u8,
    pub morph: u8,
    pub transpose: i8,
    pub ops: [AlgoOpParams; OPS],
}

/// INIT's operators 2–4 (ADR 0049). At 72 T1's stack keeps a strong
/// fundamental (0.60, beside 0.40 0.34 0.66 for harmonics 2–4); 66–70 all
/// but cancel it and INIT reads an octave up.
const INIT_LEVEL: u8 = 72;
/// Operators routed on INIT: four, so eight INIT voices fit rev V (ADR 0040).
const INIT_OPS: usize = 4;
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

    /// Operator 1 alone at full level on `wave` (the one-operator Sounds).
    pub fn single(wave: WaveId) -> Self {
        let mut ops = [AlgoOpParams::default(); OPS];
        (ops[0].wave, ops[0].level) = (wave.get(), 99);
        Self {
            alg_a: 0,
            alg_b: 0,
            morph: 0,
            transpose: 0,
            ops,
        }
    }
}

pub static ALGO_SPECS: [ParamSpec; 4] = [
    ParamSpec::choice(
        0,
        "ALG A",
        ValFmt::Names(&ALGO_NAMES),
        31.0,
        INIT_A.get() as f32,
    )
    .ident("ALG_A")
    .glyph(crate::ui::glyph::FocusGlyph::None),
    ParamSpec::choice(
        1,
        "ALG B",
        ValFmt::Names(&ALGO_NAMES),
        31.0,
        INIT_B.get() as f32,
    )
    .ident("ALG_B")
    .glyph(crate::ui::glyph::FocusGlyph::None),
    ParamSpec::stepped(2, "MORPH", ValFmt::Uni, 0.0, 127.0, 0.0, true)
        .ident("MORPH")
        .glyph(crate::ui::glyph::FocusGlyph::Crossfader)
        .short("MRPH"),
    ParamSpec::stepped(3, "TRNSP", ValFmt::Signed(24), -24.0, 24.0, 0.0, false)
        .ident("TRNSP")
        .glyph(crate::ui::glyph::FocusGlyph::Staff),
];

const _: () = assert!(ALGO_SPECS[0].max as usize == ALGO_COUNT - 1);
const _: () = assert!(ALGO_SPECS[1].max as usize == ALGO_COUNT - 1);
const _: () = assert!(ALGO_IDENTS.len() == ALGO_COUNT && ALGO_NAMES.len() == ALGO_COUNT);

impl Block for AlgoParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &ALGO_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::ALG_A => self.alg_a as f32,
            Self::ALG_B => self.alg_b as f32,
            Self::MORPH => self.morph as f32,
            Self::TRANSPOSE => self.transpose as f32,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::ALG_A => self.alg_a = v as u8,
            Self::ALG_B => self.alg_b = v as u8,
            Self::MORPH => self.morph = v as u8,
            Self::TRANSPOSE => self.transpose = v as i8,
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
        let Some(alg) = AlgoId::from_index(code) else {
            return false;
        };
        match id {
            Self::ALG_A => self.alg_a = alg.get(),
            Self::ALG_B => self.alg_b = alg.get(),
            _ => return false,
        }
        true
    }
}
