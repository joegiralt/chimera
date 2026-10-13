//! CPU costs (ADR 0013): cycles/sample per voice, measured on the bench
//! (rev V at 480 MHz, 2026-09-27) and rounded up.

mod common;

use chimera_core::budget::{CONTROL_RESERVE, FORMANT_N, FORMANT_N_UNLINKED, HEAVY_OVERHEAD};
use chimera_core::dsp::algo::algorithms::{ALGO_COUNT, AlgoId};
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::kinds::{Linked, OpKind, fits};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::filter::ladder::{LADDER_COST, LADDER_MOVE_COST};
use chimera_core::dsp::filter::{FilterKind, FilterMode, K2040Voice};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{
    CPU_HZ_REV_V, CPU_HZ_REV_Y, Cost, FLOOR_VOICES, FLOOR_VOICES_FORMANT, MAX_VOICES, SampleBudget,
};
use chimera_core::instrument::{AudioShared, NoteOrigin};
use chimera_core::modulation::{FixedCol, ModRouting, ModSource, ModState, VCA};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::taps::TapOp;

const UNROUTED: [bool; OPS] = [false; OPS];

fn cost(p: &AlgoParams) -> u32 {
    AlgoEngine::cost(p, &UNROUTED).0
}

fn voice_cost(e: EngineType) -> Cost {
    Voice::cost(&ParamSnapshot::for_engine(e), &ModState::new())
}

/// The bench's worst case: A14 and A22 mid-MORPH, six operators, all with
/// feedback.
fn worst() -> AlgoParams {
    let mut p = AlgoParams::default();
    p.try_set_algs(AlgoId::A14, AlgoId::A22).unwrap();
    p.morph = 64;
    for op in p.ops.iter_mut() {
        (op.level, op.feedback) = (99, 7);
    }
    p
}

/// The Modal Sound, STRING at BODY 0.3: `COST_STRING` 330 and `BODY` 80,
/// host estimates until the bench (Modal 2 step A, task 12), plus
/// `CHAIN_COST` (30 since ADR 0060, 36 with ADR 0110's amp gate). Algo is priced from its patch.
/// Each engine's default filter bills over the LP24 its term carries (VA's
/// 2040 REV2, 15).
#[test]
fn voice_costs_are_the_billed_literals() {
    assert_eq!(Voice::CHAIN_COST, Cost(36));
    assert_eq!(
        voice_cost(EngineType::Modal),
        Cost(446) + chimera_core::dsp::modal::ModalEngine::PITCH + ModRouting::BASE + LP24
    );
    let mods = ModState::new();
    for e in EngineType::ALL {
        let p = ParamSnapshot::for_engine(e);
        assert_eq!(
            Voice::cost(&p, &mods),
            EngineSlot::cost(&p, &mods)
                + Voice::CHAIN_COST
                + p.filter.filter_cost()
                + ModRouting::BASE,
            "{e:?}"
        );
    }
}

/// The SVF at LP24, the default: the engine terms already carry it.
const LP24: Cost = FilterKind::Svf.cost(K2040Voice::Rev2);

/// Every built (TYPE, MODE, VOICE) bills `max(0, cycles − OLD_LP24)`, as
/// counted on the bench ELF (plan § Measured, Task 15): CLEAN 35–80 and
/// DIRTY 70 sit under LP24's 95; the 2040 REV2 110, EXP 135; LADDER 210
/// (ADR 0104), billed its costliest mode.
#[test]
fn filter_bills_each_type_and_mode() {
    use FilterKind as K;
    let mut seen = 0;
    for k in FilterKind::BUILT {
        for &m in k.modes() {
            for v in K2040Voice::ALL {
                let want = match (k, v) {
                    (K::Svf | K::Dirty, _) => 0,
                    (K::K2040, K2040Voice::Rev2) => 15,
                    (K::K2040, K2040Voice::Exp) => 40,
                    (K::Ladder, _) => LADDER_COST.0,
                };
                assert_eq!(k.cost(v), Cost(want), "{k:?} {m:?} {v:?}");
                let mut p = chimera_core::params::FilterParams::default();
                p.set_kind(k);
                assert!(p.set_mode(m));
                p.voice = v;
                assert_eq!(p.filter_cost(), Cost(want), "{k:?} {m:?} {v:?}");
                seen += 1;
            }
        }
    }
    assert_eq!(seen, (8 + 1 + 4 + 5) * 2, "every built pair, each VOICE");
    // Over OLD_LP24 (95): the 2040's and LADDER's steady counts with the
    // block (LADDER's one loop for every mode and its input scan, 210).
    assert!(K::Ladder.cost(K2040Voice::Rev2) >= Cost(210 - 95));
    assert!(K::K2040.cost(K2040Voice::Rev2) >= Cost(110 - 95));
    assert!(K::K2040.cost(K2040Voice::Exp) >= Cost(135 - 95));
}

/// A snapshot on `kind` (its default mode), ALGO.
fn on(kind: FilterKind) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.filter.set_kind(kind);
    p
}

/// LFO 1 into the FREQ column at `amount`.
fn cutoff_lfo(amount: i8) -> ModState {
    let mut ms = ModState::with_sources(8);
    ms.set_fixed(ModSource::Lfo1.index(), FixedCol::Freq, amount);
    ms
}

/// The moving path bills once a block moves, however many things move it:
/// a CUTOFF or RES route, a FREQ or a Q tap. Each figure is the moving
/// loop with its block (and `ramps`' fill) less the steady one, counted:
/// CLEAN LP4 150 − 75, the 2040 REV2 170 − 110 (EXP 190 − 135), DIRTY's
/// one loop 0; `TapShape::apply` 48.45 instructions, 80.
#[test]
fn the_moving_path_bills_once() {
    use chimera_core::dsp::filter::{CLEAN_MOVE_COST, K2040_MOVE_COST};
    use chimera_core::taps::TAP_SHAPE_COST;
    assert!(CLEAN_MOVE_COST >= Cost(150 - 75));
    assert!(K2040_MOVE_COST >= Cost(170 - 110) && K2040_MOVE_COST >= Cost(190 - 135));
    assert!(LADDER_MOVE_COST >= Cost(280 - 210));
    assert!(TAP_SHAPE_COST >= Cost(80));
    let none = ModState::new();
    for (k, mv) in [
        (FilterKind::Svf, CLEAN_MOVE_COST),
        (FilterKind::K2040, K2040_MOVE_COST),
        (FilterKind::Dirty, Cost::ZERO),
        (FilterKind::Ladder, LADDER_MOVE_COST),
    ] {
        assert_eq!(k.move_cost(), mv, "{k:?}");
        let mut p = on(k);
        let steady = Voice::filter_cost(&p, &none);
        assert_eq!(steady, p.filter.filter_cost(), "{k:?}");
        assert_eq!(Voice::filter_cost(&p, &cutoff_lfo(0)), steady, "{k:?} 0");
        assert_eq!(
            Voice::filter_cost(&p, &cutoff_lfo(64)),
            steady + mv,
            "{k:?}"
        );
        let mut res = ModState::with_sources(8);
        res.set_fixed(ModSource::Lfo2.index(), FixedCol::Q, -3);
        assert_eq!(Voice::filter_cost(&p, &res), steady + mv, "{k:?} RES");
        // Taps: the move once, `apply` once, however many taps.
        p.taps.set(TapOp::Op5, FixedCol::Freq, 40);
        assert_eq!(
            Voice::filter_cost(&p, &none),
            steady + mv + TAP_SHAPE_COST,
            "{k:?}"
        );
        p.taps.set(TapOp::Op6, FixedCol::Q, -9);
        p.taps.set(TapOp::Op5, FixedCol::Q, 9);
        assert_eq!(
            Voice::filter_cost(&p, &cutoff_lfo(64)),
            steady + mv + TAP_SHAPE_COST,
            "{k:?} not twice"
        );
        // Off ALGO the taps are inert.
        let mut m = ParamSnapshot::for_engine(EngineType::Modal);
        (m.filter, m.taps) = (p.filter, p.taps);
        assert_eq!(Voice::filter_cost(&m, &none), steady, "{k:?} MODAL");
    }
}

/// Each tap destination bills its loop: the tap-VCA 15.25 instructions
/// with its block, 30; the FOLD's tapped loop 72.5 (125 with its block)
/// over the folder's moving 45 (80); each wanted op's capture 8.75, 15.
#[test]
fn taps_bill_each_destination() {
    use chimera_core::taps::{TAP_CAPTURE_COST, TAP_FOLD_COST, TAP_VCA_COST};
    assert!(TAP_VCA_COST >= Cost(30) && TAP_FOLD_COST >= Cost(125 - 80));
    assert!(TAP_CAPTURE_COST >= Cost(15) && Voice::FOLD_RAMP_COST >= Cost(80 - 45));
    let none = ModState::new();
    let base = ParamSnapshot::for_engine(EngineType::Algo);
    let bare = Voice::cost(&base, &none);
    assert_eq!(
        bare,
        EngineSlot::cost(&base, &none) + Voice::CHAIN_COST + LP24 + ModRouting::BASE,
        "no taps: as before"
    );
    let with = |taps: &[(TapOp, FixedCol, i8)], fold: f32| {
        let mut p = base.clone();
        p.folder.fold = fold;
        for &(op, c, a) in taps {
            p.taps.set(op, c, a);
        }
        p
    };
    let (op5, op6) = (TapOp::Op5, TapOp::Op6);
    let one = with(&[(op5, FixedCol::Vca, 64)], 0.0);
    assert_eq!(
        Voice::cost(&one, &none),
        bare + TAP_VCA_COST + TAP_CAPTURE_COST
    );
    let two = with(&[(op5, FixedCol::Vca, 64), (op6, FixedCol::Vca, -1)], 0.0);
    assert_eq!(
        Voice::cost(&two, &none),
        bare + TAP_VCA_COST + TAP_CAPTURE_COST + TAP_CAPTURE_COST,
        "one tap-VCA loop, two captures"
    );
    let zero = with(&[(op5, FixedCol::Vca, 0)], 0.0);
    assert_eq!(Voice::cost(&zero, &none), bare, "a 0 tap bills nothing");
    let fold = with(&[(op6, FixedCol::Fold, 64)], 0.0);
    assert_eq!(
        Voice::cost(&fold, &none),
        bare + Voice::FOLD_COST + Voice::FOLD_RAMP_COST + TAP_FOLD_COST + TAP_CAPTURE_COST
    );
    let stored = with(&[], 0.5);
    let fold = with(&[(op6, FixedCol::Fold, 64)], 0.5);
    assert_eq!(
        Voice::cost(&fold, &none),
        Voice::cost(&stored, &none) + Voice::FOLD_RAMP_COST + TAP_FOLD_COST + TAP_CAPTURE_COST
    );
    // A FOLD route already ramps the folder: the tap adds its loop only.
    let mut ms = ModState::with_sources(8);
    ms.set_fixed(ModSource::Lfo1.index(), FixedCol::Fold, 20);
    assert_eq!(
        Voice::cost(&fold, &ms),
        Voice::cost(&stored, &ms) + TAP_FOLD_COST + TAP_CAPTURE_COST
    );
    // Every destination off ALGO: nothing.
    let all = with(
        &[
            (op5, FixedCol::Vca, 64),
            (op6, FixedCol::Fold, 64),
            (op5, FixedCol::Freq, 64),
            (op6, FixedCol::Q, 64),
        ],
        0.0,
    );
    let mut modal = ParamSnapshot::for_engine(EngineType::Modal);
    let plain = Voice::cost(&modal, &none);
    modal.taps = all.taps;
    assert_eq!(Voice::cost(&modal, &none), plain, "MODAL");
}

/// LEVEL's clipper bills while it can run: stored over ½, or routed, on
/// CLEAN and DIRTY (the 2040 has none). The fading loop, 20 instructions
/// a sample, is the bill: 35.
#[test]
fn level_bills_while_it_can_clip() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::filter::FILTER_LEVEL_COST;
    use chimera_core::params::FilterParams;
    assert!(FILTER_LEVEL_COST >= Cost(35));
    let none = ModState::new();
    let mut routed = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
    let d = routed
        .push(ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL))
        .unwrap();
    routed.set_route(ModSource::Lfo1.index(), d, 5);
    for (k, lvl) in [
        (FilterKind::Svf, FILTER_LEVEL_COST),
        (FilterKind::Dirty, FILTER_LEVEL_COST),
        (FilterKind::K2040, Cost::ZERO),
        (FilterKind::Ladder, Cost::ZERO),
    ] {
        assert_eq!(k.level_cost(), lvl, "{k:?}");
        let mut p = on(k);
        let steady = p.filter.filter_cost();
        assert_eq!(Voice::filter_cost(&p, &none), steady, "{k:?} ½");
        assert_eq!(
            Voice::filter_cost(&p, &routed),
            steady + lvl,
            "{k:?} routed"
        );
        p.filter.level = 0.6;
        assert_eq!(Voice::filter_cost(&p, &none), steady + lvl, "{k:?} 0.6");
        p.filter.level = 1.0;
        assert_eq!(Voice::filter_cost(&p, &none), steady + lvl, "{k:?} 1");
    }
}

