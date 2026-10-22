# 0179. One release a note: PLAY's FADE steps aside when a VCA route holds the voice

- **Status:** Proposed
- **Deciders:** owner (#698, "fix it all"), firmware
- **Amends:** [0170](0170-tape-machines-are-play-kit-tron-reel-scrub-stretch-and-wave.md) (PLAY's FADE as its release)

## Context
The adversarial review (musical finding 3) found two releases in series on LOOP and PING. At key-up PLAY's FADE one-pole (32 ms by default) and ENV 1's release (INIT routes ENV 1 → VCA at 127) both acted, and the engine ended the voice when FADE reached −80 dB, about 295 ms later. AMP RELEASE from 300 ms to 5 s changed nothing. The knob every synth player reaches for first was dead, and the one that worked sat on page 2 under a sampler's name.

## Decision
- `Voice::note_off` tells the engine whether a VCA route holds the voice (`EngineSlot::note_off(ctx, vca)`, the deck's `release_by(vca)`, a door entry of its own).
- With a VCA route, ONE (short of HOLD), LOOP and PING ignore FADE: the route's ENV is the note's one release, and the voice ends when it lets go (`vca_holds`), as every other engine's does.
- With no VCA route, FADE is the release, as before (the key gates the voice, `key_gates`).
- TRON keeps FADE whatever the routes: the pressure pad lifting is the machine.
- The UI dims LOOP's and PING's FADE under a VCA route, the band saying `AMP RELEASE ENDS THE NOTE` (ADR 0186).

## Alternatives considered
- **FADE's default OFF on LOOP and PING.** It leaves the double release in any Sound that sets FADE, and a knob that reads a value it never applies.
- **Drop the ENV 1 → VCA route from INIT.** Then the familiar AMP page does nothing on TAPE, and LOOP would end at key-up through the voice's fade.

## Consequences
- AMP RELEASE shapes every ONE (below HOLD), LOOP and PING tail under a VCA route. Saved Sounds whose tail came from FADE now ring out on their ENV's release: they load safely and sound longer (no-legacy-patch-compat rule).
- `init_tape.snd`'s render pin is re-recorded on purpose.

## Sources
- (private notes) finding 3; #698.
- `chimera-engine/src/dsp/tape_engine/mod.rs` (`release_by`), `dsp/voice.rs`, `dsp/engines.rs`, `tape_engine/proxy.rs`.
