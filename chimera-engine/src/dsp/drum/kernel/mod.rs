//! DRUM's kernels (ADR 0265): a hit's sound from its kernel params. No-std,
//! no-alloc ports of the host prototypes (chimera-utils `drums/machines.rs`),
//! the same expressions in the same order so they null against them, built
//! from the engine's own parts as the prototypes are (`drums/parts.rs`):
//! VA's NOISE and oscillator, the filter's CLEAN, the FX slot's SOFT and
//! FOLD.

mod burst;
mod metal;
mod plain;
mod tone;

pub use burst::Burst;
pub use metal::Metal;
pub use plain::Plain;
pub use tone::Tone;

use core::mem::MaybeUninit;

use crate::Velocity;
use crate::dsp::fast_sin_turns;
use crate::dsp::filter::{Clean, FilterKind, FilterMode, FilterMods};
use crate::dsp::modal::ModalEngine;
use crate::dsp::va::noise::{Noise, NoiseKind};
use crate::dsp::va::osc::{Morph, VarOsc};
use crate::dsp::voice_fx::soft;
use crate::dsp::{Unit, clampf};
use crate::hw::BLOCK_SIZE;
use crate::in_place::in_place_enum;
use crate::params::FilterParams;

use super::laws::{KernelParams, Sweep};
use super::types::{Gain, HitSeed, Hz, SampleRate, Seconds, Semis};

pub(super) type Blk = [f32; BLOCK_SIZE];

in_place_enum! {
    /// A hit's kernel state, in place in its voice: never boxed, never
    /// built by value and moved in (a METAL bank is 2 KB, and a strike runs
    /// on ORBIT's door's stack). Idle until the voice's first hit.
    #[expect(
        clippy::large_enum_variant,
        reason = "one kernel per voice, built in place: boxing needs a heap"
    )]
    pub enum KernelState {
        Idle(Idle) => rebuild_idle, init_idle;
        Tone(Tone) => rebuild_tone, init_tone;
        Metal(Metal) => rebuild_metal, init_metal;
        Burst(Burst) => rebuild_burst, init_burst;
        Plain(Plain) => rebuild_plain, init_plain;
    }
}

/// No hit yet: silent, finished.
pub struct Idle;

// A hit plays in a voice slot, which MODAL's engine sizes.
const _: () = assert!(size_of::<KernelState>() <= size_of::<ModalEngine>());

impl KernelState {
    /// Idle, in place.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: `Idle` has no fields.
        unsafe { Self::init_idle(slot, |s| s.write(Idle)) }
    }

    /// A hit of `p` at `vel`, by value through the stack: tests and the
    /// null test. A voice strikes in place (`strike_in_place`).
    pub fn strike(p: KernelParams, vel: Velocity, seed: HitSeed, sr: SampleRate) -> KernelState {
        // SAFETY: `init_in_place` writes the whole slot.
        let mut k = unsafe { crate::in_place::by_value(Self::init_in_place) };
        k.strike_in_place(p, vel, seed, sr);
        k
    }

    /// A hit of `p` at `vel`, cutting what played.
    #[inline(never)]
    #[optimize(size)]
    pub fn strike_in_place(
        &mut self,
        p: KernelParams,
        vel: Velocity,
        seed: HitSeed,
        sr: SampleRate,
    ) {
        let h = Hit { vel, seed, sr };
        // SAFETY: each machine's `init_in_place` writes every field.
        unsafe {
            match p {
                KernelParams::Tone(p) => self.rebuild_tone(|s| Tone::init_in_place(s, p, h)),
                KernelParams::Metal(p) => self.rebuild_metal(|s| Metal::init_in_place(s, p, h)),
                KernelParams::Burst(p) => self.rebuild_burst(|s| s.write(Burst::new(p, h))),
                KernelParams::Plain(p) => self.rebuild_plain(|s| s.write(Plain::new(p, h))),
            }
        }
    }

    /// The hit's next block.
    #[inline(never)]
    pub fn render(&mut self, out: &mut Blk) {
        match self {
            KernelState::Idle(_) => out.fill(0.0),
            KernelState::Tone(k) => k.render(out),
            KernelState::Metal(k) => k.render(out),
            KernelState::Burst(k) => k.render(out),
            KernelState::Plain(k) => k.render(out),
        }
        for o in out.iter_mut() {
            *o = ceiling(*o);
        }
    }

    /// The hit has fallen under −80 dB for good: its envelopes are spent
    /// and its last block was quiet.
    pub fn finished(&self) -> bool {
        match self {
            KernelState::Idle(_) => true,
            KernelState::Tone(k) => k.finished(),
            KernelState::Metal(k) => k.finished(),
            KernelState::Burst(k) => k.finished(),
            KernelState::Plain(k) => k.finished(),
        }
    }
}

