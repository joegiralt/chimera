//! CPU costs (ADR 0013): cycles/sample per voice, measured on the bench
//! (rev V at 480 MHz, 2026-09-27) and rounded up.

use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::engines::Engines;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, CPU_HZ_REV_Y, Cost, MAX_VOICES, SampleBudget};
use chimera_core::modulation::{ModRouting, ModSource, ModState, VCA};
use chimera_core::params::{EngineType, ParamSnapshot};

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
    (p.alg_a, p.alg_b, p.morph) = (AlgoId::A14.get(), AlgoId::A22.get(), 64);
    for op in p.ops.iter_mut() {
        (op.level, op.feedback) = (99, 7);
    }
    p
}

/// Bench `MODAL /VOICE` 391: minus the chain's floor (5), rounded up to the
/// next 10, plus `CHAIN_COST`. Algo is priced from its patch.
#[test]
fn voice_costs_are_the_bench_measurements() {
    assert_eq!(Voice::CHAIN_COST, Cost(10));
    assert_eq!(voice_cost(EngineType::Modal), Cost(400) + ModRouting::BASE);
    let mods = ModState::new();
    for e in EngineType::ALL {
        let p = ParamSnapshot::for_engine(e);
        assert_eq!(
            Voice::cost(&p, &mods),
            Engines::cost(&p, &mods) + Voice::CHAIN_COST + ModRouting::BASE,
            "{e:?}"
        );
    }
}

