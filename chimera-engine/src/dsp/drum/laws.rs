//! DRUM's machine laws (ADR 0265): each machine's knob labels, curves and
//! defaults, and `resolve`, which turns a pad's knobs into its kernel's
//! typed params. Ranges, curves and defaults are the host prototypes'
//! (chimera-utils `drums/machines.rs`, the `classic` settings of
//! `drums/mod.rs`), the same expressions in the same order, so the kernels
//! null against them.

use chimera_midi::value::U7;

use crate::dsp::Unit;

use super::types::{
    Gain, Hz, Kernel, Knob, KnobSet, KnobValue, Machine, PerSecond, Ratio, Seconds, Semis,
    Transpose,
};

/// The prototype's `expo`: `k` (0..1) from `lo` to `hi` exponentially.
fn expo(k: f32, lo: f32, hi: f32) -> f32 {
    lo * libm::powf(hi / lo, k)
}

/// The prototype's `lin`.
fn lin(k: f32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * k
}

/// Which of `n` steps `k` lands on: the nearest, as the prototype's
/// `(k * (n − 1)).round()`.
fn step(k: KnobValue, n: usize) -> usize {
    let last = n.saturating_sub(1);
    (libm::roundf(k.unit().get() * last as f32) as usize).min(last)
}

/// A knob's curve from its position to its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    Lin {
        lo: f32,
        hi: f32,
    },
    Expo {
        lo: f32,
        hi: f32,
    },
    /// The nearest of the steps, evenly over the knob's travel.
    Steps(&'static [f32]),
    /// `Lin`, held at `top`.
    Held {
        lo: f32,
        hi: f32,
        top: f32,
    },
}

impl Law {
    pub fn at(self, k: KnobValue) -> f32 {
        match self {
            Law::Lin { lo, hi } => lin(k.unit().get(), lo, hi),
            Law::Expo { lo, hi } => expo(k.unit().get(), lo, hi),
            Law::Steps(s) => s.get(step(k, s.len())).copied().unwrap_or(0.0),
            Law::Held { lo, hi, top } => lin(k.unit().get(), lo, hi).min(top),
        }
    }
}

/// How a knob's value is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValFmt {
    Hz,
    /// Seconds, shown in ms below one.
    Time,
    /// Semitones, signed.
    Semis,
    /// A frequency ratio (×).
    Ratio,
    /// An amount, 0..=100.
    Amount,
    /// A whole count.
    Count,
    /// Octaves, 0..=1 and up.
    Octaves,
    /// Below the middle a low-pass closing, above a high-pass opening.
    Tilt,
}

/// One knob of a machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KnobSpec {
    pub label: &'static str,
    pub short: &'static str,
    pub law: Law,
    pub default: KnobValue,
    pub fmt: ValFmt,
}

/// A machine's names and its knobs A–H (`None` unused).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MachineSpec {
    pub name: &'static str,
    pub short: &'static str,
    pub knobs: [Option<KnobSpec>; 8],
}

impl MachineSpec {
    pub const fn knob(&self, k: Knob) -> Option<KnobSpec> {
        self.knobs[k as usize]
    }
}

const fn kv(v: u8) -> KnobValue {
    KnobValue::new(match U7::new(v) {
        Some(u) => u,
        None => U7::MAX,
    })
}

const fn k(
    label: &'static str,
    short: &'static str,
    law: Law,
    default: u8,
    fmt: ValFmt,
) -> Option<KnobSpec> {
    Some(KnobSpec {
        label,
        short,
        law,
        default: kv(default),
        fmt,
    })
}

const fn lin_(lo: f32, hi: f32) -> Law {
    Law::Lin { lo, hi }
}

const fn expo_(lo: f32, hi: f32) -> Law {
    Law::Expo { lo, hi }
}

/// A plain 0..1 amount.
const AMOUNT: Law = Law::Lin { lo: 0.0, hi: 1.0 };

const CLAPS: [f32; 8] = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];

use ValFmt::{Amount, Count, Hz as FHz, Octaves, Ratio as FRatio, Semis as FSemis, Tilt, Time};

