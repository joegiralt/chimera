//! DRUM slice 2 (ADR 0269): a DRUM hit is billed for its kit's pads'
//! filters and mods, not its Sound's filter.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

use std::sync::{LazyLock, Mutex};

use chimera_core::dsp::drum::sound::{
    Amount, KitBill, PAD_AMP_COST, PAD_ROUTED_COST, PadRoute, PadSound, RouteDest, RouteId,
    RouteSource, kit_bill,
};
use chimera_core::dsp::drum::types::{Kit, PadId};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::lpg::{self, LPG_COST};
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

fn cost(p: &ParamSnapshot) -> Cost {
    Voice::cost(p, &ModState::new())
}

fn drum(p: &ParamSnapshot, kit: &Kit) -> Cost {
    Voice::drum_cost(p, &ModState::new(), kit_bill(kit))
}

/// `p` as its pad plays it: the same Sound on the pad's kind.
fn as_kind(p: &ParamSnapshot, k: FilterKind) -> ParamSnapshot {
    let mut q = p.clone();
    q.filter.set_kind(k);
    q
}

/// `Kit::DEFAULT` with pad 5 on `k`.
fn pad5_on(k: FilterKind) -> Kit {
    let mut kit = Kit::DEFAULT;
    kit.pad_mut(pad(5)).sound_mut().filter_mut().set_kind(k);
    kit
}

#[test]
fn a_2040_pad_bills_above_svf() {
    let p = drum_init();
    assert_eq!(p.filter.kind(), FilterKind::Svf);
    let kit = pad5_on(FilterKind::K2040);
    assert!(drum(&p, &kit) > cost(&p));
    assert!(drum(&p, &kit) >= cost(&as_kind(&p, FilterKind::K2040)));
}

#[test]
fn default_kit_bills_as_slice1() {
    let p = drum_init();
    assert_eq!(drum(&p, &Kit::DEFAULT), cost(&p));
    // A loaded slice-1 kit: every pad its Sound's filter.
    let mut p = drum_init();
    p.filter.set_kind(FilterKind::Ladder);
    p.filter.cutoff = 420.0;
    p.filter.level = 0.8;
    p.filter.stability = 0.5;
    p.out.gate = [0.4, 0.2, 0.0];
    let mut kit = Kit::DEFAULT;
    for id in PadId::ALL {
        *kit.pad_mut(id).sound_mut() = PadSound::from_sound(&p);
    }
    assert_ne!(cost(&p), cost(&drum_init()));
    assert_eq!(drum(&p, &kit), cost(&p));
}

#[test]
fn a_chip_pad_under_an_unstable_svf_bills_stab() {
    let mut p = drum_init();
    p.filter.stability = 0.5;
    for k in [FilterKind::Ladder, FilterKind::K2040, FilterKind::Sem] {
        let want = cost(&as_kind(&p, k));
        assert!(drum(&p, &pad5_on(k)) >= want, "{k:?}");
    }
}

#[test]
fn a_clean_pad_under_a_hot_chip_bills_its_level() {
    for k in [FilterKind::Ladder, FilterKind::K2040, FilterKind::Sem] {
        let mut p = as_kind(&drum_init(), k);
        p.filter.level = 0.8;
        for pk in [FilterKind::Svf, FilterKind::Dirty] {
            let mut kit = Kit::DEFAULT;
            for id in PadId::ALL {
                kit.pad_mut(id).sound_mut().filter_mut().set_kind(pk);
            }
            let want = cost(&as_kind(&p, pk));
            assert!(drum(&p, &kit) >= want, "{k:?} Sound, {pk:?} pads");
        }
    }
}

#[test]
fn a_moving_dirty_sound_bills_its_pads_move() {
    let mut p = as_kind(&drum_init(), FilterKind::Dirty);
    p.filter.fb = 0.3;
    for k in [FilterKind::Ladder, FilterKind::Sem, FilterKind::K2040] {
        let want = cost(&as_kind(&p, k));
        assert!(drum(&p, &pad5_on(k)) >= want, "{k:?}");
    }
    let mut kit = Kit::DEFAULT;
    for id in PadId::ALL {
        kit.pad_mut(id)
            .sound_mut()
            .filter_mut()
            .set_kind(FilterKind::Svf);
    }
    assert!(drum(&p, &kit) >= cost(&as_kind(&p, FilterKind::Svf)));
}

