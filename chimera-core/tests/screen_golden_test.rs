//! Screen goldens (UI refresh spec § Testing): one FNV-1a hash per screen.
//!
//! A case is `Pending` until the task that converts its page type to
//! Direction A records it; from then on it is `Locked` and must match
//! bit-for-bit. Re-record a Locked case ONLY in a task whose text names that
//! case as an intended change:
//!
//!     SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture
//!
//! and paste the printed row over the case's entry. To look at the screens:
//!
//!     SCREEN_DUMP=/tmp/screens cargo test -p chimera-core --test screen_golden_test

mod screen;

use screen::*;

#[derive(Clone, Copy, Debug)]
enum Golden {
    /// Not yet converted: may change freely.
    Pending,
    /// Converted: must match.
    Locked(u64),
}
use Golden::*;

const GOLDENS: &[(&str, Golden)] = &[
    ("engine_pizza", Locked(0x4797d6f7d4edc427)),
    ("engine_fm_alg", Pending),
    ("engine_fm_op", Pending),
    ("bigviz_filter", Pending),
    ("bigviz_env", Pending),
    ("bigviz_fm_op_env", Pending),
    ("mixer_part", Pending),
    ("mixer_sends", Pending),
    ("mixer_fx_delay", Pending),
    ("mod_matrix", Pending),
    ("sound_browser", Pending),
    ("system", Locked(0x02f7456a841f9613)),
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
    for &(name, golden) in GOLDENS {
        let hash = render(name).hash();
        if record {
            println!("    (\"{name}\", Locked(0x{hash:016x})),");
            continue;
        }
        if let Locked(want) = golden
            && hash != want
        {
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
