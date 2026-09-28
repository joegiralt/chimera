//! Refactor lock (spec 2026-09-23 § Testing).
//!
//! Goldens freeze today's output, good or bad — they are not a quality claim.
//! They must match bit-for-bit after every refactor step. Re-record ONLY for
//! a change the spec lists as intended, in the task that makes it:
//!
//!     GOLDEN_RECORD=1 cargo test -p chimera-core --test golden_test -- --nocapture
//!
//! and paste the printed rows over `GOLDENS`.

mod common;

use common::*;

/// (case name, FNV-1a 64 over every sample's bits, sample bits at SPOT_IDX).
const GOLDENS: &[(&str, u64, [u32; 8])] = &[
    (
        "modal_init",
        0x90f1197c153d0b05,
        [
            3146805428, 3183273506, 3195882399, 1063217482, 3191764060, 3172592491, 993906163,
            1000698095,
        ],
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    (
        "modal_lfo_cutoff",
        0xe9e4fe3dda9b0262,
        [
            3146805428, 3183273506, 3198007719, 1063217482, 3191764060, 3172592491, 992207671,
            3142402426,
        ],
    ),
    // Recorded after the Algo sanity gate (ADR 0011).
    (
        "algo_init",
        0xf36afbe129df33fa,
        [
            971731855, 1043306224, 1058062728, 3165110143, 3205522154, 3205490133, 3120319999, 0,
        ],
    ),
    // Re-recorded: CUTOFF routes in octaves, g ramped per block (filter-routing spec § 3).
    (
        "algo_lfo_cutoff",
        0x9179ae36f88dc7a3,
        [
            971731855, 1043306224, 1058060417, 3165110143, 3205522154, 3205490133, 3120180003, 0,
        ],
    ),
    (
        "algo_t1",
        0x57f7fb7dad21330b,
        [
            975558553, 3205163018, 1035235053, 3171912679, 3196166760, 3180376198, 3116015589, 0,
        ],
    ),
    (
        "algo_t2",
        0x61296f52bf29b39f,
        [
            976084442, 1055408551, 3180754269, 3188080873, 3200843130, 3198210359, 3116015590, 0,
        ],
    ),
    (
        "algo_t3",
        0x5ed88cdff093f5d5,
        [
            978182322, 1057391737, 3181731779, 3185290458, 3201160651, 3199162194, 3116012059, 0,
        ],
    ),
    (
        "algo_t4",
        0x8470ff7a980cfbbc,
        [
            975631701, 3204752372, 3199141284, 3175316328, 1053354039, 1051217629, 3116001746, 0,
        ],
    ),
    (
        "algo_t5",
        0x3905a60e3d653931,
        [
            973579067, 1056859010, 1056369218, 3177643136, 3200636257, 3200240779, 3114767223, 0,
        ],
    ),
    (
        "algo_t6",
        0xd09b9930f903a536,
        [
            973080338, 1052346433, 1054072797, 3172710543, 3198924223, 3198447324, 3112458771, 0,
        ],
    ),
    (
        "algo_t7",
        0xd1bd6882436f4f51,
        [
            969047199, 1041386195, 1054188336, 3156789383, 3199107695, 3198911376, 3112461375, 0,
        ],
    ),
    (
        "algo_t8",
        0x97d3c3b17a154868,
        [
            969022679, 1034300548, 1052868578, 3152857871, 3198136270, 3198051589, 3111311968, 0,
        ],
    ),
    (
        "algo_morph_static",
        0x0aa07549d41d5dc0,
        [
            980865416, 3180258739, 1039999960, 3172698160, 3171969701, 3181376776, 3092390763, 0,
        ],
    ),
    (
        "algo_morph_sweep",
        0x04a6c036e951997b,
        [
            980865416, 3188008887, 1036634440, 3156268221, 1028937371, 1032383698, 3091938455, 0,
        ],
    ),
    // Re-recorded: the switch fades Algo out, then Modal starts (#33 M6).
    (
        "algo_to_modal_switch",
        0xcb7b77755402857f,
        [
            971731855, 1043306224, 1058062728, 3198051069, 3178790723, 3188171051, 3137935627,
            3125743219,
        ],
    ),
    // Filter-routing spec § Migration: recorded before the change; never re-recorded.
    (
        "factory_0",
        0xc0e212b1b98a9bcb,
        [
            964437059, 3199329880, 3193359562, 1045509919, 3189007987, 3188798728, 0, 0,
        ],
    ),
    (
        "factory_1",
        0xd11d69ebe6b0e4f7,
        [
            976193070, 1034005773, 1055856311, 3160335090, 3201989701, 3202021445, 3116575811, 0,
        ],
    ),
    (
        "factory_2",
        0x5d86bbdbd84012d3,
        [
            916897411, 1044137602, 3144861674, 3155915962, 3147564710, 1004262272, 3158174435,
            3098924637,
        ],
    ),
    (
        "factory_3",
        0xe34e4665f140410c,
        [
            983890740, 1055268379, 3187154725, 1056680784, 1027381955, 996064416, 3191809284,
            1021923848,
        ],
    ),
    (
        "factory_4",
        0x44a3e31517fc8b68,
        [
            908004282, 1055568624, 1049472589, 1032256248, 3191880895, 3191507797, 3113373005, 0,
        ],
    ),
    (
        "factory_5",
        0xdadaa3f8d1eecb4a,
        [
            898768017, 3209206341, 3207329766, 1059949211, 3206682307, 3206681785, 0, 0,
        ],
    ),
    (
        "factory_6",
        0x29b3e58dfd18575b,
        [
            834313439, 1010085667, 1036978923, 994349819, 3182746209, 3181968895, 3178439848,
            997704184,
        ],
    ),
    (
        "factory_7",
        0x3dc833ebd9700912,
        [
            972212234, 1048682744, 1049003804, 1029203815, 1040613396, 1040457063, 3110007328, 0,
        ],
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
    let record = std::env::var_os("GOLDEN_RECORD").is_some();
    let mut failures = Vec::new();
    for case in Case::ALL {
        let out = render_case(case);
        let (hash, sp) = (fnv1a(&out), spots(&out));
        if record {
            println!("    (\"{}\", 0x{hash:016x}, {sp:?}),", case.name());
            continue;
        }
        match GOLDENS.iter().find(|g| g.0 == case.name()) {
            None => failures.push(format!(
                "{}: no golden recorded (run with GOLDEN_RECORD=1)",
                case.name()
            )),
            Some(&(_, want_hash, want_spots)) => {
                if hash != want_hash || sp != want_spots {
                    failures.push(format!(
                        "{}: hash 0x{hash:016x} (want 0x{want_hash:016x}), spots {sp:?} (want {want_spots:?})",
                        case.name()
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "golden mismatch:\n{}",
        failures.join("\n")
    );
}

/// Spec § Testing: part 1's mono bus through the voice pool matches every
/// golden bit-for-bit.
#[test]
fn goldens_match_through_the_instrument() {
    let mut failures = Vec::new();
    for case in Case::ALL {
        let out = render_case_through_instrument(case);
        let (hash, sp) = (fnv1a(&out), spots(&out));
        let &(_, want_hash, want_spots) = GOLDENS
            .iter()
            .find(|g| g.0 == case.name())
            .expect("recorded");
        if hash != want_hash || sp != want_spots {
            failures.push(format!(
                "{}: hash 0x{hash:016x} (want 0x{want_hash:016x})",
                case.name()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "instrument golden mismatch:\n{}",
        failures.join("\n")
    );
}

#[test]
fn known_broken_goldens_have_issues() {
    const TRACKER: &str = "https://github.com/joegiralt/chimera/issues/";
    for (case, issue) in KNOWN_BROKEN {
        assert!(
            Case::ALL.iter().any(|c| c.name() == *case),
            "unknown case {case}"
        );
        let num = issue.strip_prefix(TRACKER);
        assert!(
            num.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())),
            "{case}: {issue} is not a GitHub issue URL"
        );
    }
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
