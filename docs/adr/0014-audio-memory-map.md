# 0014. Voices in D2, FX bus in AXI; buffers sized to the range they serve

- **Status:** Accepted (2026-09-24)
- **Deciders:** project owner

## Context
ADR 0013 makes every audio-core layout fit the STM32H750 on both builds.
Measured on `thumbv7em-none-eabihf` before this change: `Voice` 67,832 B
(Modal's eight 2,048-sample string buffers are 65,664 B of it), so
`[Voice; 6]` = 406,992 B against 288 KB of D2 SRAM; the FX bus (chorus
16,400 + tape delay 192,016 + reverb 215,180) = 423,596 B, which the
instrument-core spec did not budget at all. AXI (512 KB) also holds the
150 KB framebuffer and `main`'s stack with the UI (73,744 B frame in a
release build).

## Decision
- **Voices in D2.** `Instrument` (the `[Voice; 6]` pool, allocator, part
  buses) is asserted `<= VOICE_RAM_BUDGET` = D2 (294,912) − 8 KB kept for
  DMA/MIDI buffers = 286,720 B. The firmware reserves it in
  `.ram_d2.voices` after the DMA buffer, so the linker proves it fits.
- **Modal strings sized to E1.** `MAX_STRING_DELAY` = 1,200 samples: E1
  (MIDI 28, 41.2 Hz, period 1,164 at 48 kHz) and above play at their exact
  period; lower notes clamp (~40 Hz floor; before: notes ≤ 18 clamped at
  2,047). `Voice` = 40,696 B, `Instrument` = 246,600 B.
- **FX bus in AXI**, asserted with everything else there: framebuffer
  153,600 + UI reserve 65,536 + Performance 5,212 + SoundPool 26,496 +
  2 × AudioShared 6,216 + FxBus 253,612 = 510,672 of 524,288 B.
  `Instrument::render` borrows the `FxBus` so the two can live in
  different regions.
- **FX buffers sized to their ranges.** Reverb lines cover their longest
  fixed tap (plate tank allpass 4,096 → 2,048, tank delay 8,192 → 4,800,
  FDN 2,048 → 1,152): output is bit-identical (locked by
  `fx_golden_test.rs`). The tape delay's range becomes 10–500 ms (was
  10–1,000) so its line is 24,064 samples (was 48,000): output is
  identical up to 500 ms.

## Alternatives considered
- **Lower `MAX_VOICES` to 4** — fits the old strings in D2 but loses two
  voices and does nothing for the FX bus.
- **Pool Modal's buffers** — only Modal voices need them, but the pool
  couples allocation to engine choice; more code for less than the 27 KB
  per voice the trim saves.
- **Voices in AXI, FX in D2** — the FX bus (≥ 333 KB even with the reverb
  trimmed and a 1 s delay) does not fit D2.
- **16-bit delay line, or one arena for the three reverbs** — change every
  delay/reverb output, or need a clear on type change inside the ISR.

## Consequences
Modal notes below E1 clamp; the delay tops out at 500 ms. AXI has
~13 KB headroom and D2 ~40 KB. The UI reserve is sized from a release
build's measured stack (`main` 73,744 B including UiState's Performance
and pool); a debug firmware (core at opt-level 0) needs far more stack and
is only link-checked. Before the port touches the pool, check that the D2
SRAM clocks (RCC_AHB2ENR SRAM1/2/3EN) are on; today only the 512 B DMA
buffer lives there.

## Sources
`chimera-core/tests/memory_budget_test.rs`, `fx_golden_test.rs`;
`docs/superpowers/plans/2026-09-24-instrument-core.md` (measurements);
RM0433 §2.3 memory map.
