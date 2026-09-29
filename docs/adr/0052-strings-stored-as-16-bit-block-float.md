# 0052. Store Modal's string delay lines as 16-bit block float

- **Status:** Superseded by 0053 (2026-09-29, never accepted). ADR 0053
  (the sympathetic slot pool) is written in exclusive-state plan Task 9.
- **Deciders:** project owner

**Why superseded (owner, 2026-09-29).** A shared pool of four f32
sympathetic slots, after Rings (`kMaxPolyphony = 4`), makes 16-bit storage
unnecessary. With the pool, voices are sized for Bowed, and D2 keeps about
126 KB free, against Q16's 141 KB. Every string stays f32, so there is no
precision loss: this ADR's −80 dBFS Sympathetic bound, and its ear test,
go with it. Strings are f32 again (plan Task 7), and the goldens return to
their earlier values. Of this ADR's changes, three stay because they are
bit-identical in f32: the fused injection, the one-pass damp and the note-on
clear (exclusive-state spec § 4.8). The rest of this record is kept as it
stood.

## Context
Every `KsString` holds a 984-sample delay line (ADR 0040). In f32 that is
3,936 B. Sympathetic holds eight of them, which makes it Modal's largest
model. Modal 2's exciters need room in D2. The maths can stay f32; only the
storage has to shrink.

A host prototype showed that 16-bit fixed point alone fails (exclusive-state
spec § 4):

- **Round to nearest never goes quiet.** A Karplus-Strong loop loses about
  0.1 % a pass. Below roughly 500 LSB, rounding hands that loss back and the
  string limit-cycles. String A4 goes quiet at 2.24 s in f32 but never in
  Q1.14, so the voice is never freed.
- **Truncation halves the tails.** The same note goes quiet at 1.07 s.
- **Either way, the error is −49 to −84 dBFS RMS.**

## Decision
Each string's line is a `Store` (`chimera-core/src/dsp/modal/q16.rs`). The
shipped store is `Q16`.

- **Storage.** A sample is stored as `q = sat16(round(x · 2^e))` and read
  as `q · 2^−e`. There is one exponent `e` per line, and the line tracks
  the peak |q| written since its write position last wrapped.
  - Rounding is `(x · 2^e + copysign(0.5, x)) as i32`: `as` truncates, so
    this rounds half away from zero. It differs from `roundf` only within an
    f32 epsilon of a tie, and it avoids `roundf`'s branchy bit twiddling.
  - The clamp to i16 is one `ssat`.
  - An overshoot clips and never wraps; NaN stores 0.
- **Range.** `e` is an `Exp`, held within 14..=30.
  - Every note starts at 14. Full scale there is ±2.0, which covers the
    feedback clamp (±1.5) and the sympathetic injection.
  - The ceiling is 30, not the spec's 24. At 24, Sympathetic's main string
    limit-cycles near 3.5e-5, and the seven high-Q strings it drives keep
    ringing above the silence threshold, so the voice is never freed. At
    30, i16 · 2^−30 is still an exact, normal f32.
- **Step.** On each wrap (once a period, or the whole ring for Bowed),
  `next_exp` checks the peak:
  - Below ¼ full scale, `e` goes up by one and every sample shifts left one
    bit. That is exact for the period's samples.
  - Above ½ full scale, `e` goes down by one and every sample shifts right
    with rounding.
  - The step rescales all 984 samples, so samples past the loop keep the
    line's exponent. A stale sample past a loop that a retune shortened may
    be louder than the period's peak. It clips at full scale on an up step:
    quieter than f32, never a burst.
- **Budget.** A voice steps at most one line a block (`StepBudget`). A line
  refused a step waits for its next wrap; saturation covers any growth in
  the meantime.
- **Note-on.** Every line a note starts on is cleared: one memset of the
  ring, with `e` back at 14. The old note's samples are never read back,
  even by a PITCH route that lengthens the loop, so there is nothing to
  burst.
  - This replaces a rescale of the old tail to `e = 14`, which cost about
    7.9k halfword shifts per Sympathetic voice with no budget.
  - The f32 store clears too, so both stores keep the same semantics and
    the gate compares like with like.
  - The only change from before this ADR: a loop lengthened after a
    retrigger reads silence, where it used to read the last note's tail.
- **Sympathetic injection.** The main string's coupled input is added to
  each sympathetic string at its write position. Each string holds its last
  output in `pending` and stores it once, with the next sample's input
  (`KsString::tick_coupled`).
  - The old way was a store, then a load and a store again. The new way is
    bit-identical to it in f32; hashes were checked with note-off, a loop
    lengthened by pitch, and a two-sample loop.
  - In Q16 it rounds once instead of twice.
- **Other stores.** `[f32; MAX_STRING_DELAY]` is the other `Store`, with
  today's arithmetic. `Store` is sealed, because `KsString::init_in_place`
  trusts every store's `init_in_place` to write every field.

**The gate** is `strings_i16_match_f32` and
`strings_i16_with_feedback_stay_bounded` in `modal/mod.rs`. It runs with
the default Modal params except as listed.

