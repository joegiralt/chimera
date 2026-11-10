//! The DRUM machines of the design note's table (2026-11-10), each a law
//! over six knobs (0..1) on one of six kernels: body, fm, membrane, metal,
//! burst, plain. Knob sets from the Machinedrum manual's descriptions only;
//! no firmware, ROM or tables. Host-only prototypes for the owner's ear.

use super::parts::*;
use chimera_core::dsp::fast_sin_turns;
use chimera_core::dsp::filter::FilterMode;
use chimera_core::dsp::voice_fx::{fold, soft};

/// A sounding hit: a block of its output. `fm` (−1..1 a sample) moves
/// its pitch where the kernel has one (COMBINE FM, MCH ← SMP).
pub trait Machine {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Id {
    ToneKick,
    ToneSnare,
    ToneTom,
    ToneClap,
    ToneRim,
    ToneCowbell,
    ToneCymbal,
    MetalHat,
    FmKick,
    FmSnare,
    FmHat,
    BodyKick,
    BodySnare,
    BodyTom,
    BodyBell,
    BodyShaker,
    BodyWood,
    BodyHand,
    MetalJingle,
    PlainSine,
    PlainNoise,
    PlainImpulse,
}

pub struct Spec {
    pub id: Id,
    pub name: &'static str,
    pub family: &'static str,
    pub kernel: &'static str,
    /// A to F, then MORE's two ("-" unused).
    pub knobs: [&'static str; 8],
    /// MORE's two at their defaults (0..127).
    pub more: [u8; 2],
    /// On the 0.4 ship list (owner by ear, 2026-11-10, and the added
    /// scope); TONE RIM is cut but kept as model work.
    pub ships: bool,
}

/// Knob sets checked against the Machinedrum manual's Appendix A (OS
/// 1.63), our own names; the design note's machine table says what
/// differs.
pub const SPECS: [Spec; 22] = {
    const fn s(
        id: Id,
        name: &'static str,
        family: &'static str,
        kernel: &'static str,
        knobs: [&'static str; 8],
        more: [u8; 2],
    ) -> Spec {
        Spec {
            id,
            name,
            family,
            kernel,
            knobs,
            more,
            ships: !matches!(id, Id::ToneRim),
        }
    }
    [
        s(
            Id::ToneKick,
            "TONE KICK",
            "TONE",
            "body",
            [
                "PITCH",
                "DECAY",
                "SWEEP",
                "SWEEP TIME",
                "CLICK",
                "DRIVE",
                "START",
                "HARMONICS",
            ],
            [0, 0],
        ),
        s(
            Id::ToneSnare,
            "TONE SNARE",
            "TONE",
            "body",
            [
                "PITCH",
                "DECAY",
                "BUMP",
                "SNAP",
                "SNAP DECAY",
                "TONE",
                "INTERVAL",
                "DRIVE",
            ],
            [60, 0],
        ),
        s(
            Id::ToneTom,
            "TONE TOM",
            "TONE",
            "body",
            [
                "PITCH",
                "DECAY",
                "SWEEP",
                "SWEEP TIME",
                "NOISE",
                "DAMP",
                "DRIVE",
                "DRIVE TYPE",
            ],
            [0, 0],
        ),
        s(
            Id::ToneClap,
            "TONE CLAP",
            "TONE",
            "burst",
            [
                "TONE",
                "DECAY",
                "CLAPS",
                "SPREAD",
                "TAIL",
                "RING",
                "HARDNESS",
                "ROOM TONE",
            ],
            [64, 127],
        ),
        s(
            Id::ToneRim,
            "TONE RIM",
            "TONE",
            "body",
            ["PITCH", "DECAY", "TONE", "CLICK", "RING", "NOISE", "-", "-"],
            [0, 0],
        ),
        s(
            Id::ToneCowbell,
            "TONE COWBELL",
            "TONE",
            "metal",
            [
                "PITCH", "DECAY", "INTERVAL", "TONE", "RING", "DRIVE", "DAMP", "BUMP",
            ],
            [76, 0],
        ),
        s(
            Id::ToneCymbal,
            "TONE CYMBAL",
            "TONE",
            "metal",
            [
                "PITCH", "DECAY", "TONE", "SPREAD", "RING", "SHIMMER", "TOP", "TOP TUNE",
            ],
            [76, 64],
        ),
        s(
            Id::MetalHat,
            "METAL HAT",
            "TONE",
            "metal",
            [
                "PITCH",
                "DECAY",
                "TONE",
                "SPREAD",
                "RING",
                "NOISE",
                "HIGH PASS",
                "LOW PASS",
            ],
            [64, 127],
        ),
        s(
            Id::FmKick,
            "FM KICK",
            "FM",
            "fm",
            [
                "PITCH",
                "DECAY",
                "SWEEP",
                "MOD",
                "RATIO",
                "MOD DECAY",
                "SWEEP TIME",
                "FEEDBACK",
            ],
            [50, 0],
        ),
        s(
            Id::FmSnare,
            "FM SNARE",
            "FM",
            "fm",
            [
                "PITCH",
                "DECAY",
                "NOISE",
                "NOISE DECAY",
                "MOD",
                "RATIO",
                "MOD DECAY",
                "HIGH PASS",
            ],
            [40, 64],
        ),
        s(
            Id::FmHat,
            "FM HAT",
            "FM",
            "fm",
            [
                "PITCH",
                "DECAY",
                "TREMOLO",
                "TREMOLO RATE",
                "MOD",
                "FEEDBACK",
                "RATIO",
                "MOD DECAY",
            ],
            [64, 64],
        ),
        s(
            Id::BodyKick,
            "BODY KICK",
            "BODY",
            "membrane",
            [
                "PITCH", "DECAY", "HARDNESS", "MALLET", "TENSION", "DAMP", "POSITION", "-",
            ],
            [20, 0],
        ),
        s(
            Id::BodySnare,
            "BODY SNARE",
            "BODY",
            "membrane",
            [
                "PITCH",
                "DECAY",
                "HARDNESS",
                "TENSION",
                "WIRES",
                "WIRE DECAY",
                "RING",
                "DAMP",
            ],
            [30, 0],
        ),
        s(
            Id::BodyTom,
            "BODY TOM",
            "BODY",
            "membrane",
            [
                "PITCH", "DECAY", "HARDNESS", "TENSION", "DAMP", "POSITION", "MALLET", "SIZE",
            ],
            [40, 64],
        ),
        s(
            Id::BodyBell,
            "BODY BELL",
            "BODY",
            "membrane",
            [
                "PITCH",
                "DECAY",
                "HARDNESS",
                "STRUCTURE",
                "BRIGHT",
                "DAMP",
                "TENSION",
                "-",
            ],
            [0, 0],
        ),
        s(
            Id::BodyShaker,
            "BODY SHAKER",
            "BODY",
            "burst",
            [
                "GRAINS",
                "DECAY",
                "GRAIN LENGTH",
                "TONE",
                "SHAKE",
                "RING",
                "HARDNESS",
                "-",
            ],
            [64, 0],
        ),
        s(
            Id::BodyWood,
            "BODY WOOD",
            "BODY",
            "membrane",
            [
                "PITCH", "DECAY", "HARDNESS", "MATERIAL", "RING", "CLICK", "DUAL", "TUNE",
            ],
            [0, 64],
        ),
        s(
            Id::BodyHand,
            "BODY HAND",
            "BODY",
            "membrane",
            [
                "PITCH", "DECAY", "HARDNESS", "SHELL", "STROKE", "TENSION", "POSITION", "SIZE",
            ],
            [50, 64],
        ),
        s(
            Id::MetalJingle,
            "METAL JINGLE",
            "TONE",
            "metal",
            [
                "PITCH", "DECAY", "JINGLES", "SHAKE", "TONE", "RING", "HEAD", "SPREAD",
            ],
            [0, 40],
        ),
        s(
            Id::PlainSine,
            "PLAIN SINE",
            "PLAIN",
            "plain",
            ["PITCH", "DECAY", "SWEEP", "SWEEP TIME", "-", "-", "-", "-"],
            [0, 0],
        ),
        s(
            Id::PlainNoise,
            "PLAIN NOISE",
            "PLAIN",
            "plain",
            ["DECAY", "TONE", "-", "-", "-", "-", "-", "-"],
            [0, 0],
        ),
        s(
            Id::PlainImpulse,
            "PLAIN IMPULSE",
            "PLAIN",
            "plain",
            [
                "PITCH",
                "DECAY",
                "SWEEP",
                "SWEEP TIME",
                "-",
                "-",
                "WIDTH",
                "-",
            ],
            [117, 0],
        ),
    ]
};

pub fn spec(id: Id) -> &'static Spec {
    SPECS
        .iter()
        .find(|s| s.id == id)
        .expect("every Id has a spec")
}

/// Eight knobs (A–F and MORE's two), 0..1.
pub type Knobs = [f32; 8];

/// A hit of `id` at knobs `k` (0..1) and velocity `vel` (0..1).
pub fn build(id: Id, k: Knobs, vel: f32, seed: u32) -> Box<dyn Machine> {
    match id {
        Id::ToneKick => Box::new(ToneKick::new(k, vel, seed)),
        Id::ToneSnare => Box::new(ToneSnare::new(k, vel, seed)),
        Id::ToneTom => Box::new(ToneTom::new(k, vel, seed)),
        Id::ToneClap => Box::new(Burst::clap(k, vel, seed)),
        Id::ToneRim => Box::new(ToneRim::new(k, vel, seed)),
        Id::ToneCowbell => Box::new(Cowbell::new(k, vel)),
        Id::ToneCymbal => Box::new(Metal::cymbal(k, vel, seed)),
        Id::MetalHat => Box::new(Metal::hat(k, vel, seed)),
        Id::FmKick => Box::new(Fm::kick(k, vel, seed)),
        Id::FmSnare => Box::new(Fm::snare(k, vel, seed)),
        Id::FmHat => Box::new(Fm::hat(k, vel, seed)),
        Id::BodyKick => Box::new(Membrane::kick(k, vel, seed)),
        Id::BodySnare => Box::new(Membrane::snare(k, vel, seed)),
        Id::BodyTom => Box::new(Membrane::tom(k, vel, seed)),
        Id::BodyBell => Box::new(Membrane::bell(k, vel, seed)),
        Id::BodyShaker => Box::new(Burst::shaker(k, vel, seed)),
        Id::BodyWood => Box::new(Membrane::wood(k, vel, seed)),
        Id::BodyHand => Box::new(Membrane::hand(k, vel, seed)),
        Id::MetalJingle => Box::new(Jingle::new(k, vel, seed)),
        Id::PlainSine => Box::new(Plain::sine(k, vel)),
        Id::PlainNoise => Box::new(Plain::noise(k, vel, seed)),
        Id::PlainImpulse => Box::new(Plain::impulse(k, vel)),
    }
}

/// The prototypes as the owner first heard them (2cacaeae), for the
/// fixes' before/after: FM's pitch a block's, BODY on MODAL's bank.
pub fn build_before(id: Id, k: Knobs, vel: f32, seed: u32) -> Box<dyn Machine> {
    match id {
        Id::FmKick => Box::new(Fm::kick(k, vel, seed).held()),
        Id::FmSnare => Box::new(Fm::snare(k, vel, seed).held()),
        Id::FmHat => Box::new(Fm::hat(k, vel, seed).held()),
        Id::BodyKick => Box::new(BankMembrane::kick(k, vel, seed)),
        Id::BodySnare => Box::new(BankMembrane::snare(k, vel, seed)),
        Id::BodyTom => Box::new(BankMembrane::tom(k, vel, seed)),
        Id::BodyBell => Box::new(BankMembrane::bell(k, vel, seed)),
        _ => build(id, k, vel, seed),
    }
}

/// The FX slot's SOFT at `drive`, its TONE ½ and unity LEVEL.
fn drive(x: f32, d: f32) -> f32 {
    soft::curve(x, [d, 0.5, soft::UNITY_LEVEL])
}

/// FM MCH ← SMP's reach: ±4 octaves of linear deviation clamp to through
/// zero (the head's ADR 0159 clamp, here as a floor at 0 Hz).
fn fm_at(fm: Option<&Blk>, i: usize) -> f32 {
    fm.map_or(1.0, |b| (1.0 + 4.0 * b[i]).max(0.0))
}

// ---------------------------------------------------------------- body

/// A swept sine: TONE KICK's, TOM's and SNARE's body, PLAIN SINE.
struct Swept {
    ph: f32,
    f0: f32,
    oct: f32,
    sw: Decay,
}

impl Swept {
    fn new(f0: f32, st: f32, tau: f32) -> Self {
        Self {
            ph: 0.0,
            f0,
            oct: st / 12.0,
            sw: Decay::tau(tau),
        }
    }

