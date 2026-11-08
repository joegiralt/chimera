//! Every factory Sound's bill is an upper bound of what the unit read it
//! costs (#448, ADR 0249): `Voice::cost` at or over a model built only from
//! the 0.3 bench readings (`.superpowers/ledgers/bench-0.3/`, rev V at
//! 480 MHz, 2026-10-26, main 4eae5d81), term by term, each the worst row
//! that holds it. Run with `--nocapture` for the census: bill, model and
//! voices on rev V per Sound.

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::engine::AlgoEngine;
use chimera_core::dsp::algo::kinds::{Kinds, OpKind};
use chimera_core::dsp::algo::params::{AlgoOpParams, AlgoParams};
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::filter::k2040::K2040Voice;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::modulator::{EnvSlot, EnvType, FuncMode, LfoType};
use chimera_core::dsp::va::VaMode;
use chimera_core::dsp::va::noise::NoiseKind;
use chimera_core::dsp::voice::Voice;
use chimera_core::dsp::voice_fx::params::FxType;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::modulation::{FixedCol, ModRouting, ModSource, ModState, VCA};
use chimera_core::params::{EngineType, EnvParams, ParamSnapshot};
use chimera_core::params::{FilterParams, OutParams};
use chimera_core::preset::Sound;
use chimera_desktop::factory_sounds::{build, source};
use std::path::Path;

// Bench-0.3 rows, cycles a sample a voice: each row's warm 64-block
// average, a voice's mean (`/VOICE`) or, where a voice climbs with its
// note (VA, ALGO's FRMT rows), its top voice. NOT a worst block: the
// guard bounds what these averages read, and a block over them comes out
// of the 25 % the UI keeps (ADR 0124) until the worst-block rows are read
// on the unit (ADR 0249, "Needs the unit"). The ROUTING rows built with `matrix` carry no project
// routes; the others carry the project's ENV 1 → VCA, ENV 1 → CUTOFF and
// one more destination, so only their differences are used.