/// Above +3.5 dBFS a hit bends toward +6 dBFS and never reaches it (C1 at
/// the knee): the prototypes' loudest corners (HAT's NOISE at full, +7 dBFS)
/// stay under the bound, and every quieter sample passes untouched.
#[inline(always)]
fn ceiling(x: f32) -> f32 {
    const KNEE: f32 = 1.5;
    const ROOM: f32 = 0.5;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let o = a - KNEE;
        (KNEE + o / (1.0 + o / ROOM)).copysign(x)
    }
}

/// What every kernel takes at the strike.
#[derive(Clone, Copy)]
pub(super) struct Hit {
    pub vel: Velocity,
    pub seed: HitSeed,
    pub sr: SampleRate,
}

/// −80 dB.
pub(super) const FLOOR: f32 = 1e-4;

/// A block whose peak is under −80 dB.
pub(super) fn quiet(b: &Blk) -> bool {
    b.iter().all(|v| v.abs() < FLOOR)
}

/// The per-sample factor that falls 60 dB in `t60` seconds.
pub(super) fn per_sample(t60: Seconds, sr: SampleRate) -> f32 {
    libm::expf(-6.907_755 / (t60.get().max(1e-4) * sr.hz()))
}

/// An exponential fall from 1: the machines' decays and sweeps.
#[derive(Clone, Copy)]
pub(super) struct Decay {
    pub v: f32,
    k: f32,
}

impl Decay {
    #[inline(never)]
    pub fn t60(t: Seconds, sr: SampleRate) -> Decay {
        Decay {
            v: 1.0,
            k: per_sample(t, sr),
        }
    }

    /// Falls to 1/e in `tau`.
    #[inline(never)]
    pub fn tau(tau: Seconds, sr: SampleRate) -> Decay {
        Decay {
            v: 1.0,
            k: libm::expf(-1.0 / (tau.get().max(1e-5) * sr.hz())),
        }
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        let v = self.v;
        self.v *= self.k;
        v
    }

    pub fn spent(&self) -> bool {
        self.v < FLOOR
    }
}

/// The prototypes' LCG: particle draws and clap jitter.
#[derive(Clone, Copy)]
pub(super) struct Lcg(u32);

impl Lcg {
    /// The hit's seed, salted per use as the prototypes salt it.
    pub fn new(seed: HitSeed, salt: u32) -> Lcg {
        Lcg(seed.get() ^ salt)
    }

    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 as i32) as f32 / 2_147_483_648.0
    }

    /// 0..1.
    pub fn unit(&mut self) -> f32 {
        0.5 * (self.next() + 1.0)
    }
}

/// `Noise::add`'s white lane gain that brings its RMS to about 0.5.
const HISS_NORM: f32 = 5.0;

/// VA's NOISE, white, about ±1.
#[derive(Clone, Copy)]
pub(super) struct Hiss(Noise);

impl Hiss {
    #[inline(never)]
    pub fn new(seed: HitSeed) -> Hiss {
        Hiss(Noise::new(seed.get() | 1))
    }

    /// A block into `b`.
    #[inline(never)]
    pub fn block(&mut self, b: &mut Blk) {
        *b = [0.0; BLOCK_SIZE];
        self.0
            .add(b, NoiseKind::White, (HISS_NORM, 0.0), (1.0, 0.0, 0.0), 0.0);
    }
}

/// The filter's CLEAN at a mode, cutoff and RES fixed at the strike.
pub(super) struct Svf {
    c: Clean,
    p: FilterParams,
    sr: SampleRate,
}