/// After LEVEL is set back to ½ the clipper runs on while the ease lands
/// and the fade falls: the voice bills it (`held_level_extra`) until it
/// stops, and never twice.
#[test]
fn a_running_clipper_bills_until_it_stops() {
    use chimera_core::dsp::algo::waves::WaveId;
    use chimera_core::dsp::filter::FILTER_LEVEL_COST;
    use chimera_core::{MidiNote, Velocity};
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = AlgoParams::single(WaveId::SAW);
    p.filter.level = 0.75;
    let mods = ModState::new();
    let mut rig = common::rig::Rig::new(chimera_hal::SAMPLE_RATE);
    rig.note_on(MidiNote::new(45).unwrap(), Velocity::new(127).unwrap(), &p);
    let mut buf = [0.0f32; chimera_hal::BLOCK_SIZE];
    rig.render_glided(&mut buf, &p, &mods);
    assert!(rig.voice.filter_level_running());
    assert_eq!(
        rig.voice.held_level_extra(&p, &mods),
        Cost::ZERO,
        "billed already"
    );
    p.filter.level = 0.5;
    let mut billed = 0;
    for _ in 0..400 {
        rig.render_glided(&mut buf, &p, &mods);
        let extra = rig.voice.held_level_extra(&p, &mods);
        assert_eq!(
            extra,
            if rig.voice.filter_level_running() {
                FILTER_LEVEL_COST
            } else {
                Cost::ZERO
            }
        );
        billed += usize::from(extra != Cost::ZERO);
    }
    assert!(billed > 15 && !rig.voice.filter_level_running(), "{billed}");
}

/// The 2040 bills by its VOICE alone (spec § Cost), as measured over
/// `OLD_LP24` (95): REV2 110 cycles, EXP 135; CLEAN ignores VOICE.
#[test]
fn the_2040_bills_its_voice() {
    let mut f = chimera_core::params::FilterParams::default();
    f.set_kind(FilterKind::K2040);
    for (v, want) in [(K2040Voice::Rev2, 15), (K2040Voice::Exp, 40)] {
        f.voice = v;
        assert_eq!(f.filter_cost(), Cost(want), "{v:?}");
    }
    let mut p = ParamSnapshot::default();
    let mods = ModState::new();
    let clean = Voice::cost(&p, &mods);
    p.filter.voice = K2040Voice::Exp;
    assert_eq!(Voice::cost(&p, &mods), clean, "VOICE is inert on CLEAN");
    p.filter.set_kind(FilterKind::K2040);
    assert_eq!(Voice::cost(&p, &mods).0, clean.0 + 40);
    p.filter.voice = K2040Voice::Rev2;
    assert_eq!(Voice::cost(&p, &mods).0, clean.0 + 15);
}

/// DIRTY bills 0 over `OLD_LP24` in every mode, VOICE inert (spec § Cost:
/// it counts under LP24's 95 cycles).
#[test]
fn dirty_bills_nothing_over_lp24() {
    let mut p = ParamSnapshot::default();
    let mods = ModState::new();
    let clean = Voice::cost(&p, &mods);
    p.filter.set_kind(FilterKind::Dirty);
    for &m in FilterKind::Dirty.modes() {
        assert!(p.filter.set_mode(m));
        for v in K2040Voice::ALL {
            p.filter.voice = v;
            assert_eq!(p.filter.filter_cost(), Cost(0), "{m:?} {v:?}");
            assert_eq!(Voice::cost(&p, &mods), clean, "{m:?} {v:?}");
        }
    }
}

fn routed(routes: &[ModSource]) -> ModState {
    let mut ms = ModState::with_sources(8);
    for s in routes {
        assert!(ms.set_route_addr(s.index(), VCA, 127));
    }
    ms
}

/// The spec's shape, billed high: the defaults `BASE`; ENV 2 (A) on the
/// VCA `BASE + CLAMP + ENV_A`; the worst case, three BURST B slots (the
/// costliest B mode) and the five other sources,
/// `BASE + CLAMP + 3·(ENV_B + BURST) + 5·OTHER`.
#[test]
fn mod_routing_bills_the_spec_shape() {
    use ModRouting as M;
    use chimera_core::dsp::modulator::{EnvType, FuncMode};
    let p = ParamSnapshot::for_engine(EngineType::Algo);
    let defaults =
        chimera_core::preset::Sound::plain_init(chimera_core::params::EngineType::Algo).mod_state;
    assert_eq!(M::cost(&p, &defaults), M::BASE);
    assert_eq!(
        M::cost(&p, &routed(&[ModSource::Env2])),
        M::BASE + M::CLAMP + M::ENV_A
    );
    let mut worst = p.clone();
    for e in worst.envelopes.iter_mut() {
        (e.env_type, e.func.mode) = (EnvType::B, FuncMode::Burst);
    }
    let three_b = M::ENV_B + M::BURST + M::ENV_B + M::BURST + M::ENV_B + M::BURST;
    let five_other = Cost(5 * M::OTHER.0);
    assert_eq!(
        M::cost(&worst, &routed(&ModSource::ALL)),
        M::BASE + M::CLAMP + three_b + five_other
    );
    // A route into ENV 2's SHAPE bills the curve on a centred B ENV, SLIDE
    // for the coefficient rebuild the same route causes, and its sum.
    let mut b2 = p.clone();
    (b2.envelopes[1].env_type, b2.envelopes[1].func.mode) = (EnvType::B, FuncMode::Env);
    let mut shaped = routed(&[ModSource::Env2]);
    let d = shaped
        .push(chimera_core::addr::ParamAddr::new(
            chimera_core::addr::BlockRef::Env(chimera_core::dsp::modulator::EnvSlot::Env2),
            chimera_core::params::EnvParams::SHAPE,
        ))
        .unwrap();
    shaped.set_route(ModSource::Lfo1.index(), d, 64);
    assert_eq!(
        M::cost(&b2, &routed(&[ModSource::Env2])),
        M::BASE + M::CLAMP + M::ENV_B
    );
    assert_eq!(
        M::cost(&b2, &shaped),
        M::BASE + M::CLAMP + M::ENV_B + M::CURVE + M::SLIDE + M::DEST_FIRST
    );
    // Measured 2026-09-28 (Task 13): 47, 47 + 24 + 28, and
    // 47 + 24 + 3·(98 + 76) + 5·8.
    assert_eq!(
        (M::BASE, M::cost(&p, &routed(&[ModSource::Env2]))),
        (Cost(47), Cost(99))
    );
    assert_eq!(M::cost(&worst, &routed(&ModSource::ALL)), Cost(633));
}

/// Each destination other than the VCA with a route of nonzero amount
/// bills `DEST_FIRST`, then `DEST` each; the default routes, at 0, bill
/// nothing. An ENV slot's LEVEL counts (its sum runs), and bills `LEVEL`
/// on top only while that slot feeds the VCA.
#[test]
fn mod_routing_bills_each_routed_destination() {
    use ModRouting as M;
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modulator::EnvSlot;
    use chimera_core::params::{EnvParams, FolderParams};
    let p = ParamSnapshot::for_engine(EngineType::Algo);
    let fold = ParamAddr::new(BlockRef::Folder, FolderParams::FOLD);
    let level = |s| ParamAddr::new(BlockRef::Env(s), EnvParams::LEVEL);
    let mut ms = chimera_core::preset::Sound::plain_init(EngineType::Algo).mod_state;
    ms.set_route_addr(ModSource::Lfo3.index(), fold, 0);
    assert_eq!(M::cost(&p, &ms), M::BASE, "amount 0");
    let l = ms.push(level(EnvSlot::Env1)).unwrap();
    ms.set_route(ModSource::Vel.index(), l, 64);
    assert_eq!(
        M::cost(&p, &ms),
        M::BASE + M::DEST_FIRST,
        "LEVEL off the VCA"
    );
    ms.set_route_addr(ModSource::Lfo3.index(), fold, 64);
    assert_eq!(M::cost(&p, &ms), M::BASE + M::DEST_FIRST + M::DEST);
    let cutoff = chimera_core::modulation::CUTOFF;
    ms.set_route_addr(ModSource::Env1.index(), cutoff, 32);
    ms.set_route_addr(ModSource::Note.index(), cutoff, 32);
    assert_eq!(
        M::cost(&p, &ms),
        M::BASE + M::DEST_FIRST + M::DEST + M::DEST,
        "one per column"
    );

    // ENV 2 (A) on the VCA, LFO 1 into its LEVEL: the peak ramps.
    let mut vca = routed(&[ModSource::Env2]);
    let d = vca.push(level(EnvSlot::Env2)).unwrap();
    vca.set_route(ModSource::Lfo1.index(), d, 127);
    assert_eq!(
        M::cost(&p, &vca),
        M::BASE + M::CLAMP + M::ENV_A + M::LEVEL + M::DEST_FIRST
    );
}

/// Each LFO slot of type FUNC bills `FUNC`, routed or not: every slot runs
/// every block.
#[test]
fn mod_routing_bills_each_func_lfo() {
    use ModRouting as M;
    use chimera_core::dsp::modulator::LfoType;
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    let none = ModState::new();
    assert_eq!(M::cost(&p, &none), M::BASE);
    p.lfos[0].lfo_type = LfoType::Func;
    p.lfos[2].lfo_type = LfoType::Func;
    assert_eq!(M::cost(&p, &none), M::BASE + M::FUNC + M::FUNC);
}

/// The folder and the drive stage bill once their stored amount runs them
/// (0.001 or more, as `process` tests it) or a route of nonzero amount may.
#[test]
fn voice_bills_fold_and_drive_when_they_can_run() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::params::{DriveParams, FolderParams};
    let base = ParamSnapshot::for_engine(EngineType::Algo);
    let none = ModState::new();
    let bare = Voice::cost(&base, &none);
    let mut p = base.clone();
    p.folder.fold = 0.000_9;
    p.drive.drive = 0.000_9;
    assert_eq!(Voice::cost(&p, &none), bare, "below the threshold");
    p.folder.fold = 0.001;
    assert_eq!(Voice::cost(&p, &none), bare + Voice::FOLD_COST);
    p.drive.drive = 1.0;
    assert_eq!(
        Voice::cost(&p, &none),
        bare + Voice::FOLD_COST + Voice::DRIVE_COST
    );
    // Stored at 0, a route: 0 bills nothing, anything else both stages
    // and their ramps.
    let fold = ParamAddr::new(BlockRef::Folder, FolderParams::FOLD);
    let drive = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
    let empty = chimera_core::mod_path::ModDestRegistry::new();
    let mut ms = ModState::from_registry(&empty, 8);
    ms.set_route_addr(ModSource::Vel.index(), fold, 0);
    ms.set_route_addr(ModSource::Vel.index(), drive, 0);
    assert_eq!(Voice::cost(&base, &ms), bare);
    ms.set_route_addr(ModSource::Vel.index(), fold, -1);
    ms.set_route_addr(ModSource::Vel.index(), drive, 1);
    let routes = ModRouting::DEST_FIRST + ModRouting::DEST;
    let ramps = Voice::FOLD_RAMP_COST + Voice::DRIVE_RAMP_COST;
    assert_eq!(
        Voice::cost(&base, &ms),
        bare + Voice::FOLD_COST + Voice::DRIVE_COST + ramps + routes
    );
    // A route on TONE alone ramps a drive its stored DRIVE runs (I1).
    let mut ms = ModState::from_registry(&empty, 8);
    let t = ms
        .push(ParamAddr::new(BlockRef::Drive, DriveParams::TONE))
        .unwrap();
    ms.set_route(ModSource::Vel.index(), t, 1);
    let mut on = base.clone();
    on.drive.drive = 0.3;
    assert_eq!(
        Voice::cost(&on, &ms),
        bare + Voice::DRIVE_COST + Voice::DRIVE_RAMP_COST + ModRouting::DEST_FIRST
    );
    assert_eq!(Voice::cost(&base, &ms), bare + ModRouting::DEST_FIRST);
}

/// BURST mode adds `BURST` on top of `ENV_B`; B in ENV or LFO
/// mode does not.
#[test]
fn mod_routing_bills_burst_only_in_burst_mode() {
    use ModRouting as M;
    use chimera_core::dsp::modulator::{EnvType, FuncMode};
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.envelopes[1].env_type = EnvType::B;
    let mods = routed(&[ModSource::Env2]);

    p.envelopes[1].func.mode = FuncMode::Burst;
    assert_eq!(M::cost(&p, &mods), M::BASE + M::CLAMP + M::ENV_B + M::BURST);

    p.envelopes[1].func.mode = FuncMode::Env;
    assert_eq!(M::cost(&p, &mods), M::BASE + M::CLAMP + M::ENV_B);

    p.envelopes[1].func.mode = FuncMode::Lfo;
    assert_eq!(M::cost(&p, &mods), M::BASE + M::CLAMP + M::ENV_B);
}

