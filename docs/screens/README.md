# Screens

Every screen the UI can show, rendered by the real renderer at 2× and
regenerated, never hand-made: `just screens` (needs ImageMagick) wipes this
folder's PNGs and writes them again.

Two tests make them:

- `chimera-core/tests/screen_golden_test.rs`: the pinned golden cases, each
  a page in a chosen state (values turned, routes primed). Their names are
  older and descriptive.
- `chimera-core/tests/screen_atlas_test.rs`: the atlas. It walks every node
  and sub-page of every chain in the block registry through the real
  controls and adds each page no golden already lands on, named
  `<chain>_<map label>`. Modal pages that follow MODEL are shown once per
  model (`modal_<model>_<label>`); a page holding a choice that changes it
  (STEAL, filter KIND) is shown again at its other values
  (`algo_pit_glide`). New pages and chains appear here on the next run.
  Not pinned: without `SCREEN_DUMP` it only checks that every screen
  renders on screen.

Built without `master-tape`, as the firmware ships: the mixer has no TAPE
node (ADR 0055).

## Boot and overlays

Overlays are drawn alone on a blank screen; on the device they land on
whatever the last frame left.

| Screen | Where | What it shows |
|---|---|---|
| ![](boot_splash.png) | Boot › splash | The Yellow Sign on yellow, held 1 s at power-on |
| ![](busy.png) | Boot › BUSY | Shown while boot reads the card |
| ![](toast_new_project.png) | Boot › toast | No last project: NEW PROJECT |
| ![](toast_no_card.png) | Boot › toast | No card in the slot |
| ![](toast_exfat.png) | Boot › toast | A card error on two lines: exFAT card |
| ![](toast_card_changed.png) | Project › toast | The card was swapped since the file was listed |
| ![](toast_file_damaged.png) | Project › toast | A load error: the file's CRC fails |
| ![](toast_load_failed.png) | Project › toast | Load failed partway: the project is NEW |
| ![](toast_not_found.png) | Project › toast | The project's file is gone |
| ![](toast_saved.png) | Project › toast | Leaving SETTINGS after a write: SAVED |
| ![](toast_saved_differs.png) | Project › toast | SAVED, with a Part differing from its slot |

## Sound browser

| Screen | Where | What it shows |
|---|---|---|
| ![](sound_browser.png) | EDIT + B1 | The pool with two named Sounds, then each engine's INIT |

## Algo Part (B1, Algo engine)

| Screen | Where | What it shows |
|---|---|---|
| ![](engine_algo.png) | ALG | Home: ALG A turned to T3, the algorithm diagram |
| ![](algo_alg.png) | ALG | ALG B A17 at MORPH 50: the two diagrams blend |
| ![](algo_alg_morph_dimmed.png) | ALG | ALG B = ALG A: MORPH focused and dimmed |
| ![](algo_out_p3.png) | ALG | Part 1 sent to P3: the OUT warning on its sound pages |
| ![](algo_pitch.png) | ALG › PIT | PITCH +7, FINE −25; STEAL CUT, GLIDE TIME |
| ![](algo_pit_glide.png) | ALG › PIT | STEAL at GLIDE |
| ![](algo_wave.png) | OSC | Operator waves, op 2 a few steps in |
| ![](algo_crs.png) | OSC › CRS | Coarse ratio, ops 1–6 |
| ![](algo_fin.png) | OSC › FIN | Fine ratio, ops 1–6 |
| ![](algo_det.png) | OSC › DET | Detune, ops 1–6 |
| ![](algo_level.png) | OSC › LVL | Output level, op 2 turned |
| ![](algo_vel.png) | OSC › VEL | Velocity sensitivity, ops 1–6 |
| ![](algo_ar.png) | OSC › AR | Envelope attack rate, ops 1–6 |
| ![](algo_d1r.png) | OSC › D1R | Envelope first decay rate |
| ![](algo_d1l.png) | OSC › D1L | Envelope first decay level |
| ![](algo_d2r.png) | OSC › D2R | Envelope second decay rate |
| ![](algo_rr.png) | OSC › RR | Envelope release rate |
| ![](algo_rs.png) | OSC › RS | Rate scaling, ops 1–6 |
| ![](algo_osc_last.png) | OSC › FBK | Feedback, the last OSC sub-page |
| ![](algo_drv.png) | DRV | Drive: DRIVE, TONE, MIX |
| ![](bigviz_filter.png) | FLT | Filter response, resonance up, cutoff focused |
| ![](flt_mode.png) | FLT › MDE | MODE at BP12, DRIVE and LFO extras |
| ![](amp_vel_dimmed.png) | AMP | Fold up; VEL focused, dimmed (no route to the VCA) |
| ![](amp_vel_live.png) | AMP | VEL live once E2 → VCA is routed |