#[test]
fn a_routed_pad_bills_its_move() {
    let p = drum_init();
    let mut kit = pad5_on(FilterKind::Ladder);
    let r = PadRoute::new(
        RouteSource::ModEnv,
        RouteDest::Cutoff,
        Amount::new(64).unwrap(),
    );
    kit.pad_mut(pad(5))
        .sound_mut()
        .routes_mut()
        .set(RouteId::ALL[0], r);
    let want = cost(&as_kind(&p, FilterKind::Ladder)).0
        + FilterKind::Ladder.move_cost().0
        + PAD_ROUTED_COST.0;
    assert_eq!(drum(&p, &kit).0, want);
}

#[test]
fn an_amp_only_pad_bills_its_amp() {
    let p = drum_init();
    let mut kit = Kit::DEFAULT;
    let s = kit.pad_mut(pad(2)).sound_mut();
    *s.amp_mut().env_mut() = *PadSound::DEFAULT.env();
    assert!(!s.amp().env().is_open());
    assert_eq!(s.mod_cost(), PAD_AMP_COST);
    assert_eq!(drum(&p, &kit).0, cost(&p).0 + PAD_AMP_COST.0);
}

#[test]
fn a_volume_route_bills_mods_not_a_move() {
    let mut s = PadSound::DEFAULT;
    s.routes_mut().set(
        RouteId::ALL[1],
        PadRoute::new(
            RouteSource::Lfo,
            RouteDest::Volume,
            Amount::new(-30).unwrap(),
        ),
    );
    assert_eq!(s.mod_cost(), PAD_ROUTED_COST);
    let mut kit = Kit::DEFAULT;
    *kit.pad_mut(pad(1)).sound_mut() = s;
    let p = drum_init();
    assert_eq!(drum(&p, &kit).0, cost(&p).0 + PAD_ROUTED_COST.0);
    assert_eq!(PadSound::DEFAULT.mod_cost(), Cost::ZERO);
}

#[test]
fn the_view_carries_each_parts_bill() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let kit = pad5_on(FilterKind::Sem);
    let (p1, p2) = (PartId::ALL[0], PartId::ALL[1]);
    w.publish(p1, &kit);
    w.publish(p2, &Kit::DEFAULT);
    assert_eq!(KIT_VIEW.bill(p1), kit_bill(&kit));
    assert_eq!(KIT_VIEW.bill(p2), kit_bill(&Kit::DEFAULT));
    assert_ne!(kit_bill(&kit), kit_bill(&Kit::DEFAULT));
    w.publish(p1, &Kit::DEFAULT);
    assert_eq!(KIT_VIEW.bill(p1), kit_bill(&Kit::DEFAULT));
}

#[test]
fn a_pads_gate_bills_the_lpg() {
    let p = drum_init();
    assert!(!p.out.gated());
    let mut kit = Kit::DEFAULT;
    kit.pad_mut(pad(3))
        .sound_mut()
        .amp_mut()
        .set_gate(lpg::Knobs::of([0.5, 0.0, 0.0]));
    assert!(kit_bill(&kit).gated());
    assert!(drum(&p, &kit).0 >= drum(&p, &Kit::DEFAULT).0 + LPG_COST.0);
    // The Sound's gate is the pads' on DRUM: a gated Sound over open pads
    // runs none.
    let mut g = drum_init();
    g.out.gate = [0.5, 0.0, 0.0];
    assert_eq!(drum(&g, &Kit::DEFAULT), drum(&p, &Kit::DEFAULT));
}

#[test]
fn the_sounds_shape_bills_only_before_a_publish() {
    let p = as_kind(&drum_init(), FilterKind::Ladder);
    let m = ModState::new();
    assert_eq!(Voice::drum_cost(&p, &m, KitBill::NONE), cost(&p));
    // Published: the pads' CLEAN, not the Sound's masked LADDER.
    assert_eq!(drum(&p, &Kit::DEFAULT), cost(&as_kind(&p, FilterKind::Svf)));
    assert!(drum(&p, &Kit::DEFAULT) < cost(&p));
}