/// A route into an ENV slot's TIME, RISE, FALL or SHAPE bills `SLIDE`, once
/// per slot, whether or not that slot feeds the VCA.
#[test]
fn mod_routing_bills_slide_for_a_rebuilt_slot() {
    use ModRouting as M;
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modulator::EnvSlot;
    use chimera_core::params::EnvParams;
    let p = ParamSnapshot::for_engine(EngineType::Algo);

    // No VCA route at all: SLIDE still bills on top of BASE alone.
    let mut reg = chimera_core::mod_path::ModDestRegistry::new();
    reg.add(
        ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::RISE),
        *b"E1 RISE\0",
    )
    .unwrap();
    let mut mods = ModState::from_registry(&reg, 8);
    assert_eq!(M::cost(&p, &mods), M::BASE, "no route yet");
    mods.set_route(ModSource::Lfo1.index(), 0, 64);
    assert_eq!(M::cost(&p, &mods), M::BASE + M::SLIDE + M::DEST_FIRST);

    // Each of TIME, FALL and SHAPE bills it too, one SLIDE per slot even
    // with more than one of the four routed.
    let mut reg = chimera_core::mod_path::ModDestRegistry::new();
    for (addr, label) in [
        (
            ParamAddr::new(BlockRef::Env(EnvSlot::Env3), EnvParams::TIME),
            *b"E3 TIME\0",
        ),
        (
            ParamAddr::new(BlockRef::Env(EnvSlot::Env3), EnvParams::FALL),
            *b"E3 FALL\0",
        ),
        (
            ParamAddr::new(BlockRef::Env(EnvSlot::Env3), EnvParams::SHAPE),
            *b"E3SHAPE\0",
        ),
    ] {
        reg.add(addr, label).unwrap();
    }
    let mut mods = ModState::from_registry(&reg, 8);
    for d in 0..3 {
        mods.set_route(ModSource::Lfo1.index(), d, 64);
    }
    assert_eq!(
        M::cost(&p, &mods),
        M::BASE + M::SLIDE + M::DEST_FIRST + M::DEST + M::DEST,
        "one SLIDE, not three"
    );

    // On top of a VCA term, SLIDE still adds.
    let mut with_vca = routed(&[ModSource::Env2]);
    let d = with_vca
        .push(ParamAddr::new(
            BlockRef::Env(EnvSlot::Env2),
            EnvParams::RISE,
        ))
        .unwrap();
    with_vca.set_route(ModSource::Lfo1.index(), d, 64);
    assert_eq!(
        M::cost(&p, &with_vca),
        M::BASE + M::CLAMP + M::ENV_A + M::SLIDE + M::DEST_FIRST
    );
}

/// What the 7,500-cycle budget allows with the FX bus (the whole bus at
/// its worst) running: as many voices of each engine's default Sound as
/// fit, up to `MAX_VOICES`.
#[test]
fn budget_capacity_per_engine() {
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost();
    let fits = |e: EngineType, n: u32| FxBus::COST.0 + n * voice_cost(e).0 <= budget.0;
    for e in EngineType::ALL {
        let k = ((budget.0 - FxBus::COST.0) / voice_cost(e).0).min(MAX_VOICES as u32);
        assert!(fits(e, k), "{e:?} should fit {k}");
        if k < MAX_VOICES as u32 {
            assert!(!fits(e, k + 1), "{e:?} should not fit {}", k + 1);
        }
    }
}

/// A14 ∪ A22 is eight links. The bench's `WC /VOICE` read 790, before ADR
/// 0060's DC blocker and eases added 20 to the chain.
#[test]
fn the_worst_case_is_the_sum_of_its_terms() {
    let sum = AlgoEngine::COST_BASE.0
        + 6 * AlgoEngine::COST_OP.0
        + 8 * AlgoEngine::COST_LINK.0
        + 6 * AlgoEngine::COST_FEEDBACK.0;
    assert_eq!(cost(&worst()), sum);
    assert_eq!(Voice::CHAIN_COST.0 + sum, 836, "measured 790, + 26");
}

/// Each bench row's `/VOICE` reading, 2026-09-27, is billed at or above.
#[test]
fn the_model_bills_every_bench_row_high() {
    let row = |alg: AlgoId, lit: &[usize], fb: u8| {
        let mut p = AlgoParams::default();
        p.try_set_algs(alg, alg).unwrap();
        for (i, op) in p.ops.iter_mut().enumerate() {
            (op.level, op.feedback) = (if lit.contains(&i) { 99 } else { 0 }, fb);
        }
        Voice::CHAIN_COST.0 + cost(&p)
    };
    let all = [0, 1, 2, 3, 4, 5];
    for (name, billed, measured) in [
        ("1 OP", row(AlgoId::A1, &[0], 0), 436),
        ("ALT", row(AlgoId::A1, &[0, 2, 4], 0), 556),
        ("6 OP", row(AlgoId::A1, &all, 0), 720),
        ("CHAIN", row(AlgoId::A17, &all, 0), 776),
        ("CHN FB", row(AlgoId::A17, &all, 7), 777),
        ("WC", Voice::CHAIN_COST.0 + cost(&worst()), 790),
        ("A16+17", Voice::CHAIN_COST.0 + cost(&a16_a17()), 821),
    ] {
        assert!(billed >= measured, "{name}: {billed} < {measured}");
    }
    // The TAPE rows (spec § 5), TAPE and AGE at 0.5 on each, as the bench
    // plays them; read cold on the unit. 0 until the owner's session.
    use chimera_core::dsp::tape_engine::TapeModel::{
        Loop, Oneshot, PingPong, Scrub, Stretch, Transwave, Tron,
    };
    let tape = |model, (start, end): (f32, f32), steal_glide: bool| {
        let mut p = tape_sound(model);
        (p.tape.tape, p.tape.age, p.tape.start, p.tape.end) = (0.5, 0.5, start, end);
        if steal_glide {
            p.pitch.steal = chimera_core::params::Steal::Glide;
        }
        Voice::CHAIN_COST.0 + EngineSlot::cost(&p, &ModState::new()).0
    };
    let (fwd, rev) = ((0.0, 1.0), (1.0, 0.0));
    for (name, billed, measured) in [
        ("TAPE LOOP", tape(Loop, fwd, false), 0),
        ("TAPE STRETCH", tape(Stretch, fwd, false), 0),
        // ±12 st every block: billed as a glide or a deep route is.
        ("TAPE WAVE C7", tape(Transwave, fwd, true), 0),
        ("TINY LOOP 8", tape(Loop, fwd, false), 0),
        ("TINY REV 8", tape(Loop, rev, false), 0),
        ("TINY PING 8", tape(PingPong, fwd, false), 0),
        ("TINY SCRUB 8", tape(Scrub, fwd, false), 0),
        ("TINY LOOP", tape(Loop, fwd, false), 0),
        ("TINY REV", tape(Loop, rev, false), 0),
        ("TINY PING", tape(PingPong, fwd, false), 0),
        ("TINY SCRUB", tape(Scrub, fwd, false), 0),
        ("TINY 1SHOT", tape(Oneshot, fwd, false), 0),
        ("TINY TRON", tape(Tron, fwd, false), 0),
    ] {
        assert!(billed >= measured, "{name}: {billed} < {measured}");
    }
    assert_eq!(tape(Loop, fwd, false), 36 + 450 + 40);
    assert_eq!(tape(Stretch, fwd, false), 36 + 450 + 40);
    assert_eq!(tape(Transwave, fwd, true), 36 + 770 + 40);
    assert_eq!(tape(Oneshot, fwd, false), 36 + 230 + 40);
}

/// The Task 13 bench (rev V, 480 MHz, 2026-09-28, the bench-t13c run):
/// each ROUTING row billed at or above its reading. Every row is 1 OP (A1)
/// but A16+17. SVF HOT, LP24 HOT and Task 18's filter rows (SVF, CLEAN MOVE,
/// 2040, DIRTY, TAP FREQ, TAP VCA) are bench-only until read. The rows mirror
/// `ROUTING`'s builders in chimera-stm32/src/bench.rs (`on_vca`,
/// `b_on_vca`, `slide`, `mods`, `one_op`, `one_dest`, `a_level`): change
/// one, change the other.
#[test]
fn the_model_bills_every_routing_row_high() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modulator::{EnvForm, EnvSlot, EnvType, Func, LfoForm, LfoType};
    use chimera_core::params::EnvParams;
    let one = || {
        let mut p = ParamSnapshot::for_engine(EngineType::Algo);
        for (i, op) in p.algo.ops.iter_mut().enumerate() {
            op.level = if i == 0 { 99 } else { 0 };
        }
        p
    };
    let b = |f: Func, shape: f32| {
        let mut p = one();
        let e2 = &mut p.envelopes[1];
        (e2.env_type, e2.func.shape) = (EnvType::B, shape);
        e2.func.set_func(f);
        p
    };
    let env2 = |q| ParamAddr::new(BlockRef::Env(EnvSlot::Env2), q);
    let slide = || {
        let mut ms = routed(&[ModSource::Env2]);
        for q in [
            EnvParams::TIME,
            EnvParams::RISE,
            EnvParams::FALL,
            EnvParams::SHAPE,
        ] {
            let d = ms.push(env2(q)).unwrap();
            ms.set_route(ModSource::Lfo1.index(), d, 32);
        }
        ms
    };
    let mut a16 = ParamSnapshot::for_engine(EngineType::Algo);
    a16.algo = a16_a17();
    let env_b = |f| b(f, 0.8);
    let vca = || routed(&[ModSource::Env2]);
    let dest = |t: LfoType| {
        let mut p = one();
        p.lfos[0].lfo_type = t;
        let mut ms = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
        let d = ms
            .push(ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH))
            .unwrap();
        ms.set_route(ModSource::Lfo1.index(), d, 127);
        (p, ms)
    };
    let a_level = {
        let mut ms = vca();
        let d = ms.push(env2(EnvParams::LEVEL)).unwrap();
        ms.set_route(ModSource::Lfo1.index(), d, 127);
        ms
    };
    let (mut fold, mut drive) = (one(), one());
    (fold.folder.fold, drive.drive.drive) = (1.0, 1.0);
    let (d1, fl) = (dest(LfoType::Classic), dest(LfoType::Func));
    let mut svf = one();
    svf.filter.set_mode(FilterMode::Phaser);
    for (name, p, ms, measured) in [
        ("1 OP", one(), ModState::new(), 483),
        // BENCH screen, the 13b run.
        ("A16+17", a16, ModState::new(), 861),
        ("MODS", mods_row().0, mods_row().1, 668),
        // The old SVF's PHASER read 500 and went with it; NOTCH2 counts
        // 35 cycles to LP24's 95 (Task 15), so the row is 1 OP's until
        // the bench's NOTCH2 row is read.
        ("NOTCH2", svf, ModState::new(), 483),
        ("A VCA", one(), vca(), 535),
        ("B VCA", b(Func::Env(EnvForm::Ad), 0.5), vca(), 597),
        ("B CURVE", env_b(Func::Env(EnvForm::Ad)), vca(), 598),
        ("B LFO", env_b(Func::Lfo(LfoForm::Free)), vca(), 570),
        // Billed as B LFO: the glide is not a term of its own.
        ("B GLIDE", env_b(Func::Lfo(LfoForm::Free)), vca(), 605),
        ("BURST AD", env_b(Func::Burst(EnvForm::Ad)), vca(), 673),
        ("BURST CYC", env_b(Func::Burst(EnvForm::Cycle)), vca(), 628),
        ("VEL VCA", one(), routed(&[ModSource::Vel]), 515),
        (
            "2 VCA",
            one(),
            routed(&[ModSource::Vel, ModSource::Note]),
            523,
        ),
        ("LFO VCA", one(), routed(&[ModSource::Lfo1]), 514),
        ("A SLIDE", one(), slide(), 568),
        ("B SLIDE", env_b(Func::Env(EnvForm::Ad)), slide(), 616),
        ("FOLD", fold, ModState::new(), 526),
        ("DRIVE", drive, ModState::new(), 540),
        ("1 DEST", d1.0, d1.1, 486),
        ("FUNC LFO", fl.0, fl.1, 491),
        ("A LEVEL", one(), a_level, 548),
    ] {
        let billed = Voice::cost(&p, &ms).0;
        assert!(billed >= measured, "{name}: {billed} < {measured}");
    }
}

/// The MODS row (spec § Tests "Bench"), measured 668: 1 OP; ENV 2 type B,
/// ENV AD, SHAPE 0.8 → VCA; every source routed at 64; all three LFOs
/// FUNC; FOLD stored at 1, so the folder runs.
fn mods_row() -> (ParamSnapshot, ModState) {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modulator::{EnvForm, EnvType, Func, LfoType};
    use chimera_core::params::{FilterParams, FolderParams, OutParams};
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    for (i, op) in p.algo.ops.iter_mut().enumerate() {
        op.level = if i == 0 { 99 } else { 0 };
    }
    let e2 = &mut p.envelopes[1];
    (e2.env_type, e2.func.shape) = (EnvType::B, 0.8);
    e2.func.set_func(Func::Env(EnvForm::Ad));
    p.folder.fold = 1.0;
    for l in p.lfos.iter_mut() {
        l.lfo_type = LfoType::Func;
    }
    let routes = [
        (ModSource::Env1, chimera_core::modulation::CUTOFF),
        (ModSource::Env2, VCA),
        (
            ModSource::Env3,
            ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
        ),
        (
            ModSource::Lfo1,
            ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH),
        ),
        (
            ModSource::Lfo2,
            ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL),
        ),
        (
            ModSource::Lfo3,
            ParamAddr::new(BlockRef::Folder, FolderParams::FOLD),
        ),
        (
            ModSource::Vel,
            ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
        ),
        (ModSource::Note, chimera_core::modulation::CUTOFF),
    ];
    let empty = chimera_core::mod_path::ModDestRegistry::new();
    let mut ms = ModState::from_registry(&empty, 8);
    for (s, a) in routes {
        assert!(ms.set_route_addr(s.index(), a, 64));
    }
    (p, ms)
}

