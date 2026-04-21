# Issue #001: Display SPI blocking causes UI sluggishness during DSP rendering

## Problem

The ILI9341 display is driven via bit-banged SPI (`BitBangSpi`). During `display.flush_region()`, the CPU is blocked sending every pixel over GPIO bit-bangs. While the audio DMA ISR preempts this and audio stays glitch-free, the main loop gets starved — UI updates are visibly sluggish, encoders feel laggy, screen repainting is visible.

This was not noticeable before DSP rendering because the sine test tone used negligible CPU. With a full Voice signal chain (FM + Drive + Filter + Wavefolder + VCA) consuming ~20-30% of CPU in the DMA ISR, the remaining cycles for the main loop are barely enough for display updates.

## Root Cause

Bit-banged SPI for display transfer. A 40-row dirty region = 40 × 240 × 2 bytes = 19,200 bytes. At bit-bang speeds (~5-10 MHz effective), this takes ~2-4ms of blocking CPU time per region. Multiple dirty regions per frame compound the problem.

## How PreenFM3 Solves This

PreenFM3 uses three techniques on the same hardware (STM32H750 + ILI9341):

1. **Hardware SPI1 with DMA transfer** — `HAL_SPI_Transmit_DMA()` queues the pixel data and returns immediately. The CPU is free while DMA pushes bytes over SPI.

2. **DMA2D hardware blitter** — STM32H750 has a 2D DMA engine that composites rectangles, fills, and copies into a framebuffer without CPU involvement. PreenFM3 uses it for text rendering and fills.

3. **One strip per ~20ms** — They don't flush all dirty regions at once. Each SysTick (1ms), they process one draw action and push at most one 40-row strip via SPI DMA. The display updates gradually but the CPU is never blocked.

## Proposed Fix (Prioritized)

### Phase 1: Hardware SPI with DMA (biggest impact)

Switch from `BitBangSpi` to STM32H750's hardware SPI1 peripheral with DMA. The display is on SPI1 (PA5=SCK, PA7=MOSI). Configure SPI1 in TX-only master mode, attach a DMA stream, and use non-blocking transfers.

The `flush_region()` call becomes: set up window command (blocking, ~10 bytes), then DMA the pixel data (non-blocking, returns immediately). A completion flag or callback signals when the transfer is done and the next region can be pushed.

**Expected impact:** ~10x reduction in CPU time for display updates. The main loop becomes responsive.

### Phase 2: One strip per frame

Instead of flushing all dirty regions in one frame, flush one region per main loop iteration. Spread updates across multiple frames. This is already partially implemented (dirty region tracking returns a list), but the main loop flushes them all sequentially.

### Phase 3: DMA2D blitter (optional, future)

Use the STM32H750's DMA2D peripheral for framebuffer compositing (rectangle fills, memory copies). This offloads the drawing itself, not just the SPI transfer.

## Workarounds (Current)

- Animation speed increased (0.3 instead of 0.15) so values settle faster
- Snap threshold raised so animations stop sooner
- `libm` calls replaced with fast approximations to reduce ISR CPU usage

## Files Affected

- `chimera-stm32/src/bitbang_spi.rs` — replace with hardware SPI driver
- `chimera-stm32/src/display.rs` — update to use DMA-based SPI
- `chimera-stm32/src/main.rs` — SPI1 peripheral setup, DMA stream allocation

## Priority

**High** — this is the main UX bottleneck. Audio works, DSP works, navigation works, but the instrument feels sluggish. Hardware SPI DMA is the single biggest improvement available.
