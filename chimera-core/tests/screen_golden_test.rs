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
    ("engine_algo", 0xed6d835a479b5c88),
    ("algo_alg", 0xb9b1d8f21befcbfd),
    ("algo_wave", 0x5d06a383acd3479c),
    ("algo_level", 0x8129f2285f9620fb),
    ("algo_osc_last", 0x4fd82a2e09729f46),
    ("bigviz_filter", 0x640722cfc10353bc),
    ("bigviz_env", 0x6ec81850142c237b),
    ("mixer_part", 0x87ab4a2a75c3d238),
    ("mixer_sends", 0x650322c12ad0b389),
    ("mixer_fx_delay", 0xd13240ab6a9b4ead),
    ("mixer_fx_reverb", 0x9e3a0671fa320bdf),
    ("mixer_fx_delay_char", 0xbb2132d7ceb9c620),
    ("mixer_tape", 0x28a0e7569c2009be),
    ("mixer_master", 0xc1981d439df14ddd),
    ("mixer_master_level", 0x7554d112cc792b98),
    ("mod_matrix", 0x7d8a119fbe438eeb),
    ("sound_browser", 0xdc94376f632eebac),
    ("system", 0x66ca9f7c486d2749),
    ("system_theme", 0x6f46ed28ddb10559),
    ("system_audio", 0x281985b580ac2fb1),
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
    assert!(
        failures.is_empty(),
        "screen golden mismatch:\n{}",
        failures.join("\n")
    );
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
