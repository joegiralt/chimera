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
];

/// Goldens whose locked output no longer reflects intended behaviour, each
/// tracked at an issue: Modal failed the sanity gate (#10).
const KNOWN_BROKEN: &[(&str, &str)] = &[
    (
        "modal_init",
        "https://github.com/joegiralt/chimera/issues/10",
    ),
    (
        "modal_lfo_cutoff",
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
    assert_ne!(
        fnv1a(&render_case(Case::ModalLfoCutoff)),
        fnv1a(&render_case(Case::ModalInit)),
        "modal_lfo_cutoff renders the same as modal_init"
    );
}
