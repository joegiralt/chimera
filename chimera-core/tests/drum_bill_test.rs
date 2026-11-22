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

use chimera_core::budget::CONTROL_RESERVE;
use chimera_core::dsp::drum::sound::{
    Amount, HitKernel, KitBill, PAD_AMP_COST, PAD_ROUTED_COST, PadMods, PadRoute, PadSound,
    RouteDest, RouteId, RouteSource, kit_bill,
};
use chimera_core::dsp::drum::types::{
    KernelBill, Kit, Knob, KnobValue, Machine, PadId, Ticking, Tonal,
};
use chimera_core::dsp::drum::view::{KIT_VIEW, KitWriter};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::lpg::{self, LPG_COST};
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{BlockBudget, CPU_HZ_REV_V, Cost, Cycles};
use chimera_core::modulation::ModState;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::perf::load::{CycleCounter, LoadNet};
use chimera_core::project::PartId;
use chimera_midi::value::U7;
use common::drum::{REV_V_BUDGET, drum_hits};

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
    b.hit(HitKernel::Ended) + KernelBill::MAX_COST
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
    assert_eq!(s.mods(), PadMods::Amp);
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
    assert_eq!(s.mods(), PadMods::Routed);
    let mut kit = Kit::DEFAULT;
    *kit.pad_mut(pad(1)).sound_mut() = s;
    let p = drum_init();
    assert_eq!(drum(&p, &kit).0, cost(&p).0 + PAD_ROUTED_COST.0);
    assert_eq!(PadSound::DEFAULT.mods(), PadMods::None);
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
/// INIT DRUM: unmeasured, so not here. Pads 3 and 11 re-read on eb442a48,
/// 2026-11-16 (ADR 0275); pad 11 at INIT's BUMP 0 (ADR 0287).
const READ: [(u8, u32); 11] = [
    (1, 800),
    (2, 800),
    (3, 600),
    (4, 600),
    (5, 700),
    (6, 700),
    (7, 700),
    (9, 1400),
    (10, 1400),
    (11, 700),
    (12, 300),
];

/// The unit's reading of INIT DRUM's pad `n`.
fn reading(n: PadId) -> Option<u32> {
    READ.iter().find(|&&(p, _)| pad(p) == n).map(|&(_, r)| r)
}

/// Each INIT DRUM pad, struck alone through the admission on rev V, is
/// booked at least its unit reading × 1.1 (ADR 0056's margin).
#[test]
fn every_pad_books_its_reading() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    for (n, read) in READ {
        let mut r = drum_rig(&mut w, &Kit::DEFAULT);
        let k = key(pad(n));
        r.note_on_vel(0, k, 127);
        let c = booked(&r, k).0;
        assert!(c * 10 >= read * 11, "pad {n}: {c} for {read} × 1.1");
    }
}

/// A machine the unit read bills `ceil(1.1 × reading) − 83` (ADR 0274),
/// exactly: nothing high, nothing low. Pad 12 (BODY SHAKER) read low and
/// is an estimate, so it is out.
#[test]
fn a_read_machine_bills_its_reading() {
    for (n, read) in READ.into_iter().filter(|&(n, _)| n != 12) {
        let m = Kit::DEFAULT.pad(pad(n)).machine();
        assert_eq!(m.cost().0, (read * 11).div_ceil(10) - 83, "pad {n}");
    }
    // COWBELL's BUMP 127 worst block, read the same day (ADR 0287).
    assert_eq!(Ticking::CowbellSwept.cost().0, billed(2000));
}

