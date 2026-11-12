//! Every factory Sound's bill is an upper bound of what the unit read it
//! costs (#448, #883, ADR 0249): `Voice::cost` at or over a model built
//! only from the worst-block readings (`~/dev/unit-bench-883b/`,
//! rev V at 480 MHz, 2026-11-11, 280c8c2e), term by term, each the worst
//! row that holds it. Run with `--nocapture` for the census: bill, model
//! and voices on rev V per Sound.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::filter::k2040::K2040Voice;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::modulator::{EnvSlot, EnvType, FuncMode, LfoType};
use chimera_core::dsp::va::noise::NoiseKind;
use chimera_core::dsp::va::{VaMode, VaParams};
use chimera_core::dsp::voice::Voice;
use chimera_core::dsp::voice_fx::params::FxType;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::modulation::{FixedCol, ModRouting, ModSource, ModState, VCA};
use chimera_core::params::{EngineType, EnvParams, ParamSnapshot};
use chimera_core::params::{FilterParams, OutParams};
use chimera_core::preset::Sound;
use chimera_desktop::factory_sounds::{build, source};
use std::path::Path;

// Worst-block rows, cycles a sample a voice: each row's costliest block
// at 1..8 voices, every effect at its worst and every send at ½, the
// slope (`W`) or, where a voice climbs with its note (VA, ALGO's FRMT and
// table rows), the costliest one voice added (the top's). Every row
// carries only its own routes (`matrix`, ADR 0218). What a block costs
// with no voice is not a voice's: it is the block's own (#883).

