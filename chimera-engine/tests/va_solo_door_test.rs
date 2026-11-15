//! VA's oscillators with WAVE on an anchor (ADR 0251, bank 2's door).
//! Held on an anchor, every MODE that plays the plain runs (MIX's, SYNC's
//! slave) plays every bit it played at origin/0.4 `262039d9`. WAVE moving
//! on and off anchors mid-note, it nulls under −120 dB against the pair
//! runs (the door closed: bank 1's path, as at 262039d9).

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
    #[cfg(all(feature = "bank2", feature = "test-support"))]
    chimera_engine::bank2::DOOR_CELL.open_host();
    assert_eq!(held_prints(), HELD);
}

/// The worst null of `a` against `b`, in dB.
fn null_db(a: &[[f32; BLOCK_SIZE]], b: &[[f32; BLOCK_SIZE]]) -> f64 {
    let (mut er, mut ed) = (0.0f64, 0.0f64);
    for (x, y) in a.iter().flatten().zip(b.iter().flatten()) {
        er += f64::from(*y) * f64::from(*y);
        ed += f64::from(x - y) * f64::from(x - y);
    }
    if ed == 0.0 {
        f64::NEG_INFINITY
    } else {
        10.0 * (ed / er).log10()
    }
}

/// One test: the door is a process-wide static. Closed, VA plays bank 1's
/// pair runs: every bit as at 262039d9. Open, the moving scenes null
/// against them under −120 dB.
#[cfg(all(feature = "bank2", feature = "test-support"))]
#[test]
fn leaving_an_anchor_nulls_against_the_pair_runs() {
    use chimera_engine::bank2::DOOR_CELL;
    DOOR_CELL.close();
    assert_eq!(held_prints(), HELD, "closed, held");
    let closed = moving_scenes();
    assert_eq!(
        closed.iter().map(|s| print(s)).collect::<Vec<_>>(),
        MOVING,
        "closed, moving"
    );
    DOOR_CELL.open_host();
    let open = moving_scenes();
    for (i, (o, c)) in open.iter().zip(&closed).enumerate() {
        let db = null_db(o, c);
        assert!(db < -120.0, "scene {i}: null {db:.1} dB");
    }
    DOOR_CELL.close();
}

/// With no door (a host build without `bank2`), the moving scenes play.
#[cfg(not(all(feature = "bank2", feature = "test-support")))]
#[test]
fn leaving_an_anchor_plays_with_no_door() {
    let now = moving_scenes();
    assert!(now.iter().flatten().flatten().all(|v| v.is_finite()));
}
