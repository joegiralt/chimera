# 0031. Give every patch six voices on rev V

- **Status:** Accepted (2026-09-27); supersedes in part [0026](0026-algo-voices-billed-by-patch-shape.md) (its voice counts on rev V: four for the costliest patch, four and five for factory Sounds) and [0027](0027-shedding-fades-tails-first.md) (its "rev Y gets two")
- **Deciders:** project owner

## Context
ADR 0026 billed voices by patch shape beside a 3,310-cycle FX bus, so the
costliest patch got four voices on rev V and two on rev Y. The FX diet
(ADRs 0028–0030) put one Alesis-style ring, stereo returns and a master
section (tape, then a linked compressor) on the bus in place of Plate, FDN
and MidiVerb, aimed at ≤ 1,000 cycles.

The first on-chip bench read far over that target. The cycle-accurate
emulator traced it to instruction count, not algorithm: the ring's
per-sample reads walked a wrapped, multi-head buffer with a wrap branch at
every tap (688 instructions/sample crossfading, 410 fixed), the chorus's
float LFO phase and branchy triangle and clamp cost it double, and the
tape's per-block history copy-in added overhead with no audible effect.
Four fixes cut it down: `d7c6e0a` (the ring's shared write head, unrolled
stage loops, wrap-free segment reads: 688 → 274 crossfading, 410 → 200
fixed), `c5b8618` (the ring's rounding on VCVTA/VRINTA: 274 → 239
crossfading, 200 → 158 fixed), `335cd3a` (the chorus's u32 LFO phase and
branch-free triangle and clamp: 227 → 121 instructions/sample per line)
and `fa2790f` (the tape's scratch buffers living in its state, no
per-block zeroing or copy). `e038c59` set FZ and DN at boot (FPSCR,
FPDSCR) as insurance against denormal stalls in decaying tails.

With those in, the bench (rev V, 480 MHz, 2026-09-27) read, cycles/sample:
MIX 148, CHORUS 115, DELAY 183, REVERB 485, TAPE 280, COMP 79, BUS 1,356 —
still over 1,000, but the six-voice floor no longer needs it. Rather than
spend the spec's fallbacks (the delay's Padé clip, a shared chorus LFO,
the compressor's gain every 8 samples, two ring taps per side) to chase a
budget that buys no more voices, the owner accepted 1,356 and parked
further optimisation as [#141](https://github.com/joegiralt/chimera/issues/141).

## Decision
- `FxBus::COST` = 1,360, the BUS reading (1,356) rounded up (rev V, 480
  MHz).
- The costliest patch, A16 ∪ A17 (842 in the model, 821 measured), gets
  six voices on rev V (6 × 842 + 1,360 = 6,412 ≤ 7,000) and 5 on rev Y
  (5,833 cycles: (5,833 − 1,360) / 842 = 5.3).
- Every factory Sound gets six voices on rev V.

## Alternatives considered
- **Apply the spec's fallbacks to reach ≤ 1,000 anyway:** each trades a
  small, deliberate sound change (a cheaper saturation curve, a shared
  LFO, a coarser compressor control rate, fewer reverb taps) for headroom
  the voice count does not need at 1,360; parked with the budget in #141
  instead of spent now.
- **Spend the freed cycles on a second reverb or effect:** at 1,360 there
  are only about 98 cycles/voice beyond A16 ∪ A17's 842 (7,000 − 1,360 =
  5,640; 5,640 / 6 − 842 = 98) — not enough for another effect; the
  engine's modulation modes need them instead.

## Consequences
Nothing sheds on rev V: 6 × 842 + 1,360 = 6,412 ≤ 7,000. ADR 0027's
shedding still governs rev Y and any costlier engine; its tests pin the
pre-diet voice share. About 98 cycles per voice are left for the
modulation modes. The FX bus still runs above the spec's 1,000-cycle
target; closing that gap is tracked, not blocking, in #141.

## Sources
- `docs/superpowers/specs/2026-09-27-fx-diet-design.md` § Intent.
- Bench readings of 2026-09-27; `chimera-core/tests/cost_test.rs`,
  `chimera-core/tests/instrument_test.rs`.
- `d7c6e0a`, `c5b8618`, `335cd3a`, `fa2790f`, `e038c59`.
- [#141](https://github.com/joegiralt/chimera/issues/141).