    #[inline]
    fn tick(&mut self, fm: f32) -> f32 {
        let f = self.f0 * (self.oct * self.sw.next()).exp2() * fm;
        self.ph += f / FS;
        if self.ph >= 1.0 {
            self.ph -= self.ph.floor();
        }
        fast_sin_turns(self.ph)
    }
}

/// Noise through a filter under an envelope: clicks, snaps, wires.
struct NoiseLayer {
    hiss: Hiss,
    env: Decay,
    f: Flt,
    hz: f32,
    level: f32,
}

impl NoiseLayer {
    fn new(seed: u32, t60: f32, mode: FilterMode, hz: f32, res: f32, level: f32) -> Self {
        Self {
            hiss: Hiss::new(seed),
            env: Decay::t60(t60),
            f: Flt::clean(mode, hz, res),
            hz,
            level,
        }
    }

    fn block(&mut self) -> Blk {
        let mut b = self.hiss.block();
        for s in b.iter_mut() {
            *s *= self.env.next() * self.level;
        }
        self.f.run(&mut b, self.hz);
        b
    }
}

struct ToneKick {
    body: Swept,
    amp: Decay,
    click: NoiseLayer,
    blip: Decay,
    drive: f32,
    harm: f32,
    vel: f32,
}

impl ToneKick {
    fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        Self {
            body: Swept::new(
                expo(k[0], 30.0, 120.0),
                48.0 * k[2],
                expo(k[3], 0.002, 0.12),
            ),
            amp: Decay::t60(expo(k[1], 0.08, 3.0)),
            click: NoiseLayer::new(seed, 0.006, FilterMode::Hp24, 2500.0, 0.1, 1.2 * k[4]),
            blip: Decay::t60(0.004),
            drive: k[5],
            harm: k[7],
            vel,
        }
        .started(k[6])
    }

    /// START: the body begins up to a quarter cycle in, a harder edge.
    fn started(mut self, start: f32) -> Self {
        self.body.ph = 0.25 * start;
        self
    }
}

impl Machine for ToneKick {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let c = self.click.block();
        let blip = 0.5 * self.click.level;
        for (i, o) in out.iter_mut().enumerate() {
            let b = self.body.tick(fm_at(fm, i));
            // HARMONICS: the second and third partials folded in.
            let h = self.harm
                * (0.5 * fast_sin_turns(2.0 * self.body.ph)
                    + 0.25 * fast_sin_turns(3.0 * self.body.ph));
            let x = 0.9 * (b + h) * self.amp.next() + c[i] + blip * self.blip.next();
            *o = 0.8 * self.vel * drive(x, self.drive);
        }
    }
}

struct ToneSnare {
    lo: Swept,
    hi: Swept,
    amp: Decay,
    amp_hi: Decay,
    snap: NoiseLayer,
    drive: f32,
    vel: f32,
}

