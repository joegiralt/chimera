//! VA's oscillators with WAVE on and off its anchors (ADR 0283, the saw's
//! solo behind bank 2's door; they share `osc.rs` with DRUM's pulses).
//! Held on an anchor, every MODE that plays the plain runs (MIX's, SYNC's
//! slave) plays every bit it played at origin/0.4 `262039d9`; so does WAVE
//! moving on and off the anchors mid-note, door open, closed (bank 1's
//! pair runs) or absent. The scenes around the solo (`va_scenes`: MODE
//! detours, a Sound browsed, glide, steal, restart, SHAPE) play every bit
//! they played at origin/0.4 `807e740d`.

mod va_scenes;

use core::mem::MaybeUninit;

use chimera_engine::dsp::va::osc::{
    ANCHORS as WAVE_ANCHORS, PULSE_ANCHOR, SAW_ANCHOR, SINE_ANCHOR, TRI_ANCHOR,
};
use chimera_engine::dsp::va::{Played, VaEngine, VaMode, VaParams};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const BLOCKS: usize = 200;

/// WAVE's anchors, by name.
const SINE: f32 = SINE_ANCHOR.get();
const TRI: f32 = TRI_ANCHOR.get();
const SAW: f32 = SAW_ANCHOR.get();
const PULSE: f32 = PULSE_ANCHOR.get();

