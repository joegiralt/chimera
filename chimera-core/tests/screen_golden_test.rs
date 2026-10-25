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
// Re-recorded for ADR 0187: INIT TAPE joins the browser and NEW; the pool
// meter, cost and the factory TAPE row follow.
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
// flt_front_fx, row_amp. Gone: row_drv, amp_sym, flt_mod_2040_exp.
// Re-recorded in Task 14: the curve viz draws SOFT and FOLD at their
// `MAKE_UP` 0.483 and 0.684 (flt_fx_soft, flt_fx_fold, _soft_pre, _fold_post).
// Again for the TAPE merge: every TAPE map drops DRV (#614), and
// sound_browser_pool_full counts main's factory bytes.
// #698: WOW dims at TAPE 0 and LOOP's and PING's FADE under an AMP route
// (tape_page_*); START and END read time (tape_strip_oneshot*,
// tape_strip_loop_reversed, tape_src_pool, tape_src_loaded).
// Re-recorded for the FREQ MOD / RES MOD band naming its source: flt_mode, flt_mod_clean.
// Re-recorded for ADR 0167 (FLT's front reordered: KIND · FX · Q · MODE ·
// FX PLACE · FREQ): every FLT front page, row_flt, large_va_drive.
// The fourth TAPE merge: the browsers and POOL count 16 Sounds with STAB
// and FB (4 %).
// ADR 0203: WALK replaces ONE, LOOP, PING and TRON (its DECK, its page,
// its footsteps and strip, tape_deck_reverse and tape_deck_drift new);
// sound_browser_pool_full's bound moves with WALK's two params.
// ADR 0205: SCRUB's LENGTH 70 ms and KEY on its page (tape_deck_scrub,
// tape_page_scrub, tape_strip_scrub); a Sound's bound 2 672 B moves the
// pool's count (sound_browser_pool_full).
// ADR 0195: AMP is VEL · LPG · DECAY · STRIKE, the sends on AMP › SENDS.
// New: amp_gate, amp_sends. Re-recorded: row_amp, amp_vel_dimmed,
// amp_vel_live, modal_amp, large_bigviz_filter (an AMP screen),
// large_modal_amp; sound_browser_pool_full (a dense Sound is 15 B larger).
// Review of #761: TAPE cells read their engine laws' units (TRON's LENGTH
// in seconds, SCRUB's LENGTH, FADE and DRAG in ms, DRIFT in %, SPEED as a
// factor), the readout names MODEL and DIRECTION in full, and SLICE is
// never shown (tape_deck_*, tape_page_stretch*, tape_strip_*,
// tape_strip_oneshot_slice*: no slices); sound_browser_pool_full (a dense
// Sound's SLICE stores OFF, 5 B smaller).
// ADR 0209: WALK's glyph is STRAIGHT, runners on the home straight
// (tape_deck_* and tape_page_* of WALK's ways, tape_strip_* but stretch,
// transwave, scrub and loop_reversed).
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
    ("row_amp", 0xfa6545ca8559b8a5),
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
    ("amp_vel_dimmed", 0x97b5ec576db1686d),
    ("amp_gate", 0xac47230d2acbfe20),
    ("amp_sends", 0xc9b0116af8f449fa),
    ("amp_vel_live", 0xd2952b0ccaa7127e),
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    // Re-recorded: the scope's corner reads REF C4 (ADR 0201).
    ("algo_pitch", 0xaea5614dc8b3c57f),
    // Re-recorded: the EXC node (plan Task 13).
    // Re-recorded: PIT shows SNAP on E (TAPE Task 12b).
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    // Re-recorded: the scope's corner reads REF C4 (ADR 0201).
    ("modal_pitch", 0xf393e330a5b8f647),
    // PIT on TAPE, SNAP MUSICAL: PITCH +19 in three clicks (TAPE Task 12b);
    // again for Task 15: PIT hangs under DECK.
    // Re-recorded: a knob that glides shows a bar (ADR 0112).
    // Re-recorded at the main merge: the header's Part dots (ADR 0150).
    // Re-recorded: its map names the machine, LOOP (ADR 0178).
    ("tape_pitch_musical", 0xebb38641a3992e19),
    // New: PIT's tuner (ADR 0201). Re-recorded at the integration: the map
    // under it says WALK (ADR 0203) and NOISE above PIT (ADR 0197).
    ("algo_pitch_note", 0x84aab66825c66813),
    ("tape_pitch_root_unknown", 0xb289953a05153b28),
    ("large_va_pitch_note", 0x359d3b962f4f7710),
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
    ("modal_amp", 0xa87011df4f83c078),
    // The Mix chain's map loses its TAPE node without `master-tape`
    // (ADR 0055); with it they have their own rows, run by `just test`.
    #[cfg(not(feature = "master-tape"))]
    ("mixer_part", 0xfd1a118204f19d77),
    // mixer_sends, mixer_out_p2 re-recorded: the sends are the Sound's OUT block (ADR 0123).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_sends", 0x35e09ae3385236bb),
    // Re-recorded: TYPE in front (ADR 0202).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay", 0x0858ddab18f1090a),
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
    // Re-recorded: the map's DLY branch holds GRAIN (ADR 0202).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_char", 0xf7d292f46d3dbced),
    // GRAIN: DLY, DLY › GRAIN at PITCH +12, and bank 2 closed (ADR 0202).
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_grain", 0xef1ed3ecaaa14bba),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_grain_page", 0xd8814ef0bb2d5569),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_fx_delay_grain_needs_update", 0xc98f6ca5642f4246),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master", 0xaa7cc5f71d43600a),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_master_level", 0x33a39d11187d9bb1),
    // Re-recorded: the map names the master tape MTP, not REEL (#646).
    #[cfg(feature = "master-tape")]
    ("mixer_part", 0x9f6c12c0b45db43d),
    #[cfg(feature = "master-tape")]
    ("mixer_sends", 0x4f5f9daa56c5aea5),
    // DLY's branch hides REV's label it ran into (TAPE Task 15). With master-tape
    // the delay pages, GRAIN's among them (#749), map seven nodes with MTP (#756).
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay", 0x5b21af63949b68dc),
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
    ("mixer_fx_delay_char", 0x323fc0627565e957),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_grain", 0x869e1540291fb66c),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_grain_page", 0x45d7ede89404786f),
    #[cfg(feature = "master-tape")]
    ("mixer_fx_delay_grain_needs_update", 0x4d0db4ababe3a198),
    #[cfg(feature = "master-tape")]
    ("mixer_tape", 0xdbadbeba402b34e4),
    #[cfg(feature = "master-tape")]
    ("mixer_master", 0x7b0e9395aaa4c7f8),
    #[cfg(feature = "master-tape")]
    ("mixer_master_level", 0xdf9c8193151e72b3),
    #[cfg(not(feature = "master-tape"))]
    ("mixer_out_p2", 0xbbf4869671d1e56d),
    #[cfg(feature = "master-tape")]
    ("mixer_out_p2", 0xec847a587865a15f),
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
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("sound_browser", 0x958456622543372c),
    // pool_full, library_sounds/this_build/newer re-recorded: MAX_SOUND_COST 2288 → 2352 (VA).
    // Then pool_full: 2352 → 2544 (the formant fields).
    // Re-recorded at the merge: MAX_SOUND_COST 2688 (VOICEFX and PLAY's MODE).
    // Fourth TAPE merge: MAX_SOUND_COST 2656 (STAB, FB and TAPE), same picture.
    // Then SEM's MORPH: the dense fill leaves 0.1K FREE, not 0.0K. Then VA's
    // KIND and LEVEL (ADR 0197): each dense Sound 10 B more, the pool fills sooner.
    // Then the VCA's gate (ADR 0195): 15 B more. STRETCH GRAIN's four params
    // (ADR 0204): MAX_SOUND_COST 2720.
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("sound_browser_pool_full", 0x16e71cda6e99b5d3),
    // The browser's title is its tabs and the Part (library plan Task 7); then
    // the LIBRARY tab: each Sound's build, its pages, what won't fit.
    // Merged: FACTORY in the title (showcase) beside the formant cost.
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_empty", 0xd4f0b068fb297a11),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_no_card", 0x0ef3cabc0a005fac),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_sounds", 0x8179b02b76c07912),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_this_build", 0xe2a5bfa32dc2f895),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_cut_short", 0xc82d8ec053050608),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_older", 0xeff7f1c0da22a8d2),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_newer", 0xbc6be4aa213e6763),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_wont_fit", 0xba02297fe9fba3db),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("library_pool_full", 0xcfe515a8e7a48a23),
    ("library_naming", 0xa234ea336d54efbf),
    // The FACTORY tab: a row's description on line 2, the set's build on
    // line 3; empty, no card, and a TAPE file listed.
    // Re-recorded on the merge: the pool meter at the formant cost.
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("factory_described", 0xe16af98ceb186186),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("factory_empty", 0x4e7a62f82ecfe083),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("factory_no_card", 0xf2356a3f4d7b1db6),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("factory_tape_row", 0xeccddb1e270cf713),
    // Re-recorded: a fresh project's footer drops NEW (#286).
    // Re-recorded: INVERT in cell F (ADR 0126).
    ("settings_personal_theme", 0x69eb789acb0d98db),
    // PERSONALIZE › CHAIN MAP: PAGES, ROW (ADR 0145).
    ("settings_personal_chain_map", 0x7aa40f30346b46bd),
    // Long user names, LARGE's pair (ADR 0154).
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("sound_browser_long", 0xcd48d3df38542330),
    // PERSONALIZE › TEXT SIZE at NORMAL (ADR 0154).
    ("settings_personal_text_size", 0x771bfc59119e85d2),
    ("settings_midi_sync", 0x0e26ce69d0f54980),
    ("settings_system_diag_aud_load", 0x18fe4d081416e4cb),
    // Re-recorded: NEW holds TAPE's INIT too (TAPE Task 12), each Sound
    // sparser with the fixed columns, then VA's INIT: `12 · 2.2K`, 157K free.
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("settings_project_pool", 0x553fccecbee75ba9),
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
    ("large_bigviz_filter", 0xdf4afe14780965be),
    ("large_modal_amp", 0xe38ccc705df20b41),
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
    ("large_mixer_sends", 0x2db88c25aaaa4ce0),
    ("large_settings_top", 0x9d2976351278b0df),
    ("large_settings_personal", 0x35b1c3eb794412f7),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("large_sound_browser", 0xd12afb6198751769),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("large_sound_browser_long", 0xb600433b28399753),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("large_library_sounds", 0x87eade6ec856523e),
    ("large_settings_text_size", 0xe57ffb4a4502d385),
    // SCN: a saved slot, SAVE's, a queued one filling (plan Task 16).
    ("orbit_scn", 0x7c2558bf895b7432),
    // TAPE's pages (TAPE Task 15): SRC, and DECK and the model page per machine.
    // Re-recorded at the main merge: the header's Part dots (ADR 0150).
    // Re-recorded: every machine cell shows its machine's glyph (ADR 0173).
    ("tape_src", 0x2ce444f656e7f685),
    ("tape_src_missing", 0x85708a674c5cfd05),
    // Re-recorded: MODEL lists ONE, LOOP and PING; DECK holds the glyph's
    // knobs, the model page TAPE, AGE, WOW and the rest (ADR 0178).
    ("tape_deck_oneshot", 0x8944e72960566c6b),
    ("tape_page_oneshot", 0x15aaf372903697a0),
    ("tape_deck_loop", 0x985dbe434b72743d),
    // New: bank 2 closed (ADR 0141).
    ("tape_deck_needs_update", 0x8ea88405e665e1bf),
    ("tape_page_loop", 0x5d08319ff039180d),
    ("tape_deck_tron", 0x1b5bd93b4f9c0f56),
    ("tape_page_tron", 0x466c79792e4d48f8),
    ("tape_deck_stretch", 0x20e758645ffd57e5),
    ("tape_page_stretch", 0x370c5864b61e6cb2),
    ("tape_page_stretch_type", 0x43a2555e7b1a6d7b),
    ("tape_deck_transwave", 0xde8bbeffbb06c81a),
    ("tape_page_transwave", 0x04bbc21e5964e49d),
    ("tape_deck_scrub", 0xda740d213c4e7248),
    ("tape_page_scrub", 0x4824af71a2446b88),
    ("tape_deck_pingpong", 0x11a47368772fcd7b),
    ("tape_deck_reverse", 0x5fcdae5da43716db),
    ("tape_deck_drift", 0xf018826c7f1f0c0a),
    ("tape_page_pingpong", 0x8d5ecb1f3c42e8bb),
    ("tape_strip_oneshot", 0xf2d5d8e8d317a65a),
    ("tape_strip_loop", 0x7c7e02bb60138e34),
    ("tape_strip_tron", 0x4a28255ecd58fa8c),
    ("tape_strip_stretch", 0xa8095293937600a7),
    ("tape_strip_transwave", 0x8578d1cff2b53962),
    ("tape_strip_scrub", 0x81d506fe7de6c3a2),
    ("tape_strip_pingpong", 0x83b65cfe419c64a1),
    ("tape_strip_loop_reversed", 0x8c6bc98f7858d886),
    ("tape_strip_oneshot_slice", 0x653bdf22c0400dea),
    ("tape_strip_oneshot_slice_reversed", 0xb25bf3e3ab896335),
    ("tape_strip_pingpong_reversed", 0xf1f38ad830237d16),
    ("tape_strip_tron_idle", 0xcce2e4d1be026dcf),
    ("tape_src_pool", 0x6ac82eb8b9a261f1),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("tape_src_card", 0x83a447e85f1aa483),
    // Maps as `Bindings` 0x001F: NEW's Sounds 34 B more (MIDI phase 1).
    ("tape_src_wont_fit", 0x1de0d425a65a9ffa),
    ("tape_src_loaded", 0x2570f7f781938fd9),
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
