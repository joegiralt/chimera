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
    (
        "modal_lfo_cutoff",
        0x40afa2290a49dae2,
        [
            3146805428, 3183273506, 3196374285, 1063217482, 3191764060, 3172592491, 993770242,
            999762977,
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
    (
        "algo_lfo_cutoff",
        0xa81aa97b0c8dfeb6,
        [
            971731855, 1043306224, 1058063011, 3165110143, 3205522154, 3205490133, 3120308848, 0,
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
    (
        "algo_to_modal_switch",
        0x25a631e6c11ed372,
        [
            971731855, 1043306224, 1058062728, 1051584936, 3196878310, 3185521127, 3117844736,
            984628055,
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