impl ToneSnare {
    fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let f0 = expo(k[0], 100.0, 400.0);
        let t = expo(k[1], 0.04, 0.8);
        // INTERVAL (MORE): the upper head 1.2 to 1.77 up, 1.47 at 60.
        let bump = 12.0 * k[2];
        Self {
            lo: Swept::new(f0, bump, 0.008),
            hi: Swept::new(f0 * lin(k[6], 1.2, 1.7716), bump, 0.008),
            amp: Decay::t60(t),
            amp_hi: Decay::t60(0.6 * t),
            snap: NoiseLayer::new(
                seed,
                expo(k[4], 0.03, 0.6),
                FilterMode::Bp12,
                expo(k[5], 800.0, 9000.0),
                0.0,
                0.35 * k[3],
            ),
            drive: k[7],
            vel,
        }
    }
}

impl Machine for ToneSnare {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let n = self.snap.block();
        for (i, o) in out.iter_mut().enumerate() {
            let m = fm_at(fm, i);
            let b = 0.5 * self.lo.tick(m) * self.amp.next()
                + 0.3 * self.hi.tick(m) * self.amp_hi.next();
            let x = b + n[i];
            *o = self.vel
                * if self.drive > 0.0 {
                    0.8 * drive(x, self.drive)
                } else {
                    x
                };
        }
    }
}

struct ToneTom {
    body: Swept,
    slow: Decay,
    fast: Decay,
    damp: f32,
    noise: NoiseLayer,
    drive: f32,
    hard: f32,
    vel: f32,
}

impl ToneTom {
    fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let f0 = expo(k[0], 50.0, 400.0);
        let t = expo(k[1], 0.08, 2.0);
        Self {
            body: Swept::new(f0, 24.0 * k[2], expo(k[3], 0.004, 0.3)),
            slow: Decay::t60(t),
            fast: Decay::t60(0.2 * t),
            damp: k[5],
            noise: NoiseLayer::new(
                seed,
                0.05,
                FilterMode::Lp12,
                (4.0 * f0).min(12_000.0),
                0.0,
                0.6 * k[4],
            ),
            drive: k[6],
            hard: k[7],
            vel,
        }
    }
}

impl Machine for ToneTom {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let n = self.noise.block();
        for (i, o) in out.iter_mut().enumerate() {
            let a = (1.0 - self.damp) * self.slow.next() + self.damp * self.fast.next();
            let x = 0.85 * self.body.tick(fm_at(fm, i)) * a + n[i];
            // DRIVE through SOFT; DRIVE TYPE leans it into FOLD's harder edge.
            *o = self.vel
                * if self.drive > 0.0 {
                    let s = 0.8 * drive(x, self.drive);
                    let f = 0.7
                        * fold::curve(
                            x * (1.0 + 2.0 * self.drive),
                            [0.3 + 0.7 * self.drive, 0.5, fold::UNITY_LEVEL],
                        );
                    s + self.hard * (f - s)
                } else {
                    x
                };
        }
    }
}

/// Two pinged modes (CLEAN band-passes rung by a click) and a click.
struct ToneRim {
    m1: Flt,
    m2: Flt,
    f1: f32,
    f2: f32,
    click_hp: Flt,
    amp: Decay,
    click: f32,
    noise: NoiseLayer,
    first: bool,
    gain: f32,
    vel: f32,
}

impl ToneRim {
    fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let f1 = expo(k[0], 250.0, 1500.0);
        let f2 = (f1 * lin(k[2], 1.4, 3.2)).min(16_000.0);
        let res = lin(k[4], 0.80, 0.97);
        Self {
            m1: Flt::clean(FilterMode::Bp12, f1, res),
            m2: Flt::clean(FilterMode::Bp12, f2, res),
            f1,
            f2,
            click_hp: Flt::clean(FilterMode::Hp24, 3000.0, 0.0),
            amp: Decay::t60(expo(k[1], 0.01, 0.4)),
            click: k[3],
            noise: NoiseLayer::new(seed, 0.004, FilterMode::Bp12, 3500.0, 0.2, 2.0 * k[5]),
            first: true,
            gain: 1.0,
            vel,
        }
    }
}

impl Machine for ToneRim {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let n = self.noise.block();
        let mut ex = n;
        if core::mem::take(&mut self.first) {
            for (i, s) in ex.iter_mut().enumerate().take(4) {
                *s += [1.0, 0.6, -0.4, -0.2][i] * 6.0;
            }
        }
        let (mut a, mut b, mut c) = (ex, ex, ex);
        self.m1.run(&mut a, self.f1);
        self.m2.run(&mut b, self.f2);
        for s in c.iter_mut() {
            *s *= self.click * 0.15;
        }
        self.click_hp.run(&mut c, 3000.0);
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.vel * self.gain * ((0.6 * a[i] + 0.4 * b[i]) * self.amp.next() + c[i]);
        }
    }
}

// ---------------------------------------------------------------- metal

/// The 808-style six square frequencies (the circuit's published values),
/// and SPREAD's own detune away from them.
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const SPREAD: [f32; 6] = [0.0, 0.31, -0.17, 0.47, 0.83, -0.29];

/// Six squares (once a Part on the unit; one per hit here) and band-passes.
struct Metal {
    sq: [Pulse; 6],
    hz: [f32; 6],
    bps: Vec<(Flt, f32, f32)>,
    hp: Flt,
    hp_hz: f32,
    lp: Option<(Flt, f32)>,
    amp: Decay,
    fast: Decay,
    hiss: Hiss,
    noise: f32,
    shimmer: f32,
    t: f32,
    vel: f32,
    gain: f32,
}

impl Metal {
    fn bank(pitch: f32, spread: f32) -> [f32; 6] {
        let r = (lin(pitch, -24.0, 24.0) / 12.0).exp2();
        core::array::from_fn(|i| METAL[i] * r * (1.0 + spread * SPREAD[i]))
    }

    fn hat(k: Knobs, vel: f32, seed: u32) -> Self {
        let tone = expo(k[2], 3000.0, 14_000.0);
        let res = 0.9 * k[4];
        Self {
            sq: core::array::from_fn(|_| Pulse::new(0.0)),
            hz: Self::bank(k[0], k[3]),
            bps: vec![
                (Flt::clean(FilterMode::Bp12, tone, res), tone, 0.6),
                (
                    Flt::clean(FilterMode::Bp12, tone * 0.68, res),
                    tone * 0.68,
                    0.4,
                ),
            ],
            hp: Flt::clean(FilterMode::Hp24, tone * expo(k[6], 0.25, 1.0), 0.0),
            hp_hz: tone * expo(k[6], 0.25, 1.0),
            lp: (k[7] < 1.0).then(|| {
                let hz = expo(k[7], 2000.0, 20_000.0);
                (Flt::clean(FilterMode::Lp12, hz, 0.0), hz)
            }),
            amp: Decay::t60(expo(k[1], 0.02, 1.5)),
            fast: Decay::t60(0.01),
            hiss: Hiss::new(seed),
            noise: 0.5 * k[5],
            shimmer: 0.0,
            t: 0.0,
            vel,
            gain: 5.5 * (1.0 - 0.7 * k[4]),
        }
    }

    fn cymbal(k: Knobs, vel: f32, seed: u32) -> Self {
        let tone = expo(k[2], 2500.0, 12_000.0);
        let res = 0.9 * k[4];
        Self {
            sq: core::array::from_fn(|_| Pulse::new(0.0)),
            hz: Self::bank(k[0], k[3]),
            bps: vec![
                (Flt::clean(FilterMode::Bp12, tone, res), tone, 0.45),
                (
                    Flt::clean(FilterMode::Bp12, tone * 0.6, res),
                    tone * 0.6,
                    0.35,
                ),
                (
                    Flt::clean(
                        FilterMode::Bp12,
                        (tone * lin(k[7], 1.1, 1.6)).min(18_000.0),
                        res,
                    ),
                    (tone * lin(k[7], 1.1, 1.6)).min(18_000.0),
                    0.5 * k[6],
                ),
            ],
            hp: Flt::clean(FilterMode::Hp24, tone * 0.35, 0.0),
            hp_hz: tone * 0.35,
            lp: None,
            amp: Decay::t60(expo(k[1], 0.2, 5.0)),
            fast: Decay::t60(0.03),
            hiss: Hiss::new(seed),
            noise: 0.15,
            shimmer: k[5],
            t: 0.0,
            vel,
            gain: 2.8 * (1.0 - 0.5 * k[4]),
        }
    }
}