fn routed(routes: &[ModSource]) -> ModState {
    let mut reg = chimera_core::mod_path::ModDestRegistry::new();
    reg.add(VCA, *b"OUT VCA\0").unwrap();
    let mut ms = ModState::from_registry(&reg, 8);
    for s in routes {
        ms.set_route(s.index(), 0, 127);
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
        chimera_core::preset::Sound::init(chimera_core::params::EngineType::Algo).mod_state;
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
    let mut ms = chimera_core::preset::Sound::init(EngineType::Algo).mod_state;
    let f = ms.push(fold).unwrap();
    ms.set_route(ModSource::Lfo3.index(), f, 0);
    assert_eq!(M::cost(&p, &ms), M::BASE, "amount 0");
    let l = ms.push(level(EnvSlot::Env1)).unwrap();
    ms.set_route(ModSource::Vel.index(), l, 64);
    assert_eq!(
        M::cost(&p, &ms),
        M::BASE + M::DEST_FIRST,
        "LEVEL off the VCA"
    );
    ms.set_route(ModSource::Lfo3.index(), f, 64);
    assert_eq!(M::cost(&p, &ms), M::BASE + M::DEST_FIRST + M::DEST);
    let cutoff = ms.find(chimera_core::modulation::CUTOFF).unwrap();
    ms.set_route(ModSource::Env1.index(), cutoff, 32);
    ms.set_route(ModSource::Note.index(), cutoff, 32);
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
    // Stored at 0, a route: 0 bills nothing, anything else both terms.
    let fold = ParamAddr::new(BlockRef::Folder, FolderParams::FOLD);
    let drive = ParamAddr::new(BlockRef::Drive, DriveParams::DRIVE);
    let empty = chimera_core::mod_path::ModDestRegistry::new();
    let mut ms = ModState::from_registry(&empty, 8);
    let (f, d) = (ms.push(fold).unwrap(), ms.push(drive).unwrap());
    ms.set_route(ModSource::Vel.index(), f, 0);
    ms.set_route(ModSource::Vel.index(), d, 0);
    assert_eq!(Voice::cost(&base, &ms), bare);
    ms.set_route(ModSource::Vel.index(), f, -1);
    ms.set_route(ModSource::Vel.index(), d, 1);
    let routes = ModRouting::DEST_FIRST + ModRouting::DEST;
    assert_eq!(
        Voice::cost(&base, &ms),
        bare + Voice::FOLD_COST + Voice::DRIVE_COST + routes
    );
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

/// What the 7,000-cycle budget allows with the FX bus (the whole bus at
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

/// A14 ∪ A22 is eight links. The bench's `WC /VOICE` read 790.
#[test]
fn the_worst_case_is_the_sum_of_its_terms() {
    let sum = AlgoEngine::COST_BASE.0
        + 6 * AlgoEngine::COST_OP.0
        + 8 * AlgoEngine::COST_LINK.0
        + 6 * AlgoEngine::COST_FEEDBACK.0;
    assert_eq!(cost(&worst()), sum);
    assert_eq!(Voice::CHAIN_COST.0 + sum, 810, "measured 790");
}

/// Each bench row's `/VOICE` reading, 2026-09-27, is billed at or above.
#[test]
fn the_model_bills_every_bench_row_high() {
    let row = |alg: AlgoId, lit: &[usize], fb: u8| {
        let mut p = AlgoParams::default();
        (p.alg_a, p.alg_b) = (alg.get(), alg.get());
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
}

/// The Task 13 bench (rev V, 480 MHz, 2026-09-28, the bench-t13c run):
/// each ROUTING row billed at or above its reading. Every row is 1 OP (A1)
/// but A16+17. SVF is left to Task 15's `FilterKind::cost`. The rows mirror
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
    for (name, p, ms, measured) in [
        ("1 OP", one(), ModState::new(), 483),
        // BENCH screen, the 13b run.
        ("A16+17", a16, ModState::new(), 861),
        ("MODS", mods_row().0, mods_row().1, 668),
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
            ParamAddr::new(BlockRef::Filter, FilterParams::DRIVE),
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
        let d = ms.find(a).or_else(|| ms.push(a)).unwrap();
        ms.set_route(s.index(), d, 64);
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
    );
}

/// The patch the model prices highest: all six operators with feedback on
/// the algorithm pair whose union has the most links.
fn costliest() -> AlgoParams {
    let mut p = worst();
    let mut best = (0, p);
    for a in 0..32u8 {
        for b in 0..32u8 {
            (p.alg_a, p.alg_b) = (a, b);
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
    let voice = Voice::CHAIN_COST.0 + ModRouting::BASE.0 + cost(p);
    ((budget - FxBus::COST.0) / voice).min(MAX_VOICES as u32)
}

/// ADR 0026: a one-operator patch gets every voice.
#[test]
fn a_one_operator_patch_fits_six_voices() {
    let one = AlgoParams::single(chimera_core::dsp::algo::waves::WaveId::W1);
    assert_eq!(voices_beside_fx(&one), MAX_VOICES as u32);
}

/// A16 ∪ A17 at MORPH 64, six operators with feedback: 12 links, ADR
/// 0026's costliest shape.
fn a16_a17() -> AlgoParams {
    let mut p = worst();
    (p.alg_a, p.alg_b) = (AlgoId::A16.get(), AlgoId::A17.get());
    p
}

/// FX diet spec § Intent and ADR 0031: with the bus measured at 1,360 and
/// the modulator pool's floor (`ModRouting::BASE`, 47) added, the costliest
/// patch still gets six voices on rev V (6 × 889 + 1,360 = 6,694 ≤ 7,000),
/// and five on rev Y ((5,833 − 1,360) / 889 = 5.03), without FOLD or DRIVE.
#[test]
fn the_costliest_patch_gets_six_voices_on_rev_v() {
    let p = a16_a17();
    assert_eq!(Voice::CHAIN_COST.0 + cost(&p), 842);
    assert_eq!(Voice::CHAIN_COST.0 + ModRouting::BASE.0 + cost(&p), 889);
    assert_eq!(cost(&costliest()), cost(&p), "no pair has more links");
    assert_eq!(FxBus::COST.0, 1_360, "{:?}", FxBus::COST);
    assert_eq!(voices_at(CPU_HZ_REV_V, &p), MAX_VOICES as u32);
    assert_eq!(voices_at(CPU_HZ_REV_Y, &p), 5);
    // With the folder on (43): 932, still six on rev V, four on rev Y; the
    // drive stage too (997, DRIVE provisional): five on rev V, four on rev Y. No factory Sound
    // does either with this shape.
    let fits = |hz, voice: u32| (SampleBudget::for_cpu(hz).as_cost().0 - FxBus::COST.0) / voice;
    let fold = 889 + Voice::FOLD_COST.0;
    assert_eq!((fits(CPU_HZ_REV_V, fold), fits(CPU_HZ_REV_Y, fold)), (6, 4));
    let both = fold + Voice::DRIVE_COST.0;
    assert_eq!((fits(CPU_HZ_REV_V, both), fits(CPU_HZ_REV_Y, both)), (5, 4));
}

/// Spec § Intent and ADR 0031: every factory Sound gets six voices on rev
/// V and at least five on rev Y, billed as it plays (its routes, FOLD and
/// DRIVE in). MORPH KEYS, the costliest, bills 844: five on rev Y.
#[test]
fn every_factory_sound_gets_six_voices_on_rev_v() {
    for i in 0..8 {
        let s = chimera_core::factory::factory_sound(i).unwrap();
        let voice = Voice::cost(&s.params, &s.mod_state).0;
        let at = |hz| {
            let budget = SampleBudget::for_cpu(hz).as_cost().0;
            ((budget - FxBus::COST.0) / voice).min(MAX_VOICES as u32)
        };
        assert_eq!(at(CPU_HZ_REV_V), 6, "factory {i}");
        assert!(at(CPU_HZ_REV_Y) >= 5, "factory {i} on rev Y");
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
            (p.alg_a, p.alg_b) = (a, b);
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
        (p.alg_a, p.alg_b) = (from.get(), from.get());
        let fewer = cost(&p);
        p.alg_a = to.get();
        assert!(cost(&p) > fewer, "{from:?} to {to:?}");
    }
}

/// Links are priced by their target: A17 is 6→5→4→3→2→1, so silencing
/// operator 1 drops the link 2→1, and silencing operator 6 (a source only)
/// drops none.
#[test]
fn links_are_priced_by_their_active_target() {
    let mut all = worst();
    (all.alg_a, all.alg_b) = (AlgoId::A17.get(), AlgoId::A17.get());
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
