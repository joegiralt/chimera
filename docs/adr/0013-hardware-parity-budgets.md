# 0013. The simulator enforces the chip's limits

- **Status:** Accepted (2026-09-24)
- **Deciders:** project owner

## Context
Development moved desktop-first. A simulator with a desktop CPU and gigabytes
of RAM makes it easy to design something the STM32H750 cannot run
(480 MHz Cortex-M7; 512 KB AXI SRAM + 288 KB D2 SRAM + 128 KB DTCM; 896 KB
firmware flash; ~150 KB of RAM already taken by the framebuffer). One `Voice`
is ~68 KB today, so "6 voices" alone is ~408 KB. Discovering that at port
time means rewriting the design.

## Decision
The desktop build enforces the same limits as the hardware:
- Target limits live as constants in `chimera-core/src/hw.rs`
  (`MAX_VOICES`, `MAX_PARTS`, `DAC_PAIRS`, `BLOCK_SIZE`, sample rate, cycle
  budget, per-region RAM budgets), shared by both builds.
- All audio-core state is fixed-size; `const` size assertions per memory
  region fail the build on **both** targets when a layout stops fitting.
  No desktop-only escape hatches.
- CPU is enforced through per-engine `Cost` values and an allocator that
  refuses voices over the cycle budget — the same code on both targets.
  Costs are estimates until measured on hardware with the DWT cycle counter,
  then replaced by measurements.
- `just check` includes building and linking the firmware.

## Alternatives considered
- **Develop freely on desktop, fit to hardware at port time** — the failure
  mode this ADR exists to prevent.
- **Measure CPU on desktop with a scale factor** — too inaccurate across
  caches, FPU and memory systems to be a budget.
- **Emulate the Cortex-M7 (QEMU/Renode)** — not cycle-accurate enough to
  trust, and slow.

## Consequences
Some designs fail to compile on desktop until they fit (e.g. Modal's string
buffers). CPU limits are only as good as the cost numbers; the hardware
benchmark is required before trusting them. Capacity changes are one
constant (`MAX_VOICES`, `MAX_PARTS`).

## Sources
`docs/superpowers/specs/2026-09-24-instrument-core-design.md`;
STM32H750 memory map in `docs/chimera-synth-design.md`; `chimera-stm32/memory.x`.
