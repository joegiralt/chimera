# 0056. Modal's resonators share four modulatable macros; loops are stable by construction

- **Status:** Proposed
- **Deciders:** project owner
- **Supersedes in part:** [0010](0010-modulation-targets-are-honest.md)
  (Modal settings read at note-on: the four macros are live),
  [0040](0040-eight-voices-modal-strings-to-g1.md) (the line's length),
  [0042](0042-voice-pitch-is-a-matrix-destination.md) (`ModalEngine::PITCH`),
  [0054](0054-sympathetic-strings-from-a-shared-pool.md) (the voice's
  size assert, `COST_SYMPATHETIC`)
- **Accepted by:** the Modal 2 step A ship flash (plan task 12), once its
  bench rows fill in the chip figures below

## Context
The owner's bench report (#191) and the survey behind the Modal 2 step A
spec found the string loops unsafe. FDBK added `filtered · fdbk · 0.3`
inside the loop, so any FDBK above about 0.012 made the loop gain exceed 1
(the default, 0.2, did). The string then grew until a ±1.5 clamp, which
can latch DC. No string loop had a DC blocker, so Bowed's stick-slip and
any asymmetric excitation could drift off zero.

## Decision
So far (step A, task 1):
- `modal::loop_parts::LoopGain` is a string loop's gain per pass. Its
  constructors clamp to `[0, 0.9995]`, and NaN gives 0. Every string loop
  multiplies by one: STRING, the SYMP main string, each halo string and
  BOWED. On STRING and the SYMP main string it multiplies the loop's
  sample after every in-loop stage, so none bypasses it.
- `modal::loop_parts::DcBlocker` is a one-pole high-pass at `DC_HZ = 10`,
  `y = g·(x − x1) + r·y1`, `r = e^(−2π·10/fs)`, `g = (1 + r)/2`. Its gain
  is 1 at Nyquist and below 1 elsewhere.
  - There is one per voice, on each string model's output, outside the loop:
    STRING's string, SYMP's main-and-halo mix and BOWED's ring. It is reset
    at note-on.
  - It is not in the loop because there its phase delay, which falls with
    frequency, detunes the upper partials. That is about 50 cents flat of
    harmonic at G1 at 10 Hz, and no single compensation fixes every
    partial.
  - The loop needs no blocker: with `LoopGain` below 1, DC can't grow or
    latch, and decays with the ring.
- FDBK and its ±1.5 clamp are deleted from the DSP. Task 3 removes
  `ModalParams::ks_feedback`.

Task 2 adds fractional tuning (#163):
- A string's loop is a ring read `delay` samples behind the write, plus
  `loop_parts::Allpass1`, a first-order allpass `(η + z⁻¹)/(1 + η z⁻¹)`
  that carries the fraction of a sample.
- Every in-loop phase delay is compensated exactly at f0.
  - `split(period, other, w)` takes `other` off the period. Today `other`
    is 0: the low-pass adds no delay, and the blocker is outside the loop.
    Step A's dispersion (task 8) adds its own.
  - It puts the whole part on the line (`floor(d − 0.5)`, at least
    `MIN_LINE = 2`) and the rest, in `[0.5, 1.5)`, on the allpass.
  - `eta_for` inverts the allpass's phase delay exactly:
    `θ = ω(1 − frac)/2`, `η = sin θ / sin(ω − θ)`.
- The loop low-pass is the linear-phase three-tap
  `c/2·(x[d−1] + x[d+1]) + (1 − c)·x[d]`, centred on the line, so it adds
  no delay.
- BOWED's ring runs through the same allpass.
- The fundamental of STRING and the SYMP main string lands within
  0.05 cents from G1 to C7, and partials 2 to 4 are harmonic to it at G1
  and C3. Whole-sample tuning was up to 54 cents off at G1 and 84 cents at
  C7.
- The ring is 981 samples: a line of at most 979, plus the low-pass's two
  taps. That supersedes ADR 0040's 984-sample ring in part.
  - G1's 979.6-sample period takes a 979-sample line, 0.59 on the
    allpass, plus the low-pass's two taps.
  - F♯1 and lower clamp to the longest line.
  - `Instrument` is 161,208 B, which leaves 125,512 B of D2 (task 1: 161,496 B).

Task 3 reworks `ModalParams` (spec § 3):
- Home: MODEL, STRUCTURE, BRIGHT, DAMP, POS (and SPACE, the Part's reverb
  send). Model page: EXCITE, BODY, ENS DEPTH, ENS RATE, ENS MIX, COUPLE,
  HALO, MODES. FDBK, STIFF, INHARM, DECAY, the E.* fields, `ks_excitation`,
  `ks_color`, `bow_velocity`, `bow_force` and `note` leave the struct; their
  disk codes are retired.
- DAMP on a string is seconds, not a per-pass loss: T60 = 0.05·400^DAMP
  (50 ms to 20 s), and `LoopGain::from_t60` turns it into each loop's gain
  at its own f0. The bank's DAMP is its old DECAY, law unchanged.
- BRIGHT runs dark to bright on every model. On STRING, SYMP and BOWED
  that inverts the old field (brightness was damping), so INIT's BRIGHT is
  `1 − 0.7`, and a v1 patch's BRIGHT flips on load. The loop low-pass's
  side taps are `c·(1 − c)` for `c = 0.05 + 0.45·(1 − BRIGHT)`: the old
  two-point average's loss at low frequencies, so old patches keep their
  tone.
- SYMP's halo keeps its old relation to the main string's tone: its
  low-pass reads `1 − 0.7·(1 − BRIGHT)`, as the old halo damping was 0.7×
  the main string's.
- SYMP's STRUCTURE tunes the halo only; its main string is never stiff.

Task 4 translates old patches once, at decode (`translate_v1`):
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

Task 5 makes the four macros modulatable:
- STRUCTURE, BRIGHT, DAMP and POS are read every block from the voice's
  modulated params. The loops read `modal::Macros`, never the params.
  Each block eases them `EASE` = 0.3 of the way to the block's values. A
  note's first block snaps them, since nothing sounds yet.
- This supersedes in part ADR 0010's "Modal settings are read only at
  note-on", for these four. The model page (EXCITE, BODY, ENS, COUPLE,
  HALO, MODES) stays note-on and unmodulatable.
- POS on STRING and SYMP shapes the pluck from the first block's
  modulated value: `KsString::excite` fills the noise at note-on, and
  `shape` combs and smooths it before the first tick. So VEL and NOTE
  routes reach it. With no route, this is bit-identical to shaping at
  note-on. BANK reads POS live.
- Until the chord table (task 10), SYMP's halo retuned to the eased
  STRUCTURE whenever it moved.
- MODES latches at note-on. A sounding bank note keeps its modes, and the
  voice is billed for them (`ModalEngine::playing_cost`) until it ends.
- The bank drops the modes past Nyquist, as Rings does (the count stops at
  the first mode at or over 0.49 of the sample rate). They were clamped to 0.49
  and piled into one tone near 20.5 kHz at high STRUCTURE. The bank's
  level is otherwise today's, with the tanh as its limiter: a full strike at
  BRIGHT 1 drives it 10 to 150× over unity, which is gain staging for the
  ear (https://github.com/joegiralt/chimera/issues/231). Until then BANK's
  STRUCTURE and BRIGHT are held to a relative click rule, DAMP and POS to
  the strict one.

Task 6 gives the pages the spec's layout: the home page is RES, the model
page is named for the model (STRING, SYMP, BANK, BOWED) and shows its
cells, and a control the model ignores is dimmed with its matrix column.
The cell labels stay the spec's: STRUCT, ENS.D, ENS.R, ENS.M.
BOWED dims all four macros.

Task 7 releases the strings (#51): a note-off ramps the loop gain from the
held one to `RELEASE_T60` = 0.12 s over `RELEASE_SAMPLES` = 240 (5 ms), and
the release never gives the gain back, whatever DAMP does. Nothing scales a
buffer. Bowed's bow lifts over the same 240 samples (a softer bow sooner)
while its ring falls to the same 0.12 s. The halo gets no release: a
sitar's sympathetic strings ring until touched (the owner's rule), so a
released halo rings on its held T60 until silent, keeping its lease.

Task 8 adds STRUCTURE's dispersion on STRING and moves BODY out of the
loop (#10):
- `modal::dispersion::Dispersion` is a chain of four first-order allpasses
  in the loop, after the low-pass and before the tuning allpass; the gain
  follows them all. There is no new buffer.
  - Its law follows Rings' `ap_gain` curve, `s/(0.15 + s)` (MIT, ADR 0032),
    scaled by the period, as Rings scales its allpass line: each stage's
    DC delay is `1 + 1.15·s/(0.15 + s)·0.1·period/4` samples. It is 1 at
    STRUCTURE 0, a plain delay, and the chain's DC delay is capped at half
    the period.
  - Rings' fixed coefficient, −0.618·s/(0.15 + s), moved the 8th partial
    of C3 by 0.95 cents. The scaled law moves it 24 cents at STRUCTURE 1,
    and by the model about 21 at G1, 30 at C4 and 75 at C6: sharper up the
    keyboard, as on piano wire.
  - The chain's phase delay at f0 comes off the line (`split`'s `other`),
    so the fundamental holds within 0.05 cents from STRUCTURE 0 to 1. At G1
    and STRUCTURE 1 the line is about 880 samples, within the 979.
  - Below G1 the stiff loop clamps to its longest period, 984 samples: the
    979-sample line, one sample of fraction and the chain's four at
    STRUCTURE 0 (the ring stays 981). The chain comes
    off the line there too. Notes below G1 all play about 8 cents under G1,
    at every STRUCTURE.
  - A moved STRUCTURE re-splits the loop once a block. The chain glides:
    its DC delay, and so the line, moves at most 2 samples a block, about
    65 ms end to end at G1. Each whole-sample move of the line still
    leaves a small allpass transient, as a PITCH route's retune does.
  - SYMP's main string has no chain, and skips it: its STRUCTURE tunes the
    halo.
  - STIFF's two-sample mix is gone.
- `modal::body::Body` is BODY: three fixed resonances on the output,
  outside the loop, (102 Hz, Q 3, 1), (236 Hz, Q 4, 0.7) and (517 Hz, Q 3,
  0.5). Each is a peak-normalized band-pass, and the output is
  `(x + b·Σ gᵢ·bpᵢ(x)) / (1 + b/2)`, which peaks at about 1.4. It colours
  without transposing: the old half-delay comb in the loop sounded low
  notes an octave up. On SYMP it colours the main-and-halo mix. BODY is
  latched at note-on, as a model-page setting.
- A note-off's `Release` moves from each line to the voice: STRING and
  SYMP's main string (`string::StringVoice`) and BOWED. Halo lines never
  release.
- Sympathetic is String's voice plus a lease, and is sized within one
  align of it. That supersedes in part ADR 0054's const assert that Bowed
  or String sizes the voice. `Instrument` is 162,232 B, which leaves
  124,488 B of D2.

Task 9 rebuilds the ensemble (#50, ours): read heads on the string's own
line at `2 + A + A·sₖ` behind the write, in phase with the dry at DEPTH 0,
swung by a 0.1–6 Hz quadrature LFO to a peak Doppler of 15 cents · DEPTH.
There are two heads, at 0° and 90°: evenly spread heads (three at 120°, or
a pair at 180°) cancel each partial's first sidebands in the sum,
Σ e^(i2πk/3) = 0, so the mix barely moves.
- The swing is capped to the loop: `A ≤ (delay − 4)/2`, and 0 on a loop
  under 4 samples, so every head stays within `[2, delay − 2]` (2 on the
  shortest loops).
- A note-off reaches the heads only through their source: they read the
  line the released loop writes, and have no ramp of their own.

Task 10 gives SYMP its chords, COUPLE and HALO:
- STRUCTURE steps Rings' single-voice chord table (`part.cc`,
  `chords[0]`, MIT, ADR 0032), adapted to the 7 halo strings: each chord
  less the 0.0 the main string plays, 11 chords of 7 distinct intervals
  (`modal::chords`). The pairs 0.01 apart stay: they are Rings' detuned
  chorus. `chord_of(s) = min(⌊11·s⌋, 10)`.
- The chord reads the block's modulated STRUCTURE un-eased. A change
  glides each halo string's period linearly from where it is to the new
  chord's over `CHORD_GLIDE_SAMPLES` = 960 (20 ms), re-split every
  `GLIDE_STEP` = 16 samples through the fractional tuning. The glide is
  the easing. A re-split each block stepped the line up to 22 samples at
  G1 and ticked on low notes. A note's first block takes its routed chord
  whole, as it does the macros. A pitch change mid-glide moves the
  glide's end, rebasing its start so the strings don't jump.
- A halo string's period is `P·2^(−st/12)`, raised by octaves until its
  line fits (`fold`: `floor(period − 0.5) ≤ 979`). At G1 Rings' −12 plays
  unison. At note-on each halo ring is sized for the longest folded period
  any chord gives it at that note, so no glide grows a ring mid-note.
- Each halo string's loop gain is from its own period, at 2× the main
  string's T60, with no release (unchanged). That ring time is an open
  owner question (below).
- COUPLE and HALO replace the fixed 0.025 and 0.15: the coupling is
  `0.1·COUPLE` and the halo level `0.6·HALO`, latched at note-on. The
  defaults, 0.25, give today's values.
- A route into STRUCTURE on SYMP is billed `ModalEngine::CHORD`: 42 at
  first, re-estimated in task 11, until the bench's SYM LFO row measures
  it.
- Rings also has Bryan Noll's alternative chord set (`BRYAN_CHORDS`); it
  is not used.
- Provenance: from Rings (MIT) come the chord table and the dispersion's
  `ap_gain` law. Ours are `LoopGain`, the blocker's placement on the
  output, the fractional tuning, BODY, the ensemble, the release, the
  octave fold, the chord glide and the macro mapping.

Task 11 bills the models and re-records the goldens (its figures are
superseded by task 11b's, below):
- `COST_*` are host estimates until the ship flash's bench rows: the
  model's last benched bill, plus the hot path's added instructions in the
  thumbv7em release build (before at 6f8fffc, after), at 1.46 cycles an
  instruction (ADR 0052's rate), plus 10 %, rounded up to 10; per-block work
  spread over 64 samples. The arithmetic is in each constant's doc comment.
- The counts: `llvm-objdump -d --mcpu=cortex-m7` of
  `target/thumbv7em-none-eabihf/release/chimera-stm32` (`just firmware`),
  at 6f8fffc and at this task's commit, counting each path's instructions
  from branch to branch as the bench's default Sound takes them (release
  idle, no route). Instructions a sample:

  | Path | Symbols | Before | After |
  |---|---|---|---|
  | STRING string | 6f8fffc `KsString::tick_full` (BODY comb and FDBK on, no stiffness, no ensemble); now `StringVoice::tick` (the chain, release idle, no ensemble) | 82 | 108 |
  | STRING loop | `ModalEngine::render`'s STRING loop: call, store, level (before); call, store, BODY check (after) | 17 | 13 |
  | Output blocker and level | `ModalEngine::render`'s blocker loop, 125 per 16 samples | — | 7.8 |
  | STRING total, BODY 0 | | 99 | 128.8 |
  | BODY | the three band-passes inline in the STRING loop (SYMP's mix) | — | 65 (80) |
  | Ensemble | `StringVoice::tick`'s MIX > 0 branch | — | 94 |
  | BOWED | `ModalEngine::render`'s bowed loop, both `tanhf` dispatches (before: with the level); after, plus the blocker | 143 | 195 + 7.8 |
  | SYMP main string | `tick_full` as STRING's (before); `StringVoice::tick` without the chain (after) | 82 | 80 |
  | SYMP halo string, each of 7 | the inlined `tick_coupled` loop: before a two-point average and a modulo wrap; after the three-tap low-pass's three wrapped reads and the tuning allpass | 42 | 87 |
  | SYMP per sample, besides | the loop's head and tail, and the mix up to the `tanhf` dispatch | 31 + 12 | 26 + 19 |
  | SYMP total, BODY 0 | main + 7 halo + the rest, after the blocker | 419 | 741.8 |

  Per block, a `powf` or a `set_period` (with its two `sinf`s) is taken as
  about 130 instructions: STRING's two `powf`s and its dispersion re-split
  (`StringVoice::tune`), SYMP's ten `powf`s, and the retunes behind `PITCH`
  and `CHORD` (`KsString::set_period`, `SympatheticSet::split`).
- The marginal 1.46 cycles an instruction leaves out the larger code's
  I-cache misses (`StringVoice::tick` is 900 B, `ModalEngine::render`
  8 KB; the emulator figures billed a miss at 38 cycles). They are unbilled
  until the bench reads them.

  | Term | Was | Now | From |
  |---|---|---|---|
  | `COST_STRING` | 390 | 460 | +30 instructions a sample; DAMP's `powf`s; the dispersion's re-split, billed always |
  | `COST_BOWED` | 620 | 720 | +60 instructions a sample |
  | `COST_SYMPATHETIC` | 809 | 1,370 | +323 instructions a sample, nearly all the halo (each tick 42 → 87); ten `powf`s a block |
  | `COST_BANK`, `COST_MODE` | 460, 45 | 460, 45 | sample loop unchanged |
  | `BODY` (STRING, SYMP, BODY > 0) | — | 130 | three band-passes, 80 instructions |
  | `ENSEMBLE` (STRING, SYMP, MIX > 0) | — | 160 | two heads and the LFO, 94 instructions |
  | `PITCH` (a PITCH or FINE route) | 12 | 30 | eight `set_period`s a block on SYMP |
  | `CHORD` (a STRUCTURE route on SYMP) | 42 | 100 | seven `set_period`s every 16 samples |

  BODY and the ensemble are billed only when on, from the stored params and,
  for a sounding note, from what it latched (`playing_cost`), as MODES is.
- Voices beside the whole FX bus at its worst (rev V, rev Y): STRING bare
  8, 8; at the default BODY 8, 7; with the ensemble 7, 5. BOWED 7, 6. SYMP
  bare 4, 3; at the default BODY 3, 3; with the ensemble 3, 2. BANK at 32
  modes 2, 2; at 48, 2, 1. SYMP was 6, 5: on this estimate it keeps 6 on
  rev V only if it bills 916 or less with BODY on. With the master tape
  (ADR 0055) STRING at the default BODY gets 8, 6 and SYMP 3, 2
  (`modal_bills_each_model` pins every count).
- Host sizes: `Voice` 6,040 B, `ModelSlot` 4,160 B, `SymPool` 111,848 B,
  `Instrument` 162,840 B, which leaves 123,880 B of D2.
- The Modal goldens (`modal_init`, `modal_lfo_cutoff`, `modal_sympathetic`,
  `algo_to_modal_switch`), `init_modal.snd`'s render and instrument_test's
  `two_parts_two_pairs` are re-recorded once. #10's known-broken entries
  go.
- Bench rows for the ship flash: MDL STR, STR0 (BODY 0), STR E (BODY 0,
  the ensemble, no routes), STR+ (BODY 1, the ensemble, LFO 1 on each
  macro), BOW, SYM, SYM0, SYM+ (the ensemble,
  STRUCTURE a chord on every 8 blocks), SYM LFO (a route into STRUCTURE),
  RES and RES48.

Task 11b brings the hot paths back within budget, bit for bit (no golden
moves):
- Each string runs its block, not a sample at a time
  (`StringVoice::render`, `KsString::run_coupled`). A ring is walked in
  spans where neither the write nor the low-pass's newest tap wraps, so no
  read is wrapped or bounds-checked per sample, and the loop's state (the
  allpasses, the dispersion, the ensemble's LFO) stays in registers. The
  low-pass's two older taps are carried from the sample before: no write
  lands on them in between, as the write runs `d + 1 ≥ 3` samples ahead of
  the oldest.
- The halo runs string by string over the block, each summed into a
  block buffer in string order, as the per-sample sum was. A gliding set
  runs in `GLIDE_STEP` runs, each re-split between them as before.
- The ensemble's heads wrap only while the write is within
  `Ensemble::reach` of the ring's start (`set` holds the LFO's radius to 1
  within 1e-5, so no head passes `2 + 2.001·A`); those samples, and a
  release's ramp, run in the spans' slow form.
- BODY runs over the block (`Body::process_block`), its three band-passes'
  state in registers.
- The per-sample paths (`StringVoice::tick`, `KsString::tick_coupled`)
  remain as the tests' reference: `render_is_tick_bit_for_bit` and
  `run_coupled_is_tick_coupled_bit_for_bit` pin every chain, ensemble,
  line length and release case.
- Counted as task 11's were, with two rules added: a span's per-block work
  is taken at the bench's notes (C3 up in fifths: 2.2 spans a block), and a
  saving is taken at 90 %. Instructions a sample:

  | Path | Symbols | Task 11 | Task 11b |
  |---|---|---|---|
  | STRING string | `StringVoice::render`, `run::<true, false>`: a fast span 85 per 4 samples; entry, prologue and exit 127 a block; a span's head and remainder 63 | 108 | 25.4 |
  | STRING loop | `ModalEngine::render`'s STRING arm and BODY's early return | 13 | 0.3 |
  | Output blocker and level | as task 11 | 7.8 | 7.8 |
  | STRING total, BODY 0 | | 128.8 | 33.5 |
  | BODY | `Body::process_block`: 90 per 2 samples, 94 a block | 65 (80) | 46.5 |
  | Ensemble | `run::<true, true>`'s fast span 101 less 21.25; its slow spans, the extra span a ring and `Ensemble::set` 1.8 | 94 | 82 |
  | SYMP main string | `run::<false, false>`: 44 per 4 samples; 127 a block; a span 57 | 80 | 14.9 |
  | SYMP halo string, each of 7 | `KsString::run_coupled`: 62 per 4 samples; a call 71; a span 65 | 87 | 18.8 |
  | SYMP per sample, besides | the coupled input (55 per 16) and the two buffers' clears (116 each), the runs' setup 150, the mix (71 per 16), BODY's call, the `tanhf` dispatch 17 | 26 + 19 | 31.7 |
  | SYMP total, BODY 0 | main + 7 halo + the rest, after the blocker | 741.8 | 186.2 |
  | CHORD's cut block | a gliding halo in four runs: 3 × (87 + 7 × (71 + 65)) a block | — | 46 |

  | Term | Task 11 | Task 11b | From (vs 6f8fffc) |
  |---|---|---|---|
  | `COST_STRING` | 460 | 330 | 99 → 33.5 instructions, −86 cycles; DAMP's `powf`s and the re-split, +18 |
  | `COST_SYMPATHETIC` | 1,370 | 540 | 419 → 186.2 instructions, −306 cycles; ten `powf`s a block, +35 |
  | `BODY` | 130 | 80 | 46.5 instructions |
  | `ENSEMBLE` | 160 | 140 | 82 instructions |
  | `CHORD` | 100 | 180 | the re-splits and lerp, 60, and the cut block, 46 |
  | `COST_BOWED`, `COST_BANK`, `PITCH` | 720, 460, 30 | unchanged | |

- Voices beside the whole FX bus at its worst (rev V, rev Y; with the
  master tape the same unless noted): STRING bare 8, 8; at the default
  BODY 8, 8; with the ensemble 8, 7. BOWED 7, 6 (tape 7, 5). SYMP bare 8,
  7; at the default BODY 8, 6; with the ensemble 7, 5 (tape 6, 5); with a
  STRUCTURE route at the default BODY 6, 5. BANK as task 11. SYMP at the
  default BODY bills 620 against the 916 that keeps 6 on rev V.
- The shortest rings wrap most: at a 23-sample ring a block takes 6.6
  spans, STRING about 30 instructions a sample and a halo string about 23,
  still under task 11's per-sample paths.
- I-cache: `StringVoice::render` is 8 KB (four variants, each a fast and a
  slow span), `KsString::run_coupled` 1 KB, `Body::process_block` 0.7 KB;
  a voice's block touches one variant's fast span. Misses stay unbilled
  until the bench.

Measured on the chip (the ship flash; to fill in, rev V at 480 MHz):
- MDL STR /VOICE —, STR0 —, STR E —, STR+ —; BOW —; SYM —, SYM0 —, SYM+ —, SYM
  LFO —; RES —, RES48 —.
- `COST_*`, `BODY`, `ENSEMBLE` and `CHORD` from them: —.

Open for the owner:
- The halo's ring time: 2× the main string's T60 (about 30 s at the default
  DAMP, holding its pool slot that long, so later notes play bare), the
  main string's, or DAMP-capped (about 8 s). It stays 2× until the owner
  rules.
- DAMP is seconds at every pitch (spec § 1); the old per-pass law rang
  high notes shorter. Partial key tracking is a candidate, by ear.

## Alternatives considered
- Keep FDBK and clamp its range below the unity point: its useful range
  would be 0–0.012, and the knob would still be one bad mapping from a
  runaway.
- A DC blocker in every loop (task 1's first placement): its phase delay
  falls with frequency, so compensating it at f0 left the upper partials of
  low strings flat. They were about 50 cents off at G1, 20 at C3 and 10 at C4.
  Keeping G1 in tune also grew the line to 1,016 samples.

## Consequences
- No undriven string sustains forever: DAMP's longest is a 20 s T60, and
  no loop gain reaches 1 (`LoopGain::TOP` = 0.9995).
- DC inside a loop decays with the ring, not at 10 Hz. BOWED's stick-slip
  can hold a small offset in its ring, which the output blocker removes. Its
  output DC over 10 s is about 2e-5.
- Measured over one second, a high-passed output's mean is set by the
  window's edge samples, up to about 1e-2. The stability test measures DC
  over 10 s.
- A macro route costs nothing extra on BANK, which already recomputes its
  filters every block. On STRING, a moving STRUCTURE re-splits the loop
  once a block, billed in `COST_STRING`. On SYMP, a moving STRUCTURE
  re-splits the seven halo strings every 16 samples while a chord change
  glides (`ModalEngine::CHORD`).
- STRING's four dispersion allpasses run at every STRUCTURE, so its output
  starts 4 samples after the pluck; SYMP's main string has none.
- Each whole-sample move of a line under a glide or a PITCH route leaves a
  small tuning-allpass transient
  (https://github.com/joegiralt/chimera/issues/232). Under a PITCH route
  the strings are still plucked at the unpitched length, as before this
  step (https://github.com/joegiralt/chimera/issues/233).
- Task 11's estimate put SYMP at 3 voices on rev V at the default BODY, so
  the pool of four never filled there. Ruled (task 11b, before the ship
  flash): SYMP bills 916 or less with BODY on (6 voices on rev V), the
  default STRING Sound (BODY 0.3) keeps 8 voices on rev V and rev Y, and
  BODY and `ENSEMBLE` come down where the code allows. Task 11b's
  block-at-a-time strings bill SYMP 620 and the default STRING 410 at the
  default BODY, both below their pre-step-A bills, bit for bit.
- A voice's strings now run in spans, so a string's state lives in its
  struct only between blocks: anything that reads a line mid-block (the
  ensemble's heads) reads it inside the span.
- Modal's goldens and the INIT Modal fixtures moved, and were re-recorded
  once, at the end of step A (task 11).

## Sources
- docs/superpowers/specs/2026-09-29-modal-2-resonators-design.md § 2
- docs/superpowers/plans/2026-09-29-modal-2-resonators.md, Tasks 1 to 11
- Mutable Instruments Rings, `dsp/string.cc` (`ap_gain`) and
  `dsp/part.cc` (the chord table)
- ADR 0040 (the 984-sample ring), ADR 0054 (the dirty extent)