- **With FDBK 0.** It plays String, Bowed and Sympathetic at G1, A4 and C6,
  each at DECAY 0 and 0.3, plus String A2 with the ensemble on. Bowed is
  released at 0.5 s. Each case must meet all three bounds:
  - **Error:** the RMS of Q16 − f32, relative to full scale over the first
    second, within the case's bound.
    - **−90 dBFS** for every case except three.
    - **−80 dBFS** (`SYMPATHETIC_ERROR_DBFS`) for Sympathetic A4 at
      DECAY 0, and C6 at DECAY 0 and 0.3.
  - **Envelope:** peaks over 10 ms windows within 0.1 dB, wherever f32 is
    above −60 dBFS.
  - **Quiet time:** within 20 ms of f32, or both never quiet.

  The envelope and quiet-time bounds apply to every model, with no
  exceptions.
- **With FDBK 0.2 and 1.0.** The loop self-oscillates and amplifies any
  difference, so the check is bounds instead. String A4 and Sympathetic A3
  run for 1 s. Every sample must be finite and within ±1.5, and the envelope
  must stay within 2 dB of f32.

**Why Sympathetic has its own bound.** Its seven high-Q strings, tuned to
the main string's harmonics, ring at 0.65–0.8 and amplify any error the main
string hands them.

- The first estimate blamed their own lines. They sit at `e = 14`, where
  one LSB is 6.1e-5. Each pass rounds in about 0.29 LSB RMS, and a loop
  with gain 0.999 builds that power by 1/(1 − g²) ≈ 500, which gives a
  floor near −76 dBFS.
- Measurement moved that:
  - Storing the injection once instead of twice took C6 at DECAY 0 from
    −73.9 to −83.7 dBFS.
  - With the seven lines stored in f32 (main string still Q16), C6 still
    reads −83.3 and A4 −88.3.
- What is left is the main string's 16-bit rounding, resonated. No store
  choice for the sympathetic lines reaches −90.

**The step budget costs accuracy.** With unlimited steps, compared with one
step per voice per block:

- **Before the injection fused:** A4 at DECAY 0 read −87.8 dBFS unlimited
  against −84.4 budgeted, a 3.4 dB cost. C6 was unchanged (−73.7 against
  −73.9).
- **Now:** A4 reads −86.9 against −84.5 (2.4 dB) and C6 −86.5 against −83.7
  (2.8 dB). The budget buys a bound on the step's spike.

Measured on the host, velocity 100, FDBK 0. Error is in dBFS, the envelope
in dB, and quiet time in 64-sample blocks (Q16 / f32):

| Case | Error | Envelope | Quiet |
|---|---|---|---|
| String G1, DECAY 0 / 0.3 | −94.75 / −94.76 | 0.003 / 0.005 | never / never |
| String A4, DECAY 0 | −102.36 | 0.037 | 5390 / 5398 |
| String A4, DECAY 0.3 | −103.72 | 0.003 | 1681 / 1681 |
| String C6, DECAY 0 | −95.00 | 0.017 | 3265 / 3266 |
| String C6, DECAY 0.3 | −108.70 | 0.004 | 889 / 889 |
| String A2, ensemble on | −96.67 | 0.007 | 7418 / 7414 |
| Bowed G1, both | −inf | 0.000 | 10 / 10 |
| Bowed A4, both | **−90.01** | 0.007 | 1872 / 1872 |
| Bowed C6, both | −91.95 | 0.007 | 999 / 999 |
| Sympathetic G1, DECAY 0 / 0.3 | −94.02 / −93.90 | 0.003 / 0.005 | never / never |
| Sympathetic A4, DECAY 0 | −84.45 (bound −80) | 0.034 | never / never |
| Sympathetic A4, DECAY 0.3 | −93.80 | 0.003 | 4048 / 4048 |
| Sympathetic C6, DECAY 0 | −83.69 (bound −80) | 0.058 | 6894 / 6894 |
| Sympathetic C6, DECAY 0.3 | −84.91 (bound −80) | 0.009 | 2051 / 2051 |

With feedback, the envelope stays within 0.057 dB: String A4 measures 0.002
and 0.001, and Sympathetic A3 0.016 and 0.057.

**Margins.**

- **Bowed A4 is at the edge.** It passes −90 by 0.01 dB. A later change to
  Bowed may push it over, and the fix must not be to loosen the bound
  quietly.
- **Sympathetic sits 3.7 dB under its −80 bound** at the worst (C6,
  DECAY 0).
  - The owner first set −72, when C6 measured −73.9, a 1.9 dB margin.
  - Fusing the injection brought C6 to −83.7, which left 11.7 dB of slack
    under −72, room for a regression to hide. The bound was tightened to
    −80 to close it.
- **Bowed G1** frees itself at block 10 in both stores. That bug predates
  this ADR; see https://github.com/joegiralt/chimera/issues/206.