/// 1 DEST (1 OP, CLEAN LP24, LFO 1 → MORPH) less one destination: the bare
/// one-operator voice, chain, pool, an LFO and CLEAN in it.
const BARE: u32 = 470 - DEST_FIRST;
/// The first destination's sum and offset: 1 CUTOFF − 1 OP.
const DEST_FIRST: u32 = 463 - 455;
/// (VEL 4R − VEL 1R) / 3, up: each further destination, outputs' ramps
/// and all.
const DEST: u32 = 5;
/// VEL VCA − BARE: the VCA's clamp with one plain source.
const VCA_OTHER: u32 = 491 - BARE;
/// 2 VCA − VEL VCA: each further plain source.
const VCA_MORE: u32 = 499 - 491;
/// A VCA − BARE: an ENV of type A on the VCA, the clamp in it.
const VCA_ENV_A: u32 = 511 - BARE;
/// B GLIDE − BARE: type B on the VCA, its costliest steady row.
const VCA_ENV_B: u32 = 605 - BARE;
/// BURST AD − BARE: type B in BURST.
const VCA_BURST: u32 = 663 - BARE;
/// A SLIDE − A VCA: a slot's TIME/RISE/FALL/SHAPE routes, their four
/// destinations in it.
const SLIDE: u32 = 548 - 511;
/// FUNC LFO − 1 DEST.
const FUNC: u32 = 472 - 470;
/// ALGO's table operators and links over 1 OP (ALGO's 463), scaled from
/// their bill: the worst table row over bill, each at the higher of its
/// slope and its top voice: CHAIN (853 − 463) / (853 − 513) = 1.147.
const ALGO_SCALE: f64 = 1.15;
/// A formant or noise operator's charge over a table operator's, the same
/// way, less its tables at `ALGO_SCALE`: FRMT PM's costliest added voice
/// (942 − 463 − 1.15 × 68) / 390 = 1.028; NOIS's (500 − 463) / 20 = 1.85.
const FRMT_SCALE: f64 = 1.03;
const NOIS_SCALE: f64 = 1.85;
/// The filter kinds over CLEAN (SVF 455): 2040 R2, 2040 (EXP), LADDER, SEM;
/// DIRTY and NOTCH2 read under CLEAN.
const K2040_REV2: u32 = 554 - 455;
const K2040_EXP: u32 = 602 - 455;
const LADDER: u32 = 656 - 455;
const SEM: u32 = 512 - 455;
/// A moving filter, CUTOFF stepped every block, over its kind's steady
/// row: CLEAN MOVE − SVF, 2040 SWEEP − 2040 R2, LADDER SWEEP − LADDER, SEM
/// SWEEP − SEM.
const fn moving(k: FilterKind) -> u32 {
    match k {
        FilterKind::Svf => 484 - 455,
        // DIRTY's one loop reads its arrays every block: nothing moving adds.
        FilterKind::Dirty => 0,
        FilterKind::K2040 => 579 - 554,
        FilterKind::Ladder => 688 - 656,
        FilterKind::Sem => 541 - 512,
    }
}
/// FB at ½ over each kind's row: the moving path every block and FB.
const fn fb(k: FilterKind) -> u32 {
    match k {
        FilterKind::Svf => 669 - 455,
        FilterKind::Dirty => 566 - 451,
        FilterKind::K2040 => 796 - 602,
        FilterKind::Ladder => 830 - 656,
        FilterKind::Sem => 643 - 512,
    }
}
/// STABILITY ½ over the chip's row, the costliest (2040 STAB − 2040).
const STAB: u32 = 607 - 602;
/// LP24 HOT − SVF: CLEAN's LEVEL clipper driven at LEVEL 1, RES 1.
const LEVEL: u32 = 526 - 455;
/// TAP FREQ − 1 OP less CLEAN's moving path (the filter's, counted
/// there), and TAP VCA − 1 OP.
const TAP_FREQ: u32 = 656 - 455 - (484 - 455);
const TAP_VCA: u32 = 500 - 455;
/// The FX slot over 1 OP: FOLD, SOFT LO (SOFT's worst), FUZZ; FOLD TAP.
const FOLD: u32 = 536 - 455;
const SOFT: u32 = 547 - 455;
const FUZZ: u32 = 696 - 455;
const FOLD_TAP: u32 = 640 - 455;
/// A routed slot's ramp: SOFT RT, FOLD RT, FUZZ MOVE less 1 DEST and the
/// type at that P1 (SOFT, FOLD HALF, FUZZ).
const SOFT_RAMP: u32 = 577 - 470 - (538 - 455);
const FOLD_RAMP: u32 = 570 - 470 - (533 - 455);
const FUZZ_RAMP: u32 = 751 - 470 - FUZZ;
/// AMP's gate: LPG RT − A VCA.
const GATE: u32 = 613 - 511;
/// VA NOISE over VA, by KIND.
const fn noise(k: NoiseKind) -> u32 {
    match k {
        NoiseKind::White => 629 - 614,
        NoiseKind::Pink => 652 - 614,
        NoiseKind::Red => 647 - 614,
        NoiseKind::Digital => 643 - 614,
    }
}
/// MODAL's engine bills over its rows: each model's least bill over its
/// worst-block slope (B − W), so its engine reads at most its bill less
/// that. STRING: STR+ 672 − 601; BOWED: BOW+ 651 − 640; SYMPATHETIC: SYM+
/// 1001 − 987; MODAL: RES 1953 − 883.
const fn modal_slack(m: ResonatorMode) -> u32 {
    match m {
        ResonatorMode::String => 672 - 601,
        ResonatorMode::Bowed => 651 - 640,
        ResonatorMode::Sympathetic => 1001 - 987,
        ResonatorMode::Modal => 1953 - 883,
    }
}
/// VA's WAVE crossing a segment every block: VA WAVE − VA.
const VA_WAVE: u32 = 706 - 614;
/// VA's MODE rows at their top voice, less the 2040 REV2 they play: the
/// voice to CLEAN's equal.
const fn va_core(m: VaMode) -> u32 {
    let top = match m {
        VaMode::Mix => 669,
        VaMode::Sync => 696,
        VaMode::Ring => 675,
        VaMode::Xor => 673,
        VaMode::Bits => 691,
        VaMode::Fm => 731,
        VaMode::Am => 680,
        VaMode::Max => 667,
        VaMode::LoopFm => 626,
        VaMode::SoftSync => 716,
        VaMode::Phase => 715,
    };
    top - K2040_REV2
}

