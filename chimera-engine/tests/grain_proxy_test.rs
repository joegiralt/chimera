//! The delay through the door (ADR 0202, 0213): open, `TapeDelay::run`
//! crosses to bank 2's `tape::run`, which plays TAPE or the grains, every
//! bit of what a build without bank 2 plays; closed, the delay is silent,
//! both TYPEs. The chorus likewise. The host builds its own door
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
fn the_delay_with_the_door_closed_is_silent() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(!DOOR_CELL.is_open());
    for kind in [DelayType::Grain, DelayType::Tape] {
        assert!(render(kind).iter().all(|&b| b == 0), "{kind:?}");
    }
}

/// Open, TAPE through the door is heard, and its hash is
/// `delay_tape_hash_test`'s (the host's direct call) by the same code.
#[test]
fn tape_through_the_door_is_heard() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    DOOR_CELL.open_host();
    let tape = render(DelayType::Tape);
    DOOR_CELL.close();
    assert!(tape.iter().any(|&b| b != 0));
}

fn chorus() -> Vec<u32> {
    use chimera_engine::dsp::Stereo;
    use chimera_engine::dsp::chorus::{ChorusParams, JunoChorus};
    let mut c = Box::new(JunoChorus::new());
    let mut out = Vec::new();
    for b in 0..200 {
        let mut o = Stereo::SILENT;
        c.process_wet(&input(b), &ChorusParams::DEFAULT, 48_000, &mut o);
        out.extend(o.l.iter().chain(&o.r).map(|v| v.to_bits()));
    }
    out
}

#[test]
fn the_chorus_through_the_door_is_heard_and_closed_silent() {
    let _g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    assert!(chorus().iter().all(|&b| b == 0), "closed");
    DOOR_CELL.open_host();
    let open = chorus();
    DOOR_CELL.close();
    assert!(open.iter().any(|&b| b != 0), "open");
}
