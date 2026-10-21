# 0202. TAPE: PLAY replaces ONESHOT, LOOP, PING-PONG and TRON

- **Status:** Proposed
- **Deciders:** owner (2026-10-21: the four machines are "ultimately the same thing"; machine names are verbs; TRON "should just be toggleable"; WALK is random drift), firmware
- **Supersedes in part:** [0170](0170-tape-machines-are-play-kit-tron-reel-scrub-stretch-and-wave.md) (the lineup), [0178](0178-tape-model-lists-one-loop-and-ping-and-deck-holds-the-glyphs-knobs.md) (MODEL's seven machines, the ONE, LOOP, PING and TRON rows of its DECK table, and its DECK-is-what-moves-the-glyph rule for PLAY), [0180](0180-tape-machine-defaults-are-musical-from-the-first-turn.md) (ONE's, LOOP's, PING's and TRON's defaults, now PLAY's by DIRECTION), [0181](0181-tape-laws-walk-in-seconds-stretch-slow-age-from-the-band.md) (WALK in seconds of source a second) and [0189](0189-tape-params-hold-one-machine-the-codec-splits-it.md) (MODE folded into the machine, `KeptMode`)

## Context
ONE, LOOP, PING and TRON all play one head through the window. They differ in which way it goes, whether it comes round, and whether the tape runs out. MODEL listed them as four machines (0178) beside STRETCH, SCRUB and WAVE, each with its own cells, so the same LENGTH or FADE sat in a different place on each. The owner merged them into one machine, PLAY, and named the machines as verbs: PLAY · STRETCH · SCRUB · WAVE, with SLICE coming (0199, `tape-slice`).

Today's TRON was measured against ONE before the merge, on its own factory Sound turned to ONE (a 4 s strip at 8 kHz, through the whole instrument). They don't sound the same. On a chord let go at 1.5 s, TRON is at −59 dB by 1.6 s, while ONE (FADE at HOLD) plays on to past 6 s, slower below the root. On a chord held 10 s, TRON's tape runs out at 4.0 s, while ONE plays to 12 s. So the run-out and the pad already existed; the toggle keeps them.

## Decision
- **MODEL lists PLAY, STRETCH, SCRUB, WAVE** (`TapeMachine::ALL`, values 0 to 3), and SLICE after them when it lands. The MODEL cell reads PLAY, STRCH, SCRUB, WAVE; the model page's header reads PLAY, STRETCH, SCRUB, TRANSWAVE (TRANSWAVE keeps its header name: not renamed here).
- **PLAY's controls**, on 6-cell pages:

  | Page | Cells |
  |---|---|
  | SRC | SOURCE · START · END · VOL (unchanged: START and END are the window) |
  | DECK | MODEL · LENGTH · FADE · DIRECTION · POSITION · WALK |
  | PLAY (the model page) | TAPE · AGE · WOW · TRON · XFADE · RETRIG |

  - **DIRECTION**: FORWARD · REVERSE · LOOP · PING-PONG, read FWD, REVERSE, LOOP, PING (FORWARD and PING-PONG overrun a cell). FORWARD is ONE's path, LOOP LOOP's, PING-PONG PING's. REVERSE is ONE's path with the window flipped: REVERSE with START before END plays exactly as FORWARD with END before START, and REVERSE over END before START plays forward again. END before START still reverses FORWARD and LOOP (#369).
  - **POSITION**: where the run starts. Around a loop it places the loop in the room the window leaves, as LOOP did. Once through it is the start, counted the way it plays (REVERSE counts from END).
  - **LENGTH**: how much plays from POSITION. Around a loop it is LOOP's loop law. Once through it is the same law over what is left from POSITION, so LENGTH 1 plays to the window's end. With TRON on, once through, LENGTH is the tape, as TRON's was: 0.25 s up to the strip, at most 8 s, the same at every pitch.
  - **FADE**: the note's edge fade, PLAY's and TRON's FADE as before (10 to 500 ms). Once through with TRON off its last step is HOLD: key up is ignored and the note plays to its end. Around a loop it releases at key up, stepping aside for AMP under a VCA route (0179). A loop's seam crossfade stays XFADE, on the model page.
  - **WALK** (the owner's): at each note-on POSITION steps by a random amount, up to ±WALK of the window, from where the last note on that voice left it. So repeats wander through the sample; WALK 1 reaches anywhere. The place **wraps** at the window's ends (as the old walk did), rather than reflecting. WALK 0 is off: no draw, and the place rests at POSITION. Each voice draws from its own xorshift, seeded by its slot (`walk::seed`), apart from its wow's, so a render repeats. The steps are uniform: mean 0, mean |step| WALK/2, RMS WALK/√3. WALK is read at note-on (its own param, id 20, a byte on the knob's 1/128 grid); MOTION no longer moves PLAY.
  - **TRON**: OFF · ON (the full word fits). ON adds the Mellotron M400: each note's tape runs out (LENGTH's once through, 8 s around a loop, then a 10 ms fall), and key up lifts the pressure pad through FADE, always, even under a VCA route. HOLD has no meaning under TRON, so turning TRON on with FADE at HOLD writes TRON's 32 ms (`part_page::machine_edit`). TRON once through refuses a zone set, as TRON did.
  - **XFADE** is read only around a LOOP, and **RETRIG** only once through with TRON off. Elsewhere each is dimmed with its reason in the band: `ONLY LOOP CROSSFADES ITS SEAM`, `CUTS ONLY ONCE THROUGH`.
