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

The strings were also tuned to whole samples (up to 54 cents off at G1 and
84 at C7, #163), BODY was a half-delay comb in the loop that sounded low
notes an octave up (#10), the ensemble was a fixed chorus (#50), a note-off
did not release a string (#51), and the model's settings were read only at
note-on, so nothing could modulate a string's character.

## Decision

### Loops
- `modal::loop_parts::LoopGain` is a string loop's gain per pass. Its
  constructors clamp to `[0, 0.9995]` (`LoopGain::TOP`), and NaN gives 0.
  It is the only gain any loop multiplies by: STRING, the SYMP main string,
  each halo string and BOWED. On STRING and the SYMP main string it
  multiplies the loop's sample after every in-loop stage, so none bypasses
  it. Every other in-loop stage has a gain of at most 1.
- FDBK and its ±1.5 clamp are gone.
- `modal::loop_parts::DcBlocker` is a one-pole high-pass at `DC_HZ = 10`,
  `y = g·(x − x1) + r·y1`, `r = e^(−2π·10/fs)`, `g = (1 + r)/2`, gain 1 at
  Nyquist and below 1 elsewhere. There is one per voice, on the model's
  output, outside the loop: STRING's string, SYMP's main-and-halo mix and
  BOWED's ring. It is reset at note-on. The loop needs none: with
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
  no delay. BOWED's ring runs through the same allpass.
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
- DAMP on a string is seconds, not a per-pass loss:
  `T60 = 0.05·400^DAMP` (50 ms to 20 s), and `LoopGain::from_t60` turns it
  into each loop's gain at its own f0. The bank's DAMP is its old DECAY,
  law unchanged.
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
- The bow's force is `FORCE·(0.5 + 0.5·velocity)`, so a soft key still
  bows; at FORCE 0.5 and full velocity it is the old bow, bit for bit.
  `SPEED·0.3` is the bow's velocity (`BOW_SPEED`); SPEED 0.5 is the old
  one. FORCE 0 or SPEED 0 is a bow at rest, and silent. While the bow is
  on, the force and velocity ease each sample `EASE / 64` of the way to
  the block's FORCE and SPEED (`BOW_EASE`, about the macros' easing a
  block); at their targets no bit moves, so the v1 pin holds.
- DAMP is the ring after the lift: while bowed the loop's gain is
  `LoopGain::TOP` and the bow sustains the string, as before; at note-off
  it ramps to DAMP's T60 and never gives the gain back.
- BRIGHT is the linear-phase three-tap low-pass on the loop's tap,
  `c/2·(x[d−1] + x[d+1]) + (1 − c)·x[d]` at `c = 0.5·(1 − BRIGHT)`
  (`BOW_LP`), centred, so it adds no delay; at BRIGHT 1 the tap is read
  alone, as before. `|H| = (1 − c) + c·cos ω` is at most 1 for
  `c ≤ 0.5`. The plan's 0.25 moved a routed BRIGHT's sound 7.4e-4 RMS,
  under the test's 1e-3; 0.5 is the most the bound allows. Even there the
  harmonics move under 0.5 dB: the bow's stick-slip re-sharpens the wave
  every pass, so BRIGHT is gentle on this bow.
- POS is the bow point's comb on the output, the pluck's law:
  `0.5·(x + tap)`, the tap `d − POS·d` behind the write
  (`KsString::ring_tap_at`, interpolated, clamped to what the note has
  written, #206), above POS 0.03; at or below it the one tap, as before.
  A bowed string lacks the harmonics its bow point nulls, and the comb
  puts the nulls there. The spec had the friction read the comb: that
  bows a second loop, `(1 − POS)·d` long, which takes the pitch to
  `1/(1 − POS)` times it (C3's bow, 65 Hz, played 93 Hz at POS 0.3 and
  218 Hz at 0.7). On the output the pitch holds within 0.3 cents at every
  POS and BRIGHT.
- The loop and the friction read the one tap. The bow sounds an octave
  below its note, as it always has: its stick-slip inverts the loop each
  pass, so the ring holds half a period. The v1 pin keeps that sound.
- Stability holds by construction: the loop multiplies `x` by a
  `LoopGain` below 1, the low-pass's gain is at most 1, the friction
  (`4·force·tanh(8·(bow_vel − x))`) is bounded, `tanh` bounds the push,
  and the output has its blocker.

### Release
- A note-off (#51) ramps the loop gain from the held one to
  `RELEASE_T60` = 0.12 s over `RELEASE_SAMPLES` = 240 (5 ms), and never
  gives the gain back, whatever DAMP does. Nothing scales a buffer. The
  `Release` is the voice's: STRING and SYMP's main string
  (`string::StringVoice`) and BOWED. A lifted bow sheds its force over the
  same 240 samples while its ring ramps to DAMP's T60, not
  `RELEASE_T60`.
- The halo gets no release: a sitar's sympathetic strings ring until
  touched (the owner's rule), so a released halo rings on at its held T60
  until silent, keeping its lease.

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
- Old Bowed patches stored BRIGHT, DAMP and POS but never read them. They
  load as the old sound: DAMP `damp_for(RELEASE_T60)` (the old lifted
  bow's 0.12 s), BRIGHT 1 and POS 0, after the DECAY and BRIGHT rules.
  `a_v1_bowed_patch_bows_as_before` pins a held second bit for bit.

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
- Host sizes: `Voice` 6,056 B, `ModelSlot` 4,160 B, `SymPool` 111,848 B,
  `Instrument` 162,968 B, which leaves 123,752 B of D2. Firmware
  `.ram_d2` is 162,108 B. `ModalParams` grew by EXC's four fields;
  `BowedString` by its lift, bow velocity, velocity scale and bowing
  flag (4,016 B) and
  `StringVoice` by COLOR's passes, within its padding.

### Costs
`COST_*` are host estimates until the ship flash's bench rows. Each is the
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
| BOWED | `ModalEngine::render`'s bowed loop, both `tanhf` dispatches, plus the blocker | 143 | 195 + 7.8 |
| BOWED, playable (Task 13) | the same loop: BRIGHT's two side taps and their wraps 23, POS's check 4, its clamped, lerped second tap 44, FORCE and SPEED eased 10; DAMP's `powf` once lifted, 2 | 203 | 286 |
| SYMP main string | `run::<false, false>`: 44 per 4 samples; 127 a block; a span 57 | 82 | 14.9 |
| SYMP halo string, each of 7 | `KsString::run_coupled`: 62 per 4 samples; a call 71; a span 65 | 42 | 18.8 |
| SYMP per sample, besides | the coupled input (55 per 16), the two buffers' clears (116 each), the runs' setup 150, the mix (71 per 16), BODY's call, the `tanhf` dispatch 17 | 31 + 12 | 31.7 |
| SYMP total, BODY 0 | main + 7 halo + the rest, after the blocker | 419 | 186.2 |
| CHORD's cut block | a gliding halo in four runs: 3 × (87 + 7 × (71 + 65)) a block, over 64 | — | 48.7 |

| Term | Before step A | Now | From |
|---|---|---|---|
| `COST_STRING` | 390 | 330 | 99 → 33.5 instructions, −86 cycles; DAMP's two `powf`s and the dispersion's re-split, billed always, +18 |
| `COST_BOWED` | 620 | 860 | +60 instructions a sample at step A, 87; +83 for the playable bow, 133.3, POS billed always |
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
8, 8; with the ensemble 8, 7. BOWED 6, 5 (tape 6, 4). SYMP bare 8, 7; at
the default BODY 8, 6; with the ensemble 7, 5 (tape 6, 5); with a
STRUCTURE route at the default BODY 6, 5. BANK at 32 modes 2, 2; at 48, 2,
1. `modal_bills_each_model` pins every count.

The tightest margin is the default STRING Sound on rev Y with the master
tape: a voice bills 467 against the 545 that keeps 8 (4,363 / 8), 78
cycles. The loops are chains of dependent float operations; at about 2.4
cycles an instruction STRING and BODY spend those 78 and it plays 7. The
bench's MDL STR row, on rev Y with the tape, is the case to check.

Measured on the chip (the ship flash; to fill in, rev V at 480 MHz):
- MDL STR /VOICE —, STR0 —, STR E —, STR+ —; BOW —, BOW+ —; SYM —,
  SYM0 —, SYM+ —, SYM LFO —; RES —, RES48 —. DARK NOTE+BLOCK —, DARK +6 PASSES — cycles.
- `COST_*`, `BODY`, `ENSEMBLE` and `CHORD` from them: —.

The bench rows: MDL STR, STR0 (BODY 0), STR E (BODY 0, the ensemble, no
routes), STR+ (BODY 1, the ensemble, LFO 1 on each macro), BOW, BOW+
(FORCE 1, SPEED 1, POS 0.5, BRIGHT 0, LFO 1 at 10 Hz into BRIGHT, DAMP and
POS at 64), SYM, SYM0, SYM+ (the ensemble, STRUCTURE a chord on every 8
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
- DAMP is seconds at every pitch (spec § 1); the old per-pass law rang
  high notes shorter. Partial key tracking is a candidate, by ear.
- The bank's gain staging (#231), and an ensemble of 2 heads or 3.
- Bowed sounds an octave below its note and BRIGHT barely moves it; both
  come from the one-loop bow, which the v1 pin keeps. A bow junction on
  two delays, nut and bridge side, each reflection inverting (Smith; STK's
  `Bowed`), plays at pitch and lets POS and BRIGHT act as on a string. It
  would move the old Bowed sound: step B's, or the owner's call.

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
  pitch (`1/(1 − POS)` times it). The comb is on the output instead.
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
- Modal's goldens and the INIT Modal fixtures moved once, with step A.
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
- docs/superpowers/plans/2026-09-29-modal-2-resonators.md, Tasks 1 to 11b
  and 13
- The owner's decision of 2026-09-30, after the bench: the exciters get
  their own node, first in the chain, and Bowed becomes playable (spec
  § 1, § 2 BOWED and § 3, amended)
- Mutable Instruments Rings (MIT, ADR 0032): `dsp/string.cc` (`ap_gain`,
  the dispersion law) and `dsp/part.cc` (the chord table). Ours are
  `LoopGain`, the blocker's placement on the output, the fractional tuning,
  BODY, the ensemble, the release, the octave fold, the chord glide, the
  block-at-a-time spans and the macro mapping.
- ADR 0040 (the 984-sample ring), ADR 0052 (1.46 cycles an instruction),
  ADR 0054 (the pool, the dirty extent), ADR 0055 (the master tape)