/// Every slice-1 machine, in `Machine::ALL`'s order; defaults are the
/// prototypes' `classic` settings, MORE's their `more`.
static SPECS: [MachineSpec; 12] = [
    MachineSpec {
        name: "TONE KICK",
        short: "KICK",
        knobs: [
            k("PITCH", "PTCH", expo_(30.0, 120.0), 40, FHz),
            k("DECAY", "DEC", expo_(0.08, 3.0), 80, Time),
            k("SWEEP", "SWP", lin_(0.0, 48.0), 40, FSemis),
            k("SWEEP TIME", "SWPT", expo_(0.002, 0.12), 40, Time),
            k("CLICK", "CLCK", lin_(0.0, 1.2), 20, Amount),
            k("DRIVE", "DRIV", AMOUNT, 10, Amount),
            k("START", "STRT", AMOUNT, 0, Amount),
            k("HARMONICS", "HARM", AMOUNT, 0, Amount),
        ],
    },
    MachineSpec {
        name: "TONE SNARE",
        short: "SNARE",
        knobs: [
            k("PITCH", "PTCH", expo_(100.0, 400.0), 50, FHz),
            k("DECAY", "DEC", expo_(0.04, 0.8), 40, Time),
            k("BUMP", "BUMP", lin_(0.0, 12.0), 40, FSemis),
            k("SNAP", "SNAP", lin_(0.0, 0.35), 80, Amount),
            k("SNAP DECAY", "SDEC", expo_(0.03, 0.6), 50, Time),
            k("TONE", "TONE", expo_(800.0, 9000.0), 70, FHz),
            k("INTERVAL", "INTV", lin_(1.2, 1.7716), 60, FRatio),
            k("DRIVE", "DRIV", AMOUNT, 0, Amount),
        ],
    },
    MachineSpec {
        name: "TONE TOM",
        short: "TOM",
        knobs: [
            k("PITCH", "PTCH", expo_(50.0, 400.0), 50, FHz),
            k("DECAY", "DEC", expo_(0.08, 2.0), 70, Time),
            k("SWEEP", "SWP", lin_(0.0, 24.0), 30, FSemis),
            k("SWEEP TIME", "SWPT", expo_(0.004, 0.3), 40, Time),
            k("NOISE", "NOIS", lin_(0.0, 0.6), 20, Amount),
            k("DAMP", "DAMP", AMOUNT, 20, Amount),
            k("DRIVE", "DRIV", AMOUNT, 0, Amount),
            k("DRIVE TYPE", "DTYP", AMOUNT, 0, Amount),
        ],
    },
    MachineSpec {
        name: "TONE CLAP",
        short: "CLAP",
        knobs: [
            k("TONE", "TONE", expo_(500.0, 4000.0), 60, FHz),
            k("DECAY", "DEC", expo_(0.05, 1.0), 50, Time),
            k("CLAPS", "CLPS", Law::Steps(&CLAPS), 54, Count),
            k("SPREAD", "SPRD", expo_(0.004, 0.03), 50, Time),
            k("TAIL", "TAIL", AMOUNT, 90, Amount),
            k("RING", "RING", lin_(0.0, 0.9), 30, Amount),
            // Each clap's fall as a share of SPREAD, harder shorter: the
            // prototype's `expo(1 − k, 0.15, 0.8)` read from the other end.
            k("HARDNESS", "HARD", expo_(0.8, 0.15), 64, FRatio),
            k("ROOM TONE", "ROOM", expo_(400.0, 24_000.0), 127, FHz),
        ],
    },
    MachineSpec {
        name: "TONE COWBELL",
        short: "BELL",
        knobs: [
            k("PITCH", "PTCH", expo_(300.0, 1200.0), 55, FHz),
            k("DECAY", "DEC", expo_(0.08, 1.5), 50, Time),
            k("INTERVAL", "INTV", lin_(1.2, 1.8), 60, FRatio),
            k("TONE", "TONE", expo_(600.0, 6000.0), 70, FHz),
            k("RING", "RING", lin_(0.0, 0.85), 40, Amount),
            k("DRIVE", "DRIV", AMOUNT, 30, Amount),
            k("DAMP", "DAMP", AMOUNT, 76, Amount),
            // Up to an octave above, falling in 6 ms.
            k("BUMP", "BUMP", AMOUNT, 0, Octaves),
        ],
    },
    MachineSpec {
        name: "TONE CYMBAL",
        short: "CYMB",
        knobs: [
            k("PITCH", "PTCH", lin_(-24.0, 24.0), 64, FSemis),
            k("DECAY", "DEC", expo_(0.2, 5.0), 80, Time),
            k("TONE", "TONE", expo_(2500.0, 12_000.0), 70, FHz),
            k("SPREAD", "SPRD", AMOUNT, 30, Amount),
            k("RING", "RING", lin_(0.0, 0.9), 30, Amount),
            k("SHIMMER", "SHIM", AMOUNT, 40, Amount),
            k("TOP", "TOP", lin_(0.0, 0.5), 76, Amount),
            k("TOP TUNE", "TTUN", lin_(1.1, 1.6), 64, FRatio),
        ],
    },
    MachineSpec {
        name: "METAL HAT",
        short: "HAT",
        knobs: [
            k("PITCH", "PTCH", lin_(-24.0, 24.0), 64, FSemis),
            k("DECAY", "DEC", expo_(0.02, 1.5), 20, Time),
            k("TONE", "TONE", expo_(3000.0, 14_000.0), 80, FHz),
            k("SPREAD", "SPRD", AMOUNT, 0, Amount),
            k("RING", "RING", lin_(0.0, 0.9), 30, Amount),
            k("NOISE", "NOIS", lin_(0.0, 0.5), 15, Amount),
            // A share of TONE.
            k("HIGH PASS", "HP", expo_(0.25, 1.0), 64, FRatio),
            // Off at the top.
            k("LOW PASS", "LP", expo_(2000.0, 20_000.0), 127, FHz),
        ],
    },
    MachineSpec {
        name: "METAL JINGLE",
        short: "JNGL",
        knobs: [
            // The bank's pitch over the hat's six an octave up: the
            // prototype's `lin(lin(k, 0.25, 1), −24, 24)`, −12..=+24
            // semitones. HEAD's pitch rides the same knob on its own law.
            k("PITCH", "PTCH", lin_(-12.0, 24.0), 64, FSemis),
            k("DECAY", "DEC", expo_(0.05, 1.0), 50, Time),
            k("JINGLES", "JNGL", expo_(300.0, 6000.0), 70, FHz),
            k("SHAKE", "SHAK", lin_(0.0, 0.6), 30, Amount),
            k("TONE", "TONE", expo_(5000.0, 14_000.0), 60, FHz),
            // Held just under 0.95, CLEAN's self-oscillation onset, where
            // the prototype's jingles ring on for good: 127 alone moves.
            k(
                "RING",
                "RING",
                Law::Held {
                    lo: 0.5,
                    hi: 0.95,
                    top: JINGLE_RING_TOP.get(),
                },
                50,
                Amount,
            ),
            k("HEAD", "HEAD", lin_(0.0, 0.7), 0, Amount),
            k("SPREAD", "SPRD", AMOUNT, 40, Amount),
        ],
    },
    MachineSpec {
        name: "BODY SHAKER",
        short: "SHKR",
        knobs: [
            k("GRAINS", "GRNS", expo_(100.0, 3000.0), 80, FHz),
            k("DECAY", "DEC", expo_(0.03, 0.5), 50, Time),
            k("GRAIN LENGTH", "GLEN", expo_(0.0002, 0.003), 40, Time),
            k("TONE", "TONE", expo_(2000.0, 12_000.0), 80, FHz),
            k("SHAKE", "SHAK", lin_(0.05, 0.7), 30, Amount),
            k("RING", "RING", lin_(0.0, 0.9), 30, Amount),
            k("HARDNESS", "HARD", AMOUNT, 64, Amount),
            None,
        ],
    },
    MachineSpec {
        name: "PLAIN SINE",
        short: "SINE",
        knobs: [
            k("PITCH", "PTCH", expo_(30.0, 2000.0), 20, FHz),
            k("DECAY", "DEC", expo_(0.02, 3.0), 70, Time),
            k("SWEEP", "SWP", lin_(0.0, 48.0), 30, FSemis),
            k("SWEEP TIME", "SWPT", expo_(0.002, 0.2), 30, Time),
            None,
            None,
            None,
            None,
        ],
    },
    MachineSpec {
        name: "PLAIN NOISE",
        short: "NOIS",
        knobs: [
            k("DECAY", "DEC", expo_(0.01, 2.0), 30, Time),
            k("TONE", "TONE", AMOUNT, 100, Tilt),
            None,
            None,
            None,
            None,
            None,
            None,
        ],
    },
    MachineSpec {
        name: "PLAIN IMPULSE",
        short: "IMPL",
        knobs: [
            k("PITCH", "PTCH", expo_(30.0, 2000.0), 40, FHz),
            k("DECAY", "DEC", expo_(0.02, 3.0), 30, Time),
            k("SWEEP", "SWP", lin_(0.0, 48.0), 0, FSemis),
            k("SWEEP TIME", "SWPT", expo_(0.002, 0.2), 0, Time),
            None,
            None,
            k("WIDTH", "WDTH", AMOUNT, 117, Amount),
            None,
        ],
    },
];

