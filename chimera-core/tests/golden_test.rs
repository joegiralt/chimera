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
    // Recorded: pins Sympathetic before its set moves to the pool (exclusive-state spec § 4.8).
    (
        "modal_sympathetic",
        (
            0x4bb5969650e6f28d,
            [
                3146803683, 3183282215, 3195097727, 1061213380, 3181792418, 1025081920, 1035423354,
                1043506943,
            ],
        ),
    ),
    // Re-recorded, every Algo case: the output scale is the carrier power
    // (ADR 0049); INIT routes four operators. Factory Sounds compensate in
    // their data and match 937b89f to about 1e-7, but MORPH PAD (#192).
    // Recorded after the Algo sanity gate (ADR 0011).
    (
        "algo_init",
        (
            0xfd37f75c4096594b,
            [
                979028267, 3207916546, 1058702061, 1049487467, 3206756596, 3207238360, 3123848149,
                0,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    (
        "algo_lfo_cutoff",
        (
            0x5a30634ae49954d3,
            [
                979028267, 3207916546, 1058803516, 1049487467, 3206756596, 3207238360, 3123749027,
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
            0xce95395377a29a50,
            [
                979804951, 1061535611, 1061255510, 3183764018, 3207082620, 3206746387, 3121510640,
                0,
            ],
        ),
    ),
    (
        "algo_t6",
        (
            0xcdcdf11eaf89cc01,
            [
                981111285, 1060474575, 1061104222, 3180880537, 3207071769, 3206605087, 3120661572,
                0,
            ],
        ),
    ),
    (
        "algo_t7",
        (
            0xfd300d3c20a2bee7,
            [
                977162770, 1049569427, 1061215398, 3164941039, 3207251313, 3207059199, 3120664121,
                0,
            ],
        ),
    ),
    (
        "algo_t8",
        (
            0xbc6cf4fe496531f9,
            [
                978600191, 1043706901, 1061354415, 3162601003, 3207502930, 3207410335, 3120875091,
                0,
            ],
        ),
    ),
    (
        "algo_morph_static",
        (
            0x8a7de0f876efb3f9,
            [
                988166709, 3188017838, 1047273407, 3180401599, 3179722106, 3189060722, 3099853643,
                0,
            ],
        ),
    ),
    (
        "algo_morph_sweep",
        (
            0x6534b8bd7df2ab02,
            [
                988166709, 3191722895, 1044519805, 3163132163, 1030556663, 1033427679, 3099621367,
                0,
            ],
        ),
    ),
    // Re-recorded: the switch fades Algo out, then Modal starts (#33 M6).
    (
        "algo_to_modal_switch",
        (
            0x501ad70c947a9c4d,
            [
                979028267, 3207916546, 1058702061, 3198051069, 3178790723, 3188171051, 3137935627,
                3125743219,
            ],
        ),
    ),
    // Filter-routing spec § Migration: recorded before the change; never re-recorded.
    (
        "factory_0",
        (
            0x878c9ca6ca353aa1,
            [
                964437060, 3199329879, 3193359563, 1045509917, 3189007987, 3188798729, 0, 0,
            ],
        ),
    ),
    (
        "factory_1",
        (
            0x0edce6a988cf7f1b,
            [
                976193070, 1034005775, 1055856310, 3160335094, 3201989703, 3202021448, 3116575812,
                0,
            ],
        ),
    ),
    (
        "factory_2",
        (
            0xe0277f16dca55649,
            [
                916897409, 1044137601, 3144861666, 3155915961, 3147564708, 1004262271, 3158174435,
                3098924639,
            ],
        ),
    ),
    (
        "factory_3",
        (
            0x10424d2460e991df,
            [
                983890739, 1055268379, 3187154724, 1056680785, 1027381960, 996064463, 3191809284,
                1021923851,
            ],
        ),
    ),
    (
        "factory_4",
        (
            0x05a4301cc61a2332,
            [
                908004282, 1055568621, 1049472589, 1032256244, 3191880893, 3191507797, 3113373004,
                0,
            ],
        ),
    ),
    (
        "factory_5",
        (
            0x73b6b9429389dc2a,
            [
                898768015, 3209206342, 3207329765, 1059949212, 3206682304, 3206681783, 0, 0,
            ],
        ),
    ),
    (
        "factory_6",
        (
            0x2cf3c112b967ff6f,
            [
                834313439, 1010034469, 1036664749, 993748116, 3182035191, 3181304489, 3177014926,
                995803289,
            ],
        ),
    ),
    (
        "factory_7",
        (
            0x7ef436e312c3c9e6,
            [
                972212234, 1048682743, 1049003805, 1029203814, 1040613395, 1040457062, 3110007328,
                0,
            ],
        ),
    ),
];

/// Goldens whose locked output no longer reflects intended behaviour, each
/// tracked at an issue: Modal failed the sanity gate (#10), the switch
/// case's second half is Modal's, and Sympathetic rings on the same KS
/// main string.
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
    (
        "modal_sympathetic",
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