#[test]
fn a_part_bills_its_costliest_machine() {
    let kicks = kicks();
    assert_eq!(kit_bill(&kicks).kernel(), Machine::ToneKick.cost());
    // A Part bills the worst its machines can run up, at any knobs and
    // pitch: INIT's IMPULSE floored and swept, a COWBELL bumped.
    assert_eq!(
        kit_bill(&Kit::DEFAULT).kernel(),
        Ticking::ImpulseFlooredSwept.cost()
    );
    let mut bell = kicks;
    bell.pad_mut(pad(11)).set_machine(Machine::ToneCowbell);
    assert_eq!(kit_bill(&bell).kernel(), Ticking::CowbellSwept.cost());
    let mut one = kicks;
    one.pad_mut(pad(7)).set_machine(Machine::MetalJingle);
    assert_eq!(kit_bill(&one).kernel(), Machine::MetalJingle.cost());
    // No kit read yet: the costliest of all, as `Voice::cost` bills.
    assert_eq!(KitBill::NONE.kernel(), KernelBill::MAX_COST);
    let p = drum_init();
    assert_eq!(
        Voice::drum_cost(&p, &ModState::new(), KitBill::NONE).whole(),
        cost(&p)
    );
    // A hit of a cheaper machine bills under its Part; an ended one, no
    // kernel.
    let part = Voice::drum_cost(&p, &ModState::new(), kit_bill(&Kit::DEFAULT));
    let kick = part.hit(HitKernel::Sounding(KernelBill::Machine(Machine::ToneKick)));
    assert!(kick < part.whole());
    assert_eq!(
        part.hit(HitKernel::Sounding(KernelBill::Ticking(
            Ticking::ImpulseFlooredSwept
        ))),
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
    let budget = REV_V_BUDGET;
    let gap = (0.06 * common::SR as f32) as usize / chimera_hal::BLOCK_SIZE;
    let reserved = FxBus::COST.0 + CONTROL_RESERVE.0;
    let (mut most, mut struck, mut refused) = (0, 0, vec![]);
    let tail = 4 * common::SR as usize / chimera_hal::BLOCK_SIZE;
    for blk in 0..36 * gap + tail {
        if blk % gap == 0 && struck < 36 {
            let was = r.inst.allocator().refused();
            r.note_on_vel(0, key(PadId::ALL[struck % 12]), 127);
            if r.inst.allocator().refused() != was {
                refused.push(struck);
            }
            struck += 1;
        }
        r.render(1);
        // A sequencer's gate: each key up a block after its strike.
        if blk % gap == 1 && struck <= 36 {
            r.note_off(0, key(PadId::ALL[(struck - 1) % 12]));
        }
        let slots = r.inst.allocator().slots();
        let mut n = 0;
        for (v, h) in drum_hits(&r.inst) {
            n += 1;
            let c = slots[v].cost().0;
            if let Some(read) = reading(h.pad()) {
                assert!(
                    c * 10 >= read * 11,
                    "block {blk}: {:?} {c} for {read}",
                    h.pad()
                );
            }
        }
        let live: u32 = slots
            .iter()
            .filter(|s| !s.dying())
            .map(|s| s.cost().0)
            .sum();
        assert!(
            live + reserved <= budget.as_cost().0,
            "block {blk}: live {live}"
        );
        most = most.max(n);
    }
    // PLAIN IMPULSE read 600 (ADR 0275), no longer billed beside the
    // metals' 1,457: no strike is refused.
    assert_eq!(refused, [], "the strikes refused");
    assert_eq!(r.inst.allocator().refused(), 0);
    assert!(most >= 4, "the roll keeps {most} hits");
}

/// A second of blocks.
const SR_BLOCKS: usize = common::SR as usize / chimera_hal::BLOCK_SIZE;

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
    common::drum::drum_rig(w, REV_V_BUDGET, kit, kit)
}

/// Part 1's slot booked for `key`, not dying.
fn slot_for(r: &common::InstRig, key: u8) -> Option<usize> {
    r.inst.allocator().slots().iter().position(|s| {
        s.part() == Some(PartId::ALL[0]) && !s.dying() && s.note().map(|n| n.get()) == Some(key)
    })
}

/// What Part 1's slot for `key` books.
fn booked(r: &common::InstRig, key: u8) -> Cost {
    let v = slot_for(r, key).expect("a slot for the key");
    r.inst.allocator().slots()[v].cost()
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
    r.note_on_vel(0, key(pad(2)), 127);
    let mut jingle = kicks;
    jingle.pad_mut(pad(1)).set_machine(Machine::MetalJingle);
    w.publish(PartId::ALL[0], &jingle);
    r.note_on_vel(0, key(pad(1)), 127);
    r.note_on_vel(0, key(pad(3)), 127);
    let p = drum_init();
    let m = ModState::new();
    let want_jingle = Voice::drum_cost(&p, &m, kit_bill(&jingle)).whole();
    let want_kick = Voice::drum_cost(&p, &m, kit_bill(&kicks)).whole();
    assert!(
        booked(&r, 0) >= want_jingle,
        "JINGLE {:?} for {want_jingle:?}",
        booked(&r, 0)
    );
    assert!(
        booked(&r, 2) >= want_kick,
        "KICK {:?} for {want_kick:?}",
        booked(&r, 2)
    );
}

