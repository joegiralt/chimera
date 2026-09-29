# 0052. Store Modal's string delay lines as 16-bit block float

- **Status:** Proposed. Before acceptance, the owner compares Sympathetic's
  high notes against f32 by ear on the unit.
- **Deciders:** project owner

## Context
Every `KsString` holds a 984-sample delay line (ADR 0040). In f32 that is
3,936 B, and Sympathetic holds eight of them, which makes it Modal's largest
model. Modal 2's exciters need room in D2. The maths can stay f32; only the
storage has to shrink.

A host prototype showed that 16-bit fixed point alone fails (exclusive-state
spec § 4):

- **Round to nearest never goes quiet.** A Karplus-Strong loop loses about
  0.1 % a pass. Below roughly 500 LSB, rounding hands that loss back and the
  string limit-cycles. String A4 goes quiet at 2.24 s in f32, but never in
  Q1.14, so the voice is never freed.
- **Truncation halves the tails.** The same note goes quiet at 1.07 s.
- **Either way, the error is −49 to −84 dBFS RMS.**

## Decision
Each string's line is a `Store` (`chimera-core/src/dsp/modal/q16.rs`). The
shipped store is `Q16`:

- **Storage.** It stores `q = sat16(round(x · 2^e))` and reads
  `q · 2^−e`. There is one exponent `e` per line, and the line tracks the
  peak |q| written since its write position last wrapped. `as i16`
  saturates and maps NaN to 0, so an overshoot clips and never wraps.
- **Range.** `e` is an `Exp`, held within 14..=30.
  - Every note starts at 14. Full scale there is ±2.0, which covers the
    feedback clamp (±1.5) and the sympathetic injection.
  - The ceiling is 30, not the spec's 24. At 24, Sympathetic's main string
    limit-cycles near 3.5e-5, and the seven high-Q strings it drives keep
    ringing above the silence threshold, so the voice is never freed. At 30,
    i16 · 2^−30 is still an exact, normal f32.
- **Step.** On each wrap (once a period, or the whole ring for Bowed),
  `next_exp` checks the peak:
  - Below ¼ full scale, `e` goes up by one and every sample shifts left one
    bit, which is exact.
  - Above ½ full scale, `e` goes down by one and every sample shifts right
    with rounding.
  - Each step rescales all 984 samples, so samples beyond the loop keep the
    line's level.
- **Budget.** A voice steps at most one line a block (`StepBudget`). A line
  refused a step waits for its next wrap; saturation covers any growth in
  the meantime.
- **Restart.** A note-on rescales the whole line back to `e = 14` with
  rounding. A stale sample that a later pitch drop reads back keeps its
  level instead of bursting 2^(e−14) times louder.
- **Other stores.** `[f32; MAX_STRING_DELAY]` is the other `Store`, with
  today's arithmetic. `Store` is sealed, because `KsString::init_in_place`
  trusts every store's `init_in_place` to write every field.

**The gate** is `strings_i16_match_f32` and
`strings_i16_with_feedback_stay_bounded` in `modal/mod.rs`, run with the
default Modal params except as listed.

- **With FDBK 0,** the test plays String, Bowed and Sympathetic at G1, A4
  and C6, each at DECAY 0 and 0.3, plus String A2 with the ensemble on.
  Bowed is released at 0.5 s. Each case must meet all three bounds:
  - **Error:** RMS of Q16 − f32, relative to full scale over the first
    second, within the case's bound:
    - **−90 dBFS** for every case except three.
    - **−72 dBFS** (`SYMPATHETIC_ERROR_DBFS`) for Sympathetic A4 at DECAY 0
      and C6 at DECAY 0 and 0.3.

      Those sympathetic lines resonate at 0.65–0.8, so they sit at `e = 14`,
      where one LSB is 6.1e-5 at every level. Each pass adds about 0.29 LSB
      RMS of rounding, and a loop with gain 0.999 builds that power up by
      1/(1 − g²) ≈ 500. That gives about 4e-4 a string, and times 0.15 · √7
      at the mix, a floor near −76 dBFS. This is a limit of 16-bit storage
      in that model, not of the exponent's steps: unlimited steps measure
      the same.
  - **Envelope:** peaks over 10 ms windows within 0.1 dB, wherever f32 is
    above −60 dBFS.
  - **Quiet time:** within 20 ms of f32, or both never quiet.

  The envelope and quiet-time bounds apply to every model, with no
  exceptions.
