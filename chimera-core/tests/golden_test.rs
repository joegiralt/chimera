//! Refactor lock (spec 2026-09-23 § Testing).
//!
//! Goldens freeze today's output, good or bad — they are not a quality claim.
//! They must match bit-for-bit after every refactor step. Re-record ONLY for
//! a change the spec lists as intended, in the task that makes it
//! (`common::golden`).

mod common;

use common::*;

/// (case name, FNV-1a 64 over every sample's bits, sample bits at SPOT_IDX).
const GOLDENS: &[(&str, (u64, [u32; 8]))] = &[
    (
        "modal_init",
        (
            0x90f1197c153d0b05,
            [
                3146805428, 3183273506, 3195882399, 1063217482, 3191764060, 3172592491, 993906163,
                1000698095,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    (
        "modal_lfo_cutoff",
        (
            0xe9e4fe3dda9b0262,
            [
                3146805428, 3183273506, 3198007719, 1063217482, 3191764060, 3172592491, 992207671,
                3142402426,
            ],
        ),
    ),
    // Re-recorded, every Algo case but the morphs: silent carriers take no
    // share of the output (#189), a pure gain before the filter.
    // INIT, LFO CUTOFF and the Modal switch again: INIT is routed FM (ADR 0049).
    // Recorded after the Algo sanity gate (ADR 0011).
    (
        "algo_init",
        (
            0xde8a72f6cae976e7,
            [
                975759426, 3204829857, 1053061536, 1045082777, 3201491096, 3202226391, 3121230836,
                0,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    (
        "algo_lfo_cutoff",
        (
            0x6febb404a729912d,
            [
                975759426, 3204829857, 1053252594, 1045082776, 3201491096, 3202226391, 3121153935,
                0,
            ],
        ),
    ),
    (
        "algo_t1",
        (
            0x8a950cc516ca09c2,
            [
                980060491, 3208933760, 1040133075, 3175809379, 3199685806, 3184303931, 3120821931,
                0,
            ],
        ),
    ),
    (
        "algo_t2",
        (
            0x43504ede9373e5b8,
            [
                980804211, 1059338987, 3184838602, 3191725309, 3205373723, 3202575890, 3120821931,
                0,
            ],
        ),
    ),
    (
        "algo_t3",
        (
            0x4860570fe26b6bfe,
            [
                982619099, 1061043339, 3186221008, 3189462392, 3205598247, 3203921991, 3120819435,
                0,
            ],
        ),
    ),
    (
        "algo_t4",
        (
            0x77e8a9261b38dea3,
            [
                980163939, 3208353019, 3203892416, 3179952651, 1057886227, 1055786504, 3120812141,
                0,
            ],
        ),
    ),
    (
        "algo_t5",
        (
            0x66dfa92fba89cd3f,
            [
                975576866, 1058785250, 1058485312, 3180163876, 3203550136, 3203065776, 3117235440,
                0,
            ],
        ),
    ),
    (
        "algo_t6",
        (
            0xaf4cec3cf1ae90aa,
            [
                974378372, 1054227490, 1056220928, 3174289352, 3200665140, 3200114463, 3113800648,
                0,
            ],
        ),
    ),
    (
        "algo_t7",
        (
            0x68db5112f8b419f6,
            [
                971019043, 1042869409, 1056354341, 3158500627, 3200876996, 3200650306, 3113803655,
                0,
            ],
        ),
    ),
    (
        "algo_t8",
        (
            0xfdfa584bb70fc923,
            [
                970524175, 1035585929, 1054365328, 3154403056, 3199371472, 3199276798, 3112682023,
                0,
            ],
        ),
    ),
    (
        "algo_morph_static",
        (
            0x0aa07549d41d5dc0,
            [
                980865416, 3180258739, 1039999960, 3172698160, 3171969701, 3181376776, 3092390763,
                0,
            ],
        ),
    ),
    (
        "algo_morph_sweep",
        (
            0x04a6c036e951997b,
            [
                980865416, 3188008887, 1036634440, 3156268221, 1028937371, 1032383698, 3091938455,
                0,
            ],
        ),
    ),
    // Re-recorded: the switch fades Algo out, then Modal starts (#33 M6).
    (
        "algo_to_modal_switch",
        (
            0xd2c6bd5310d0f2de,
            [
                975759426, 3204829857, 1053061536, 3198051069, 3178790723, 3188171051, 3137935627,
                3125743219,
            ],
        ),
    ),
    // Filter-routing spec § Migration: recorded before the change; never re-recorded.
    (
        "factory_0",
        (
            0xb17cfda81990ee71,
            [
                966420381, 3201950157, 3196291495, 1048583721, 3191193765, 3190937477, 0, 0,
            ],
        ),
    ),
    (
        "factory_1",
        (
            0x5e691a37b4565603,
            [
                978778357, 1036387089, 1058171222, 3163061596, 3204828009, 3204847449, 3119450501,
                0,
            ],
        ),
    ),
    (
        "factory_2",
        (
            0x847ff053f5c8738a,
            [
                921423832, 1048912255, 3148590087, 3160135955, 3151800175, 1008431307, 3162917568,
                3103822871,
            ],
        ),
    ),
    (
        "factory_3",
        (
            0x2eda182a42f56930,
            [
                986320739, 1057811188, 3189240167, 1058676107, 1030159901, 998794720, 3194624645,
                1024385296,
            ],
        ),
    ),
    (
        "factory_4",
        (
            0xc2f5834cfc44f2ff,
            [
                914061624, 1060735927, 1054868575, 1037397037, 3197630998, 3197336039, 3118944989,
                0,
            ],
        ),
    ),
    (
        "factory_5",
        (
            0x1e46181ca96fa272,
            [
                902662618, 3208501822, 3207548569, 1061431587, 3208417344, 3208417008, 0, 0,
            ],
        ),
    ),
    (
        "factory_6",
        (
            0x29b3e58dfd18575b,
            [
                834313439, 1010085667, 1036978923, 994349819, 3182746209, 3181968895, 3178439848,
                997704184,
            ],
        ),
    ),
    (
        "factory_7",
        (
            0x3dc833ebd9700912,
            [
                972212234, 1048682744, 1049003804, 1029203815, 1040613396, 1040457063, 3110007328,
                0,
            ],
        ),
    ),
];

/// Goldens whose locked output no longer reflects intended behaviour, each
/// tracked at an issue: Modal failed the sanity gate (#10), and the switch
/// case's second half is Modal's.
const KNOWN_BROKEN: &[(&str, &str)] = &[
    (
        "modal_init",
        "https://github.com/joegiralt/chimera/issues/10",
    ),
    (
        "modal_lfo_cutoff",
        "https://github.com/joegiralt/chimera/issues/10",
    ),
    (
        "algo_to_modal_switch",
        "https://github.com/joegiralt/chimera/issues/10",
    ),
];

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
