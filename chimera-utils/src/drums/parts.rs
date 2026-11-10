//! The engine's own parts as the DRUM prototypes play them: VA's NOISE and
//! oscillator, the filter's TYPEs, ALGO's kernel, MODAL's bank, the FX
//! slot's curves. Thin wrappers only; the DSP is chimera-engine's.

use chimera_core::MidiNote;
use chimera_core::dsp::algo::env::{EnvCoefs, EnvRates, OpEnv};
use chimera_core::dsp::algo::kernel::{Kernel, KernelBlock, KindBlock, OpBlock, SAMPLE_SCALE};
use chimera_core::dsp::algo::plan::{EvalPlan, OPS};
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::{Filter, FilterKind, FilterMode, FilterMods};
use chimera_core::dsp::modal::{
    BankModes, ModalEngine, ModalParams, Model, ResonatorMode, SymPool,
};
use chimera_core::dsp::va::noise::{Noise, NoiseKind};
use chimera_core::dsp::va::osc::{Morph, VarOsc};
use chimera_core::dsp::{Unit, note_to_freq};
use chimera_core::params::FilterParams;
use chimera_core::taps::TapRows;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

pub const B: usize = BLOCK_SIZE;
pub const FS: f32 = SAMPLE_RATE as f32;
pub type Blk = [f32; B];

/// The per-sample factor that falls 60 dB in `t60` seconds.
pub fn per_sample(t60: f32) -> f32 {
    (-6.907_755 / (t60.max(1e-4) * FS)).exp()
}

/// `k` in 0..1 on an exponential law from `lo` to `hi`.
pub fn expo(k: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(k)
}

pub fn lin(k: f32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * k
}

/// An exponential fall from 1: the machines' decays and sweeps.
#[derive(Clone, Copy)]
pub struct Decay {
    pub v: f32,
    k: f32,
}

impl Decay {
    pub fn t60(t: f32) -> Self {
        Self {
            v: 1.0,
            k: per_sample(t),
        }
    }

    /// Falls to 1/e in `tau` seconds.
    pub fn tau(tau: f32) -> Self {
        Self {
            v: 1.0,
            k: (-1.0 / (tau.max(1e-5) * FS)).exp(),
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let v = self.v;
        self.v *= self.k;
        v
    }
}

/// VA's NOISE (white), scaled to ±1 full scale.
pub struct Hiss(Noise);

/// `Noise::add`'s white lane gain that brings its RMS to about 0.5
/// (measured: unit lane is about 0.1 RMS).
const HISS_NORM: f32 = 5.0;

impl Hiss {
    pub fn new(seed: u32) -> Self {
        Self(Noise::new(seed | 1))
    }

    pub fn block(&mut self) -> Blk {
        let mut b = [0.0; B];
        self.0.add(
            &mut b,
            NoiseKind::White,
            (HISS_NORM, 0.0),
            (1.0, 0.0, 0.0),
            0.0,
        );
        b
    }
}

/// One of the filter's TYPEs at a mode, its cutoff and RES per block.
pub struct Flt {
    f: Filter,
    p: FilterParams,
}

impl Flt {
    pub fn new(kind: FilterKind, mode: FilterMode, hz: f32, res: f32) -> Self {
        let mut p = FilterParams::default();
        p.set_kind(kind);
        assert!(p.set_mode(mode), "{mode:?} is not a mode of {kind:?}");
        p.cutoff = hz;
        p.resonance = res;
        Self {
            f: Filter::new(),
            p,
        }
    }

    pub fn clean(mode: FilterMode, hz: f32, res: f32) -> Self {
        Self::new(FilterKind::Svf, mode, hz, res)
    }

    pub fn run(&mut self, b: &mut Blk, hz: f32) {
        self.p.cutoff = hz.clamp(20.0, 20_000.0);
        self.f.process(b, &self.p, SAMPLE_RATE, &FilterMods::NONE);
    }
}

/// VA's oscillator at its pulse end (WAVE 1): SHAPE 0 the square.
pub struct Pulse {
    o: VarOsc,
    m: Morph,
}

impl Pulse {
    pub fn new(width: f32) -> Self {
        let mut o = VarOsc::NEW;
        o.prepare(FS);
        Self {
            o,
            m: Morph {
                wave: Unit::ONE,
                shape: Unit::new(width),
            },
        }
    }

