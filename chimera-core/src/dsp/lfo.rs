//! LFO — Low Frequency Oscillator for modulation.
//! Outputs -1.0 to +1.0 (bipolar) at sub-audio rates.

use chimera_hal::BLOCK_SIZE;

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code, identity_code};
use crate::dsp::fast_sin;
use crate::dsp::modulator::func::{BCoefs, FuncGen, Slides};
use crate::dsp::modulator::{Func, FuncParams, Glide, LfoForm, LfoType, pick};

/// LFO waveform shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoShape {
    Sine = 0,
    Triangle = 1,
    Saw = 2,
    Square = 3,
    Random = 4, // sample & hold
}

impl LfoShape {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => LfoShape::Sine,
            1 => LfoShape::Triangle,
            2 => LfoShape::Saw,
            3 => LfoShape::Square,
            4 => LfoShape::Random,
            _ => LfoShape::Sine,
        }
    }
}

impl DiskCode for LfoShape {
    fn disk_code(self) -> u8 {
        match self {
            LfoShape::Sine => 0,
            LfoShape::Triangle => 1,
            LfoShape::Saw => 2,
            LfoShape::Square => 3,
            LfoShape::Random => 4,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            LfoShape::Sine => "SINE",
            LfoShape::Triangle => "TRIANGLE",
            LfoShape::Saw => "SAW",
            LfoShape::Square => "SQUARE",
            LfoShape::Random => "RANDOM",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(LfoShape::Sine),
            1 => Some(LfoShape::Triangle),
            2 => Some(LfoShape::Saw),
            3 => Some(LfoShape::Square),
            4 => Some(LfoShape::Random),
            _ => None,
        }
    }
}

/// LFO parameters.
#[derive(Clone, Copy, Debug)]
pub struct LfoParams {
    /// Rate in Hz (0.01 to 20.0)
    pub rate: f32,
    /// Waveform shape (0-4)
    pub shape: u8,
    /// 0 = free-running, 1 = retrigger on note-on
    pub sync: u8,
    /// Start phase when retriggered (0.0 to 1.0)
    pub phase_offset: f32,
    /// Output depth multiplier (0.0 to 1.0)
    pub depth: f32,
    /// Stored, no longer applied (spec § LFO slots).
    pub offset: f32,
    pub lfo_type: LfoType,
    /// FUNC's FORM and sliders; its MODE is always LFO.
    pub func: FuncParams,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            rate: 1.0, // 1 Hz
            shape: 0,  // sine
            sync: 0,   // free-running
            phase_offset: 0.0,
            depth: 1.0,  // full depth
            offset: 0.0, // centered (bipolar)
            lfo_type: LfoType::Classic,
            func: FuncParams::LFO,
        }
    }
}

impl LfoParams {
    pub const RATE: ParamId = ParamId(0);
    pub const SHAPE: ParamId = ParamId(1);
    pub const SYNC: ParamId = ParamId(2);
    pub const PHASE: ParamId = ParamId(3);
    pub const DEPTH: ParamId = ParamId(4);
    pub const OFFSET: ParamId = ParamId(5);
    pub const TYPE: ParamId = ParamId(6);
    pub const FORM: ParamId = ParamId(7);
    pub const RISE: ParamId = ParamId(8);
    pub const FALL: ParamId = ParamId(9);
    /// FUNC's SHAPE; `SHAPE` is CLASSIC's wave.
    pub const SHAPE_B: ParamId = ParamId(10);
}

