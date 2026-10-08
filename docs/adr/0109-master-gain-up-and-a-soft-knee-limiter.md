# 0109. The master gain goes up 11 dB and the limiter gets a soft knee

- **Status:** Accepted (owner, 2026-10-08)
- **Deciders:** owner (#411); firmware
- **Supersedes:** [0058](0058-a-loudness-reference-modal-matches-algo-init.md)
  (its reference, already moved by [0063](0063-levels-at-the-factory-median.md))
  and, in part, [0063](0063-levels-at-the-factory-median.md) (the
  reference's absolute level, −22 LUFS) and
  [0050](0050-gain-staging-trim-and-final-limiter.md) (the 1/√8 trim, the
  hard-kneed limiter)
- **Relates to:** ADR 0055 (the master tape stage), #411, #193

## Context
#411: a single note sits at −20 to −28 dBFS, so normal play uses a sliver
of the range. #193 asked for a limiter on the instrument path; ADR 0050
added one, at −1 dBFS, behind a 1/√8 (−9 dB) trim that makes eight summed
voices land where one did.

Measured through the real `Instrument` (`loudness_test::loudness_table`),
P1, held 1 s, dBFS, before this ADR:

| Sound | C4 v100 peak | RMS | 4-note chord peak |
|---|---|---|---|
| ALGO INIT | −22.9 | −25.3 | −11.0 |
| MODAL INIT (STRING) | −17.6 | −31.4 | −6.1 |
| TAPE INIT, LOOP, a Chimera source | −13.2 | −25.4 | −3.2 |
| factory Sounds (8) | −26.2 to −14.8 | −34.9 to −19.6 | −16.2 to −3.4 |
| six Parts, 24 notes v127, sends 0.6, FX on | | | −1.0 (5.8 dB of limiting) |

A single note's median peak is −19 dBFS; the dense scene already reaches
the ceiling. The plucks (MODAL, TAPE, SQR BASS) have crests 10 to 14 dB
over ALGO INIT's, which is why equal-loudness INITs differ so in peak.

## Decision
- **The master gain is 1.25 (+1.94 dB), where the trim was 0.354
  (−9.03 dB): +10.97 dB.** `limiter::OUTPUT_GAIN`, the limiter's rest
  gain, a constant. It lifts ALGO INIT's C4 at velocity 100 from −22.9 to
  **−11.9 dBFS peak** (RMS −25.3 to −14.3), the low end of the target
  −12 to −8: ALGO INIT is the lowest-crest INIT, so every other sound
  lands hotter (MODAL INIT −6.6, TAPE INIT −3.6, SQR BASS −4.4) and the
  limiter, not the player, takes the difference. A higher gain would put
  the plucks' single notes into the limiter.
  - ADR 0063's reference follows: ALGO INIT's C4 at velocity 100 reads
    −11.0 LUFS (`REFERENCE_LUFS`), the factory median plus the gain. Every
    Sound and INIT keeps its level relative to the others; nothing before
    the limiter moves (`PRE_LIMITER` goldens unchanged).
  - **Never snap:** the gain is a constant folded into the limiter's gain
    average, whose ramps (the compressor's `ramp_gains`) carry it; no
    parameter moves it at runtime.
- **The limiter's ceiling is a soft knee.** Same stage, same place (last,
  linked across pairs and sides, one block of lookahead = 1.33 ms, instant
  attack within it, 50 ms release, no sample past −1 dBFS). The gain it
  asks of a chunk is now `knee(o) / o` for `o = peak · OUTPUT_GAIN`:
  `o` up to `KNEE` (0.5, −6 dBFS) passes unchanged; over it the output
  follows `KNEE + S·d/(S + d)` with `d = o − KNEE` and `S = AIM − KNEE`.
  The slope is 1 at the knee and falls to 0 toward `AIM`, so the curve is
  continuous in value and slope and never passes the ceiling
  (`the_knee_is_monotonic_smooth_and_bounded`). It was a hard step at the
  threshold before: a chord that ducked 3 dB did so from the first sample
  over.
- **Cost: one divide and four multiply-adds per over chunk** (a chunk is
  4 samples), replacing the old chunk's one divide: under 2 cycles per
  sample at the worst (estimated from the instruction count, not benched,
  no hardware) and nothing at rest (the over test is the same integer
  compare). The stage keeps ADR 0050's benched 75 cycles per sample (MIX
  223 against 148) plus that.
- **Why not the master tape (ADR 0055).** It is a coloured effect with a
  drive and mix for the player, and ADR 0055 took it off the chain for its
  bus cost; a limiter's job is the opposite: transparent below its knee
  and bounded above it. The tape's clip would also be level-dependent
  before the limiter, moving every FX golden.
- **No pumping on a sustained chord.** Held ALGO INIT chords, four notes
  at v100 and eight at v127, take 2.5 and 5.8 dB of reduction; the level
  out varies in time no more than the level in plus 0.15 dB (21 ms
  windows; `a_sustained_chord_does_not_pump`).
- **Pinned by** `loudness_test` (a lone ALGO INIT note −12.5 to −8 dBFS;
  #193's INIT through REV 0.5 and at MORPH B, and the six-Part dense scene,
  all at or under the ceiling; silence stays exactly silent) and by
  `gain_staging_test` and `param_sweep_test`, re-based on the gain.

After, the same table:

| Sound | C4 v100 peak | RMS | 4-note chord peak |
|---|---|---|---|
| ALGO INIT | −11.9 | −14.3 | −2.9 (2.8 dB of limiting) |
| MODAL INIT (STRING) | −6.6 | −20.4 | −2.0 (6.9 dB) |
| TAPE INIT, LOOP | −3.6 | −14.8 | −1.7 (9.4 dB) |
| factory Sounds (8) | −15.2 to −4.4 | −23.9 to −8.7 | −5.3 to −1.7 |
| six Parts, 24 notes v127, FX on | | | −1.3 (17 dB at the loudest moment) |

## Alternatives considered
- **Raise the gain to −8 dBFS for ALGO INIT** (+15 dB): every pluck's
  single note would sit in the knee, and a four-note chord would take 8 dB
  or more.
- **A gain per engine:** ADR 0063 already levels the INITs by loudness,
  by ear; the peaks differ by crest, which is the limiter's job.
- **Reuse the master tape stage:** above.
- **A tanh or clipper after the limiter:** a second curve on the bus; the
  owner wants one non-linear stage (ADR 0050).
- **A longer release for smoother chords:** 0.1 s and 0.2 s moved a held
  chord's gain ripple from 0.133 to 0.108 and 0.087 dB and left a
  strike's ducking in the mix for longer. Not worth it.

## Consequences
- Every sound plays 10.97 dB louder at the DAC; saved Sounds do not
  change. A new Sound's C4 is at −12 dBFS where it was −23.
- Chords and dense scenes now live in the knee: the six-Part scene takes 17
  dB at its loudest moment. That is the price of using the range; the
  limiter's release (50 ms) was heard as distortion on STRING v127 chords
  at 15 dB of reduction (ADR 0063); four-note chords now take 3 to 9 dB.
  The owner played it on the unit and approved it by ear (2026-10-08).
- BANK INIT's single note, the peakiest, loses 2.9 dB of loudness to the
  knee, so its post-limiter loudness reads that much under the reference;
  its unlimited loudness is still within ±1 dB (`modal_models_match_the_loudness_reference`
  measures it so). ADR 0063's "may take at most 3 dB of a chord" becomes
  7 dB (`MAX_CHORD_LIMITED_DB`) and STRING's eight-note reduction 15 dB.
- The reverb's hot return (ADR 0050) now limits where it used to pass; the
  reverb is untouched.
- Re-recorded: the `instrument_test` post-limiter goldens. Three tests that
  assumed an unbent output (`fx_send_puts_no_dry_signal_on_pair_1`,
  `out_moves_the_part_to_its_pair`, `everything_before_the_limiter_is_mains_mix`)
  play quieter sends.
- #193 closes: INIT through REV 0.5 and at MORPH B peak under full scale.

## Sources
- #411, #193; ADR 0050, 0055, 0058, 0063.
- `chimera-core/tests/loudness_test.rs`, `common/scene.rs`.
