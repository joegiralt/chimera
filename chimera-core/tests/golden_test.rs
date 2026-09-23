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
    ("pizza_init", 0xc533d18a26331e4c, [3111930299, 1053289385, 999362770, 1051778009, 3163126675, 3167254354, 1028737615, 0]),
    ("pizza_lfo_cutoff", 0x54c74c6f8ae46bd4, [3111930299, 1053289385, 995579318, 1051778010, 3163126675, 3167254354, 1028758405, 0]),
    ("fm_init", 0x9bfe44d54ef0385b, [898059883, 1045152839, 1054792150, 3163439516, 3202136915, 3201882817, 0, 0]),
    ("fm_lfo_cutoff", 0x34b678f3574b1538, [898059883, 1045152839, 1054642467, 3163439516, 3202136915, 3201882817, 0, 0]),
    ("fm_lfo_op_a_level", 0x2016ba5789cfbf28, [898059883, 1045152839, 1049001337, 3163439517, 3202136915, 3201882817, 0, 0]),
    // Re-recorded in Task 22 (spec step 8): the FM pre-wire is gone, so the
    // FM init patch's own ModState is empty and this equals `fm_init`.
    ("fm_init_patch_mod", 0x9bfe44d54ef0385b, [898059883, 1045152839, 1054792150, 3163439516, 3202136915, 3201882817, 0, 0]),
    ("modal_init", 0x90f1197c153d0b05, [3146805428, 3183273506, 3195882399, 1063217482, 3191764060, 3172592491, 993906163, 1000698095]),
    ("modal_lfo_cutoff", 0x40afa2290a49dae2, [3146805428, 3183273506, 3196374285, 1063217482, 3191764060, 3172592491, 993770242, 999762977]),
    ("va_init", 0x5125674880996325, [0, 0, 0, 0, 0, 0, 0, 0]),
    ("pizza_to_modal_switch", 0x2b4f3ce20159fef6, [3111930299, 1053289385, 999362770, 1051584936, 3196878310, 3185521127, 3117844736, 984628055]),
];

/// Goldens that lock output which failed the sanity gate.
const KNOWN_BROKEN: &[(&str, &str)] = &[
    ("modal_init", "docs/issues/003-modal-sanity-gate.md"),
    ("modal_lfo_cutoff", "docs/issues/003-modal-sanity-gate.md"),
    ("pizza_to_modal_switch", "docs/issues/003-modal-sanity-gate.md"),
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
    assert!(failures.is_empty(), "golden mismatch:\n{}", failures.join("\n"));
}

#[test]
fn known_broken_goldens_have_issues() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    for (case, issue) in KNOWN_BROKEN {
        assert!(Case::ALL.iter().any(|c| c.name() == *case), "unknown case {case}");
        assert!(root.join(issue).exists(), "{case}: missing {issue}");
    }
}

/// Spec step 8: without the pre-wire, the FM init patch renders exactly like
/// FM init params with no modulation.
#[test]
fn fm_init_patch_has_no_prewire() {
    assert_eq!(fnv1a(&render_case(Case::FmInitPatchMod)), fnv1a(&render_case(Case::FmInit)));
}

#[test]
fn harness_is_deterministic() {
    for case in [Case::PizzaInit, Case::FmInit, Case::ModalInit] {
        assert_eq!(fnv1a(&render_case(case)), fnv1a(&render_case(case)), "{}", case.name());
    }
}

/// A route that silently does nothing would make its golden a copy of the
/// unmodulated one and lock nothing.
#[test]
fn modulated_cases_differ_from_unmodulated() {
    for (modulated, plain) in [
        (Case::PizzaLfoCutoff, Case::PizzaInit),
        (Case::FmLfoCutoff, Case::FmInit),
        (Case::FmLfoOpALevel, Case::FmInit),
        (Case::ModalLfoCutoff, Case::ModalInit),
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
