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
// Re-recorded for #614 (FX in the filter, Task 7: layout A), the map
// without DRV, FLT's and AMP's new cells: engine_algo, algo_alg,
// algo_alg_morph_dimmed, algo_formant_page, mod_matrix_morph_inert,
// algo_wave, algo_level, algo_osc_last, algo_op_page_3,
// algo_op_page_frmt, algo_op_page_nois, algo_op_page_3_turned, algo_ar,
// algo_d1l, algo_d2r, bigviz_filter, flt_mode, flt_clean,
// flt_mod_clean, flt_2040, flt_2040_env_amt, flt_clean_lfo,
// flt_mod_2040, flt_dirty, flt_mod_dirty, flt_ladder, flt_mod_ladder,
// flt_led_0, flt_led_half, flt_led_full, flt_mod_repatched, row_flt,
// row_mod, row_osc_mid, row_osc_last, env_a, env_b_env_ad,
// env_b_env_ahr, env_b_env_cycle, env_b_lfo_free, env_b_lfo_sync,
// env_b_lfo_lfv, env_b_burst_ad, env_b_burst_ahr, env_b_burst_cycle,
// spd, lfo_classic, lfo_func, amp_vel_dimmed, amp_vel_live, algo_pitch,
// tape_pitch_musical, modal_amp, algo_out_p3, mod_matrix,
// mod_matrix_wide, mod_matrix_algo_taps, tape_src, tape_src_missing,
// tape_deck_oneshot, tape_page_oneshot, tape_deck_loop,
// tape_deck_needs_update, tape_page_loop, tape_deck_tron,
// tape_page_tron, tape_deck_stretch, tape_page_stretch,
// tape_page_stretch_type, tape_deck_transwave, tape_page_transwave,
// tape_deck_scrub, tape_page_scrub, tape_deck_pingpong,
// tape_page_pingpong, tape_strip_oneshot, tape_strip_loop,
// tape_strip_tron, tape_strip_stretch, tape_strip_transwave,
// tape_strip_scrub, tape_strip_pingpong, tape_strip_loop_reversed,
// tape_strip_oneshot_slice, tape_strip_oneshot_slice_reversed,
// tape_strip_pingpong_reversed, tape_strip_tron_idle, tape_src_pool,
// tape_src_card, tape_src_wont_fit, tape_src_loaded,
// tape_src_read_failed.
// New: flt_2040_kind, flt_2040_exp, flt_fx_off, flt_fx_soft, flt_fx_fold,
// flt_front_fx, row_amp. Gone: row_drv, amp_sym, flt_mod_2040_exp.// Re-recorded in Task 14: the curve viz draws SOFT and FOLD at their
// `MAKE_UP` 0.483 and 0.684 (flt_fx_soft, flt_fx_fold, _soft_pre, _fold_post).
// Re-recorded for the FREQ MOD / RES MOD band naming its source: flt_mode, flt_mod_clean.
// Re-recorded for ADR 0167 (FLT's front reordered: KIND · FX · Q · MODE ·
// FX PLACE · FREQ): every FLT front page, row_flt, large_va_drive.
const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0xf68690304bba80fb),
    ("algo_alg", 0x4b8d653ed15f4a3e),
    ("algo_alg_morph_dimmed", 0x1bbeb26ba408b7e0),
    // VOWEL takes ALGO's sixth cell, dimmed with no formant; FRM page.
    // Re-recorded: a value's unit sits small beside its number (ADR 0146).
    ("algo_formant_page", 0x8bc8e0509339504a),
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17);
    // then FOLD's column named FX (#614).
    ("mod_matrix_morph_inert", 0x1ef3db3220f17a71),
    // Re-recorded: WAVE shows its scope (#310).
    ("algo_wave", 0x7e339372006543ce),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_level", 0x3351d0e53f28923a),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_osc_last", 0xc9826973874d9856),
    // New: OP3's page (ADR 0130), the band's envelope and wave, and with
    // its LEVEL cell turned.
    ("algo_op_page_3", 0x78e1083b9c71ac40),
    ("algo_op_page_frmt", 0x5da1e716152e57f2),
    ("algo_op_page_nois", 0xc5c92e254217406e),
    ("algo_op_page_3_turned", 0x609eb129181c2d39),
    // The ENV stages' band draws the focused operator's envelope (#311).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_ar", 0x39f560afdf0de14d),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d1l", 0x0cdb5026bb4dcc99),
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_d2r", 0x0401035e7dfd9e2a),
    // Re-recorded: MODE reads LP4, CLEAN's label for LP24 (ADR 0096); then
    // TYPE live with two kinds (Task 8); then CLEAN's Cascadia words FREQ,
    // Q, FM1, FM2 (Task 12): was 0x032b329ca06df69b.
    ("bigviz_filter", 0x78a8f269dbaa88cb),
    // Re-recorded: MODE reads HP4, CLEAN's label for HP24 (ADR 0096); then
    // LEVEL 64 in DRIVE's cell (Task 6); then FLT › MOD's six cells, FM3
    // turned, its delete hint showing (Task 12): was 0xf6e7d94448ca7888.
    ("flt_mode", 0x39f981d71a39b49b),
    // Each type's FLT and FLT › MOD (filter-types § 5, Task 17): CLEAN's
    // NOTCH1, the 2040's lowered pass band and KEYBD HALF, its fixed MODE
    // dimmed and VOICE EXP, DIRTY's NOTCH1 at FREQ (its own MODE names).
    // Layout A (#614): VOICE EXPNDR on FLT (flt_2040_exp, was
    // flt_mod_2040_exp), KEYBD HALF on FLT › MOD, no MODE cell.
    ("flt_clean", 0x56110a2e1e38df2e),
    ("flt_mod_clean", 0xed08e904e809a166),
    ("flt_2040", 0x15653ec5210b6730),
    // KIND focused on the 2040: its readout names 4-POLE LP (#614).
    ("flt_2040_kind", 0x1e8668949f243f60),
    // Modulation draws a ghost beside the set curve (#575).
    ("flt_2040_env_amt", 0xdabadff9e26fed4f),
    ("flt_clean_lfo", 0xcdf3c95d4dd3230c),
    ("flt_mod_2040", 0x874d2be769536a3a),
    ("flt_2040_exp", 0x4cc08b535dd4156d),
    ("flt_dirty", 0x6c4b2e89b9de9bf7),
    ("flt_mod_dirty", 0xee8493802d548242),
    ("flt_ladder", 0xce6acb4caebff20a),
    ("flt_mod_ladder", 0x96def7f072a03866),
    ("flt_sem", 0x75e5ccb533d0012e),
    ("flt_mod_sem", 0x2fb7e90b0c31855c),
    ("flt_sem_bp", 0x0a6d2faa61b68b67),
    // New (#574): the peak at its real height, 2040 RES 0.3, CLEAN BP2 and
    // LADDER RES 0.
    // Re-recorded on the merge of main (#614): the filter picture and the
    // #574 rows in layout A (also flt_clean, flt_2040, flt_dirty, flt_ladder,
    // flt_2040_kind, flt_2040_exp, flt_led_full, flt_front_fx, row_flt,
    // bigviz_filter above).
    ("flt_2040_res_30", 0xec0d1a26aabe229b),
    ("flt_clean_bp2", 0x09dd5feef072a997),
    ("flt_ladder_res_0", 0xb120396d7760b858),
    // The LEVEL LED at 0 (dark), ½ and 1: beside LEVEL, beside the 2040's
    // DRIVE and in FLT's viz corner (Task 17); then beside FLT › FX's INPUT
    // (#614).
    ("flt_led_0", 0xaaaf412f13f766ca),
    ("flt_led_half", 0xff79b17ba9dd0333),
    ("flt_led_full", 0xc679e27959a0f747),
    // FM3 IN on LFO 2, QM IN on OP 6 (Task 17).
    // FB on FLT › MOD (ADR 0190).
    ("flt_mod_fb", 0xae55fae78261ebbe),
    ("flt_mod_repatched", 0xafec0f75419629ad),
    // FLT › FX (#614): OFF's dashes, SOFT's and FOLD's pots; FX and PLACE
    // on FLT's front.
    ("flt_fx_off", 0x7398595b237b8ceb),
    ("flt_fx_soft", 0x2a1a36ca519cce4d),
    ("flt_fx_fold", 0x43626f2582a4d9a8),
    ("flt_fx_stab", 0xbe3738e95c9891a2),
    ("flt_front_fx", 0x0186830cde57e60f),
    // Task 8 (#614): PLACE's mini glyph in its cell (every FLT front, row_flt,
    // large_va_drive) and FLT › FX's strip and curve (flt_fx_*, flt_led_*).
    ("flt_fx_soft_pre", 0x7b18a5267a6ada43),
    ("flt_fx_fold_post", 0x786864094029b3df),
    // THEME's PAGES ROW (ADR 0145): every other screen is recorded under
    // STACK (`screen::ui_for`). FLT's three pages, RES's three, MOD at E2
    // with `>`, OSC at D1R with both arrows and at OP6 with `<`, AMP bare.
    // Re-recorded: the row's pages in full words (`ROW_WORDS`).
    ("row_flt", 0x51ab20fe12e65d68),
    ("row_res", 0x52e7d782b213fed2),
    ("row_mod", 0x61ae81a248025efe),
    ("row_osc_mid", 0x10a75dc9cea244b8),
    ("row_osc_last", 0xa3198b2d2fb8a67d),
    ("row_amp", 0xedf62a5f9b585964),
    // New: VA OSC1 and OSC2 under STACK and ROW; the ROW bar says OSC1 / OSC2, not OSCILLATOR n.
    ("va_osc1", 0x117052eda46fe399),
    ("va_osc2", 0xd1bd400c5e8d1428),
    // Then the row gains NOISE (ADR 0197): OSC1 · OSC2 · NOISE · PITCH.
    ("row_va_osc1", 0x1ce4f23ee9eb7f84),
    ("row_va_osc2", 0x413f81a42d6a8978),
    ("env_a", 0x5e214e1a2c59ff77),
    ("env_b_env_ad", 0xcfb572705ac049f9),
    ("env_b_env_ahr", 0x8027de37baeab87c),
    ("env_b_env_cycle", 0xfa9c03d64def296e),
    ("env_b_lfo_free", 0x2b83679fab6288dc),
    ("env_b_lfo_sync", 0x3a50202b47f9d61c),
    ("env_b_lfo_lfv", 0x08a5dca5aa5715c5),
    ("env_b_burst_ad", 0x2b1e2232c9647c44),
    ("env_b_burst_ahr", 0xc8d91ad6c14aaf4f),
    ("env_b_burst_cycle", 0xace5233f63f26a97),
    ("spd", 0x8354b555c4c1c4a0),
    // Re-recorded: the scope draws the LFO and each control its glyph;
    // RATE reads Hz (ADR 0177). Then re-recorded on the merge with #614: the
    // chain map without DRV under main's LFO page.
    ("lfo_classic", 0x0cff5358dd1c18e0),
    ("lfo_func", 0xae823f94b4712202),
    // Re-recorded: AMP is the sends over FOLD, VEL, MIX, SYM one page down (sends Task 5);
    // mod_matrix_wide primes FOLD and MIX on their new cells. Then AMP is
    // the sends and VEL alone, SYM gone (#614).
    ("amp_vel_dimmed", 0xb39846680183f2f5),
    ("amp_vel_live", 0xd455067ceda96ed2),
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("algo_pitch", 0xd16ac1fa1fd56727),
    // Re-recorded: the EXC node (plan Task 13).
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("modal_pitch", 0xa1d8e11218a836be),
    // PIT on TAPE, SNAP MUSICAL: PITCH +19 in three clicks (TAPE Task 12b);
    // again for Task 15: PIT hangs under DECK.
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    ("tape_pitch_musical", 0x57bccbb87c98b3a5),
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
    ("modal_amp", 0xc6aa8cf54614ca9d),
    // The Mix chain's map loses its TAPE node without `master-tape`
    // (ADR 0055); with it they have their own rows, run by `just test`.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_part", 0xfd1a118204f19d77),
    // mixer_sends, mixer_out_p2 re-recorded: the sends are the Sound's OUT block (ADR 0123).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_sends", 0x35e09ae3385236bb),
    // Re-recorded: TYPE in front (ADR 0199).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay", 0x43f7a0dc7de5b19b),
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
    // GRAIN: DLY, its CHAR at PITCH +12, and bank 2 closed (ADR 0199).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_grain", 0x539faab78dcd5dab),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_char_grain", 0x3e86370045ab0bc9),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_grain_needs_update", 0xebf99b8b035ecdef),
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
    ("mixer_fx_delay", 0x3aafee28e7d8d358),
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
    ("mixer_fx_delay_grain", 0x449acab305e48e68),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_char_grain", 0xdd746404e0ae40fa),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_grain_needs_update", 0x0a00d3afc8dea3c4),
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
    ("algo_out_p3", 0x543753bd58a29fa6),
    // Re-recorded: four fixed columns, FREQ Q VCA FOLD (filter-types § 7);
    // then ALGO's ten rows of 14, OP5 and OP6 with X cells (Task 17);
    // then FOLD's column named FX (#614), here and in the next three.
    ("mod_matrix", 0x5dec043ad53b73a4),
    // Task 9 (#614): its FX headers read SOFT's TONE, not SHAP.
    ("mod_matrix_wide", 0x7bb6ee0e2f3c4143),
    // OP5 → FREQ, the cursor on OP5's X cell and its hint; MODAL's eight
    // rows of 17 (Task 17).
    ("mod_matrix_algo_taps", 0x62d4a32f114d8080),
    ("mod_matrix_modal", 0xea127c9de06a34ec),
    // Re-recorded: the pool meter, row costs, the hints' bytes (sound-pool
    // plan Task 6); and its refusal. Then the merge with filter-types: a
    // Sound is smaller (fixed columns, no registry entry).
    // Then, with the library_* screens: FACTORY joins the title and B ◂▸
    // moves to hint line 1 (showcase Task 4).
    ("sound_browser", 0xe6ebc09a421a5c11),
    // pool_full, library_sounds/this_build/newer re-recorded: MAX_SOUND_COST 2288 → 2352 (VA).
    // Then pool_full: 2352 → 2544 (the formant fields). Then VA's KIND and
    // LEVEL (ADR 0197): each dense Sound 10 B more, the pool fills sooner.
    ("sound_browser_pool_full", 0x81ce95e09e0bd574),
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
    ("settings_project_pool", 0xaedbd6aa48dfcbc4),
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
    // Re-recorded on the merge of main (#614): LARGE text (ADR 0154) over the
    // chain map without DRV, and FLT layout A: large_engine_algo,
    // large_algo_level, large_bigviz_filter, large_modal_amp, large_va_drive.
    ("large_engine_algo", 0x05c1170e863ceb31),
    ("large_algo_level", 0x6781c7502e39f90e),
    ("large_bigviz_filter", 0xcabe5de5ca85d1d7),
    ("large_modal_amp", 0x0a265b6a0a9884ed),
    // VA's NOISE page (ADR 0197): KIND · LEVEL; LEVEL at 60 with its bar;
    // KIND on DIGITAL.
    ("va_noise", 0xa02dcec5295dacbb),
    ("va_noise_level", 0x257365b54b1d88a2),
    ("va_noise_digital", 0x1eaa6a804b49021f),
    ("large_va_drive", 0x2257cff3560615e2),
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
    ("tape_src", 0x2ce444f656e7f685),
    ("tape_src_missing", 0x85708a674c5cfd05),
    ("tape_deck_oneshot", 0xf0404a951d313e58),
    ("tape_page_oneshot", 0x4214b7cafbf80e83),
    ("tape_deck_loop", 0xc76ebef5c7ae7d91),
    // New: bank 2 closed (ADR 0141).
    ("tape_deck_needs_update", 0xed4151c2af8a9ad8),
    ("tape_page_loop", 0x5f2dd678cbd3df95),
    ("tape_deck_tron", 0x8917ca0bee2e4020),
    ("tape_page_tron", 0x0fa1313170d7d76d),
    ("tape_deck_stretch", 0x77801ffd4b09f795),
    ("tape_page_stretch", 0x40b53e08880d943d),
    ("tape_page_stretch_type", 0xbdad5ba1e147c069),
    ("tape_deck_transwave", 0x9e80c12e371ebc81),
    ("tape_page_transwave", 0x37710bf1dec9e6e7),
    ("tape_deck_scrub", 0x6ae2ffb7d909827d),
    ("tape_page_scrub", 0xff848ff50ecd85ef),
    ("tape_deck_pingpong", 0x866e512118d56e00),
    ("tape_page_pingpong", 0xd7d9121a05f4f1d6),
    ("tape_strip_oneshot", 0xb01738aedde49e7d),
    ("tape_strip_loop", 0x870c72dc064eccc8),
    ("tape_strip_tron", 0x85b4d95422eb61b3),
    ("tape_strip_stretch", 0x1ed2362f660b4d7f),
    ("tape_strip_transwave", 0x2b819b488ccb62b1),
    ("tape_strip_scrub", 0x0e2f08b87a2e3e07),
    ("tape_strip_pingpong", 0xf625f6c153b7acbd),
    ("tape_strip_loop_reversed", 0x3cd8d0d69e8fda7e),
    ("tape_strip_oneshot_slice", 0x7a5e6875d9976c86),
    ("tape_strip_oneshot_slice_reversed", 0xa6e3cac4d5abd6fb),
    ("tape_strip_pingpong_reversed", 0x50e5909946ae9749),
    ("tape_strip_tron_idle", 0xfab668e8bd9e558c),
    ("tape_src_pool", 0x1a2310f01387e05a),
    ("tape_src_card", 0xd9e193292c4987f3),
    ("tape_src_wont_fit", 0x18d3447434e6f195),
    ("tape_src_loaded", 0x4587d0ef8bbb46b6),
    ("tape_src_read_failed", 0xa7d6268da8eee639),
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
        .map(|name| {
            let fb = render(name);
            fb.dump(name);
            (name, fb.hash())
        })
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
