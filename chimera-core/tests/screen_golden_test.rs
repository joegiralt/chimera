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
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17).
    ("mod_matrix_morph_inert", 0xc540d682384b1445),
    // Re-recorded: WAVE shows its scope (#310).
    ("algo_wave", 0xe156f3e83d0aab72),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_level", 0x9ac776e29fea212e),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_osc_last", 0x7ea039d2daabda4a),
    // The ENV stages' band draws the focused operator's envelope (#311).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_ar", 0x1c0c610d213d6bb9),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d1l", 0x54e66c604bf0cf69),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d2r", 0xfecabc26ac35f902),
    // Re-recorded: MODE reads LP4, CLEAN's label for LP24 (ADR 0096); then
    // TYPE live with two kinds (Task 8); then CLEAN's Cascadia words FREQ,
    // Q, FM1, FM2 (Task 12): was 0x032b329ca06df69b.
    ("bigviz_filter", 0x5a832bd651e51efe),
    // Re-recorded: MODE reads HP4, CLEAN's label for HP24 (ADR 0096); then
    // LEVEL 64 in DRIVE's cell (Task 6); then FLT › MOD's six cells, FM3
    // turned, its delete hint showing (Task 12): was 0xf6e7d94448ca7888.
    ("flt_mode", 0xc9a4e03001119a94),
    // Each type's FLT and FLT › MOD (filter-types § 5, Task 17): CLEAN's
    // NOTCH1, the 2040's lowered pass band and KEYBD HALF, its fixed MODE
    // dimmed and VOICE EXP, DIRTY's NOTCH1 at FREQ (its own MODE names).
    ("flt_clean", 0x48ad43f82d53c3b8),
    ("flt_mod_clean", 0xe4409e57eb8c885c),
    ("flt_2040", 0x45bf4079e38e3316),
    ("flt_mod_2040", 0x168df3b9ea844b69),
    ("flt_mod_2040_exp", 0x5fef62806549d50e),
    ("flt_dirty", 0x2fd9a7df881017c9),
    ("flt_mod_dirty", 0x8069a71da376c64a),
    ("flt_ladder", 0x533bf12f36283be5),
    ("flt_mod_ladder", 0xf09e62080b1742f2),
    // The LEVEL LED at 0 (dark), ½ and 1: beside LEVEL, beside the 2040's
    // DRIVE and in FLT's viz corner (Task 17).
    ("flt_led_0", 0x0536d5a167d2ceaa),
    ("flt_led_half", 0xe2cfe2de7f8c1e37),
    ("flt_led_full", 0x458fb02687f0911b),
    // FM3 IN on LFO 2, QM IN on OP 6 (Task 17).
    ("flt_mod_repatched", 0xd0193d60eed2fa02),
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
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_pitch", 0x58e8a368827c4a5f),
    // Re-recorded: the EXC node (plan Task 13).
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("modal_pitch", 0xd982f36a8dd7adff),
    // PIT on TAPE, SNAP MUSICAL: PITCH +19 in three clicks (TAPE Task 12b);
    // again for Task 15: PIT hangs under DECK.
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("tape_pitch_musical", 0x9d824579d7e1a553),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc", 0xdf9684baab508f23),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bank", 0x9afb273b79c982eb),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bowed", 0xb182a2665edcbaf0),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home", 0x714dd827ea1a279d),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_mdl2_symp", 0xa21e22e85bcd12b8),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home_bowed", 0x997c6d6606b0dd7c),
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
    // TYPE first, the panel's units, and REV › MORE (reverb TYPEs T6);
    // the REV pages re-recorded for RATE's default and law (ADR 0121).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb", 0x511aff474332e012),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_more", 0x75d670f5545caa06),
    // HALL (its SIZE cell PLATE at SIZE 0) and GATE, each page (T10);
    // MORE re-recorded: the SYNC cell, and GATE's synced LENGTH (T12).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall", 0xfc941630851b83be),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall_more", 0x294f0d43d2e8e2bf),
    // HALL re-recorded as fitted (T10b): its knobs, SPREAD, MORE 2's DENSITY.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall_more2", 0x17b7da341482dcdf),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate", 0x20ce834959d730b0),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate_more", 0x286d4c6b82bbf6ed),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate_sync", 0x8a41594bb680ee68),
    // BLOOM, each page (T11).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_bloom", 0x508fea4a2cbf10ed),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_bloom_more", 0xb8c9a258c854705b),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_char", 0xc6fd6e6be7eb22e8),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master", 0x56a92de14c3c7d2f),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master_level", 0xe699d6651832774d),
    #[cfg(feature = "master-tape")]
    ("mixer_part", 0x91b06a3780c2bbb5),
    #[cfg(feature = "master-tape")]
    ("mixer_sends", 0xcf97b892a7d5a933),
    // DLY's branch hides REV's label it ran into (TAPE Task 15).
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay", 0x6bfad50017562c25),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb", 0x4a284e675148ec1d),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_more", 0x5112307c2ebf2d6d),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall", 0xa14653a47fb15291),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall_more", 0x009f09dbbd885180),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall_more2", 0xd28a6bad16b350c0),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate", 0x588ed520be572fdb),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate_more", 0xc5297468f739990e),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate_sync", 0x189823bdfed79d53),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_bloom", 0x9aa85a2e46eeed12),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_bloom_more", 0xc730d8e88bc28db4),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_char", 0x8efda1c817189d8f),
    #[cfg(feature = "master-tape")]
    ("mixer_tape", 0xe4642868fa275a5c),
    #[cfg(feature = "master-tape")]
    ("mixer_master", 0x3a5be1b8bfc87215),
    #[cfg(feature = "master-tape")]
    ("mixer_master_level", 0xfa809f321d216a97),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_out_p2", 0xc657c44494c829ac),
    #[cfg(feature = "master-tape")]
    ("mixer_out_p2", 0x23ca28848fded48a),
    ("algo_out_p3", 0x3e91d6b588f4ebfa),
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17).
    ("mod_matrix", 0x7fac5633d604d50c),
    ("mod_matrix_wide", 0xeeb753551d0d9d1c),
    // OP5 → FREQ, the cursor on OP5's X cell and its hint; MODAL's eight
    // rows of 17 (Task 17).
    ("mod_matrix_algo_taps", 0x5a2a634ad30444b4),
    ("mod_matrix_modal", 0xff6ac1e21a0ccc52),
    // Re-recorded: the pool meter, row costs, the hints' bytes (sound-pool
    // plan Task 6); and its refusal. Then the merge with filter-types: a
    // Sound is smaller (fixed columns, no registry entry).
    ("sound_browser", 0x7e5c9171f094822b),
    // pool_full, library_sounds/this_build/newer re-recorded: MAX_SOUND_COST 2288 → 2352 (VA).
    ("sound_browser_pool_full", 0x83fc082530a2c28c),
    // The browser's title is its tabs and the Part (library plan Task 7); then
    // the LIBRARY tab: each Sound's build, its pages, what won't fit.
    ("library_empty", 0xa94e4a6edf448c09),
    ("library_no_card", 0xa7957021c5215364),
    ("library_sounds", 0x6d5c9ca6aae65005),
    ("library_this_build", 0xe5de1af86823f93a),
    ("library_cut_short", 0xd797a54b3cebf09d),
    ("library_older", 0xddc7ed0f09a1592b),
    ("library_newer", 0x75c4fb27df07827c),
    ("library_wont_fit", 0x46253d45450aded1),
    ("library_pool_full", 0x6660706bd8a9d80c),
    ("library_naming", 0xab3925136bf27a0f),
    // Re-recorded: a fresh project's footer drops NEW (#286).
    // Re-recorded: INVERT in cell F (ADR 0126).
    ("settings_personal_theme", 0x69eb789acb0d98db),
    ("settings_midi_sync", 0x0e26ce69d0f54980),
    ("settings_system_diag_aud_load", 0x0063331a51f3da0e),
    // Re-recorded: NEW holds TAPE's INIT too (TAPE Task 12), each Sound
    // sparser with the fixed columns, then VA's INIT: `12 · 2.2K`, 157K free.
    ("settings_project_pool", 0xc96d14eba15c42a4),
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
    // Re-recorded: a stepped param shows no bar (ADR 0112).
    ("orbit_rhythm", 0x6a2750f9c7181a45),
    // PLAY's cell (plan Task 14); KEY, SCALE and BARS gone to ORB; SEGS,
    // DEGREE and PLAY on A to C, D to F reserved (ADR 0079, 2026-10-03).
    ("orbit_chrd", 0x9e90b371b6b38670),
    // Chords played into segments: a name for a numeral (Task 14).
    ("orbit_chrd_played", 0x7739f4cbbc895e11),
    // ORB (plan Task 10); B to E gain KEY, SCALE, BARS, SCENE (ADR 0079),
    // and CUSTOM hangs under it; each ring's Part by its top and in RING,
    // and CUSTOM by EDIT on a named scale (ADR 0084).
    // Re-recorded: a stepped param shows no bar (ADR 0112).
    ("orbit_orb", 0xdd08d634fc91934e),
    ("orbit_orb_midi", 0x35a2ff6ecca97ad1),
    ("orbit_custom", 0x7c5bdf51a79d88a1),
    ("orbit_custom_named", 0x6f9acf1a43362d0e),
    // RING's VOICE and NOTES, a STEP ring's NOTES (plan Task 13).
    // Re-recorded: a stepped param shows no bar (ADR 0112).
    ("orbit_voice", 0x97d0371ef20d5b8c),
    ("orbit_notes", 0x2865bfd4ee762860),
    ("orbit_notes_step", 0xc1c3dab7eeb50a2f),
    // 32 steps, step 32 picked: the row scrolled to its last eight.
    ("orbit_notes_step32", 0x211fc000f7a5be91),
    // SCN: a saved slot, SAVE's, a queued one filling (plan Task 16).
    ("orbit_scn", 0x7c2558bf895b7432),
    // TAPE's pages (TAPE Task 15): SRC, and DECK and the model page per machine.
    ("tape_src", 0xd81e5d7ee47a12b9),
    ("tape_src_missing", 0xe32642060f0d88ed),
    ("tape_deck_oneshot", 0x031eabc66a261f19),
    ("tape_page_oneshot", 0x6ec0cc6ab200711a),
    ("tape_deck_loop", 0x0c30beebb549d224),
    ("tape_page_loop", 0x5a71496c59e24e28),
    ("tape_deck_tron", 0xf32256eda828b21d),
    ("tape_page_tron", 0xd146bec467e2e194),
    ("tape_deck_stretch", 0xa0d4b2318cae937c),
    ("tape_page_stretch", 0x213f0019718af53c),
    ("tape_page_stretch_type", 0x1f4bf7ad5f8d6284),
    ("tape_deck_transwave", 0xcdfbaaf69852cdac),
    ("tape_page_transwave", 0x902f5e3c0b411f96),
    ("tape_deck_scrub", 0x6535fb6019ab3668),
    ("tape_page_scrub", 0xac348e679cc0052a),
    ("tape_deck_pingpong", 0xf073fb50618b87fd),
    ("tape_page_pingpong", 0x5105f91c1e3b39ef),
    ("tape_strip_oneshot", 0x3376ab51dd68682c),
    ("tape_strip_loop", 0x0915ec394b829b05),
    ("tape_strip_tron", 0x3de2412e568ca86e),
    ("tape_strip_stretch", 0x44390590e7c63db2),
    ("tape_strip_transwave", 0x156a05b0e3b87314),
    ("tape_strip_scrub", 0x2b0fac2d22995c8a),
    ("tape_strip_pingpong", 0xb01ceb8fa610f288),
    ("tape_strip_loop_reversed", 0x2678af02cbcac4de),
    ("tape_strip_oneshot_slice", 0xc4ed7cd8d85a0aaf),
    ("tape_strip_oneshot_slice_reversed", 0x793c4ddc11fdfb96),
    ("tape_strip_pingpong_reversed", 0x78c6bdd24e2b2eb8),
    ("tape_strip_tron_idle", 0x1e78eb2f30e3b43d),
    ("tape_src_pool", 0x5cc2849e855b0b0e),
    ("tape_src_card", 0xfdc516336deb965f),
    ("tape_src_wont_fit", 0x33e55fa0fbeb043d),
    ("tape_src_loaded", 0x6e6533e2a42a05d2),
    ("tape_src_read_failed", 0x60b02af140ccd019),
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

/// The LED's fill holds the peak, then falls to 0 over 150 ms, never
/// rising without a new peak (filter-types spec § 2, H1).
#[test]
fn clip_hold_holds_then_falls_in_150_ms() {
    use chimera_core::ui::ClipHold;
    let mut h = ClipHold::default();
    assert!(!h.lit());
    assert_eq!(h.step(1.0, 0), 1.0);
    let mut last = 1.0;
    for ms in [50, 100, 150] {
        let v = h.step(0.0, 50);
        assert!(v < last, "{ms} ms: {v} after {last}");
        if ms == 50 {
            assert_eq!(h.step(0.2, 0), v, "a smaller clip doesn't cut the fall");
        }
        last = v;
    }
    assert_eq!(last, 0.0);
    assert!(!h.lit(), "dark once the hold has ended");
    assert_eq!(h.step(0.5, 16), 0.5, "a new peak lights it again");
    assert!(h.lit());
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
