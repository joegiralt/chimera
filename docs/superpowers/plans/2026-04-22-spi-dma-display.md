# SPI DMA Display Transfer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace bit-banged SPI display transfer with hardware SPI1 + DMA, freeing the CPU during framebuffer flush and making the UI responsive during DSP rendering.

**Architecture:** Configure SPI1 in TX-only master mode using the `stm32h7xx-hal` SPI driver. For pixel data transfer, set up a DMA stream from the framebuffer to SPI1_TXDR. The `flush_region()` call sends the window command (blocking, ~10 bytes), then queues the pixel data via DMA (non-blocking). A `transfer_busy` flag prevents overlapping transfers. The byte-swap (RGB565 big-endian) is done in-place in the framebuffer before DMA, or the SPI is configured for 16-bit mode with byte swap.

**Tech Stack:** `stm32h7xx-hal 0.16` SPI driver, STM32H750 DMA1 (separate stream from audio), DMAMUX request ID 38 (SPI1_TX)

**Spec:** `docs/issues/001-display-spi-dma.md`

---

## Key Facts

- **SPI1 pins:** PA5 = SCK (AF5), PA7 = MOSI (AF5) — same pins currently used for bit-bang
- **SPI1_TX DMAMUX request ID:** 38 (RM0433 Table 121)
- **Audio uses DMA1_Stream0** — display needs a different stream (DMA1_Stream1 or DMA2)
- **Framebuffer:** 240×320 × 2 bytes = 153,600 bytes in static BSS
- **ILI9341 expects:** RGB565 big-endian (MSB first). STM32 is little-endian.
- **A 40-row region** = 40 × 240 × 2 = 19,200 bytes → at 50 MHz SPI clock, ~0.4ms via DMA vs ~2ms via bit-bang

## Approach

### Phase 1: Hardware SPI (blocking, no DMA yet)

Replace `BitBangSpi` with the HAL's hardware SPI1 driver. Still blocking (CPU waits for transfer), but much faster — hardware SPI at 50 MHz vs bit-bang at ~5 MHz. This alone gives ~10x speedup.

### Phase 2: DMA transfer (non-blocking)

Add DMA to the SPI transfer. The CPU queues the data and returns immediately. A completion flag or polling check prevents overlapping transfers.

This plan implements **Phase 1 only** — hardware SPI blocking. Phase 2 (DMA) can follow once Phase 1 is verified working. Hardware SPI alone should make the UI feel dramatically more responsive.

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `chimera-stm32/src/main.rs` | **Modify** | Configure PA5/PA7 as AF5 for SPI1 instead of push-pull GPIO. Create HAL SPI1 instance. |
| `chimera-stm32/src/display.rs` | **Modify** | Accept HAL SPI type instead of BitBangSpi. Use 16-bit SPI mode for byte-swap-free transfer. |
| `chimera-stm32/src/bitbang_spi.rs` | **Delete** | No longer needed. |

---

### Task 1: Switch PA5/PA7 to hardware SPI1

**Files:**
- Modify: `chimera-stm32/src/main.rs`

- [ ] **Step 1: Read current pin configuration**

Currently PA5 and PA7 are configured as push-pull outputs for bit-banging:
```rust
let sck = gpioa.pa5.into_push_pull_output();
let mosi = gpioa.pa7.into_push_pull_output();
let spi = BitBangSpi::new(sck, mosi, 0x5802_0000, 5, 7);
```

- [ ] **Step 2: Replace with HAL SPI1 construction**

```rust
use stm32h7xx_hal::spi;

// SPI1 pins: PA5 = SCK (AF5), PA7 = MOSI (AF5)
let sck = gpioa.pa5.into_alternate::<5>();
let mosi = gpioa.pa7.into_alternate::<5>();

// Configure SPI1: TX-only master, 8-bit, MODE_0, 50 MHz
let spi1 = dp.SPI1.spi(
    (sck, spi::NoMiso::new(), mosi),
    spi::Config::new(spi::MODE_0)
        .communication_mode(spi::CommunicationMode::Transmitter),
    50.MHz(),
    ccdr.peripheral.SPI1,
    &ccdr.clocks,
);
```

Note: We need `dp.SPI1` which means it can't be consumed elsewhere. The `ccdr.peripheral.SPI1` provides the clock enable.

- [ ] **Step 3: Update display construction**

```rust
let mut display = Stm32Display::new(spi1, dc, reset, cs);
```

The `Stm32Display` generic types will change from `BitBangSpi<...>` to `Spi<SPI1, Enabled, u8>`.

- [ ] **Step 4: Remove BitBangSpi import and `mod bitbang_spi`**

Delete `mod bitbang_spi;` and the `use bitbang_spi::BitBangSpi;` from main.rs.

