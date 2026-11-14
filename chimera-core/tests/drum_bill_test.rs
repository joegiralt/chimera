//! DRUM slice 2 (ADR 0269): a DRUM hit is billed for its kit's pads'
//! filters and mods, not its Sound's filter.
//!
//! A hit bills its pad's machine; a Part's bill, its kit's costliest
//! (#951).
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

mod common;

use std::sync::{LazyLock, Mutex};

use chimera_core::MidiChannel;
use chimera_core::budget::CONTROL_RESERVE;
use chimera_core::dsp::drum::sound::{
    Amount, HitKernel, KitBill, PAD_AMP_COST, PAD_ROUTED_COST, PadRoute, PadSound, RouteDest,
    RouteId, RouteSource, kit_bill,
};
use chimera_core::dsp::drum::types::{Kit, Machine, PadId};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::lpg::{self, LPG_COST};
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, Cost, MAX_VOICES, SampleBudget};
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

/// `kit`'s bill on `p` at `Voice::cost`'s kernel, the costliest machine,
/// so its filter and mods compare with `cost`'s.
fn drum(p: &ParamSnapshot, kit: &Kit) -> Cost {
    let b = Voice::drum_cost(p, &ModState::new(), kit_bill(kit));
    b.hit(HitKernel::Ended) + Machine::MAX_COST
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
    assert_eq!(Voice::drum_cost(&p, &m, KitBill::NONE).whole(), cost(&p));
    // Published: the pads' CLEAN, not the Sound's masked LADDER.
    assert_eq!(drum(&p, &Kit::DEFAULT), cost(&as_kind(&p, FilterKind::Svf)));
    assert!(drum(&p, &Kit::DEFAULT) < cost(&p));
}

/// INIT DRUM's pads struck alone on the unit, whole voice over idle,
/// cycles a sample (`.superpowers/sdd/2026-11-14-drum-perf/
/// unit-costs-9c2cfd1d.md`: rev V, build 9c2cfd1d, 2026-11-14). Pad 8, the
/// closed hat, ended inside the window and read nothing; pads 3 and 12
/// ended inside it too, so read low. JINGLE, SINE and NOISE are not in
/// INIT DRUM: unmeasured, so not here.
const READ: [(u8, u32); 11] = [
    (1, 800),
    (2, 800),
    (3, 1200),
    (4, 600),
    (5, 700),
    (6, 700),
    (7, 700),
    (9, 1400),
    (10, 1400),
    (11, 1400),
    (12, 300),
];

/// The unit's reading of INIT DRUM's pad `n`.
fn reading(n: PadId) -> Option<u32> {
    READ.iter()
        .find(|&&(p, _)| pad(p) == n)
        .map(|&(_, r)| r)
}

/// Each INIT DRUM pad, struck alone through the admission on rev V, is
/// booked at least its unit reading × 1.1 (ADR 0056's margin).
#[test]
fn every_pad_books_its_reading() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    for (n, read) in READ {
        let mut r = drum_rig(&mut w, &Kit::DEFAULT);
        let key = n - 1;
        r.note_on_vel(0, key, 127);
        let c = booked(&r, key).0;
        assert!(c * 10 >= read * 11, "pad {n}: {c} for {read} × 1.1");
    }
    for m in Machine::ALL {
        assert!(m.cost() <= Machine::MAX_COST);
    }
}

