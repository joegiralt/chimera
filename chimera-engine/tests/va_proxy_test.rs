//! VA FM's OSC 1 and SOFT SYNC's OSC 2 through the door (ADR 0163): with
//! no door (a build without `bank2`) or open, each plays every bit it
//! played before the move; closed, its voice is silent and the other MODEs
//! play on.

use core::mem::MaybeUninit;

use chimera_engine::dsp::va::{VaEngine, VaMode, VaParams};
use chimera_engine::project::PartId;
use chimera_hal::BLOCK_SIZE;

const SR: u32 = 48_000;
const BLOCKS: usize = 200;

/// FM's three runs, a sub, STABILITY's jitter, the duck both ways (MIX
/// to FM at block 60, back at 140), on a low and a high note.
fn scene(note: u8, b: usize) -> VaParams {
    scene_in(VaMode::Fm, note, b)
}

/// The scene with `mode` (a door MODE) in place of FM.
fn scene_in(mode: VaMode, note: u8, b: usize) -> VaParams {
    VaParams {
        mode: if (60..140).contains(&b) {
            mode
        } else {
            VaMode::Mix
        },
        wave: [(b % 4) as f32 / 3.0, 2.0 / 3.0],
        shape: [0.3, 0.6],
        amount: if b < 100 { 0.5 } else { 1.0 },
        stability: 0.5,
        sub: 0.5,
        coarse: [
            0,
            if note > 70 || mode == VaMode::SoftSync {
                7
            } else {
                0
            },
        ],
        ..VaParams::default()
    }
}

/// Each block's output, for `params` from a fresh note.
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

/// FNV-1a over the blocks' bits.
fn print(blocks: &[[f32; BLOCK_SIZE]]) -> u64 {
    blocks.iter().flatten().fold(0xcbf2_9ce4_8422_2325, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The scenes' prints before the move (`896bae53`, OSC 1's MOD render
/// called in place).
const BEFORE: [(u8, u64); 2] = [(48, 0x69d1_9009_2f94_a263), (84, 0xe30a_41c6_2610_2b05)];

/// SOFT SYNC's scenes before its move (`52b48dd7`, OSC 2's reversing render
/// called in place).
const SOFT_BEFORE: [(u8, u64); 2] = [(48, 0x08e9_a681_9062_1129), (84, 0x91c2_07b0_480f_bb78)];

fn plays_as_before() {
    for (note, want) in BEFORE {
        assert_eq!(print(&take(note, |b| scene(note, b))), want, "note {note}");
    }
    for (note, want) in SOFT_BEFORE {
        let y = take(note, |b| scene_in(VaMode::SoftSync, note, b));
        assert_eq!(print(&y), want, "SOFT SYNC note {note}");
    }
}

#[cfg(not(all(feature = "bank2", feature = "test-support")))]
#[test]
fn fm_plays_as_before_with_no_door() {
    plays_as_before();
}

/// One test: the door is a process-wide static.
#[cfg(all(feature = "bank2", feature = "test-support"))]
#[test]
fn fm_plays_through_the_door_and_is_silent_closed() {
    use chimera_engine::bank2::DOOR_CELL;
    assert!(!DOOR_CELL.is_open());
    let silent = |b: &[f32; BLOCK_SIZE]| b.iter().all(|&v| v == 0.0);
    // Closed: FM and SOFT SYNC are silent from the duck's swap, and only
    // there; MIX plays into them and out of them (no panic, the duck runs
    // on).
    for mode in [VaMode::Fm, VaMode::SoftSync] {
        for note in [48, 84] {
            let y = take(note, |b| scene_in(mode, note, b));
            for (b, blk) in y.iter().enumerate() {
                // the duck swaps after two blocks down, both ways
                let door = (62..142).contains(&b);
                assert_eq!(silent(blk), door, "{mode:?} note {note} block {b}");
            }
        }
    }
    // Every other MODE plays as with the door open.
    let other = |m: VaMode| {
        move |b: usize| VaParams {
            mode: m,
            ..scene(48, b)
        }
    };
    let modes = VaMode::ALL.into_iter().filter(|m| !m.in_bank2());
    let closed: Vec<_> = modes.clone().map(|m| print(&take(48, other(m)))).collect();
    DOOR_CELL.open_host();
    let open: Vec<_> = modes.map(|m| print(&take(48, other(m)))).collect();
    assert_eq!(closed, open, "a bank-1 MODE changed closed");
    // Open: bit for bit as before.
    plays_as_before();
    DOOR_CELL.close();
}