/// The machine's names, knobs, laws and defaults.
pub const fn spec(m: Machine) -> &'static MachineSpec {
    &SPECS[m as usize]
}

impl KnobSet {
    /// The machine's defaults; unused knobs 0.
    pub const fn defaults(m: Machine) -> KnobSet {
        let mut s = KnobSet::ZERO;
        let mut i = 0;
        while i < Knob::ALL.len() {
            if let Some(ks) = spec(m).knobs[i] {
                s.set(Knob::ALL[i], ks.default);
            }
            i += 1;
        }
        s
    }
}

// ------------------------------------------------------------- params

/// A swept sine's pitch: from `pitch` × 2^(`depth`/12) down to `pitch`,
/// falling to 1/e in `time`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep {
    pub pitch: Hz,
    pub depth: Semis,
    pub time: Seconds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KickParams {
    pub body: Sweep,
    pub decay: Seconds,
    /// The click's level.
    pub click: Gain,
    pub drive: Unit,
    /// Up to a quarter cycle into the body.
    pub start: Unit,
    pub harmonics: Unit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnareParams {
    pub pitch: Hz,
    pub decay: Seconds,
    /// Both heads' sweep (fixed 8 ms tau).
    pub bump: Semis,
    pub snap: Gain,
    pub snap_decay: Seconds,
    /// The snap's band-pass.
    pub tone: Hz,
    /// The upper head over the lower.
    pub interval: Ratio,
    pub drive: Unit,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TomParams {
    pub body: Sweep,
    pub decay: Seconds,
    pub noise: Gain,
    /// The noise's low-pass: four times the pitch, at most 12 kHz.
    pub noise_hz: Hz,
    pub damp: Unit,
    pub drive: Unit,
    /// SOFT (0) leaning into FOLD (1).
    pub drive_type: Unit,
}

/// The tone kernel's machines: a swept sine body, a noise layer, drive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToneParams {
    Kick(KickParams),
    Snare(SnareParams),
    Tom(TomParams),
}

/// One of a metal bank's band-passes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub hz: Hz,
    pub res: Unit,
    pub weight: Gain,
}

