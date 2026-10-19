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
// Re-recorded for #562: a Part's SOUND and MIX headers draw six Part dots.
const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0x0d2d9d0ea7e6476d),
    ("algo_alg", 0x2ffaabdfb0b2b2b4),
    ("algo_alg_morph_dimmed", 0xb6bf03cc2e9ce6c6),
    // VOWEL takes ALGO's sixth cell, dimmed with no formant; FRM page.
    // Re-recorded: a value's unit sits small beside its number (ADR 0146).
    ("algo_formant_page", 0xc2798d511cef5b20),
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17);
    // then FOLD's column named FX (#614).
    ("mod_matrix_morph_inert", 0x7be44a5dd2dffa93),
    // Re-recorded: WAVE shows its scope (#310).
    ("algo_wave", 0xe96d1f664d72dff8),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_level", 0xfa7578f99a21d1b8),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_osc_last", 0xb651d8b56742b594),
    // New: OP3's page (ADR 0130), the band's envelope and wave, and with
    // its LEVEL cell turned.
    ("algo_op_page_3", 0x9c9a947da8bf9d5e),
    ("algo_op_page_frmt", 0x09cdcdd1b6c43200),
    ("algo_op_page_nois", 0xc0f32d438825ff1c),
    ("algo_op_page_3_turned", 0x8fe9f2208c87b32b),
    // The ENV stages' band draws the focused operator's envelope (#311).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_ar", 0x2359b3233f7e5c27),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d1l", 0x6be507451cb500ab),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d2r", 0x0145c38f4341953c),
    // Re-recorded: MODE reads LP4, CLEAN's label for LP24 (ADR 0096); then
    // TYPE live with two kinds (Task 8); then CLEAN's Cascadia words FREQ,
    // Q, FM1, FM2 (Task 12): was 0x032b329ca06df69b.
    ("bigviz_filter", 0x7325aeca21330df4),
    // Re-recorded: MODE reads HP4, CLEAN's label for HP24 (ADR 0096); then
    // LEVEL 64 in DRIVE's cell (Task 6); then FLT › MOD's six cells, FM3
    // turned, its delete hint showing (Task 12): was 0xf6e7d94448ca7888.
    ("flt_mode", 0x1fc9f39e9154b753),
    // Each type's FLT and FLT › MOD (filter-types § 5, Task 17): CLEAN's
    // NOTCH1, the 2040's lowered pass band and KEYBD HALF, its fixed MODE
    // dimmed and VOICE EXP, DIRTY's NOTCH1 at FREQ (its own MODE names).
    ("flt_clean", 0x74d92b2f88dcbd21),
    ("flt_mod_clean", 0x4a9001fada6343bb),
    ("flt_2040", 0x6b1f20b1690db849),
    // Modulation draws a ghost beside the set curve (#575).
    ("flt_2040_env_amt", 0x9af3e6b28f4cde78),
    ("flt_clean_lfo", 0xbae65670855e43a3),
    ("flt_mod_2040", 0x4edd80b5ff004393),
    ("flt_mod_2040_exp", 0xd4b4157e07474c0c),
    ("flt_dirty", 0xcc55bba2bcc9f784),
    ("flt_mod_dirty", 0xb610cb3f42782750),
    ("flt_ladder", 0x4b84729279c65395),
    // New (#574): the peak at its real height, 2040 RES 0.3, CLEAN BP2 and
    // LADDER RES 0.
    ("flt_2040_res_30", 0x3e76dac83783d845),
    ("flt_clean_bp2", 0xa15f80c685493b0d),
    ("flt_ladder_res_0", 0x45a414900348c835),
    ("flt_mod_ladder", 0x49035174dbbef6f4),
    // The LEVEL LED at 0 (dark), ½ and 1: beside LEVEL, beside the 2040's
    // DRIVE and in FLT's viz corner (Task 17).
    ("flt_led_0", 0xa736064226f64804),
    ("flt_led_half", 0x8f43921cc7b244c9),
    ("flt_led_full", 0x6738db5f52794808),
    // FM3 IN on LFO 2, QM IN on OP 6 (Task 17).
    ("flt_mod_repatched", 0x2f1a43cd3fb36944),
    // THEME's PAGES ROW (ADR 0145): every other screen is recorded under
    // STACK (`screen::ui_for`). FLT's two pages, RES's three, MOD at E2
    // with `>`, OSC at D1R with both arrows and at OP6 with `<`, DRV bare.
    // Re-recorded: the row's pages in full words (`ROW_WORDS`).
    ("row_flt", 0x1a99f505a09d08cb),
    ("row_res", 0x52e7d782b213fed2),
    ("row_mod", 0x49aae570cf1c06d8),
    ("row_osc_mid", 0xeb1a0363a9c29c8e),
    ("row_osc_last", 0xa5313545b849c1cf),
    ("row_drv", 0x895a37b928501e07),
    ("env_a", 0xdf5e9438fe953789),
    ("env_b_env_ad", 0x32e7eff8f3ef2d73),
    ("env_b_env_ahr", 0x3a6ee459e6aeec8a),
    ("env_b_env_cycle", 0xf1272fca70d90bcc),
    ("env_b_lfo_free", 0x7597f85253ef1f2a),
    ("env_b_lfo_sync", 0x87538ca1edb1786a),
    ("env_b_lfo_lfv", 0x946c4ab1b6a5cf1f),
    ("env_b_burst_ad", 0xb0a76b2e42dd1af2),
    ("env_b_burst_ahr", 0x7ba2ccbb1c1f77a1),
    ("env_b_burst_cycle", 0x5bdb2cb3d202f7e9),
    ("spd", 0x72e39f2d2f94f382),
    ("lfo_classic", 0xf1b5267023ff79aa),
    ("lfo_func", 0xee922d0687f7756e),
    // Re-recorded: AMP is the sends over FOLD, VEL, MIX, SYM one page down (sends Task 5);
    // mod_matrix_wide primes FOLD and MIX on their new cells.
    ("amp_vel_dimmed", 0xbfaae22b7783bedd),
    ("amp_vel_live", 0x2b87901fce638e81),
    ("amp_sym", 0x18f6d3abce3e99df),
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_pitch", 0xcb7f0c04f3dbb229),
    // Re-recorded: the EXC node (plan Task 13).
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("modal_pitch", 0xa1d8e11218a836be),
    // PIT on TAPE, SNAP MUSICAL: PITCH +19 in three clicks (TAPE Task 12b);
    // again for Task 15: PIT hangs under DECK.
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("tape_pitch_musical", 0x769043dfc1afbc4a),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc", 0x1a9d74fd3e32a2a1),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bank", 0xcb81646f4c74b37d),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_exc_bowed", 0x5a8756dd8d827e0a),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home", 0xedcfc7a8ab947a47),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_mdl2_symp", 0x28a9783358085952),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_home_bowed", 0xedeb3ad6cfdbe66e),
    // Re-recorded: the EXC node (plan Task 13).
    ("modal_amp", 0xf8fda68f81ea21f9),
    // The Mix chain's map loses its TAPE node without `master-tape`
    // (ADR 0055); with it they have their own rows, run by `just test`.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_part", 0xfd1a118204f19d77),
    // mixer_sends, mixer_out_p2 re-recorded: the sends are the Sound's OUT block (ADR 0123).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_sends", 0x35e09ae3385236bb),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay", 0xfbe0808a050ed002),
    // TYPE first, the panel's units, and REV › MORE (reverb TYPEs T6);
    // the REV pages re-recorded for RATE's default and law (ADR 0121).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb", 0x9d9d0b61c361768c),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_more", 0x7b25fe8ae4125c8c),
    // HALL (its SIZE cell PLATE at SIZE 0) and GATE, each page (T10);
    // MORE re-recorded: the SYNC cell, and GATE's synced LENGTH (T12).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall", 0x110a3c12bb55b53e),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall_more", 0x733a41e95f057403),
    // HALL re-recorded as fitted (T10b): its knobs, SPREAD, MORE 2's DENSITY.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_hall_more2", 0x8bfbd6ba87c2ac03),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate", 0x6da0632fff3b4421),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate_more", 0x9c5959ebe27922a1),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_gate_sync", 0x382b5c06de0836b8),
    // BLOOM, each page (T11).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_bloom", 0x8bb0cf42db45e7a5),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_reverb_bloom_more", 0x1cadf3eefa17fedb),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_char", 0xb3e9570b26525f00),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master", 0xaa7cc5f71d43600a),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master_level", 0x33a39d11187d9bb1),
    #[cfg(feature = "master-tape")]
    ("mixer_part", 0x429ab992c626b285),
    #[cfg(feature = "master-tape")]
    ("mixer_sends", 0xba83be0d8d177f8d),
    // DLY's branch hides REV's label it ran into (TAPE Task 15).
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay", 0x498e957495d45875),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb", 0xf884d95677bc4c9f),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_more", 0x78bfe9ae1a6f8293),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall", 0x21c8e0e268a28611),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall_more", 0x33e1535b46093a7c),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_hall_more2", 0xd48d23ad4d9a46dc),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate", 0x505d2202f043580e),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate_more", 0xdc0bd1ce1ea6779a),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_gate_sync", 0xcb7bc913fa920663),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_bloom", 0x0d7e76a3ba5578ea),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_reverb_bloom_more", 0xa267930e1d560034),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_char", 0xb47f7ef56c42b2a7),
    #[cfg(feature = "master-tape")]
    ("mixer_tape", 0x43381076b873ad98),
    #[cfg(feature = "master-tape")]
    ("mixer_master", 0x29929673ac6c7c44),
    #[cfg(feature = "master-tape")]
    ("mixer_master_level", 0x041e07e545a86dfb),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_out_p2", 0xbbf4869671d1e56d),
    #[cfg(feature = "master-tape")]
    ("mixer_out_p2", 0x60429572e55d3aaf),
    ("algo_out_p3", 0x6a41c9fba77fd7fc),
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17);
    // then FOLD's column named FX (#614), here and in the next three.
    ("mod_matrix", 0xef455a07fb204322),
    ("mod_matrix_wide", 0xcb96b37ccdad88fe),
    // OP5 → FREQ, the cursor on OP5's X cell and its hint; MODAL's eight
    // rows of 17 (Task 17).
    ("mod_matrix_algo_taps", 0xe159632a9ac1f92e),
    ("mod_matrix_modal", 0xea127c9de06a34ec),
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
    ("library_naming", 0xa234ea336d54efbf),
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
    // Long user names, LARGE's pair (ADR 0154).
    ("sound_browser_long", 0x9f219d2caa40b9bd),
    // PERSONALIZE › TEXT SIZE at NORMAL (ADR 0154).
    ("settings_personal_text_size", 0x771bfc59119e85d2),
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
    ("settings_prompt_load", 0xfb661eb8bbfd7b2b),
    ("settings_naming", 0x39679c2d5eca087a),
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
    // TEXT SIZE LARGE (ADR 0154): each its NORMAL case's screen a size up.
    ("large_engine_algo", 0xe91f2bcb6eb8bf3b),
    ("large_algo_level", 0xf225b94d99567b7c),
    ("large_bigviz_filter", 0xbc513237f6c402d9),
    ("large_modal_amp", 0xbfc8b95f0eb078b8),
    ("large_va_drive", 0x01744038da287ded),
    // The Mix chain's map has a TAPE node with `master-tape` (ADR 0055).
    #[cfg(not(feature = "master-tape"))]
    ("large_mixer_sends", 0x3ea6919e3a4e3772),
    #[cfg(feature = "master-tape")]
    ("large_mixer_sends", 0x66e4fced766b46b8),
    ("large_settings_top", 0x9d2976351278b0df),
    ("large_settings_personal", 0x35b1c3eb794412f7),
    ("large_sound_browser", 0x018933e3d8721858),
    ("large_sound_browser_long", 0xa914e71773ba2c56),
    ("large_library_sounds", 0xc680b63718145a73),
    ("large_settings_text_size", 0xe57ffb4a4502d385),
    // SCN: a saved slot, SAVE's, a queued one filling (plan Task 16).
    ("orbit_scn", 0x7c2558bf895b7432),
    // TAPE's pages (TAPE Task 15): SRC, and DECK and the model page per machine.
    ("tape_src", 0x34010e9905b39be3),
    ("tape_src_missing", 0x2d91b7ddf880e963),
    ("tape_deck_oneshot", 0xb85329adae011ca7),
    ("tape_page_oneshot", 0x561417f56b885448),
    ("tape_deck_loop", 0x39827508f0883e1e),
    // New: bank 2 closed (ADR 0141).
    ("tape_deck_needs_update", 0x41244a50122b6ffb),
    ("tape_page_loop", 0xeb16a2c996ee60e2),
    ("tape_deck_tron", 0x67b08780b2a0253f),
    ("tape_page_tron", 0x9207452429d59f82),
    ("tape_deck_stretch", 0xfdb47bf92dcb5f42),
    ("tape_page_stretch", 0xf14f22169f726452),
    ("tape_page_stretch_type", 0x0f95ee1198eaee56),
    ("tape_deck_transwave", 0xd6f95db652c6a652),
    ("tape_page_transwave", 0xf1705be7ef6aa37c),
    ("tape_deck_scrub", 0x914d037d3335a072),
    ("tape_page_scrub", 0x9405497c2fe85974),
    ("tape_deck_pingpong", 0xec7edf944ce6cfe7),
    ("tape_page_pingpong", 0xd4678741ac553c91),
    ("tape_strip_oneshot", 0xba01ed96d2024696),
    ("tape_strip_loop", 0xd4442fa6a801fd2b),
    ("tape_strip_tron", 0x4234aebaeef2ac7c),
    ("tape_strip_stretch", 0x0163b0fb232ecef4),
    ("tape_strip_transwave", 0xfc70d5c5cde51b42),
    ("tape_strip_scrub", 0xbba3b72c53578624),
    ("tape_strip_pingpong", 0x172eae52caf079be),
    ("tape_strip_loop_reversed", 0xc8840f14239e5b88),
    ("tape_strip_oneshot_slice", 0x007c5087c7599755),
    ("tape_strip_oneshot_slice_reversed", 0x761a61fa5c3afeb4),
    ("tape_strip_pingpong_reversed", 0x28022ee2966d3b02),
    ("tape_strip_tron_idle", 0xc3c3c1d5e34f1563),
    ("tape_src_pool", 0x4075e32c420bd2a4),
    ("tape_src_card", 0x8fec60e2a095f9e9),
    ("tape_src_wont_fit", 0x43d0f49e9e0fb753),
    ("tape_src_loaded", 0xbef4ee2ea3b6b0c0),
    ("tape_src_read_failed", 0x5a5a668cb751c657),
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