/// The brief's MODS check: the voice's bill at or above the 668 read.
#[test]
fn the_model_bills_the_mods_row_high() {
    use ModRouting as M;
    let (p, ms) = mods_row();
    let billed = Voice::cost(&p, &ms);
    assert!(billed.0 >= 668, "{billed:?}");
    let func = M::FUNC + M::FUNC + M::FUNC;
    assert_eq!(
        billed,
        Voice::CHAIN_COST
            + AlgoEngine::cost(&p.algo, &UNROUTED)
            + M::BASE
            + M::CLAMP
            + M::ENV_B
            + M::CURVE
            + M::DEST_FIRST
            + Cost(5 * M::DEST.0)
            + func
            + Voice::FOLD_COST
            + Voice::FOLD_RAMP_COST
            + LP24
            // CUTOFF and RES routed: the moving path, once; LEVEL routed.
            + chimera_core::dsp::filter::CLEAN_MOVE_COST
            + chimera_core::dsp::filter::FILTER_LEVEL_COST
    );
}

/// The patch the model prices highest: all six operators with feedback on
/// the algorithm pair whose union has the most links.
fn costliest() -> AlgoParams {
    let mut p = worst();
    let mut best = (0, p);
    for a in 0..32u8 {
        for b in 0..32u8 {
            p.try_set_algs(AlgoId::clamped(a), AlgoId::clamped(b))
                .unwrap();
            if cost(&p) > best.0 {
                best = (cost(&p), p);
            }
        }
    }
    best.1
}

fn voices_beside_fx(p: &AlgoParams) -> u32 {
    voices_at(CPU_HZ_REV_V, p)
}

fn voices_at(cpu_hz: u32, p: &AlgoParams) -> u32 {
    let budget = SampleBudget::for_cpu(cpu_hz).as_cost().0;
    let voice = Voice::CHAIN_COST.0 + ModRouting::BASE.0 + LP24.0 + cost(p);
    ((budget - FxBus::COST.0) / voice).min(MAX_VOICES as u32)
}

/// ADR 0026, 0040: a one-operator patch gets every voice; the pool, not
/// the budget, is what stops it.
#[test]
fn a_one_operator_patch_gets_every_voice() {
    let one = AlgoParams::single(chimera_core::dsp::algo::waves::WaveId::W1);
    assert_eq!(voices_beside_fx(&one), MAX_VOICES as u32);
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    let voice = Voice::CHAIN_COST.0 + ModRouting::BASE.0 + cost(&one);
    assert!((budget - FxBus::COST.0) / voice > MAX_VOICES as u32);
}

/// A16 ∪ A17 at MORPH 64, six operators with feedback: 12 links, ADR
/// 0026's costliest shape.
fn a16_a17() -> AlgoParams {
    let mut p = worst();
    p.try_set_algs(AlgoId::A16, AlgoId::A17).unwrap();
    p
}

/// The master tape is on the chain (ADR 0055): the bus costs more, and
/// some counts below are one fewer.
const TAPE: bool = cfg!(feature = "master-tape");

/// FX diet spec § Intent and ADR 0031: with the bus at 1,204 (BUS+V
/// 1,188 of 2026-10-04 and the reverb's tail gate; 1,494 with the master
/// tape) and the modulator pool's floor (`ModRouting::BASE`, 47) added,
/// the costliest patch gets six voices on rev V (6 × 915 + 1,204 = 6,694
/// ≤ 7,500; a seventh would be 7,609), and five on rev Y ((6,250 − 1,204)
/// / 915 = 5.5; (6,250 − 1,494) / 915 = 5.2 with the tape),
/// without FOLD or DRIVE, at LP24 (its SVF term 0). ADR 0040: the budget,
/// not the eight-voice pool, is what stops it.
#[test]
fn the_costliest_patch_gets_six_voices_on_rev_v() {
    let p = a16_a17();
    assert_eq!(Voice::CHAIN_COST.0 + cost(&p), 868);
    assert_eq!(Voice::CHAIN_COST.0 + ModRouting::BASE.0 + cost(&p), 915);
    let plain = 915 + LP24.0;
    assert_eq!(cost(&costliest()), cost(&p), "no pair has more links");
    let bus = if TAPE { 1_494 } else { 1_204 };
    assert_eq!(FxBus::COST.0, bus, "{:?}", FxBus::COST);
    assert_eq!(voices_at(CPU_HZ_REV_V, &p), 6);
    assert_eq!(MAX_VOICES, 8, "the budget stops it short of the pool");
    assert_eq!(voices_at(CPU_HZ_REV_Y, &p), 5);
    // ADR 0091: while ORBIT plays, its own block (171) and a five-note
    // burst (857) are reserved beside the bus: five on rev V
    // ((7,500 − 2,232) / 915 = 5.8; (7,500 − 2,522) / 915 = 5.4 with the tape).
    // Stopped, six.
    use chimera_core::instrument::Instrument;
    use chimera_core::orbit::runner::OrbitRunner;
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    let playing = FxBus::COST.0 + Instrument::BURST.0 + OrbitRunner::COST.0;
    assert_eq!((budget - playing) / 915, 5);
    // With the folder on, unrouted (45, ADR 0060): 960, six on rev V and
    // five on rev Y (six and four with the tape); the drive stage too
    // (1,017): six on rev V, four on rev Y (five and four with the tape).
    // No factory Sound does either with this shape.
    let fits = |hz, voice: u32| (SampleBudget::for_cpu(hz).as_cost().0 - FxBus::COST.0) / voice;
    let fold = plain + Voice::FOLD_COST.0;
    let want = if TAPE { (6, 4) } else { (6, 5) };
    assert_eq!((fits(CPU_HZ_REV_V, fold), fits(CPU_HZ_REV_Y, fold)), want);
    let both = fold + Voice::DRIVE_COST.0;
    let want = if TAPE { (5, 4) } else { (6, 4) };
    assert_eq!((fits(CPU_HZ_REV_V, both), fits(CPU_HZ_REV_Y, both)), want);
}

/// CHOIR, TALK BASS, BREATH and VOX (factory 8–11) with their routes,
/// filters and drive keep the formant floor beside the tape on rev V (ADR
/// 0131).
#[test]
fn formant_sounds_keep_the_floor_with_their_routes() {
    for i in 8..12 {
        let s = chimera_core::factory::factory_sound(i).unwrap();
        let v = (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - FxBus::COST_TAPE.0)
            / Voice::cost(&s.params, &s.mod_state).0;
        assert!(v >= FLOOR_VOICES_FORMANT, "F{i}: {v}");
    }
}

/// Spec § Intent and ADR 0031, 0040, 0124: billed as it plays (its routes,
/// FOLD and DRIVE in; all at LP24) beside the bus alone, every factory Sound
/// fits six voices on rev V at 75 % and five on rev Y; the TX and
/// single-oscillator Sounds all eight, MORPH PAD and KEYS seven (six with
/// the master tape). That is what ORBIT stopped admits. ADR 0091: while ORBIT
/// plays, its own block (171) and a five-note burst (857) are reserved too:
/// on rev V the TX Sounds get seven, MORPH six, SAW LEAD and SQR BASS eight
/// (with the tape: seven, seven, six, seven; MORPH five).
#[test]
fn every_factory_sound_gets_at_least_six_voices_on_rev_v() {
    use chimera_core::instrument::Instrument;
    use chimera_core::orbit::runner::OrbitRunner;
    const FITS: [u32; 8] = if TAPE {
        [8, 8, 8, 8, 8, 8, 6, 6]
    } else {
        [8, 8, 8, 8, 8, 8, 7, 7]
    };
    const ORBIT: [u32; 8] = if TAPE {
        [7, 7, 6, 7, 8, 8, 5, 5]
    } else {
        [7, 7, 7, 7, 8, 8, 6, 6]
    };
    for i in 0..8 {
        let s = chimera_core::factory::factory_sound(i).unwrap();
        let voice = Voice::cost(&s.params, &s.mod_state).0;
        let at = |hz, reserved: Cost| {
            let budget = SampleBudget::for_cpu(hz).as_cost().0;
            ((budget - reserved.0) / voice).min(MAX_VOICES as u32)
        };
        assert_eq!(at(CPU_HZ_REV_V, FxBus::COST), FITS[i], "factory {i}");
        // ADR 0110's amp gate (6 more a voice) takes SQR BASS to four on rev Y
        // beside the master tape.
        let floor = if TAPE && i == 7 { 4 } else { 5 };
        assert!(
            at(CPU_HZ_REV_Y, FxBus::COST) >= floor,
            "factory {i} on rev Y"
        );
        let playing = FxBus::COST + Instrument::BURST + OrbitRunner::COST;
        assert_eq!(at(CPU_HZ_REV_V, playing), ORBIT[i], "factory {i}, ORBIT");
    }
}

/// MORPH does not matter: the plan runs the union's links at any MORPH.
#[test]
fn morph_does_not_change_the_cost() {
    let mut p = worst();
    let mid = cost(&p);
    for m in [0, 127] {
        p.morph = m;
        assert_eq!(cost(&p), mid);
    }
}

/// Every single step towards a bigger patch (an operator, feedback, a link)
/// never lowers the cost, from every algorithm pair.
#[test]
fn cost_is_monotonic_in_operators_links_and_feedback() {
    let base = AlgoParams::default();
    for a in 0..32u8 {
        for b in 0..32u8 {
            let mut p = base;
            p.try_set_algs(AlgoId::clamped(a), AlgoId::clamped(b))
                .unwrap();
            for op in p.ops.iter_mut() {
                (op.level, op.feedback) = (0, 0);
            }
            for i in 0..OPS {
                let before = cost(&p);
                p.ops[i].level = 1;
                let lit = cost(&p);
                assert!(lit > before, "op {i} on {a}/{b}");
                p.ops[i].feedback = 1;
                assert!(cost(&p) > lit, "fb {i} on {a}/{b}");
            }
        }
    }
    // A link: A1 has none, A2 adds 6→1, A3 adds 6→2 as well; with B held,
    // a superset of links in A never costs less.
    let mut p = worst();
    for (from, to) in [(AlgoId::A1, AlgoId::A2), (AlgoId::A2, AlgoId::A3)] {
        p.try_set_algs(from, from).unwrap();
        let fewer = cost(&p);
        p.try_set_algs(to, p.algs().1).unwrap();
        assert!(cost(&p) > fewer, "{from:?} to {to:?}");
    }
}

/// Links are priced by their target: A17 is 6→5→4→3→2→1, so silencing
/// operator 1 drops the link 2→1, and silencing operator 6 (a source only)
/// drops none.
#[test]
fn links_are_priced_by_their_active_target() {
    let mut all = worst();
    all.try_set_algs(AlgoId::A17, AlgoId::A17).unwrap();
    for op in all.ops.iter_mut() {
        op.feedback = 0;
    }
    let (op, link) = (AlgoEngine::COST_OP.0, AlgoEngine::COST_LINK.0);
    let full = cost(&all);
    assert_eq!(full, AlgoEngine::COST_BASE.0 + 6 * op + 5 * link);
    let mut no_1 = all;
    no_1.ops[0].level = 0;
    assert_eq!(full - cost(&no_1), op + link, "4 links");
    let mut no_6 = all;
    no_6.ops[5].level = 0;
    assert_eq!(full - cost(&no_6), op, "5 links");
}

/// A route on a silent operator's LEVEL may lift it, so it is priced.
#[test]
fn a_routed_silent_operator_is_priced() {
    let mut p = worst();
    p.ops[5].level = 0;
    let mut routed = UNROUTED;
    let bare = AlgoEngine::cost(&p, &routed).0;
    routed[5] = true;
    assert_eq!(AlgoEngine::cost(&p, &routed).0, cost(&worst()));
    assert!(bare < cost(&worst()));
}