## MOD node (both engines)

The MOD node is the same on both Part chains; the goldens show it on Algo,
the atlas on Modal.

| Screen | Where | What it shows |
|---|---|---|
| ![](mod_matrix.png) | Algo MOD › MTX | The matrix: three routes, LFO1 → CUTOFF selected |
| ![](mod_matrix_wide.png) | Algo MOD › MTX | Eight primed columns, scrolled: `<` and `>` |
| ![](mod_matrix_morph_inert.png) | Algo MOD › MTX | A route to MORPH while MORPH is inert |
| ![](env_a.png) | Algo MOD › E1 | Type A envelope (AHDSR) |
| ![](algo_e2.png) | Algo MOD › E2 | E2 at INIT: type A |
| ![](env_b_env_ad.png) | Algo MOD › E3 | Type B, ENV · AD |
| ![](env_b_env_ahr.png) | Algo MOD › E3 | Type B, ENV · AHR |
| ![](env_b_env_cycle.png) | Algo MOD › E3 | Type B, ENV · CYCLE |
| ![](env_b_lfo_free.png) | Algo MOD › E3 | Type B, LFO · FREE |
| ![](env_b_lfo_sync.png) | Algo MOD › E3 | Type B, LFO · SYNC |
| ![](env_b_lfo_lfv.png) | Algo MOD › E3 | Type B, LFO · LFV |
| ![](env_b_burst_ad.png) | Algo MOD › E3 | Type B, BURST · AD |
| ![](env_b_burst_ahr.png) | Algo MOD › E3 | Type B, BURST · AHR |
| ![](env_b_burst_cycle.png) | Algo MOD › E3 | Type B, BURST · CYCLE |
| ![](spd.png) | Algo MOD › SPD | Speed and hold for E1–E3, E2 FAST |
| ![](lfo_classic.png) | Algo MOD › L1 | Classic LFO, rate turned |
| ![](lfo_func.png) | Algo MOD › L1 | TYPE FUNC: the function generator |
| ![](algo_l2.png) | Algo MOD › L2 | LFO 2 at INIT |
| ![](algo_l3.png) | Algo MOD › L3 | LFO 3 at INIT |
| ![](modal_mtx.png) | Modal MOD › MTX | The matrix at INIT |
| ![](modal_e1.png) | Modal MOD › E1 | Envelope 1 at INIT |
| ![](modal_e2.png) | Modal MOD › E2 | Envelope 2 at INIT |
| ![](modal_e3.png) | Modal MOD › E3 | Envelope 3 at INIT |
| ![](modal_spd.png) | Modal MOD › SPD | Envelope speeds at INIT |
| ![](modal_l1.png) | Modal MOD › L1 | LFO 1 at INIT |
| ![](modal_l2.png) | Modal MOD › L2 | LFO 2 at INIT |
| ![](modal_l3.png) | Modal MOD › L3 | LFO 3 at INIT |

## Modal Part (B1, Modal engine)

EXC, RES and MDL2 follow MODEL (STRING, BANK, BOWED, SYMP); each is shown
per model.

