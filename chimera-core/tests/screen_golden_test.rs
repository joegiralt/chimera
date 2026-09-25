//! Screen goldens (UI refresh spec § Testing): one FNV-1a hash per screen.
//!
//! Every case must match bit-for-bit. Re-record a case ONLY for a change
//! that is meant to alter that screen, in the commit that makes it:
//!
//!     SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture
//!
//! and paste the printed row over the case's entry. To look at the screens:
//!
//!     SCREEN_DUMP=/tmp/screens cargo test -p chimera-core --test screen_golden_test

mod screen;

use screen::*;

const GOLDENS: &[(&str, u64)] = &[
    ("engine_pizza", 0xbf9c1f573baa9d59),
    ("engine_fm_alg", 0x2a5b3df0165783a3),
    ("engine_fm_op", 0x2353d264169904b3),
    ("bigviz_filter", 0x24ef227569f308c9),
    ("bigviz_env", 0xf22986571c209654),
    ("bigviz_fm_op_env", 0xd64cac3d5290898e),
    ("mixer_part", 0x3e3480eee3041370),
    ("mixer_sends", 0x9a495656576afaf9),
    ("mixer_fx_delay", 0x872c17a4bee40923),
    ("mod_matrix", 0x5064c6063bd0880d),
    ("sound_browser", 0x91b39fd39c530bcd),
    ("system", 0x66ca9f7c486d2749),
];

#[test]
fn every_case_has_a_golden_entry() {
    let names: Vec<&str> = CASES.iter().map(|c| c.0).collect();
    let goldens: Vec<&str> = GOLDENS.iter().map(|g| g.0).collect();
    assert_eq!(names, goldens);
}

#[test]
fn screen_goldens_match() {
    let record = std::env::var_os("SCREEN_RECORD").is_some();
    let mut failures = Vec::new();
    for &(name, want) in GOLDENS {
        let hash = render(name).hash();
        if record {
            println!("    (\"{name}\", 0x{hash:016x}),");
            continue;
        }
        if hash != want {
            failures.push(format!("{name}: 0x{hash:016x} (want 0x{want:016x})"));
        }
    }
    assert!(failures.is_empty(), "screen golden mismatch:\n{}", failures.join("\n"));
}

#[test]
fn rendering_is_deterministic() {
    for &(name, _) in GOLDENS {
        assert_eq!(render(name).hash(), render(name).hash(), "{name}");
    }
}

#[test]
fn no_screen_draws_outside_240x320() {
    for &(name, _) in GOLDENS {
        assert_eq!(render(name).oob, 0, "{name}");
    }
}
