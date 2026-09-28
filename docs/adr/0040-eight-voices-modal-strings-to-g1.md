# 0040. Eight voices in D2; Modal strings sized to G1

- **Status:** Proposed (2026-09-28)
- **Deciders:** project owner
- Supersedes in part [0014](0014-audio-memory-map.md) (Modal strings sized
  to E1) and [0031](0031-six-voices-on-rev-v.md) (six voices as the pool).

## Context
The owner asked for eight voices, to stress test on rev V. The cost model
(ADR 0026, 0031) already gates admission: 7,000 cycles/sample, less the
FX bus's 1,360, leaves 5,640 for voices, so any voice billed at 705 or
less fits eight times. The pool, not the budget, was the limit.

Measured on `thumbv7em-none-eabihf` at six voices: `Voice` 42,112 B, of
which Modal's eight 1,200-sample `f32` strings are 38,400 B; `Instrument`
255,272 B. Eight voices would be 339 KB against the 286,720 B
`VOICE_RAM_BUDGET` (D2 less 8 KB). Free RAM in the six-voice ELF: D2
36 KB, AXI about 154 KB (`.data` + `.bss` 369,976 B of 524,288), DTCM
none beyond the 32 KB stack floor (ADR 0025).

D2 holds the voices for capacity alone (ADR 0014: the FX bus did not
fit there). D2 and AXI SRAM are both write-back cached (only the 4 KB DMA
region is not, ADR 0020), and AXI is the faster of the two on a miss, so
neither region is a performance constraint.

## Decision
- `MAX_VOICES` = 8, the whole pool still in D2 (`.ram_d2.voices`).
- `MAX_STRING_DELAY` = 984: G1 (MIDI 31, 49.0 Hz, period 979) and above
  play at their exact period; E1, F1 and F♯1 clamp to 983 samples
  (48.8 Hz) and sound sharp. `Voice` = 35,200 B, `[Voice; 8]` =
  281,600 B, `Instrument` = 284,256 B (host) / 284,632 B (firmware) of
  286,720 B. D2 in the firmware ELF: 287,704 of 294,912 B.
- Admission is unchanged: the allocator bills each voice by `Voice::cost`.
  On rev V the TX and single-oscillator factory Sounds (555–692) get
  eight; MORPH PAD (839), MORPH KEYS (844) and A16 ∪ A17 (889) get six.
  On rev Y the TX Sounds get six, SAW LEAD eight, SQR BASS seven.

## Alternatives considered
- **Six voices in D2, two in AXI.** AXI has the room, and the placement
  can be asserted, but `Instrument` holds its pool inline: a split pool
  means `Instrument` borrowing out-of-line voices from two statics, which
  every host test and the desktop would have to build. Worth it only if
  the lowest Modal notes matter more than that plumbing.
- **The whole `Instrument` in AXI, the FX bus in D2.** The FX bus
  (162 KB) fits D2, but the eight-voice `Instrument` (339 KB) beside the
  framebuffer and UI is 547 KB of AXI's 512.
- **16-bit string buffers.** Halves the strings and keeps E1, but
  quantizes a feedback loop and changes every Modal output.
- **Trim `D2_DMA_RESERVE` to the 4 KB DMA region.** Only buys 4 KB; with
  it the strings could stay at 1,000 samples, still short of E1.

## Consequences
Modal notes below G1 clamp (before: below E1). About 2 KB of the D2
budget is left, so any growth in `Voice` (the modulation modes) fails the
build until something gives: the Modal 2 engine (Elements exciters, Rings
resonators) is the natural point to revisit string sizing. Eight voices
of a light patch are about two more voices of CPU than six; heavy patches
are billed as before.

## Sources
`chimera-core/tests/memory_budget_test.rs`, `cost_test.rs`,
`instrument_test.rs`; `llvm-size -A` of the release ELF; RM0433 §2.3.