/// The ALGO terms as billed when the rows were read (883b, 280c8c2e), so
/// the model stays the readings' as the bills move: base, a table
/// operator, a link and FDBK; a FRMT's and a NOIS's charges over a table
/// operator.
const OP: u32 = 60;
const LINK: u32 = 8;
const FEEDBACK: u32 = 1;
const fn frmt_charge(skirt: u8, linked: bool) -> u32 {
    match (skirt == 0, linked) {
        (true, false) => 210 + 40 - OP,
        (false, false) => 220 + 40 - OP,
        (true, true) => 380 + 60 - OP,
        (false, true) => 390 + 60 - OP,
    }
}
const fn nois_charge(linked: bool) -> u32 {
    if linked { 100 - OP } else { 80 - OP }
}
/// The bench's 1 OP (A1, OP1 a table at LEVEL 99): its tables' term.
const ONE_OP_TABLES: u32 = OP;

/// A patch's tables (every priced operator as a table, its links and
/// FDBK) and its FRMT and NOIS charges, at the readings' terms, as
/// `AlgoEngine::cost` counts them.
fn algo_shape(p: &AlgoParams, routed: &[bool; OPS]) -> (u32, u32, u32) {
    use chimera_core::dsp::algo::algorithms::ALGORITHMS;
    use chimera_core::dsp::algo::plan::MAX_EDGES;
    let (a, b) = p.algs();
    let (a, b) = (&ALGORITHMS[a.get() as usize], &ALGORITHMS[b.get() as usize]);
    let linked = p.linked();
    let (mut active, mut tables, mut frmt, mut nois) = (0u8, 0, 0, 0);
    for i in (0..OPS).filter(|&i| p.ops[i].level > 0 || routed[i]) {
        active |= 1 << i;
        tables += OP;
        let kind = p.kinds().get(i);
        match kind {
            OpKind::Formant => frmt += frmt_charge(p.ops[i].skirt, linked.has(i)),
            OpKind::Noise => nois += nois_charge(linked.has(i)),
            OpKind::Table(_) => {}
        }
        if p.ops[i].feedback > 0 && !matches!(kind, OpKind::Noise) {
            tables += FEEDBACK;
        }
    }
    let links: u32 = (0..OPS)
        .map(|i| ((a.mods[i] | b.mods[i]) & active).count_ones())
        .sum();
    tables += links.min(MAX_EDGES as u32) * LINK;
    (tables, frmt, nois)
}

/// ALGO's FREQ or Q tap: the filter moves, as a route moves it.
fn shaped(p: &ParamSnapshot) -> bool {
    p.engine() == EngineType::Algo && (p.taps.moving(FixedCol::Freq) || p.taps.moving(FixedCol::Q))
}

/// A CUTOFF or RES route (a FREQ or Q tap's move is in `TAP_FREQ`).
fn filter_moves(m: &ModState) -> bool {
    m.fixed_moves(FixedCol::Freq) || m.fixed_moves(FixedCol::Q)
}

fn moves(m: &ModState, b: BlockRef, q: chimera_core::block::ParamId) -> bool {
    m.moves_addr(ParamAddr::new(b, q))
}

/// The filter as read: its kind over CLEAN, then FB's moving path, or a
/// route's, STABILITY and CLEAN's (or DIRTY's) clipper.
fn filter_model(p: &ParamSnapshot, m: &ModState) -> u32 {
    let f = &p.filter;
    let k = f.kind();
    let still = match k {
        FilterKind::Svf | FilterKind::Dirty => 0,
        FilterKind::K2040 if f.voice == K2040Voice::Rev2 => K2040_REV2,
        FilterKind::K2040 => K2040_EXP,
        FilterKind::Ladder => LADDER,
        FilterKind::Sem => SEM,
    };
    let fb = f.fb != 0.0 || moves(m, BlockRef::Filter, FilterParams::FB);
    let path = if fb {
        self::fb(k)
    } else if filter_moves(m) || shaped(p) {
        moving(k)
    } else {
        0
    };
    let stab =
        k.has_chip() && (f.stability < 1.0 || moves(m, BlockRef::Filter, FilterParams::STAB));
    let open = f.level > 0.5 || moves(m, BlockRef::Filter, FilterParams::LEVEL);
    // DIRTY has no driven row: its bill.
    let level = match k {
        FilterKind::Svf if open => LEVEL,
        FilterKind::Dirty if open => k.level_cost().0,
        _ => 0,
    };
    still + path + if stab { STAB } else { 0 } + level
}

