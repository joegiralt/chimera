# 0053. The delay's WOW becomes MECHANICS: a slow wow and an irregular flutter

- **Status:** Proposed
- **Deciders:** owner; firmware

## Context
The tape delay's one modulation knob, WOW (id 2, ident `WOW`), drove two
sines: 0.5 Hz at 0.7 of `WOW × 20` samples and 6 Hz at 0.3. The 6 Hz part
was a perfectly regular vibrato: at the default 0.15 it was about 0.9 cents
RMS and read as barely there, and at full it was an obvious, steady wobble
rather than tape. The owner wants the El Capistan's simplicity: one knob, no
crinkle, dropouts or glitches (the reverb's GRIT already covers lo-fi
dirt).

## Decision
- **One knob, relabelled.** Param id 2 keeps its field (`wow_flutter`) and
  its frozen disk ident `WOW` (ADR 0045); its label is now `MECHANICS`,
  short `MECH`. The disk-code golden's keys are unchanged, only its readable
  column. A cell whose label outruns its bar shows the spec's short form, so
  the cell reads MECH and the focus band MECHANICS.
- **What it does** (`dsp::delay::Transport`), every depth linear in the
  knob `k`:
  - the slow wow, as before: a 0.5 Hz sine, ±14 samples at full (1.6 cents
    peak);
  - a flutter of ±8 samples at full: half a 9 Hz capstan sine, half
    band-limited noise. The noise is the repo's xorshift32
    (`dsp::xorshift_noise`, fixed seed) through two one-poles at 10 Hz,
    scaled to 0.4 RMS of its ±1 range and limited to ±1.
  - Measured pitch deviation at TIME 10 ms (independent of TIME):
    k 0.15: 1.0 cents RMS (was 0.9), 3.4 peak; k 0.5: 3.5 RMS; k 1: 6.9
    RMS, 15.5 at the 99th percentile, 22.7 peak. Full is a worn machine,
    about 0.4 % RMS speed deviation (unweighted): clearly heard, still
    musical.
- **k 0 is today's delay at WOW 0, bit for bit**: both depths are exact
  zeros.
- **The read stays in the line.** 500 ms (24,000 samples) plus 14 + 8 and
  the interpolator's second tap fit `MAX_DELAY_SAMPLES` (24,064); a const
  assertion pins it. No memory change beyond the transport's 12 bytes.
- **Cost.** About +15–25 cycles per sample (xorshift, two one-poles, a
  clamp, a blend); the capstan sine costs what the 6 Hz one did. Not benched.
  `FxBus::COST` stays at its bench reading until the next bench.

## Alternatives considered
- **A separate FLUTTER knob.** More control, but two knobs for one tape
  character; the owner chose one.
- **Media events (dropouts, splice bumps) on a MECHANICS knob, after the
  Strymon Volante.** Dropped: crinkle and dirt are left to the reverb's GRIT.
- **A squared curve.** Keeps low settings clean, but moves the 0.15
  default far from today's sound; linear keeps it within 0.2 cents RMS.

## Consequences
- Non-zero MECH sounds different from WOW: irregular rather than a steady
  vibrato. No factory Sound or Performance uses the delay audibly (the
  default MIX is 0), so none changes; the FX goldens for the delay are
  re-recorded.
- The delay has no per-event state or new param; a multi-head delay with
  its own transport controls is #208's future, and would supersede this.

## Sources
- Strymon El Capistan and Volante manuals (the knob's scope, not code).
- https://github.com/joegiralt/chimera/issues/208
- `chimera-core/tests/delay_mechanics_test.rs`
