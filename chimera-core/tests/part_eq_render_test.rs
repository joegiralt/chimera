//! The per-Part EQ in the Instrument (ADR 0284): after the Sound, before
//! LEVEL, PAN and the sends; flat and off, not there at all.

mod common;
use common::InstRig;

use chimera_core::hw::SampleBudget;
use chimera_core::part_eq::{LoCut, PartEq, Tilt};
use chimera_core::project::PartId;
use chimera_hal::BLOCK_SIZE;

/// Part 0 holding C3 under `eq` for `blocks` blocks: pair 1's left.
fn play(eq: PartEq, blocks: usize) -> (Vec<f32>, InstRig) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.parts[0].mix.eq = eq;
    rig.note_on(0, 48);
    let mut l = Vec::new();
    for _ in 0..blocks {
        rig.render(1);
        l.extend(rig.out()[0].iter().step_by(2));
    }
    (l, rig)
}

/// The share of `x`'s energy in its sample-to-sample moves: higher is
/// brighter.
fn brightness(x: &[f32]) -> f32 {
    let e: f32 = x.iter().map(|v| v * v).sum();
    let d: f32 = x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
    d / e
}

#[test]
fn tilt_darkens_and_brightens_the_part() {
    let flat = brightness(&play(PartEq::FLAT, 200).0[BLOCK_SIZE * 100..]);
    let dark = brightness(&play(PartEq::new(Tilt::at(-64), LoCut::Off), 200).0[BLOCK_SIZE * 100..]);
    let bright =
        brightness(&play(PartEq::new(Tilt::at(63), LoCut::Off), 200).0[BLOCK_SIZE * 100..]);
    assert!(
        dark < 0.5 * flat && bright > 2.0 * flat,
        "{dark} {flat} {bright}"
    );
}

#[test]
fn flat_and_off_never_wakes_the_eq() {
    let (_, mut rig) = play(PartEq::FLAT, 20);
    assert!(!rig.inst.part_eqs().any_live());
    let (_, mut rig) = play(PartEq::new(Tilt::FLAT, LoCut::On), 20);
    assert!(rig.inst.part_eqs().running(PartId::FIRST));
}

#[test]
fn back_to_flat_plays_as_never_eq_d() {
    // A Part EQ'd, then flat again and settled, plays bit for bit as one
    // never EQ'd: the bypass is the plain path.
    let (plain, _) = play(PartEq::FLAT, 300);
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.parts[0].mix.eq = PartEq::new(Tilt::at(30), LoCut::On);
    rig.note_on(0, 48);
    let mut l = Vec::new();
    for b in 0..300 {
        if b == 20 {
            rig.shared.parts[0].mix.eq = PartEq::FLAT;
        }
        rig.render(1);
        l.extend(rig.out()[0].iter().step_by(2));
    }
    assert!(!rig.inst.part_eqs().any_live(), "settled");
    // The limiter's own state differs from the EQ'd blocks for a while.
    let tail = BLOCK_SIZE * 250;
    let diff = l[tail..]
        .iter()
        .zip(&plain[tail..])
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(diff < 1e-4, "{diff}");
    assert!(!rig.inst.part_eqs().running(PartId::FIRST));
}