/// The pool as read: the VCA's sources, every destination but the VCA,
/// slides and FUNC LFOs.
fn routes_model(p: &ParamSnapshot, m: &ModState) -> u32 {
    let bits = m.routes_into(VCA);
    let mut vca = 0;
    let mut first = true;
    for s in ModSource::ALL
        .iter()
        .filter(|s| bits & (1 << s.index()) != 0)
    {
        let term = match s.env_slot() {
            None => VCA_OTHER,
            Some(slot) => {
                let e = &p.envelopes[slot.index()];
                match e.env_type {
                    EnvType::A => VCA_ENV_A,
                    EnvType::B if e.func.mode == FuncMode::Burst => VCA_BURST,
                    EnvType::B => VCA_ENV_B,
                }
            }
        };
        // The clamp once: each further source its own term less it.
        vca += if first {
            term
        } else {
            term - VCA_OTHER + VCA_MORE
        };
        first = false;
    }
    let fixed = [FixedCol::Freq, FixedCol::Q, FixedCol::Fx]
        .into_iter()
        .filter(|&c| m.fixed_moves(c))
        .count() as u32;
    let reg = (0..m.num_dests()).filter(|&d| m.moves(d)).count() as u32;
    let dests = fixed + reg;
    let dest = if dests == 0 {
        0
    } else {
        DEST_FIRST + (dests - 1) * DEST
    };
    let slides = EnvSlot::ALL
        .iter()
        .filter(|&&slot| {
            [
                EnvParams::TIME,
                EnvParams::RISE,
                EnvParams::FALL,
                EnvParams::SHAPE,
            ]
            .iter()
            .any(|&q| m.routes_into(ParamAddr::new(BlockRef::Env(slot), q)) != 0)
        })
        .count() as u32;
    let func = p
        .lfos
        .iter()
        .filter(|l| l.lfo_type == LfoType::Func)
        .count() as u32;
    vca + dest + slides * SLIDE + func * FUNC
}

/// The FX slot, ALGO's taps and AMP's gate as read.
fn rest_model(p: &ParamSnapshot, m: &ModState) -> u32 {
    let algo = p.engine() == EngineType::Algo;
    let tapped = algo && p.fx.ty() == FxType::Fold && p.taps.moving(FixedCol::Fx);
    let fx = if !Voice::fx_runs(p, m) {
        0
    } else if tapped {
        FOLD_TAP
    } else {
        use chimera_core::dsp::voice_fx::params::VoiceFxParams as X;
        let ramps = [X::P1, X::P2, X::P3, X::MIX]
            .iter()
            .any(|&q| moves(m, BlockRef::VoiceFx, q));
        match p.fx.ty() {
            FxType::Off => 0,
            FxType::Soft => SOFT + if ramps { SOFT_RAMP } else { 0 },
            FxType::Fold => FOLD + if ramps { FOLD_RAMP } else { 0 },
            FxType::Fuzz => FUZZ + if ramps { FUZZ_RAMP } else { 0 },
        }
    };
    let taps = if !algo {
        0
    } else if p.taps.moving(FixedCol::Freq) || p.taps.moving(FixedCol::Q) {
        TAP_FREQ
    } else if p.taps.moving(FixedCol::Vca) {
        TAP_VCA
    } else {
        0
    };
    let gate = p.out.gate.iter().any(|&g| g > 0.0) || moves(m, BlockRef::Out, OutParams::LPG);
    fx + taps + if gate { GATE } else { 0 }
}

/// What the unit read a voice of `s` costs, term by term. MODAL bills its
/// engine at or over its rows already (ADR 0218): its engine is the bill.
fn model(s: &Sound) -> u32 {
    let (p, m) = (&s.params, &s.mod_state);
    let core = match p.engine() {
        EngineType::Algo => {
            let (tables, frmt, nois) = algo_shape(&p.algo, &m.algo_levels_routed());
            let over = f64::from(tables) - f64::from(ONE_OP_TABLES);
            let scaled =
                over * ALGO_SCALE + f64::from(frmt) * FRMT_SCALE + f64::from(nois) * NOIS_SCALE;
            BARE + scaled.ceil() as u32
        }
        EngineType::Va => {
            let nz = p.va.noise > 0.0 || moves(m, BlockRef::Va, VaParams::NOISE);
            let wave = [VaParams::WAVE1, VaParams::WAVE2]
                .iter()
                .any(|&q| moves(m, BlockRef::Va, q));
            va_core(p.va.mode)
                + if nz { noise(p.va.noise_kind) } else { 0 }
                + if wave { VA_WAVE } else { 0 }
        }
        EngineType::Modal => {
            EngineSlot::cost(p, m).0 + Voice::CHAIN_COST.0 + ModRouting::BASE.0
                - modal_slack(p.modal.mode)
        }
        EngineType::Tape => EngineSlot::cost(p, m).0 + Voice::CHAIN_COST.0 + ModRouting::BASE.0,
    };
    core + filter_model(p, m) + routes_model(p, m) + rest_model(p, m)
}