/// Pad 5 on LADDER struck, then the kit republished with it on CLEAN: the
/// hit keeps the KIND it was struck on (`PadSound::live_target`), so it
/// keeps LADDER's bill for the rest of its ring, never CLEAN's (#977).
#[test]
fn a_ringing_hit_bills_the_filter_it_runs() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let ladder = pad5_on(FilterKind::Ladder);
    let mut r = drum_rig(&mut w, &ladder);
    r.note_on_vel(0, key(pad(5)), 127);
    r.render(1);
    let struck = booked(&r, key(pad(5)));
    w.publish(PartId::ALL[0], &Kit::DEFAULT);
    r.render(2);
    assert!(sounding(&r).contains(&pad(5)), "pad 5 still rings");
    assert_eq!(booked(&r, key(pad(5))), struck);
}

/// Pad 5 ringing when a publish moves its CUTOFF: the hit follows (ADR
/// 0269), so its block decodes the pad and starts the glide, and the
/// glide steps the filter and rebuilds the envelopes. Each is booked
/// beside the hit, as a knob's glide (#962).
#[test]
fn a_followed_publish_books_its_hits_decode_and_glide() {
    use chimera_core::dsp::Hz;
    use chimera_core::dsp::drum::engine::{FOLLOW_DECODE_COST, FOLLOW_GLIDE_COST};
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = drum_rig(&mut w, &Kit::DEFAULT);
    r.note_on_vel(0, key(pad(5)), 127);
    r.render(2);
    let rest = booked(&r, key(pad(5)));
    let mut kit = Kit::DEFAULT;
    let f = kit.pad_mut(pad(5)).sound_mut().filter_mut();
    f.set_cutoff(Hz::new(f.cutoff().get() / 4.0));
    w.publish(PartId::ALL[0], &kit);
    let glide = FOLLOW_GLIDE_COST + FilterKind::Svf.glide_cost();
    r.render(1);
    assert_eq!(booked(&r, key(pad(5))), rest + FOLLOW_DECODE_COST + glide);
    r.render(1);
    assert!(sounding(&r).contains(&pad(5)), "pad 5 still rings");
    assert_eq!(booked(&r, key(pad(5))), rest + glide);
}

/// A kit of DIRTY pads under a CLEAN Sound: the pads mask the Sound's
/// filter, so a knob glide bills DIRTY's step (0), not CLEAN's (#962),
/// as `drum_cost` bills the pads' shapes alone.
#[test]
fn a_kits_glide_bills_its_pads_not_the_masked_sound() {
    use chimera_core::dsp::knobs::Gliding;
    let mut kit = Kit::DEFAULT;
    for n in 1..=12 {
        kit.pad_mut(pad(n))
            .sound_mut()
            .filter_mut()
            .set_kind(FilterKind::Dirty);
    }
    let g = Gliding {
        filter: true,
        fx: false,
    };
    let p = drum_init();
    let extra = Voice::drum_glide_extra(&p, &ModState::new(), g, kit_bill(&kit));
    assert_eq!(extra, Cost::ZERO);
}

/// The MIDI key INIT DRUM's pad `p` answers to.
fn key(p: PadId) -> u8 {
    p.index() as u8
}

/// The pads sounding.
fn sounding(r: &common::InstRig) -> Vec<PadId> {
    let mut p: Vec<PadId> = drum_hits(&r.inst).map(|(_, h)| h.pad()).collect();
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
        r.note_on_vel(0, key(pad(1)), 127);
        r.render(1);
    }
    r.note_off(0, 0);
    r.render(1);
    r.note_on_vel(0, key(pad(8)), 127);
    let mut heard = false;
    for _ in 0..4 {
        r.render(1);
        heard |= sounding(&r).contains(&pad(8));
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
/// e2e test plays them at twice it). At default the CYMBAL (pad 10) books
/// by stealing a KICK and waits out its fade, and the COWBELL (pad 11),
/// in its choke group D, chokes it unheard a block later: one choke, no
/// budget refusal. With `master-tape` the bus bills 1494, not 1190, and
/// the CYMBAL is refused at admission; the COWBELL at INIT's BUMP 0 bills
/// its plateau (ADR 0287) and plays: one refusal, no choke. Each unheard
/// pad is counted refused either way.
#[test]
fn twelve_pads_a_block_apart_on_rev_v() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = drum_rig(&mut w, &Kit::DEFAULT);
    let mut heard = [false; 12];
    let (mut chokes, mut refusals) = (0, 0);
    for b in 0..15u8 {
        if b < 12 {
            let was = r.inst.allocator().refused();
            r.note_on_vel(0, key(PadId::ALL[usize::from(b)]), 127);
            if r.inst.allocator().refused() != was {
                // A refused hit books nothing; a choke drops an earlier one.
                if slot_for(&r, b).is_some() {
                    chokes += 1;
                } else {
                    refusals += 1;
                }
            }
        }
        r.render(1);
        for p in sounding(&r) {
            heard[p.index()] = true;
        }
    }
    let unheard: Vec<u8> = PadId::ALL
        .into_iter()
        .filter(|p| !heard[p.index()])
        .map(PadId::number)
        .collect();
    let (want_unheard, want_chokes, want_refusals): (&[u8], u32, u32) =
        if cfg!(feature = "master-tape") {
            (&[10], 0, 1)
        } else {
            (&[10], 1, 0)
        };
    assert_eq!(unheard, want_unheard, "pads never heard");
    assert_eq!((chokes, refusals), (want_chokes, want_refusals));
    assert_eq!(
        r.inst.allocator().refused(),
        want_chokes + want_refusals,
        "refused"
    );
}

