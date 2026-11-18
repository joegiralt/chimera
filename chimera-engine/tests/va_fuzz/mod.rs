//! VA's fuzzed scenes (ADR 0283): seeded draws of every MODE, WAVE and
//! SHAPE (moving apart and together, on the saw's anchor, a ulp either
//! side, −0, NaN, ±inf, huge, out of range), STABILITY, BLEND, SUB,
//! tuning, glides, steals, restarts, pitch and phase sets, and the door
//! toggled mid-note (`door`, a no-op without one). They use only the
//! engine's playing API, so the same scenes play on a tree without the
//! solo, which pins their digests.

use core::mem::MaybeUninit;

use chimera_engine::dsp::va::{VaEngine, VaMode, VaParams};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;

pub const SR: u32 = 48_000;
/// Blocks a scene plays.
pub const BLOCKS: usize = 160;
/// WAVE's saw anchor, as `osc::SAW_ANCHOR` names it (a tree without the
/// solo has no name for it).
pub const SAW: f32 = 2.0 / 3.0;

/// xorshift64: a scene's draws, the same every time it plays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// 0..1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[(self.next() % v.len() as u64) as usize]
    }

    fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }

    /// One of `v`, or a fresh draw, or now and then nonsense.
    fn knob(&mut self, v: &[f32]) -> f32 {
        if self.chance(0.05) {
            self.pick(&NONSENSE)
        } else if self.chance(0.5) {
            self.pick(v)
        } else {
            self.unit()
        }
    }
}

/// Values no knob sends, which the params must survive.
const NONSENSE: [f32; 8] = [
    f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    1e30,
    -1e30,
    f32::MAX,
    f32::MIN_POSITIVE,
    -0.0,
];

pub const MODES: [VaMode; 11] = [
    VaMode::Mix,
    VaMode::Sync,
    VaMode::Ring,
    VaMode::Xor,
    VaMode::Bits,
    VaMode::Fm,
    VaMode::Am,
    VaMode::Max,
    VaMode::LoopFm,
    VaMode::SoftSync,
    VaMode::Phase,
];

/// The MODEs that play the plain renders, and so the solos.
const PLAIN: [VaMode; 7] = [
    VaMode::Mix,
    VaMode::Sync,
    VaMode::Ring,
    VaMode::Xor,
    VaMode::Bits,
    VaMode::Am,
    VaMode::Max,
];

/// WAVE: mostly on or beside the saw's anchor, else anywhere or nonsense.
fn wave(r: &mut Rng) -> f32 {
    let near = [
        SAW,
        SAW,
        SAW,
        f32::from_bits(SAW.to_bits() - 1),
        f32::from_bits(SAW.to_bits() + 1),
        f32::from_bits(SAW.to_bits() + 2),
        0.0,
        1.0 / 3.0,
        1.0,
        0.7,
        0.8,
        0.5,
        1.5,
        -0.2,
    ];
    if r.chance(0.8) {
        r.pick(&near)
    } else if r.chance(0.8) {
        r.unit()
    } else {
        r.pick(&NONSENSE)
    }
}

/// SHAPE: its ends, a hair inside them, anywhere, or nonsense.
fn shape(r: &mut Rng) -> f32 {
    r.knob(&[0.0, 1.0, 1e-7, 1.0 - 1e-7, 0.5, -1.0, 2.0])
}

/// Scene `seed`'s blocks, each block's FNV-1a over its bits; `door(open)`
/// where the scene opens or closes bank 2's door, `seen` after each block.
pub fn play(seed: u64, mut door: impl FnMut(bool), mut seen: impl FnMut(&VaEngine)) -> Vec<u64> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let part = PartId::new((seed % 4) as u8).unwrap();
    let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
    let e = VaEngine::init_in_place(&mut slot);
    let plain_heavy = seed % 3 == 0;
    let mut p = VaParams {
        // every MODE starts some scene
        mode: MODES[(seed % MODES.len() as u64) as usize],
        wave: [wave(&mut r), wave(&mut r)],
        shape: [shape(&mut r), shape(&mut r)],
        stability: r.knob(&[0.0, 0.5, 1.0]),
        sub: r.knob(&[0.0, 0.5]),
        amount: r.knob(&[0.0, 1.0]),
        coarse: [r.pick(&[0i8, 0, 12, -12]), r.pick(&[0i8, 7, -5, 19])],
        fine: [0.0, r.pick(&[0.0, 0.3, -0.2])],
        ..VaParams::default()
    };
    let mut note = r.pick(&[24u8, 36, 48, 60, 84, 96, 108, 120]);
    door(true);
    e.hold();
    e.note_on(note, &p, part, SR);
    let mut prints = Vec::with_capacity(BLOCKS);
    for _ in 0..BLOCKS {
        let roll = r.unit();
        if roll < 0.05 {
            p.mode = if plain_heavy && r.chance(0.7) {
                r.pick(&PLAIN)
            } else {
                r.pick(&MODES)
            };
        } else if roll < 0.12 {
            let w = wave(&mut r);
            match r.next() % 3 {
                0 => p.wave[0] = w,
                1 => p.wave[1] = w,
                _ => p.wave = [w; 2],
            }
        } else if roll < 0.15 {
            p.shape = [shape(&mut r), shape(&mut r)];
        } else if roll < 0.17 {
            // WAVE and SHAPE in the same block
            p.wave = [wave(&mut r), wave(&mut r)];
            p.shape = [shape(&mut r), shape(&mut r)];
        } else if roll < 0.18 {
            p.stability = r.knob(&[0.0, 0.5, 1.0]);
        } else if roll < 0.19 {
            note = r.pick(&[24u8, 36, 48, 60, 72, 84, 127]);
            e.glide_on(note, &p, part, (SR, 0.05));
        } else if roll < 0.20 {
            note = r.pick(&[24u8, 36, 48, 60, 72, 84]);
            e.hold();
            e.note_on(note, &p, part, SR);
        } else if roll < 0.21 {
            e.note_on(note, &p, part, SR);
        } else if roll < 0.22 {
            e.set_pitch(r.unit() * 200.0 - 100.0);
        } else if roll < 0.23 {
            e.set_osc_phase([r.unit(), r.unit()]);
        } else if roll < 0.25 {
            door(r.chance(0.5));
        } else if roll < 0.26 {
            (p.amount, p.sub) = (r.knob(&[0.0, 1.0]), r.knob(&[0.0, 1.0]));
        }
        let mut out = [0.0f32; BLOCK_SIZE];
        e.render(&mut out, &p, SR);
        seen(e);
        prints.push(out.iter().fold(0xcbf2_9ce4_8422_2325, |h, v| {
            (h ^ u64::from(v.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
        }));
    }
    door(false);
    prints
}

/// A scene's digest: its blocks' prints folded.
pub fn digest(prints: &[u64]) -> u64 {
    prints.iter().fold(0xcbf2_9ce4_8422_2325, |h, p| {
        (h ^ p).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
