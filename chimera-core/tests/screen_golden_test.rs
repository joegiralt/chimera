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
// amp_vel_*, modal_amp, mixer_sends, mixer_out_p2 again: send and SYM hints.
// SETTINGS crumbs again for ORBIT (plan Task 10): they always clear its
// beat dot, so three paths drop their first part.
// The pool, library and TAPE-source screens again for CHOIR, TALK BASS,
// BREATH and VOX in the bank: the pool bar counts their bytes.
const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0x38036cf069b6f8d7),
    ("algo_alg", 0xc43f8fe941a69fd6),
    ("algo_alg_morph_dimmed", 0x37c1f515da8aa9cc),
    // VOWEL takes ALGO's sixth cell, dimmed with no formant; FRM page.
    // Re-recorded: a value's unit sits small beside its number (ADR 0146).
    ("algo_formant_page", 0x5d207f32f5389b52),
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17).
    ("mod_matrix_morph_inert", 0x27f70ba2fbed39e1),
    // Re-recorded: WAVE shows its scope (#310).
    ("algo_wave", 0x9723d55078a3689a),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_level", 0xb2b96ddc40e39b1e),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_osc_last", 0xabb43bb349537702),
    // New: OP3's page (ADR 0130), the band's envelope and wave, and with
    // its LEVEL cell turned.
    ("algo_op_page_3", 0x20aeeb19bb668188),
    ("algo_op_page_frmt", 0xcbbaf0b5c7c17182),
    ("algo_op_page_nois", 0x93f2a5e3f7e666de),
    ("algo_op_page_3_turned", 0x3c17c0c5d8a07999),
    // The ENV stages' band draws the focused operator's envelope (#311).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_ar", 0x02d92037a4a5fea1),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d1l", 0xcc76371b50536bf9),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d2r", 0x83d50dc60c7a4512),
    // Re-recorded: MODE reads LP4, CLEAN's label for LP24 (ADR 0096); then
    // TYPE live with two kinds (Task 8); then CLEAN's Cascadia words FREQ,
    // Q, FM1, FM2 (Task 12): was 0x032b329ca06df69b.
    ("bigviz_filter", 0x5a832bd651e51efe),
    // Re-recorded: MODE reads HP4, CLEAN's label for HP24 (ADR 0096); then
    // LEVEL 64 in DRIVE's cell (Task 6); then FLT › MOD's six cells, FM3
    // turned, its delete hint showing (Task 12): was 0xf6e7d94448ca7888.
    ("flt_mode", 0xbc50e415f6274421),
    // Each type's FLT and FLT › MOD (filter-types § 5, Task 17): CLEAN's
    // NOTCH1, the 2040's lowered pass band and KEYBD HALF, its fixed MODE
    // dimmed and VOICE EXP, DIRTY's NOTCH1 at FREQ (its own MODE names).
    ("flt_clean", 0x48ad43f82d53c3b8),
    ("flt_mod_clean", 0xa8e1c2139dc9d441),
    ("flt_2040", 0x45bf4079e38e3316),
    ("flt_mod_2040", 0x739059e65565d859),
    ("flt_mod_2040_exp", 0xa75e45413d0f33fe),
    ("flt_dirty", 0x2fd9a7df881017c9),
    ("flt_mod_dirty", 0x52a872af1ad9e63a),
    ("flt_ladder", 0x533bf12f36283be5),
    ("flt_mod_ladder", 0x804291fcb121eca2),
    // The LEVEL LED at 0 (dark), ½ and 1: beside LEVEL, beside the 2040's
    // DRIVE and in FLT's viz corner (Task 17).
    ("flt_led_0", 0x5e725d1446b222da),
    ("flt_led_half", 0xdb0b393eaefa9987),
    ("flt_led_full", 0x458fb02687f0911b),
    // FM3 IN on LFO 2, QM IN on OP 6 (Task 17).
    ("flt_mod_repatched", 0xd0193d60eed2fa02),
    // THEME's PAGES ROW (ADR 0145): every other screen is recorded under
    // STACK (`screen::ui_for`). FLT's two pages, RES's three, MOD at E2
    // with `>`, OSC at D1R with both arrows and at OP6 with `<`, DRV bare.
    // Re-recorded: the row's pages in full words (`ROW_WORDS`).
    ("row_flt", 0xafb54bf210453ad0),
    ("row_res", 0x92503689734f4ddc),
    ("row_mod", 0x1db005ea4ea974e2),
    ("row_osc_mid", 0x9b2c7a9fd24e3120),
    ("row_osc_last", 0x1b1f078c8285ea81),
    ("row_drv", 0xa319372fd505ebc9),
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
    ("lfo_classic", 0x2e4f722acedab974),
    ("lfo_func", 0x327f92aad53cf5c0),
    // Re-recorded: AMP is the sends over FOLD, VEL, MIX, SYM one page down (sends Task 5);
    // mod_matrix_wide primes FOLD and MIX on their new cells.
    ("amp_vel_dimmed", 0xe4f7cd9ac611053b),
    ("amp_vel_live", 0xf3ce4cc3246f4673),
    ("amp_sym", 0x68be9fd83dc5b011),
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_pitch", 0x187108b52e8535b7),
    // Re-recorded: the EXC node (plan Task 13).
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("modal_pitch", 0x6b8da28409bba9d8),
    // PIT on TAPE, SNAP MUSICAL: PITCH +19 in three clicks (TAPE Task 12b);
    // again for Task 15: PIT hangs under DECK.
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("tape_pitch_musical", 0x04460b03eba3c630),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc", 0x3dcb3e058d83de73),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bank", 0x278ee292b0a1419b),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bowed", 0xe84c9d6fc072fc00),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home", 0x47bb2efee5fd120d),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_mdl2_symp", 0x47480f0b352e4aa8),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home_bowed", 0xce64178eb0c0670c),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_amp", 0x44e231e2a0f622db),
    // The Mix chain's map loses its TAPE node without `master-tape`
    // (ADR 0055); with it, the screens are as before.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_part", 0x44c46534e226f795),
    // mixer_sends, mixer_out_p2 re-recorded: the sends are the Sound's OUT block (ADR 0123).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_sends", 0xe04acfd7821d85d9),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay", 0x06d33e7115588a5f),
    // TYPE first, the panel's units, and REV › MORE (reverb TYPEs T6);
    // the REV pages re-recorded for RATE's default and law (ADR 0121).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb", 0xd7557d426c214e95),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_more", 0xa08f460297c7feb5),
    // HALL (its SIZE cell PLATE at SIZE 0) and GATE, each page (T10);
    // MORE re-recorded: the SYNC cell, and GATE's synced LENGTH (T12).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall", 0x78d84195c05b8343),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall_more", 0xface8515437b5d36),
    // HALL re-recorded as fitted (T10b): its knobs, SPREAD, MORE 2's DENSITY.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall_more2", 0x4ef7260ee50491b6),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate", 0xe8f501ae0b0c4710),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate_more", 0x52bb45fe10882418),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate_sync", 0x38fedd73166e5621),
    // BLOOM, each page (T11).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_bloom", 0x7a431159e411a474),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_bloom_more", 0x95e21ecdcac11a0e),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_char", 0xd85a5c6e74ff99b1),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master", 0x56a92de14c3c7d2f),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master_level", 0x6fc81b19f9527540),
    #[cfg(feature = "master-tape")]
    ("mixer_part", 0x6c62b07122b423b6),
    #[cfg(feature = "master-tape")]
    ("mixer_sends", 0x5a0a0f6caea9f67b),
    // DLY's branch hides REV's label it ran into (TAPE Task 15).
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay", 0x364b8608736dd631),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb", 0xec89096986df2f47),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_more", 0xf5795852136e2c13),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall", 0x2d7e131c9a01535d),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall_more", 0x009f09dbbd885180),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall_more2", 0xd28a6bad16b350c0),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate", 0x0908875e4470c3a7),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate_more", 0xc5297468f739990e),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate_sync", 0x1ae7fa73664a157f),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_bloom", 0x4653d819fbea6cd6),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_bloom_more", 0xc730d8e88bc28db4),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_char", 0xf907b08d9f6706ab),
    #[cfg(feature = "master-tape")]
    ("mixer_tape", 0xe4642868fa275a5c),
    #[cfg(feature = "master-tape")]
    ("mixer_master", 0x3a5be1b8bfc87215),
    #[cfg(feature = "master-tape")]
    ("mixer_master_level", 0xfa809f321d216a97),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_out_p2", 0xc7fad296ace82da3),
    #[cfg(feature = "master-tape")]
    ("mixer_out_p2", 0x97c6eb559ffe4f95),
    ("algo_out_p3", 0x0fbb7e2baf40587a),
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17).
    ("mod_matrix", 0x7fac5633d604d50c),
    ("mod_matrix_wide", 0x2d53cf82f2c6856c),
    // OP5 → FREQ, the cursor on OP5's X cell and its hint; MODAL's eight
    // rows of 17 (Task 17).
    ("mod_matrix_algo_taps", 0x96eee5ae9fa3f7bc),
    ("mod_matrix_modal", 0x1fb996901f31f57a),
    // Re-recorded: the pool meter, row costs, the hints' bytes (sound-pool
    // plan Task 6); and its refusal. Then the merge with filter-types: a
    // Sound is smaller (fixed columns, no registry entry).
    // Then, with the library_* screens: FACTORY joins the title and B ◂▸
    // moves to hint line 1 (showcase Task 4).
    ("sound_browser", 0xe6ebc09a421a5c11),
    // pool_full, library_sounds/this_build/newer re-recorded: MAX_SOUND_COST 2288 → 2352 (VA).
    // Then pool_full: 2352 → 2544 (the formant fields).
    ("sound_browser_pool_full", 0x4327d7766a8f1115),
    // The browser's title is its tabs and the Part (library plan Task 7); then
    // the LIBRARY tab: each Sound's build, its pages, what won't fit.
    // Merged: FACTORY in the title (showcase) beside the formant cost.
    ("library_empty", 0xaf1407579ab08624),
    ("library_no_card", 0x386e6c2c8027ccd9),
    ("library_sounds", 0x8964371b11796e5f),
    ("library_this_build", 0x0719763e23b44930),
    ("library_cut_short", 0x8b4bddae0c9784ad),
    ("library_older", 0x004f2248949f5c1f),
    ("library_newer", 0x54415a7d9a958936),
    ("library_wont_fit", 0x24494e0f831f3bab),
    ("library_pool_full", 0x950a8303f1cef0f3),
    ("library_naming", 0xab3925136bf27a0f),
    // The FACTORY tab: a row's description on line 2, the set's build on
    // line 3; empty, no card, and a stray TAPE file greyed COMING SOON.
    // Re-recorded on the merge: the pool meter at the formant cost.
    ("factory_described", 0xde88e442deb01c5b),
    ("factory_empty", 0x8b3ffd0a81b93e56),
    ("factory_no_card", 0xeb75c38b1f28726b),
    ("factory_coming_soon", 0xe9464e21b7ec8f87),
    // Re-recorded: a fresh project's footer drops NEW (#286).
    // Re-recorded: INVERT in cell F (ADR 0126).
    ("settings_personal_theme", 0x69eb789acb0d98db),
    // PERSONALIZE › CHAIN MAP: PAGES, ROW (ADR 0145).
    ("settings_personal_chain_map", 0x7aa40f30346b46bd),
    ("settings_midi_sync", 0x0e26ce69d0f54980),
    ("settings_system_diag_aud_load", 0x18fe4d081416e4cb),
    // Re-recorded: NEW holds TAPE's INIT too (TAPE Task 12), each Sound
    // sparser with the fixed columns, then VA's INIT: `12 · 2.2K`, 157K free.
    ("settings_project_pool", 0x6ceb6837fe4ebecc),
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
    ("settings_naming", 0xc4298c7a6a0c5b5a),
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
    ("tape_deck_oneshot", 0xcee951d363a9b989),
    ("tape_page_oneshot", 0xdaa2dad45cc62c6a),
    ("tape_deck_loop", 0x4d56090584f310d4),
    // New: bank 2 closed (ADR 0141).
    ("tape_deck_needs_update", 0xbfc70954a2cccdd9),
    ("tape_page_loop", 0x933afc3a391854d0),
    ("tape_deck_tron", 0xa947b940b896b56d),
    ("tape_page_tron", 0xb3d7bdd0216e81dc),
    ("tape_deck_stretch", 0x06017f5c2324e5cc),
    ("tape_page_stretch", 0x0c2ddf6b8c92368c),
    ("tape_page_stretch_type", 0x1f4bf7ad5f8d6284),
    ("tape_deck_transwave", 0xe1770391001229fc),
    ("tape_page_transwave", 0x7b54386ad187711e),
    ("tape_deck_scrub", 0x65f92500d05b3978),
    ("tape_page_scrub", 0x39e1050f181e2382),
    ("tape_deck_pingpong", 0xa39b9dfd52dfccad),
    ("tape_page_pingpong", 0x771c4bbcff31fba7),
    ("tape_strip_oneshot", 0x06d6e8b1b66118dc),
    ("tape_strip_loop", 0x8e12e585f858c935),
    ("tape_strip_tron", 0x2778d486af638a1e),
    ("tape_strip_stretch", 0x13f85efe6b4d6502),
    ("tape_strip_transwave", 0x5878846563723ee4),
    ("tape_strip_scrub", 0x5da8ccf2a13a947a),
    ("tape_strip_pingpong", 0x156517d3d543f438),
    ("tape_strip_loop_reversed", 0x70f697c0ecbfed6e),
    ("tape_strip_oneshot_slice", 0xcb31dbda53626b7f),
    ("tape_strip_oneshot_slice_reversed", 0xa74f813de1fcc786),
    ("tape_strip_pingpong_reversed", 0x7bb33060d124e1c8),
    ("tape_strip_tron_idle", 0x2e9a7c033cdd056d),
    ("tape_src_pool", 0x5cc2849e855b0b0e),
    ("tape_src_card", 0x2851ad3cea608bdf),
    ("tape_src_wont_fit", 0x7a0855b2f56038bd),
    ("tape_src_loaded", 0x6e6533e2a42a05d2),
    ("tape_src_read_failed", 0x60b02af140ccd019),
    ("busy", 0x198544cd36863145),
    ("toast_saved", 0xe344dd99a47d0dad),
    ("toast_exfat", 0xfc0cb3b4feb1a1b5),
    // Bank 2's install: DON'T POWER OFF over the whole screen.
    ("installing", 0xb2af55fe85e8e58d),
    // Its phase line at WRITING.
    ("installing_writing", 0x7f68e79694a6310d),
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
