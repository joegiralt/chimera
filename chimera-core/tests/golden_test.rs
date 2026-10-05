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
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "modal_init",
        (
            0x5176d0ac65f9a3d1,
            [
                0, 1048126220, 1052218609, 3177234998, 1048314789, 1048569258, 1040495686,
                3182558310,
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
        "modal_lfo_cutoff",
        (
            0x102a7b03053e661d,
            [
                0, 1048126220, 1053102339, 3177236354, 1048314741, 1048569210, 1040458604,
                996184393,
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
            0xc40e2c3dd96cc9d7,
            [
                1012689062, 1038551816, 1036302816, 3198907799, 1055292148, 1054094316, 1051530843,
                3193926805,
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
            0xac18195fc3101ceb,
            [
                972060177, 3198287341, 1049035940, 1041192466, 3197134115, 3197551485, 3169230522,
                3155283520,
            ],
        ),
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_lfo_cutoff",
        (
            0x1dab49a9800513aa,
            [
                972060177, 3198287341, 1049133884, 1041193425, 3197134037, 3197551406, 3168592718,
                1023771867,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t1",
        (
            0x338914f3a4a9afb3,
            [
                973136596, 3199095550, 1029401004, 3168216480, 3188781221, 3166622429, 3185833548,
                3160620186,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t2",
        (
            0x1d58e515e3735a87,
            [
                973545296, 1049607733, 3177018799, 3184382992, 3195443999, 3191636009, 3185821175,
                3161709992,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t3",
        (
            0x99e521972604a034,
            [
                975175686, 1051148883, 3178282459, 3181992097, 3195881675, 3192897117, 3185609824,
                3163500002,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t4",
        (
            0xd442c1935b585ad3,
            [
                973193445, 3198350446, 3193788259, 3172016707, 1048064525, 1044971578, 3184260242,
                3159716904,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t5",
        (
            0x87063bb722952de9,
            [
                972913803, 1052282164, 1052238905, 3177438969, 3197288083, 3196992014, 3186424013,
                3161057541,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t6",
        (
            0x6b5b7b394ee8e944,
            [
                973714044, 1050250631, 1052049530, 3174298559, 3197277840, 3196853927, 3180957860,
                3158113119,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t7",
        (
            0xb08ecc3848fdbdb7,
            [
                970009862, 1038215128, 1052175815, 3162593418, 3197461555, 3197296821, 3182571825,
                3154648884,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_t8",
        (
            0xa1c30bf3e89efef8,
            [
                971589691, 1032249938, 1052339992, 3160236107, 3197701223, 3197614217, 3183235600,
                3152545449,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_morph_static",
        (
            0x39ed449b407c938c,
            [
                981272904, 3178920668, 1036455980, 3173319077, 3170829742, 3180559784, 3173539048,
                3147980193,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "algo_morph_sweep",
        (
            0x39888b5354294798,
            [
                981272904, 3182365268, 1034303929, 3161722387, 1023475985, 1022702980, 3173462416,
                991728612,
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
            0xb8704edef6f53de2,
            [
                972060177, 3198287341, 1049035940, 3201774263, 992947158, 1024834130, 3148631123,
                3195747569,
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
            0x2b0219df38902ede,
            [
                966633155, 3199652310, 3193953164, 1045452874, 3188637170, 3188420254, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_1",
        (
            0x0e3d85b43ff9ccf9,
            [
                979074595, 1032020609, 1055785139, 3166978861, 3201981805, 3202000541, 3116682528,
                0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_2",
        (
            0xfe00848f39d1981e,
            [
                919634797, 1043285534, 982936894, 3161177423, 3140431246, 1007666467, 3158423231,
                3100585116,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_3",
        (
            0x901698e789a3ccf9,
            [
                986599185, 1055273736, 3186656317, 1056926714, 1025655748, 3147485307, 3192124968,
                1021142159,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_4",
        (
            0x68328532bf9779a7,
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
    (
        "factory_5",
        (
            0xdb1cd08add369508,
            [
                898824421, 3209678144, 3209854415, 1059992875, 3206492922, 3206485466, 0, 0,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_6",
        (
            0x144e8d10ce6ac51e,
            [
                834352396, 1009650854, 1036674498, 968914263, 3182119491, 3181374484, 3177329682,
                993036649,
            ],
        ),
    ),
    // Then CLEAN's ACCURATE tan and exact divides (#57).
    // Then CLEAN's k law and trims (ADR 0096).
    (
        "factory_7",
        (
            0xcabe755163058e15,
            [
                974638215, 1046941265, 1045727262, 999705034, 1033284198, 1032940907, 3133150842, 0,
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