/// HAT's and CYMBAL's: six squares through band-passes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BankParams {
    pub hz: [Hz; 6],
    pub bands: [Option<Band>; 3],
    pub hp: Hz,
    pub lp: Option<Hz>,
    pub decay: Seconds,
    /// The fast decay's T60 (a fifth of the amp).
    pub fast: Seconds,
    pub noise: Gain,
    pub shimmer: Unit,
    pub gain: Gain,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CowbellParams {
    pub pitch: Hz,
    pub interval: Ratio,
    pub tone: Hz,
    pub ring: Unit,
    pub decay: Seconds,
    pub drive: Unit,
    pub damp: Unit,
    /// Octaves above, 0..=1, falling in 6 ms.
    pub bump: Unit,
}

/// METAL JINGLE: the bank's squares, rung higher, gated by the burst's
/// particles; HEAD a frame drum's thump.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JingleParams {
    pub hz: [Hz; 6],
    pub tone: Hz,
    /// The second band-pass, 1.42 × TONE at most 19 kHz.
    pub top: Hz,
    pub res: Unit,
    /// Particles a second at the shake's peak.
    pub jingles: PerSecond,
    pub length: Seconds,
    /// The peak's place in the shake.
    pub shake: Unit,
    /// Each particle's T60.
    pub particle: Seconds,
    pub head_pitch: Hz,
    pub head: Gain,
}

/// The metal kernel's machines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MetalParams {
    Cowbell(CowbellParams),
    /// HAT and CYMBAL.
    Bank(BankParams),
    Jingle(JingleParams),
}

/// How many claps, 1..=8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClapCount(u8);

