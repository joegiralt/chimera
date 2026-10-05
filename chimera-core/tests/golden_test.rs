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
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    (
        "modal_init",
        (
            0x0b86619362c2bd86,
            [
                0, 1048130049, 1052223445, 3177195002, 1048312954, 1048568510, 1040495471,
                3182556235,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    (
        "modal_lfo_cutoff",
        (
            0x6b7b182a0801dd22,
            [
                0, 1048130049, 1053104401, 3177196352, 1048312903, 1048568458, 1040457572,
                996432783,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062), then SYMP's out clip, stmlib's
    // rational curve for `tanhf` (ADR 0097).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "modal_sympathetic",
        (
            0x237e18a837e03e23,
            [
                1012634585, 1038584383, 1036349511, 3198907774, 1055296120, 1054098540, 1051530283,
                3193932763,
            ],
        ),
    ),
    // Re-recorded, every Algo case: the output scale is the carrier power
    // (ADR 0049); INIT routes four operators. Factory Sounds compensate in
    // their data and match 937b89f to about 1e-7, but MORPH PAD (#192).
    // Recorded after the Algo sanity gate (ADR 0011).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_init",
        (
            0x17badeee4796aa04,
            [
                972000743, 3198286712, 1049036115, 1041165645, 3197132999, 3197549984, 3169224985,
                3155278114,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_lfo_cutoff",
        (
            0x3afe861d1833814e,
            [
                972000743, 3198286712, 1049135525, 1041166648, 3197132912, 3197549896, 3168584400,
                1023793799,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t1",
        (
            0x2fcff6beeda3ea0b,
            [
                973104740, 3199093595, 1029424440, 3168178737, 3188801531, 3166864242, 3185833603,
                3160611643,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t2",
        (
            0xf5ecef181702b7ea,
            [
                973511898, 1049606175, 3176979084, 3184345673, 3195447274, 3191650866, 3185821110,
                3161700955,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t3",
        (
            0xeedd1bcbb3f4fbfb,
            [
                975136140, 1051147691, 3178234101, 3181961713, 3195882728, 3192908927, 3185609780,
                3163494781,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t4",
        (
            0x258096473c4d7b9b,
            [
                973161373, 3198352118, 3193782070, 3171990724, 1048066720, 1044983843, 3184259921,
                3159709337,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t5",
        (
            0x4e0b407d256c5073,
            [
                972851153, 1052282914, 1052239202, 3177401553, 3197289251, 3196993042, 3186423998,
                3161048607,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t6",
        (
            0x1d99854a90c1eae9,
            [
                973680009, 1050252135, 1052050473, 3174268141, 3197279196, 3196855413, 3180957695,
                3158106002,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t7",
        (
            0xaa00f5945c74fd50,
            [
                969958162, 1038221324, 1052176241, 3162577594, 3197462327, 3197297358, 3182570698,
                3154643559,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_t8",
        (
            0x7dd73887b38d5584,
            [
                971532030, 1032254560, 1052340098, 3160211768, 3197701516, 3197614532, 3183234059,
                3152536390,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_morph_static",
        (
            0x7654c7bf0b352e6b,
            [
                981210362, 3178905166, 1036463405, 3173273820, 3170827040, 3180534557, 3173544681,
                3147977966,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "algo_morph_sweep",
        (
            0xb70aef06b8dbb656,
            [
                981210362, 3182354122, 1034307198, 3161573018, 1023432677, 1022712148, 3173467981,
                991723855,
            ],
        ),
    ),
    // Re-recorded: Modal 2 step A's resonators (spec § Tests), then ADR 0058's gain,
    // then Task 18's DAMP make-up and relative silence (ADR 0056),
    // then the free ring on release (ADR 0062).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then STRING's out_gain 5.7 for it (ADR 0063's peak rule).
    (
        "algo_to_modal_switch",
        (
            0x2e71270bdc728ca2,
            [
                972000743, 3198286712, 1049036115, 3201771932, 992742944, 1024799747, 3148467438,
                3195749056,
            ],
        ),
    ),
    // Filter-routing spec § Migration: recorded before the change; re-recorded
    // only for ADR 0060's DC blocker.
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_0",
        (
            0xaa7131f2dfebe366,
            [
                966594190, 3199651666, 3193952662, 1045452929, 3188637911, 3188421023, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_1",
        (
            0x1a1b51b985d2434f,
            [
                979020344, 1032024055, 1055785694, 3166954412, 3201981665, 3202000478, 3116681394,
                0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_2",
        (
            0xeb4d1ecae1a1ab64,
            [
                919583258, 1043289988, 982293406, 3161136891, 3140603612, 1007623845, 3158422115,
                3100577339,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_3",
        (
            0x91023c60160d5d1d,
            [
                986548193, 1055271554, 3186664128, 1056922158, 1025689417, 3147140860, 3192121623,
                1021144886,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_4",
        (
            0xcb63c26c7687e135,
            [
                908006558, 1055245974, 1049487006, 1030822531, 3191695551, 3191314470, 3113304079,
                0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_5",
        (
            0xcf21fffaad39cf99,
            [
                898764886, 3209726290, 3209318023, 1060029535, 3206500300, 3206492947, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_6",
        (
            0xaecf1b8852ff9ee6,
            [
                834310083, 1009665465, 1036684398, 974376884, 3182100486, 3181362640, 3177305616,
                993245287,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    (
        "factory_7",
        (
            0xdc097c46ef5fbee9,
            [
                974600695, 1046941466, 1045726236, 999345947, 1033284981, 1032942180, 3133150627, 0,
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