/// The LFO sources are computed from the unmodulated `params.lfos`;
/// modulating an LFO itself is out of scope, so nothing here is modulatable.
pub static LFO_SPECS: [ParamSpec; 11] = [
    ParamSpec::continuous(0, "RATE", ValFmt::Uni, 0.01, 20.0, 1.0, 0.15, false).ident("RATE"),
    ParamSpec::choice(1, "SHAPE", ValFmt::Int(4), 4.0, 0.0).ident("SHAPE"),
    ParamSpec::choice(2, "SYNC", ValFmt::Int(1), 1.0, 0.0).ident("SYNC"),
    ParamSpec::continuous(3, "PHASE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false)
        .ident("PHASE"),
    ParamSpec::continuous(4, "DEPTH", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false)
        .ident("DEPTH"),
    ParamSpec::continuous(5, "OFST", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false).ident("OFST"),
    ParamSpec::choice(6, "TYPE", ValFmt::Names(&["CLASSIC", "FUNC"]), 1.0, 0.0)
        .ident("TYPE")
        .glyph(crate::ui::glyph::FocusGlyph::None),
    ParamSpec::choice(7, "FORM", ValFmt::Names(&["FREE", "SYNC", "LFV"]), 2.0, 0.0)
        .ident("FORM")
        .glyph(crate::ui::glyph::FocusGlyph::None),
    ParamSpec::continuous(8, "RISE", ValFmt::Uni, 0.0, 1.0, 0.309, 1.0 / 128.0, false)
        .ident("RISE"),
    ParamSpec::continuous(9, "FALL", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false).ident("FALL"),
    ParamSpec::continuous(10, "SHAPE", ValFmt::Bi, 0.0, 1.0, 0.5, 1.0 / 128.0, false)
        .ident("SHAPE_B"),
];

/// SYNC's values, by the flag's byte (a flag's byte is its meaning).
const SYNC_IDENTS: [&str; 2] = ["FREE", "RETRIG"];
const _: () = assert!(SYNC_IDENTS.len() == LFO_SPECS[2].max as usize + 1);
const _: () = assert!(LfoShape::Random as usize + 1 == LFO_SPECS[1].max as usize + 1);

impl Block for LfoParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &LFO_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::RATE => self.rate,
            Self::SHAPE => self.shape as f32,
            Self::SYNC => self.sync as f32,
            Self::PHASE => self.phase_offset,
            Self::DEPTH => self.depth,
            Self::OFFSET => self.offset,
            Self::TYPE => self.lfo_type as u8 as f32,
            Self::FORM => self.func.form_index(),
            Self::RISE => self.func.rise,
            Self::FALL => self.func.fall,
            Self::SHAPE_B => self.func.shape,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::RATE => self.rate = v,
            Self::SHAPE => self.shape = v as u8,
            Self::SYNC => self.sync = v as u8,
            Self::PHASE => self.phase_offset = v,
            Self::DEPTH => self.depth = v,
            Self::OFFSET => self.offset = v,
            Self::TYPE => self.lfo_type = pick(&LfoType::ALL, v),
            Self::FORM => self.func.set_form_index(v),
            Self::RISE => self.func.rise = v,
            Self::FALL => self.func.fall = v,
            Self::SHAPE_B => self.func.shape = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        match id {
            Self::SHAPE => Some(LfoShape::from_u8(self.shape).disk_code()),
            // SYNC is a flag: 0 free-runs, 1 retriggers. The byte is the meaning.
            Self::SYNC => Some(self.sync),
            Self::TYPE => Some(self.lfo_type.disk_code()),
            Self::FORM => Some(self.func.lfo_form.disk_code()),
            _ => None,
        }
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        match id {
            Self::SHAPE => Some(LfoShape::from_u8(self.shape).disk_ident()),
            Self::SYNC => SYNC_IDENTS.get(usize::from(self.sync)).copied(),
            Self::TYPE => Some(self.lfo_type.disk_ident()),
            Self::FORM => Some(self.func.lfo_form.disk_ident()),
            _ => None,
        }
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        match id {
            Self::SHAPE => apply_code(LfoShape::from_disk_code(code), |s| self.shape = s as u8),
            Self::SYNC => apply_code(identity_code(&LFO_SPECS, id, code), |c| self.sync = c),
            Self::TYPE => apply_code(LfoType::from_disk_code(code), |t| self.lfo_type = t),
            // An LFO's MODE is always LFO, so FORM is its lfo_form, whatever else is loaded.
            Self::FORM => apply_code(LfoForm::from_disk_code(code), |f| self.func.lfo_form = f),
            _ => false,
        }
    }
}

/// LFO state.
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    /// Phase accumulator (0.0 to 1.0)
    phase: f32,
    /// Current output value
    output: f32,
    /// Random/S&H: held value until next cycle
    random_value: f32,
    /// Simple PRNG state for random
    rng_state: u32,
    /// FUNC's generator, and its coefficients with the inputs they came from.
    func: FuncGen,
    bc: Option<(FuncParams, u32, BCoefs)>,
    /// What ran last block; `None` before the first.
    kind: Option<Kind>,
    /// A TYPE or FORM change's leftover, gliding out.
    glide: Glide,
    /// Last block's output, for the glide.
    last: f32,
}