#[test]
fn a_part_bills_its_costliest_machine() {
    let kicks = kicks();
    assert_eq!(kit_bill(&kicks).kernel(), Machine::ToneKick.cost());
    assert_eq!(
        kit_bill(&Kit::DEFAULT).kernel(),
        Machine::PlainImpulse.cost()
    );
    let mut one = kicks;
    one.pad_mut(pad(7)).set_machine(Machine::MetalJingle);
    assert_eq!(kit_bill(&one).kernel(), Machine::MetalJingle.cost());
    // No kit read yet: the costliest of all, as `Voice::cost` bills.
    assert_eq!(KitBill::NONE.kernel(), Machine::MAX_COST);
    let p = drum_init();
    assert_eq!(
        Voice::drum_cost(&p, &ModState::new(), KitBill::NONE).whole(),
        cost(&p)
    );
    // A hit of a cheaper machine bills under its Part; an ended one, no
    // kernel.
    let part = Voice::drum_cost(&p, &ModState::new(), kit_bill(&Kit::DEFAULT));
    let kick = part.hit(HitKernel::Sounding(Machine::ToneKick));
    assert!(kick < part.whole());
    assert_eq!(
        part.hit(HitKernel::Sounding(Machine::PlainImpulse)),
        part.whole()
    );
    assert_eq!(part.hit(HitKernel::Ended) + Machine::ToneKick.cost(), kick);
}

/// INIT DRUM's 12 pads struck 60 ms apart, three times over, on rev V's
/// budget, each key up a block later: every sounding hit is booked at
/// least its pad's unit reading × 1.1 (`READ`), and only the hits the
/// one steal can't make room for are refused.
#[test]
fn init_drum_roll_stays_in_budget() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = drum_rig(&mut w, &Kit::DEFAULT);
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V);
    let gap = (0.06 * common::SR as f32) as usize / chimera_hal::BLOCK_SIZE;
    let reserved = FxBus::COST.0 + CONTROL_RESERVE.0;
    let (mut most, mut struck, mut refused) = (0, 0, vec![]);
    let tail = 4 * common::SR as usize / chimera_hal::BLOCK_SIZE;
    for blk in 0..36 * gap + tail {
        if blk % gap == 0 && struck < 36 {
            let was = r.inst.allocator().refused();
            r.note_on_vel(0, (struck % 12) as u8, 127);
            if r.inst.allocator().refused() != was {
                refused.push(struck);
            }
            struck += 1;
        }
        r.render(1);
        // A sequencer's gate: each key up a block after its strike.
        if blk % gap == 1 && struck <= 36 {
            r.note_off(0, ((struck - 1) % 12) as u8);
        }
        let slots = r.inst.allocator().slots();
        let mut n = 0;
        for v in 0..MAX_VOICES {
            let Some(h) = r.inst.voice(v).drum_hit() else {
                continue;
            };
            n += 1;
            let c = slots[v].cost().0;
            if let Some(read) = reading(h.pad()) {
                assert!(c * 10 >= read * 11, "block {blk}: {:?} {c} for {read}", h.pad());
            }
        }
        let live: u32 = slots.iter().filter(|s| !s.dying()).map(|s| s.cost().0).sum();
        assert!(live + reserved <= budget.as_cost().0, "block {blk}: live {live}");
        most = most.max(n);
    }
    // PLAIN IMPULSE (pad 3, billed at its high estimate) in rounds 2 and
    // 3: the oldest tail, a TOM, is the one voice a hit may steal, and it
    // frees too little beside the two metals still ringing.
    assert_eq!(refused, [14, 26], "the strikes refused");
    assert_eq!(r.inst.allocator().refused(), 2);
    assert!(most >= 4, "the roll keeps {most} hits");
}

/// Every pad on TONE KICK.
fn kicks() -> Kit {
    let mut k = Kit::DEFAULT;
    for id in PadId::ALL {
        k.pad_mut(id).set_machine(Machine::ToneKick);
    }
    k
}

/// Part 1 (channel 1) DRUM on rev V's budget, the others on channel 16,
/// every Part playing `kit`.
fn drum_rig(w: &mut KitWriter, kit: &Kit) -> common::InstRig {
    for id in PartId::ALL {
        w.publish(id, kit);
        let _ = w.take_hit(id);
    }
    let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix.channel = MidiChannel::new(if i == 0 { 0 } else { 15 }).unwrap();
    }
    *r.shared.parts[0].params = drum_init();
    r
}

