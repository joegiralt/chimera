// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Refactor lock (spec 2026-09-23 § Testing).
//!
//! Goldens freeze today's output, good or bad — they are not a quality claim.
//! They must match bit-for-bit after every refactor step. Re-record ONLY for
//! a change the spec lists as intended, in the task that makes it
//! (`common::golden`).

mod common;

use common::*;

use chimera_core::dsp::tape_engine::TapeEngineParams;
use chimera_core::dsp::tape_engine::TapeMachine;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::{MidiNote, Velocity};

/// (case name, FNV-1a 64 over every sample's bits, sample bits at SPOT_IDX).
const GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    // Every row re-recorded at Task 17 for ADR 0060: the voice's DC blocker
    // (and on Modal the blocker on BANK and SYMP's zero-mean pluck); with
    // those four off, every row matched its value before. Each row's comment
    // names what came before that. The Algo rows, SQR BASS and the Algo
    // half of `algo_to_modal_switch` re-recorded for the filter's C1
    // `saturate` (ADR 0063): each moved only where a state passed 1. Then
    // every row for ADR 0063's INIT (OUT LEVEL 45/128, RR 5) and the
    // harness's 1.6 s release (`OFF_BLOCKS`).
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "modal_init",
        (
            0x21882077049fce24,
            [
                0, 1045961661, 1050776390, 3182676167, 1048548409, 1048597135, 1040450511,
                3182822204,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    // Then CLEAN's k law and trims (ADR 0096).
    // Then LP4's open band's place by its reciprocal width (#1135's
    // re-review): the LFO sweeps CUTOFF through the band, an ulp moved.
    (
        "modal_lfo_cutoff",
        (
            0xb5ea89077536c3f8,
            [
                0, 1045961661, 1053105301, 3182675859, 1048548423, 1048597142, 1040497259,
                996145808,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062), then SYMP's out clip, stmlib's
    // rational curve for `tanhf` (ADR 0097).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "modal_sympathetic",
        (
            0x05dcf0e452e319be,
            [
                1027343775, 1026070151, 1019406411, 3198608516, 1054405622, 1053163647, 1051656223,
                3192623309,
            ],
        ),
    ),
    // Re-recorded, every Algo case: the output scale is the carrier power
    // (ADR 0049); INIT routes four operators. Factory Sounds compensate in
    // their data and match 937b89f to about 1e-7, but MORPH PAD (#192).
    // Recorded after the Algo sanity gate (ADR 0011).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_init",
        (
            0xde7f09f18b9ae449,
            [
                986521787, 3198534240, 1049049206, 1046340872, 3197429473, 3197919749, 3170445903,
                3156441109,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    // Then LP4's open band's place by its reciprocal width (#1135's
    // re-review): the LFO sweeps CUTOFF through the band, an ulp moved.
    (
        "algo_lfo_cutoff",
        (
            0x948953155782e9a5,
            [
                986521787, 3198534240, 1049134767, 1046346072, 3197429057, 3197919334, 3169520018,
                1023773430,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t1",
        (
            0xbb7c15efd1e422ee,
            [
                987489574, 3199169892, 1046386099, 3168227646, 3172456842, 1018200283, 3185812601,
                3162397654,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t2",
        (
            0x39bfcd36b6ae2259,
            [
                988186865, 1049978955, 3180158642, 3189624674, 3192895230, 3187535060, 3185826879,
                3163046055,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t3",
        (
            0xdf314f1543984691,
            [
                990412126, 1051396597, 3181947276, 3187850413, 3194071704, 3189254560, 3185621407,
                3164575486,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t4",
        (
            0xf9043447bfc3aa00,
            [
                987586563, 3197776821, 3197906035, 3175067810, 1045899565, 1042141265, 3184313183,
                3161277601,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t5",
        (
            0xeda47b610e665299,
            [
                987249984, 1051978301, 1051955637, 3182354278, 3197069331, 3196752199, 3186425203,
                3162731351,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t6",
        (
            0x862a31403df42379,
            [
                988474769, 1049862642, 1051569243, 3180043529, 3196975302, 3196480146, 3181013584,
                3159593251,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t7",
        (
            0x2ed534cfa81c4592,
            [
                984772746, 1036821186, 1051903330, 3166207050, 3197336717, 3197168999, 3182807467,
                3155779113,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t8",
        (
            0x901cb8e5c074d4e3,
            [
                986120434, 1030764416, 1052285029, 3163999886, 3197637764, 3197544478, 3183558836,
                3154291588,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_morph_static",
        (
            0xf1ebb3a28a4cf680,
            [
                995613419, 3179585298, 1017664761, 3179493066, 3184434619, 3171272739, 3172124857,
                3148214762,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_morph_sweep",
        (
            0xdf7768c1598fc062,
            [
                995613419, 3182396664, 1024619650, 3178513042, 1003268444, 1023649644, 3172072988,
                992417869,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_to_modal_switch",
        (
            0x71ab9ad1de8c2b6b,
            [
                986521787, 3198534240, 1049049206, 3202107720, 1020081277, 1033099465, 3167240166,
                3195401095,
            ],
        ),
    ),
    // Filter-routing spec § Migration: recorded before the change; re-recorded
    // only for ADR 0060's DC blocker.
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_0",
        (
            0x1cbfa2a144a266a5,
            [
                981892212, 3199783104, 3194099492, 1045438985, 3188479405, 3188247244, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_1",
        (
            0x1bd3d1c2f05c4f8f,
            [
                993738130, 1031819968, 1055660809, 3171517507, 3202002190, 3201998038, 3116897624,
                0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_2",
        (
            0x795fd276f0cc224b,
            [
                934404059, 1042323911, 1015746383, 3166129769, 1002622737, 1015885521, 3158658333,
                3102249783,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_3",
        (
            0x8aafe4b466d74229,
            [
                1001389678, 1055558080, 3184812150, 1057343676, 1005963747, 3173729227, 3192776992,
                1020446637,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_4",
        (
            0x62fbdd207ea6ef12,
            [
                908219966, 1055265999, 1049507872, 1030503929, 3191651237, 3191270130, 3113279447,
                0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    // Then the voice-end fade (ADR 0096): its filter tail, −65 dB, fades
    // over the last block instead of cutting.
    // Re-recorded in Task 14: DRV is SOFT, voiced at LEVEL 0.7626 to cancel
    // its `MAKE_UP` 0.483 (the factory Sounds keep their loudness).
    (
        "factory_5",
        (
            0xfa8a040324329443,
            [
                898827728, 3209682936, 3209857559, 1059996833, 3206496509, 3206489051, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_6",
        (
            0x8e3348ab90b89d95,
            [
                834352396, 1009650854, 1036674498, 968914263, 3182119491, 3181374484, 3177329682,
                993036649,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    // Then the voice-end fade past −80 dB (#487): its DC blocker's tail,
    // −69 dB, ramps out over the last block instead of cutting.
    (
        "factory_7",
        (
            0x54f583a5c0edc2d4,
            [
                989953630, 1046926614, 1045583927, 1026758132, 1033068704, 1032572665, 3133179670,
                0,
            ],
        ),
    ),
    // The formant demo Sounds (spec § Demo clips): CHOIR, TALK BASS, BREATH
    // and VOX.
    (
        "factory_8",
        (
            0x11fa1b44c25f7db1,
            [
                809965482, 3130400255, 1016198788, 3141165751, 3193279422, 3194216559, 1011502076,
                992344687,
            ],
        ),
    ),
    // Re-recorded in Task 14: DRV is SOFT, voiced at LEVEL 0.7626 to cancel
    // its `MAKE_UP` 0.483 (the factory Sounds keep their loudness).
    (
        "factory_9",
        (
            0x1c98f9f505030d19,
            [
                886068374, 3196599706, 995173522, 3194527354, 3191504998, 3179667062, 3126567846, 0,
            ],
        ),
    ),
    (
        "factory_10",
        (
            0x3791cfa980449ea0,
            [
                837617169, 1017595293, 3179822902, 1032463702, 1044536700, 1044567218, 999749924,
                959177985,
            ],
        ),
    ),
    // Re-recorded in Task 14: DRV is SOFT, voiced at LEVEL 0.7626 to cancel
    // its `MAKE_UP` 0.483 (the factory Sounds keep their loudness). Again
    // for #164 (ADR 0316): its ENV 3 → PITCH swoop ramps across the block.
    (
        "factory_11",
        (
            0x8b46709cbde15660,
            [
                944197035, 1056300261, 1053647021, 1031346519, 3173889874, 3173433364, 0, 0,
            ],
        ),
    ),
    // Recorded at VA Task 10: INIT at VA_OUT_GAIN 3.631 (ADR 0109).
    (
        "va_init",
        (
            0xc22f6c59684767d8,
            [
                0, 1035274050, 1057410685, 3194645188, 3211538507, 3211140287, 0, 0,
            ],
        ),
    ),
];

/// TAPE through one voice on INIT (ENV 1 into VCA), each model and LOOP
/// with AGE and TAPE at 0.5 (`tape_render`). Recorded at TAPE Task 12; the
/// machines' rows appended at Task 12b, the rest unchanged; re-recorded for
/// `out_gain` (Task 19), then the smoothstep note-on ramp and LOOP's
/// smoothstep fades (#385), then CLEAN's ACCURATE tan, exact divides, k law
/// and trims (#57, ADR 0096) on main's merge; then #698's round: INIT's
/// LENGTH 0.8, one release a note, equal-power fades by correlation, AGE
/// from the source's band and TRON's LENGTH over the strip (ADRs
/// 0179–0182); then the TRON rows for WALK (ADR 0203).
const TAPE_GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    (
        "tape_loop",
        (
            0x5c7491d2727b64d7,
            [
                0, 3193912025, 3166947230, 1051426063, 3188120255, 3187988644, 3202021239,
                1019591020,
            ],
        ),
    ),
    (
        "tape_reverse",
        (
            0xab570647fe08031a,
            [
                0, 1045741244, 1016337029, 3199078996, 1040102523, 1039847087, 1054320072,
                3189145435,
            ],
        ),
    ),
    (
        "tape_pingpong",
        (
            0xa1e3f7a62b6b9492,
            [
                0, 3193912025, 3166947230, 1051426063, 3188120255, 3187988644, 3202021239,
                1019591020,
            ],
        ),
    ),
    (
        "tape_transwave",
        (
            0xd81172ab6beb47c7,
            [
                0, 3201632400, 3192134221, 1050906781, 3203906981, 3204101774, 1046214700,
                3199060036,
            ],
        ),
    ),
    (
        "tape_scrub",
        // ADR 0205: SCRUB's LENGTH is a time (2 ms · 1000^LENGTH) and its
        // key places the finger at ROOT.
        (
            0xe3bc85f800c25e87,
            [
                0, 1049830829, 1046951855, 1006535864, 3195970706, 3195808503, 1048268177,
                3181616822,
            ],
        ),
    ),
    (
        "tape_stretch",
        (
            0x115dde9030c1bedf,
            [
                0, 3193224958, 3162721296, 3185903403, 1036343105, 1036592403, 1026670330,
                3162725960,
            ],
        ),
    ),
    (
        "tape_loop_age",
        (
            0x2ab9e2bd96509e6c,
            [
                3085829851, 3194016615, 3165711770, 1051361184, 3188363480, 3187888216, 3191874181,
                1034694557,
            ],
        ),
    ),
    (
        "tape_loop_tape",
        (
            0x0b8fca16b7c05fff,
            [
                0, 3193792447, 3158208976, 1052754345, 3177315703, 3176819996, 3196359685,
                1046051029,
            ],
        ),
    ),
    (
        "tape_oneshot",
        (
            0xd55919c7073c840e,
            [
                0, 3193912025, 3166947230, 1051426063, 3188120255, 3187988644, 3202021239,
                1019591020,
            ],
        ),
    ),
    (
        "tape_oneshot_slice7",
        (
            0xd55919c7073c840e,
            [
                0, 3193912025, 3166947230, 1051426063, 3188120255, 3187988644, 3202021239,
                1019591020,
            ],
        ),
    ),
    // Re-recorded for #1126: the block its 1.41 s tape runs out in eases
    // into the run-out (`run_out::onset`); the engine plays every
    // other sample as before, and the voice's filter carries the block's
    // change under −80 dB until the note ends. Re-recorded again under
    // the open LP4 (#695).
    (
        "tape_tron_held",
        (
            0x79bd93807d679bd8,
            [
                0, 3193912025, 3166947230, 1051426063, 3188120255, 3187988644, 3202021239,
                1019591020,
            ],
        ),
    ),
    (
        "tape_tron_released",
        (
            0x9b9d49e63d0774a8,
            [
                0, 3193912025, 3166947230, 1051426063, 3188120255, 3187988644, 3202021239,
                1019591020,
            ],
        ),
    ),
    (
        "tape_stretch_reverse",
        (
            0x1259cc07132d89de,
            [
                0, 1045055057, 1008748397, 1037312167, 3184747599, 3185008982, 3176170532,
                1006779331,
            ],
        ),
    ),
    (
        "tape_stretch_grain",
        (
            0xecbe86d11e862daf,
            [
                0, 3193945450, 3176512711, 1050196705, 3193713522, 3193580211, 1050269903,
                3168085015,
            ],
        ),
    ),
    (
        "tape_stretch_grain_reverse",
        (
            0xc1bf635320393e30,
            [
                0, 1045771557, 1019119228, 3199076693, 1041200577, 1041072150, 3200304298,
                3186538561,
            ],
        ),
    ),
    (
        "tape_loop_warble",
        (
            0x54eb7b6236a11a4d,
            [
                0, 3193905689, 3166118364, 1051556014, 3188366750, 3188232406, 1051766794,
                3196696516,
            ],
        ),
    ),
];

/// Goldens whose locked output no longer reflects intended behaviour, each
/// with its issue. Empty: Modal 2 step A closed #10, and Modal's four
/// goldens were re-recorded then; `known_broken` guards whatever comes next.
const KNOWN_BROKEN: &[(&str, &str)] = &[];

#[test]
fn goldens_match() {
    let got: Vec<_> = Case::ALL
        .iter()
        .map(|&case| {
            let out = render_case(case);
            (case.name(), (fnv1a(&out), spots(&out)))
        })
        .collect();
    golden::check(GOLDENS, &got);
}

/// Spec § Testing: part 1's mono bus through the voice pool matches every
/// golden bit-for-bit.
#[test]
fn goldens_match_through_the_instrument() {
    let got: Vec<_> = Case::ALL
        .iter()
        .map(|&case| {
            let out = render_case_through_instrument(case);
            (case.name(), (fnv1a(&out), spots(&out)))
        })
        .collect();
    // Compares even under GOLDEN_RECORD; `goldens_match` alone records.
    let failures = golden::mismatches(GOLDENS, &got);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn known_broken_goldens_have_issues() {
    let cases: Vec<&str> = Case::ALL.iter().map(|c| c.name()).collect();
    golden::known_broken(KNOWN_BROKEN, &cases);
}

#[test]
fn harness_is_deterministic() {
    for case in [Case::AlgoInit, Case::ModalInit] {
        assert_eq!(
            fnv1a(&render_case(case)),
            fnv1a(&render_case(case)),
            "{}",
            case.name()
        );
    }
}

/// A route that silently does nothing would make its golden a copy of the
/// unmodulated one and lock nothing.
#[test]
fn modulated_cases_differ_from_unmodulated() {
    for (modulated, plain) in [
        (Case::ModalLfoCutoff, Case::ModalInit),
        (Case::AlgoLfoCutoff, Case::AlgoInit),
        (Case::AlgoMorphSweep, Case::AlgoMorphStatic),
    ] {
        assert_ne!(
            fnv1a(&render_case(modulated)),
            fnv1a(&render_case(plain)),
            "{} renders the same as {}",
            modulated.name(),
            plain.name()
        );
    }
}

/// Spec § Testing: a patch per T1–T8, each a different algorithm.
#[test]
fn the_eight_tx_algorithms_render_differently() {
    let hashes: Vec<u64> = (0..8)
        .map(|t| fnv1a(&render_case(Case::AlgoTx(t))))
        .collect();
    for (i, h) in hashes.iter().enumerate() {
        assert!(
            !hashes[..i].contains(h),
            "T{} renders like an earlier T",
            i + 1
        );
    }
}

/// Filter-routing spec § Migration: every factory Sound is locked.
#[test]
fn every_factory_sound_has_a_golden() {
    for i in 0..chimera_core::factory::FACTORY_LEN {
        let name = format!("factory_{i}");
        assert!(GOLDENS.iter().any(|g| g.0 == name), "{name}");
    }
}

/// The shared check fails a changed value, a row left behind by a renamed
/// or deleted case, and a case with no row (#108).
#[test]
fn golden_check_catches_every_disagreement() {
    let table = [("a", 1u64), ("b", 2), ("d", 4), ("a", 1)];
    let got = [("a", 1u64), ("c", 3), ("d", 5)];
    assert_eq!(
        golden::mismatches(&table, &got),
        [
            "c: no golden recorded (run with GOLDEN_RECORD=1)",
            "d: 0x0000000000000005 (want 0x0000000000000004)",
            "b: recorded, but no such case",
            "a: recorded twice",
        ]
    );
    assert!(golden::mismatches(&table[..3], &table[..3]).is_empty());
}

/// The TAPE cases: each model, then LOOP with AGE, then with TAPE, at 0.5;
/// then the machines (TAPE Task 12b). `tape_reverse` is Task 12's REVERSE,
/// now LOOP with END before START: its row is unchanged.
fn tape_cases() -> Vec<(&'static str, TapeEngineParams)> {
    use chimera_core::dsp::tape_engine::{Direction, FADE_HOLD, TapeModel as M};
    let p = TapeEngineParams::default();
    let model = |model| TapeEngineParams {
        machine: TapeMachine::of(model),
        ..p
    };
    // The old machines, played as PLAY in each MODE (ADR 0170): ONE at
    // HOLD is ONESHOT.
    let ping = TapeEngineParams {
        machine: TapeMachine::Walk,
        direction: Direction::Ping,
        ..p
    };
    let one = TapeEngineParams {
        machine: TapeMachine::Walk,
        direction: Direction::Forward,
        drag: FADE_HOLD,
        ..p
    };
    let rev = |model| TapeEngineParams {
        machine: TapeMachine::of(model),
        start: 1.0,
        end: 0.0,
        ..p
    };
    vec![
        ("tape_loop", model(M::Walk)),
        ("tape_reverse", rev(M::Walk)),
        ("tape_pingpong", ping),
        ("tape_transwave", model(M::Transwave)),
        // A moving tape: the neutral's MOTION 0 is a stopped, silent one;
        // ½ is 1×, which plays as LOOP does (#706).
        (
            "tape_scrub",
            TapeEngineParams {
                motion: 0.3,
                ..model(M::Scrub)
            },
        ),
        ("tape_stretch", model(M::Stretch)),
        ("tape_loop_age", TapeEngineParams { age: 0.5, ..p }),
        ("tape_loop_tape", TapeEngineParams { tape: 0.5, ..p }),
        ("tape_oneshot", one),
        // SLICE 7: its value 8. SLICE is on hold (`slice_in_play`, review
        // of #761): it plays as `tape_oneshot`, whose hash it now is.
        (
            "tape_oneshot_slice7",
            TapeEngineParams {
                slice: chimera_core::dsp::tape_engine::SliceCode::new(8),
                ..one
            },
        ),
        // Re-recorded for WALK's TRON (ADR 0203): its tape is LENGTH's of
        // 8 s whatever the strip, the window round again until then.
        // Held: LENGTH ½, a 1.4 s tape; released: LENGTH 1, 8 s, fades at
        // the key up.
        (
            "tape_tron_held",
            TapeEngineParams {
                size: 0.5,
                ..p.with_way(chimera_core::dsp::tape_engine::Way::Tron)
            },
        ),
        (
            "tape_tron_released",
            p.with_way(chimera_core::dsp::tape_engine::Way::Tron),
        ),
        ("tape_stretch_reverse", rev(M::Stretch)),
        // GRAIN (ADR 0204): its own rows; CLASSIC's above are as recorded
        // before GRAIN existed. Re-recorded on #749's final core: the strike
        // reads its own head (a hair off the old grain's at the second
        // spot), and a reversed grain reads forward at a negative speed,
        // its taps no longer a frame ahead of its place. Re-recorded again
        // for the window's own period (review of #761, M2): the source has
        // no PERIOD, and its grains now lock to the one its window repeats at;
        // again for that search's mean taken off and its answer kept to 1/64
        // frame, once a window.
        (
            "tape_stretch_grain",
            TapeEngineParams {
                kind: chimera_core::dsp::tape_engine::StretchType::Grain,
                ..model(M::Stretch)
            },
        ),
        (
            "tape_stretch_grain_reverse",
            TapeEngineParams {
                kind: chimera_core::dsp::tape_engine::StretchType::Grain,
                ..rev(M::Stretch)
            },
        ),
        (
            "tape_loop_warble",
            TapeEngineParams {
                tape: 0.5,
                wow: chimera_core::dsp::tape_engine::Wow::Warble,
                ..p
            },
        ),
    ]
}

/// `t` on INIT through one voice: on the saw with its burst at 24 and at
/// 8 kHz (TRANSWAVE: the saw-to-square transwave), C3, C4 and C5, each 2 s,
/// its last 0.5 s released.
fn tape_render(t: &TapeEngineParams) -> Vec<f32> {
    use chimera_core::dsp::tape_engine::TapeModel;
    use common::tape::{held_plain, saw_to_square, saw_with_burst};
    let mut s = Sound::init(EngineType::Tape);
    s.params.tape = *t;
    let sources = if t.model() == TapeModel::Transwave {
        vec![saw_to_square()]
    } else {
        [24_000, 8_000]
            .map(|r| held_plain(&saw_with_burst(r, 1.0), r))
            .to_vec()
    };
    let mut out = Vec::new();
    let mut b = [0.0f32; chimera_hal::BLOCK_SIZE];
    for held in &sources {
        let mut v = Rig::with_source(SR, held);
        for n in [48, 60, 72] {
            v.note_on(
                MidiNote::new(n).unwrap(),
                Velocity::new(VEL).unwrap(),
                &s.params,
            );
            for i in 0..1_500 {
                if i == 1_125 {
                    v.note_off();
                }
                v.render(&mut b, &s.params, &s.mod_state);
                out.extend_from_slice(&b);
            }
        }
    }
    out
}

/// Task 12's REVERSE Sound (MODEL code 1, START and END at their base),
/// loaded through `translate_v1`: LOOP with END before START, its render
/// the recorded `tape_reverse` row, bit for bit (#369).
#[test]
fn reverse_golden_replays_bit_identically() {
    use chimera_core::dsp::tape_engine::TapeModel;
    use chimera_core::storage::{MIGRATIONS, TRANSLATIONS, decode_block};
    let mut s = Sound::init(EngineType::Tape);
    // The golden plays the neutral's LENGTH 1, not INIT's 0.8.
    s.params.tape.size = 1.0;
    let mut payload = vec![34u8, 0];
    payload.extend_from_slice(&1u32.to_le_bytes());
    decode_block(&payload, MIGRATIONS, TRANSLATIONS, Some(&mut s.params)).unwrap();
    let t = s.params.tape;
    assert_eq!((t.model(), t.start, t.end), (TapeModel::Walk, 1.0, 0.0));
    let out = tape_render(&t);
    let want = TAPE_GOLDENS
        .iter()
        .find(|g| g.0 == "tape_reverse")
        .unwrap()
        .1;
    assert_eq!((fnv1a(&out), spots(&out)), want);
}

#[test]
fn tape_goldens_match() {
    let got: Vec<_> = tape_cases()
        .iter()
        .map(|(name, t)| {
            let out = tape_render(t);
            assert!(out.iter().any(|&x| x.abs() > 1e-3), "{name} silent");
            (*name, (fnv1a(&out), spots(&out)))
        })
        .collect();
    golden::check(TAPE_GOLDENS, &got);
}

/// DRUM slice 2's baseline (Task 0): the slice-1 project written by slice-1
/// code (`drum_kit_codec_test::write_slice1_fixture`), pads 1, 2, 8, 9 and
/// 10 struck at once-a-block steps, 2 s through the `Instrument`. Every
/// later task keeps it bit-identical, but for one deliberate change: a
/// pad's VEL law is its kernel's level (design ruling 2026-11-14, ADR
/// 0269), and the fixture's pads are on VEL 0.6. At VEL 1 it is slice 1's
/// own render, bit for bit (`drum_slice1_vel_test`). Re-recorded for #963
/// (ADR 0316): a pad's STRIKE hits at its level, so VEL 0.6's.
const DRUM_GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    (
        "drum_slice1_kit",
        (
            0x241f2ed2b7338765,
            [
                746962870, 1017080472, 1023637572, 996942397, 3154439718, 3154438215, 3127904708,
                974432302,
            ],
        ),
    ),
    (
        "drum_slice1_kit_lr",
        (
            0x5dc84f6617e44bbe,
            [
                0, 3147695438, 3171463568, 1017082813, 966338201, 959734228, 3149906471, 3148883336,
            ],
        ),
    ),
];

/// Part 1's mono bus, then pair 1's interleaved L/R as the DACs play it
/// (pad PAN lives only there).
fn drum_slice1_render() -> (Vec<f32>, Vec<f32>) {
    common::drum_slice1::render(|_| ())
}

#[test]
fn drum_slice1_kit_matches() {
    let (out, lr) = drum_slice1_render();
    assert!(out.iter().any(|&x| x.abs() > 1e-3), "silent");
    assert!(
        lr.as_chunks::<2>().0.iter().any(|c| c[0] != c[1]),
        "L and R never differ: the pad pan is not heard"
    );
    golden::check(
        DRUM_GOLDENS,
        &[
            ("drum_slice1_kit", (fnv1a(&out), spots(&out))),
            ("drum_slice1_kit_lr", (fnv1a(&lr), spots(&lr))),
        ],
    );
}
