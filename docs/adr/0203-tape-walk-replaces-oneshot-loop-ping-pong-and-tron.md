# 0203. TAPE: WALK replaces ONESHOT, LOOP, PING-PONG and TRON

- **Status:** Proposed
- **Deciders:** owner (2026-10-21: the four machines are "ultimately the same thing"; machine names are verbs; the machine is WALK, not PLAY, which would read as the transport's play; TRON "should just be toggleable"; the drift is random; WALK gets a glyph of its own), firmware (the coordinator's rulings, recorded below)
- **Supersedes in part:** [0170](0170-tape-machines-are-play-kit-tron-reel-scrub-stretch-and-wave.md) (the lineup), [0173](0173-tape-machine-glyphs-are-knob-driven-still-and-live.md) (ONE's, LOOP's, PING's and TRON's glyphs, now WALK's footsteps), [0178](0178-tape-model-lists-one-loop-and-ping-and-deck-holds-the-glyphs-knobs.md) (MODEL's seven machines, their DECK rows, and the DECK-is-what-moves-the-glyph rule for WALK), [0180](0180-tape-machine-defaults-are-musical-from-the-first-turn.md) (ONE's, LOOP's, PING's and TRON's defaults, now WALK's by DIRECTION), [0181](0181-tape-laws-walk-in-seconds-stretch-slow-age-from-the-band.md) (WALK in seconds of source a second) and [0189](0189-tape-params-hold-one-machine-the-codec-splits-it.md) (MODE folded into the machine, `KeptMode`)

## Context
ONE, LOOP, PING and TRON all play one head through the window. They differ in which way it goes, whether it comes round, and whether the tape runs out. MODEL listed them as four machines (0178) beside STRETCH, SCRUB and WAVE, each with its own cells, so the same LENGTH or FADE sat in a different place on each. The owner merged them into one machine and named the machines as verbs: WALK · STRETCH · SCRUB · WAVE.

TRON sounded the same as ONE. It was ONE's path plus a run-out counter of `0.25 · (4·T)^LENGTH` s over a strip of T seconds, so LENGTH 1 ended exactly at END at ROOT, where ONE at HOLD ends too; key up faded it in 32 ms. On the factory TRON BOWED (a 4 s strip) through the whole instrument the two differed only off the root or at key up: a chord let go at 1.5 s was at −59 dB by 1.6 s on TRON and played past 6 s on ONE.

## Decision
- **MODEL lists WALK, STRETCH, SCRUB, WAVE** (`TapeMachine::ALL`, values 0 to 3). The MODEL cell reads WALK, STRCH, SCRUB, WAVE; the model page's header WALK, STRETCH, SCRUB, TRANSWAVE (TRANSWAVE keeps its header name; not renamed here). SLICE (0199) is on hold until PREP: disk code 8 and param ids 17 and 18 stay free for it.
- **WALK's controls**, on 6-cell pages:

  | Page | Cells |
  |---|---|
  | SRC | SOURCE · START · END · VOL (unchanged: START and END are the window) |
  | DECK | MODEL · LENGTH · FADE · DIRECTION · POSITION · DRIFT |
  | WALK (the model page) | TAPE · AGE · WOW · TRON · XFADE · RETRIG |

  - **DIRECTION**: FORWARD · REVERSE · LOOP · PING-PONG, read FWD, REVERSE, LOOP, PING (FORWARD and PING-PONG overrun a cell's 70 px). FORWARD is ONE's path, LOOP LOOP's, PING-PONG PING's. REVERSE is ONE's path with the window flipped: REVERSE with START before END plays exactly as FORWARD with END before START, and REVERSE over END before START plays forward again. END before START still reverses FORWARD and LOOP (#369).
  - **POSITION**: where the run starts. Around a loop it places the loop in the room the window leaves, as LOOP did. Once through it is the start, counted the way it plays (REVERSE counts from END).
  - **LENGTH**: how much plays from POSITION. Around a loop it is LOOP's loop law; once through, the same law over what is left from POSITION, so LENGTH 1 plays to the window's end. With TRON on once through, LENGTH is the tape (below).
  - **FADE**: the note's edge fade, PLAY's and TRON's FADE as before (10 to 500 ms). Once through with TRON off its last step is HOLD: key up is ignored and the note plays to its end. Around a loop it releases at key up and steps aside for AMP under a VCA route (0179). The seam crossfade stays XFADE, on the model page.
  - **DRIFT** (the owner's "random drift"; the coordinator's ruling named the param DRIFT once the machine took the name WALK): at each note-on POSITION steps by a random amount, up to ±DRIFT of the window, from where the last note on that voice left it. Repeats wander through the sample; DRIFT 1 reaches anywhere. The place **wraps** at the window's ends, as the old walk did, rather than reflecting. DRIFT 0 is off: no draw, the place rests at POSITION. Each voice draws from its own xorshift, seeded by its slot (`drift::seed`) apart from its wow's, so a render repeats. The steps are uniform: mean 0, mean |step| DRIFT/2, RMS DRIFT/√3. DRIFT is its own param (id 20, a byte on the knob's 1/128 grid), read at note-on; MOTION no longer moves WALK.
  - **TRON**: OFF · ON. ON is the Mellotron M400's tape, by the coordinator's rule that its run-out be heard whatever LENGTH and the strip: each key has a tape of LENGTH's `0.25 · 32^LENGTH` s, 0.25 to 8 s, the same at every pitch and whatever the strip. Once through, the window plays from POSITION and comes round again, its seam crossfaded by XFADE, until the tape runs out; around a loop or a ping-pong the tape is 8 s. The run-out falls in 2 ms. Key up lifts the pressure pad through FADE, always, even under a VCA route. HOLD means nothing under TRON, so switching TRON on with FADE at HOLD writes TRON's 32 ms. TRON refuses a zone set, as before; zones are not built.
  - **XFADE** is heard around a LOOP and on TRON's round; **RETRIG** only once through with TRON off. Elsewhere each is dimmed, its reason in the band: `ONLY LOOP AND TRON CROSSFADE`, `CUTS ONLY ONCE THROUGH`.
- **Defaults.** A MODEL turn onto WALK writes its cells, keeps DIRECTION and switches TRON off. LENGTH and FADE take DIRECTION's: once through, LENGTH 1 and FADE HOLD, with ENV 1's hit (0180); around a loop, LENGTH 0.8 and FADE 32 ms. A DIRECTION turn between once through and a loop writes the same, as a MODEL turn between ONE and LOOP did; within a side it writes nothing. The neutral and INIT TAPE are WALK in LOOP, TRON off, DRIFT 0.
- **Streaming.** REVERSE and PING-PONG need RAM (`needs_ram`), as END-before-START and PING did; a DIRECTION turn onto one on a streamed source is held, dimmed with `… NEEDS RAM · STREAMED`, nothing written (0186's rule, now for DIRECTION too). TRON streams; its round lands back on START's head. A FORWARD note started past START by POSITION or DRIFT on a streamed source starts cold, as a moved START does.
- **The codec.** Codes are append-only; old codes are mapped on load, never reused.
  - MODEL: WALK 0 (PLAY's code, ident `LOOP` frozen), TRANSWAVE 3, SCRUB 4, STRETCH 5. TRON's 6 is retired, beside REVERSE 1, PING-PONG 2 and ONESHOT 7. 8 is SLICE's.
  - DIRECTION keeps MODE's id (15) and ident `MODE`: LOOP 1, PING 2 (codes and meaning kept), FORWARD 3, REVERSE 4. MODE ONE (0) is retired: ONE read neither POSITION nor LENGTH, and FORWARD does.
  - TRON is id 19 (OFF 0, ON 1), DRIFT id 20.
  - `translate_v1`: MODEL TRON loads as WALK FORWARD with TRON on, POSITION 0 (TRON never read it), LENGTH kept. MODE ONE on WALK loads as FORWARD with POSITION 0 and LENGTH 1; off WALK the code was only kept, so it becomes FORWARD alone. ONESHOT, PING-PONG and REVERSE load as before.
  - In memory `TapeEngineParams` holds `machine`, `direction`, `tron` and `drift`; the deck's path is `Way` (ONE, LOOP, PING, TRON, SCRUB, STRETCH, WAVE), from which the costs, the glyph and the strip read. TRON is billed as LOOP (450), the path it now takes.
- **The glyph.** WALK has its own: footsteps (`StepStyle::Prints`, heel, ball and toe). FORWARD walks the run POSITION and LENGTH cut from the window left to right, REVERSE right to left, LOOP round a track LENGTH sizes, PING-PONG there on one lane and back on the other. DRIFT scatters each step by a fixed hash of its place, FADE fades the last steps out, and TRON marks its tape's end (a leader's edge) and greys the steps past it. Two other styles, boot treads and a dotted trail, are drawn for the owner to pick from (`render_style`; the listening page). The inputs are quantised set values, with DIRECTION, TRON and DRIFT added to `TAPE_GLYPH_PARAMS`; the key has no frame term, so nothing redraws at idle and a dirty frame is a full one. ONE's match, LOOP's wheel, PING's court and TRON's blind (the "window with a pull") stay in the code, unused by any machine now.
- **Exceptions to 0178's DECK rule** (which holds for STRETCH, SCRUB and WAVE): on WALK every DECK cell moves the footsteps, FADE only their last steps; TRON, on the model page, marks the tape's end; START and END are the window strip's.

## Alternatives considered
- **DRIFT bipolar, a fixed step either way.** The coordinator's first ruling, replaced by the owner's random drift.
- **DRIFT reflecting at the window's ends.** It lingers at the ends; wrapping is how the old walk moved the loop and keeps the drift uniform.
- **DRIFT on MOTION** (the old WALK's param). MOTION is bipolar and SCRUB's, STRETCH's and WAVE's speed; a param of its own makes DRIFT 0 bit-identical by construction.
- **TRON's old law, LENGTH over the strip.** Every old TRON Sound would stay bit-identical, but at LENGTH 1 the run-out falls on END at ROOT, where FORWARD ends anyway: the toggle would be heard only at key up.
- **TRON's tape law with the strip played once.** A short strip ends at END before its tape: the run-out would go unheard on most factory sources (0.6 to 4 s).
- **FADE as one control for the seam and the key-up.** Old Sounds hold XFADE and FADE apart, so none would be bit-identical.
- **Seven DIRECTION × TRON machines in MODEL.** What the owner found confusing (0178's UAT).

## Consequences
- **Old Sounds.** `chimera-desktop/tests/fixtures/tape_legacy.txt` holds 48 Sound files written at origin/tape-streaming 0d026a9a, before WALK (every factory TAPE Sound, and ONE, LOOP, PING and TRON Sounds with the params each never read moved), and their render hashes through the whole instrument on a phrase with key-ups. `tape_legacy_test` decodes each now and renders it: 45 are bit-identical, every factory Sound among them (TRON BOWED too: the phrase lets each key go before its strip ends). Three differ, each for a reason the fixture names: a LOOP whose old WALK (MOTION) drifted it in time (DRIFT steps it at each note instead), and two TRON Sounds whose tape now outlasts their strip (LENGTH 0.5 over a 4 s strip, and TRON on a 0.6 s source). They load safely (no-legacy-patch-compat rule). The audio goldens are unchanged but TRON's two, re-recorded.
- **Factory TAPE Sounds** are re-pointed to WALK in `recipes.toml` (MODE ONE as FORWARD; TRON BOWED as WALK FORWARD with TRON on, the TRON machine's FADE, TAPE and WARBLE written out). Source CRCs are unchanged; the set's file CRC is re-pinned (`factory_sounds_save_as_loaded`), the files now carrying DIRECTION's and TRON's codes. Built now, each renders as its file from before did (`factory_sounds_on_play_render_as_before`).
- **RAM:** `TapeEngineParams` keeps its size (DRIFT a byte where `KeptMode` was). The deck grows by 10 B a voice (DRIFT's draws, TRON's run-out, two flags). `MAX_SOUND_COST` rises 2,656 → 2,672 B for the two params.
- **Flash:** BANK_DELTAS.
- `tape_machines_audio_pin_test`, on SLICE's branch, names ONE, LOOP, PING and TRON. When SLICE resumes, its pins load as WALK and hold where this ADR claims bit-identity.

## Sources
- The owner, 2026-10-21 (via the coordinator). Memory `tape-machines-nudge`. The TRON findings, (private notes). Elektron Digitakt II manual (Playback modes), Octatrack manual OS 1.25 (random start), Mellotron M400 (8 s strips, pressure pad).
- `chimera-engine/src/dsp/tape_engine/{params,machine,drift,mod,tron}.rs`, `chimera-engine/src/storage/codes.rs`, `chimera-engine/src/tape_glyph/`, `chimera-core/src/ui/{part_page,view,held,cc}.rs`, `chimera-core/src/ui/tape_strip/raster.rs`.
- Tests: `chimera-core/tests/{tape_walk_test,tape_walk_glyph_test}.rs`, `chimera-desktop/tests/tape_legacy_test.rs`, the TAPE suites.
- Listening page: (private notes).
