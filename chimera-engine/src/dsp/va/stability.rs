//! STABILITY (spec § Stability): per-voice drift walks, trim, HF tracking,
//! scale error and per-cycle jitter, all scaled by `d = (1 − STAB)²`. At
//! `d == 0` every output is exactly neutral, so STAB 1 is bit-identical;
//! below it the character recalls from the (Part, slot) seed.

use crate::dsp::clampf;
use crate::dsp::provenance::{PatentLine, Provenance};

pub const PROVENANCE: &[Provenance] = &[
    Provenance::Original {
        author: "AI-assisted, owner-directed and reviewed",
    },
    Provenance::CircuitModel {
        circuit: "VCO core reset time; exponential-converter scale",
        refs: "CEM3340 datasheet; SSI2130 datasheet",
        check: "C7 at τ 4 µs = −14.4 cents",
    },
];

pub const PATENT: &[PatentLine] = &[];

pub const GPL_CHECK: &str = "clean room; no GPL source read";

/// Cents at `d = 1`: the shared walk, each oscillator's own, the trim
/// and the per-cycle jitter.
pub const DRIFT_SHARED_CENTS: f32 = 12.0;
pub const DRIFT_OWN_CENTS: f32 = 6.0;
pub const TRIM_CENTS: f32 = 3.0;
pub const JITTER_CENTS: f32 = 0.5;
/// The core's reset time at `d = 1`, u = 1: an estimate from the CEM3340
/// datasheet, not a measurement.
pub const TAU_MAX_S: f32 = 4.0e-6;
/// u's low end: a slightly over-compensated (sharp) VCO.
pub const TAU_U_LO: f32 = -0.25;
/// V/oct error at `d = 1`, about C4.
pub const SCALE_ERR: f32 = 0.002;
/// The walks' corner and RMS.
pub const WALK_HZ: f32 = 0.2;
pub const WALK_RMS: f32 = 0.30;

pub const SALT_TRIM: [u32; 2] = [0x7121, 0x7122];
pub const SALT_TAU: [u32; 2] = [0x7A01, 0x7A02];
pub const SALT_SCALE: [u32; 2] = [0xE501, 0xE502];
/// Shared, OSC 1, OSC 2.
pub const SALT_WALK: [u32; 3] = [0x3A1C, 0x3A1D, 0x3A1E];
pub const SALT_JITTER: u32 = 0x1177;

/// C4 in the pitch conversion's cents (MIDI note × 100).
const C4_CENTS: f32 = 6000.0;

/// The depth curve: fine-grained at the top of the knob; NaN is 0 (d 1).
pub fn depth(stability: f32) -> f32 {
    let x = 1.0 - clampf(stability, 0.0, 1.0);
    x * x
}

/// splitmix32's finaliser on a golden step: every input bit reaches
/// every output bit, so neighbouring (Part, slot)s draw apart.
pub const fn mix32(z: u32) -> u32 {
    let z = z.wrapping_add(0x9E37_79B9);
    let z = (z ^ z >> 16).wrapping_mul(0x85EB_CA6B);
    let z = (z ^ z >> 13).wrapping_mul(0xC2B2_AE35);
    z ^ z >> 16
}

/// A word as −1..1.
pub fn uni(x: u32) -> f32 {
    (x as i32) as f32 / 2_147_483_648.0
}

/// A voice's identity: (Part, voice slot), hashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seed(u32);

impl Seed {
    pub const fn of(part: u32, slot: u32) -> Seed {
        debug_assert!(slot < 256);
        Seed(mix32(part << 8 | slot))
    }

    /// One fixed draw in −1..1 for a purpose.
    pub fn draw(self, salt: u32) -> f32 {
        uni(mix32(self.0 ^ salt))
    }

    /// A xorshift32 state for a purpose (never 0).
    pub fn stream(self, salt: u32) -> u32 {
        match mix32(self.0 ^ salt) {
            0 => 0x6D2B_79F5,
            s => s,
        }
    }
}

/// What recalls from a seed, per oscillator: trim (−1..1), τ factor u
/// (−0.25..1) and scale error (−1..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Character {
    pub trim: [f32; 2],
    pub tau_u: [f32; 2],
    pub scale: [f32; 2],
}

impl Character {
    pub fn of(s: Seed) -> Character {
        let span = 1.0 - TAU_U_LO;
        Character {
            trim: SALT_TRIM.map(|k| s.draw(k)),
            tau_u: SALT_TAU.map(|k| TAU_U_LO + 0.5 * span * (s.draw(k) + 1.0)),
            scale: SALT_SCALE.map(|k| s.draw(k)),
        }
    }
}

