//! The operator envelope, stepped per sample in `f32`. Every four steps of
//! effective rate doubles the speed; the scale puts AR 31 at about 12 samples.

use crate::MidiNote;
use crate::dsp::algo::math::{exp2, log2};
use crate::dsp::algo::tx::d1l_level;

const ATTACK_SECONDS: f32 = 11.6;
/// Time a decay takes to fall 96 dB (16 octaves) at effective rate 0.
const DECAY_SECONDS: f32 = 92.8;
pub const DECAY_OCTAVES: f32 = 16.0;
/// `log2(DECAY_SECONDS / ATTACK_SECONDS)`: at one rate, a full decay takes
/// eight full attacks.
pub const DECAY_OVER_ATTACK_OCTAVES: f32 = 3.0;
const _: () = assert!(DECAY_SECONDS == ATTACK_SECONDS * 8.0);
/// -80 dB: a decay or release below this is over.
pub const ENV_FLOOR: f32 = 1.0e-4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvRates {
    pub ar: u8,
    pub d1r: u8,
    pub d1l: u8,
    pub d2r: u8,
    pub rr: u8,
    pub rs: u8,
}

/// Key code 0-31 (four per octave from A0), shifted by rate scaling 0-3.
pub fn key_scale(rs: u8, note: MidiNote) -> u8 {
    let code = (note.get().saturating_sub(21) / 3).min(31);
    code >> (3 - rs.min(3))
}

/// Effective rate 0-63 of a 5-bit rate; rate 0 stays 0 (off or hold).
pub fn effective(rate: u8, ks: u8) -> u8 {
    if rate == 0 {
        0
    } else {
        (2 * rate.min(31) + ks).min(63)
    }
}

pub fn effective_release(rr: u8, ks: u8) -> u8 {
    (4 * rr.clamp(1, 15) + 2 + ks).min(63)
}

/// Seconds a full attack (0 to 1) takes at effective rate `r`.
pub const fn attack_seconds(r: u8) -> f32 {
    ATTACK_SECONDS * exp2(-(r as f32) / 4.0)
}

/// Seconds a decay at effective rate `r` takes to fall `DECAY_OCTAVES`.
pub const fn decay_seconds(r: u8) -> f32 {
    DECAY_SECONDS * exp2(-(r as f32) / 4.0)
}

pub const fn attack_add(r: u8, sample_rate: f32) -> f32 {
    if r == 0 {
        return 0.0;
    }
    1.0 / (attack_seconds(r) * sample_rate)
}

/// `log2` of the per-sample decay factor (0: holds). Kept as a log so a
/// stage's length is one division, exact even for the slowest rates.
pub const fn decay_log2(r: u8, sample_rate: f32) -> f32 {
    if r == 0 {
        return 0.0;
    }
    -DECAY_OCTAVES / (decay_seconds(r) * sample_rate)
}

/// Effective rates, 0–63.
const RATES: usize = 64;

/// `attack_add` and `decay_log2` of every effective rate at the build's
/// `SAMPLE_RATE`, computed at compile time by the same `const fn`s, so a
/// note-on reads them bit for bit instead of running `exp2` and a division
/// per stage (#355). 512 B of flash.
struct RateTables {
    attack_add: [f32; RATES],
    decay_log2: [f32; RATES],
}

const SR: f32 = crate::hw::SAMPLE_RATE as f32;

static TABLES: RateTables = {
    let mut t = RateTables {
        attack_add: [0.0; RATES],
        decay_log2: [0.0; RATES],
    };
    let mut r = 0;
    while r < RATES {
        t.attack_add[r] = attack_add(r as u8, SR);
        t.decay_log2[r] = decay_log2(r as u8, SR);
        r += 1;
    }
    t
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvCoefs {
    pub attack_add: f32,
    pub d1_log2: f32,
    pub d1_level: f32,
    pub d2_log2: f32,
    pub rr_log2: f32,
}

impl EnvCoefs {
    /// From the tables at `SAMPLE_RATE` (every build's rate), else computed.
    pub fn new(r: EnvRates, note: MidiNote, sample_rate: f32) -> Self {
        if sample_rate.to_bits() != SR.to_bits() {
            return Self::compute(r, note, sample_rate);
        }
        let ks = key_scale(r.rs, note);
        let t = &TABLES;
        let at = |e: u8| e as usize % RATES;
        Self {
            attack_add: t.attack_add[at(effective(r.ar, ks))],
            d1_log2: t.decay_log2[at(effective(r.d1r, ks))],
            d1_level: d1l_level(r.d1l),
            d2_log2: t.decay_log2[at(effective(r.d2r, ks))],
            rr_log2: t.decay_log2[at(effective_release(r.rr, ks))],
        }
    }

    /// `new` worked out with `exp2` and divisions: any rate. Counted under
    /// `test-support`, so a test can tell the tables served a note.
    pub fn compute(r: EnvRates, note: MidiNote, sample_rate: f32) -> Self {
        #[cfg(feature = "test-support")]
        COMPUTED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let ks = key_scale(r.rs, note);
        Self {
            attack_add: attack_add(effective(r.ar, ks), sample_rate),
            d1_log2: decay_log2(effective(r.d1r, ks), sample_rate),
            d1_level: d1l_level(r.d1l),
            d2_log2: decay_log2(effective(r.d2r, ks), sample_rate),
            rr_log2: decay_log2(effective_release(r.rr, ks), sample_rate),
        }
    }
}

/// `EnvCoefs::compute` calls: for the tests.
#[cfg(feature = "test-support")]
pub static COMPUTED: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Idle,
    Attack,
    Decay1,
    Decay2,
    Release,
}