- **With FDBK 0.2 and 1.0,** the loop self-oscillates and amplifies any
  difference, so the check is bounds instead. String A4 and Sympathetic A3
  run for 1 s. Every sample must be finite and within ±1.5, and the envelope
  must stay within 2 dB of f32.

Measured on the host, with velocity 100 and FDBK 0. Error is in dBFS,
envelope in dB, and quiet time in 64-sample blocks (Q16 / f32):

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
| Sympathetic G1, DECAY 0 / 0.3 | −93.95 / −93.68 | 0.003 / 0.005 | never / never |
| Sympathetic A4, DECAY 0 | −84.38 (bound −72) | 0.055 | never / never |
| Sympathetic A4, DECAY 0.3 | −94.15 | 0.019 | 4048 / 4048 |
| Sympathetic C6, DECAY 0 | −73.94 (bound −72) | 0.048 | 6895 / 6894 |
| Sympathetic C6, DECAY 0.3 | −82.20 (bound −72) | 0.015 | 2051 / 2051 |

With feedback, the envelope stays within 0.057 dB: String A4 measures 0.002
and 0.001, and Sympathetic A3 0.016 and 0.057.

Two cases need a note:

- **Bowed A4 is at the edge.** It passes −90 by 0.01 dB. Any later change
  to Bowed may push it over, and the fix must not be to loosen the bound
  quietly.
- **Bowed G1 at block 10** is a bug from before this ADR, the same in both
  stores; see
  https://github.com/joegiralt/chimera/issues/206.

## Alternatives considered
- **f32 strings, which is today's layout.** Exact, but it frees only the
  roughly 13 KB a voice that exclusivity (ADR 0051) already gave.
- **Plain Q1.14 with round to nearest.** Limit-cycles and never frees the
  voice.
- **Plain Q1.14 with truncation.** Halves the tails.
- **Sympathetic's seven lines in f32, the main string in Q16.** This meets
  −90 dBFS. But Sympathetic is the largest model, so `ModalEngine` would
  stay at about 29.7 KB, saving about 2 KB a voice instead of 15,680 B.
- **Noise shaping, with error feedback per line.** Needs a stored residual
  per sample, which defeats the saving.
- **Keeping the band at ½–1 full scale instead of ¼–½.** Gains about 6 dB,
  which is not enough for Sympathetic, and gives up the headroom that keeps
  growth from clipping.

## Consequences
- **Memory.** On the host, `KsString` drops to 2,000 B and `ModalEngine`
  from 31,744 B to 16,064 B. A `Voice` drops from 33,584 B to 17,904 B, and
  `[Voice; 8]` from 268,672 B to 143,232 B. That frees **15,680 B a voice
  and 125,440 B across eight**. It is headroom for Modal 2 and is not spent
  here.
- **Cost.**
  - Every read gains a multiply by 2^−e, and every write gains a multiply,
    a round, a saturate and a max for the peak.
  - A step costs at most 984 halfword shifts, about 1,000 cycles, at most
    once per voice per block. The worst case is eight voices, about 1.3 % of
    a block.
  - The halved lines ease Sympathetic's D-cache misses.
  - `ModalEngine::COST_*` stay as they are until the chip bench re-measures
    String and Sympathetic (plan Task 6).
- **Sound.** Modal's sound changes below −90 dBFS, or below −72 dBFS for
  Sympathetic's loud high notes. Every render that plays Modal is
  re-recorded once (ADR 0011): the goldens `modal_init`, `modal_lfo_cutoff`
  and `algo_to_modal_switch`; the instrument golden `two_parts_two_pairs`
  and its hash before the limiter; and the render of the `init_modal.snd`
  v1 fixture. The fixture's bytes are unchanged.
- **Before acceptance,** the owner compares Sympathetic's high notes against
  the f32 store by ear on the unit. The f32 store stays in the code as the
  reference, so the comparison is a one-line change of default.

## Sources
- `docs/superpowers/specs/2026-09-29-exclusive-state-design.md`, § 4 and
  § Tests.
- The exclusive-state plan, Task 5, where the measurements and the floor
  analysis come from.
- ADR 0040 (string length), ADR 0051 (one engine per voice) and ADR 0011
  (goldens).