/// A route into PITCH or FINE retunes Modal's strings every block: billed
/// `ModalEngine::PITCH` (a host estimate) on any route, at amount 0 too,
/// SYMP `HALO_PITCH` more for its halo's seven targets;
/// Algo's pitch rides its per-block operator update.
#[test]
fn a_pitch_route_on_modal_bills_the_retune() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modal::ModalEngine;
    use chimera_core::params::PitchParams;
    assert_eq!(ModalEngine::PITCH, Cost(30));
    assert_eq!(ModalEngine::HALO_PITCH, Cost(23));
    let routed = |q, amount| {
        let mut ms = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
        let d = ms.push(ParamAddr::new(BlockRef::Pitch, q)).unwrap();
        ms.set_route(ModSource::Lfo1.index(), d, amount);
        ms
    };
    let modal = ParamSnapshot::for_engine(EngineType::Modal);
    // The pitch wheel (#437) may retune any Modal voice, so every one bills
    // the retune whether or not a route does.
    let bare = EngineSlot::cost(&modal, &ModState::new());
    assert_eq!(bare, ModalEngine::cost(&modal.modal) + ModalEngine::PITCH);
    for q in [PitchParams::PITCH, PitchParams::FINE] {
        for amount in [127, 0] {
            assert_eq!(EngineSlot::cost(&modal, &routed(q, amount)), bare);
        }
    }
    let mut symp = modal.clone();
    symp.modal.mode = chimera_core::dsp::modal::ResonatorMode::Sympathetic;
    assert_eq!(
        EngineSlot::cost(&symp, &routed(PitchParams::PITCH, 127)),
        EngineSlot::cost(&symp, &ModState::new())
    );
    assert_eq!(
        EngineSlot::cost(&symp, &ModState::new()),
        ModalEngine::cost(&symp.modal) + ModalEngine::PITCH + ModalEngine::HALO_PITCH
    );
    let algo = ParamSnapshot::for_engine(EngineType::Algo);
    assert_eq!(
        EngineSlot::cost(&algo, &routed(PitchParams::PITCH, 127)),
        EngineSlot::cost(&algo, &ModState::new())
    );
}

/// A route into STRUCTURE can keep SYMP's halo gliding between chords:
/// billed `ModalEngine::CHORD` (estimated) on any route, at amount 0
/// too; the other models' STRUCTURE is billed with them.
#[test]
fn a_structure_route_on_symp_bills_the_chord_glide() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modal::{ModalEngine, ModalParams, ResonatorMode};
    assert_eq!(ModalEngine::CHORD, Cost(80));
    let routed = |amount| {
        let mut ms = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
        let d = ms
            .push(ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE))
            .unwrap();
        ms.set_route(ModSource::Lfo1.index(), d, amount);
        ms
    };
    for mode in [
        ResonatorMode::String,
        ResonatorMode::Modal,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ] {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = mode;
        let bare = EngineSlot::cost(&p, &ModState::new());
        let extra = if mode == ResonatorMode::Sympathetic {
            ModalEngine::CHORD
        } else {
            Cost(0)
        };
        for amount in [127, 0] {
            assert_eq!(
                EngineSlot::cost(&p, &routed(amount)),
                bare + extra,
                "{mode:?}"
            );
        }
    }
}

/// Modal is billed per model (#49), and STRING and SYMP for BODY and the
/// ensemble when on: host estimates until the bench's MDL rows (Modal 2
/// step A, task 12; `modal` in chimera-stm32/src/bench.rs) read them.
/// Voices beside the whole FX bus at its worst, on rev V and rev Y: the
/// costlier models get fewer, as they must. BANK's and SYMP's out clip,
/// stmlib's rational curve for `tanhf` (ADR 0097), gave BANK a third voice
/// on rev V and SYMP one more on most rows.
#[test]
fn modal_bills_each_model() {
    use chimera_core::dsp::modal::{BankModes, ModalEngine, ResonatorMode};
    let sound = |mode, body, ens_mix| {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        (p.modal.mode, p.modal.body, p.modal.ens_mix) = (mode, body, ens_mix);
        p
    };
    use ResonatorMode::{Bowed, Modal, String, Sympathetic};
    // Voices on rev V and rev Y, then with the master tape (ADR 0055).
    for (p, billed, voices, taped) in [
        (sound(String, 0.0, 0.0), 330, (8, 8), (8, 8)),
        (sound(String, 0.3, 0.0), 410, (8, 8), (8, 8)),
        (sound(String, 0.3, 0.5), 550, (8, 7), (8, 7)),
        (sound(Bowed, 0.3, 0.5), 640, (8, 6), (7, 6)),
        (sound(Sympathetic, 0.0, 0.0), 470, (8, 8), (8, 7)),
        (sound(Sympathetic, 0.3, 0.0), 550, (8, 7), (8, 6)),
        (sound(Sympathetic, 0.3, 0.5), 690, (7, 6), (7, 5)),
        (sound(Modal, 0.3, 0.5), 1_840, (3, 2), (3, 2)),
    ] {
        let (rev_v, rev_y) = if cfg!(feature = "master-tape") {
            taped
        } else {
            voices
        };
        let m = (p.modal.mode, p.modal.body, p.modal.ens_mix);
        let engine = EngineSlot::cost(&p, &ModState::new());
        // The model's own bill, then the wheel's retune (#437).
        let halo = if p.modal.mode == Sympathetic {
            ModalEngine::HALO_PITCH
        } else {
            Cost(0)
        };
        assert_eq!(ModalEngine::cost(&p.modal), Cost(billed), "{m:?}");
        assert_eq!(engine, Cost(billed) + ModalEngine::PITCH + halo, "{m:?}");
        let voice = Voice::cost(&p, &ModState::new()).0;
        let at = |hz| {
            let budget = SampleBudget::for_cpu(hz).as_cost().0;
            ((budget - FxBus::COST.0) / voice).min(MAX_VOICES as u32)
        };
        assert_eq!(
            (at(CPU_HZ_REV_V), at(CPU_HZ_REV_Y)),
            (rev_v, rev_y),
            "{m:?}"
        );
    }
    // The bank scales with its modes: 48, the most, is 16 more than 32.
    let mut bank = sound(Modal, 0.0, 0.0);
    bank.modal.modes = BankModes::M48;
    assert_eq!(ModalEngine::cost(&bank.modal), Cost(1_840 + 16 * 45));
}

/// A bow from C6 up re-takes its lock correction each block while FORCE
/// or SPEED eases (`grip`, a loop re-split). No route can move them: they
/// are no modulation destination, so only a knob moves them, briefly, in
/// the headroom (ADR 0061), and nothing is billed. Made modulatable, they
/// need a bill as PITCH's.
#[test]
fn force_and_speed_take_no_route_so_the_bows_retune_bills_none() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modal::ModalParams;
    for q in [ModalParams::FORCE, ModalParams::SPEED] {
        let addr = ParamAddr::new(BlockRef::Modal, q);
        assert!(!addr.modulatable(), "{q:?}");
        let mut ms = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
        assert_eq!(ms.push(addr), None, "{q:?}");
    }
}

/// STEAL GLIDE retunes a gliding Modal voice every block, as a PITCH
/// route does: billed `ModalEngine::PITCH`, once beside a route; ALGO's
/// glide rides its per-block operator update (#254).
#[test]
fn a_glide_steal_bills_the_retune() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::modal::{ModalEngine, ResonatorMode};
    use chimera_core::params::{PitchParams, Steal};
    let pitch_route = || {
        let mut ms = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
        let d = ms
            .push(ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH))
            .unwrap();
        ms.set_route(ModSource::Lfo1.index(), d, 127);
        ms
    };
    for mode in [
        ResonatorMode::String,
        ResonatorMode::Modal,
        ResonatorMode::Bowed,
        ResonatorMode::Sympathetic,
    ] {
        let mut p = ParamSnapshot::for_engine(EngineType::Modal);
        p.modal.mode = mode;
        let cut = EngineSlot::cost(&p, &ModState::new());
        p.pitch.steal = Steal::Glide;
        let glide = EngineSlot::cost(&p, &ModState::new());
        let halo = if mode == ResonatorMode::Sympathetic {
            ModalEngine::HALO_PITCH
        } else {
            Cost(0)
        };
        assert_eq!(glide, cut, "{mode:?}");
        assert_eq!(
            cut,
            ModalEngine::cost(&p.modal) + ModalEngine::PITCH + halo,
            "{mode:?}"
        );
        assert_eq!(EngineSlot::cost(&p, &pitch_route()), glide, "{mode:?}");
    }
    let mut algo = ParamSnapshot::for_engine(EngineType::Algo);
    let cut = EngineSlot::cost(&algo, &ModState::new());
    algo.pitch.steal = Steal::Glide;
    assert_eq!(EngineSlot::cost(&algo, &ModState::new()), cut);
}

/// What the voices render now, each at the Sound it plays (a stolen
/// voice's fade at the Sound it fades), the FX bus beside them: what the
/// allocator's bill must cover.
fn rendered(inst: &chimera_core::instrument::Instrument, shared: &AudioShared) -> u32 {
    let parts = inst.sounding_parts();
    let active = inst.active();
    (0..MAX_VOICES)
        .filter(|&v| active[v])
        .map(|v| {
            let p = &shared.parts[parts[v] as usize];
            Voice::cost(&p.params, &p.mod_state).0
        })
        .sum::<u32>()
        + FxBus::COST.0
}

/// A16 ∪ A17 as the bench plays it: the costliest voice (909).
fn a16_a17_sound() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = a16_a17();
    p
}

/// ADR 0091: a voice taken for another Part's note fades out on its own
/// Sound for `Voice::FADE` before the note starts, so its slot is billed
/// at the costlier of the two until then. A16 ∪ A17 keys fill the budget;
/// a TX note (704) takes a key's voice, still 909 while it fades. Billed at
/// 704, the 205 "freed" would let a Modal note (487) in beside the fade,
/// over the budget; billed at 909, it takes a key too, and waits.
#[test]
fn a_stolen_voice_is_billed_until_its_fade_ends() {
    let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    *r.shared.parts[5].params = a16_a17_sound();
    *r.shared.parts[0].params = chimera_core::factory::factory_sound(0).unwrap().params;
    *r.shared.parts[1].params = ParamSnapshot::for_engine(EngineType::Modal);
    // Beside the master tape's bus, ADR 0110's 6 a voice leaves room for five.
    for k in 0..if TAPE { 5 } else { 6 } {
        r.note_on(5, 36 + 12 * k);
    }
    r.render(1);
    r.note_on(0, 60);
    r.note_on(1, 64);
    let mut most = 0;
    for _ in 0..8 {
        let before = rendered(&r.inst, &r.shared);
        r.render(1);
        most = most.max(before).max(rendered(&r.inst, &r.shared));
    }
    assert!(most <= budget, "{most} rendered of {budget}");
    let sounding = r.inst.sounding_parts();
    let active = r.inst.active();
    for p in [0u8, 1] {
        assert!(
            (0..MAX_VOICES).any(|v| active[v] && sounding[v] == p),
            "Part {} sounds",
            p + 1
        );
    }
}

/// The STRESS scene (the bench's): NEW's Sounds on Parts 1–5, A16 ∪ A17
/// on Part 6, its six keys down a block after ORBIT starts (or, with
/// `keys_first`, before), ORBIT playing NEW's groove. Every block, the
/// most voices sounding and the most the voices and bus rendered; and
/// whether every block from ORBIT's start held all six keys, none stolen
/// or shed.
fn stress(keys_first: bool, blocks: usize) -> (usize, u32, bool) {
    use chimera_core::factory::factory_sound;
    use chimera_core::orbit::runner::OrbitRunner;
    use chimera_core::part::PartParams;
    use chimera_core::project::{NEW_PARTS, Project};
    let (project, _) = Project::boxed();
    let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *r.shared = AudioShared::from_performance(project.perf());
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix = PartParams::for_part(i);
        let s = NEW_PARTS.get(i).and_then(|s| factory_sound(s.index()));
        let s = s.unwrap_or_else(|| chimera_core::preset::Sound::plain_init(EngineType::Algo));
        (*part.params, *part.mod_state) = (s.params, s.mod_state);
    }
    *r.shared.parts[5].params = a16_a17_sound();
    let ch = r.shared.parts[5].mix.channel.get();
    let keys = |r: &mut common::InstRig| {
        for k in 0..6 {
            r.note_on(ch, 36 + 12 * k);
        }
    };
    if keys_first {
        keys(&mut r);
    }
    r.shared
        .orbit
        .push(chimera_core::orbit::channel::Cmd::TogglePlay);
    let sr = core::num::NonZeroU32::new(common::SR).unwrap();
    let mut runner = Box::new(OrbitRunner::new(sr));
    let (mut voices, mut most, mut kept) = (0, 0, true);
    for b in 0..blocks {
        if b == 1 && !keys_first {
            keys(&mut r);
        }
        runner.block(&mut r.inst, &r.shared);
        let before = rendered(&r.inst, &r.shared);
        r.render(1);
        voices = voices.max(r.inst.sounding());
        most = most.max(before).max(rendered(&r.inst, &r.shared));
        let a = r.inst.allocator();
        let origins = r.inst.origins();
        let held = (0..MAX_VOICES)
            .filter(|&v| {
                let s = a.slots()[v];
                s.part() == Some(5) && s.held() && !s.dying() && origins[v] == NoteOrigin::Live
            })
            .count();
        kept &= held == 6;
    }
    (voices, most, kept)
}

/// The bench's STRESS run: 10 s, past NEW's four bars.
const STRESS_BLOCKS: usize = 7_500;

