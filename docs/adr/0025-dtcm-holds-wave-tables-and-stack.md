# 0025. DTCM holds the wave tables and the stack

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
ADR 0020 gave the stack all 128 KB of DTCM. The algorithmic engine reads
16 waves × 8 mips of `i16` tables (about 64 KB, plus one guard sample per
table) with phase modulation, so its reads land almost anywhere in a
period. From flash through the 16 KB D-cache, the honest worst-case bench
has six voices on different mips (about 21 KB of tables), and it measured
493 cycles per voice-sample. The engine's room is about 415. The stack's
painted high-water mark is about 15 KB.

## Decision
- **The tables' working copy lives at the top of DTCM.** This is the NOLOAD
  section `.dtcm_waves`, 65,792 B at 0x2000_FF00–0x2002_0000, linked by
  `chimera-stm32/build.rs` (`dtcm.x`).
- **The stack is the rest of DTCM.** `_stack_start = __sdtcm_waves`
  (0x2000_FF00) and `_stack_end = ORIGIN(DTCM)`, which is 65,280 B
  (63.75 KB). It grows down toward 0x2000_0000, so an overflow faults in
  reserved space instead of overwriting the tables.
- **Link-time asserts:**
  - the copy ends DTCM;
  - the stack is exactly the rest;
  - the stack keeps at least **32 KB**.
- **The flash tables stay the source.** `shared::copy_waves` copies them
  once at boot. That is one `memcpy` with nothing on the stack, right after
  cache setup, before the bench, `engine::init` and any interrupt.
  `waves::copy_into` then publishes the copy.
- **`WaveId::table` reads whichever tables are published,** through an
  `AtomicPtr` that starts at the flash tables. The host and desktop never
  copy, so they read flash.
- **The AUDIO page's stack mark** paints and scans `[_stack_end,
  _stack_start)`, the stack only.
- This **supersedes ADR 0020's clause** that the stack has all of DTCM.
  ADR 0020's note that DTCM holds `main`'s `UiState` is also out of date:
  `UiState` lives in AXI (`shared.rs`).

## Alternatives considered
- **Leave the tables in flash** — this is the 493 measurement: misses on a
  working set larger than the D-cache.
- **Tables at the bottom of DTCM, stack above** — this was the first
  layout. A stack overflow would silently corrupt the audio tables instead
  of faulting.
- **Copy into AXI SRAM** — it is cached, so it has the same eviction
  pattern with a cheaper refill. DTCM is zero-wait and never evicted.
- **Fewer or shorter tables** — this changes the sound, and the 32 KB stack
  floor already leaves room for today's tables.

## Consequences
- Table reads are zero-wait. On the chip, `KERNEL /VOICE` went from 493 to
  469.
- The stack shrinks from 128 KB to 63.75 KB, which is about 4× the measured
  high-water mark. Anything else that wants DTCM must fit beside the
  32 KB stack floor, or be traded against it.
- Boot copies 64 KB once. `WaveId::table` costs one acquire load per call:
  per block per operator, never per sample.

## Sources
- `docs/superpowers/specs/2026-09-26-algo-engine-design.md` § Addendum:
  kernel bench result and rulings.
- `chimera-stm32/build.rs` (`dtcm.x`), `chimera-stm32/memory.x`,
  `chimera-stm32/src/shared.rs`.
- `chimera-core/src/dsp/algo/waves.rs`,
  `chimera-core/tests/algo_waves_copy_test.rs`.
- STM32H750 reference manual RM0433 §2.3 (the memory map).
