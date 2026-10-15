# 0143. A param cell eases at the knobs' glide

- **Status:** Proposed
- **Deciders:** owner (the MIDI map's feel on the unit), firmware

## Context
On the unit a single CC 0→127 on WAVE1 read 65 at 306 ms, 120 at 658 ms and 127 only at 1,287 ms (2026-10-15, build a313a12c). The sound was there long before: a host bench of the same jump plays 0.99 of the way by about 100 ms (the 15 ms ramp and the knobs' 30 ms τ, ADR 0139, spec § 5). The cells show `Renderer::anim`, an `AnimatedValue` easing 0.3 of the gap a frame at 20 fps: a τ of about 140 ms, some 850 ms before the number settles, longer when frames drop (a console screenshot holds the loop). Its own doc promised a settle in about three frames.

## Decision
- **The six cells' values (`anim`) and their set values (`set`) ease `KNOB_SPEED` = 0.81 a frame:** 1 − e^(−FRAME_MS / τ) for the knobs' 30 ms τ, so a cell moves as the sound does. A full-range jump shows its end within four frames (200 ms).
- **Still a lerp, never a snap** (CLAUDE.md): the first frame after a jump shows a value between the two.
- Other eases (branch scroll, ORBIT's cursor and sweep) keep their own speeds.

## Alternatives considered
- **Snap the number, ease only the bar:** a snap, against CLAUDE.md's rule, and the two would disagree.
- **Raise the frame rate:** costs the loop redraw time it doesn't have (an 18 ms peak) and still leaves the ease 5× slower than the sound.
- **Ease by elapsed ms, not frames:** right if frames drop, but more code than a constant; the pacer already keeps frames at `UI_FPS`.

## Consequences
- A turn's or CC's number follows within a frame or two; big jumps cross in fewer, larger frame steps (the sound itself never steps: its glide is at block rate, ADR 0107).
- If `GlideMs::KNOB` or `UI_FPS` changes, `knob_speed_is_the_glide_a_frame` fails until `KNOB_SPEED` follows.

## Sources
- `chimera-core/tests/cc_apply_test.rs`: `a_cc_jump_shows_as_fast_as_it_sounds`, `a_swept_end_shows_promptly`; `docs/superpowers/specs/2026-10-13-midi-map-design.md` § 5; ADR 0107, 0139.