impl Svf {
    #[inline(never)]
    pub fn new(mode: FilterMode, hz: Hz, res: Unit, sr: SampleRate) -> Svf {
        let mut p = FilterParams::default();
        p.set_kind(FilterKind::Svf);
        // Every mode a kernel names is CLEAN's.
        p.set_mode(mode);
        p.cutoff = clampf(hz.get(), 20.0, 20_000.0);
        p.resonance = res.get();
        Svf {
            c: Clean::new(),
            p,
            sr,
        }
    }

    pub fn run(&mut self, b: &mut Blk) {
        self.c.process(b, &self.p, self.sr.get(), &FilterMods::NONE);
    }
}

/// VA's oscillator at its pulse end: SHAPE 0 the square.
pub(super) struct Pulse {
    o: VarOsc,
    m: Morph,
}

impl Pulse {
    #[inline(never)]
    pub fn new(width: Unit, sr: SampleRate) -> Pulse {
        let mut o = VarOsc::NEW;
        o.prepare(sr.hz());
        Pulse {
            o,
            m: Morph {
                wave: Unit::ONE,
                shape: width,
            },
        }
    }

    #[inline]
    pub fn tick(&mut self, hz: f32) -> f32 {
        let inc = self.o.inc_for(hz);
        self.o.tick_pulse(inc, self.m.shape)
    }
}

/// A swept sine: from `f0` × 2^`oct` down to `f0`, the sweep to 1/e in
/// its tau.
pub(super) struct Swept {
    pub ph: f32,
    f0: f32,
    oct: f32,
    sw: Decay,
    sr: SampleRate,
}

impl Swept {
    #[inline(never)]
    pub fn new(f0: Hz, depth: Semis, tau: Seconds, sr: SampleRate) -> Swept {
        Swept {
            ph: 0.0,
            f0: f0.get(),
            oct: depth.get() / 12.0,
            sw: Decay::tau(tau, sr),
            sr,
        }
    }

    /// The sweep a law resolved.
    pub fn of(s: Sweep, sr: SampleRate) -> Swept {
        Swept::new(s.pitch, s.depth, s.time, sr)
    }

    /// A block of the sine into `v`, and its phase after each sample into
    /// `ph` if given: one loop for every kernel.
    #[inline(never)]
    pub fn block(&mut self, v: &mut Blk, mut ph: Option<&mut Blk>) {
        // The pitch factors first, each read back where its sample goes.
        octaves(&mut self.sw, self.oct, v);
        for (i, v) in v.iter_mut().enumerate() {
            let f = self.f0 * *v;
            self.ph += f / self.sr.hz();
            if self.ph >= 1.0 {
                self.ph -= libm::floorf(self.ph);
            }
            *v = fast_sin_turns(self.ph);
            if let Some(ph) = ph.as_deref_mut() {
                ph[i] = self.ph;
            }
        }
    }
}

/// 2^(`oct` × each of `d`'s next block of values) into `m`: a sweep's
/// pitch factor, its one `exp2f` for every kernel. A recurrence:
/// private issue #952
#[inline(never)]
pub(super) fn octaves(d: &mut Decay, oct: f32, m: &mut Blk) {
    for m in m.iter_mut() {
        *m = libm::exp2f(oct * d.next());
    }
}

/// Noise through a filter under an envelope: clicks, snaps, wires.
pub(super) struct NoiseLayer {
    hiss: Hiss,
    env: Decay,
    f: Svf,
    pub level: Gain,
}

impl NoiseLayer {
    #[inline(never)]
    pub fn new(
        seed: HitSeed,
        t60: Seconds,
        mode: FilterMode,
        hz: Hz,
        res: Unit,
        level: Gain,
        sr: SampleRate,
    ) -> NoiseLayer {
        NoiseLayer {
            hiss: Hiss::new(seed),
            env: Decay::t60(t60, sr),
            f: Svf::new(mode, hz, res, sr),
            level,
        }
    }

    /// A block into `b`.
    #[inline(never)]
    pub fn block(&mut self, b: &mut Blk) {
        self.hiss.block(b);
        for s in b.iter_mut() {
            *s *= self.env.next() * self.level.get();
        }
        self.f.run(b);
    }

    pub fn spent(&self) -> bool {
        self.env.v * self.level.get() < FLOOR
    }
}

/// The FX slot's SOFT at `drive`, its TONE ½ and unity LEVEL.
pub(super) fn drive(x: f32, d: f32) -> f32 {
    soft::curve(x, [d, 0.5, soft::UNITY_LEVEL])
}