impl Machine for Metal {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let n = self.hiss.block();
        let mut x = [0.0; B];
        // SHIMMER: two slow LFOs detune the squares (±1.5 %) and sway the
        // band-passes' weights; once a block.
        self.t += B as f32 / FS;
        let l1 = fast_sin_turns(4.3 * self.t);
        let l2 = fast_sin_turns(6.1 * self.t + 0.3);
        let det = 0.015 * self.shimmer;
        for (i, s) in x.iter_mut().enumerate() {
            let mut sum = 0.0;
            for (j, (o, &hz)) in self.sq.iter_mut().zip(&self.hz).enumerate() {
                let d = if j % 2 == 0 { l1 } else { l2 };
                sum += o.tick(hz * (1.0 + det * d));
            }
            *s = sum * (1.0 / 6.0) + self.noise * n[i];
        }
        let mut y = [0.0; B];
        for (j, (f, hz, w)) in self.bps.iter_mut().enumerate() {
            let mut b = x;
            f.run(&mut b, *hz);
            let sway = 1.0 + 0.5 * self.shimmer * if j % 2 == 0 { l1 } else { l2 };
            for (o, v) in y.iter_mut().zip(&b) {
                *o += *w * sway * v;
            }
        }
        self.hp.run(&mut y, self.hp_hz);
        if let Some((f, hz)) = &mut self.lp {
            f.run(&mut y, *hz);
        }
        for (o, v) in out.iter_mut().zip(&y) {
            let a = 0.8 * self.amp.next() + 0.2 * self.fast.next();
            *o = self.vel * self.gain * v * a;
        }
    }
}

/// Two of VA's pulses, a band-pass and SOFT.
struct Cowbell {
    p: [Pulse; 2],
    hz: [f32; 2],
    bp: Flt,
    tone: f32,
    slow: Decay,
    fast: Decay,
    drive: f32,
    damp: f32,
    bump: Decay,
    bump_oct: f32,
    vel: f32,
}

impl Cowbell {
    fn new(k: Knobs, vel: f32) -> Self {
        let f = expo(k[0], 300.0, 1200.0);
        let tone = expo(k[3], 600.0, 6000.0);
        Self {
            p: [Pulse::new(0.0), Pulse::new(0.0)],
            hz: [f, f * lin(k[2], 1.2, 1.8)],
            bp: Flt::clean(FilterMode::Bp12, tone, 0.85 * k[4]),
            tone,
            slow: Decay::t60(expo(k[1], 0.08, 1.5)),
            fast: Decay::t60(0.05),
            drive: k[5],
            damp: k[6],
            bump: Decay::tau(0.006),
            bump_oct: k[7],
            vel,
        }
    }
}

impl Machine for Cowbell {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let mut x = [0.0; B];
        for (i, s) in x.iter_mut().enumerate() {
            // BUMP: up to an octave above, falling in 6 ms.
            let m = fm_at(fm, i) * (self.bump_oct * self.bump.next()).exp2();
            *s = 0.5 * (self.p[0].tick(self.hz[0] * m) + self.p[1].tick(self.hz[1] * m));
        }
        self.bp.run(&mut x, self.tone);
        for (o, v) in out.iter_mut().zip(&x) {
            let a = self.damp * self.fast.next() + (1.0 - self.damp) * self.slow.next();
            *o = 0.8 * self.vel * drive(1.6 * v * a, self.drive);
        }
    }
}

/// Tambourine and jingles: the metal bank's squares, rung higher and
/// narrower, gated by SHAKER's particles (each a jingle pair colliding);
/// HEAD a frame drum's thump under them.
struct Jingle {
    sq: [Pulse; 6],
    hz: [f32; 6],
    bps: [(Flt, f32, f32); 2],
    hp: Flt,
    rng: Lcg,
    hiss: Hiss,
    /// Particles a sample at the gesture's peak, its length and the peak's
    /// place (0..1), each particle's fall.
    density: f32,
    len: f32,
    peak: f32,
    env: f32,
    env_k: f32,
    head: Swept,
    head_amp: Decay,
    head_level: f32,
    n: u32,
    vel: f32,
}

impl Jingle {
    fn new(k: Knobs, vel: f32, seed: u32) -> Self {
        let tone = expo(k[4], 5000.0, 14_000.0);
        let res = lin(k[5], 0.5, 0.95);
        Self {
            sq: core::array::from_fn(|_| Pulse::new(0.0)),
            hz: Metal::bank(lin(k[0], 0.25, 1.0), k[7]).map(|f| 2.0 * f),
            bps: [
                (Flt::clean(FilterMode::Bp12, tone, res), tone, 0.6),
                (
                    Flt::clean(FilterMode::Bp12, (tone * 1.42).min(19_000.0), res),
                    (tone * 1.42).min(19_000.0),
                    0.4,
                ),
            ],
            hp: Flt::clean(FilterMode::Hp24, 3000.0, 0.0),
            rng: Lcg(seed ^ 0x7A3B),
            hiss: Hiss::new(seed),
            density: expo(k[2], 300.0, 6000.0) / FS,
            len: expo(k[1], 0.05, 1.0) * FS,
            peak: lin(k[3], 0.0, 0.6),
            env: 1.0,
            env_k: per_sample(expo(k[5], 0.02, 0.12)),
            head: Swept::new(expo(k[0], 120.0, 300.0), 3.0, 0.01),
            head_amp: Decay::t60(0.12),
            head_level: 0.7 * k[6],
            n: 0,
            vel,
        }
    }
}

impl Machine for Jingle {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        let nz = self.hiss.block();
        let mut x = [0.0; B];
        for (i, s) in x.iter_mut().enumerate() {
            let u = self.n as f32 / self.len;
            let shape = if u < self.peak {
                0.4 + 0.6 * u / self.peak
            } else if u < 1.0 {
                let d = (1.0 - u) / (1.0 - self.peak);
                d * d
            } else {
                0.0
            };
            if self.rng.unit() < self.density * shape {
                self.env = (self.env + 0.3 + 0.5 * self.rng.unit()).min(1.5);
            }
            self.env *= self.env_k;
            let mut sum = 0.0;
            for (o, &hz) in self.sq.iter_mut().zip(&self.hz) {
                sum += o.tick(hz * fm_at(fm, i));
            }
            *s = self.env * (sum * (1.0 / 6.0) + 0.3 * nz[i]);
            self.n += 1;
        }
        let mut y = [0.0; B];
        for (f, hz, w) in self.bps.iter_mut() {
            let mut b = x;
            f.run(&mut b, *hz);
            for (o, v) in y.iter_mut().zip(&b) {
                *o += *w * v;
            }
        }
        self.hp.run(&mut y, 3000.0);
        for (o, v) in out.iter_mut().zip(&y) {
            let h = self.head_level * self.head.tick(1.0) * self.head_amp.next();
            *o = self.vel * (0.5 * v + h);
        }
    }
}

// ---------------------------------------------------------------- burst

/// Noise under a schedule of bursts or grains, through a band-pass:
/// TONE CLAP and BODY SHAKER.
struct Burst {
    hiss: Hiss,
    rng: Lcg,
    bp: Flt,
    hz: f32,
    /// Clap: burst onsets (samples) and the tail's start.
    onsets: Vec<u32>,
    burst: f32,
    tail_at: u32,
    tail: Decay,
    tail_level: f32,
    /// Shaker: grains a sample, the shake's length and its peak's place.
    density: f32,
    shake_len: f32,
    peak: f32,
    grain: Decay,
    grain_k: f32,
    shaker: bool,
    n: u32,
    env: f32,
    /// ROOM TONE: the tail's one-pole low-pass coefficient (1 open) and
    /// its state.
    room: f32,
    room_lp: f32,
    /// SHAKER's HARDNESS: dry grains over the band (above ½), a low-pass
    /// on them (below).
    hard: f32,
    hard_lp: f32,
    gain: f32,
    vel: f32,
}