/// ADR 0091: in the STRESS scene with the keys down a block after ORBIT
/// starts, what the voices and bus render leaves room for ORBIT's own
/// block and a burst in every block. Keys down before it keep sounding,
/// all six, past that (its reserve refuses, it never steals them or sheds),
/// within the budget.
#[test]
fn orbit_and_a_burst_fit_beside_what_renders() {
    use chimera_core::instrument::Instrument;
    use chimera_core::orbit::runner::OrbitRunner;
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    for keys_first in [true, false] {
        let (voices, most, kept) = stress(keys_first, STRESS_BLOCKS);
        // All six keys sound (none stolen or shed once ORBIT starts, when
        // down first, without the `master-tape` bus), either way.
        assert_eq!(voices, 6, "{keys_first}");
        assert!(kept || !(keys_first && !TAPE), "a key stolen or shed");
        let room = match keys_first {
            true => 0,
            false => Instrument::BURST.0 + OrbitRunner::COST.0,
        };
        assert!(
            most + room <= budget,
            "keys first {keys_first}: {most} + {room} of {budget}, {voices} voices"
        );
    }
}

/// ADR 0091: the Parts are costed once a block. A burst of note-ons, here
/// all refused (no budget), costs them once between them and the render;
/// the next block costs them again.
#[test]
fn a_refused_note_on_does_not_recost() {
    let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(0));
    r.render(1);
    let before = r.inst.costings();
    for n in 60..65 {
        r.note_on(0, n);
    }
    r.render(1);
    assert_eq!(r.inst.allocator().refused(), 5);
    assert_eq!(r.inst.costings(), before + 1, "once for the block");
    r.render(1);
    assert_eq!(r.inst.costings(), before + 2, "and once the next");
}

/// ADR 0091's reserve beside the voices: the bus's 1,204, the knobs' 14
/// (ADR 0116), the LFO clocks' 10 (ADR 0135) and a note-on burst's 836
/// (1,494 + 14 + 10 + 836 with the master tape), 171 more while ORBIT
/// plays. Spec § 5's admission table is counted
/// against it.
// ponytail: literal until `Instrument::RESERVED` (orbit-budget) merges.
const RESERVED: u32 = 2_064;
const RESERVED_TAPE: u32 = 2_354;
const ORBIT_COST: u32 = 171;

/// A matrix with `routes` (source, destination, amount) and nothing else;
/// `None` if a destination takes no route.
fn try_routes(routes: &[(ModSource, chimera_core::addr::ParamAddr, i8)]) -> Option<ModState> {
    let mut ms = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
    for &(s, addr, amt) in routes {
        let d = ms.find(addr).or_else(|| ms.push(addr))?;
        ms.set_route(s.index(), d, amt);
    }
    Some(ms)
}

fn routes(r: &[(ModSource, chimera_core::addr::ParamAddr, i8)]) -> ModState {
    try_routes(r).expect("every destination modulatable")
}

fn tape_sound(model: chimera_core::dsp::tape_engine::TapeModel) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Tape);
    p.tape.model = model;
    p
}

/// Spec § 5: each model bills its measured worst, TAPE +10 and AGE +30
/// each when it runs (stored above 0, or routed); and the voices each is
/// admitted beside the reserve on rev V, rev Y, rev V with ORBIT playing
/// and rev V with the master tape, as tabled, with TAPE and AGE both off
/// and both at 0.5.
#[test]
fn tape_bills_each_model() {
    use TapeModel::{Loop, Oneshot, PingPong, Scrub, Stretch, Transwave, Tron};
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::tape_engine::{TapeEngineParams, TapeModel};
    let rooms = {
        let v = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
        let y = SampleBudget::for_cpu(CPU_HZ_REV_Y).as_cost().0;
        [
            v - RESERVED,
            y - RESERVED,
            v - RESERVED - ORBIT_COST,
            v - RESERVED_TAPE,
        ]
    };
    assert_eq!(rooms, [5_436, 4_186, 5_265, 5_146]);
    let at = |voice: u32| rooms.map(|r| (r / voice).min(MAX_VOICES as u32));
    let on = |q| ParamAddr::new(BlockRef::TapeEngine, q);
    // Model, bill, voices with TAPE and AGE off, with both.
    for (model, bill, off, both) in [
        (Loop, 450, [8, 7, 8, 8], [8, 7, 8, 8]),
        (PingPong, 490, [8, 7, 8, 8], [8, 6, 8, 8]),
        (Scrub, 480, [8, 7, 8, 8], [8, 6, 8, 8]),
        (Transwave, 400, [8, 8, 8, 8], [8, 8, 8, 8]),
        (Stretch, 450, [8, 7, 8, 8], [8, 7, 8, 8]),
        (Oneshot, 230, [8, 8, 8, 8], [8, 8, 8, 8]),
        (Tron, 230, [8, 8, 8, 8], [8, 8, 8, 8]),
    ] {
        for reversed in [false, true] {
            let mut p = tape_sound(model);
            if reversed {
                (p.tape.start, p.tape.end) = (1.0, 0.0);
            }
            // TAPE and AGE each stored 0, 0.5, and routed at 0.
            for (tape, tape_routed, t) in [(0.0, false, 0), (0.5, false, 10), (0.0, true, 10)] {
                for (age, age_routed, a) in [(0.0, false, 0), (0.5, false, 30), (0.0, true, 30)] {
                    (p.tape.tape, p.tape.age) = (tape, age);
                    let mut r = Vec::new();
                    if tape_routed {
                        r.push((ModSource::Lfo1, on(TapeEngineParams::TAPE), 64));
                    }
                    if age_routed {
                        r.push((ModSource::Lfo2, on(TapeEngineParams::AGE), 64));
                    }
                    // TAPE and AGE take no route until the voice reads
                    // TAPE_ENG (`BlockRef::voice_reads`): then these run.
                    let Some(mods) = try_routes(&r) else {
                        assert!(r.iter().all(|&(_, addr, _)| !addr.modulatable()));
                        continue;
                    };
                    let why = format!(
                        "{model:?} rev {reversed} TAPE {tape}/{tape_routed} AGE {age}/{age_routed}"
                    );
                    assert_eq!(EngineSlot::cost(&p, &mods), Cost(bill + t + a), "{why}");
                }
            }
            for (stored, want, extra) in [(0.0, off, 0), (0.5, both, 40)] {
                (p.tape.tape, p.tape.age) = (stored, stored);
                let voice = Voice::cost(&p, &ModState::new()).0;
                assert_eq!(voice, bill + extra + 83, "{model:?} at {stored}");
                assert_eq!(at(voice), want, "{model:?} at {stored}");
            }
        }
    }
}

fn va_sound(mode: chimera_core::dsp::va::VaMode) -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Va);
    p.va.mode = mode;
    p
}

/// Spec § Budget: each MODE bills ADR 0056's count of its worst voice, and
/// the voices each is admitted beside the reserve on the four rooms (rev V,
/// rev Y, rev V with ORBIT, rev V with the master tape), INIT's 2040 REV2
/// (+15) and the chain and routing (83) on top. ADR 0040's done-when is six
/// on each: met on rev V and rev Y in every MODE, eight on rev V at 75 % (ADR 0124) (ADR 0118: WAVE's reach
/// and SYNC's master held under 7 kHz, points 11 and 12; the sub at the
/// shaped edge, point 13).
#[test]
fn va_bills_each_mode() {
    use chimera_core::dsp::va::VaMode::{Bits, Mix, Ring, Sync, Xor};
    let rooms = {
        let v = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
        let y = SampleBudget::for_cpu(CPU_HZ_REV_Y).as_cost().0;
        [
            v - RESERVED,
            y - RESERVED,
            v - RESERVED - ORBIT_COST,
            v - RESERVED_TAPE,
        ]
    };
    assert_eq!(rooms, [5_436, 4_186, 5_265, 5_146]);
    let at = |voice: u32| rooms.map(|r| (r / voice).min(MAX_VOICES as u32));
    for (mode, bill, voices) in [
        (Mix, 530, [8, 6, 8, 8]),
        (Sync, 540, [8, 6, 8, 8]),
        (Ring, 530, [8, 6, 8, 8]),
        (Xor, 550, [8, 6, 8, 7]),
        (Bits, 530, [8, 6, 8, 8]),
    ] {
        let p = va_sound(mode);
        assert_eq!(
            EngineSlot::cost(&p, &ModState::new()),
            Cost(bill),
            "{mode:?}"
        );
        let voice = Voice::cost(&p, &ModState::new()).0;
        assert_eq!(voice, bill + 83 + 15, "{mode:?}");
        assert_eq!(at(voice), voices, "{mode:?}");
    }
}

/// A VA note holds its playing MODE's bill until the duck swaps the MODE:
/// held in SYNC with MIX stored, it bills SYNC's extra over MIX for the
/// duck's first block, then none.
#[test]
fn a_held_va_note_bills_its_playing_mode() {
    use chimera_core::dsp::va::{COST_MIX, COST_SYNC, VaMode};
    use chimera_core::{MidiNote, Velocity};
    let (sync, mix) = (va_sound(VaMode::Sync), va_sound(VaMode::Mix));
    let mods = ModState::new();
    let mut rig = common::Rig::new(common::SR);
    rig.note_on(
        MidiNote::new(60).unwrap(),
        Velocity::new(100).unwrap(),
        &sync,
    );
    let mut block = [0.0; chimera_hal::BLOCK_SIZE];
    rig.render(&mut block, &sync, &mods);
    assert_eq!(rig.held_model_extra(&mix), Cost(COST_SYNC.0 - COST_MIX.0));
    // MIX stored: block 0 ducks down, block 1 ducks down and swaps.
    rig.render(&mut block, &mix, &mods);
    assert_eq!(rig.held_model_extra(&mix), Cost(COST_SYNC.0 - COST_MIX.0));
    rig.render(&mut block, &mix, &mods);
    assert_eq!(rig.held_model_extra(&mix), Cost::ZERO);
}

/// Spec § 5: TRANSWAVE bills a level change's 370 more only where its pitch
/// can move more than 620 cents in a note: STEAL GLIDE, or PITCH and FINE
/// routes whose `|amount| × range` sum past it. A vibrato bills none. A
/// re-snap's glide bills it beside a TRANSWAVE that isn't billed it
/// already; no other model has levels.
#[test]
fn vibrato_costs_no_level_heads() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::dsp::tape_engine::TapeModel;
    use chimera_core::params::{PitchParams, Steal};
    let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
    let fine = ParamAddr::new(BlockRef::Pitch, PitchParams::FINE);
    let wave = tape_sound(TapeModel::Transwave);
    let (l1, l2) = (ModSource::Lfo1, ModSource::Lfo2);
    // 1 st is 5.3 of 127 at PITCH's ±24 st; 620 cents is 32.8.
    for (r, bill) in [
        (vec![], 400),
        (vec![(l1, pitch, 5)], 400),
        (vec![(l1, pitch, -5)], 400),
        (vec![(l1, fine, 127)], 400),
        (vec![(l1, pitch, 32)], 400),
        (vec![(l1, pitch, 33)], 770),
        (vec![(l1, pitch, -33)], 770),
        (vec![(l1, pitch, 64)], 770),
        (vec![(l1, pitch, 17), (l2, pitch, -17)], 770),
        (vec![(l1, pitch, 30), (l2, fine, 127)], 770),
        (vec![(l1, pitch, 30), (l2, fine, 60)], 400),
    ] {
        let mods = routes(&r);
        assert_eq!(EngineSlot::cost(&wave, &mods), Cost(bill), "{r:?}");
        let glide = Voice::glide_cost(&wave, &mods);
        assert_eq!(glide, Cost(770 - bill), "a re-snap under {r:?}");
    }
    let mut glide = wave;
    glide.pitch.steal = Steal::Glide;
    assert_eq!(EngineSlot::cost(&glide, &ModState::new()), Cost(770));
    assert_eq!(Voice::glide_cost(&glide, &ModState::new()), Cost(0));
    let deep = routes(&[(l1, pitch, 64)]);
    for m in TapeModel::ALL
        .into_iter()
        .filter(|&m| m != TapeModel::Transwave)
    {
        let mut p = tape_sound(m);
        let base = EngineSlot::cost(&p, &ModState::new());
        assert_eq!(EngineSlot::cost(&p, &deep), base, "{m:?}");
        assert_eq!(Voice::glide_cost(&p, &ModState::new()), Cost(0), "{m:?}");
        p.pitch.steal = Steal::Glide;
        assert_eq!(EngineSlot::cost(&p, &ModState::new()), base, "{m:?}");
    }
}

/// Every audio path is billed at its measured cost: the reverb TYPE's extra
/// over QUAD comes off the voice budget, so the dearest TYPE admits fewer
/// voices, and a switch mid-play (both engines, the clear) sheds voices
/// rather than overrun: once the fades end, the booked cost is in budget.
#[test]
fn the_reverb_type_is_paid_for_by_admission() {
    use chimera_core::dsp::rev::{SWITCH_BILLED, extra_cost};
    use chimera_core::dsp::reverb::ReverbType as T;
    // Rev Y's budget leaves QUAD five A16 ∪ A17 voices and 487 to spare:
    // BLOOM's extra (206) fits.
    let rig = || common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_Y));
    let held = |kind: T| {
        let mut r = rig();
        r.shared.fx.reverb.kind = kind;
        *r.shared.parts[5].params = a16_a17_sound();
        r.render(2);
        for k in 0..8 {
            r.note_on(5, 36 + 6 * k);
        }
        r.render(4);
        r.inst.sounding()
    };
    let (quad, bloom) = (held(T::Quad), held(T::Bloom));
    eprintln!("admitted on rev Y: QUAD {quad}, BLOOM {bloom}");
    assert!(extra_cost(T::Bloom) > Cost(0));
    assert_eq!(quad, 5);
    // With the master tape QUAD's five leave 197 spare: BLOOM takes one.
    assert_eq!(
        bloom,
        if TAPE { 4 } else { 5 },
        "BLOOM {bloom} vs QUAD {quad}"
    );

    // QUAD → BLOOM → HALL → QUAD → ROOM mid-play: never more voices than
    // the TYPE's budget holds once the switch and its fades are over.
    let mut r = rig();
    *r.shared.parts[5].params = a16_a17_sound();
    for k in 0..8 {
        r.note_on(5, 36 + 6 * k);
    }
    for kind in [T::Quad, T::Bloom, T::Hall, T::Quad, T::Room] {
        r.shared.fx.reverb.kind = kind;
        r.render(SWITCH_BILLED as usize + 40);
        assert!(r.inst.cost_pct() <= 100, "{kind:?}: {}%", r.inst.cost_pct());
    }
}

