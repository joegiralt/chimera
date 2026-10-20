//! FUZZ through the door (ADR 0157): open, `Fuzz::play` crosses to bank 2's
//! `Fuzz::run` and returns its every bit; closed, the block stays dry and
//! the pedal's state waits. The host builds its own door (`open_host`); the
//! door is a process-wide static, so the tests take turns.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::voice_fx::Ends;
use chimera_engine::dsp::voice_fx::fuzz::Fuzz;
use chimera_hal::BLOCK_SIZE;

static DOOR: Mutex<()> = Mutex::new(());

fn block(b: usize) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| {
        let n = (b * BLOCK_SIZE + i) as f32;
        0.8 * (n * 0.0291).sin()
    })
}

/// Blocks steady, moving and glided, so each of `run`'s paths crosses.
fn ends(b: usize) -> (Ends, bool) {
    let steady = Ends {
        pots: [(0.7, 0.7), (0.5, 0.5), (0.5, 0.5)],
        mix: (1.0, 1.0),
        gate: (1.0, 1.0),
    };
    match b % 3 {
        0 => (steady, false),
        1 => (
            Ends {
                pots: [(0.7, 0.4), (0.5, 0.9), (0.5, 0.6)],
                ..steady
            },
            true,
        ),
        _ => (
            Ends {
                pots: [(0.4, 0.4), (0.9, 0.9), (0.6, 0.6)],
                mix: (1.0, 0.5),
                ..steady
            },
            false,
        ),
    }
}

fn bits(x: &[f32; BLOCK_SIZE]) -> Vec<u32> {
    x.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn fuzz_through_the_door_equals_fuzz_direct() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    DOOR_CELL.open_host();
    let (mut via, mut direct) = (Fuzz::new(), Fuzz::new());
    let mut heard = false;
    for b in 0..24 {
        let (e, glide) = ends(b);
        let (mut a, mut d) = (block(b), block(b));
        let dry = a;
        via.play(&mut a, &e, glide);
        direct.run(&mut d, &e, glide);
        assert_eq!(bits(&a), bits(&d), "block {b}");
        heard |= a != dry;
    }
    assert!(heard, "FUZZ changed nothing open");
    DOOR_CELL.close();
}

#[test]
fn fuzz_with_the_door_closed_is_dry() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!DOOR_CELL.is_open());
    let mut fz = Fuzz::new();
    for b in 0..24 {
        let (e, glide) = ends(b);
        let mut x = block(b);
        fz.play(&mut x, &e, glide);
        assert_eq!(bits(&x), bits(&block(b)), "block {b} not dry");
    }
}