impl Burst {
    fn clap(k: Knobs, vel: f32, seed: u32) -> Self {
        let mut rng = Lcg(seed ^ 0x5eed);
        let claps = 1 + (k[2] * 7.0).round() as u32;
        let spread = expo(k[3], 0.004, 0.03) * FS;
        let mut onsets = Vec::new();
        let mut t = 0.0f32;
        for _ in 0..claps {
            onsets.push(t as u32);
            t += spread * (1.0 + 0.25 * rng.next());
        }
        let tail_at = *onsets.last().unwrap();
        let hz = expo(k[0], 500.0, 4000.0);
        Self {
            hiss: Hiss::new(seed),
            rng,
            bp: Flt::clean(FilterMode::Bp12, hz, 0.9 * k[5]),
            hz,
            onsets,
            burst: (-1.0 / (expo(1.0 - k[6], 0.15, 0.8) * spread)).exp(),
            tail_at,
            tail: Decay::t60(expo(k[1], 0.05, 1.0)),
            tail_level: k[4],
            density: 0.0,
            shake_len: 0.0,
            peak: 0.0,
            grain: Decay::t60(1.0),
            grain_k: 0.0,
            shaker: false,
            n: 0,
            env: 0.0,
            room: 1.0 - (-core::f32::consts::TAU * expo(k[7], 400.0, 24_000.0) / FS).exp(),
            room_lp: 0.0,
            hard: 0.5,
            hard_lp: 0.0,
            gain: 0.6,
            vel,
        }
    }

    fn shaker(k: Knobs, vel: f32, seed: u32) -> Self {
        let mut c = Self::clap([0.5; 8], vel, seed);
        c.onsets.clear();
        c.shaker = true;
        c.density = expo(k[0], 100.0, 3000.0) / FS;
        c.shake_len = expo(k[1], 0.03, 0.5) * FS;
        c.grain_k = per_sample(expo(k[2], 0.0002, 0.003));
        c.hz = expo(k[3], 2000.0, 12_000.0);
        c.peak = lin(k[4], 0.05, 0.7);
        c.bp = Flt::clean(FilterMode::Bp12, c.hz, 0.9 * k[5]);
        c.hard = k[6];
        c.room = 1.0;
        c.gain = 0.8;
        c
    }
}

impl Machine for Burst {
    fn block(&mut self, out: &mut Blk, _fm: Option<&Blk>) {
        let mut x = self.hiss.block();
        for s in x.iter_mut() {
            let e = if self.shaker {
                let u = self.n as f32 / self.shake_len;
                let shape = if u < self.peak {
                    u / self.peak
                } else if u < 1.0 {
                    let d = (1.0 - u) / (1.0 - self.peak);
                    d * d
                } else {
                    0.0
                };
                if self.rng.unit() < self.density * shape.sqrt() {
                    self.grain.v = 0.4 + 0.6 * self.rng.unit();
                }
                self.grain.v *= self.grain_k;
                self.grain.v * shape
            } else {
                if self.onsets.contains(&self.n) {
                    self.env = 1.0;
                }
                self.env *= self.burst;
                let t = if self.n >= self.tail_at {
                    self.tail_level * self.tail.next()
                } else {
                    0.0
                };
                let e = self.env.max(0.6 * t);
                // ROOM TONE darkens the tail's share alone.
                self.room_lp += self.room * (*s - self.room_lp);
                let w = if e > 0.0 { 0.6 * t / e } else { 0.0 };
                *s += w * (self.room_lp - *s);
                e
            };
            *s *= e;
            self.n += 1;
        }
        let dry = x;
        self.bp.run(&mut x, self.hz);
        let (wet, lpk) = if self.hard >= 0.5 {
            (2.0 * (self.hard - 0.5), 1.0)
        } else {
            (0.0, expo(2.0 * self.hard, 0.05, 1.0))
        };
        for ((o, v), d) in out.iter_mut().zip(&x).zip(&dry) {
            self.hard_lp += lpk * (v - self.hard_lp);
            *o = self.vel * self.gain * (self.hard_lp + 0.25 * wet * d);
        }
    }
}

// ---------------------------------------------------------------- fm

/// ALGO's operators: op 1 → op 0 (KICK, SNARE) or 2 → 1 → 0 with 2's
/// feedback (HAT). DRUM's own loop (`FmOps`) moves the pitch a sample;
/// `held` plays ALGO's kernel, its pitch a block's (the first prototypes).
struct Fm {
    ops: FmOps,
    kernel: Option<Ops>,
    f0: f32,
    oct: f32,
    sw: f32,
    sw_k: f32,
    amp: f32,
    amp_k: f32,
    idx: f32,
    md: f32,
    md_k: f32,
    ratio: [f32; 3],
    fb: f32,
    trem: (f32, f32),
    noise: Option<NoiseLayer>,
    hp: Option<(Flt, f32)>,
    t: f32,
    vel: f32,
}

const RATIOS: [f32; 8] = [0.5, 1.0, 1.41, 2.0, 2.5, 3.0, 4.0, 7.0];

fn ratio(k: f32) -> f32 {
    RATIOS[((k * 7.99) as usize).min(7)]
}

impl Fm {
    fn base(f0: f32, t60: f32, idx: f32, md_t60: f32, ratio: [f32; 3]) -> Self {
        let bk = B as f32;
        Self {
            ops: FmOps::new(ratio[2] > 0.0),
            kernel: None,
            f0,
            oct: 0.0,
            sw: 1.0,
            sw_k: 1.0,
            amp: 1.0,
            amp_k: per_sample(t60).powf(bk),
            idx,
            md: 1.0,
            md_k: per_sample(md_t60).powf(bk),
            ratio,
            fb: 0.0,
            trem: (0.0, 0.0),
            noise: None,
            hp: None,
            t: 0.0,
            vel: 1.0,
        }
    }

    /// ALGO's kernel instead: the pitch held a block.
    fn held(mut self) -> Self {
        let mods = if self.ratio[2] > 0.0 {
            [0, 1 << 0, 1 << 1, 0, 0, 0]
        } else {
            [0, 1 << 0, 0, 0, 0, 0]
        };
        self.kernel = Some(Ops::new(mods, 1));
        self
    }

    fn kick(k: Knobs, vel: f32, _seed: u32) -> Self {
        let mut m = Self::base(
            expo(k[0], 30.0, 120.0),
            expo(k[1], 0.1, 3.0),
            0.5 * k[3] * k[3],
            expo(k[5], 0.01, 1.0),
            [1.0, ratio(k[4]), 0.0],
        );
        m.oct = 48.0 * k[2] / 12.0;
        // SWEEP TIME (MORE): 5 to 150 ms to 1/e, 19 ms at 50.
        m.sw_k = (-(B as f32) / (expo(k[6], 0.005, 0.15) * FS)).exp();
        // FEEDBACK (MORE): the modulator's own, as EFM-BD's MFB.
        m.fb = 0.6 * k[7];
        m.vel = vel;
        m
    }

    fn snare(k: Knobs, vel: f32, seed: u32) -> Self {
        let mut m = Self::base(
            expo(k[0], 120.0, 400.0),
            expo(k[1], 0.05, 0.8),
            0.4 * k[4] * k[4],
            expo(k[6], 0.01, 1.0),
            [1.0, ratio(k[5]), 0.0],
        );
        let hp = expo(k[7], 300.0, 5000.0);
        m.noise = Some(NoiseLayer::new(
            seed,
            expo(k[3], 0.03, 0.6),
            FilterMode::Hp24,
            hp,
            0.0,
            0.25 * k[2],
        ));
        m.vel = vel;
        m
    }