/// WAVE's anchors in its order (`osc::ANCHORS`).
const ANCHORS: [f32; 4] = [SINE, TRI, SAW, PULSE];

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
        0..30 => SAW,
        30..50 => 0.8,
        50..80 => PULSE,
        80..110 => TRI,
        // a stepped LFO up and down WAVE, three blocks a step: it
        // dwells on each anchor and leaves it
        _ => {
            // the anchors and the midpoints between them
            const STEPS: [f32; 12] = [
                SINE,
                1.0 / 6.0,
                TRI,
                0.5,
                SAW,
                5.0 / 6.0,
                PULSE,
                5.0 / 6.0,
                SAW,
                0.5,
                TRI,
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

/// What each block's plain renders played, held on each anchor, from block
/// 4 (WAVE settled).
fn solos(mode: VaMode) -> [Vec<[Option<Played>; 2]>; 4] {
    ANCHORS.map(|a| {
        let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
        let e = VaEngine::init_in_place(&mut slot);
        let p = held(mode, [a; 2]);
        e.hold();
        e.note_on(48, &p, PartId::new(0).unwrap(), SR);
        (0..40)
            .map(|_| {
                let mut out = [0.0f32; BLOCK_SIZE];
                e.render(&mut out, &p, SR);
                e.played()
            })
            .skip(4)
            .collect()
    })
}

/// Held on WAVE's saw anchor, OSC 1 and OSC 2 of every MODE that plays
/// the plain renders play the saw alone (the instances bank 2 holds, ADR
/// 0283), SYNC's slave its pair; every other anchor their pairs. The MODEs
/// with renders of their own run no plain render at all.
fn plays_solos() {
    use Played::{Alone, Pair};
    use VaMode::*;
    for mode in [
        Mix, Sync, Ring, Xor, Bits, Fm, Am, Max, LoopFm, SoftSync, Phase,
    ] {
        let (saw, pair) = match mode {
            Mix | Ring | Xor | Bits | Max => ([Some(Alone); 2], [Some(Pair); 2]),
            // AM's oscillators list their steps for its product (#660):
            // pairs on every anchor
            Am => ([Some(Pair); 2], [Some(Pair); 2]),
            Sync => ([Some(Alone), Some(Pair)], [Some(Pair); 2]),
            Fm | LoopFm | SoftSync | Phase => ([None; 2], [None; 2]),
        };
        let want = [pair, pair, saw, pair];
        for (s, w) in solos(mode).iter().zip(want) {
            assert!(s.iter().all(|b| *b == w), "{mode:?} {w:?}: {s:?}");
        }
    }
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

/// The other MODEs at BLEND 1, as the bench plays them: RING, XOR, BITS,
/// AM and MAX run the plain renders as MIX does; FM, LOOPFM, SOFT SYNC and
/// PHASE their own, untouched by the solos.
const OTHERS: [VaMode; 9] = [
    VaMode::Ring,
    VaMode::Xor,
    VaMode::Bits,
    VaMode::Am,
    VaMode::Max,
    VaMode::Fm,
    VaMode::LoopFm,
    VaMode::SoftSync,
    VaMode::Phase,
];

/// The first `n` of `OTHERS` on C3: held on each anchor, then `moving`.
/// With bank 2's door closed only the first five play (the rest are its).
fn other_prints(n: usize) -> Vec<u64> {
    let v = other_prints_in(n);
    if std::env::var_os("VA_SOLO_PRINT").is_some() {
        for h in &v {
            println!("    0x{h:016x},");
        }
    }
    v
}

fn other_prints_in(n: usize) -> Vec<u64> {
    let blend = |p: VaParams| VaParams { amount: 1.0, ..p };
    let mut v = Vec::new();
    for &mode in &OTHERS[..n] {
        for a in ANCHORS {
            v.push(print(&take(48, |_| blend(held(mode, [a; 2])))));
        }
        v.push(print(&take(48, |b| {
            blend(held(mode, [moving(b), moving(b + 13)]))
        })));
    }
    v
}

/// AM's five rows of `OTHER` with bank 2's door closed: bank 1's
/// uncorrected product, as at origin/0.4 `807e740d` (#660).
#[cfg(all(feature = "bank2", feature = "test-support"))]
const AM_CLOSED: [u64; 5] = [
    0x8152a1739d7c09a5,
    0x1a344d7a77fd13d5,
    0xae9c956fc726637a,
    0xbc77d3e7d72c6f9d,
    0xb24d2c95850f4b46,
];

/// `other_prints` at origin/0.4 `807e740d` (VA's oscillators as at
/// `262039d9`): five a MODE, the four anchors then `moving`. AM's five
/// (15..20) re-taken for its corrected product (#660, ADR 0313).
const OTHER: [u64; 45] = [
    0x44b546c94c265857,
    0x4c7711c123beaf92,
    0xb041ab148343f65e,
    0x9ff09b1c1154c7a3,
    0xc1d0518ef2a37625,
    0x04b541229149fdda,
    0xf7c48606817f3fce,
    0x6a25e306b9791558,
    0x1648ab81ecfd7ee7,
    0xd38a6730410ef06d,
    0xda47af245e13deb6,
    0xeed413aa4da1c613,
    0x6b288df36883fe17,
    0xa4b041a6f267ead9,
    0x1893a7b880b527e1,
    0x32d121963631df52,
    0xc3b8a1e83cb31a90,
    0x2b5b3ff0e111cd70,
    0x7dbfee176a5f00b6,
    0xf0a3317ade52f15f,
    0xa4141ee928f4c96c,
    0x152b7a359be93a4e,
    0x180e450ff9a25aac,
    0x18dca0790b3f5fdd,
    0xdf14011ac64ecfbc,
    0xdde401b6589f3228,
    0x590117201e0f0074,
    0x2a7dfeda71ac0848,
    0x50a44e9ab0b8cef2,
    0x20d7f03af2fc24e9,
    0xcc7e1ddd4a3b1784,
    0x410abdd116b6b9de,
    0x03f5fb06ebb611cb,
    0x03f5fb06ebb611cb,
    0xd35ad7915cd9de2e,
    0x90d9865cfbb4eba5,
    0xf14a8442a54732e0,
    0x62151a773f96ce44,
    0xa8e739e1a30b1b8d,
    0x58ef5373b11e63b4,
    0x7826f3b3024a689c,
    0x202bfd6fac2dd6a0,
    0x4ec2787c619e85c4,
    0xaa7a3c18a3db57a3,
    0xaf9b77f1074bd21c,
];

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

/// With no door (a host build without `bank2`), the solos are called
/// directly: held, bit for bit as before.
#[cfg(not(all(feature = "bank2", feature = "test-support")))]
#[test]
fn held_on_an_anchor_plays_as_before() {
    assert_eq!(held_prints(), HELD);
    assert_eq!(other_prints(OTHERS.len()), OTHER);
}

/// One test: the door is a process-wide static. Closed, VA plays bank 1's
/// pair runs; open, the saw's solos: both every bit as at 262039d9.
#[cfg(all(feature = "bank2", feature = "test-support"))]
#[test]
fn the_door_plays_solos_open_pairs_closed_as_before() {
    use chimera_engine::bank2::DOOR_CELL;
    DOOR_CELL.close();
    for mode in [VaMode::Mix, VaMode::Sync, VaMode::Ring, VaMode::Max] {
        for s in solos(mode) {
            let pair = [Some(Played::Pair); 2];
            assert!(s.iter().all(|b| *b == pair), "closed {mode:?}: {s:?}");
        }
    }
    assert_eq!(held_prints(), HELD, "closed, held");
    let closed: Vec<u64> = moving_scenes().iter().map(|s| print(s)).collect();
    assert_eq!(closed, MOVING, "closed, moving");
    // closed, AM plays bank 1's uncorrected product (#660)
    let mut plain = OTHER[..25].to_vec();
    plain[15..20].copy_from_slice(&AM_CLOSED);
    assert_eq!(other_prints(5), plain, "closed, the plain MODEs");
    DOOR_CELL.open_host();
    plays_solos();
    assert_eq!(held_prints(), HELD, "open, held");
    let open: Vec<u64> = moving_scenes().iter().map(|s| print(s)).collect();
    assert_eq!(open, MOVING, "open, moving");
    assert_eq!(other_prints(OTHERS.len()), OTHER, "open, other MODEs");
    // the scenes' detours are bank 2's MODEs: open only
    the_scenes_play_as_before();
    DOOR_CELL.close();
}

/// With no door (a host build without `bank2`), the solos play, called
/// directly; the moving scenes as before.
#[cfg(not(all(feature = "bank2", feature = "test-support")))]
#[test]
fn leaving_an_anchor_plays_as_before_with_no_door() {
    let now: Vec<u64> = moving_scenes().iter().map(|s| print(s)).collect();
    assert_eq!(now, MOVING);
    plays_solos();
}

/// `va_scenes::prints` at origin/0.4 `807e740d`, door open: every scene as
/// before the solo.
const SCENES: [u64; 21] = [
    0xf8c013c5830747d2,
    0xb69e917b497e4636,
    0x5a5466ea4ac0de9a,
    0x6ceae1f3378ae011,
    0xb8798484a4c61738,
    0x4eb691f721d322e3,
    0x830a77cf92e51e48,
    0x68ec412a8b85a2e1,
    0x7689ac8285627fe5,
    0x421ab479f81ae0cd,
    0xb2ff66e25af7e855,
    0x4176827baec83b64,
    0x6f8bac5461ef61fe,
    0xf35dd5460946e525,
    0x8d50a4bb43e4ca21,
    0x84ec05f12b7416d1,
    0xb15e9108a2ef9251,
    0x1c65ff2c6bd9ed12,
    0x51bd2db1d682e7d7,
    0xc4c008a34202303c,
    0xd7a2cc34ef0b02c7,
];

/// The scenes name the saw's anchor as the engine does.
#[test]
fn the_scenes_saw_is_the_anchor() {
    assert_eq!(va_scenes::SAW, SAW_ANCHOR.get());
    assert_eq!(ANCHORS, WAVE_ANCHORS.map(|u| u.get()));
}

/// What the plain renders played in each block of `scene`.
fn played_in(scene: &va_scenes::Scene) -> Vec<[Option<Played>; 2]> {
    let mut v = Vec::new();
    va_scenes::play(scene, |_, e| v.push(e.played()));
    v
}

/// A detour off the anchor (WAVE 0.8) through LOOPFM, back on it with MIX:
/// LOOPFM holds no constants, so the first MIX block finds the pulse's at
/// weight 0.4 and plays the pair, which lerps them home; the next plays
/// alone. LOOPFM's own blocks run no plain render.
fn a_detour_returns_through_the_pair() {
    use Played::{Alone, Pair};
    let p = played_in(&va_scenes::SCENES[2]);
    let back = (40..).find(|&b| p[b] != [None; 2]).unwrap();
    assert!(
        p[22..back].iter().all(|b| *b == [None; 2]),
        "LOOPFM: {:?}",
        &p[20..back]
    );
    assert_eq!(p[back], [Some(Pair); 2], "block {back}");
    assert_eq!(p[back + 1], [Some(Alone); 2], "block {}", back + 1);
}

/// With no door (a host build without `bank2`), every scene as before.
#[cfg(not(all(feature = "bank2", feature = "test-support")))]
#[test]
fn the_scenes_play_as_before_with_no_door() {
    the_scenes_play_as_before();
}

/// Every scene, as before; and the detour's blocks.
fn the_scenes_play_as_before() {
    let now = va_scenes::prints();
    if std::env::var_os("VA_SCENES_PRINT").is_some() {
        for (n, s) in now.iter().zip(&va_scenes::SCENES) {
            println!("    0x{n:016x}, // {}", s.name);
        }
    }
    for (i, (n, w)) in now.iter().zip(SCENES).enumerate() {
        assert_eq!(*n, w, "{}", va_scenes::SCENES[i].name);
    }
    a_detour_returns_through_the_pair();
}
