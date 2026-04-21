---
name: Debugging lessons from hardware bringup
description: Critical debugging patterns for STM32 firmware — panic_halt silently freezes, always use NUM_ENCODERS not magic 6, always verify build target
type: feedback
---

panic_halt makes panics SILENT. The firmware just freezes with no indication. An array bounds panic looks identical to a stack overflow or a hardware fault. Always use LED checkpoint sequences when debugging hangs.

**Why:** Spent hours debugging what turned out to be `ENC_LAST_EDGE: [AtomicU32; 6]` accessed with index 0..NUM_ENCODERS (which is 7). The panic froze the CPU on the first `snapshot()` call.

**How to apply:**
- NEVER use magic number 6 for encoder arrays — always `NUM_ENCODERS` 
- When firmware hangs: add LED checkpoint blinks (500ms on, 500ms off, 2s pause between groups) to binary-search the crash location
- Always verify cargo is building for the correct target — `cargo build` from workspace root builds for x86 unless `--target thumbv7em-none-eabihf` is specified. Build from chimera-stm32/ directory or use explicit --target flag.
- The framebuffer (150KB) must be in static BSS, not on the stack. Use `static mut` with `addr_of_mut!` pattern.
- After clean build, verify with `size` command: BSS should be ~153KB (framebuffer), text ~57KB.