/// Voices of `bill` a sample on rev V beside the bus and the control
/// reserve (QUAD, no ORBIT): the pool caps it.
fn voices(bill: u32) -> (u32, u32) {
    let room = SampleBudget::for_cpu(CPU_HZ_REV_V).as_cost().0
        - FxBus::COST.0
        - chimera_core::budget::CONTROL_RESERVE.0;
    let n = room / bill;
    (n.min(MAX_VOICES as u32), n)
}

fn factory() -> Vec<(String, Sound)> {
    let dir = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/factory-sounds"
    ));
    build::sounds(dir, None)
        .unwrap()
        .iter()
        .map(|s| (s.name.as_str().to_string(), source::to_sound(s).unwrap()))
        .collect()
}

/// Sounds the model reads over their bill, each with the margin it may
/// not fall under: VOWEL BUZZ and THROAT SING (a 2040 tapped and routed)
/// and MORPH BASS, 1 or 2 under, left as the owner chose (ADR 0262). A bill
/// cut that widens one fails here; closing one moves it out.
const KNOWN_UNDER: &[(&str, i64)] = &[("VOWEL BUZZ", -2), ("THROAT SING", -2), ("MORPH BASS", -1)];

#[test]
fn every_factory_sound_is_billed_at_or_over_its_reading() {
    let mut under = Vec::new();
    for (name, s) in factory() {
        let bill = Voice::cost(&s.params, &s.mod_state).0;
        let read = model(&s);
        let margin = i64::from(bill) - i64::from(read);
        let (v, room) = voices(bill);
        println!(
            "BILL\t{name}\t{:?}\tbill {bill}\tmodel {read}\tmargin {margin}\tvoices {v} ({room} by room)",
            s.params.engine(),
        );
        let floor = KNOWN_UNDER.iter().find(|k| k.0 == name).map_or(0, |k| k.1);
        if margin < floor {
            under.push(format!(
                "{name}: bill {bill}, model {read}, margin {margin} under {floor}"
            ));
        }
    }
    assert!(
        under.is_empty(),
        "billed under the unit's reading:\n{}",
        under.join("\n")
    );
}

/// A pinned gap is a factory Sound still under its model: one that has
/// closed (margin 0 or more) comes off the list.
#[test]
fn every_known_gap_is_still_open() {
    let all = factory();
    for (k, floor) in KNOWN_UNDER {
        assert!(*floor < 0, "{k}: pinned at {floor}");
        let (_, s) = all
            .iter()
            .find(|(n, _)| n == k)
            .unwrap_or_else(|| panic!("{k}: no such Sound"));
        let margin = i64::from(Voice::cost(&s.params, &s.mod_state).0) - i64::from(model(s));
        assert!(
            margin < 0,
            "{k}: margin {margin}, closed: take it off KNOWN_UNDER"
        );
    }
}

/// NOIS's term bills under its reading, known (ADR 0262): its charge over a
/// table operator reads × 1.85 (NOIS's costliest added voice), billed at
/// 95/125 so CRACK SNARE keeps six voices. Unlinked 2, linked 9 short a
/// NOIS operator; every factory Sound still bills over its model. Raising
/// the bill to the reading takes this off.
#[test]
fn nois_term_shortfall_is_known() {
    use chimera_core::dsp::algo::engine::AlgoEngine;
    let short = |linked| {
        let read = (f64::from(nois_charge(linked)) * NOIS_SCALE).ceil() as u32;
        read - (AlgoEngine::nois_cost(linked) - AlgoEngine::COST_OP.0)
    };
    assert_eq!((short(false), short(true)), (2, 9));
}