| Screen | Where | What it shows |
|---|---|---|
| ![](modal_exc.png) | EXC (STRING) | PLUCK exciter: EXCITE, COLOR |
| ![](modal_exc_bank.png) | EXC (BANK) | STRIKE exciter: EXCITE, BURST |
| ![](modal_exc_bowed.png) | EXC (BOWED) | BOW exciter: FORCE, SPEED |
| ![](modal_symp_exc.png) | EXC (SYMP) | PLUCK exciter for the sympathetic set |
| ![](modal_home.png) | RES (STRING) | Home: MODEL, the four macros, SPACE |
| ![](modal_bank_res.png) | RES (BANK) | MODEL BANK |
| ![](modal_home_bowed.png) | RES (BOWED) | MODEL BOWED: STRUCT dims |
| ![](modal_symp_res.png) | RES (SYMP) | MODEL SYMP |
| ![](modal_string_mdl2.png) | RES › MDL2 (STRING) | BODY and the ensemble |
| ![](modal_bank_mdl2.png) | RES › MDL2 (BANK) | The bank's model page |
| ![](modal_bowed_mdl2.png) | RES › MDL2 (BOWED) | The bow's model page |
| ![](modal_mdl2_symp.png) | RES › MDL2 (SYMP) | The sympathetic set's model page |
| ![](modal_pitch.png) | RES › PIT | PITCH −12 focused; STEAL CUT |
| ![](modal_pit_glide.png) | RES › PIT | STEAL at GLIDE |
| ![](modal_flt.png) | FLT | Filter response at INIT |
| ![](modal_mde.png) | FLT › MDE | MODE and the kind's extras at INIT |
| ![](modal_amp.png) | AMP | Fold / VCA at INIT |

## Mixer (MIX + B1)

Part 1's PART and SENDS; the other Parts' differ only in the header. The
shared FX come after Part 6's SENDS (PLUS from there), so they read
PART 6 · MIX.

| Screen | Where | What it shows |
|---|---|---|
| ![](mixer_part.png) | PRT | CHANNEL, MODE, OUT, LEVEL, PAN and the Parts' levels |
| ![](mixer_sends.png) | SND | Home: the three sends and the FX flow |
| ![](mixer_out_p2.png) | SND | Part 1 sent to P2: the OUT warning |
| ![](mixer_chr.png) | CHR | Chorus: MODE, RATE, DEPTH, MIX |
| ![](mixer_fx_delay.png) | DLY | Delay: TIME, FEEDBACK, TONE, REV send, MIX |
| ![](mixer_fx_delay_char.png) | DLY › CHAR | Tape character: WOW turned, SATURATION |
| ![](mixer_fx_reverb.png) | REV | Reverb, GRIT turned |
| ![](mixer_master.png) | MST | Compressor curve, RATIO 4:1 |
| ![](mixer_master_level.png) | MST › LVL | Master VOL and PAN |

## SETTINGS (MENU)

A breadcrumb for the header, a list with a bar, and the project footer in
the map's band: the project's name, its status (none while NEW, `SAVED`,
`* MODIFIED`), and what the keys do. The atlas walks the tree depth first
through the real keys; LOAD, MANAGE and the project prompts run against an
in-memory card.

| Screen | Where | What it shows |
|---|---|---|
| ![](settings_top.png) | SETTINGS | The top list on a fresh project: the footer is the name alone |
| ![](settings_footer_saved.png) | SETTINGS | ALPHA loaded: SAVED |
| ![](settings_footer_modified.png) | SETTINGS | ALPHA edited since: `* MODIFIED` |
| ![](settings_project.png) | PROJECT | LOAD, SAVE AS, MANAGE |
| ![](settings_load.png) | PROJECT › LOAD | Three projects, ALPHA `● LOADED`, the bar on BETA |
| ![](settings_project_load_no_card.png) | PROJECT › LOAD | No card in the slot |
| ![](settings_naming.png) | PROJECT › SAVE AS | NAMING, the proposed name |
| ![](settings_project_manage.png) | PROJECT › MANAGE | The projects, and the commands of the one under the bar |
| ![](settings_project_manage_commands.png) | PROJECT › MANAGE | The bar in the commands, on RENAME: dimmed, LOAD TO RENAME |
| ![](settings_part.png) | PART | The active Part's strip and its actions |
| ![](settings_part_save_to.png) | PART › SAVE TO | Part 1 edited from SLOT 03: OVER SLOT 03, TO NEW SLOT |
| ![](settings_part_rename.png) | PART › RENAME | NAMING the Part's Sound |
| ![](settings_midi.png) | MIDI | SYNC and PORT later, CHANNELS |
| ![](settings_audio.png) | AUDIO | OUTPUTS, SENDS later, TUNING |
| ![](settings_personal.png) | PERSONAL | THEME |
| ![](settings_system.png) | SYSTEM | OS, ABOUT, DEMO; the rest later |

