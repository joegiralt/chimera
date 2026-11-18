//! VA's saw solo (ADR 0283) against the pairs it stands in for, fuzzed:
//! every scene plays twice in one tree, the solos taken where they may and
//! then every plain block forced to its pair (`osc::solo::force_pairs`),
//! and each block's bits must match. The scenes draw MODE, WAVE (on the
//! saw's anchor, a ulp either side, −0, NaN, out of range), SHAPE,
//! STABILITY, BLEND, SUB, tuning, glides, steals, restarts, pitch and
//! phase sets, and the door closing and opening mid-note. A process-wide
//! door and switch: one test in its own binary.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use core::mem::MaybeUninit;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::va::osc::{SAW_ANCHOR, solo};
use chimera_engine::dsp::va::{Played, VaEngine, VaMode, VaParams};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
/// Scenes and blocks a scene: about a second in `just check`.
const SCENES: u64 = 400;
const BLOCKS: usize = 160;

/// xorshift64: the scene's draws, the same both times it plays.
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

    /// One of `v`, or a fresh draw, or now and then NaN.
    fn knob(&mut self, v: &[f32]) -> f32 {
        if self.chance(0.03) {
            f32::NAN
        } else if self.chance(0.5) {
            self.pick(v)
        } else {
            self.unit()
        }
    }
}

const MODES: [VaMode; 11] = [
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

/// The plain MODEs, which play the solos.
const PLAIN: [VaMode; 4] = [VaMode::Mix, VaMode::Sync, VaMode::Ring, VaMode::Max];

/// WAVE: mostly on or beside the saw's anchor, or off it, or nonsense.
fn wave(r: &mut Rng) -> f32 {
    let saw = SAW_ANCHOR.get();
    let near = [
        saw,
        saw,
        saw,
        f32::from_bits(saw.to_bits() - 1),
        f32::from_bits(saw.to_bits() + 1),
        f32::from_bits(saw.to_bits() + 2),
        0.0,
        -0.0,
        1.0 / 3.0,
        1.0,
        0.7,
        0.8,
        0.5,
        f32::NAN,
        1.5,
        -0.2,
    ];
    if r.chance(0.8) {
        r.pick(&near)
    } else {
        r.unit()
    }
}

/// A scene's bits, block by block, and the plain blocks it played alone.
fn play(seed: u64) -> (Vec<u64>, u32) {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let part = PartId::new((seed % 4) as u8).unwrap();
    let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
    let e = VaEngine::init_in_place(&mut slot);
    let plain_heavy = seed % 3 == 0;
    let mut p = VaParams {
        mode: if plain_heavy {
            VaMode::Mix
        } else {
            r.pick(&MODES)
        },
        wave: [wave(&mut r), wave(&mut r)],
        shape: [r.unit(), r.unit()],
        stability: r.knob(&[0.0, 0.5, 1.0]),
        sub: r.knob(&[0.0, 0.5]),
        amount: r.unit(),
        coarse: [r.pick(&[0i8, 0, 12, -12]), r.pick(&[0i8, 7, -5, 19])],
        fine: [0.0, r.pick(&[0.0, 0.3, -0.2])],
        ..VaParams::default()
    };
    let mut note = r.pick(&[24u8, 36, 48, 60, 84, 96, 108, 120]);
    DOOR_CELL.open_host();
    e.hold();
    e.note_on(note, &p, part, SR);
    let mut prints = Vec::with_capacity(BLOCKS);
    let mut alone = 0;
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
            p.shape = [r.knob(&[0.0, 1.0]), r.knob(&[0.0, 1.0])];
        } else if roll < 0.16 {
            p.stability = r.pick(&[0.0, 0.5, 1.0]);
        } else if roll < 0.17 {
            note = r.pick(&[24u8, 36, 48, 60, 72, 84, 127]);
            e.glide_on(note, &p, part, (SR, 0.05));
        } else if roll < 0.18 {
            note = r.pick(&[24u8, 36, 48, 60, 72, 84]);
            e.hold();
            e.note_on(note, &p, part, SR);
        } else if roll < 0.19 {
            e.note_on(note, &p, part, SR);
        } else if roll < 0.20 {
            e.set_pitch(r.unit() * 200.0 - 100.0);
        } else if roll < 0.21 {
            e.set_osc_phase([r.unit(), r.unit()]);
        } else if roll < 0.23 {
            if r.chance(0.5) {
                DOOR_CELL.close()
            } else {
                DOOR_CELL.open_host()
            }
        } else if roll < 0.24 {
            (p.amount, p.sub) = (r.unit(), r.unit());
        }
        let mut out = [0.0f32; BLOCK_SIZE];
        e.render(&mut out, &p, SR);
        alone += e
            .played()
            .iter()
            .filter(|x| **x == Some(Played::Alone))
            .count() as u32;
        prints.push(out.iter().fold(0xcbf2_9ce4_8422_2325, |h, v| {
            (h ^ u64::from(v.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
        }));
    }
    DOOR_CELL.close();
    (prints, alone)
}

#[test]
fn solos_play_their_pairs_bit_for_bit() {
    let mut alone = 0;
    for seed in 0..SCENES {
        solo::force_pairs(false);
        let (solos, n) = play(seed);
        solo::force_pairs(true);
        let (pairs, _) = play(seed);
        solo::force_pairs(false);
        alone += n;
        if let Some(b) = solos.iter().zip(&pairs).position(|(a, b)| a != b) {
            panic!("scene {seed}: block {b} differs");
        }
    }
    // the fuzz reaches the solo: a good share of its plain blocks
    assert!(alone > 5_000, "only {alone} solo blocks");
}
