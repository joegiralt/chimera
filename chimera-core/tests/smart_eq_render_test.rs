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
    assert!(
        lower > 0.5,
        "the MID and BACK Parts yield: {lower} dB in all"
    );
}

#[test]
fn every_part_one_role_cuts_nothing_and_writes_nothing() {
    let (plain, _) = play(SmartEq::OFF, [Role::Mid; 3]);
    let (out, mut rig) = play(on(127), [Role::Mid; 3]);
    // Listening reads the buses and never writes a Part with no cut.
    assert!(out == plain);
    assert!(rig.inst.part_eqs().smart.live());
}

/// Part 1 (FRONT) holding a chord, sent to the reverb, under `smart`:
/// every DAC sample, as bits.
fn washed(smart: SmartEq) -> (Vec<u32>, InstRig) {
    use chimera_core::dsp::SendLevel;
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.fx.comp.smart = smart;
    rig.shared.parts[0].mix.role = Role::Front;
    rig.shared.parts[0].params.out.sends[2] = SendLevel::new(1.0);
    for n in [45, 52, 57, 60] {
        rig.note_on(0, n);
    }
    let mut out = Vec::new();
    for _ in 0..BLOCKS {
        rig.render(1);
        for pair in rig.out().iter() {
            out.extend(pair.iter().map(|s| s.to_bits()));
        }
    }
    (out, rig)
}

#[test]
fn the_reverb_s_return_yields_to_a_front_part_and_off_it_is_as_played() {
    let (plain, _) = washed(SmartEq::OFF);
    let (zero, mut rig) = washed(on(0));
    assert!(zero == plain, "AMOUNT 0 changed the return");
    assert!(!rig.inst.part_eqs().any_live());
    // On: the return (BACK by default) is cut under the Part that feeds
    // it; the Part itself is not.
    let (out, mut rig) = washed(on(64));
    assert!(out != plain);
    let eqs = rig.inst.part_eqs();
    let wash: f32 = Band::ALL.map(|b| eqs.smart.wash_cut(b).get()).iter().sum();
    assert!(wash > 1.0, "the return yields: {wash} dB in all");
    for b in Band::ALL {
        assert_eq!(eqs.smart.cut(PartId::ALL[0], b), CutDb::ZERO);
    }
}
