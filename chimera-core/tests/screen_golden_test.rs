//! Screen goldens (UI refresh spec § Testing): one FNV-1a hash per screen.
//!
//! Every case must match bit-for-bit. Re-record ONLY for a change that is
//! meant to alter a screen, in the commit that makes it (`common::golden`).
//! To look at the screens:
//!
//!     SCREEN_DUMP=/tmp/screens cargo test -p chimera-core --test screen_golden_test

// The goldens and the atlas are recorded with ORBIT built: its pages,
// its SETTINGS list and the crumbs left of its beat dot.
#![cfg(feature = "orbit")]

mod common;
mod screen;

use screen::*;

// Re-recorded for ADR 0057: every Part and mixer header reads
// `PART n · SOUND` or `· MIX`; the mixer recipes open on SENDS. The Algo
// screens again for ADR 0063: INIT's VOL 45 and RR 5.
// SETTINGS crumbs again for ORBIT (plan Task 10): they always clear its
// beat dot, so three paths drop their first part.
const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0x29fa1ef579c4a0e7),
    ("algo_alg", 0xb73321ba103c1886),
    ("algo_alg_morph_dimmed", 0xd8e8a78a528ad3b4),
    ("mod_matrix_morph_inert", 0xe5dc545f78f89d13),
    // Re-recorded: WAVE shows its scope (#310).
    ("algo_wave", 0xe156f3e83d0aab72),
    ("algo_level", 0x408f6bef763157fe),
    ("algo_osc_last", 0x7ac731494a9568e2),
    // The ENV stages' band draws the focused operator's envelope (#311).
    ("algo_ar", 0x661a8ad43d30be01),
    ("algo_d1l", 0x7ca2aaf1d50c3055),
    ("algo_d2r", 0x60965afdf2d22552),
    ("bigviz_filter", 0x64de3de945e82cf3),
    ("flt_mode", 0x9dd12a044e75cc68),
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
    ("lfo_func", 0x60ec706c0ecfc450),
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
    ("modal_home", 0x36bc37f1707051b1),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_mdl2_symp", 0xa21e22e85bcd12b8),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home_bowed", 0xc8b8ec05462bb0a8),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_amp", 0x126f578eb867adcf),
    // The Mix chain's map loses its TAPE node without `master-tape`
    // (ADR 0055); with it, the screens are as before.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_part", 0x24ee8f61ddea9047),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_sends", 0xdf3aab7c23ee0c31),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay", 0x57508228791992f2),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb", 0xd831539df40c3730),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_char", 0xc6fd6e6be7eb22e8),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master", 0x56a92de14c3c7d2f),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master_level", 0x46ef06da0f464e86),
    #[cfg(feature = "master-tape")]
    ("mixer_part", 0x91b06a3780c2bbb5),
    #[cfg(feature = "master-tape")]
    ("mixer_sends", 0xcf97b892a7d5a933),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay", 0xde205a6af1b18294),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb", 0xc0311d87f95d9df6),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_char", 0x3b8f3b6dc208a5aa),
    #[cfg(feature = "master-tape")]
    ("mixer_tape", 0xe4642868fa275a5c),
    #[cfg(feature = "master-tape")]
    ("mixer_master", 0x3a5be1b8bfc87215),
    #[cfg(feature = "master-tape")]
    ("mixer_master_level", 0x38d97b4c60a9a7a4),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_out_p2", 0xc657c44494c829ac),
    #[cfg(feature = "master-tape")]
    ("mixer_out_p2", 0x23ca28848fded48a),
    ("algo_out_p3", 0x3e91d6b588f4ebfa),
    ("mod_matrix", 0x908cc3655fdaf8ee),
    ("mod_matrix_wide", 0xc34ca17250bc0e55),
    // Re-recorded: the pool meter, row costs, the hints' bytes (sound-pool
    // plan Task 6); and its refusal.
    ("sound_browser", 0xff8f72f449506956),
    ("sound_browser_pool_full", 0x60971131ef0b2f89),
    // Re-recorded: a fresh project's footer drops NEW (#286).
    ("settings_personal_theme", 0xc35039e65219cdc2),
    ("settings_midi_sync", 0x0e26ce69d0f54980),
    ("settings_system_diag_aud_load", 0x0d5edcdc4826225e),
    // Re-recorded: NEW holds TAPE's INIT too (TAPE Task 12).
    ("settings_project_pool", 0x0398cb5ed06d500b),
    // ORBIT's row opens its list (plan Task 17).
    ("settings_top", 0x08d6ee26dad9fb97),
    // Part 3's LIVE SNAP leaf (plan Task 15).
    ("settings_part_live_snap", 0x78c80cf3c4c1cc6a),
    // SETTINGS › ORBIT and its SAVE TO PROJ (plan Task 17); the crumb
    // `SCENE n` (fix round 1).
    ("settings_orbit", 0xf0d520f40ac7f0bb),
    // Slot 3 written: MODIFIED, as ORBIT is in the project's CRC (Task 18).
    ("settings_orbit_save_to", 0xa4d2405ea9f09bf6),
    ("settings_load", 0x34b0bd576d341497),
    ("settings_prompt_load", 0xc47d45b862dea1e3),
    ("settings_naming", 0x3ad617885a99ddf2),
    // Every ORBIT page again for Task 16: the map gains SCN.
    // ORBIT's RHYTHM, CHRD and CUSTOM (plan Task 9); Task 10's header,
    // `SCENE n`, and the beat dot on the playing ones.
    ("orbit_rhythm", 0x535a82923b7caa35),
    // PLAY's cell (plan Task 14); KEY, SCALE and BARS gone to ORB; SEGS,
    // DEGREE and PLAY on A to C, D to F reserved (ADR 0079, 2026-10-03).
    ("orbit_chrd", 0x9e90b371b6b38670),
    // Chords played into segments: a name for a numeral (Task 14).
    ("orbit_chrd_played", 0x7739f4cbbc895e11),
    // ORB (plan Task 10); B to E gain KEY, SCALE, BARS, SCENE (ADR 0079),
    // and CUSTOM hangs under it; each ring's Part by its top and in RING,
    // and CUSTOM by EDIT on a named scale (ADR 0084).
    ("orbit_orb", 0xddf44d94d5f622ca),
    ("orbit_orb_midi", 0x35a2ff6ecca97ad1),
    ("orbit_custom", 0x7c5bdf51a79d88a1),
    ("orbit_custom_named", 0x6f9acf1a43362d0e),
    // RING's VOICE and NOTES, a STEP ring's NOTES (plan Task 13).
    ("orbit_voice", 0xdaefdb55fbe49e98),
    ("orbit_notes", 0x2865bfd4ee762860),
    ("orbit_notes_step", 0xc1c3dab7eeb50a2f),
    // 32 steps, step 32 picked: the row scrolled to its last eight.
    ("orbit_notes_step32", 0x211fc000f7a5be91),
    // SCN: a saved slot, SAVE's, a queued one filling (plan Task 16).
    ("orbit_scn", 0x7c2558bf895b7432),
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