/// Each sample is `level * mul + add`. A stage's length is counted when it
/// starts, so the sample loop makes no float comparison.
#[derive(Clone, Copy, Debug)]
pub struct OpEnv {
    level: f32,
    mul: f32,
    add: f32,
    left: u32,
    /// Set with `left = u32::MAX` by `hold()`: `advance` reads this instead
    /// of comparing `mul`/`add` to exact floats.
    holding: bool,
    stage: Stage,
    coefs: EnvCoefs,
}

/// `OpEnv::step`'s state in registers; its `step` matches bit for bit.
#[derive(Clone, Copy)]
pub(crate) struct EnvRun {
    level: f32,
    mul: f32,
    add: f32,
    left: u32,
}

impl EnvRun {
    #[inline(always)]
    pub(crate) fn step(&mut self, env: &mut OpEnv) -> f32 {
        self.level = self.level * self.mul + self.add;
        self.left -= 1;
        if self.left == 0 {
            *self = env.stage_end(self.level);
        }
        self.level
    }
}

impl OpEnv {
    pub const IDLE: OpEnv = OpEnv {
        level: 0.0,
        mul: 1.0,
        add: 0.0,
        left: u32::MAX,
        holding: true,
        stage: Stage::Idle,
        coefs: EnvCoefs {
            attack_add: 0.0,
            d1_log2: 0.0,
            d1_level: 1.0,
            d2_log2: 0.0,
            rr_log2: 0.0,
        },
    };

    /// Attacks from the current level, so a retrigger never clicks.
    pub fn note_on(&mut self, coefs: EnvCoefs) {
        self.coefs = coefs;
        self.enter(Stage::Attack);
    }

    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.enter(Stage::Release);
        }
    }

    pub fn set_coefs(&mut self, coefs: EnvCoefs) {
        self.coefs = coefs;
        self.enter(self.stage);
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    #[inline(always)]
    pub fn step(&mut self) -> f32 {
        self.level = self.level * self.mul + self.add;
        self.left -= 1;
        if self.left == 0 {
            self.advance();
        }
        self.level
    }

    /// The fields `step` changes, for a loop that keeps them in registers.
    #[inline(always)]
    pub(crate) fn run(&self) -> EnvRun {
        EnvRun {
            level: self.level,
            mul: self.mul,
            add: self.add,
            left: self.left,
        }
    }

    #[inline(always)]
    pub(crate) fn store(&mut self, r: EnvRun) {
        self.level = r.level;
        self.left = r.left;
    }

    #[cold]
    #[inline(never)]
    fn stage_end(&mut self, level: f32) -> EnvRun {
        self.level = level;
        self.left = 0;
        self.advance();
        self.run()
    }

    fn advance(&mut self) {
        if self.holding {
            self.left = u32::MAX; // idle or holding: never ends
        } else {
            self.next();
        }
    }

    fn next(&mut self) {
        match self.stage {
            Stage::Attack => {
                self.level = 1.0;
                self.enter(Stage::Decay1);
            }
            Stage::Decay1 => {
                self.level = self.coefs.d1_level;
                self.enter(Stage::Decay2);
            }
            Stage::Decay2 | Stage::Release => {
                self.level = 0.0;
                self.enter(Stage::Idle);
            }
            Stage::Idle => {}
        }
    }

    fn enter(&mut self, stage: Stage) {
        let c = self.coefs;
        self.stage = stage;
        match stage {
            Stage::Idle => self.hold(),
            Stage::Attack if c.attack_add > 0.0 => {
                let left = ((1.0 - self.level) / c.attack_add) as u32 + 1;
                self.set(1.0, c.attack_add, left);
            }
            Stage::Attack => self.hold(),
            Stage::Decay1 => self.decay(c.d1_log2, c.d1_level.max(ENV_FLOOR), Stage::Decay2),
            Stage::Decay2 => self.decay(c.d2_log2, ENV_FLOOR, Stage::Idle),
            Stage::Release => self.decay(c.rr_log2, ENV_FLOOR, Stage::Idle),
        }
    }

    /// Enters a `log2_mul`-rate decay toward `target`, moving to `after`
    /// once there. If the level is already at or below `target` — including
    /// a live D1L raised above it mid-decay — the target is already met:
    /// the level holds where it is and the stage advances immediately, so a
    /// parameter change never lifts it. A stage that ends by countdown
    /// instead (in `next`) still snaps exactly to its target.
    fn decay(&mut self, log2_mul: f32, target: f32, after: Stage) {
        if self.level <= target {
            self.enter(after);
        } else if log2_mul < 0.0 {
            let left = (log2(target / self.level) / log2_mul) as u32 + 1;
            self.set(exp2(log2_mul), 0.0, left);
        } else {
            self.hold();
        }
    }

    fn set(&mut self, mul: f32, add: f32, left: u32) {
        (self.mul, self.add, self.left, self.holding) = (mul, add, left, false);
    }

    /// The canonical "never ends" state: `advance` renews it forever.
    fn hold(&mut self) {
        (self.mul, self.add, self.left, self.holding) = (1.0, 0.0, u32::MAX, true);
    }
}