    fn hat(k: Knobs, vel: f32, _seed: u32) -> Self {
        let f0 = expo(k[0], 300.0, 3000.0);
        let r = expo(k[6], 0.5, 2.0);
        let mut m = Self::base(
            f0,
            expo(k[1], 0.02, 1.2),
            0.6 * k[4],
            expo(k[7], 0.02, 1.2),
            [1.0, 3.47 * r, 5.19 * r],
        );
        m.fb = k[5];
        m.trem = (k[2], expo(k[3], 2.0, 60.0));
        m.hp = Some((Flt::clean(FilterMode::Hp24, 5000.0, 0.0), 5000.0));
        m.vel = vel;
        m
    }
}

impl Machine for Fm {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        self.sw *= self.sw_k;
        let f = self.f0 * (self.oct * self.sw).exp2();
        self.amp *= self.amp_k;
        self.md *= self.md_k;
        self.t += B as f32 / FS;
        let trem = 1.0 - self.trem.0 * 0.5 * (1.0 + fast_sin_turns(self.trem.1 * self.t));
        let g = self.amp * trem * 0.8;
        let mi = self.idx * self.md;
        let three = self.ratio[2] > 0.0;
        if let Some(k) = &mut self.kernel {
            // The kernel's `inc` is a block's: the clip's mean over it.
            let m = fm.map_or(1.0, |b| {
                (1.0 + 4.0 * b.iter().sum::<f32>() / B as f32).max(0.0)
            });
            let hz = [
                f * m * self.ratio[0],
                f * m * self.ratio[1],
                f * m * self.ratio[2],
                0.0,
                0.0,
                0.0,
            ];
            let gains = if three {
                [g, mi, mi * 0.7, 0.0, 0.0, 0.0]
            } else {
                [g, mi, 0.0, 0.0, 0.0, 0.0]
            };
            k.block(out, hz, gains, [0.0, 0.0, self.fb, 0.0, 0.0, 0.0]);
        } else {
            let gains = [g, mi, if three { mi * 0.7 } else { 0.0 }];
            self.ops
                .block(out, self.ratio.map(|r| f * r), gains, self.fb, fm);
        }
        if let Some((f, hz)) = &mut self.hp {
            f.run(out, *hz);
        }
        if let Some(n) = &mut self.noise {
            let b = n.block();
            for (o, v) in out.iter_mut().zip(&b) {
                *o += v;
            }
        }
        for o in out.iter_mut() {
            *o *= self.vel;
        }
    }
}

// ------------------------------------------------------------- membrane

/// Modes a BODY hit rings (the design note's membrane state: 8 modes).
const MODES: usize = 8;

/// MODAL's STRUCTURE law (Rings' stiffness, `modal::rings`) as mode
/// ratios, so BODY keeps the tuning the owner approved.
fn stretched(structure: f32) -> [f32; MODES] {
    let mut stiff = if structure < 0.24 {
        -0.0625 * (1.0 - structure / 0.24)
    } else if structure < 0.3 {
        0.0
    } else {
        let t = (structure - 0.3) / 0.7;
        t * t * 0.5
    };
    let mut stretch = 1.0f32;
    core::array::from_fn(|i| {
        let r = (i + 1) as f32 * stretch;
        stretch = (stretch + stiff).max(0.1);
        if stiff < 0.0 {
            stiff *= 0.93;
        }
        r
    })
}

/// WOOD's materials, rim to woodblock; MATERIAL glides between
/// neighbours. Clave is the free-free bar (Euler–Bernoulli: 1, 2.756,
/// 5.404, 8.933, ...); the others are our own shell-and-rim sets.
const WOOD: [[f32; MODES]; 4] = [
    [1.0, 1.72, 2.31, 3.05, 3.9, 4.6, 5.8, 7.1],
    [1.0, 2.12, 2.98, 4.2, 5.1, 6.6, 7.9, 9.3],
    [1.0, 2.756, 5.404, 8.933, 13.34, 18.64, 24.81, 31.87],
    [1.0, 1.55, 2.42, 3.7, 4.95, 6.2, 7.6, 9.1],
];

/// The fundamental's T60 for MODAL's DAMP (0..1), its Q law at a mode
/// (`1 + f·q`, `q = 500·10^(3·DAMP)`): 91 ms at 0.2, 0.72 s at ½.
fn damp_t60(d: f32) -> f32 {
    0.022_88 * 10f32.powf(3.0 * d.clamp(0.0, 1.0))
}

/// A BODY hit's laws: how loud and how long each mode is.
#[derive(Clone, Copy)]
struct Law {
    f0: f32,
    ratio: [f32; MODES],
    /// The fundamental's T60, s; mode n's is `t60 · ratio^−tilt`.
    t60: f32,
    tilt: f32,
    /// Mode n's amplitude is `ratio^−slope` (a struck membrane's modes
    /// fall with frequency), times the strike position's weight.
    slope: f32,
    /// 0 centre (the upper modes 0.4 down) to 1 edge.
    pos: f32,
    /// The mallet's contact, s: a half-sine force pulse this long.
    contact: f32,
    /// MODAL's noise burst in the strike, for texture, 0..1.
    grit: f32,
}

impl Law {
    fn modes(&self) -> Modes<MODES> {
        let mut m = Modes::new();
        m.ratio = self.ratio;
        for i in 0..MODES {
            let r = self.ratio[i];
            let w = if i == 0 {
                1.0 - 0.4 * self.pos
            } else {
                lin(self.pos, 0.4, 1.0) * (0.75 + 0.25 * fast_sin_turns(i as f32 * self.pos + 0.25))
            };
            m.amp[i] = w * r.powf(-self.slope);
            m.t60[i] = self.t60 * r.powf(-self.tilt);
        }
        m
    }
}

/// BODY: a mode bank whose fundamental leads (each mode impulse-normalised,
/// its amplitude and decay set by the law), struck by a mallet's force
/// pulse with a little of MODAL's burst noise; TENSION a falling pitch.
/// The snare's wires, WOOD's click and second strike ride on it.
struct Membrane {
    modes: Modes<MODES>,
    f0: f32,
    bend: f32,
    bend_k: f32,
    /// The strike: pulse samples left and its length; burst left, its
    /// level, its one-pole state; a second strike's onset (DUAL).
    n: u32,
    pulse: u32,
    burst: u32,
    grit: f32,
    lp: f32,
    rng: Lcg,
    dual: Option<(u32, f32)>,
    vel: f32,
    wires: Option<NoiseLayer>,
    click: Option<NoiseLayer>,
    follow: f32,
    gain: f32,
}

impl Membrane {
    fn new(law: Law, vel: f32, seed: u32) -> Self {
        let mut modes = law.modes();
        modes.set(law.f0);
        Self {
            modes,
            f0: law.f0,
            bend: 0.0,
            bend_k: 1.0,
            n: 0,
            pulse: ((law.contact * FS) as u32).max(1),
            burst: ((law.contact.max(0.002) * 1.5) * FS) as u32,
            grit: law.grit,
            lp: 0.0,
            rng: Lcg(seed ^ 0xB0D1),
            dual: None,
            vel,
            wires: None,
            click: None,
            follow: 0.0,
            gain: 0.9,
        }
    }

    /// TENSION: `st` semitones above, falling to 1/e in `tau`.
    fn tension(mut self, st: f32, tau: f32) -> Self {
        self.bend = st / 12.0;
        self.bend_k = (-(B as f32) / (tau * FS)).exp();
        self.modes.set(self.f0 * self.bend.exp2());
        self
    }

    /// The strike's force at sample `n` from its start: the pulse (area 1)
    /// and the burst's noise.
    fn force(&mut self, n: u32) -> f32 {
        let mut x = 0.0;
        if n < self.pulse {
            let l = self.pulse as f32;
            x += core::f32::consts::FRAC_PI_2 / l * fast_sin_turns(0.5 * (n as f32 + 0.5) / l);
        }
        if n < self.burst && self.grit > 0.0 {
            self.lp += 0.4 * (self.rng.next() - self.lp);
            x += 0.08 * self.grit * self.lp;
        }
        x
    }

