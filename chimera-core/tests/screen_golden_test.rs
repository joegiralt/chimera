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

// Re-recorded for ADR 0057: every Part and mixer header reads
// `PART n · SOUND` or `· MIX`; the mixer recipes open on SENDS. The Algo
// screens again for ADR 0063: INIT's VOL 45 and RR 5.
const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0x3c1db80e3e80b53f),
    ("algo_alg", 0xbffad8304f828aee),
    ("algo_alg_morph_dimmed", 0xd8e8a78a528ad3b4),
    ("mod_matrix_morph_inert", 0xc0a153ae29f9167b),
    ("algo_wave", 0xdbd599e8964b6a89),
    ("algo_level", 0x7b3a4495d5c597e0),
    ("algo_osc_last", 0x6112961e4f96c7ab),
    ("bigviz_filter", 0x64de3de945e82cf3),
    ("flt_mode", 0xf5635604c002fec2),
    ("env_a", 0x6f15c442d71edf83),
    ("env_b_env_ad", 0x849b12e99ea94bfd),
    ("env_b_env_ahr", 0x25bd96be29456024),
    ("env_b_env_cycle", 0x731397722d8939e6),
    ("env_b_lfo_free", 0x393aca1db211bd20),
    ("env_b_lfo_sync", 0xe99f8846653ec2a8),
    ("env_b_lfo_lfv", 0x0eae7ee6c8214c21),
    ("env_b_burst_ad", 0x051b9e98ae0429b4),
    ("env_b_burst_ahr", 0x8af61048dba1b1f7),
    ("env_b_burst_cycle", 0x4506fd5dfe4a8d5b),
    ("spd", 0x96c5fde07700d7dc),
    ("lfo_classic", 0x3136c2e044ecb304),
    ("lfo_func", 0x9fbaed70f0baac26),
    ("amp_vel_dimmed", 0x496ba84d34eb9c06),
    ("amp_vel_live", 0x07fe7ee2b28245d6),
    // Re-recorded: PIT shows STEAL and TIME (#254).
    ("algo_pitch", 0x245c47fc88561cef),
    // Re-recorded: the EXC node (plan Task 13).
    // Re-recorded: PIT shows STEAL and TIME (#254).
    ("modal_pitch", 0x68e6cf758f76373f),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc", 0xdf9684baab508f23),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bank", 0x9afb273b79c982eb),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bowed", 0xb182a2665edcbaf0),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home", 0x6f33887c0e8894d5),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_mdl2_symp", 0xa21e22e85bcd12b8),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home_bowed", 0x1b4c79d6f245b6a9),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_amp", 0x126f578eb867adcf),
    // The Mix chain's map loses its TAPE node without `master-tape`
    // (ADR 0055); with it, the screens are as before.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_part", 0xa692924186da849a),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_sends", 0x26c03d61d4dffe1b),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay", 0x7cad11c8821d60af),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb", 0x114c421e48eb2959),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_char", 0xee4a9257032bb195),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master", 0xeb7a9f3352f2416f),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master_level", 0xb898d28bbb71bce6),
    #[cfg(feature = "master-tape")]
    ("mixer_part", 0xc25c1c60d1ccf57c),
    #[cfg(feature = "master-tape")]
    ("mixer_sends", 0xd9c254b0347d110d),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay", 0x40a01377e2ee0701),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb", 0xb6abe4aa3463b5ab),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_char", 0x2acb22b0d9a133df),
    #[cfg(feature = "master-tape")]
    ("mixer_tape", 0x2548917571779052),
    #[cfg(feature = "master-tape")]
    ("mixer_master", 0x4e68a2befa356ab1),
    #[cfg(feature = "master-tape")]
    ("mixer_master_level", 0x1fea2dbc893512ec),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_out_p2", 0x922c1706c7a6113f),
    #[cfg(feature = "master-tape")]
    ("mixer_out_p2", 0x72f5aa7f2c775a91),
    ("algo_out_p3", 0xe8b5d1afc600d5f6),
    ("mod_matrix", 0xa6cf67e584e06c56),
    ("mod_matrix_wide", 0xca78c74fca4ac959),
    ("sound_browser", 0xc1a53459edabbb6b),
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
