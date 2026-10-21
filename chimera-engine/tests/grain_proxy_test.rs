//! GRAIN through the door (ADR 0202): open, `TapeDelay::run` crosses to
//! bank 2's `grain::run` and plays the grains, every bit of what a build
//! without bank 2 plays; closed, GRAIN plays as TAPE, bit for bit. The host builds its own door
//! (`open_host`); the door is a process-wide static, so the tests take
//! turns.
#![cfg(all(feature = "bank2", feature = "test-support"))]

use std::sync::Mutex;

use chimera_engine::bank2::DOOR_CELL;
use chimera_engine::dsp::delay::{DelayParams, DelayType, TapeDelay};
use chimera_hal::BLOCK_SIZE;

static DOOR: Mutex<()> = Mutex::new(());

fn input(b: usize) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| {
        let n = (b * BLOCK_SIZE + i) as f32;
        0.6 * (n * 0.0291).sin() * if (b / 40) % 2 == 0 { 1.0 } else { 0.0 }
    })
}

fn render(kind: DelayType) -> Vec<u32> {
    let p = DelayParams {
        time_ms: 200.0,
        feedback: 0.6,
        ..DelayParams::DEFAULT
    }
    .with(DelayParams::TYPE, kind.value())
    .with(DelayParams::PITCH, 12.0);
    let mut d = Box::new(TapeDelay::new());
    let mut out = Vec::new();
    for b in 0..200 {
        let mut buf = input(b);
        d.process_wet(&mut buf, &p, 48_000);
        out.extend(buf.iter().map(|v| v.to_bits()));
    }
    out
}

#[test]
fn grain_through_the_door_is_heard() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    DOOR_CELL.open_host();
    assert_ne!(render(DelayType::Grain), render(DelayType::Tape));
    DOOR_CELL.close();
}

#[test]
fn grain_with_the_door_closed_plays_tape() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!DOOR_CELL.is_open());
    assert_eq!(render(DelayType::Grain), render(DelayType::Tape));
}
