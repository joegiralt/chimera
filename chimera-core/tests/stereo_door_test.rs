//! The stereo through bank 2's door (ADR 0294): open, OUT PAN places a
//! note and WIDTH narrows it; closed, every note plays centred and every
//! Part's side as panned (`stereo_test` holds the rest).
#![cfg(feature = "bank2")]

mod common;
use common::InstRig;

use chimera_core::bank2::DOOR_CELL;
use chimera_core::hw::SampleBudget;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::stereo::{VoicePan, Width};

/// Pair 1's left and right energy over a hard-right VA C4's blocks 10–30.
fn play(width: Width) -> (f32, f32) {
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    // VA: its oscillators play from bank 1 with the door closed.
    *rig.shared.parts[0].params = ParamSnapshot::for_engine(EngineType::Va);
    rig.shared.parts[0].params.out.pan = VoicePan::new(1.0);
    rig.shared.parts[0].params.out.width = width;
    // A block first: the Part's WIDTH lands while it has no side.
    rig.render(1);
    rig.note_on(0, 60);
    let (mut l, mut r) = (0.0f32, 0.0f32);
    for b in 0..30 {
        rig.render(1);
        if b >= 10 {
            for f in rig.out()[0].chunks(2) {
                l += f[0] * f[0];
                r += f[1] * f[1];
            }
        }
    }
    (l, r)
}

/// One test: the door is process-wide.
#[test]
fn the_door_places_open_and_centres_closed() {
    DOOR_CELL.open_host();
    let (l, r) = play(Width::UNITY);
    assert!(r > 0.0 && l < 1e-8 * r, "open: hard right {l} {r}");
    let (l, r) = play(Width::MONO);
    assert!((l - r).abs() < 1e-4 * r, "open: WIDTH 0 is mono {l} {r}");
    DOOR_CELL.close();
    let (l, r) = play(Width::UNITY);
    assert!(r > 0.0 && l == r, "closed: centred {l} {r}");
}