- [ ] **Step 5: Build**

Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`
Expected: May fail if display.rs type bounds don't match the HAL SPI type. Fix in Task 2.

- [ ] **Step 6: Commit**

```bash
git add chimera-stm32/src/main.rs
git commit -m "feat(stm32): configure PA5/PA7 as hardware SPI1 for display"
```

---

### Task 2: Update display driver for hardware SPI

**Files:**
- Modify: `chimera-stm32/src/display.rs`

The display driver already uses the `embedded_hal::blocking::spi::Write<u8>` trait. The HAL's SPI type implements this trait, so the display code should work with minimal changes.

- [ ] **Step 1: Verify trait bounds**

The `Stm32Display` struct is generic over `SPI: Write<u8>`. The HAL's `Spi<SPI1, Enabled, u8>` implements `Write<u8>`. This should just work.

- [ ] **Step 2: Optimize flush_region for hardware SPI**

The current code converts pixels to big-endian bytes in 512-byte chunks:
```rust
let mut bytes = [0u8; 512];
for chunk in fb()[start..end].chunks(256) {
    for (i, &pixel) in chunk.iter().enumerate() {
        bytes[i * 2] = (pixel >> 8) as u8;
        bytes[i * 2 + 1] = pixel as u8;
    }
    let _ = self.spi.write(&bytes[..chunk.len() * 2]);
}
```

With hardware SPI at 50 MHz, larger chunks are better. Increase to 2048 bytes:
```rust
let mut bytes = [0u8; 2048];
for chunk in fb()[start..end].chunks(1024) {
    for (i, &pixel) in chunk.iter().enumerate() {
        bytes[i * 2] = (pixel >> 8) as u8;
        bytes[i * 2 + 1] = pixel as u8;
    }
    let _ = self.spi.write(&bytes[..chunk.len() * 2]);
}
```

Or even better: use the SPI in 16-bit mode with hardware byte swap. But that requires changing the SPI type parameter from `u8` to `u16` and using `Write<u16>`. The STM32H7 SPI can do byte-order swap in hardware (CFG1.BYTEORD bit). This would let us write the framebuffer directly without the byte-swap loop.

For Phase 1, keep the byte-swap loop but with larger chunks. Phase 2 will use DMA with 16-bit mode.

- [ ] **Step 3: Build and test**

Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`

- [ ] **Step 4: Commit**

```bash
git add chimera-stm32/src/display.rs
git commit -m "feat(stm32): display uses hardware SPI1 — larger transfer chunks"
```

---

### Task 3: Delete bitbang_spi.rs

**Files:**
- Delete: `chimera-stm32/src/bitbang_spi.rs`
- Modify: `chimera-stm32/src/main.rs` (remove `mod bitbang_spi`)

- [ ] **Step 1: Delete the file**

```bash
rm chimera-stm32/src/bitbang_spi.rs
```

- [ ] **Step 2: Remove the module declaration from main.rs**

Remove `mod bitbang_spi;` and any remaining `use bitbang_spi::...` lines.

- [ ] **Step 3: Build**

Run: `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor(stm32): remove BitBangSpi — replaced by hardware SPI1"
```

---

### Task 4: Flash and verify

- [ ] **Step 1: Build and flash**

```bash
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000:leave
```

- [ ] **Step 2: Verify display**

- Screen renders correctly (all pages, dungeon map, visualizations)
- UI feels significantly more responsive
- Encoder turns update the display faster
- LFO animation on modulated params is smoother
- No display corruption or glitches

- [ ] **Step 3: Verify audio**

- Audio still plays (DMA audio uses DMA1_Stream0, SPI uses blocking — no conflict)
- No audio glitches during display updates

- [ ] **Step 4: Commit**

```bash
git add chimera.bin
git commit -m "feat(stm32): hardware SPI1 display — verified on hardware"
```

---

## Notes

### Why Phase 1 Only (blocking hardware SPI, no DMA yet)

1. **Hardware SPI alone is ~10x faster than bit-bang.** At 50 MHz SPI clock vs ~5 MHz effective bit-bang, a 40-row region takes ~0.4ms instead of ~4ms. This should make the UI feel responsive.

2. **DMA adds complexity.** Non-blocking DMA requires a completion flag, careful CS/DC pin management, and a state machine to handle "transfer in progress" while the main loop wants to start another transfer. Get blocking working first, then optimize.

3. **Audio DMA is on DMA1_Stream0.** Display DMA would need a separate stream. Using DMA2 instead of DMA1 avoids any bus contention, but needs separate configuration.

### SPI Clock Speed

The STM32H7 SPI1 kernel clock comes from PLL1_Q by default. With our 400 MHz sysclk and typical PLL1_Q of 200 MHz, the SPI prescaler of 4 gives 50 MHz. The ILI9341 supports up to ~60 MHz SPI clock, so 50 MHz is safe.

### Byte Order

The ILI9341 expects RGB565 data in big-endian (MSB first). The STM32 is little-endian. Options:
1. **Software byte swap** in the transfer loop (current approach, works with DMA too if we swap in-place)
2. **SPI hardware byte swap** via CFG1.BYTEORD in 16-bit mode (most efficient, eliminates the swap loop entirely)

Phase 1 keeps the software swap. Phase 2 should use hardware byte swap with 16-bit SPI + DMA.