    #[inline]
    pub fn tick(&mut self, hz: f32) -> f32 {
        let inc = self.o.inc_for(hz);
        self.o.tick(inc, self.m, None).out
    }
}

/// ALGO's kernel: up to six sine operators (W1), routed by `mods`, each
/// at its envelope's sustain (the machines draw their own decays as the
/// operators' gain ramps). The pitch is a block's: `inc` per block.
pub struct Ops {
    k: Kernel,
    env: [OpEnv; OPS],
    plan: EvalPlan,
    gain: [f32; OPS],
}

impl Ops {
    /// `mods[i]` bit `j`: op `i` modulates op `j`; `carriers` bit `i`.
    pub fn new(mods: [u8; OPS], carriers: u8) -> Self {
        const HOLD: EnvRates = EnvRates {
            ar: 31,
            d1r: 0,
            d1l: 15,
            d2r: 0,
            rr: 8,
            rs: 0,
        };
        let env = core::array::from_fn(|_| {
            let mut e = OpEnv::IDLE;
            e.note_on(EnvCoefs::new(HOLD, MidiNote::A4, FS));
            e
        });
        Self {
            k: Kernel::new(),
            env,
            plan: EvalPlan::build(&mods, carriers, &mods, carriers),
            gain: [0.0; OPS],
        }
    }

    /// A block: op `i` at `hz[i]`, its gain ramping to `gain[i]` (a
    /// modulator's gain is its PM depth, `PM_CYCLES` at 1), `fb[i]` its
    /// feedback.
    pub fn block(&mut self, out: &mut Blk, hz: [f32; OPS], gain: [f32; OPS], fb: [f32; OPS]) {
        let t = WaveId::W1.table(0);
        let ops = core::array::from_fn(|i| OpBlock {
            inc: (hz[i].clamp(0.0, 20_000.0) as f64 / FS as f64 * 4_294_967_296.0) as u32,
            gain_from: self.gain[i] * SAMPLE_SCALE,
            gain_to: gain[i] * SAMPLE_SCALE,
            feedback: fb[i],
            kind: KindBlock::Table {
                lo: t,
                hi: t,
                xfade_from: 0.0,
                xfade_to: 0.0,
            },
        });
        self.gain = gain;
        let blk = KernelBlock {
            plan: &self.plan,
            ops,
            morph_from: 0.0,
            morph_to: 0.0,
            norm_from: 1.0,
            norm_to: 1.0,
        };
        self.k
            .render(&blk, &mut self.env, out, &mut TapRows::new([false; 2]));
    }
}

/// MODAL's BANK (Rings' modal resonator and its strike burst), sixteen
/// modes, at any pitch: the nearest note below and `set_pitch`'s ratio.
pub struct Bank {
    e: Box<ModalEngine>,
    pool: Box<SymPool>,
    pub p: ModalParams,
    note: u8,
}

impl Bank {
    pub fn new() -> Self {
        let mut raw = Box::<ModalEngine>::new_uninit();
        ModalEngine::init_in_place(&mut raw, Model::Bank);
        // SAFETY: `init_in_place` wrote every field of the engine.
        let e = unsafe { raw.assume_init() };
        let mut raw = Box::<SymPool>::new_uninit();
        SymPool::init_in_place(&mut raw);
        // SAFETY: `init_in_place` built the pool in the box.
        let pool = unsafe { raw.assume_init() };
        let p = ModalParams {
            mode: ResonatorMode::Modal,
            modes: BankModes::M16,
            ..ModalParams::default()
        };
        Self {
            e,
            pool,
            p,
            note: 0,
        }
    }

    /// Struck at `hz` and `vel` (0..1).
    pub fn strike(&mut self, hz: f32, vel: f32) {
        let mut n = 0u8;
        while n < 127 && note_to_freq(n + 1) <= hz {
            n += 1;
        }
        self.note = n;
        self.retune(hz);
        let v = (vel * 127.0).round().clamp(1.0, 127.0) as u8;
        self.e.note_on(n, v, &self.p, SAMPLE_RATE, &mut self.pool);
    }

    /// The bank's pitch now (TENSION's bend).
    pub fn retune(&mut self, hz: f32) {
        self.e.set_pitch(hz / note_to_freq(self.note));
    }

    pub fn block(&mut self, out: &mut Blk) {
        self.e.render(out, &self.p, SAMPLE_RATE, &mut self.pool);
    }
}

/// Denormals flushed on the host (the M7 plays them at full speed; x86
/// does not, and the decays reach them): for the bench's sake.
pub fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    #[allow(deprecated)]
    // SAFETY: sets MXCSR's FTZ and DAZ bits on this thread only; no memory
    // touched.
    unsafe {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        _mm_setcsr(_mm_getcsr() | 0x8040);
    }
}

/// White noise from our own LCG: the clips' raw material and jitter.
pub struct Lcg(pub u32);

impl Lcg {
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 as i32) as f32 / 2_147_483_648.0
    }

    /// 0..1.
    pub fn unit(&mut self) -> f32 {
        0.5 * (self.next() + 1.0)
    }
}