/// The load net (ADR 0273) sheds only a sounding voice. A closed HAT
/// that ended with its key still held has freed its slot (a hit ends with
/// its voice, ADR 0281's ruling): there is nothing to shed or count. A
/// ringing KICK is.
#[test]
fn the_load_net_sheds_only_a_sounding_voice() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut r = drum_rig(&mut w, &Kit::DEFAULT);
    r.note_on_vel(0, key(pad(8)), 127);
    r.render(SR_BLOCKS / 2);
    assert_eq!(
        slot_for(&r, key(pad(8))),
        None,
        "the HAT ended: its slot is free, its key held or not"
    );
    let block = BlockBudget::for_cpu(CPU_HZ_REV_V);
    let counter = CycleCounter::new(true).expect("a counting DWT");
    let net = LoadNet::new(counter, block);
    let over = Cycles::new(block.block_cycles());
    assert!(
        !net.after(over, &mut r.inst),
        "nothing shed, nothing counted"
    );
    r.note_on_vel(0, key(pad(1)), 127);
    r.render(1);
    assert!(net.after(over, &mut r.inst), "one shed, counted once");
    let s = r.inst.allocator().slots();
    assert!(
        s.iter()
            .any(|s| s.dying() && s.note().map(|n| n.get()) == Some(0))
    );
}

/// ADR 0273's skip rule: the load net sheds only a voice that sounds and
/// isn't already fading. Part 1 fills the pool; Part 2's note steals Part
/// 1's oldest voice, which fades out on Part 1's Sound with Part 2's note
/// waiting on it: booked, the newest held, so the shed order would take it
/// first, but its fade frees nothing yet. The net passes over it and
/// sheds Part 1's newest note.
#[test]
fn the_load_net_skips_a_booked_voice_still_fading() {
    let mut r = common::InstRig::with_budget(chimera_core::hw::SampleBudget::for_cpu(u32::MAX));
    r.shared.parts[1].mix.channel = chimera_core::MidiChannel::new(1).unwrap();
    for n in 0..8 {
        r.note_on(0, 60 + n);
        r.render(1);
    }
    r.note_on(1, 40);
    let slots = r.inst.allocator().slots();
    let stolen = slots
        .iter()
        .position(|s| s.part() == Some(PartId::ALL[1]))
        .expect("Part 2's note took a voice");
    assert!(
        r.inst.voice(stolen).fading(),
        "it fades out Part 1's note first"
    );
    assert!(slots[stolen].held() && !slots[stolen].dying());
    let block = BlockBudget::for_cpu(CPU_HZ_REV_V);
    let net = LoadNet::new(CycleCounter::new(true).expect("a counting DWT"), block);
    assert!(
        net.after(Cycles::new(block.block_cycles()), &mut r.inst),
        "one shed"
    );
    let s = r.inst.allocator().slots();
    assert!(!s[stolen].dying(), "the fading voice is passed over");
    let shed: Vec<usize> = (0..s.len()).filter(|&v| s[v].dying()).collect();
    assert_eq!(shed.len(), 1);
    assert_eq!(
        s[shed[0]].note().map(|n| n.get()),
        Some(67),
        "Part 1's newest"
    );
}

/// What Part 1's hit on `key` books for its kernel: its booking less a
/// lone KICK's on the same kit, plus the KICK's machine.
fn kernel_booked(w: &mut KitWriter, kit: &Kit, key: u8, after: usize) -> Option<u32> {
    let mut k = drum_rig(w, kit);
    k.note_on_vel(0, self::key(pad(1)), 127);
    let rest = booked(&k, self::key(pad(1))).0 - Machine::ToneKick.cost().0;
    let mut r = drum_rig(w, kit);
    r.note_on_vel(0, key, 127);
    r.render(after);
    slot_for(&r, key).map(|v| r.inst.allocator().slots()[v].cost().0 - rest)
}

