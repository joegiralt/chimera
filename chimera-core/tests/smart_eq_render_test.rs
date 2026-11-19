//! SMART EQ in the Instrument (ADR 0296): three Parts playing at once
//! (bass, chord, lead). OFF, or ON at AMOUNT 0, is the plain render bit for
//! bit with no door call; ON cuts the lower roles and never FRONT.

mod common;
use common::InstRig;

use chimera_core::hw::SampleBudget;
use chimera_core::part_eq::smart::{Amount, Band, CutDb, Role, SmartEq, Switch};
use chimera_core::project::PartId;

const BLOCKS: usize = 600;

/// Bass (Part 1), a chord (Part 2) and a lead (Part 3), each its default
/// Sound, under `smart` with `roles`: every DAC sample, as bits.
fn play(smart: SmartEq, roles: [Role; 3]) -> (Vec<u32>, InstRig) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.fx.comp.smart = smart;
    for (p, r) in roles.into_iter().enumerate() {
        rig.shared.parts[p].mix.role = r;
    }
    rig.note_on(0, 36);
    for n in [48, 52, 55, 59] {
        rig.note_on(1, n);
    }
    rig.note_on(2, 72);
    let mut out = Vec::new();
    for _ in 0..BLOCKS {
        rig.render(1);
        for pair in rig.out().iter() {
            out.extend(pair.iter().map(|s| s.to_bits()));
        }
    }
    (out, rig)
}

const MIXED: [Role; 3] = [Role::Front, Role::Mid, Role::Back];

fn on(a: u8) -> SmartEq {
    SmartEq::new(Switch::On, Amount::from_code(a).unwrap())
}

#[test]
fn off_or_amount_zero_is_the_plain_render_bit_for_bit() {
    let (plain, _) = play(SmartEq::OFF, [Role::Mid; 3]);
    for smart in [
        SmartEq::OFF,
        on(0),
        SmartEq::new(Switch::Off, Amount::from_code(127).unwrap()),
    ] {
        let (out, mut rig) = play(smart, MIXED);
        assert!(out == plain, "{smart:?} changed a sample");
        assert!(!rig.inst.part_eqs().any_live(), "{smart:?} called the door");
    }
}

#[test]
fn on_it_cuts_the_lower_roles_and_never_front() {
    let (plain, _) = play(SmartEq::OFF, MIXED);
    let (out, mut rig) = play(on(127), MIXED);
    assert!(out != plain);
    let eqs = rig.inst.part_eqs();
    assert!(eqs.smart.live());
    for b in Band::ALL {
        assert_eq!(eqs.smart.cut(PartId::ALL[0], b), CutDb::ZERO, "FRONT {b:?}");
    }
    let lower: f32 = [1, 2]
        .iter()
        .flat_map(|&p| Band::ALL.map(|b| eqs.smart.cut(PartId::ALL[p], b).get()))
        .sum();
    assert!(lower > 0.5, "the MID and BACK Parts yield: {lower} dB in all");
}

#[test]
fn every_part_one_role_cuts_nothing_and_writes_nothing() {
    let (plain, _) = play(SmartEq::OFF, [Role::Mid; 3]);
    let (out, mut rig) = play(on(127), [Role::Mid; 3]);
    // Listening reads the buses and never writes a Part with no cut.
    assert!(out == plain);
    assert!(rig.inst.part_eqs().smart.live());
}
