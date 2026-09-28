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
    ("engine_algo", 0x0142a10b2a5efce8),
    ("algo_alg", 0x194e5ec92bb3327d),
    ("algo_wave", 0x077421b3106b8510),
    ("algo_level", 0xd71899ea7d4a4d2b),
    ("algo_osc_last", 0xe14e98097058782e),
    ("bigviz_filter", 0xb50c845f7f09909e),
    ("flt_mode", 0x7bb550d437421757),
    ("env_a", 0x54ba3f08b9436d40),
    ("env_b_env_ad", 0x8c13c7f2b2b13a2c),
    ("env_b_env_ahr", 0x239b8de9de91ae55),
    ("env_b_env_cycle", 0x8661f41e9aaba3bb),
    ("env_b_lfo_free", 0xa44221f904c8a5a1),
    ("env_b_lfo_sync", 0x2ae7574467c3e409),
    ("env_b_lfo_lfv", 0x87f0d152f27af928),
    ("env_b_burst_ad", 0x85aa6fdb690493dd),
    ("env_b_burst_ahr", 0xf18a55e8ad5fed5a),
    ("env_b_burst_cycle", 0x45af240dacae29a6),
    ("amp_vel_dimmed", 0xc8acdbe703babe70),
    ("amp_vel_live", 0xe969b1ab3a3e37fb),
    ("modal_amp", 0xad2feb025c2dfa59),
    ("mixer_part", 0x87ab4a2a75c3d238),
    ("mixer_sends", 0x650322c12ad0b389),
    ("mixer_fx_delay", 0xd13240ab6a9b4ead),
    ("mixer_fx_reverb", 0x9e3a0671fa320bdf),
    ("mixer_fx_delay_char", 0xbb2132d7ceb9c620),
    ("mixer_tape", 0x28a0e7569c2009be),
    ("mixer_master", 0xc1981d439df14ddd),
    ("mixer_master_level", 0x7554d112cc792b98),
    ("mod_matrix", 0xaf1d32846ef434b2),
    ("sound_browser", 0xaf0c38105a4aee59),
    ("system", 0xbadd5e55da36c80d),
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