/// 1 DEST (1 OP, CLEAN LP24, LFO 1 → MORPH) less one destination: the bare
/// one-operator voice, chain, pool and CLEAN in it.
const BARE: u32 = 489 - DEST_FIRST;
/// The first destination's sum and offset: no 0.3 row has it alone (VEL
/// 1R − A VCA, 17, is VEL into VOLUME, an output's ramp); the t13 runs'
/// 1 CUTOFF − 1 OP, 6 (ModRouting::DEST_FIRST's note).
const DEST_FIRST: u32 = 6;
/// (VEL 4R − VEL 1R) / 3, up: each further destination, outputs' ramps
/// and all.
const DEST: u32 = 4;
/// VEL VCA − BARE: the VCA's clamp with one plain source.
const VCA_OTHER: u32 = 508 - BARE;
/// 2 VCA − VEL VCA: each further plain source.
const VCA_MORE: u32 = 517 - 508;
/// A VCA − BARE: an ENV of type A on the VCA, the clamp in it.
const VCA_ENV_A: u32 = 528 - BARE;
/// B GLIDE − BARE: type B on the VCA, its costliest steady row.
const VCA_ENV_B: u32 = 614 - BARE;
/// BURST AD − BARE: type B in BURST.
const VCA_BURST: u32 = 680 - BARE;
/// A SLIDE − A VCA: a slot's TIME/RISE/FALL/SHAPE routes, their four
/// destinations in it.
const SLIDE: u32 = 561 - 528;
/// FUNC LFO − 1 DEST.
const FUNC: u32 = 494 - 489;
/// ALGO's table operators and links over 1 OP, scaled from their bill:
/// the worst table row's read over bill, each row at the higher of its
/// mean and its top voice, against 1 OP's lower (top 564): WC (980 − 564)
/// / (1017 − 647) = 1.124.
const ALGO_SCALE: f64 = 1.13;
/// A formant or noise operator's charge, the same way: FRMT PM's top
/// (1061 − 564) / (1113 − 647) = 1.067, the worst FRMT row; NOIS (590 −
/// 564) / (667 − 647) = 1.3.
const FRMT_SCALE: f64 = 1.07;
const NOIS_SCALE: f64 = 1.3;
/// The filter kinds over CLEAN, as the project's rows read them (each
/// under ENV 1 → CUTOFF, so at least still): 2040 R2 − SVF, 2040 − SVF
/// (EXP), LADDER − SVF, SEM − SVF; DIRTY and NOTCH2 read under CLEAN.
const K2040_REV2: u32 = 632 - 558;
const K2040_EXP: u32 = 682 - 558;
const LADDER: u32 = 735 - 558;
const SEM_UNDER_ENV: u32 = 596 - 558;
/// SEM still: the 0.2 bench's, 9 over CLEAN LP4 (2026-10-22, `SEM_COST`'s
/// note); under ENV 1 it reads `SEM_UNDER_ENV`, its moving share.
const SEM: u32 = 9;
/// CLEAN MOVE − SVF: CUTOFF jumping every block, over the ENV's move: a
/// moving filter's worst, charged on every kind (no other kind has a sweep
/// row; bench rows to add).
const MOVE: u32 = 600 - 558;
/// FB at ½ over each kind's row: the moving path every block and FB.
const fn fb(k: FilterKind) -> u32 {
    match k {
        FilterKind::Svf => 753 - 558,
        FilterKind::Dirty => 647 - 532,
        FilterKind::K2040 => 874 - 682,
        FilterKind::Ladder => 909 - 735,
        FilterKind::Sem => 727 - 596,
    }
}
/// STABILITY ½ over the chip's row, the costliest (SEM STAB − SEM).
const STAB: u32 = 5;
/// LP24 HOT − SVF: CLEAN's LEVEL clipper driven at LEVEL 1, RES 1.
const LEVEL: u32 = 655 - 558;
/// TAP FREQ − 1 OP and TAP VCA − 1 OP.
const TAP_FREQ: u32 = 736 - 558;
const TAP_VCA: u32 = 602 - 558;
/// The FX slot over 1 OP: FOLD, SOFT LO (SOFT's worst), FUZZ; FOLD TAP.
const FOLD: u32 = 635 - 558;
const SOFT: u32 = 648 - 558;
const FUZZ: u32 = 794 - 558;
const FOLD_TAP: u32 = 741 - 558;
/// A routed slot's ramp: SOFT RT, FOLD RT, FUZZ MOVE less 1 DEST and the
/// type at that P1 (SOFT, FOLD HALF, FUZZ).
const SOFT_RAMP: u32 = 592 - 489 - (636 - 558);
const FOLD_RAMP: u32 = 588 - 489 - FOLD;
const FUZZ_RAMP: u32 = 768 - 489 - FUZZ;
/// AMP's gate: LPG RT − A VCA (above LPG − 1 OP's 82).
const GATE: u32 = 630 - 528;
/// VA NOISE over VA, by KIND.
const fn noise(k: NoiseKind) -> u32 {
    match k {
        NoiseKind::White => 711 - 695,
        NoiseKind::Pink => 732 - 695,
        NoiseKind::Red => 728 - 695,
        NoiseKind::Digital => 725 - 695,
    }
}
/// VA's MODE rows at their top voice, less the project routes' 59 (ADR
/// 0218) and the 2040 REV2 they play: the voice to CLEAN's equal.
const fn va_core(m: VaMode) -> u32 {
    let top = match m {
        VaMode::Mix => 744,
        VaMode::Sync => 769,
        VaMode::Ring => 755,
        VaMode::Xor => 767,
        VaMode::Bits => 760,
        VaMode::Fm => 800,
        VaMode::Am => 766,
        VaMode::Max => 769,
        VaMode::LoopFm => 701,
        VaMode::SoftSync => 829,
        VaMode::Phase => 809,
    };
    top - 59 - K2040_REV2
}