- **Defaults.** A MODEL turn onto PLAY writes its cells, keeps DIRECTION and switches TRON off. LENGTH and FADE take DIRECTION's: once through, LENGTH 1 and FADE HOLD, with ENV 1's hit (0180); around a loop, LENGTH 0.8 and FADE 32 ms. A DIRECTION turn between once through and a loop writes the same, as a MODEL turn between ONE and LOOP did; within a side it writes nothing. The neutral, and INIT TAPE, are PLAY in LOOP, TRON off, WALK 0.
- **Streaming.** REVERSE and PING-PONG need RAM (`needs_ram`), as END-before-START and PING did. A DIRECTION turn onto one on a streamed source is held, dimmed with `… NEEDS RAM · STREAMED`, nothing written (0186's rule, now for DIRECTION too). TRON streams. A FORWARD note started past START by POSITION or WALK on a streamed source starts cold (no RAM head there), as a moved START does.
- **The codec.** Codes are append-only; old codes are mapped on load, never reused.
  - MODEL: PLAY 0, TRANSWAVE 3, SCRUB 4, STRETCH 5 (SLICE 8, 0199). TRON's 6 is retired, beside REVERSE 1, PING-PONG 2 and ONESHOT 7.
  - DIRECTION keeps MODE's id (15) and ident `MODE`: LOOP 1, PING 2 (their codes and meaning kept), FORWARD 3, REVERSE 4. MODE ONE (0) is retired: ONE read neither POSITION nor LENGTH, and FORWARD does.
  - TRON is id 19 (OFF 0, ON 1), WALK id 20. Ids 17 and 18 are left for SLICE's SLICES and TUNE (0199).
  - `translate_v1`: MODEL TRON loads as PLAY FORWARD with TRON on, POSITION 0 (TRON never read it), LENGTH kept. MODE ONE on PLAY loads as FORWARD with POSITION 0 and LENGTH 1; off PLAY, the code was only kept, so it becomes FORWARD alone. ONESHOT, PING-PONG and REVERSE load as before.
  - In memory, `TapeEngineParams` holds `machine`, `direction`, `tron` and `walk`; the deck's path is `Way` (ONE, LOOP, PING, TRON, SCRUB, STRETCH, WAVE), from which the costs, the glyph and the strip read. The costs are unchanged.
- **Glyphs.** The machine glyph is the path's: FORWARD and REVERSE ONE's match, its run cut by POSITION and LENGTH and burning from the far end on REVERSE; TRON's blind; LOOP's wheel and PING-PONG's court, whose drift is WALK's. Its inputs are quantised set values, with DIRECTION, TRON and WALK added to `TAPE_GLYPH_PARAMS`; the key holds no frame term, so nothing redraws at idle.
- **Exceptions to 0178's DECK rule** (which still holds for STRETCH, SCRUB and WAVE): FADE moves only TRON's blind, WALK only the loops', and XFADE and TRON, on the model page, move LOOP's wheel and choose the blind.

## Alternatives considered
- **WALK bipolar, a fixed step either way.** That was the coordinator's ruling before the owner's: replaced by the random drift.
- **WALK reflecting at the window's ends.** It lingers at the ends; wrapping is how the old walk moved the loop, and it keeps the drift uniform.
- **WALK on MOTION** (the old WALK's param). MOTION is bipolar and SCRUB's, STRETCH's and WAVE's speed; a note-on step on it would have made every old LOOP Sound with a walk sound different in a new way. A param of its own makes WALK 0 bit-identical by construction.
- **TRON's LENGTH as the window's share, run-out fixed at 8 s.** One LENGTH law throughout, but no old TRON Sound would sound the same. TRON keeps its tape law once through.
- **FADE as one control for the seam and the key-up.** It would merge XFADE and FADE, which old Sounds hold apart, so none would be bit-identical.
- **Seven DIRECTION × TRON machines in MODEL.** That is what the owner found confusing (0178's UAT).

## Consequences
- **Old Sounds sound the same.** `chimera-desktop/tests/fixtures/tape_legacy.txt` holds 48 Sound files written at origin/tape-streaming 0d026a9a, before PLAY (every factory TAPE Sound, and ONE, LOOP, PING and TRON Sounds with the params each never read moved), and their render hashes through the whole instrument. `tape_legacy_test` decodes each now and renders it: all 47 marked `same` are bit-identical. The one that isn't is a LOOP whose old WALK (MOTION) drifted it in time: the owner redefined WALK, and an old Sound loads safely without it (no-legacy-patch-compat rule). The audio goldens (`tape_goldens_match`, the REVERSE replay) are unchanged.
- **Factory TAPE Sounds** are re-pointed to PLAY in `recipes.toml` (MODE ONE as FORWARD, TRON BOWED as PLAY FORWARD with TRON on and the TRON machine's FADE, TAPE and WARBLE written out). Their source CRCs are unchanged; the set's file CRC is re-pinned (`factory_sounds_save_as_loaded`, `0x0386F811`), because the files now carry DIRECTION's codes. Each, built now, renders as its file from before did (`factory_sounds_on_play_render_as_before`).
- **RAM:** `TapeEngineParams` keeps its size (WALK a byte where `KeptMode` was, and two one-byte enums in padding). The deck grows by 8 B a voice (WALK's draws and TRON's run-out).
- **Flash:** BANK_DELTAS.

## Sources
- The owner, 2026-10-21 (via the coordinator). Memory `tape-machines-nudge`. Elektron Digitakt II manual (Playback modes), Octatrack manual OS 1.25 (random start), Mellotron M400 (8 s strips, pressure pad).
- `chimera-engine/src/dsp/tape_engine/{params,machine,walk,mod}.rs`, `chimera-engine/src/storage/codes.rs`, `chimera-core/src/ui/{part_page,view,held,cc}.rs`, `chimera-core/src/ui/tape_strip/raster.rs`, `chimera-engine/src/tape_glyph/`.
- Tests: `chimera-core/tests/tape_play_test.rs`, `chimera-desktop/tests/tape_legacy_test.rs`, and the TAPE suites.
- Listening page: (private notes).