### Prompts

One per kind; the list beneath blanks. Outside SETTINGS the panel clears
its band over the frozen screen.

| Screen | Where | What it shows |
|---|---|---|
| ![](settings_prompt_load.png) | PROJECT › LOAD | LOAD BETA over an edited ALPHA |
| ![](settings_prompt_name_exists.png) | PROJECT › SAVE AS | The proposed name is on the card |
| ![](settings_prompt_card_changed.png) | MENU hold | Quick save refused: another card |
| ![](settings_prompt_delete.png) | MANAGE › DELETE | DELETE BETA |
| ![](settings_prompt_clear.png) | MANAGE › CLEAR | CLEAR BETA |
| ![](settings_prompt_save_over.png) | MANAGE › SAVE TO | SAVE OVER BETA |
| ![](settings_prompt_replace_part.png) | PART › CLEAR | CLEAR an edited Part to INIT |
| ![](settings_prompt_reload_part.png) | PART › RELOAD | RELOAD an edited Part from its slot |
| ![](settings_prompt_update_stale.png) | PART › SAVE TO | P4 also plays the slot saved over |
| ![](settings_prompt_clear_slot.png) | Sound rung, MIX + MINUS | CLEAR a slot no Part plays |

### Leaves

A leaf is the page it was on the old System chain. On a leaf with several
pages, the breadcrumb ends on the page past the first.

| Screen | Where | What it shows |
|---|---|---|
| ![](settings_midi_channels.png) | MIDI › CHANNELS | Each Part's MIDI channel: the mixer's own value |
| ![](settings_audio_outputs.png) | AUDIO › OUTPUTS | Each Part's OUT: the mixer's own value |
| ![](settings_audio_tuning.png) | AUDIO › TUNING | TUNE and SCALE |
| ![](settings_personal_theme.png) | PERSONAL › THEME | BRIGHT, GAMMA, ACCENT (AMBER, focused), BLACK |
| ![](settings_system_os.png) | SYSTEM › OS | Updates: no controls yet |
| ![](settings_system_about.png) | SYSTEM › ABOUT | About: no content yet |
| ![](settings_system_about_audio.png) | ABOUT › AUD | Audio load, peak, overruns, drops, desyncs, stack |

## DEMO (SETTINGS › SYSTEM › DEMO, debug builds)

The UI component storyboard.

| Screen | Where | What it shows |
|---|---|---|
| ![](settings_system_demo_wav.png) | WAV | Wave icons |
| ![](settings_system_demo_shp.png) | SHP | Shape icons |
| ![](settings_system_demo_mot.png) | MOT | Motion icons |
| ![](settings_system_demo_fm.png) | FM | FM icons |
| ![](settings_system_demo_mtx.png) | MTX | A matrix |
| ![](settings_system_demo_arc.png) | ARC | Glyph: ARC; a drives it unipolar, b bipolar |
| ![](settings_system_demo_none.png) | NONE | Glyph: NONE; a steps MODEL, its word has the whole band |
| ![](settings_system_demo_sw.png) | SW | Glyph: SWITCH; a flips LFO 1's SYNC, FREE / RETRIG |
| ![](settings_system_demo_lvl.png) | LVL | Glyph: LEVEL BAR; a drives VOLUME (8 ticks), b operator A's FDBK (a tick per step) |
| ![](settings_system_demo_xf.png) | XF | Glyph: CROSSFADER; a slides ALG A to B (MORPH), b dry to wet (DRV MIX); set values only |
| ![](settings_system_demo_brd.png) | BRD | Glyph: chorus BRAID; a MODE (strands), b RATE (twist speed), c DEPTH (swing), d MIX (strand weight); animated on the UI clock |
| ![](settings_system_demo_rng.png) | RNG | Glyph: delay RINGS; a TIME (spacing), b FDBK (survivors), c TONE (crisp or blurred), d MIX (ring weight), e MECHANICS (wobble), f SAT (the newest ring's weight); REV dots the edge |
| ![](settings_system_demo_cub.png) | CUB | Glyph: reverb CUBE; a SIZE (the room), b TIME (afterimage trails), c DAMP (far edges dim, then dot), d MIX (edge weight), e GRIT (crackle) |