/// ADR 0274's bill for a reading: `ceil(1.1 × reading) − 83`.
const fn billed(read: u32) -> u32 {
    (read * 11).div_ceil(10) - 83
}

/// A knob at `v`.
fn at(v: u8) -> KnobValue {
    KnobValue::new(U7::new(v).expect("0..=127"))
}

/// #991: a COWBELL at BUMP 127 is booked its sweep head's worst block
/// (about 2,000 over idle on the unit, 2026-11-16) while its pulses render
/// a sample at a time, then its plateau (600–700) once the sweep is
/// spent; at BUMP 0 (INIT) its plateau from the strike.
#[test]
fn a_bumped_cowbell_bills_its_head_then_its_plateau() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let bell = key(pad(11));
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(11)).knobs_mut().set(Knob::B, at(127));
    assert_eq!(kernel_booked(&mut w, &k, bell, 0), Some(billed(700)));
    k.pad_mut(pad(11)).knobs_mut().set(Knob::H, at(127));
    assert_eq!(kernel_booked(&mut w, &k, bell, 0), Some(billed(2000)));
    assert_eq!(kernel_booked(&mut w, &k, bell, 1), Some(billed(2000)));
    // 200 ms on, the 6 ms bump is spent.
    assert_eq!(
        kernel_booked(&mut w, &k, bell, SR_BLOCKS / 5),
        Some(billed(700))
    );
}

/// #991: a PLAIN IMPULSE at SWEEP 64 is booked its sweeping pulse
/// (estimated 1,300) until the sweep is spent, then its plateau (600).
#[test]
fn a_swept_impulse_bills_its_sweep_then_its_plateau() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let imp = key(pad(3));
    let mut k = Kit::DEFAULT;
    k.pad_mut(pad(3)).knobs_mut().set(Knob::B, at(127));
    assert_eq!(kernel_booked(&mut w, &k, imp, 0), Some(billed(600)));
    k.pad_mut(pad(3)).knobs_mut().set(Knob::C, at(64));
    assert_eq!(kernel_booked(&mut w, &k, imp, 0), Some(billed(1300)));
    assert_eq!(
        kernel_booked(&mut w, &k, imp, SR_BLOCKS / 5),
        Some(billed(600))
    );
}

/// #991: an IMPULSE whose WIDTH floors at its pitch renders a sample at a
/// time for as long as it sounds (`Pulse::sweep`), at full SWEEP dearer
/// still, and a sweep floors it while its head is high enough: TONAL on
/// it, 2 kHz (an octave up) at full WIDTH. Desktop timing
/// puts it near a 2,000 reading, 2,700 with SWEEP 127, both estimates
/// pending the unit (ADR 0287).
#[test]
fn a_floored_impulse_bills_its_floor_and_its_sweep() {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    let mut k = Kit::DEFAULT;
    k.set_tonal(Tonal::Pad(pad(3)));
    let knobs = k.pad_mut(pad(3)).knobs_mut();
    for (kn, v) in [(Knob::A, 127), (Knob::B, 127), (Knob::G, 127)] {
        knobs.set(kn, at(v));
    }
    assert_eq!(kernel_booked(&mut w, &k, 60, 0), Some(billed(600)));
    assert_eq!(kernel_booked(&mut w, &k, 72, 0), Some(billed(2000)));
    assert_eq!(
        kernel_booked(&mut w, &k, 72, SR_BLOCKS / 5),
        Some(billed(2000))
    );
    let knobs = k.pad_mut(pad(3)).knobs_mut();
    knobs.set(Knob::C, at(127));
    knobs.set(Knob::D, at(30));
    assert_eq!(kernel_booked(&mut w, &k, 72, 0), Some(billed(2700)));
    assert_eq!(
        kernel_booked(&mut w, &k, 72, SR_BLOCKS / 2),
        Some(billed(2000))
    );
    // At its tuned pitch the sweep's head floors it, its tail doesn't.
    assert_eq!(kernel_booked(&mut w, &k, 60, 0), Some(billed(2700)));
    assert_eq!(
        kernel_booked(&mut w, &k, 60, SR_BLOCKS / 2),
        Some(billed(600))
    );
    // The dearest bill there is: a DRUM voice's with no kit read.
    assert_eq!(KernelBill::MAX_COST.0, billed(2700));
}
