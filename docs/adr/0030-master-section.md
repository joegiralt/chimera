# 0030. Feed the delay into the reverb, put tape on pair 1 and a linked compressor last

- **Status:** Accepted (2026-09-27)
- **Deciders:** project owner

## Context
The FX diet (ADRs 0028, 0029) left room in the 1,000-cycle bus for a small
master section. The owner wanted the delay's echoes to sit in the reverb,
some glue and character on the main pair, and one compressor over
everything that leaves the DACs.

## Decision
- **REV SEND**, the delay's new `ParamId(6)` (0–5 in use; ADR 0009): the
  delay's return (wet × MIX) × REV SEND joins the reverb's send in the
  same block, the delay running first. It is smoothed over 20 ms; at 0 the
  bus is bit-identical to before.
- **Tape on DAC pair 1 only**, after the pair-1 sum: wow/flutter, a +6 dB
  pre-emphasis shelf at 3 kHz, a divide-free quintic wide-knee saturator
  (`v − ⅔v³ + ⅕v⁵` on `v` clamped to ±1, so both slope and curvature are
  zero at its ±1 ceiling) run 2× oversampled through a 15-tap half-band
  (49 dB stopband, passband ±0.03 dB to 16 kHz — far less than the
  reverb's 35-tap filter needs, because saturation aliasing needs far less
  stopband than a dark-top band limit), the shelf's exact inverse, a
  +2 dB head bump at 80 Hz, and a one-pole roll-off that darkens with
  DRIVE. Level compensation is `1/gain`, so small-signal gain stays at
  unity at every DRIVE: quiet material is unchanged, loud material is
  squashed. The pre-emphasis puts the highs into the saturator hottest, so
  they saturate before the bass. The law was retuned against the owner's
  own description: a +6 dB input peak comes out +5.67 dB at low DRIVE,
  +3.53 dB at DRIVE ½ and +1.95 dB at DRIVE 1 (harmonics rising with it,
  0.34/2.51/4.89% THD at the same points — `04d824c`); a later pass added
  extra gain above DRIVE ½ (`a8b28e7`) so the top of the knob drives
  hotter still. MIX is parallel, the dry tapped at the wet's 19-sample
  latency (12 for the wow's lookahead, 7 for the oversampler). MIX 0 is an
  exact bypass; switching crossfades over 10 ms.
- **No separate glue compressor on pair 1:** the tape's saturation is the
  glue.
- **A master compressor last**, linked across all three pairs. One
  detector takes the larger of the summed pairs' L and R, peak per 4
  samples, and gives one hard-knee gain to every pair. The gain is
  computed in f32 log2/exp2, attack and release are smoothed in the same
  log domain, and MIX is parallel. MIX 0, or 1:1 with no makeup, is an
  exact bypass. Its gain reduction reaches the UI through one relaxed
  atomic word (`meter::MASTER_GR`).
- **Pages:** DLY keeps TIME, FDBK, TONE, REV SEND, MIX, and WOW and SAT
  move to DLY › CHAR; TAPE gets its own page; MST binds the compressor,
  and the legacy VOL and PAN move to MST › LEVEL. The two sub-page moves
  were assumed defaults pending the owner's word; both shipped unchanged
  through review. The Mix map reads MIX · CHR · DLY · REV · TAPE · MST;
  EFX keeps its id (16) and only its short label becomes REV.
- **Cost:** the plan set tape at 80 and the compressor at 50 cycles per
  sample, taken out of the reverb's share (leaving it 297 of the 1,000).
  The built forms run well over that: tape is about 170–230
  instructions/sample (`e535a79`, 174 steady/~205 worst, before the later
  DRIVE retunes) and the compressor about 59 (`519863c`, mutation
  verified). Task 14's chip bench decides what, if anything, has to give.

## Alternatives considered
- **Share the reverb's 35-tap half-band and a `tanh` saturator:** paper
  cost ran 150–250 instructions against the 80-cycle budget even before
  the divide; saturation aliasing needs far less stopband than the
  reverb's dark-top filter, so a short half-band and a divide-free
  polynomial clip were used instead.
- **A cubic soft clip** (`88a85d5`, the first cut): met the structural
  budget but its knee couldn't be tuned to the owner's "peaks round off,
  not squash" target; replaced by the quintic (`04d824c`).
- **A glue compressor on pair 1 as well:** more cycles than the bus has,
  and it would pump against the master compressor.
- **A compressor per pair:** the pairs would pump apart; one linked gain
  keeps their balance.
- **REV SEND before the delay's MIX:** the reverb would stay loud with
  the delay's return turned down.
- **Tape on every pair:** three times the cost; out of scope.
- **A sidechain input:** out of scope.

## Consequences
- Tape and compressor both run over their original budgets; the final
  bench (Task 14) reads TAPE and COMP on their own rows and decides.
- Pair 1 is 19 samples late only while the tape is on, and the switch
  crossfades so nothing clicks.
- Every audio golden stands: the master section defaults to off.
- TAPE's page has no live-output viz yet
  ([#117](https://github.com/joegiralt/chimera/issues/117)).

## Sources
- `docs/superpowers/specs/2026-09-27-fx-diet-design.md` § Master section.
- `.superpowers/sdd/2026-09-27-fx-diet/progress.md` (Task 8, Task 9 rulings and readings).
- `chimera-core/src/dsp/tape.rs`, `comp.rs`, `fx_bus.rs`, `meter.rs`, `ui/block_registry.rs`; `tests/tape_test.rs`, `comp_test.rs`, `fx_bus_test.rs`.
