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
    // names what came before that.
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    (
        "modal_init",
        (
            0xdbbb98e8b60c7fba,
            [
                0, 1065998110, 1070383691, 3193276627, 1065949295, 1066117027, 1058078416,
                3200321076,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    (
        "modal_lfo_cutoff",
        (
            0x4f273d07806c9c34,
            [
                0, 1065998110, 1071124959, 3193277774, 1065949270, 1066117002, 1058028533,
                1014620204,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    (
        "modal_sympathetic",
        (
            0x821d0a45aaa916a3,
            [
                1019770853, 1049714224, 1048805706, 3208819952, 1065636210, 1064583994, 1061427631,
                3204548148,
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
            0x002d6721f776f6d6,
            [
                979023578, 3208099471, 1058666167, 1049457832, 3206758411, 3207233017, 3123899385,
                0,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    (
        "algo_lfo_cutoff",
        (
            0x32e603be42342182,
            [
                979023578, 3208099471, 1058770735, 1049450744, 3206758978, 3207233584, 3123816827,
                0,
            ],
        ),
    ),
    (
        "algo_t1",
        (
            0xb8c7b95d10e081bd,
            [
                980055464, 3208955362, 1039837331, 3176608090, 3199687711, 3184291440, 3120810018,
                0,
            ],
        ),
    ),
    (
        "algo_t2",
        (
            0x865ae348ce956055,
            [
                980798941, 1059209186, 3185227205, 3192582927, 3205358055, 3202533603, 3121162531,
                0,
            ],
        ),
    ),
    (
        "algo_t3",
        (
            0xb8902b01b8df8a52,
            [
                982615979, 1060982329, 3186208981, 3190243355, 3205543798, 3203801607, 3121069083,
                0,
            ],
        ),
    ),
    (
        "algo_t4",
        (
            0x95dc415c590a5c6f,
            [
                980158877, 3208306622, 3203744975, 3180322295, 1057915552, 1055833923, 3121033000,
                0,
            ],
        ),
    ),
    (
        "algo_t5",
        (
            0xe4f10c4bacd1256d,
            [
                979800007, 1061204758, 1061237236, 3185819651, 3207062822, 3206719499, 3121605471,
                0,
            ],
        ),
    ),
    (
        "algo_t6",
        (
            0xadaaa533295a5310,
            [
                981105914, 1060123248, 1061074427, 3182630167, 3207062939, 3206589211, 3120736283,
                0,
            ],
        ),
    ),
    (
        "algo_t7",
        (
            0xf48b4c927c5d3f1d,
            [
                977158690, 1048796397, 1061183769, 3171278751, 3207239247, 3207039883, 3120710545,
                0,
            ],
        ),
    ),
    (
        "algo_t8",
        (
            0x9a777c173de5651c,
            [
                978595640, 1042171162, 1061327875, 3169275871, 3207487319, 3207387277, 3120884052,
                0,
            ],
        ),
    ),
    (
        "algo_morph_static",
        (
            0x29fddb3a37ba02d0,
            [
                988161773, 3188304491, 1047246592, 3180744362, 3179888374, 3189139159, 3102945915,
                0,
            ],
        ),
    ),
    (
        "algo_morph_sweep",
        (
            0xd6b2781f7c726024,
            [
                988161773, 3191734728, 1044569539, 3163508695, 1029831469, 1033059501, 3105394975,
                0,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    (
        "algo_to_modal_switch",
        (
            0xabac56023ea7eee9,
            [
                979023578, 3208099471, 1058666167, 3219751083, 982097749, 1040690967, 3152069440,
                3213492891,
            ],
        ),
    ),
    // Filter-routing spec § Migration: recorded before the change; re-recorded
    // only for ADR 0060's DC blocker.
    (
        "factory_0",
        (
            0x3bf08f68be7bc44f,
            [
                964431655, 3199621196, 3193929155, 1045455415, 3188672660, 3188457327, 0, 0,
            ],
        ),
    ),
    (
        "factory_1",
        (
            0xc67ca8068a7877d9,
            [
                976189307, 1032186217, 1055811887, 3165805991, 3201975514, 3201997890, 3116632131,
                0,
            ],
        ),
    ),
    (
        "factory_2",
        (
            0x3525cc1931c29ca2,
            [
                916893834, 1043500485, 3139678468, 3159224572, 3147298735, 1004526703, 3158368408,
                3100205277,
            ],
        ),
    ),
    (
        "factory_3",
        (
            0x14e064252e938abf,
            [
                983887203, 1055168726, 3187037984, 1056707535, 1027287216, 994480067, 3191962860,
                1021274334,
            ],
        ),
    ),
    (
        "factory_4",
        (
            0x8e66d469654ec811,
            [
                908000873, 1055245821, 1049486858, 1030830351, 3191695879, 3191314783, 3113304278,
                0,
            ],
        ),
    ),
    (
        "factory_5",
        (
            0xdf137d14d05668ec,
            [
                898764883, 3209887663, 3207934820, 1060108783, 3206499179, 3206491827, 0, 0,
            ],
        ),
    ),
    (
        "factory_6",
        (
            0xfd388eee4c112513,
            [
                834309438, 1009665531, 1036684447, 974381382, 3182100435, 3181362640, 3177305534,
                993246012,
            ],
        ),
    ),
    (
        "factory_7",
        (
            0x81c2fbb41ba2c813,
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
