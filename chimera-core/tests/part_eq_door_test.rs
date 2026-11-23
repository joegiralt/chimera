// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-Part EQ through bank 2's door (ADR 0284): open, the EQ plays;
//! closed, every Part plays flat.
#![cfg(feature = "bank2")]

mod common;
use common::InstRig;

use chimera_core::bank2::DOOR_CELL;
use chimera_core::hw::SampleBudget;
use chimera_core::part_eq::{LoCut, PartEq, Tilt};
use chimera_core::project::PartId;

fn play(eq: PartEq) -> Vec<f32> {
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.parts[0].mix.eq = eq;
    rig.note_on(0, 48);
    let mut l = Vec::new();
    for _ in 0..60 {
        rig.render(1);
        l.extend(rig.out()[0].iter().step_by(2));
    }
    l
}

/// One test: the door is a process-wide static.
#[test]
fn the_door_plays_the_eq_open_and_flat_closed() {
    let eq = PartEq::new(Tilt::at(-40), LoCut::On);
    DOOR_CELL.close();
    let (flat, closed) = (play(PartEq::FLAT), play(eq));
    assert_eq!(flat, closed, "closed: flat");
    // Open, the other doors play too (the reverb's engines): compare open
    // with open.
    DOOR_CELL.open_host();
    let (flat, open) = (play(PartEq::FLAT), play(eq));
    assert_ne!(open, flat, "open: EQ'd");
    // Closed mid-note, the EQ plays flat and its state goes to rest, so
    // reopened it starts from flat, never from the taps it left.
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.parts[0].mix.eq = eq;
    rig.note_on(0, 48);
    rig.render(20);
    assert!(rig.inst.part_eqs().running(PartId::FIRST));
    DOOR_CELL.close();
    rig.render(1);
    assert!(!rig.inst.part_eqs().any_live(), "closed: at rest");
}
