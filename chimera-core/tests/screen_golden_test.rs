//! Screen goldens (UI refresh spec § Testing): one FNV-1a hash per screen.
//!
//! Every case must match bit-for-bit. Re-record ONLY for a change that is
//! meant to alter a screen, in the commit that makes it (`common::golden`).
//! To look at the screens:
//!
//!     SCREEN_DUMP=/tmp/screens cargo test -p chimera-core --test screen_golden_test

mod common;
mod screen;

use screen::*;

const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0x2fc2acd0ca736b91),
    ("algo_alg", 0x7d93a049d17ce0d4),
    ("algo_alg_morph_dimmed", 0x897c253790c1a56a),
    ("mod_matrix_morph_inert", 0xcb22cbb86571bf4e),
    ("algo_wave", 0x077421b3106b8510),
    ("algo_level", 0xbf166056bf540001),
    ("algo_osc_last", 0xe14e98097058782e),
    ("bigviz_filter", 0xb50c845f7f09909e),
    ("flt_mode", 0x7bb550d437421757),
    ("env_a", 0x83a56036fe4bec0e),
    ("env_b_env_ad", 0x8c13c7f2b2b13a2c),
    ("env_b_env_ahr", 0x239b8de9de91ae55),
    ("env_b_env_cycle", 0x8661f41e9aaba3bb),
    ("env_b_lfo_free", 0xa44221f904c8a5a1),
    ("env_b_lfo_sync", 0x2ae7574467c3e409),
    ("env_b_lfo_lfv", 0x87f0d152f27af928),
    ("env_b_burst_ad", 0x85aa6fdb690493dd),
    ("env_b_burst_ahr", 0xf18a55e8ad5fed5a),
    ("env_b_burst_cycle", 0x45af240dacae29a6),
    ("spd", 0xc4f026243b3d0db5),
    ("lfo_classic", 0x4e9744d2d0d414fd),
    ("lfo_func", 0x125c4f66cf905b5b),
    ("amp_vel_dimmed", 0x906c5da68f92294b),
    ("amp_vel_live", 0xe969b1ab3a3e37fb),
    ("algo_pitch", 0x90d2841bef4b43a6),
    ("modal_pitch", 0xecd2cbfd3f40c2e6),
    ("modal_amp", 0xad2feb025c2dfa59),
    ("mixer_part", 0x87ab4a2a75c3d238),
    ("mixer_sends", 0x650322c12ad0b389),
    ("mixer_fx_delay", 0xd13240ab6a9b4ead),
    ("mixer_fx_reverb", 0x9e3a0671fa320bdf),
    ("mixer_fx_delay_char", 0xbb2132d7ceb9c620),
    ("mixer_tape", 0x28a0e7569c2009be),
    ("mixer_master", 0xc1981d439df14ddd),
    ("mixer_master_level", 0x7554d112cc792b98),
    ("mod_matrix", 0x1ac795629104192b),
    ("mod_matrix_wide", 0x4605f59b8c26e5b8),
    ("sound_browser", 0xaf0c38105a4aee59),
    ("system", 0xbadd5e55da36c80d),
    ("system_theme", 0x6f46ed28ddb10559),
    ("system_audio", 0x281985b580ac2fb1),
    ("busy", 0x198544cd36863145),
    ("toast_saved", 0xe344dd99a47d0dad),
    ("toast_exfat", 0xfc0cb3b4feb1a1b5),
];

#[test]
fn screen_goldens_match() {
    let got: Vec<_> = case_names()
        .map(|name| (name, render(name).hash()))
        .collect();
    common::golden::check(GOLDENS, &got);
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

/// BUSY and every toast draw only inside the band they report,
/// which the shell flushes, and draw something in every row of it.
#[test]
fn busy_draws_only_its_band() {
    use chimera_core::storage::FileError;
    use chimera_hal::store::{StoreError, Unsupported};
    let messages = [
        StoreError::NoCard,
        StoreError::Unsupported(Unsupported::Exfat),
        StoreError::Unsupported(Unsupported::NoPartitionTable),
        StoreError::Unsupported(Unsupported::NotFat(0)),
        StoreError::Unsupported(Unsupported::BadBootSector),
        StoreError::Unsupported(Unsupported::FatNotMirrored),
        StoreError::NotFound,
        StoreError::Full,
        StoreError::Timeout,
        StoreError::Corrupt,
        StoreError::Io,
    ]
    .map(StoreError::message)
    .into_iter()
    .chain(
        [
            FileError::Truncated,
            FileError::BadMagic,
            FileError::BadCrc,
            FileError::NeedsNewerFirmware,
            FileError::WrongKind,
            FileError::Bounds,
            FileError::BadName,
            FileError::Corrupt,
        ]
        .map(FileError::message),
    );
    let overlays = [Overlay::Busy].into_iter().chain(
        core::iter::once("SAVED")
            .chain(messages)
            .map(Overlay::Toast),
    );
    for label in overlays {
        let (fb, (y0, y1)) = render_overlay(label);
        assert_eq!(fb.oob, 0, "{label:?} draws off screen");
        assert!(y0 < y1 && y1 as usize <= H, "{label:?}: {y0}..{y1}");
        for y in 0..H {
            let row = &fb.px[y * W..(y + 1) * W];
            let inside = (y0 as usize..y1 as usize).contains(&y);
            assert_eq!(row.iter().any(|&p| p != 0), inside, "{label:?} row {y}");
        }
    }
}