/// A walk's one-pole coefficient at `corner_hz`, stepped at `update_hz`,
/// and the gain to `rms`: from the cascade's closed-form variance for
/// unit-variance-⅓ noise, so the RMS and corner don't depend on the rate.
pub fn walk_coefs(update_hz: f32, corner_hz: f32, rms: f32) -> (f32, f32) {
    let a = -libm::expm1f(-2.0 * core::f32::consts::PI * corner_hz / update_hz);
    let b = 1.0 - a;
    // a(2 − a) is 1 − b² without the cancellation
    let var = (1.0 / 3.0) * a * a * a * a * (1.0 + b * b) / libm::powf(a * (2.0 - a), 3.0);
    (a, rms / libm::sqrtf(var))
}

pub(crate) fn xorshift(state: &mut u32) -> f32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    uni(x)
}

/// Walks: shared, OSC 1, OSC 2. Streams: the three walks, then jitter.
#[derive(Clone, Copy, Debug)]
pub struct Stability {
    character: Character,
    rng: [u32; 4],
    y1: [f32; 3],
    y2: [f32; 3],
    /// The one-poles' coefficient, and the gain to `WALK_RMS`; 0 (silent
    /// walks) until `prepare`.
    a: f32,
    g: f32,
}

impl Stability {
    pub const NEW: Stability = Stability {
        character: Character {
            trim: [0.0; 2],
            tau_u: [0.0; 2],
            scale: [0.0; 2],
        },
        rng: [0x6D2B_79F5; 4],
        y1: [0.0; 3],
        y2: [0.0; 3],
        a: 0.0,
        g: 0.0,
    };

    /// `update_hz` is the rate `step` runs at (the block rate).
    pub fn prepare(&mut self, update_hz: f32) {
        (self.a, self.g) = walk_coefs(update_hz, WALK_HZ, WALK_RMS);
    }

    /// The character, and the streams restart from the seed; the walks'
    /// filter states run on.
    pub fn seed(&mut self, s: Seed) {
        self.character = Character::of(s);
        self.rng = [
            s.stream(SALT_WALK[0]),
            s.stream(SALT_WALK[1]),
            s.stream(SALT_WALK[2]),
            s.stream(SALT_JITTER),
        ];
    }

    pub fn character(&self) -> Character {
        self.character
    }

    /// Once a block: each oscillator's drift + trim in cents. The walks
    /// advance at any `d`; `d == 0` returns exactly 0.
    pub fn step(&mut self, d: f32) -> [f32; 2] {
        let mut w = [0.0f32; 3];
        for (k, w) in w.iter_mut().enumerate() {
            let x = xorshift(&mut self.rng[k]);
            self.y1[k] += self.a * (x - self.y1[k]);
            self.y2[k] += self.a * (self.y1[k] - self.y2[k]);
            *w = clampf(self.g * self.y2[k], -1.0, 1.0);
        }
        if d == 0.0 {
            return [0.0; 2];
        }
        let shared = DRIFT_SHARED_CENTS * w[0];
        let t = self.character.trim;
        [
            d * (shared + DRIFT_OWN_CENTS * w[1] + TRIM_CENTS * t[0]),
            d * (shared + DRIFT_OWN_CENTS * w[2] + TRIM_CENTS * t[1]),
        ]
    }

    /// Walk `k` (0 shared, 1 OSC 1, 2 OSC 2) before its clamp.
    pub fn walk_raw(&self, k: usize) -> f32 {
        self.g * self.y2[k]
    }

    /// HF tracking: `f/(1 + f·τ)`, τ = d·τ_max·u, the divisor floored at
    /// ½. `d == 0` returns `hz` untouched.
    pub fn tracked(&self, hz: [f32; 2], d: f32) -> [f32; 2] {
        if d == 0.0 {
            return hz;
        }
        let u = self.character.tau_u;
        [0, 1].map(|k| hz[k] / (1.0 + hz[k] * d * TAU_MAX_S * u[k]).max(0.5))
    }

    /// Scale error: the extra cents for pitches `cents` (C4 = 6000).
    pub fn scale_cents(&self, cents: [f32; 2], d: f32) -> [f32; 2] {
        if d == 0.0 {
            return [0.0; 2];
        }
        let s = self.character.scale;
        [0, 1].map(|k| (cents[k] - C4_CENTS) * SCALE_ERR * s[k] * d)
    }

    /// The cents for the cycle a wrap starts.
    pub fn jitter(&mut self, d: f32) -> f32 {
        let x = uni(self.jitter_word() as u32);
        if d == 0.0 { 0.0 } else { JITTER_CENTS * d * x }
    }

    /// `jitter`'s draw as a raw word: `jitter(d)` is `JITTER_CENTS · d ·
    /// uni(word)` (0 at `d == 0`), the scale set once a block.
    #[inline(always)]
    pub fn jitter_word(&mut self) -> i32 {
        let mut x = self.rng[3];
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng[3] = x;
        x as i32
    }
}
