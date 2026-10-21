# Kobol A/B: measures (`kexp ab`)

(The lab's output, 2026-10-21. The jobs are `takes::{drive_job, sweep_job, phrase_job, selfosc_job}`; the rows these tests check are in `src/ssm2040_kobol/tests.rs`.)

Sources: ["kobol", "circuit", "expc", "exp", "rev2"]. Level-matched per take by RMS over the louder half of 0.1 s frames (the drive series: one gain per source, from drive_f, so the steps stay).

| take | expc vs kobol (shape dB) | circuit vs kobol | exp vs kobol | rev2 vs kobol | expc vs circuit: LSD · null | expc vs exp: LSD · null | exp vs circuit: LSD · null |
|---|---|---|---|---|---|---|---|
| drive_q | 3.81 | 3.82 | 3.82 | 3.80 | 0.93 · -1.2 | 2.28 · -12.8 | 7.98 · -1.1 |
| drive_h | 4.03 | 4.00 | 3.97 | 3.93 | 0.56 · -4.1 | 0.97 · -19.3 | 2.85 · -4.1 |
| drive_f | 4.22 | 4.18 | 4.28 | 4.27 | 0.38 · -12.1 | 0.27 · -24.9 | 0.61 · -11.7 |
| track2 | — | — | — | — | 0.72 · +3.2 | 0.67 · +1.5 | 3.13 · +2.6 |
| phrase_bass_vca8 | — | — | — | — | 3.22 · +3.0 | 3.22 · +3.0 | 3.92 · +3.1 |
| phrase_bass | — | — | — | — | 1.13 · -4.4 | 0.53 · -8.1 | 1.05 · -3.5 |
| phrase_acid_vca8 | — | — | — | — | 7.81 · +2.9 | 2.41 · +3.1 | 5.68 · +3.0 |
| phrase_acid | — | — | — | — | 0.59 · -10.4 | 0.24 · -27.3 | 0.74 · -10.4 |
| sweep1 | 5.16 | 5.34 | 5.08 | 5.13 | 0.50 · -12.4 | 0.92 · -12.9 | 1.19 · -9.6 |
| sweep_res5 | 5.35 | 5.46 | 5.27 | 5.28 | 1.33 · -4.4 | 1.17 · -14.8 | 0.69 · -4.4 |
| sweep_res10 | 11.47 | 11.12 | 9.26 | 8.67 | 1.06 · +2.8 | 2.79 · +3.2 | 1.74 · +2.6 |

## Drive linearity, shape change re ¼ (H1's step removed): peak · top · worst (dB); and H1 step ¼→full

- kobol: ½ -0.00 · -0.00 · 0.15; full -0.00 · -0.00 · 0.37; H1 ¼→full +17.27 dB
- circuit: ½ -0.00 · -0.00 · 0.02; full -0.00 · -0.00 · 0.08; H1 ¼→full +17.27 dB
- expc: ½ -0.00 · -0.00 · 0.01; full -0.00 · -0.00 · 0.07; H1 ¼→full +17.27 dB
- exp: ½ +0.00 · -0.00 · 0.03; full +0.00 · -0.00 · 0.29; H1 ¼→full +17.28 dB
- rev2: ½ +0.00 · -0.00 · 0.02; full +0.00 · -0.00 · 0.08; H1 ¼→full +17.27 dB

## Self-oscillation (track2), per source: notes, level spread (dB), mean H2 / H3 (dBc), pitch vs the Kobol's note (cents, mean |·|)

- kobol: 15 notes, spread 0.7 dB, H2 -39.3, H3 -43.6, pitch 0.0 c
- circuit: 15 notes, spread 0.8 dB, H2 -38.7, H3 -47.1, pitch 1.2 c
- expc: 15 notes, spread 1.9 dB, H2 -37.7, H3 -54.2, pitch 0.0 c
- exp: 15 notes, spread 0.1 dB, H2 -150.6, H3 -54.6, pitch 0.0 c
- rev2: 15 notes, spread 0.3 dB, H2 -150.0, H3 -59.7, pitch 0.0 c

## Phrases: spectral-centroid error vs the Kobol (octaves rms over frames within 40 dB of the take's top)

- phrase_bass_vca8: circuit 0.39 · expc 0.37 · exp 0.38 · rev2 0.38
- phrase_bass: circuit 0.39 · expc 0.39 · exp 0.39 · rev2 0.39
- phrase_acid_vca8: circuit 1.86 · expc 1.89 · exp 1.86 · rev2 1.86
- phrase_acid: circuit 0.42 · expc 0.42 · exp 0.42 · rev2 0.42

## Sweeps' bass: H1 over the last 2 s re sweep1's (dB)

- sweep_res5: kobol: -3.3 · circuit: -10.4 · expc: -9.8 · exp: -4.2 · rev2: -9.9
- sweep_res10: kobol: -6.9 · circuit: -18.8 · expc: -15.8 · exp: -7.1 · rev2: -14.2