    fn kick(k: Knobs, vel: f32, seed: u32) -> Self {
        let law = Law {
            f0: expo(k[0], 35.0, 120.0),
            ratio: stretched(0.12),
            t60: damp_t60(lin(k[1], 0.2, 0.75) - 0.2 * k[5]),
            tilt: lin(k[5], 0.7, 1.5),
            slope: lin(k[2], 1.6, 0.7),
            pos: k[6],
            // MALLET: a softer mallet the higher (as PI-BD's HAMR).
            contact: expo(k[3], 0.0004, 0.006),
            grit: 0.3 * k[2],
        };
        let mut m = Self::new(law, vel, seed).tension(12.0 * k[4], 0.04);
        m.gain = 1.0;
        m
    }

    fn snare(k: Knobs, vel: f32, seed: u32) -> Self {
        let law = Law {
            f0: expo(k[0], 120.0, 400.0),
            ratio: stretched(0.15),
            t60: damp_t60(lin(k[1], 0.15, 0.6) - 0.3 * k[7]),
            // RING (MORE, PI-SD's metallic ringing): the upper modes ring
            // nearer the fundamental's length.
            tilt: 0.8 - 0.5 * k[6],
            slope: lin(k[2], 1.2, 0.5),
            pos: 0.2,
            contact: expo(1.0 - k[2], 0.0003, 0.002),
            grit: 0.5,
        };
        let mut m = Self::new(law, vel, seed).tension(7.0 * k[3], 0.03);
        m.wires = Some(NoiseLayer::new(
            seed,
            expo(k[5], 0.05, 0.6),
            FilterMode::Hp24,
            3000.0,
            0.0,
            0.2 * k[4] * vel,
        ));
        m.gain = 0.8;
        m
    }

    fn tom(k: Knobs, vel: f32, seed: u32) -> Self {
        let law = Law {
            f0: expo(k[0], 60.0, 400.0),
            // SIZE (MORE): the shell's stretch and the ring's length.
            ratio: stretched(lin(k[7], 0.04, 0.16)),
            t60: damp_t60(lin(k[1], 0.25, 0.75) - 0.25 * k[4]) * lin(k[7], 0.7, 1.3),
            tilt: lin(k[4], 0.7, 1.5),
            slope: lin(k[2], 1.6, 0.7),
            pos: k[5] * 0.5,
            contact: expo(k[6], 0.0004, 0.006),
            grit: 0.3 * k[2],
        };
        let mut m = Self::new(law, vel, seed).tension(12.0 * k[3], 0.08);
        m.gain = 0.8;
        m
    }

    fn bell(k: Knobs, vel: f32, seed: u32) -> Self {
        let law = Law {
            f0: expo(k[0], 150.0, 1500.0),
            ratio: stretched(lin(k[3], 0.3, 0.8)),
            t60: damp_t60(lin(k[1], 0.4, 0.95) - 0.3 * k[5]),
            tilt: lin(k[4], 1.0, 0.4) + 0.5 * k[5],
            slope: lin(k[4], 1.3, 0.0),
            pos: 0.6,
            contact: expo(1.0 - k[2], 0.0002, 0.003),
            grit: 0.2,
        };
        // TENSION (MORE): a hard hit's pitch rise, as PI-ML's TENS.
        let mut m = Self::new(law, vel, seed).tension(3.0 * k[6] * vel, 0.15);
        m.gain = 0.9;
        m
    }

    /// WOOD: rim, sidestick, clave, woodblock (MATERIAL), short and
    /// bright-damped; RING lets the second mode sing (a rim's ring), CLICK
    /// a stick's tick, DUAL a second strike (TRX-CL's), TUNE the second
    /// mode's pitch.
    fn wood(k: Knobs, vel: f32, seed: u32) -> Self {
        let x = k[3].clamp(0.0, 1.0) * 3.0;
        let (i, t) = ((x as usize).min(2), x - (x as usize).min(2) as f32);
        let mut ratio: [f32; MODES] =
            core::array::from_fn(|n| WOOD[i][n] + t * (WOOD[i + 1][n] - WOOD[i][n]));
        ratio[1] *= expo(k[7], 0.85, 1.18);
        let law = Law {
            f0: expo(k[0], 200.0, 3000.0),
            ratio,
            t60: expo(k[1], 0.015, 0.5),
            tilt: 1.4,
            slope: lin(k[2], 1.4, 0.6),
            pos: 0.5,
            contact: expo(1.0 - k[2], 0.0001, 0.0012),
            grit: 0.3,
        };
        let mut modes = law.modes();
        modes.t60[1] *= 1.0 + 6.0 * k[4];
        modes.amp[1] *= 1.0 + 0.6 * k[4];
        let mut m = Self::new(law, vel, seed);
        m.modes = modes;
        m.modes.set(m.f0);
        m.click = Some(NoiseLayer::new(
            seed ^ 7,
            0.004,
            FilterMode::Hp24,
            3000.0,
            0.0,
            0.25 * k[5],
        ));
        m.dual = (k[6] > 0.0).then(|| ((expo(k[6], 0.002, 0.03) * FS) as u32, 0.7));
        m.gain = 0.9;
        m
    }

    /// Hand drums: BODY TOM's membrane with SHELL (bongo, conga, timbale's
    /// metal shell) and STROKE: OPEN, SLAP or MUTE.
    fn hand(k: Knobs, vel: f32, seed: u32) -> Self {
        let shell = k[3];
        let (pos, contact, slope, grit, ring, keep) = match stroke(k[4]) {
            Stroke::Open => (0.25 + 0.5 * k[6], 0.0018, 1.3, 0.15, 1.0, 1.0),
            Stroke::Slap => (0.9, 0.0004, 0.5, 0.9, 0.35, 0.8),
            Stroke::Mute => (0.3 + 0.5 * k[6], 0.003, 1.5, 0.1, 0.15, 0.6),
        };
        let mut ratio = stretched(lin(shell, 0.05, 0.13));
        // Timbale: the steel shell's two ringing modes in the top slots.
        let metal = ((shell - 0.66) * 3.0).clamp(0.0, 1.0);
        if metal > 0.0 {
            ratio[6] = 4.73;
            ratio[7] = 6.91;
        }
        let law = Law {
            f0: expo(k[0], 80.0, 600.0),
            ratio,
            t60: expo(k[1], 0.08, 1.5) * lin(k[7], 0.6, 1.4) * ring,
            tilt: lin(shell, 1.3, 0.9),
            slope: slope * lin(k[2], 1.2, 0.8),
            pos,
            contact: contact * expo(1.0 - k[2], 0.5, 2.0),
            grit,
        };
        let mut modes = law.modes();
        modes.amp[0] *= keep;
        if metal > 0.0 {
            for i in 6..8 {
                modes.amp[i] = 0.35 * metal;
                modes.t60[i] = 0.6 * law.t60 / ring.max(0.35);
            }
        }
        let mut m = Self::new(law, vel, seed).tension(5.0 * k[5] * vel, 0.05);
        m.modes = modes;
        m.modes.set(m.f0 * m.bend.exp2());
        if matches!(stroke(k[4]), Stroke::Slap) {
            m.click = Some(NoiseLayer::new(
                seed ^ 11,
                0.03,
                FilterMode::Bp12,
                2500.0,
                0.2,
                0.6,
            ));
        }
        m.gain = 0.95;
        m
    }
}

#[derive(Clone, Copy)]
enum Stroke {
    Open,
    Slap,
    Mute,
}

/// STROKE's three zones.
fn stroke(k: f32) -> Stroke {
    if k < 1.0 / 3.0 {
        Stroke::Open
    } else if k < 2.0 / 3.0 {
        Stroke::Slap
    } else {
        Stroke::Mute
    }
}

