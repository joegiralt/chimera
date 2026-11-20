//! A Part's EQ and SMART EQ are billed (ADR 0296): on the Part's first
//! voice, as its knobs, so admission accounts for them. Flat and off,
//! every bill is as before.

mod common;
use common::InstRig;

use chimera_core::budget::CONTROL_RESERVE;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{Cost, SampleBudget};
use chimera_core::part_eq::bill::{
    CUT, LISTEN, SIDE, SmartBill, SmartLoad, TILT, WASH_CUT, WASH_LISTEN, first_voice, wash,
};
use chimera_core::part_eq::smart::{Amount, PartKind, Role, SmartEq, Switch};
use chimera_core::part_eq::{LoCut, PartEq, Tilt};

const P1: chimera_core::project::PartId = chimera_core::project::PartId::ALL[0];
const ON: SmartEq = SmartEq::new(Switch::On, Amount::DEFAULT);
const TILTED: PartEq = PartEq::new(Tilt::at(10), LoCut::Off);

#[test]
fn flat_and_at_rest_a_part_s_eq_bills_nothing() {
    for role in Role::ALL {
        for kind in [PartKind::Tonal, PartKind::Drum] {
            let rests = SmartBill::RESTS;
            assert_eq!(first_voice(PartEq::FLAT, role, kind, rests, P1), Cost::ZERO);
        }
    }
}

#[test]
fn each_part_is_billed_what_its_bands_run() {
    let (t, runs) = (PartKind::Tonal, SmartBill::RUNS);
    assert_eq!(
        first_voice(TILTED, Role::Mid, t, SmartBill::RESTS, P1),
        TILT
    );
    assert_eq!(first_voice(PartEq::FLAT, Role::Front, t, runs, P1), LISTEN);
    assert_eq!(first_voice(PartEq::FLAT, Role::Mid, t, runs, P1), CUT);
    assert_eq!(first_voice(PartEq::FLAT, Role::Back, t, runs, P1), CUT);
    // A DRUM Part's side runs the same filters again; FRONT's is never cut.
    let d = PartKind::Drum;
    assert_eq!(
        first_voice(TILTED, Role::Front, d, runs, P1),
        TILT + TILT + LISTEN
    );
    assert_eq!(
        first_voice(TILTED, Role::Back, d, runs, P1),
        TILT + TILT + CUT + SIDE
    );
    // SMART EQ runs while it is on with an AMOUNT, and while it lets go.
    use chimera_core::part_eq::PartEqs;
    let rest = PartEqs::REST;
    let zero = SmartEq::new(Switch::On, Amount::ZERO);
    assert_eq!(SmartLoad::now(ON, &rest), SmartLoad::Runs);
    assert_eq!(SmartLoad::now(SmartEq::OFF, &rest), SmartLoad::Rests);
    assert_eq!(SmartLoad::now(zero, &rest), SmartLoad::Rests);
    // The reverb's return: billed beside the bus while the reverb is on.
    use chimera_core::dsp::reverb::ReverbParams;
    let mut rev = ReverbParams::DEFAULT;
    assert_eq!(wash(&rev, SmartBill::RESTS), Cost::ZERO);
    assert_eq!(wash(&rev, runs), WASH_CUT);
    rev.role = Role::Front;
    assert_eq!(wash(&rev, runs), WASH_LISTEN);
    rev.mix = 0.0;
    assert_eq!(wash(&rev, runs), Cost::ZERO);
}

/// Part 1's first voice's bill and its second's, under `eq`, `role`, `smart`.
fn bills(eq: PartEq, role: Role, smart: SmartEq) -> (Cost, Cost) {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.parts[0].mix.eq = eq;
    rig.shared.parts[0].mix.role = role;
    rig.shared.fx.comp.smart = smart;
    rig.note_on(0, 48);
    rig.note_on(0, 52);
    rig.render(2);
    let mut costs: Vec<Cost> = rig
        .inst
        .allocator()
        .slots()
        .iter()
        .filter(|s| !s.is_free())
        .map(|s| s.cost())
        .collect();
    costs.sort();
    (costs[1], costs[0])
}

#[test]
fn the_first_voice_of_a_part_carries_its_eq_at_note_on_and_at_recost() {
    let (plain_first, plain_next) = bills(PartEq::FLAT, Role::Mid, SmartEq::OFF);
    let (first, next) = bills(TILTED, Role::Mid, ON);
    assert_eq!(first.0, plain_first.0 + TILT.0 + CUT.0);
    assert_eq!(next, plain_next, "only the first voice carries it");
    let (front, _) = bills(PartEq::FLAT, Role::Front, ON);
    assert_eq!(front.0, plain_first.0 + LISTEN.0);
}

#[test]
fn a_full_pool_admits_fewer_voices_with_smart_eq_on() {
    // A budget of four plain voices and the bus: with SMART EQ on, the
    // Part's bands take room a voice would have had.
    let (first, next) = bills(PartEq::FLAT, Role::Mid, SmartEq::OFF);
    let count = |smart: SmartEq| {
        // The reverb's return is billed beside the bus while SMART EQ runs.
        let reserved = FxBus::COST + CONTROL_RESERVE + WASH_CUT;
        let room = Cost(reserved.0 + first.0 + 3 * next.0 + CUT.0 / 2);
        // The audio's 75 % share of `cpu` Hz is `cpu × 0.75 / 48 000` a sample.
        let mut rig = InstRig::with_budget(SampleBudget::for_cpu(room.0 * 64_000 + 63_999));
        rig.shared.fx.comp.smart = smart;
        for n in 0..6 {
            rig.note_on(0, 48 + n);
        }
        rig.render(12);
        rig.inst
            .allocator()
            .slots()
            .iter()
            .filter(|s| !s.is_free())
            .count()
    };
    assert_eq!(count(SmartEq::OFF), 4);
    assert_eq!(count(ON), 3);
}

#[test]
fn a_cut_part_moved_to_front_keeps_the_cut_bill_until_its_cuts_let_go() {
    // Part 2 (MID) plays the note Part 1 (FRONT) plays: cut. Made FRONT
    // too, its bands run on at the full rate while the cut lets go, and
    // its first voice carries CUT until then (`SmartBill::now`'s `still`).
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let (plain, _) = bills(PartEq::FLAT, Role::Mid, SmartEq::OFF);
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    rig.shared.parts[0].mix.role = Role::Front;
    rig.shared.parts[1].mix.role = Role::Mid;
    rig.shared.fx.comp.smart = SmartEq::new(Switch::On, Amount::from_f32(127.0));
    rig.note_on(0, 36);
    rig.note_on(1, 36);
    let most = |rig: &InstRig| {
        let slots = rig.inst.allocator().slots();
        slots
            .iter()
            .filter(|s| !s.is_free())
            .map(|s| s.cost())
            .max()
            .unwrap()
    };
    rig.render(3_000);
    assert_eq!(most(&rig).0, plain.0 + CUT.0);
    rig.shared.parts[1].mix.role = Role::Front;
    let mut blocks = 0;
    while most(&rig).0 == plain.0 + CUT.0 {
        rig.render(50);
        blocks += 50;
        assert!(blocks < 30_000, "the cut lets go");
    }
    assert!(
        blocks > 750,
        "billed as cut while it lets go: {blocks} blocks"
    );
    assert_eq!(most(&rig).0, plain.0 + LISTEN.0);
}
