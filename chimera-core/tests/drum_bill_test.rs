//! DRUM slice 2 (ADR 0269): a DRUM hit is billed for its kit's pads'
//! filters and mods, not its Sound's filter.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

use std::sync::{LazyLock, Mutex};

use chimera_core::dsp::drum::sound::{
    Amount, PAD_MOD_COST, PadEnv, PadRoute, PadSound, RouteDest, RouteId, RouteSource, kit_bill,
};
use chimera_core::dsp::drum::types::{Kit, PadId};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::dsp::filter::{FilterKind, K2040Voice};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::Cost;
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::project::PartId;

static WRITER: LazyLock<Mutex<KitWriter>> =
    LazyLock::new(|| Mutex::new(KitWriter::take().expect("the kit writer, once")));

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).expect("pads 1–12")
}

fn drum_init() -> ParamSnapshot {
    ParamSnapshot::for_engine(EngineType::Drum)
}

fn costs(p: &ParamSnapshot, kit: &Kit) -> (Cost, Cost) {
    let m = ModState::new();
    (Voice::cost(p, &m), Voice::drum_cost(p, &m, kit_bill(kit)))
}

#[test]
fn a_2040_pad_bills_above_svf() {
    let p = drum_init();
    assert_eq!(p.filter.kind(), FilterKind::Svf);
    let mut kit = Kit::DEFAULT;
    kit.pad_mut(pad(5))
        .sound_mut()
        .filter_mut()
        .set_kind(FilterKind::K2040);
    let (cost, drum) = costs(&p, &kit);
    assert!(drum > cost, "{drum:?} vs {cost:?}");
    let v = kit.pad(pad(5)).sound().filter().voice();
    let gap = FilterKind::K2040.cost(v).0 - FilterKind::Svf.cost(K2040Voice::Rev2).0;
    assert!(drum.0 - cost.0 >= gap, "{drum:?} − {cost:?} < {gap}");
}

#[test]
fn default_kit_bills_as_slice1() {
    let p = drum_init();
    let (cost, drum) = costs(&p, &Kit::DEFAULT);
    assert_eq!(drum, cost);
    // A loaded slice-1 kit: every pad its Sound's filter.
    let mut p = drum_init();
    p.filter.set_kind(FilterKind::Ladder);
    p.filter.cutoff = 420.0;
    let mut kit = Kit::DEFAULT;
    for id in PadId::ALL {
        *kit.pad_mut(id).sound_mut() = PadSound::from_sound(&p);
    }
    let (cost, drum) = costs(&p, &kit);
    assert_ne!(cost, costs(&drum_init(), &Kit::DEFAULT).0);
    assert_eq!(drum, cost);
}

#[test]
fn a_routed_pad_bills_its_move() {
    let mut kit = Kit::DEFAULT;
    let s = kit.pad_mut(pad(3)).sound_mut();
    s.filter_mut().set_kind(FilterKind::Ladder);
    let r = PadRoute::new(
        RouteSource::ModEnv,
        RouteDest::Cutoff,
        Amount::new(64).unwrap(),
    );
    s.routes_mut().set(RouteId::ALL[0], r);
    let k = FilterKind::Ladder;
    let steady = k.cost(K2040Voice::Rev2);
    assert_eq!(kit_bill(&kit), steady + k.move_cost() + PAD_MOD_COST);
}

#[test]
fn a_volume_route_or_a_shaped_amp_bills_mods_not_a_move() {
    let mut s = PadSound::DEFAULT;
    s.routes_mut().set(
        RouteId::ALL[1],
        PadRoute::new(
            RouteSource::Lfo,
            RouteDest::Volume,
            Amount::new(-30).unwrap(),
        ),
    );
    assert_eq!(s.cost(), PAD_MOD_COST);
    let mut s = PadSound::DEFAULT;
    *s.amp_mut().env_mut() = *PadSound::DEFAULT.env();
    assert!(!s.amp().env().is_open());
    assert_eq!(s.cost(), PAD_MOD_COST);
    assert_eq!(PadSound::DEFAULT.cost(), Cost::ZERO);
    assert!(PadEnv::OPEN.is_open());
}

#[test]
fn the_view_carries_each_parts_bill() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut kit = Kit::DEFAULT;
    kit.pad_mut(pad(12))
        .sound_mut()
        .filter_mut()
        .set_kind(FilterKind::Sem);
    let (p1, p2) = (PartId::ALL[0], PartId::ALL[1]);
    w.publish(p1, &kit);
    w.publish(p2, &Kit::DEFAULT);
    assert_eq!(KIT_VIEW.bill(p1), kit_bill(&kit));
    assert_eq!(KIT_VIEW.bill(p2), Cost::ZERO);
    w.publish(p1, &Kit::DEFAULT);
    assert_eq!(KIT_VIEW.bill(p1), Cost::ZERO);
}
