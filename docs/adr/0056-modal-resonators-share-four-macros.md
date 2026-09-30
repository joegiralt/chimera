# 0056. Modal's resonators share four modulatable macros; loops are stable by construction

- **Status:** Proposed
- **Deciders:** project owner
- **Supersedes in part:** [0010](0010-modulation-targets-are-honest.md)
  (Modal settings read at note-on: the four macros are live),
  [0040](0040-eight-voices-modal-strings-to-g1.md) (the line's length),
  [0042](0042-voice-pitch-is-a-matrix-destination.md) (`ModalEngine::PITCH`),
  [0054](0054-sympathetic-strings-from-a-shared-pool.md) (the voice's
  size assert, `COST_SYMPATHETIC`)
- **Accepted by:** the owner, after the Modal 2 step A ship flash (plan
  task 12); the costs below are host estimates until the chip bench
  (https://github.com/joegiralt/chimera/issues/242)

## Context
The owner's bench report (#191) and the survey behind the Modal 2 step A
spec found the string loops unsafe. FDBK added `filtered · fdbk · 0.3`
inside the loop, so any FDBK above about 0.012 made the loop gain exceed 1
(the default, 0.2, did). The string then grew until a ±1.5 clamp, which
can latch DC. No string loop had a DC blocker, so Bowed's stick-slip and
any asymmetric excitation could drift off zero.

The strings were also tuned to whole samples (up to 54 cents off at G1 and
84 at C7, #163), BODY was a half-delay comb in the loop that sounded low
notes an octave up (#10), the ensemble was a fixed chorus (#50), a note-off
did not release a string (#51), and the model's settings were read only at
note-on, so nothing could modulate a string's character.

## Decision

### Loops
- `modal::loop_parts::LoopGain` is a string loop's gain per pass. Its
  constructors clamp to `[0, 0.99999]` (`LoopGain::MAX`, a T60 of
  690,000 / f0 s: past DAMP's 20 s to 34 kHz), and NaN gives 0. It was
  0.9995 (Task 18), which held C6 to 13 s and G6 to 9.
  It is the only gain any loop multiplies by: STRING, the SYMP main string,
  each halo string and BOWED. On STRING and the SYMP main string it
  multiplies the loop's sample after every in-loop stage, so none bypasses
  it. Every other in-loop stage has a gain of at most 1.
- FDBK and its ±1.5 clamp are gone.
- `modal::loop_parts::DcBlocker` is a one-pole high-pass at `DC_HZ = 10`,
  `y = g·(x − x1) + r·y1`, `r = e^(−2π·10/fs)`, `g = (1 + r)/2`, gain 1 at
  Nyquist and below 1 elsewhere. There is one per voice, on the model's
  output, outside the loop: STRING's string, SYMP's main-and-halo mix and
  BOWED's bow. It is reset at note-on. The loop needs none: with
  `LoopGain` below 1, DC can't grow or latch, and decays with the ring.

### Tuning
- A string's loop is a ring read `delay` samples behind the write, plus
  `loop_parts::Allpass1`, `(η + z⁻¹)/(1 + η z⁻¹)`, carrying the fraction.
- `split(period, other, w)` takes the other in-loop stages' phase delay at
  f0 (`other`) off the period, puts the whole part on the line
  (`floor(d − 0.5)`, at least `MIN_LINE = 2`) and the rest, in
  `[0.5, 1.5)`, on the allpass. `eta_for` inverts the allpass's phase delay
  exactly: `θ = ω(1 − frac)/2`, `η = sin θ / sin(ω − θ)`. `set_period`
  clamps the fraction to `[0.5, 1.5]` when the line clamps.
- The loop low-pass is the linear-phase three-tap
  `c/2·(x[d−1] + x[d+1]) + (1 − c)·x[d]`, centred on the line, so it adds
  no delay. BOWED's bridge line's input runs through the same allpass,
  and its two ends' filters' four samples come off the period
  (`ENDS_DELAY`).
- The fundamental of STRING and the SYMP main string lands within
  0.05 cents from G1 to C7, and partials 2 to 4 are harmonic to it at G1
  and C3.
- The ring is 981 samples: a line of at most 979 (`MAX_LINE`), plus the
  low-pass's two taps. G1's 979.6-sample period takes a 979-sample line
  and 0.59 on the allpass. This supersedes ADR 0040's 984-sample ring in
  part.

### Macros
- The Modal chain is EXC · RES · FLT · AMP · MOD, and a Part's Modal
  chain opens on EXC (the owner's decision, 2026-09-30).
- EXC holds the model's exciter. Its cells follow MODEL through
  `page_cells(ModalPage::Exciter, _)`, and its header is named for the
  exciter (`EXCITER_NAMES`): PLUCK (STRING, SYMP) EXCITE and COLOR; STRIKE
  (BANK) EXCITE and BURST; BOW (BOWED) FORCE and SPEED. Its map node reads
  EXC.
- The home page (RES) is MODEL, STRUCTURE, BRIGHT, DAMP, POS and SPACE
  (the Part's reverb send). The model page, named for the model (STRING,
  SYMP, BANK, BOWED), holds BODY, ENS DEPTH, ENS RATE, ENS MIX, COUPLE,
  HALO and MODES (`page_cells(ModalPage::Model, _)`); EXCITE left it for
  EXC. Its cell labels are STRUCT, ENS.D, ENS.R and ENS.M. FDBK, STIFF,
  INHARM, DECAY, the E.* fields, `ks_excitation`, `ks_color`,
  `bow_velocity`, `bow_force` and `note` leave `ModalParams`, and their
  disk codes are retired.
- COLOR (id 19), BURST (20), FORCE (17) and SPEED (18) are not
  modulatable, like the model page. COLOR and BURST are read at note-on;
  FORCE and SPEED every block, eased, so turning them moves a held bow
  (the controller's ruling: the owner's complaint was values that didn't
  move). COLOR is the
  pluck's smoothing passes, `⌊(1 − COLOR)·7⌋`, the old `ks_color` law
  (the old hidden 0.8 is one pass). BURST is the strike's noise burst,
  `2 + 4·BURST` ms; EXCITE is the strike's level alone.
- STRUCTURE, BRIGHT, DAMP and POS are read every block from the voice's
  modulated params, into `modal::Macros`; the loops read `Macros`, never
  the params. Each block eases them `EASE` = 0.3 of the way to the block's
  values. This supersedes in part ADR 0010's "Modal settings are read only
  at note-on", for these four. The model page stays note-on and
  unmodulatable.
- A note's first block takes its modulated macros whole, since nothing
  sounds yet: the macros, POS's pluck shape, SYMP's chord and STRING's
  stiffness all start at the routed value. POS shapes the pluck from the
  first block (`KsString::excite` fills the noise at note-on, `shape`
  combs and smooths it before the first tick), so VEL and NOTE routes
  reach it; with no route this is bit-identical to shaping at note-on.
  BANK reads POS live.
- POS is β, the pluck's or strike's place as a fraction of the string
  from its end: `0.5·POS` (`params::beta` from `END`), the end at 0 and
  the middle at 1. A pluck at β and at 1 − β is the same, so the knob
  once went out and back: POS 0 and 1 rendered bit for bit alike on
  STRING, SYMP and BANK (objective QA, Task 18). BOWED keeps its own
  start, `BOW_END` 0.06 (below). On a pluck, `string::comb` runs round
  the line once, in place (each cycle of `i → i + n` walked, its first
  sample kept for its last), `(x[i] + x[i + n]) / 2` for
  `n = round(β·len / 2)`: harmonic k is `|cos πkβ/2|` of the noise's, the
  pluck's nulls at the odd multiples of 1/β. It passes the noise whole at
  the end, as POS 0 always did, so INIT is unchanged; nulls the 2nd,
  6th, 10th… at the middle; and never nulls the fundamental. The 2nd
  over the 1st, `cos πβ / cos πβ/2`, falls at every step of POS. The old
  comb, `period·POS` and one-sided, nulled the fundamental at POS 0.5
  and stopped combing once its delay reached the line. A pluck's
  partials still carry its noise's own spectrum, a dB or two each: the
  loop's period is not the line's (the allpass fraction and the
  dispersion's four samples). Two ends were tried and not taken: the
  pluck comb `|sin πkβ|` from β 0.06, Bowed's start, is thin there, and
  even with its energy kept INIT's C4 fell 3.1 dB on SYMP; Rings'
  weights from β 0.06 took 2.3 dB off BANK's. ADR 0058 holds every INIT
  within 1 dB of the reference, and its gains wait on the owner.
- BANK's POS is Rings' cosine weights at β (`cos² πβ` the 2nd mode's,
  the 1st's always 1). A moved POS glides the weights' coefficient over
  the block, a step a sample (`CosineOsc::glide`), landing on it
  exactly; a step once scaled every sounding mode at a block's edge (a
  POS jump clicked 13× the steady state once POS 0 and 1 differed). A
  note's first block takes it whole.
- DAMP on a string is seconds, not a per-pass loss:
  `T60 = 0.05·400^DAMP` (50 ms to 20 s), the fundamental's at every
  pitch. `loop_parts::damped` turns it into each loop's gain at its own
  f0, and makes up the loop low-pass's loss there, `lp·(1 − cos ω0)`, so
  the fundamental's gain a pass is DAMP's. Before Task 18 that loss came
  on top, growing as f0³: at DAMP 1 C5 rang 8.5 s, C6 2.5 and G6 0.8, and
  INIT's G6 and C7 were silent within 0.5 s.
- The low-pass takes at most `LP_SHARE` = 0.5 of DAMP's loss a pass at
  f0; where BRIGHT's side taps would take more (a long ring on a high
  note: above about B4 at INIT's DAMP, 440 Hz at DAMP 1, 1.2 kHz at DAMP
  0.5), they are cut to that share, and the loop is brighter than BRIGHT
  alone would make it. It is stable by construction: every other
  frequency's gain is the loop's times `1 − lp·(1 − cos ω)`, highest at
  0 Hz, and there under `g / (1 − LP_SHARE·(1 − g))` for DAMP's `g`, below
  one; a 0 Hz ring lasts at most twice DAMP's, and a pluck carries none
  (`shape` takes its mean out). The make-up is `1 / (1 − loss)`'s series
  to `loss⁴`, a hair under it (`loss` is under 0.01 to C7), and the gain
  an `expf`, not a `powf`: the block costs no more than before. SYMP's
  halo strings run the same law at twice the T60. A released string's
  ramp starts from the made-up gain. The bank's DAMP is its old DECAY,
  law unchanged; a lifted bow's ends are plain delays, with nothing to
  make up.
- BRIGHT runs dark to bright on every model. On STRING, SYMP and BOWED
  that inverts the old field (brightness was damping): INIT's BRIGHT is
  `1 − 0.7`, and a v1 patch's BRIGHT flips on load. The low-pass's side
  taps are `c·(1 − c)` for `c = 0.05 + 0.45·(1 − BRIGHT)`, the old
  two-point average's loss at low frequencies, so old patches keep their
  tone. SYMP's halo low-pass reads `1 − 0.7·(1 − BRIGHT)`, as the old halo
  damping was 0.7× the main string's.
- A control the model ignores is dimmed, with its matrix column, from one
  table (`modal::reads`). BOWED dims STRUCTURE.
- MODES latches at note-on; a sounding bank note keeps its modes and is
  billed for them (`ModalEngine::playing_cost`) until it ends.
- The bank drops the modes past Nyquist, as Rings does: the count stops at
  the first mode at or over 0.49 of the sample rate. Its level is otherwise
  as before, with the tanh as its limiter; a full strike at BRIGHT 1
  drives it 10 to 150× over unity, gain staging left to the ear
  (https://github.com/joegiralt/chimera/issues/231). Until then BANK's
  STRUCTURE and BRIGHT are held to a relative click rule, DAMP and POS to
  the strict one.

### STRING: dispersion and BODY
- `modal::dispersion::Dispersion` is STRUCTURE's stiffness on STRING: four
  first-order allpasses in the loop, after the low-pass and before the
  tuning allpass, the gain after them all. There is no new buffer.
- Each stage's DC delay is `1 + 1.15·s/(0.15 + s)·0.1·period/4` samples:
  Rings' `ap_gain` curve, `s/(0.15 + s)`, scaled by the period as Rings
  scales its allpass line. It is a plain delay at STRUCTURE 0, and the
  chain's DC delay is capped at half the period. At STRUCTURE 1 it moves
  the 8th partial of C3 24 cents (about 21 at G1, 30 at C4, 75 at C6),
  sharper up the keyboard, as on piano wire; Rings' fixed coefficient moved
  it 0.95 cents.
- The chain's phase delay at f0 comes off the line (`split`'s `other`), so
  the fundamental holds within 0.05 cents from STRUCTURE 0 to 1. At G1 and
  STRUCTURE 1 the line is about 880 samples.
- Below G1 the stiff loop clamps to its longest period, 984 samples: the
  979-sample line, one sample of fraction and the chain's four at
  STRUCTURE 0. Notes below G1 all play about 8 cents under G1, at every
  STRUCTURE.
- A moved STRUCTURE re-splits the loop once a block. The chain glides: its
  DC delay, and so the line, moves at most `DISP_SLEW` = 2 samples a
  block, about 65 ms end to end at G1. A note's first block snaps it.
- SYMP's main string has no chain: SYMP's STRUCTURE tunes the halo only.
- `modal::body::Body` is BODY: three fixed resonances on the output,
  outside the loop, (102 Hz, Q 3, 1), (236 Hz, Q 4, 0.7) and (517 Hz, Q 3,
  0.5), each a peak-normalized band-pass; the output is
  `(x + b·Σ gᵢ·bpᵢ(x)) / (1 + b/2)`, peaking at about 1.4. It colours
  without transposing. On SYMP it colours the main-and-halo mix. BODY
  latches at note-on.

### Bowed
Bowed is J. O. Smith's digital-waveguide bowed string (plan Task 14, the
owner's decision on
https://github.com/joegiralt/chimera/issues/240), in Chimera's own code (`modal::bow`).
- **Two lines on one ring.** The bow splits the loop into the bridge line
  (bow to bridge and back) and the nut line (bow to nut and back). The
  string's existing ring holds both: at the write the nut line's oldest
  cell is read (the nut's return) and the bridge line's input written;
  `split` cells behind it the bridge's return is read and the nut line's
  input written in its place. The two lines' sum is the ring's line `d`,
  so D2 does not grow.
- **The pitch law.** Each end reflects inverted, the nut through `−H`,
  the bridge through `−g·H`, so a wave comes back upright once a period:
  C3 plays 130.8 Hz, where the one-loop bow played 65. The loop is the
  line, the ends' filters' four samples (`ENDS_DELAY`) and the tuning
  allpass's fraction on the bridge line's input, `set_period(period,
  ENDS_DELAY, w)`. G1 to C7 plays within ±5 cents (the v1 patch: G1
  49.000 Hz, C3 130.813, C6 1046.35, −0.25 cents); a bow's stick-slip moves its
  pitch a few cents, as a real one's does, so Bowed's gate is ±5 where
  STRING and SYMP keep ±2 (controller ruling).
- **POS** is the bow position β, `0.06 + 0.44·POS` of the string from the
  bridge. The split is whole samples, β of the whole loop less the bridge
  side's filter and allpass (`bridge_len`), so POS is the same position at
  every note and the pitch is exact at every split. It glides one whole
  sample at most, one block in 8 (`BOW_SLEW`), at a block's first sample:
  `+1` re-reads the last bridge return and drops the newest nut-line
  sample, `−1` writes the nut line's input into both cells. Neither line
  reads the other's, and the sum holds. A faster glide moves the bow fast
  enough to Doppler-shift the waves it reflects: at two steps a block, a
  square LFO on POS took G1 21 cents flat. At POS 1 the bow is at the
  middle and the even harmonics fall away (the 2nd over 10 dB under POS
  0.15's at C3).
- **BRIGHT** is the bridge-to-body path, a one-pole low-pass on the
  output, outside the loop, its corner from 2·f0 at 0 to 64·f0 at 1
  (`tone`), scaled by the bow's corner sharpness (below). From BRIGHT 1
  to 0 the v1 patch's harmonics 8 to 24 fall 15.4 dB, and it cannot move
  the pitch. The spec's
  first law, the bridge's reflection low-pass, is not heard: the bow
  re-sharpens the Helmholtz corner every period, and eight cascaded
  three-taps in the reflection moved harmonics 8 to 24 by under 3 dB
  (controller ruling).
- **The ends' loss.** While bowed the bridge reflects at 0.97 a pass
  (`BOW_LOSS`; a T60 of 1.7 s at C3, 4.6 s at G1), the nut at 0.95
  (`NUT_LOSS`), and both through the three-tap squared at c 0.5 (`END_C`:
  linear phase, two samples, `|H| ≤ 1`). Without it the ripples between
  the bow and either end never decayed: the harmonics were irregular,
  notes wandered 3 cents and INIT's C2 swelled 9 dB over 23 s. The nut is
  lossier than the bridge: unlike ends clear the string's lock at β ≈ 1/6
  (measured: nut 1.0, 0.97, 0.95 and 0.93, bridge 0.97 to 0.99).
- **The bow table** is ours: `ρ = w⁴/(w⁴ + Δv⁴ + 1e-20)`, the push
  `Δv·ρ`, at most `0.57·w`: 1 at rest (the string sticks), ½ at `|Δv| =
  w`, falling as `Δv⁻⁴` (it slips); no offset, which would put DC into the
  string, and 0 at force 0, so a lifted bow lets the string ring free.
  One `vdiv.f32` a sample, no `tanhf` or `powf`.
- **The playable window.** Whatever FORCE and SPEED do,
  the bow's width stays where the string keeps Helmholtz motion: one slip
  a period, no sub-harmonic at −20 dB. The motion depends only on R, the
  curve's width over the bow's velocity (the junction is homogeneous in
  the two), so the window was measured in R: β by 1/32 × R in 10 % steps
  × G1 to C7, 3 s each, clean over 0.5–1.5 s and 2–3 s. Three things
  showed:
  - a ceiling at every β, about 4.3: past it the bow slides the string
    steadily (a junction equilibrium, silent after the blocker);
  - Schelleng's floor near the bridge, falling as β rises: `0.29 / β`
    clears every measured floor from β 0.06 to 0.14;
  - bands where the string locks, at β ≈ 1/6, 1/4, 1/3 and near 1/2, all
    below R ≈ 3.5. The window keeps R above them, but β ≈ 1/4 still locks
    under light force or high SPEED
    (https://github.com/joegiralt/chimera/issues/241).
  So R's window is `[max(0.29 / β, 3.5), 4.2]` (`R_BRIDGE`, `R_FLOOR`,
  `R_CEIL`), held under the ceiling; at β 0.06 it closes to 4.2. FORCE ×
  velocity (`FORCE·(0.5 + 0.5·velocity)`) places R in it geometrically,
  floor at 0 to ceiling at 1, and SPEED tilts it by `√(0.15 / v_b)`, so the
  width goes as `√v_b`, as Schelleng's window scales with the bow's
  velocity (controller ruling); the tilted R is held in the window. Below
  an effective force of 0.05 the width fades to 0: a lifted bow is off.
  The width⁴ eases to its target at `EASE / 64` a sample while bowed, as
  the bow's velocity does, and starts there on a note's first block.
- **FORCE and SPEED are heard in the tone.** Inside the window the loop's
  corner barely moves, so Cremer's corner rounding carries them: the
  corner sharpens as the
  bow presses harder or moves slower, as `√(f / v_b)`, 1 at FORCE and
  SPEED 0.5, and it scales BRIGHT's corner (`sharpness`). SPEED 0.25
  against 1 moves the harmonics' shares by 0.08 to 0.30 (summed |Δ|,
  harmonics 1 to 24).
- **Robustness.** Over the reviewer's sweep (7 notes ×
  velocity 20, 64, 127 × FORCE and SPEED 0.1, 0.5, 1 × POS by quarters,
  945 cases) every case is clean, against 88.7 % before; over POS by 1/32
  × FORCE × SPEED at velocity 100 (2,079 cases) 98.2 %, the rest at β ≈
  1/4 (0.239–0.266) and 0.459, a few samples of POS wide.
  `bowed_plays_clean_across_the_instrument` gates it: 97 % of its coarse
  grid, and INIT's POS, FORCE and SPEED at every velocity.
- **DAMP** is the ring after the lift, at every note. At note-off the
  bow's width sheds over 240 samples, the ends' filters fade to pure
  delays and the nut's gain to 1 over the same samples, and the bridge's
  gain ramps from `BOW_LOSS` to DAMP's, up or down (`Release::lift`):
  lifted, the loop is linear and `LoopGain` keeps it stable.
- **The output** is the wave the bow sends toward the bridge, through
  BRIGHT's low-pass, × `BOW_OUT` = 1.16: what the bridge hears, from the
  first sample (#206). 1.16 puts the v1 patch's held C3 within 0.04 dB of
  the one-loop bow's RMS (0.381).
- **The attack's DC (#248).** The bow's pull has a mean, which holds the
  string aside: a static deflection that the lossy ends carry into the
  wave toward the bridge. It settles, and the Helmholtz motion grows, at
  the loop's rate, `BOW_LOSS·NUT_LOSS` a period (12 periods: 92 ms at C3,
  250 ms at G1), and the blockers take it. Once settled the bow does not
  drift: from 1 s, every 0.1 s of its output, over whole periods, holds a
  mean at least 69 dB under its RMS, G1 to C7, at SPEED's and BRIGHT's
  corners (`a_settled_bow_does_not_drift`). The −30 dB drift #248
  reported is a window that cuts a period of the bow's pulse wave. What
  the sweep reads as DC with FOLD on a bowed chord (0.0065 on P1, 95 ms
  in) is the fold rectifying the growing attack, gone 0.3 s in, and it is
  allowed with that reason. Tried at the source and not taken: taking
  the push's mean out (the Helmholtz motion needs it: INIT fell 31 dB), a body
  high-pass at f0/16 to f0/2 on the output (the fold's DC moved either
  way, to 0.017), a nut lossless at 0 Hz below f0/16 (no change), and an
  ideal DC removal, the output less its mean over the last period (the
  fold's DC 0.0070 → 0.0079). Only a nut lossless below f0 itself cleared
  the gate (0.0024), by moving the nut's loss at f0 (0.95 → about 0.965)
  and about 7 cents of phase there (estimated) that the lift would glide
  off; it is a tone change for the owner.
- **Stability by construction.** While bowed the loop's linear gain is
  at most `0.97·0.95·|H|⁴ < 1` a period (each end's filter at most 1);
  lifted, the bridge's `LoopGain` below 1. The junction adds at most
  `0.57·w` to each line a sample, so every wave is bounded. The tests hold the output under 4.0 at every corner. The
  output keeps its blocker.
- The block runs in spans where no index (the write, the splice, the nut's
  read) wraps; the split's step is a sample of its own. `tick` stays the
  tests' reference, bit for bit.

### Release
- A note-off (#51) ramps the loop gain from the held one to
  `RELEASE_T60` = 0.12 s over `RELEASE_SAMPLES` = 240 (5 ms), and never
  gives the gain back, whatever DAMP does. Nothing scales a buffer. The
  `Release` is the voice's: STRING and SYMP's main string
  (`string::StringVoice`) and BOWED. A lifted bow sheds its force over the
  same 240 samples while its ring ramps to DAMP's T60, not
  `RELEASE_T60`: up or down, since the lifted loop is linear.
- The halo gets no release: a sitar's sympathetic strings ring until
  touched (the owner's rule), so a released halo rings on at its held T60
  until silent, keeping its lease.
- A Modal note is silent, and frees its voice, once its output, after
  the blocker, stays 60 dB under the note's own peak (`SILENT_REL`,
  DAMP's T60) for 10 blocks, and never under −120 dBFS
  (`SILENT_FLOOR`), unless the model still excites itself (a bow on the
  string, a strike's burst). The peak is held from note-on. An absolute
  0.001 once cut a quiet high note: DAMP 1 fell silent at 5.1 s on
  STRING's C5 and 0.5 s on its G6.

### Ensemble
- The ensemble (#50) is read heads on the string's own line, at
  `2 + A + A·sₖ` behind the write: centred so that at DEPTH 0 they read in
  phase with the dry. A 0.1–6 Hz quadrature LFO swings them to a peak
  Doppler of 15 cents · DEPTH.
- There are two heads, at 0° and 90°. Evenly spread heads (three at 120°,
  or a pair at 180°) cancel each partial's first sidebands in the sum,
  Σ e^(i2πk/3) = 0, so the mix barely moves.
- The swing is capped to the loop: `A ≤ (delay − 4)/2`, and 0 on a loop
  under 4 samples. Each head is clamped to `[2, min(top, 2 + 2.001·A)]`,
  NaN-safe, so no read leaves the ring however the LFO drifts.
- A note-off reaches the heads only through their source: they read the
  line the released loop writes.

### SYMP: chords, COUPLE and HALO
- STRUCTURE steps Rings' single-voice chord table (`part.cc`,
  `chords[0]`), adapted to the 7 halo strings: each chord less the 0.0 the
  main string plays, 11 chords of 7 distinct intervals (`modal::chords`),
  `chord_of(s) = min(⌊11·s⌋, 10)`. The pairs 0.01 apart stay: they are
  Rings' detuned chorus. Bryan Noll's alternative set (`BRYAN_CHORDS`) is
  not used.
- The chord reads the block's modulated STRUCTURE un-eased. A change
  glides each halo string's period linearly to the new chord's over
  `CHORD_GLIDE_SAMPLES` = 960 (20 ms), re-split every `GLIDE_STEP` = 16
  samples; the glide is the easing. A pitch change mid-glide moves the
  glide's end and rebases its start, so the strings don't jump. A note's
  first block takes its chord whole.
- A halo string's period is `P·2^(−st/12)`, raised by octaves until its
  line fits (`fold`: `floor(period − 0.5) ≤ 979`); at G1 Rings' −12 plays
  unison. At note-on each halo ring is sized for the longest folded period
  any chord gives it at that note, so no glide grows a ring mid-note.
- Each halo string's loop gain is from its own period, at 2× the main
  string's T60, with no release. That ring time is open (below).
- COUPLE and HALO set the coupling, `0.1·COUPLE`, and the halo level,
  `0.6·HALO`, latched at note-on. The defaults, 0.25, give the old fixed
  0.025 and 0.15.

### Old patches
- `translate_v1` runs once, at decode, only when a retired id is present.
- DECAY becomes DAMP on STRING, SYMP and BOWED through the ring time the
  old loop gave at C3 (130.81 Hz): `g = 0.999 − 0.009·DECAY`,
  `T60 = −3 / (130.81·log10 g)`, DAMP its inverse under the new law
  (`damp_from_v1_decay`). INIT's DAMP is the same expression at DECAY 0.3,
  about 0.943 (C3 rings 14.25 s, as before). On BANK, DAMP is DECAY.
- BRIGHT and POS carry over (BRIGHT flipped on the string models);
  STRUCTURE comes from STIFF on STRING and from INHARM on BANK and SYMP;
  E.DPT, E.RAT and E.MIX become the ENS controls; FDBK and the hidden
  fields are dropped. The v1 fixture `init_modal.snd` renders bit for bit
  as INIT does.
- An old patch's EXCITE keeps its value and shows on EXC. COLOR, FORCE
  and SPEED load at their defaults, 0.8, 0.5 and 0.5, the old hidden
  values. On BANK, BURST is the file's EXCITE, so an old strike keeps its
  length.
- Old Bowed patches stored BRIGHT, DAMP and POS but never read them, and
  their old sound was the bug (https://github.com/joegiralt/chimera/issues/240): an octave low. They load as an
  in-tune bow, after the DECAY and BRIGHT rules: POS 0.15 (β ≈ 0.126,
  about an eighth of the string from the bridge), BRIGHT 0.5, DAMP
  `damp_for(0.5)` (a 0.5 s ring after the lift), FORCE and SPEED 0.5.
  `a_v1_bowed_patch_bows_in_tune` checks its pitch and level and pins a
  held second and its release bit for bit, re-recorded deliberately.
  INIT is untouched: MODEL → BOWED bows at POS 0 (near the bridge), BRIGHT
  0.3 and INIT's ring.

### Rendering
- Each string runs its block, not a sample at a time
  (`StringVoice::render`, `KsString::run_coupled`). A ring is walked in
  spans where neither the write nor the low-pass's newest tap wraps, so no
  read is wrapped or bounds-checked per sample, and the loop's state stays
  in registers. The low-pass's two older taps are carried from the sample
  before: no write lands on them in between.
- The ensemble's heads wrap only while the write is within
  `Ensemble::reach` (the head limit's older tap, plus one) of the ring's
  start; those samples, and a release's ramp, run in the spans' slow form.
- The halo runs string by string over the block, summed into a block
  buffer in string order; a gliding set runs in `GLIDE_STEP` runs. BODY
  runs over the block (`Body::process_block`).
- The per-sample paths (`StringVoice::tick`, `KsString::tick_coupled`)
  stay as the tests' reference, bit for bit.

### Memory
- Sympathetic is String's voice plus a lease, sized within one align of
  it. That supersedes in part ADR 0054's const assert that Bowed or String
  sizes the voice.
- Task 18 added the note's peak to `ModalEngine` (4 B) and the POS
  glide's end, step and count to `CosineOsc` (12 B), each within its
  padding: no host size moved (`Voice` 6,208 B and `Instrument`
  164,424 B, 122,296 B of D2 left, since ADR 0060's chain).
- Host sizes: `Voice` 6,056 B, `ModelSlot` 4,160 B, `SymPool` 111,848 B,
  `Instrument` 162,968 B, which leaves 123,752 B of D2. Firmware
  `.ram_d2` is 162,108 B. `ModalParams` grew by EXC's four fields;
  `BowedString` by its lift, bow velocity, velocity scale and bowing
  flag, the ends' filters, the split, BRIGHT's low-pass and the glide's
  count, less the one-loop bow's `written` (4,056 B, inside `ModelSlot`)
  and
  `StringVoice` by COLOR's passes, within its padding.

### Costs
Task 18 moved no bill. `damped` swaps each loop gain's `powf` for an
`expf` and adds a few multiplies and a compare a block, a divide only
where `LP_SHARE` binds; the silence rule a compare a block; BANK's POS
glide about 5 instructions a sample while POS moves, inside the bank's
benched loop (1,808 of 1,900 billed at 32 modes).

`COST_*` are host estimates by the method below, not yet measured on the
chip (https://github.com/joegiralt/chimera/issues/242). Each is the
model's last benched bill, plus the hot path's added instructions in the
thumbv7em release build at 1.46 cycles an instruction (ADR 0052's rate),
plus 10 % (a saving taken at 90 %), rounded up to 10. Per-block work is
spread over the block's 64 samples, a ring's wrap-free spans taken at the
bench's notes (C3 up in fifths: 2.2 spans a block). A `powf` or a
`set_period` (with its two `sinf`s) is about 130 instructions. The
arithmetic is in each constant's doc comment.

The counts are `llvm-objdump -d --mcpu=cortex-m7` of
`target/thumbv7em-none-eabihf/release/chimera-stm32` (`just firmware`),
before step A at 6f8fffc and after, each path counted from branch to
branch as the bench's default Sound takes it (release idle, no route).
Instructions a sample:

| Path | Symbols | 6f8fffc | Now |
|---|---|---|---|
| STRING string | before `KsString::tick_full` (BODY comb and FDBK on); now `StringVoice::render`, `run::<true, false>`: a fast span 85 per 4 samples, entry, prologue and exit 127 a block, a span's head and remainder 63 | 82 | 25.4 |
| STRING loop | `ModalEngine::render`'s STRING arm (before: call, store, level; now with BODY's early return) | 17 | 0.3 |
| Output blocker and level | `ModalEngine::render`'s blocker loop, 125 per 16 samples | — | 7.8 |
| STRING total, BODY 0 | | 99 | 33.5 |
| BODY | `Body::process_block`: 90 per 2 samples, 94 a block | — | 46.5 |
| Ensemble | `run::<true, true>`'s fast span 101 less 21.25; its slow spans, the extra span a ring and `Ensemble::set` 1.8 | — | 82 |
| BOWED | `BowedString::render`: the fast span 64 (the junction, both ends' filters and their fade, the allpass, the bow table's `vdiv`, BRIGHT's low-pass, the easing); per block at most 14 (the setup's 327, three `expf`s and two `sqrtf`s, the spans' heads, the step one block in 8); the blocker 7.8. Dropped: the two `tanhf` bodies, T = 135 (the friction's `expm1f` path for k ≥ 2, 73; the push's for k = 1, 54; each one's inlined tail, 4) | 143 | 85.8 |
| SYMP main string | `run::<false, false>`: 44 per 4 samples; 127 a block; a span 57 | 82 | 14.9 |
| SYMP halo string, each of 7 | `KsString::run_coupled`: 62 per 4 samples; a call 71; a span 65 | 42 | 18.8 |
| SYMP per sample, besides | the coupled input (55 per 16), the two buffers' clears (116 each), the runs' setup 150, the mix (71 per 16), BODY's call, the `tanhf` dispatch 17 | 31 + 12 | 31.7 |
| SYMP total, BODY 0 | main + 7 halo + the rest, after the blocker | 419 | 186.2 |
| CHORD's cut block | a gliding halo in four runs: 3 × (87 + 7 × (71 + 65)) a block, over 64 | — | 48.7 |

| Term | Before step A | Now | From |
|---|---|---|---|
| `COST_STRING` | 390 | 330 | 99 → 33.5 instructions, −86 cycles; DAMP's two `powf`s and the dispersion's re-split, billed always, +18 |
| `COST_BOWED` | 620 | 390 | 620 + (85.8 − 143 − 135) × 1.46 × 0.9 + (14 − 1.46) × 1.1 for the `vdiv` = 381.3 |
| `COST_SYMPATHETIC` | 809 | 540 | 419 → 186.2 instructions, −306 cycles; ten `powf`s a block, +35 |
| `COST_BANK`, `COST_MODE` | 460, 45 | 460, 45 | sample loop unchanged |
| `BODY` (STRING, SYMP, BODY > 0) | — | 80 | 46.5 instructions |
| `ENSEMBLE` (STRING, SYMP, MIX > 0) | — | 140 | 82 instructions |
| `PITCH` (a PITCH or FINE route) | 12 | 30 | eight `set_period`s a block on SYMP |
| `CHORD` (a STRUCTURE route on SYMP) | — | 180 | the re-splits and lerp, 60, and the cut block, 48.7: 174.6 |

BODY and the ensemble are billed only when on, from the stored params and,
for a sounding note, from what it latched (`playing_cost`), as MODES is.
The marginal 1.46 cycles an instruction leaves out I-cache misses:
`StringVoice::render` is 8 KB (four variants, each a fast and a slow
span), `KsString::run_coupled` 1 KB, `Body::process_block` 0.7 KB; a
voice's block touches one variant's fast span. Misses stay unbilled until
the bench.

Voices beside the whole FX bus at its worst (rev V, rev Y; with the
master tape the same unless noted): STRING bare 8, 8; at the default BODY
8, 8; with the ensemble 8, 7. BOWED 8, 8 (tape 8, 8). SYMP bare 8, 7; at
the default BODY 8, 6; with the ensemble 7, 5 (tape 6, 5); with a
STRUCTURE route at the default BODY 6, 5. BANK at 32 modes 2, 2; at 48, 2,
1. `modal_bills_each_model` pins every count.

The tightest margin is the default STRING Sound on rev Y with the master
tape: a voice bills 467 against the 545 that keeps 8 (4,363 / 8), 78
cycles. The loops are chains of dependent float operations; at about 2.4
cycles an instruction STRING and BODY spend those 78 and it plays 7. The
bench's MDL STR row, on rev Y with the tape, is the case to check.

Not yet measured on the chip: every `COST_*`, `BODY`, `ENSEMBLE` and
`CHORD` above is a host estimate by the instruction-count method in Costs,
and the voice counts here follow from them. The bench is filed as
https://github.com/joegiralt/chimera/issues/242.

The rows that bench will run: MDL STR, STR0 (BODY 0), STR E (BODY 0, the ensemble, no
routes), STR+ (BODY 1, the ensemble, LFO 1 on each macro), BOW, BOW+
(FORCE 1, SPEED 1, POS 0.5, BRIGHT 0, LFO 1 at 10 Hz into BRIGHT, DAMP and
POS at 64: the split stepping one block in 8, the bow's worst case), SYM, SYM0, SYM+ (the ensemble, STRUCTURE a chord on every 8
blocks), SYM LFO (a route into STRUCTURE), RES and RES48. The MEMORY
screen's DARK NOTE+BLOCK is a STRING note-on at G1 and COLOR 0 and its
first block, the whole render included, against the block's budget;
DARK +6 PASSES is that less the same at the default COLOR, the six
smoothing passes over the line COLOR 0 adds. Both are timed as SYM
NOTE-ON is, by `Rig::time_note_on`.

### Open for the owner
- The halo's ring time: 2× the main string's T60 (about 30 s at INIT
  DAMP and 40 s at DAMP 1, holding its pool slot that long, so a fifth
  SYMP note plays bare), the main string's, or DAMP-capped (about 8 s). It
  stays 2× until the owner rules.
- DAMP is seconds at every pitch (spec § 1), now the fundamental's
  exactly (Task 18); the old per-pass law rang high notes shorter.
  Partial key tracking is a candidate, by ear. A long ring on a high
  note is brighter than BRIGHT asks (`LP_SHARE`): the price of ringing.
- POS starts at the end on a pluck or strike (β 0) and at 0.06 on a bow
  (Schelleng's floor); one start for all three would move INIT's level
  (above).
- The bank's gain staging (#231), and an ensemble of 2 heads or 3.

## Alternatives considered
- Keep FDBK and clamp its range below the unity point: its useful range
  would be 0–0.012, and the knob would still be one bad mapping from a
  runaway.
- A DC blocker in every loop: its phase delay falls with frequency, so
  compensating it at f0 left the upper partials of low strings flat, about
  50 cents at G1, 20 at C3 and 10 at C4, and no single compensation fixes
  every partial. Keeping G1 in tune also grew the line to 1,016 samples.
- Rings' fixed dispersion coefficient, −0.618·s/(0.15 + s): on our
  period it moves C3's 8th partial 0.95 cents at most, so STRUCTURE would
  barely stiffen the string.
- Three ensemble heads at 120°, or two at 180°: their first sidebands
  cancel in the sum.
- Re-splitting the halo to a new chord each block: it stepped the line up
  to 22 samples at G1 and ticked on low notes. The 20 ms glide replaces it.
- Bowed's friction reading POS's two-tap comb (the spec's first law): it
  bows a second, shorter loop through the second tap, which takes the
  pitch (`1/(1 − POS)` times it).
- Task 13's one-loop bow with POS as an output comb: the pitch held, but
  an octave low, POS was not a bow position and BRIGHT was not heard
  (https://github.com/joegiralt/chimera/issues/240). Replaced by the
  two-delay bow.
- Two separate rings for the two lines: about 1.5× the line, +1.8 KB a
  voice, past `ModelSlot`. One ring spliced at the bow costs none.
- A fractional split by interpolation (its loss moves the tone with the
  fraction) or by two allpasses (more cost, and the same whole-sample
  crossing).
- BRIGHT as the bridge's reflection low-pass (the amended spec's law):
  under 3 dB even at eight cascaded three-taps, since the bow re-sharpens
  the corner every period. On the output instead.
- A lossless bowed loop (the gain at `LoopGain::MAX`, the nut at −1): its ripples
  never decayed. A fixed loss at both ends instead.
- Other cures for the β ≈ 1/n locks, measured on the window map and
  dropped: a two-point bow one cell wide (Pitteroff–Woodhouse's finite
  width, two junctions; it detuned high notes and period-doubled at the
  middle), the friction seeing the string's velocity smoothed over 2 or 3
  samples (worse everywhere), a stiff string (Rings' dispersion; it moved
  the locks, sharpened the pitch 2–8 cents), bow noise and a bow-speed
  attack (no gain), and Coulomb or velocity-weakening friction in the
  explicit junction (noisy windows). A lossier nut cleared 1/6, and the
  window's floor keeps R above the rest.
- A window tuned to the tests (Task 14's first pass: width 2.7 × v_b ×
  force^¼, widened towards the ends): it passed the tests and left 11.3 %
  of the reviewer's sweep sub-harmonic.
- STK's bow table (`|x·slope + offset| + 0.75` to the −4th, slope
  `5 − 4·pressure`): a licensed constant set; ours is a rational curve with
  no `powf`, scaled to the bow's velocity.
- Per-sample string loops: at the first estimate they billed SYMP 1,370
  (3 voices on rev V, so the pool of four never filled) and STRING 460.
  The block-at-a-time spans bill 540 and 330, bit for bit.

## Consequences
- No undriven string sustains forever: DAMP's longest is a 20 s T60, and
  no loop gain reaches 1.
- DC inside a loop decays with the ring, not at 10 Hz. BOWED's stick-slip
  can hold a small offset in its ring, which the output blocker removes;
  its output DC over 10 s is about 2e-5. Measured over one second, a
  high-passed output's mean is set by the window's edge samples, up to
  about 1e-2, so the stability test measures DC over 10 s.
- A macro route costs nothing extra on BANK, which already recomputes its
  filters every block. On STRING, a moving STRUCTURE re-splits the loop
  once a block, billed in `COST_STRING`. On SYMP, a moving STRUCTURE
  re-splits the seven halo strings every 16 samples while a chord glides
  (`CHORD`).
- STRING's four dispersion allpasses run at every STRUCTURE, so its output
  starts 4 samples after the pluck; SYMP's main string has none.
- Each whole-sample move of a line under a glide or a PITCH route leaves a
  small tuning-allpass transient
  (https://github.com/joegiralt/chimera/issues/232). Under a PITCH route
  the strings are still plucked at the unpitched length
  (https://github.com/joegiralt/chimera/issues/233).
- A string's state lives in its struct only between blocks: anything that
  reads a line mid-block (the ensemble's heads) reads it inside the span.
- Modal's goldens and the INIT Modal fixtures moved with step A, and
  again with Task 18's DAMP make-up and silence rule (`modal_init`,
  `modal_lfo_cutoff`, `modal_sympathetic`, `algo_to_modal_switch`,
  `two_parts_two_pairs` and its pre-limiter row, `init_modal.snd`, the v1
  bow's release). POS 0 renders as before, so no golden moved for POS.
- A Modal voice holds until its own ring is 60 dB down, so a quiet note
  keeps its voice longer than under the old 0.001: DAMP 1 on STRING C5
  rings past 9 s where it freed at 5.1 s.
- Known gaps, tracked: the chord glide's test bound
  (https://github.com/joegiralt/chimera/issues/234), the long stability
  test at C2 only (https://github.com/joegiralt/chimera/issues/235),
  `fold` on an infinite period
  (https://github.com/joegiralt/chimera/issues/236), the chord ratios'
  `exp2f`s at note-on (https://github.com/joegiralt/chimera/issues/237),
  the `Modal` variant shown as BANK
  (https://github.com/joegiralt/chimera/issues/238), and the blocker, BODY
  and release fixed at 48 kHz
  (https://github.com/joegiralt/chimera/issues/239).

## Sources
- docs/superpowers/specs/2026-09-29-modal-2-resonators-design.md § 2
- docs/superpowers/plans/2026-09-29-modal-2-resonators.md, Tasks 1 to 11b,
  13 and 14
- The owner's decision of 2026-09-30, after the bench: the exciters get
  their own node, first in the chain, and Bowed becomes playable (spec
  § 1, § 2 BOWED and § 3, amended); and on #240, Bowed rebuilt as a
  two-delay bowed string before the ship flash, with the controller's
  rulings of the same day (BRIGHT on the output, the ends' loss, the bow
  table against the bow's velocity, Bowed's ±5 cents)
- J. O. Smith, *Physical Audio Signal Processing*, CCRMA, "Bowed Strings"
  and "Digital Waveguide Bowed-String"; M. E. McIntyre, R. T. Schumacher
  and J. Woodhouse, "On the oscillations of musical instruments", JASA
  74(5), 1983; J. C. Schelleng's bow-force limits, as those sources give
  them. STK's `Bowed` is a known implementation, not a source: no STK
  code, constant or table is used, so THIRD_PARTY.md does not change.
- Mutable Instruments Rings (MIT, ADR 0032): `dsp/string.cc` (`ap_gain`,
  the dispersion law) and `dsp/part.cc` (the chord table). Ours are
  `LoopGain`, the blocker's placement on the output, the fractional tuning,
  BODY, the ensemble, the release, the octave fold, the chord glide, the
  block-at-a-time spans and the macro mapping.
- ADR 0040 (the 984-sample ring), ADR 0052 (1.46 cycles an instruction),
  ADR 0054 (the pool, the dirty extent), ADR 0055 (the master tape)