impl Machine for Membrane {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        self.bend *= self.bend_k;
        let m = fm.map_or(1.0, |b| {
            (1.0 + 4.0 * b.iter().sum::<f32>() / B as f32).max(0.01)
        });
        if self.bend != 0.0 || fm.is_some() {
            self.modes.set(self.f0 * self.bend.exp2() * m);
        }
        let ends = self.pulse.max(self.burst);
        for o in out.iter_mut() {
            let n = self.n;
            let mut x = if n < ends { self.force(n) } else { 0.0 };
            if let Some((at, g)) = self.dual
                && n >= at
                && n < at + ends
            {
                x += g * self.force(n - at);
            }
            self.n += 1;
            *o = self.vel * self.modes.tick(x);
        }
        if let Some(c) = &mut self.click {
            let b = c.block();
            for (o, v) in out.iter_mut().zip(&b) {
                *o += self.vel * v;
            }
        }
        if let Some(w) = &mut self.wires {
            let n = w.block();
            for (o, v) in out.iter_mut().zip(&n) {
                // The wires ride the head's own level a little.
                self.follow += 0.002 * (o.abs() - self.follow);
                *o += v * (1.0 + 4.0 * self.follow);
            }
        }
        for o in out.iter_mut() {
            *o = self.gain * (*o).tanh();
        }
    }
}

// ------------------------------------------------- membrane (first prototypes)

/// The first prototypes' BODY (2cacaeae): MODAL's BANK struck by its
/// burst, TENSION a falling pitch, plus a wire noise for the snare. Its
/// modes' gain rises with their pitch (unnormalised band-passes, Q ∝ f),
/// so the upper modes lead: kept for the before/after and as model work.
struct BankMembrane {
    bank: Bank,
    f0: f32,
    bend: f32,
    bend_k: f32,
    wires: Option<NoiseLayer>,
    follow: f32,
    gain: f32,
}

impl BankMembrane {
    fn new(
        f0: f32,
        structure: f32,
        damp: f32,
        bright: f32,
        burst: f32,
        pos: f32,
        vel: f32,
    ) -> Self {
        let mut bank = Bank::new();
        bank.p.structure = structure;
        bank.p.damp = damp.clamp(0.0, 1.0);
        bank.p.bright = bright.clamp(0.0, 1.0);
        bank.p.burst = burst.clamp(0.0, 1.0);
        bank.p.pos = pos;
        bank.p.excite = 1.0;
        Self {
            bank,
            f0,
            bend: 0.0,
            bend_k: 1.0,
            wires: None,
            follow: 0.0,
            gain: 1.6,
        }
        .struck(vel)
    }

    fn struck(mut self, vel: f32) -> Self {
        self.bank.strike(self.f0, vel);
        self
    }

    /// TENSION: `st` semitones above, falling to 1/e in `tau`.
    fn tension(mut self, st: f32, tau: f32) -> Self {
        self.bend = st / 12.0;
        self.bend_k = (-(B as f32) / (tau * FS)).exp();
        self.bank.retune(self.f0 * self.bend.exp2());
        self
    }

    fn kick(k: Knobs, vel: f32, _seed: u32) -> Self {
        // POSITION (MORE) at its default.
        let damp = lin(k[1], 0.2, 0.75) - 0.2 * k[5];
        Self::new(
            expo(k[0], 35.0, 120.0),
            0.12,
            damp,
            lin(k[2], 0.05, 0.5) - 0.2 * k[5],
            k[3],
            0.15,
            vel,
        )
        .tension(12.0 * k[4], 0.04)
    }

    fn snare(k: Knobs, vel: f32, seed: u32) -> Self {
        let mut m = Self::new(
            expo(k[0], 120.0, 400.0),
            0.15,
            lin(k[1], 0.15, 0.6),
            lin(k[2], 0.15, 0.7),
            0.3,
            0.2,
            vel,
        )
        .tension(7.0 * k[3], 0.03);
        m.wires = Some(NoiseLayer::new(
            seed,
            expo(k[5], 0.05, 0.6),
            FilterMode::Hp24,
            3000.0,
            0.0,
            0.2 * k[4] * vel,
        ));
        m.gain = 1.1;
        m
    }

    fn tom(k: Knobs, vel: f32, _seed: u32) -> Self {
        let damp = lin(k[1], 0.25, 0.75) - 0.25 * k[4];
        Self::new(
            expo(k[0], 60.0, 400.0),
            0.1,
            damp,
            lin(k[2], 0.1, 0.6),
            0.4,
            k[5] * 0.5,
            vel,
        )
        .tension(12.0 * k[3], 0.08)
    }

    fn bell(k: Knobs, vel: f32, _seed: u32) -> Self {
        let damp = lin(k[1], 0.4, 0.95) - 0.3 * k[5];
        Self::new(
            expo(k[0], 150.0, 1500.0),
            lin(k[3], 0.3, 0.8),
            damp,
            0.35 * k[4],
            1.0 - k[2],
            0.3,
            vel,
        )
    }
}

impl Machine for BankMembrane {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        self.bend *= self.bend_k;
        let m = fm.map_or(1.0, |b| {
            (1.0 + 4.0 * b.iter().sum::<f32>() / B as f32).max(0.01)
        });
        self.bank.retune(self.f0 * self.bend.exp2() * m);
        self.bank.block(out);
        if let Some(w) = &mut self.wires {
            let n = w.block();
            for (o, v) in out.iter_mut().zip(&n) {
                // The wires ride the head's own level a little.
                self.follow += 0.002 * (o.abs() - self.follow);
                *o += v * (1.0 + 4.0 * self.follow);
            }
        }
        for o in out.iter_mut() {
            *o *= self.gain;
        }
    }
}

// ---------------------------------------------------------------- plain

struct Plain {
    body: Option<Swept>,
    pulse: Option<(Pulse, f32, f32, Decay, Flt)>,
    noise: Option<(Hiss, Flt, f32)>,
    amp: Decay,
    vel: f32,
}

impl Plain {
    fn sine(k: Knobs, vel: f32) -> Self {
        Self {
            body: Some(Swept::new(
                expo(k[0], 30.0, 2000.0),
                48.0 * k[2],
                expo(k[3], 0.002, 0.2),
            )),
            pulse: None,
            noise: None,
            amp: Decay::t60(expo(k[1], 0.02, 3.0)),
            vel,
        }
    }

    fn noise(k: Knobs, vel: f32, seed: u32) -> Self {
        // TONE: below ½ a low-pass closing, above a high-pass opening.
        let (mode, hz) = if k[1] < 0.5 {
            (FilterMode::Lp12, expo(k[1] * 2.0, 200.0, 20_000.0))
        } else {
            (FilterMode::Hp24, expo((k[1] - 0.5) * 2.0, 20.0, 10_000.0))
        };
        Self {
            body: None,
            pulse: None,
            noise: Some((Hiss::new(seed), Flt::clean(mode, hz, 0.0), hz)),
            amp: Decay::t60(expo(k[0], 0.01, 2.0)),
            vel,
        }
    }

    fn impulse(k: Knobs, vel: f32) -> Self {
        Self {
            body: None,
            pulse: Some((
                Pulse::new(k[6]),
                expo(k[0], 30.0, 2000.0),
                48.0 * k[2] / 12.0,
                Decay::tau(expo(k[3], 0.002, 0.2)),
                Flt::clean(FilterMode::Hp24, 20.0, 0.0),
            )),
            noise: None,
            amp: Decay::t60(expo(k[1], 0.02, 3.0)),
            vel,
        }
    }
}

impl Machine for Plain {
    fn block(&mut self, out: &mut Blk, fm: Option<&Blk>) {
        if let Some(b) = &mut self.body {
            for (i, o) in out.iter_mut().enumerate() {
                *o = 0.9 * b.tick(fm_at(fm, i));
            }
        } else if let Some((p, f0, oct, sw, hp)) = &mut self.pulse {
            for (i, o) in out.iter_mut().enumerate() {
                *o = -1.2 * p.tick(*f0 * (*oct * sw.next()).exp2() * fm_at(fm, i));
            }
            hp.run(out, 20.0);
        } else if let Some((h, f, hz)) = &mut self.noise {
            *out = h.block().map(|v| 0.4 * v);
            f.run(out, *hz);
        }
        for o in out.iter_mut() {
            *o *= self.vel * self.amp.next();
        }
    }
}