/// The bench's 1 OP engine (A1, OP1 a table at LEVEL 99): what `BARE`'s
/// engine bills.
fn one_op_bill() -> u32 {
    let mut ops = [AlgoOpParams::DEFAULT; OPS];
    ops[0].level = 99;
    for o in &mut ops[1..] {
        o.level = 0;
    }
    let p = AlgoParams::new(AlgoId::A1, AlgoId::A1, 0, Kinds::TABLE, ops);
    AlgoEngine::cost(&p, &[false; OPS]).0
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
    } else if filter_moves(m) {
        // DIRTY's one loop reads its arrays every block (its row under
        // ENV 1 reads under CLEAN's): no moving share.
        match k {
            FilterKind::Dirty => 0,
            FilterKind::Sem => MOVE + SEM_UNDER_ENV - SEM,
            _ => MOVE,
        }
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
            let routed = m.algo_levels_routed();
            let linked = p.algo.linked();
            let (mut frmt, mut nois) = (0, 0);
            for i in (0..OPS).filter(|&i| p.algo.ops[i].level > 0 || routed[i]) {
                match p.algo.kinds().get(i) {
                    OpKind::Formant => {
                        frmt += AlgoEngine::frmt_cost(p.algo.ops[i].skirt, linked.has(i))
                    }
                    OpKind::Noise => nois += AlgoEngine::nois_cost(linked.has(i)),
                    OpKind::Table(_) => {}
                }
            }
            let tables = AlgoEngine::cost(&p.algo, &routed).0 - frmt - nois;
            let over = tables.saturating_sub(one_op_bill());
            let scaled = f64::from(over) * ALGO_SCALE
                + f64::from(frmt) * FRMT_SCALE
                + f64::from(nois) * NOIS_SCALE;
            BARE + scaled.ceil() as u32
        }
        EngineType::Va => {
            let nz =
                p.va.noise > 0.0 || moves(m, BlockRef::Va, chimera_core::dsp::va::VaParams::NOISE);
            va_core(p.va.mode) + if nz { noise(p.va.noise_kind) } else { 0 }
        }
        EngineType::Modal | EngineType::Tape => {
            EngineSlot::cost(p, m).0 + Voice::CHAIN_COST.0 + ModRouting::BASE.0
        }
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

/// Sounds the model reads over their bill before #448's re-bill, each
/// with the margin it may not fall under: the formant and six-operator
/// ALGO Sounds (ADR 0211 point 3, ADR 0218 point 4: ALGO's terms held for
/// the formant floor), CLEAN driven over ½ (LP24 HOT, no bill for it yet)
/// and the rest within the bench's spread (≤ 12). A bill cut that widens
/// one fails here; closing one moves it out. ADR 0249.
const KNOWN_UNDER: &[(&str, i64)] = &[
    ("VOWEL BUZZ", -61),
    ("THROAT SING", -61),
    ("GLASS MALLET", -5),
    ("VOCAL KEYS", -53),
    ("SINGING BASS", -38),
    ("WAH BRASS", -49),
    ("TALKING BELL", -36),
    ("HUMMING PAD", -9),
    ("YELLING LEAD", -40),
    ("NASAL MONO", -6),
    ("DEAD CHOIR", -11),
    ("CRACK SNARE", -4),
    ("BROKEN WIRE", -6),
    ("NULL CHOIR", -40),
    ("RIOT STAB", -4),
    ("ASH CHORD", -12),
    ("SPARK GAP", -1),
    ("NYLON", -3),
    ("GAMELAN", -6),
    ("GLITCH BARS", -12),
    ("SOLID BASS", -32),
    ("MORPH BASS", -2),
    ("TINE PIANO", -8),
    ("FUNK CLAV", -32),
    ("DRIFT ORGAN", -3),
    ("ICE BELLS", -2),
    ("MUSIC BOX", -2),
    ("SOFT HORNS", -39),
    ("WARM PAD", -59),
    ("GLASS PAD", -55),
];

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
