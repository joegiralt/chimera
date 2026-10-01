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
    (
        "modal_init",
        (
            0x4243cb9c42f83c9f,
            [
                0, 1048807944, 1052993046, 3175859692, 1048761361, 1048921425, 1040866814,
                3182965587,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    (
        "modal_lfo_cutoff",
        (
            0xdf51262461e9ea61,
            [
                0, 1048807944, 1053700428, 3175860787, 1048761337, 1048921401, 1040819211,
                997094396,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    (
        "modal_sympathetic",
        (
            0xa54f47f7a39b707f,
            [
                1009791328, 1040156560, 1038559555, 3198886150, 1055430442, 1054256918, 1051482770,
                3194203623,
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
            0xfb0049be73414def,
            [
                968899253, 3198255792, 1049044563, 1039617262, 3197079237, 3197478036, 3168962637,
                3155020483,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    (
        "algo_lfo_cutoff",
        (
            0x26b1ec7cfa304762,
            [
                968899253, 3198255792, 1049146926, 1039619110, 3197079167, 3197477965, 3168417864,
                1023793805,
            ],
        ),
    ),
    (
        "algo_t1",
        (
            0x7349c3ee8f6e0882,
            [
                969806185, 3199004609, 1029459700, 3166510955, 3189843948, 3174257329, 3185836196,
                3160209368,
            ],
        ),
    ),
    (
        "algo_t2",
        (
            0x37c021f200407c9b,
            [
                970459631, 1049532273, 3175103285, 3182583717, 3195627288, 3192355118, 3185818291,
                3161273669,
            ],
        ),
    ),
    (
        "algo_t3",
        (
            0x647287b0cd63475a,
            [
                973066362, 1051091222, 3175965882, 3180527447, 3195953789, 3193473282, 3185607553,
                3163247232,
            ],
        ),
    ),
    (
        "algo_t4",
        (
            0x3c792488e577d704,
            [
                969897075, 3198433800, 3193408956, 3170689807, 1048215971, 1045556635, 3184243638,
                3159350147,
            ],
        ),
    ),
    (
        "algo_t5",
        (
            0x7c24e572bcc73f92,
            [
                969581661, 1052320098, 1052257028, 3175623428, 3197341792, 3197041711, 3186423701,
                3160622414,
            ],
        ),
    ),
    (
        "algo_t6",
        (
            0xe091f4c42fd58aa8,
            [
                970729431, 1050323904, 1052098571, 3172820053, 3197341897, 3196927102, 3180949047,
                3157770830,
            ],
        ),
    ),
    (
        "algo_t7",
        (
            0xf5aada8c1e120778,
            [
                967260192, 1038517857, 1052198843, 3161143014, 3197496831, 3197323584, 3182518081,
                3154394079,
            ],
        ),
    ),
    (
        "algo_t8",
        (
            0xa9d76f274af36f65,
            [
                968523136, 1032473239, 1052345316, 3159053175, 3197714901, 3197629147, 3183160844,
                3152113729,
            ],
        ),
    ),
    (
        "algo_morph_static",
        (
            0x68892615d036ee8c,
            [
                977946678, 3178364303, 1036987351, 3171162915, 3169927340, 3179564142, 3173810278,
                3147875321,
            ],
        ),
    ),
    (
        "algo_morph_sweep",
        (
            0x3a75e01191013bb5,
            [
                977946678, 3181838225, 1034634472, 3153848919, 1019649474, 1023531709, 3173731854,
                991512499,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    (
        "algo_to_modal_switch",
        (
            0xfa1ca6255bbf0713,
            [
                968899253, 3198255792, 1049044563, 3202274326, 964908236, 1023507262, 3134618867,
                3196302216,
            ],
        ),
    ),
    // Filter-routing spec § Migration: recorded before the change; re-recorded
    // only for ADR 0060's DC blocker.
    (
        "factory_0",
        (
            0xe98faae37ce6244f,
            [
                964431655, 3199621196, 3193929155, 1045455415, 3188672660, 3188457327, 0, 0,
            ],
        ),
    ),
    (
        "factory_1",
        (
            0xab3ee51d703f17d9,
            [
                976189307, 1032186217, 1055811887, 3165805991, 3201975514, 3201997890, 3116632131,
                0,
            ],
        ),
    ),
    (
        "factory_2",
        (
            0x07d23e6d0289e847,
            [
                916893834, 1043500485, 3139678468, 3159224572, 3147298735, 1004526703, 3158368408,
                3100205277,
            ],
        ),
    ),
    (
        "factory_3",
        (
            0x6136e6c43abf648a,
            [
                983887203, 1055168726, 3187037984, 1056707535, 1027287216, 994480067, 3191962860,
                1021274334,
            ],
        ),
    ),
    (
        "factory_4",
        (
            0x9f9f17695f406811,
            [
                908000873, 1055245821, 1049486858, 1030830351, 3191695879, 3191314783, 3113304278,
                0,
            ],
        ),
    ),
    (
        "factory_5",
        (
            0x1c921c39e8ff9a60,
            [
                898764883, 3209726295, 3209318024, 1060029534, 3206500298, 3206492945, 0, 0,
            ],
        ),
    ),
    (
        "factory_6",
        (
            0xaa780f9a0cee1652,
            [
                834309438, 1009665531, 1036684447, 974381382, 3182100435, 3181362640, 3177305534,
                993246012,
            ],
        ),
    ),
    (
        "factory_7",
        (
            0x2f4ec4667083a813,
            [
                972207030, 1046949504, 1045678340, 3144296506, 1033321750, 1033002702, 3133144091,
                0,
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