/// What an LFO slot runs: today's LFO, or B locked to LFO mode with a FORM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Classic,
    Func(LfoForm),
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub const fn new() -> Self {
        Self {
            phase: 0.0,
            output: 0.0,
            random_value: 0.0,
            rng_state: 12345,
            func: FuncGen::new(),
            bc: None,
            kind: None,
            glide: Glide::NONE,
            last: 0.0,
        }
    }

    /// A note-on: a CLASSIC LFO with SYNC 1 restarts at its PHASE; FUNC
    /// resets on SYNC; FREE and LFV run on.
    pub fn note_on(&mut self, p: &LfoParams) {
        match p.lfo_type {
            LfoType::Classic if p.sync == 1 => self.retrigger(p.phase_offset),
            LfoType::Classic => {}
            LfoType::Func => self.func.note_on(Func::Lfo(p.func.lfo_form)),
        }
    }

    /// One block (spec § LFO slots): the value at the block's start, then
    /// the advance. CLASSIC is today's arithmetic, bit for bit.
    pub fn run_block(&mut self, p: &LfoParams, sample_rate: u32) -> f32 {
        let kind = match p.lfo_type {
            LfoType::Classic => Kind::Classic,
            LfoType::Func => Kind::Func(p.func.lfo_form),
        };
        let raw = match p.lfo_type {
            LfoType::Classic => self.process(p, sample_rate),
            LfoType::Func => {
                // Reused while FUNC's inputs hold (it runs per block).
                let c = match self.bc {
                    Some((f, sr, c)) if f == p.func && sr == sample_rate => c,
                    _ => {
                        let c = BCoefs::new(&p.func, &Slides::default(), sample_rate, false);
                        self.bc = Some((p.func, sample_rate, c));
                        c
                    }
                };
                self.func.set(&c);
                let v = self.func.output();
                self.func.advance(&c, false, BLOCK_SIZE as u32);
                v
            }
        };
        if self.kind.replace(kind).is_some_and(|k| k != kind) {
            self.glide.start(self.last - raw);
        }
        // No glide: `raw` itself, so CLASSIC stays bit for bit.
        let out = if self.glide.active() {
            (raw + self.glide.value()).clamp(-1.0, 1.0)
        } else {
            raw
        };
        self.glide.advance(BLOCK_SIZE as u16);
        self.last = out;
        out
    }

    /// Reset phase (called on note-on if sync mode)
    pub fn retrigger(&mut self, phase_offset: f32) {
        self.phase = phase_offset;
    }

    /// Get current output without advancing
    pub fn current(&self) -> f32 {
        self.output
    }

    /// Advance LFO by one block and return the output value.
    /// Called once per render block (not per sample).
    pub fn process(&mut self, params: &LfoParams, sample_rate: u32) -> f32 {
        let shape = LfoShape::from_u8(params.shape);

        // Compute raw waveform from phase (0.0 to 1.0)
        let raw = match shape {
            LfoShape::Sine => fast_sin(self.phase * core::f32::consts::TAU),
            LfoShape::Triangle => {
                // 0→1→0→-1→0 over one cycle
                let t = self.phase;
                if t < 0.25 {
                    t * 4.0
                } else if t < 0.75 {
                    2.0 - t * 4.0
                } else {
                    t * 4.0 - 4.0
                }
            }
            LfoShape::Saw => {
                // -1 to +1 ramp
                self.phase * 2.0 - 1.0
            }
            LfoShape::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoShape::Random => {
                self.random_value // held until phase wraps
            }
        };

        // Depth; OFFSET is no longer applied (spec § LFO slots).
        self.output = (raw * params.depth).clamp(-1.0, 1.0);

        // Advance phase
        let phase_inc = params.rate / sample_rate as f32;
        // LFO runs at block rate, so multiply by block size
        self.phase += phase_inc * BLOCK_SIZE as f32;

        // Wrap phase
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            // Random: generate new value on wrap
            if shape == LfoShape::Random {
                self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                self.random_value = (self.rng_state >> 16) as f32 / 32768.0 - 1.0;
            }
        }

        self.output
    }
}