## Alternatives considered
- **f32 strings, today's layout.** Exact, but it frees only the roughly
  13 KB a voice that exclusivity (ADR 0051) already gave.
- **Plain Q1.14 with round to nearest.** It limit-cycles and never frees the
  voice.
- **Plain Q1.14 with truncation.** It halves the tails.
- **Sympathetic's seven lines in f32, the main string in Q16.** Measured, it
  does not meet −90 either: C6 reads −83.3 and A4 −88.3. It also saves only
  about 2 KB a voice, because Sympathetic is the largest model and
  `ModalEngine` would stay near 29.7 KB.
- **Noise shaping, with error feedback per line.** It needs a stored
  residual per sample, which defeats the saving.
- **A band of ½–1 full scale instead of ¼–½.** It gains about 6 dB and gives
  up the headroom that keeps growth from clipping.
- **At note-on, rescaling only the loop, or skipping the rescale when `e`
  is 14.** The worst case is still a full-ring rescale per line. Spreading
  the rescale under the step budget would read stale samples at the wrong
  exponent until it finished. Clearing is a fixed cost, and there is
  nothing left to burst.

## Consequences
- **Memory.** On the host:
  - `KsString` drops to 2,000 B, and `ModalEngine` from 31,744 B to
    16,064 B.
  - A `Voice` drops from 33,584 B to 17,904 B, and `[Voice; 8]` from
    268,672 B to 143,232 B.
  - That frees **15,680 B a voice and 125,440 B across eight**. It is
    headroom for Modal 2 and is not spent here.
- **Cost per sample, estimated.** These are static instruction counts on the
  hot path: thumbv7em, opt-level 2, default params (String with body and
  feedback on). They exclude `tanhf` and the voice chain. The chip bench
  (plan Task 6) is authoritative; these are what it is checked against.
  Cycles use the rates behind `ModalEngine::COST_*`: 1.36 cycles an
  instruction, times 1.07 for bench over emulator, so about 1.46 cycles an
  instruction.

  | Model | f32 before | f32 now | Q16 first cut | Q16 now | Q16 now − f32 before |
  |---|---|---|---|---|---|
  | String | 99 | 99 | 178 | 140 | +41 instr ≈ +60 cycles |
  | Sympathetic | 593 | 411 | ≈ 1,644 | 701 | +108 instr ≈ +158 cycles |

  - Two changes brought Q16 down from its first cut:
    - Inlining the store and replacing `roundf` and the float clamp. A store
      was an out-of-line call, `roundf` and a float clamp, about 46
      instructions; it is now about 11 inline, plus 9 for the peak.
    - Fusing the injection, which dropped a load, a store and a full
      `tick_full` per sympathetic string.
  - A Q16 read costs 4 instructions to f32's 1 (`ldrsh`, `vmov`, `vcvt`,
    `vmul`).
  - **String** comes to about 450 cycles against the 390 billed, about
    15 % over. The halved line may win back up to about 23 cycles of
    D-cache misses (78 misses a block today).
  - **Sympathetic** comes to about 1,430 against the 1,400 billed (cap
    1,457), less up to about 50 from halved D-cache misses (174 a block
    today).
  - If the bench confirms either overrun, the plan stops and reports; it
    does not raise the constant. The next saving would be the peak, about 9
    instructions a store: scan the loop at the wrap instead of tracking the
    peak on every store.
- **Cost of an exponent step.** At most 984 halfword shifts, about 1,000
  cycles, at most once per voice per block. The worst case is eight voices,
  about 1.3 % of a block.
- **Cost at note-on.** A memset of each line the model holds: 1,968 B for
  String or Bowed, and 15,744 B for Sympathetic's eight. That is about 500
  cycles a line at 4 B a cycle, about 3.9k cycles a Sympathetic voice, and
  about 31k (5 % of a block) for an eight-voice Sympathetic chord. It is
  the same whatever came before; `note_on_clears_every_line_at_a_fixed_cost`
  pins it. The excitation that follows costs more than the clear: up to
  seven filter passes over the loop.
- **Sound.** Modal's sound changes below −90 dBFS, or below −80 dBFS for
  Sympathetic's loud high notes. Every render that plays Modal is
  re-recorded once (ADR 0011):
  - the goldens `modal_init`, `modal_lfo_cutoff` and `algo_to_modal_switch`;
  - the instrument golden `two_parts_two_pairs` and its hash before the
    limiter;
  - the render of the `init_modal.snd` v1 fixture. The fixture's bytes are
    unchanged.
- **Before acceptance,** the owner compares Sympathetic's high notes against
  the f32 store by ear on the unit. The f32 store stays in the code as the
  reference, so the comparison is a one-line change of default.

## Sources
- `docs/superpowers/specs/2026-09-29-exclusive-state-design.md`, § 4 and
  § Tests.
- The exclusive-state plan, Task 5, where the measurements, the floor
  analysis and the instruction counts come from.
- ADR 0040 (string length), ADR 0051 (one engine per voice) and ADR 0011
  (goldens).