/// Part 1's slot booked for `key`, not dying.
fn booked(r: &common::InstRig, key: u8) -> Cost {
    let s = r.inst.allocator().slots();
    let s = s
        .iter()
        .find(|s| s.part() == Some(0) && !s.dying() && s.note().map(|n| n.get()) == Some(key));
    s.expect("a slot for the key").cost()
}

/// A kit of KICKs, pad 1 switched to JINGLE and struck before the next
/// snapshot (the view moves between two of the audio's snapshots): its
/// hit bills JINGLE over the Part's rest, and a KICK beside it its own,
/// never under what a fresh snapshot bills.
#[test]
fn a_pad_switched_before_the_next_snapshot_bills_its_machine() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let kicks = kicks();
    let mut r = drum_rig(&mut w, &kicks);
    // Pad 2's KICK: the block's Part bills are worked out on the KICKs.
    r.note_on_vel(0, 1, 127);
    let mut jingle = kicks;
    jingle.pad_mut(pad(1)).set_machine(Machine::MetalJingle);
    w.publish(PartId::ALL[0], &jingle);
    r.note_on_vel(0, 0, 127);
    r.note_on_vel(0, 2, 127);
    let p = drum_init();
    let m = ModState::new();
    let want_jingle = Voice::drum_cost(&p, &m, kit_bill(&jingle)).whole();
    let want_kick = Voice::drum_cost(&p, &m, kit_bill(&kicks)).whole();
    assert!(booked(&r, 0) >= want_jingle, "JINGLE {:?} for {want_jingle:?}", booked(&r, 0));
    assert!(booked(&r, 2) >= want_kick, "KICK {:?} for {want_kick:?}", booked(&r, 2));
}

/// The pads sounding, by pad number.
fn sounding(r: &common::InstRig) -> Vec<u8> {
    let mut p: Vec<u8> = (0..MAX_VOICES)
        .filter_map(|v| r.inst.voice(v).drum_hit())
        .map(|h| h.pad().index() as u8 + 1)
        .collect();
    p.sort_unstable();
    p
}

/// `n` STACK KICKs a block apart on rev V, their key let go, then a
/// closed HAT: the pads heard over the next four blocks, and the notes
/// refused.
fn hat_after_kicks(w: &mut KitWriter, n: usize) -> (bool, u32) {
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(1)).set_retrig(Retrig::Stack);
    let mut r = drum_rig(w, &k);
    for _ in 0..n {
        r.note_on_vel(0, 0, 127);
        r.render(1);
    }
    r.note_off(0, 0);
    r.render(1);
    r.note_on_vel(0, 7, 127);
    let mut heard = false;
    for _ in 0..4 {
        r.render(1);
        heard |= sounding(&r).contains(&8);
    }
    (heard, r.inst.allocator().refused())
}

/// Rev V's budget, where `drum_hits_test` and `drum_e2e_test` play at
/// twice it: a fresh closed HAT among four KICK tails sounds; among eight
/// it is refused, as the one tail a hit may steal frees too little.
#[test]
fn a_hat_among_kick_tails_on_rev_v() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(hat_after_kicks(&mut w, 4), (true, 0));
    assert_eq!(hat_after_kicks(&mut w, 8), (false, 1));
}

/// INIT DRUM's 12 pads a block apart, keys held, on rev V's budget (the
/// e2e test plays them at twice it): every pad is heard but the CYMBAL
/// (pad 10), which waits on a stolen KICK and is dropped unheard when
/// the COWBELL's bill comes in a block later.
#[test]
fn twelve_pads_a_block_apart_on_rev_v() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = drum_rig(&mut w, &Kit::DEFAULT);
    let mut heard = [false; 12];
    for b in 0..15u8 {
        if b < 12 {
            r.note_on_vel(0, b, 127);
        }
        r.render(1);
        for p in sounding(&r) {
            heard[usize::from(p - 1)] = true;
        }
    }
    let unheard: Vec<usize> = (1..=12).filter(|&p| !heard[p - 1]).collect();
    assert_eq!(unheard, [10], "pads never heard");
    assert_eq!(r.inst.allocator().refused(), 1);
}