impl ClapCount {
    pub const MAX: ClapCount = ClapCount(8);

    pub const fn new(n: u8) -> Option<ClapCount> {
        if n >= 1 && n <= 8 {
            Some(ClapCount(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// CLAPS: the nearest of 1..=8 over the knob's travel, as the
    /// prototype's `1 + (k · 7).round()`. Its steps fall at 10, 28, 46, 64,
    /// 82, 100 and 118 of 127, which is `(v + 8) / 18`.
    pub const fn from_knob(k: KnobValue) -> ClapCount {
        ClapCount(1 + (k.get().get() + 8) / 18)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClapParams {
    pub tone: Hz,
    pub ring: Unit,
    pub claps: ClapCount,
    pub spread: Seconds,
    /// Each clap's fall, a share of SPREAD.
    pub burst: Ratio,
    pub decay: Seconds,
    pub tail: Unit,
    /// The tail's low-pass.
    pub room: Hz,
}

/// SHAKER's HARDNESS: below the middle a low-pass on the grains (its
/// one-pole coefficient), above dry grains over the band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hardness {
    Dark(Unit),
    Dry(Unit),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShakerParams {
    /// Grains a second at the shake's peak.
    pub grains: PerSecond,
    pub length: Seconds,
    /// Each grain's T60.
    pub grain: Seconds,
    pub tone: Hz,
    /// The peak's place in the shake.
    pub shake: Unit,
    pub ring: Unit,
    pub hardness: Hardness,
}

/// The burst kernel's machines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BurstParams {
    Clap(ClapParams),
    Shaker(ShakerParams),
}

/// PLAIN NOISE's TONE.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoiseTone {
    LowPass(Hz),
    HighPass(Hz),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoiseParams {
    pub decay: Seconds,
    pub tone: NoiseTone,
}

/// The plain kernel's machines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlainParams {
    Sine {
        body: Sweep,
        decay: Seconds,
    },
    Noise(NoiseParams),
    Impulse {
        body: Sweep,
        decay: Seconds,
        width: Unit,
    },
}

/// A pad's kernel and its params.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KernelParams {
    Tone(ToneParams),
    Metal(MetalParams),
    Burst(BurstParams),
    Plain(PlainParams),
}

impl KernelParams {
    pub const fn kernel(&self) -> Kernel {
        match self {
            KernelParams::Tone(_) => Kernel::Tone,
            KernelParams::Metal(_) => Kernel::Metal,
            KernelParams::Burst(_) => Kernel::Burst,
            KernelParams::Plain(_) => Kernel::Plain,
        }
    }
}

impl KernelParams {
    /// Played `by` semitones from the pad's tuned pitch (TONAL's note, ADR
    /// 0270): whatever PITCH sets moves, the rest stays. Machines without a PITCH
    /// knob (CLAP, SHAKER, NOISE) play as tuned.
    pub fn transposed(self, by: Transpose) -> KernelParams {
        if by == Transpose::ZERO {
            return self;
        }
        let r = libm::exp2f(f32::from(by.get()) / 12.0);
        let up = |h: Hz| Hz::new(h.get() * r);
        let sweep = |s: Sweep| Sweep {
            pitch: up(s.pitch),
            ..s
        };
        match self {
            KernelParams::Tone(ToneParams::Kick(p)) => {
                KernelParams::Tone(ToneParams::Kick(KickParams {
                    body: sweep(p.body),
                    ..p
                }))
            }
            KernelParams::Tone(ToneParams::Snare(p)) => {
                KernelParams::Tone(ToneParams::Snare(SnareParams {
                    pitch: up(p.pitch),
                    ..p
                }))
            }
            KernelParams::Tone(ToneParams::Tom(p)) => {
                let body = sweep(p.body);
                KernelParams::Tone(ToneParams::Tom(TomParams {
                    body,
                    noise_hz: tom_noise(body.pitch),
                    ..p
                }))
            }
            KernelParams::Metal(MetalParams::Cowbell(p)) => {
                KernelParams::Metal(MetalParams::Cowbell(CowbellParams {
                    pitch: up(p.pitch),
                    ..p
                }))
            }
            KernelParams::Metal(MetalParams::Bank(p)) => {
                KernelParams::Metal(MetalParams::Bank(BankParams {
                    hz: p.hz.map(up),
                    ..p
                }))
            }
            KernelParams::Metal(MetalParams::Jingle(p)) => {
                KernelParams::Metal(MetalParams::Jingle(JingleParams {
                    hz: p.hz.map(up),
                    head_pitch: up(p.head_pitch),
                    ..p
                }))
            }
            KernelParams::Plain(PlainParams::Sine { body, decay }) => {
                KernelParams::Plain(PlainParams::Sine {
                    body: sweep(body),
                    decay,
                })
            }
            KernelParams::Plain(PlainParams::Impulse { body, decay, width }) => {
                KernelParams::Plain(PlainParams::Impulse {
                    body: sweep(body),
                    decay,
                    width,
                })
            }
            KernelParams::Burst(_) | KernelParams::Plain(PlainParams::Noise(_)) => self,
        }
    }
}

// ------------------------------------------------------------- resolve

/// A machine's knobs as its laws read them.
struct Read<'a> {
    #[cfg(test)]
    m: &'static MachineSpec,
    k: &'a KnobSet,
    /// Every knob through its law, in one loop (0 where the machine has
    /// none): one call to `Law::at` for every machine's reads.
    v: [f32; 8],
    /// Tests: the knobs read that the machine lacks, a bit each.
    #[cfg(test)]
    missing: core::cell::Cell<u8>,
}

impl<'a> Read<'a> {
    fn new(machine: Machine, k: &'a KnobSet) -> Read<'a> {
        let m = spec(machine);
        let mut v = [0.0; 8];
        for (v, kn) in v.iter_mut().zip(Knob::ALL) {
            *v = m.knob(kn).map_or(0.0, |s| s.law.at(k.get(kn)));
        }
        Read {
            #[cfg(test)]
            m,
            k,
            v,
            #[cfg(test)]
            missing: core::cell::Cell::new(0),
        }
    }

    /// The knob through its law (0 where the machine has none; the tests
    /// below hold that no resolve reads one).
    fn at(&self, kn: Knob) -> f32 {
        self.check(kn);
        self.v[kn as usize]
    }

    /// The knob's raw 0..1, for a nested second law on it.
    fn u(&self, kn: Knob) -> f32 {
        self.check(kn);
        self.k.get(kn).unit().get()
    }

    #[cfg(test)]
    fn check(&self, kn: Knob) {
        if self.m.knob(kn).is_none() {
            self.missing.set(self.missing.get() | 1 << kn as u8);
        }
    }

    #[cfg(not(test))]
    fn check(&self, _: Knob) {}
}

/// The pad's knobs as its machine's kernel params.
pub fn resolve(machine: Machine, knobs: &KnobSet) -> KernelParams {
    read(machine, &Read::new(machine, knobs))
}

fn read(machine: Machine, r: &Read) -> KernelParams {
    match machine {
        Machine::ToneKick => KernelParams::Tone(ToneParams::Kick(kick(r))),
        Machine::ToneSnare => KernelParams::Tone(ToneParams::Snare(snare(r))),
        Machine::ToneTom => KernelParams::Tone(ToneParams::Tom(tom(r))),
        Machine::ToneClap => KernelParams::Burst(BurstParams::Clap(clap(r))),
        Machine::ToneCowbell => KernelParams::Metal(MetalParams::Cowbell(cowbell(r))),
        Machine::ToneCymbal => KernelParams::Metal(MetalParams::Bank(cymbal(r))),
        Machine::MetalHat => KernelParams::Metal(MetalParams::Bank(hat(r))),
        Machine::MetalJingle => KernelParams::Metal(MetalParams::Jingle(jingle(r))),
        Machine::BodyShaker => KernelParams::Burst(BurstParams::Shaker(shaker(r))),
        Machine::PlainSine => KernelParams::Plain(PlainParams::Sine {
            body: sweep(r),
            decay: Seconds::new(r.at(Knob::B)),
        }),
        Machine::PlainNoise => KernelParams::Plain(PlainParams::Noise(noise(r))),
        Machine::PlainImpulse => KernelParams::Plain(PlainParams::Impulse {
            body: sweep(r),
            decay: Seconds::new(r.at(Knob::B)),
            width: Unit::new(r.at(Knob::G)),
        }),
    }
}

/// PITCH, SWEEP and SWEEP TIME on A, C and D.
fn sweep(r: &Read) -> Sweep {
    Sweep {
        pitch: Hz::new(r.at(Knob::A)),
        depth: Semis::new(r.at(Knob::C)),
        time: Seconds::new(r.at(Knob::D)),
    }
}

/// `ToneKick::new`, machines.rs:704.
fn kick(r: &Read) -> KickParams {
    KickParams {
        body: sweep(r),
        decay: Seconds::new(r.at(Knob::B)),
        click: Gain::new(r.at(Knob::E)),
        drive: Unit::new(r.at(Knob::F)),
        start: Unit::new(r.at(Knob::G)),
        harmonics: Unit::new(r.at(Knob::H)),
    }
}

/// `ToneSnare::new`, machines.rs:755.
fn snare(r: &Read) -> SnareParams {
    SnareParams {
        pitch: Hz::new(r.at(Knob::A)),
        decay: Seconds::new(r.at(Knob::B)),
        bump: Semis::new(r.at(Knob::C)),
        snap: Gain::new(r.at(Knob::D)),
        snap_decay: Seconds::new(r.at(Knob::E)),
        tone: Hz::new(r.at(Knob::F)),
        interval: Ratio::new(r.at(Knob::G)),
        drive: Unit::new(r.at(Knob::H)),
    }
}

/// `ToneTom::new`, machines.rs:809.
fn tom(r: &Read) -> TomParams {
    let body = sweep(r);
    TomParams {
        body,
        decay: Seconds::new(r.at(Knob::B)),
        noise: Gain::new(r.at(Knob::E)),
        noise_hz: tom_noise(body.pitch),
        damp: Unit::new(r.at(Knob::F)),
        drive: Unit::new(r.at(Knob::G)),
        drive_type: Unit::new(r.at(Knob::H)),
    }
}

/// TOM's noise low-pass: four times the pitch, at most 12 kHz.
fn tom_noise(pitch: Hz) -> Hz {
    Hz::new((4.0 * pitch.get()).min(12_000.0))
}

/// `Burst::clap`, machines.rs:1236.
fn clap(r: &Read) -> ClapParams {
    ClapParams {
        tone: Hz::new(r.at(Knob::A)),
        ring: Unit::new(r.at(Knob::F)),
        claps: ClapCount::from_knob(r.k.get(Knob::C)),
        spread: Seconds::new(r.at(Knob::D)),
        burst: Ratio::new(r.at(Knob::G)),
        decay: Seconds::new(r.at(Knob::B)),
        tail: Unit::new(r.at(Knob::E)),
        room: Hz::new(r.at(Knob::H)),
    }
}

/// `Burst::shaker`, machines.rs:1276.
fn shaker(r: &Read) -> ShakerParams {
    let hard = r.at(Knob::G);
    ShakerParams {
        grains: PerSecond::new(r.at(Knob::A)),
        length: Seconds::new(r.at(Knob::B)),
        grain: Seconds::new(r.at(Knob::C)),
        tone: Hz::new(r.at(Knob::D)),
        shake: Unit::new(r.at(Knob::E)),
        ring: Unit::new(r.at(Knob::F)),
        // Burst::block, machines.rs:1336.
        hardness: if hard >= 0.5 {
            Hardness::Dry(Unit::new(2.0 * (hard - 0.5)))
        } else {
            Hardness::Dark(Unit::new(expo(2.0 * hard, 0.05, 1.0)))
        },
    }
}

/// `Cowbell::new`, machines.rs:1067.
fn cowbell(r: &Read) -> CowbellParams {
    CowbellParams {
        pitch: Hz::new(r.at(Knob::A)),
        interval: Ratio::new(r.at(Knob::C)),
        tone: Hz::new(r.at(Knob::D)),
        ring: Unit::new(r.at(Knob::E)),
        decay: Seconds::new(r.at(Knob::B)),
        drive: Unit::new(r.at(Knob::F)),
        damp: Unit::new(r.at(Knob::G)),
        bump: Unit::new(r.at(Knob::H)),
    }
}

/// The 808-style six square frequencies (the circuit's published values),
/// and SPREAD's own detune away from them; machines.rs:917.
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];
const SPREAD: [f32; 6] = [0.0, 0.31, -0.17, 0.47, 0.83, -0.29];

/// `Metal::bank` at a pitch in semitones: `lin(pitch, −24, 24)` is the
/// caller's.
fn bank(semis: f32, spread: f32, scale: f32) -> [Hz; 6] {
    let r = libm::exp2f(semis / 12.0);
    core::array::from_fn(|i| Hz::new(scale * (METAL[i] * r * (1.0 + spread * SPREAD[i]))))
}

/// `Metal::hat`, machines.rs:943.
fn hat(r: &Read) -> BankParams {
    let tone = r.at(Knob::C);
    let res = Unit::new(r.at(Knob::E));
    let hp = tone * r.at(Knob::G);
    BankParams {
        hz: bank(r.at(Knob::A), r.u(Knob::D), 1.0),
        bands: [
            Some(Band {
                hz: Hz::new(tone),
                res,
                weight: Gain::new(0.6),
            }),
            Some(Band {
                hz: Hz::new(tone * 0.68),
                res,
                weight: Gain::new(0.4),
            }),
            None,
        ],
        hp: Hz::new(hp),
        lp: (r.u(Knob::H) < 1.0).then(|| Hz::new(r.at(Knob::H))),
        decay: Seconds::new(r.at(Knob::B)),
        fast: Seconds::new(0.01),
        noise: Gain::new(r.at(Knob::F)),
        shimmer: Unit::ZERO,
        gain: Gain::new(5.5 * (1.0 - 0.7 * r.u(Knob::E))),
    }
}

/// `Metal::cymbal`, machines.rs:977.
fn cymbal(r: &Read) -> BankParams {
    let tone = r.at(Knob::C);
    let res = Unit::new(r.at(Knob::E));
    BankParams {
        hz: bank(r.at(Knob::A), r.u(Knob::D), 1.0),
        bands: [
            Some(Band {
                hz: Hz::new(tone),
                res,
                weight: Gain::new(0.45),
            }),
            Some(Band {
                hz: Hz::new(tone * 0.6),
                res,
                weight: Gain::new(0.35),
            }),
            Some(Band {
                hz: Hz::new((tone * r.at(Knob::H)).min(18_000.0)),
                res,
                weight: Gain::new(r.at(Knob::G)),
            }),
        ],
        hp: Hz::new(tone * 0.35),
        lp: None,
        decay: Seconds::new(r.at(Knob::B)),
        fast: Seconds::new(0.03),
        noise: Gain::new(0.15),
        shimmer: Unit::new(r.at(Knob::F)),
        gain: Gain::new(2.8 * (1.0 - 0.5 * r.u(Knob::E))),
    }
}

/// The jingles' RING at most: CLEAN's `k` about 0.01, a ring of a few ms.
const JINGLE_RING_TOP: Unit = Unit::new(0.945);

/// `Jingle::new`, machines.rs:1127.
fn jingle(r: &Read) -> JingleParams {
    let tone = r.at(Knob::E);
    JingleParams {
        hz: bank(r.at(Knob::A), r.u(Knob::H), 2.0),
        tone: Hz::new(tone),
        top: Hz::new((tone * 1.42).min(19_000.0)),
        res: Unit::new(r.at(Knob::F)),
        jingles: PerSecond::new(r.at(Knob::C)),
        length: Seconds::new(r.at(Knob::B)),
        shake: Unit::new(r.at(Knob::D)),
        particle: Seconds::new(expo(r.u(Knob::F), 0.02, 0.12)),
        // PITCH's second reading: the head's own law on the same knob.
        head_pitch: Hz::new(expo(r.u(Knob::A), 120.0, 300.0)),
        head: Gain::new(r.at(Knob::G)),
    }
}

/// `Plain::noise`, machines.rs:2220.
fn noise(r: &Read) -> NoiseParams {
    let t = r.at(Knob::B);
    NoiseParams {
        decay: Seconds::new(r.at(Knob::A)),
        tone: if t < 0.5 {
            NoiseTone::LowPass(Hz::new(expo(t * 2.0, 200.0, 20_000.0)))
        } else {
            NoiseTone::HighPass(Hz::new(expo((t - 0.5) * 2.0, 20.0, 10_000.0)))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every knob a machine's resolve reads is one its spec has.
    #[test]
    fn resolves_read_only_their_knobs() {
        for m in Machine::ALL {
            let k = KnobSet::defaults(m);
            let r = Read::new(m, &k);
            read(m, &r);
            assert_eq!(r.missing.get(), 0, "{m:?} reads a knob it lacks");
        }
    }
}