/// The costliest routing, as `ModRouting::HEAVY` names it: ENV 2 (B, BURST)
/// on the VCA with its LEVEL routed and a TIME route, FREQ and Q, 16
/// registry columns (the filter's LEVEL among them; no FOLD or DRIVE
/// param), LFOs 1-3 FUNC; LP24 SVF.
fn heavy_routes() -> (ParamSnapshot, ModState) {
    use chimera_core::addr::{BlockRef, Op, ParamAddr};
    use chimera_core::dsp::algo::params::AlgoOpParams;
    use chimera_core::dsp::modulator::{EnvSlot, EnvType, FuncMode, LfoType};
    use chimera_core::params::{EnvParams, FilterParams};
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    (p.envelopes[1].env_type, p.envelopes[1].func.mode) = (EnvType::B, FuncMode::Burst);
    for l in p.lfos.iter_mut().take(3) {
        l.lfo_type = LfoType::Func;
    }
    let mut ms = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
    assert!(ms.set_route_addr(ModSource::Env2.index(), VCA, 127));
    for a in [FixedCol::Freq, FixedCol::Q] {
        assert!(ms.set_route_addr(ModSource::Lfo1.index(), a.addr(), 64));
    }
    let mut regs = vec![
        ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::LEVEL),
        ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::TIME),
        ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL),
    ];
    for op in [Op::A, Op::B, Op::C, Op::D, Op::E, Op::F] {
        for q in [
            AlgoOpParams::LEVEL,
            AlgoOpParams::FEEDBACK,
            AlgoOpParams::DETUNE,
            AlgoOpParams::FINE,
        ] {
            regs.push(ParamAddr::new(BlockRef::AlgoOp(op), q));
        }
    }
    regs.extend([
        ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH),
        ParamAddr::new(BlockRef::Out, chimera_core::params::OutParams::VOLUME),
        ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::RISE),
        ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::FALL),
        ParamAddr::new(BlockRef::Env(EnvSlot::Env2), EnvParams::SHAPE),
        ParamAddr::new(BlockRef::Env(EnvSlot::Env1), EnvParams::LEVEL),
        ParamAddr::new(BlockRef::Env(EnvSlot::Env3), EnvParams::LEVEL),
    ]);
    regs.retain(|a| a.modulatable());
    regs.truncate(16);
    for a in regs {
        assert!(ms.set_route_addr(ModSource::Lfo2.index(), a, 64), "{a:?}");
    }
    assert_eq!(ms.num_dests(), 16);
    (p, ms)
}

#[test]
fn heavy_routes_bill_their_named_sum() {
    let (p, mods) = heavy_routes();
    assert_eq!(ModRouting::cost(&p, &mods), ModRouting::HEAVY);
    assert_eq!(
        Voice::cost(&p, &mods).0 - EngineSlot::cost(&p, &mods).0,
        HEAVY_OVERHEAD.0
    );
}

/// `k` on ALG `a` and `b` at MORPH 64, every operator at LEVEL 99 and
/// FDBK 1. The FRMTs go in first: until one is in, the four-voice budget
/// holds the noise bands.
fn algs_with(a: AlgoId, b: AlgoId, k: [OpKind; OPS]) -> AlgoParams {
    let mut p = AlgoParams::default();
    p.try_set_algs(a, b).unwrap();
    p.morph = 64;
    for op in p.ops.iter_mut() {
        (op.level, op.feedback) = (99, 1);
    }
    let order = (0..OPS)
        .filter(|&i| k[i] == OpKind::Formant)
        .chain((0..OPS).filter(|&i| k[i] != OpKind::Formant));
    for i in order {
        p.try_set_kind(i, k[i]).unwrap();
    }
    p
}

fn a16_a17_with(k: [OpKind; OPS]) -> AlgoParams {
    algs_with(AlgoId::A16, AlgoId::A17, k)
}

/// Recost's reach on a Sound with a FRMT (ADR 0131): its Part's term
/// and its voices' each.
fn reach(p: &AlgoParams) -> (u32, u32) {
    use chimera_core::dsp::voice::HighNote;
    if p.kinds().all().contains(&OpKind::Formant) {
        (HighNote::PART_COST.0, HighNote::VOICE_COST.0)
    } else {
        (0, 0)
    }
}

/// Voices of `p` beside `bus` with `extra` over its engine each, and its
/// formant reach.
fn voices_beside(p: &AlgoParams, bus: Cost, extra: u32) -> u32 {
    let (part, voice) = reach(p);
    (SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0 - bus.0 - CONTROL_RESERVE.0 - part)
        / (AlgoEngine::cost(p, &[false; OPS]).0 + extra + voice)
}

/// Voices of `p` at the heavy routing, with the tape and without.
fn voices(p: &AlgoParams) -> (u32, u32) {
    let v = |bus: Cost| voices_beside(p, bus, HEAVY_OVERHEAD.0);
    (v(FxBus::COST_TAPE), v(FxBus::COST_NO_TAPE))
}

/// Every kind mix the budget holds on each pair in `pairs`, at LEVEL 99
/// and FDBK 1, at the heavy routing, with and without the tape: at least
/// `floor` voices.
fn floor_holds(frmt: bool, floor: u32, pairs: &[(AlgoId, AlgoId)]) {
    let kinds = [OpKind::Table(WaveId::W1), OpKind::Formant, OpKind::Noise];
    let mut seen = 0;
    for &(a, b) in pairs {
        let l = Linked::of(a, b);
        for code in 0..3usize.pow(6) {
            let k: [OpKind; OPS] = core::array::from_fn(|i| kinds[code / 3usize.pow(i as u32) % 3]);
            if k.contains(&OpKind::Formant) != frmt || !fits(&k, l) {
                continue;
            }
            seen += 1;
            let (tape, no_tape) = voices(&algs_with(a, b, k));
            assert!(tape >= floor && no_tape >= floor, "{a:?} {b:?} {k:?}");
        }
    }
    assert!(seen > 0);
}

/// ADR 0026's floor for Sounds of tables and noise bands.
#[test]
fn formant_budget_keeps_four_voices() {
    floor_holds(false, FLOOR_VOICES, &[(AlgoId::A16, AlgoId::A17)]);
}

/// Every pair of algorithms, unordered (`Linked` and the cost are the
/// union of both).
fn all_pairs() -> Vec<(AlgoId, AlgoId)> {
    let ids = || (0..ALGO_COUNT as u8).map(AlgoId::clamped);
    ids()
        .flat_map(|a| {
            ids()
                .filter(move |b| b.get() >= a.get())
                .map(move |b| (a, b))
        })
        .collect()
}

/// The owner's three-voice floor (2026-10-12) for Sounds with a FRMT, on
/// every algorithm pair: a FRMT bills linked where either algorithm links
/// into it. One FRMT past what the budget holds breaks it.
#[test]
fn formant_budget_keeps_three_voices() {
    floor_holds(true, FLOOR_VOICES_FORMANT, &all_pairs());
    let t = OpKind::Table(WaveId::W1);
    // Linked: FRMT on operator 1 of A16 ∪ A17 (modulated); one more is over.
    let mut at_n = [t; OPS];
    at_n[0] = OpKind::Formant;
    assert_eq!(FORMANT_N, 1);
    let over =
        AlgoEngine::cost(&a16_a17_with(at_n), &[false; OPS]).0 + OpKind::Formant.charge(true);
    let budget = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0;
    let (part, voice) = reach(&a16_a17_with(at_n));
    let floor = |over: u32| {
        (budget - FxBus::COST_TAPE.0 - CONTROL_RESERVE.0 - part) / (over + HEAVY_OVERHEAD.0 + voice)
    };
    assert!(floor(over) < FLOOR_VOICES_FORMANT);
    // Unlinked, on A1: `FORMANT_N_UNLINKED` carriers, then one more.
    let mut at_n = [t; OPS];
    for slot in at_n.iter_mut().take(FORMANT_N_UNLINKED as usize) {
        *slot = OpKind::Formant;
    }
    let p = algs_with(AlgoId::A1, AlgoId::A1, at_n);
    assert!(voices(&p).0 >= FLOOR_VOICES_FORMANT);
    let over = AlgoEngine::cost(&p, &[false; OPS]).0 + OpKind::Formant.charge(false);
    assert!(floor(over) < FLOOR_VOICES_FORMANT);
}

/// A FRMT bills its counted lanes, linked when ALG A or B runs a link into
/// it, and its law moving always: the pitch wheel and a glide steal move
/// it with no route to see, as Modal's pitch (#437).
#[test]
fn a_formant_bills_its_lanes_and_its_moving_law() {
    use chimera_core::dsp::algo::algorithms::ALGORITHMS;
    let t = [OpKind::Table(WaveId::W1); OPS];
    let base = cost(&a16_a17_with(t));
    let (a, b) = (
        &ALGORITHMS[AlgoId::A16.get() as usize],
        &ALGORITHMS[AlgoId::A17.get() as usize],
    );
    let modded = (0..OPS).fold(0u8, |m, i| m | a.mods[i] | b.mods[i]);
    let l = Linked::of(AlgoId::A16, AlgoId::A17);
    let (mut linked_seen, mut unlinked_seen) = (false, false);
    for op in 0..OPS {
        let linked = modded & (1 << op) != 0;
        assert_eq!(l.has(op), linked);
        linked_seen |= linked;
        unlinked_seen |= !linked;
        for skirt in [0, 3] {
            let mut k = t;
            k[op] = OpKind::Formant;
            let mut p = a16_a17_with(k);
            p.ops[op].skirt = skirt;
            let lanes = match (skirt == 0, linked) {
                (true, false) => AlgoEngine::COST_FRMT_ONE,
                (false, false) => AlgoEngine::COST_FRMT,
                (true, true) => AlgoEngine::COST_FRMT_ONE_LINKED,
                (false, true) => AlgoEngine::COST_FRMT_LINKED,
            };
            let moving = if linked {
                AlgoEngine::COST_FRMT_MOVE_LINKED
            } else {
                AlgoEngine::COST_FRMT_MOVE
            };
            assert_eq!(
                cost(&p) - base,
                lanes.0 + moving.0 - AlgoEngine::COST_OP.0,
                "op {op} skirt {skirt}"
            );
            // Unrouted, the slot bills just that.
            let mut s = ParamSnapshot::for_engine(EngineType::Algo);
            s.algo = p;
            assert_eq!(
                EngineSlot::cost(&s, &ModState::new()),
                AlgoEngine::cost(&p, &UNROUTED)
            );
        }
    }
    assert!(linked_seen && unlinked_seen);
    // Selecting a FRMT is never billed below its counted worst case.
    for linked in [false, true] {
        assert_eq!(
            OpKind::Formant.charge(linked) + AlgoEngine::COST_OP.0,
            AlgoEngine::frmt_cost(1, linked)
        );
    }
    assert!(OpKind::Formant.charge(true) + AlgoEngine::COST_OP.0 >= AlgoEngine::COST_FRMT_LINKED.0);
}

/// The FRMT terms cover the worst block of a grain clock up to C8: seven
/// spans (fires, linked). C7 runs four, MIDI 127 eighteen.
#[test]
fn the_frmt_terms_cover_spans_up_to_c8() {
    let hz = |n: f32| 440.0 * 2f32.powf((n - 69.0) / 12.0);
    assert_eq!(AlgoEngine::spans(hz(108.0)), AlgoEngine::SPANS_COVERED);
    assert_eq!(AlgoEngine::SPANS_COVERED, 7);
    assert_eq!(AlgoEngine::spans(hz(96.0)), 4);
    assert_eq!(AlgoEngine::spans(hz(127.0)), 18);
    assert_eq!(AlgoEngine::spans(0.0), 1);
    assert_eq!(AlgoEngine::spans(1e9), 65);
}

