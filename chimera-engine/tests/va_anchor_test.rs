//! VA's oscillators with WAVE on and off its anchors: they share `osc.rs`
//! with DRUM's pulses. Held on an anchor, MIX and SYNC (its slave) play
//! every bit they played at origin/0.4 `262039d9`; so does WAVE moving on
//! and off the anchors mid-note.

use core::mem::MaybeUninit;

use chimera_engine::dsp::va::{VaEngine, VaMode, VaParams};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const BLOCKS: usize = 200;

/// WAVE's anchors: sine, triangle, saw, pulse.
const ANCHORS: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];

/// A held note on `mode`, both oscillators at WAVE `w`, a sub, STABILITY's
/// jitter, SHAPE off the ends; OSC 2 a fifth up.
fn held(mode: VaMode, w: [f32; 2]) -> VaParams {
    VaParams {
        mode,
        wave: w,
        shape: [0.3, 0.6],
        stability: 0.5,
        sub: 0.5,
        coarse: [0, 7],
        ..VaParams::default()
    }
}

/// WAVE's path for a moving scene at block `b`: on the saw, off it toward
/// the pulse, on the pulse, a jump to the triangle, then a slow sweep
/// through every anchor that dwells on each.
fn moving(b: usize) -> f32 {
    match b {
        0..30 => ANCHORS[2],
        30..50 => 0.8,
        50..80 => ANCHORS[3],
        80..110 => ANCHORS[1],
        // a stepped LFO up and down WAVE, three blocks a step: it
        // dwells on each anchor and leaves it
        _ => {
            const STEPS: [f32; 12] = [
                0.0,
                1.0 / 6.0,
                1.0 / 3.0,
                0.5,
                2.0 / 3.0,
                5.0 / 6.0,
                1.0,
                5.0 / 6.0,
                2.0 / 3.0,
                0.5,
                1.0 / 3.0,
                1.0 / 6.0,
            ];
            STEPS[(b - 110) / 3 % STEPS.len()]
        }
    }
}

/// Each block's output for `params` from a fresh note.
fn take(note: u8, params: impl Fn(usize) -> VaParams) -> Vec<[f32; BLOCK_SIZE]> {
    let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
    let e = VaEngine::init_in_place(&mut slot);
    let p = params(0);
    e.hold();
    e.note_on(note, &p, PartId::new(0).unwrap(), SR);
    (0..BLOCKS)
        .map(|b| {
            let mut out = [0.0f32; BLOCK_SIZE];
            e.render(&mut out, &params(b), SR);
            out
        })
        .collect()
}

/// FNV-1a over the blocks' bits, of a scene that sounds.
fn print(blocks: &[[f32; BLOCK_SIZE]]) -> u64 {
    assert!(blocks.iter().flatten().any(|v| v.abs() > 0.01), "silent");
    blocks.iter().flatten().fold(0xcbf2_9ce4_8422_2325, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The held scenes: MIX and SYNC, a low and a high note, each anchor.
fn held_scenes() -> Vec<(VaMode, u8, usize)> {
    let mut v = Vec::new();
    for mode in [VaMode::Mix, VaMode::Sync] {
        for note in [36, 84] {
            for a in 0..ANCHORS.len() {
                v.push((mode, note, a));
            }
        }
    }
    v
}

fn held_prints() -> Vec<u64> {
    held_scenes()
        .into_iter()
        .map(|(mode, note, a)| print(&take(note, |_| held(mode, [ANCHORS[a]; 2]))))
        .collect()
}

/// The moving scenes: MIX and SYNC, a low and a high note; OSC 2 runs
/// `moving` a third of a sweep behind OSC 1.
fn moving_scenes() -> Vec<Vec<[f32; BLOCK_SIZE]>> {
    let mut v = Vec::new();
    for mode in [VaMode::Mix, VaMode::Sync] {
        for note in [36, 84] {
            v.push(take(note, |b| held(mode, [moving(b), moving(b + 13)])));
        }
    }
    v
}

/// `held_prints` at origin/0.4 `262039d9`.
const HELD: [u64; 16] = [
    0x9396f73c16762493,
    0x072cd7f1e81cc8f2,
    0xe7c148cee5a60e44,
    0xe41987d88316fc57,
    0x0995643b6316e914,
    0x3e67c39057dbdaae,
    0xfdfb69055079cc75,
    0xa718477880decc39,
    0x64e4a6bb58b5feed,
    0x9fa662c5fbbda91a,
    0xaa08d8318e477cf7,
    0x61173624d66f9f75,
    0x4e0198fee4a38820,
    0xb091e1fa66e6975a,
    0x94508b12ea0fe863,
    0xe0b11b1d1705b339,
];

/// `moving_scenes`' prints at origin/0.4 `262039d9`.
const MOVING: [u64; 4] = [
    0x1c234f25c8453ace,
    0xea978f13ceff4bbb,
    0x09543e1eb1e1c4d4,
    0x192eea060f91785f,
];

#[test]
fn held_on_an_anchor_plays_as_before() {
    assert_eq!(held_prints(), HELD);
}

#[test]
fn leaving_an_anchor_plays_as_before() {
    let now: Vec<u64> = moving_scenes().iter().map(|s| print(s)).collect();
    assert_eq!(now, MOVING);
}