/// A formant voice whose pitch can reach past C8 pays its extra spans per
/// voice: none at C8, ADR 0056's cycles past it; a linked FRMT pays its
/// fires. Bend, TRNSP, TUNING, PITCH routes, a glide's start and a routed
/// FINE all raise the reach.
#[test]
fn a_formant_voice_past_c8_pays_its_extra_spans() {
    use chimera_core::MidiNote;
    use chimera_core::addr::{BlockRef, Op, ParamAddr};
    use chimera_core::dsp::algo::params::AlgoOpParams;
    let note = |n| MidiNote::new(n).unwrap();
    let mut s = ParamSnapshot::for_engine(EngineType::Algo);
    s.algo = AlgoParams::single(WaveId::W1);
    s.algo.try_set_algs(AlgoId::A1, AlgoId::A1).unwrap();
    s.algo.try_set_kind(0, OpKind::Formant).unwrap();
    let m = ModState::new();
    let x = |s: &ParamSnapshot, m: &ModState, bend, tune, n, slide| {
        Voice::high_note_extra(s, m, bend, tune, note(n), slide).0
    };
    assert_eq!(x(&s, &m, 0, 0, 108, 1.0), 0);
    // MIDI 127, unlinked: 11 spans past, 276 instructions each.
    let over = (11 * 276 * 1606u32).div_ceil(64_000);
    assert_eq!(x(&s, &m, 0, 0, 127, 1.0), over);
    // Each reach past C8 bills.
    // Each reach past C8 bills (MIDI 111 already runs a span more).
    let base = x(&s, &m, 0, 0, 111, 1.0);
    assert!(base > 0);
    assert!(x(&s, &m, 12, 0, 111, 1.0) > base, "bend");
    assert!(x(&s, &m, 0, 100, 111, 1.0) > base, "tune");
    assert_eq!(
        x(&s, &m, 0, -100, 111, 1.0),
        base,
        "tune down never lowers it"
    );
    assert!(
        x(&s, &m, 0, 0, 111, 2.0) > base,
        "a glide from an octave up"
    );
    let mut t = s.clone();
    t.algo.transpose = 12;
    assert!(x(&t, &m, 0, 0, 111, 1.0) > base, "TRNSP");
    let mut c = s.clone();
    c.algo.ops[0].coarse = 8;
    assert!(x(&c, &m, 0, 0, 111, 1.0) > base, "COARSE");
    let mut routed = ModState::from_registry(&chimera_core::mod_path::ModDestRegistry::new(), 8);
    let fine = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::FINE);
    if fine.modulatable() {
        assert!(routed.set_route_addr(ModSource::Lfo1.index(), fine, 64));
        assert!(x(&s, &routed, 0, 0, 111, 1.0) > base, "FINE route");
    }
    // Linked (T1 links 2 → 1): the fires, 47 instructions each.
    let mut l = s.clone();
    l.algo.try_set_algs(AlgoId::T1, AlgoId::T1).unwrap();
    assert_eq!(
        x(&l, &m, 0, 0, 127, 1.0),
        (11 * 47 * 1606u32).div_ceil(64_000)
    );
    // Not a formant Sound: nothing.
    let t = ParamSnapshot::for_engine(EngineType::Algo);
    assert_eq!(x(&t, &m, 24, 0, 127, 4.0), 0);
}

/// Admission and the per-block recost both bill a formant voice's spans
/// past C8: a CHOIR note at MIDI 127 books its extra over one at 60, and
/// keeps it while it sounds.
#[test]
fn a_high_formant_note_is_booked_its_extra_spans() {
    use chimera_core::factory::{Demo, demo_sound};
    let booked = |n: u8| {
        let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
        let s = demo_sound(Demo::Choir);
        (*r.shared.parts[0].params, *r.shared.parts[0].mod_state) = (s.params, s.mod_state);
        let ch = r.shared.parts[0].mix.channel.get();
        r.note_on(ch, n);
        let at_on = r
            .inst
            .allocator()
            .slots()
            .iter()
            .map(|s| s.cost().0)
            .sum::<u32>();
        r.render(2);
        let held = r
            .inst
            .allocator()
            .slots()
            .iter()
            .map(|s| s.cost().0)
            .sum::<u32>();
        (at_on, held)
    };
    let (low, low_held) = booked(60);
    let (high, high_held) = booked(127);
    let p = demo_sound(Demo::Choir);
    let shared = AudioShared::default();
    let extra = Voice::high_note_extra(
        &p.params,
        &p.mod_state,
        shared.parts[0].mix.bend,
        0,
        chimera_core::MidiNote::new(127).unwrap(),
        1.0,
    )
    .0;
    assert!(extra > 0);
    assert_eq!(high - low, extra);
    assert_eq!(high_held - low_held, extra);
}

/// Recost takes a Part's reach once a block (`HighNote::of`) and each
/// voice's extra from it: the bill `Voice::high_note_extra` gives alone.
#[test]
fn a_part_s_high_note_reach_bills_each_voice_as_alone() {
    use chimera_core::MidiNote;
    use chimera_core::dsp::voice::HighNote;
    use chimera_core::factory::{Demo, demo_sound};
    let mut linked = demo_sound(Demo::Choir);
    linked.params.algo = AlgoParams::single(WaveId::W1);
    linked
        .params
        .algo
        .try_set_algs(AlgoId::T1, AlgoId::T1)
        .unwrap();
    linked.params.algo.try_set_kind(0, OpKind::Formant).unwrap();
    linked.params.algo.ops[0].coarse = 3;
    let table = chimera_core::preset::Sound::plain_init(EngineType::Algo);
    for s in [demo_sound(Demo::Choir), linked, table] {
        for (bend, tune) in [(0, 0), (12, 100), (2, -100)] {
            let h = HighNote::of(&s.params, &s.mod_state, bend, tune);
            for n in [0, 60, 108, 111, 120, 127] {
                for slide in [0.5, 1.0, 2.0] {
                    let note = MidiNote::new(n).unwrap();
                    let alone =
                        Voice::high_note_extra(&s.params, &s.mod_state, bend, tune, note, slide);
                    let part = h.map_or(chimera_core::hw::Cost::ZERO, |h| h.extra(note, slide));
                    assert_eq!(part, alone, "{n} {slide} {bend} {tune}");
                }
            }
        }
    }
}

/// Recost's formant reach is billed at its count (ADR 0131): a formant
/// Part's `HighNote::PART_COST` once, on its first voice (as its knobs),
/// and `HighNote::VOICE_COST` on each voice, the same at admission and at
/// the recost after; a table Part pays neither.
#[test]
fn a_formant_part_bills_its_reach_once_and_each_voice() {
    use chimera_core::dsp::knobs::Knobs;
    use chimera_core::dsp::voice::HighNote;
    use chimera_core::factory::{Demo, demo_sound};
    let mut linked = demo_sound(Demo::Choir);
    linked.params.algo = AlgoParams::single(WaveId::W1);
    linked
        .params
        .algo
        .try_set_algs(AlgoId::T1, AlgoId::T1)
        .unwrap();
    linked.params.algo.try_set_kind(0, OpKind::Formant).unwrap();
    let table = chimera_core::preset::Sound::plain_init(EngineType::Algo);
    for (s, frmt) in [
        (demo_sound(Demo::Choir), true),
        (linked, true),
        (table, false),
    ] {
        let mut r = common::InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
        (*r.shared.parts[0].params, *r.shared.parts[0].mod_state) = (s.params, s.mod_state);
        r.shared.parts[0].params.envelopes[0].sustain = 1.0;
        let ch = r.shared.parts[0].mix.channel.get();
        let voice = Voice::cost(&r.shared.parts[0].params, &r.shared.parts[0].mod_state).0;
        let (part_term, voice_term) = if frmt {
            (HighNote::PART_COST.0, HighNote::VOICE_COST.0)
        } else {
            (0, 0)
        };
        let bill = |r: &common::InstRig| {
            r.inst
                .allocator()
                .slots()
                .iter()
                .map(|s| s.cost().0)
                .sum::<u32>()
        };
        for (k, n) in [48u8, 52, 55].into_iter().enumerate() {
            r.note_on(ch, n);
            let want = (k as u32 + 1) * (voice + voice_term) + Knobs::COST.0 + part_term;
            assert_eq!(bill(&r), want, "admission, {} voices, frmt {frmt}", k + 1);
            r.render(2);
            assert_eq!(bill(&r), want, "recost, {} voices, frmt {frmt}", k + 1);
        }
    }
}

/// The costliest Algo patch the formant budget admits (ADR 0131): every
/// kind mix with a FRMT on every algorithm pair, at LEVEL 99 and FDBK 1.
fn frmt_max() -> AlgoParams {
    let kinds = [OpKind::Table(WaveId::W1), OpKind::Formant, OpKind::Noise];
    let mut best: Option<(u32, AlgoParams)> = None;
    for (a, b) in all_pairs() {
        let l = Linked::of(a, b);
        for code in 0..3usize.pow(6) {
            let k: [OpKind; OPS] = core::array::from_fn(|i| kinds[code / 3usize.pow(i as u32) % 3]);
            if !k.contains(&OpKind::Formant) || !fits(&k, l) {
                continue;
            }
            let p = algs_with(a, b, k);
            if best.is_none_or(|(c, _)| cost(&p) > c) {
                best = Some((cost(&p), p));
            }
        }
    }
    best.unwrap().1
}

/// Voices of `p` on rev V beside `bus`, with `extra` over its engine.
fn voices_with(p: &AlgoParams, bus: Cost, extra: u32) -> u32 {
    voices_beside(p, bus, extra)
}

/// ADR 0131: each kind bills its counted term, and the costliest patch
/// the budget admits (the bench's FRMT MAX row) keeps the three-voice
/// floor at the heavy routing, with the tape and without. The table:
/// each demo Sound's voices at the light routing beside the whole bus.
#[test]
fn formant_costs_bill_by_kind() {
    use chimera_core::factory::{Demo, demo_sound};
    let d = |x: Demo| demo_sound(x).params.algo;
    // BREATH: three unlinked FRMT (SKIRT 2, 1, 1) and a noise band.
    assert_eq!(
        cost(&d(Demo::Breath)),
        AlgoEngine::COST_BASE.0 + 3 * AlgoEngine::frmt_cost(1, false) + AlgoEngine::COST_NOIS.0
    );
    let worst = frmt_max();
    let t = OpKind::Table(WaveId::W1);
    let (f, n) = (OpKind::Formant, OpKind::Noise);
    // The bench's FRMT MAX row (five other pairs with A16 tie it): T1 ∪
    // A16, a linked FRMT on 2, the unlinked one on 6, NOIS on 1 and 4.
    assert_eq!(
        cost(&worst),
        cost(&algs_with(AlgoId::T1, AlgoId::A16, [n, f, t, n, t, f]))
    );
    let floor = HEAVY_OVERHEAD.0;
    for bus in [FxBus::COST_TAPE, FxBus::COST_NO_TAPE] {
        assert!(voices_with(&worst, bus, floor) >= FLOOR_VOICES_FORMANT);
    }
    let light = ModRouting::BASE.0 + Voice::CHAIN_COST.0;
    for (name, p) in [
        ("BREATH", d(Demo::Breath)),
        ("CHOIR", d(Demo::Choir)),
        ("TALK BASS", d(Demo::TalkBass)),
        ("VOX", d(Demo::Vox)),
        ("MAX", worst),
    ] {
        let (part, voice) = reach(&p);
        let y = |bus: chimera_core::hw::Cost| {
            ((SampleBudget::for_cpu(CPU_HZ_REV_Y).as_cost().0 - bus.0 - CONTROL_RESERVE.0 - part)
                / (cost(&p) + light + voice))
                .min(MAX_VOICES as u32)
        };
        let (y, y_tape) = (y(FxBus::COST), y(FxBus::COST_TAPE));
        println!(
            "{name}: cost {} | rev V {} / rev Y {y} ({y_tape} with the tape) | heavy {}",
            cost(&p),
            voices_with(&p, FxBus::COST, light).min(MAX_VOICES as u32),
            voices_with(&p, FxBus::COST_TAPE, floor)
        );
    }
}

/// ADR 0031 holds beside the formant kinds: the costliest table patch
/// still gets six voices on rev V.
#[test]
fn the_costliest_table_patch_still_gets_six_voices_on_rev_v() {
    let light = ModRouting::BASE.0 + Voice::CHAIN_COST.0;
    assert!(voices_with(&costliest(), FxBus::COST, light) >= 6);
}

/// A NOIS with a link in runs `run_noise::<1>` and the links' sum: it
/// bills its linked term, as a FRMT does.
#[test]
fn a_noise_band_bills_its_link() {
    let t = [OpKind::Table(WaveId::W1); OPS];
    let base = cost(&a16_a17_with(t));
    let l = Linked::of(AlgoId::A16, AlgoId::A17);
    for op in 0..OPS {
        let mut k = t;
        k[op] = OpKind::Noise;
        let term = if l.has(op) {
            AlgoEngine::COST_NOIS_LINKED
        } else {
            AlgoEngine::COST_NOIS
        };
        // NOIS takes no feedback.
        let fb = AlgoEngine::COST_FEEDBACK.0;
        assert_eq!(
            cost(&a16_a17_with(k)) + AlgoEngine::COST_OP.0 + fb,
            base + term.0,
            "op {op}"
        );
        assert_eq!(
            OpKind::Noise.charge(l.has(op)),
            term.0 - AlgoEngine::COST_OP.0
        );
    }
    const { assert!(AlgoEngine::COST_NOIS_LINKED.0 > AlgoEngine::COST_NOIS.0) };
}
