# Audio Test Tone Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get a 440 Hz sine wave out of DAC 1 (CS4344 via SAI1_A) to prove the audio hardware chain.

**Architecture:** Direct register writes for PLL3 clock config and SAI1_A I2S setup. Main loop polls SAI FIFO and writes sine samples. No DMA, no DSP engine — pure hardware bringup.

**Tech Stack:** stm32h7 PAC (0.15, stm32h750 feature), direct register access via PAC, libm for sinf.

**Spec:** `docs/superpowers/specs/2026-04-20-audio-test-tone-design.md`

**IMPORTANT BUILD NOTE:** Always build with explicit target: `cargo build --release --target thumbv7em-none-eabihf -p chimera-stm32`. Running `cargo build --release` from the workspace root builds for x86 (wrong). Always clean before rebuilding: `cargo clean -p chimera-stm32 --target thumbv7em-none-eabihf --release`.

---

## File Map

| Action | File | Responsibility |
|---|---|---|
| Rewrite | `chimera-stm32/src/audio.rs` | `init_pll3()`, `init_sai1a()`, `sai_fifo_has_room()`, `write_sai_data()` |
| Modify | `chimera-stm32/src/main.rs` | SAI pin setup (PE2/4/5/6 AF6), call audio init, sine poll in main loop |

No tests for this plan — it's all hardware register configuration that can only be verified on the physical device. Verification is done via LED checkpoints and listening to the DAC output.

---

### Task 1: Rewrite `audio.rs` with PLL3 and SAI1_A init

**Files:**
- Rewrite: `chimera-stm32/src/audio.rs`

This task replaces the placeholder audio.rs with the actual PLL3 clock configuration and SAI1_A I2S setup using the stm32h7 PAC.

- [ ] **Step 1: Write `audio.rs` with all audio init functions**

Rewrite `chimera-stm32/src/audio.rs` with these contents:

```rust
//! SAI1 Block A audio output — test tone bringup.
//!
//! Configures PLL3 for ~48kHz audio clock, sets up SAI1_A as I2S master TX,
//! and provides FIFO polling + data write helpers.
//!
//! PLL3: HSE 8MHz / M=1 * N=46 / P=3 = 122.67 MHz SAI kernel clock
//! SAI1_A: MCKDIV=5 → MCLK=12.27MHz → FS=47917Hz

use stm32h7xx_hal::pac;

/// Configure PLL3 to produce the SAI audio clock.
/// Must be called after rcc.freeze() — uses read-modify-write to preserve PLL1/PLL2.
///
/// # Safety
/// Modifies shared RCC registers. Call once during init, before SAI is enabled.
pub fn init_pll3() {
    // SAFETY: single-threaded init, no interrupts access RCC at this point
    let rcc = unsafe { &*pac::RCC::ptr() };

    // 1. Enable SAI1 peripheral clock
    rcc.apb2enr.modify(|_, w| w.sai1en().enabled());
    // Small delay for clock to stabilize
    cortex_m::asm::delay(100);

    // 2. Disable PLL3
    rcc.cr.modify(|_, w| w.pll3on().off());
    while rcc.cr.read().pll3rdy().is_ready() {}

    // 3. Set PLL3 input divider: DIVM3 = 1 (preserve DIVM1/DIVM2)
    rcc.pllckselr.modify(|_, w| {
        // SAFETY: DIVM3 is bits 24:29, value 1
        unsafe { w.divm3().bits(1) }
    });

    // 4. Set PLL3 multiplier and dividers: N=46 (val 45), P=3 (val 2)
    rcc.pll3divr.write(|w| unsafe {
        w.divn3().bits(45)  // N-1
         .divp3().bits(2)   // P-1
         .divq3().bits(1)   // Q not used, but must be valid
         .divr3().bits(1)   // R not used, but must be valid
    });

    // 5. Configure PLL3: wide VCO range, enable P output
    rcc.pllcfgr.modify(|_, w| {
        w.pll3vcosel().wide_vco()  // Wide VCO: 1-16 MHz input
         .pll3rge().range8()       // Input range 8-16 MHz
         .divp3en().enabled()      // Enable PLL3_P output
    });

    // 6. Enable PLL3
    rcc.cr.modify(|_, w| w.pll3on().on());
    while !rcc.cr.read().pll3rdy().is_ready() {}

    // 7. Set SAI1 clock source to PLL3_P
    rcc.d2ccip1r.modify(|_, w| {
        // SAI1SEL: 0b001 = PLL3_P
        unsafe { w.sai1sel().bits(0b001) }
    });
}

/// Configure SAI1 Block A as I2S master TX.
/// Call after init_pll3() and after SAI1 pins are configured as AF6.
pub fn init_sai1a() {
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    let cha = sai1.cha();

    // Disable SAI before configuration
    cha.cr1.modify(|_, w| w.saien().disabled());
    // Wait for SAI to be disabled
    while cha.cr1.read().saien().is_enabled() {}

    // CR1: Master TX, Free I2S, 32-bit, MCKDIV=5, MCLK output enabled
    cha.cr1.write(|w| unsafe {
        w.mode().bits(0b00)      // Master TX
         .prtcfg().bits(0b00)    // Free protocol (I2S)
         .ds().bits(0b110)       // 32-bit data
         .mckdiv().bits(5)       // MCLK divider = 5
         .mcken().set_bit()      // MCLK output enable
    });

    // CR2: FIFO threshold 1/4, flush FIFO
    cha.cr2.write(|w| unsafe {
        w.fth().bits(0b001)      // FIFO threshold = 1/4
         .fflush().set_bit()     // Flush FIFO
    });

    // FRCR: 64-bit frame, FS active 32 bits, channel ID, active low, offset 1
    cha.frcr.write(|w| unsafe {
        w.frl().bits(63)         // Frame length = 64 bits
         .fsall().bits(31)       // FS active for 32 bits
         .fsdef().set_bit()      // FS is channel identification
         .fspol().clear_bit()    // FS active low
         .fsoff().set_bit()      // FS one bit before first data
    });

    // SLOTR: 2 slots, both active, 32-bit slot size
    cha.slotr.write(|w| unsafe {
        w.nbslot().bits(0b01)    // 2 slots (N-1)
         .sloten().bits(0b0011)  // Slots 0 and 1 active
         .slotsz().bits(0b10)    // 32-bit slot size
    });

    // Enable SAI
    cha.cr1.modify(|_, w| w.saien().enabled());
}

/// Check if the SAI1_A FIFO has room for more data.
/// Returns true if FIFO level < full (FLVL < 5).
#[inline]
pub fn sai_fifo_has_room() -> bool {
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    let flvl = sai1.cha().sr.read().flvl().bits();
    flvl < 5 // 0=empty, 1=1/4, 2=1/2, 3=3/4, 4=full, 5=full(one frame to write)
}

/// Write a 32-bit sample to the SAI1_A FIFO.
/// Call twice per stereo sample (left then right).
#[inline]
pub fn write_sai_data(sample: i32) {
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    sai1.cha().dr.write(|w| unsafe { w.bits(sample as u32) });
}
```

- [ ] **Step 2: Verify it compiles**

Run:
```bash
cargo clean -p chimera-stm32 --target thumbv7em-none-eabihf --release 2>/dev/null
cargo build --release --target thumbv7em-none-eabihf -p chimera-stm32 2>&1 | grep -E "Compiling chimera-stm32|Finished|^error"
```

Expected: Compiles with warnings about unused functions (they're not called from main yet). There may be PAC API mismatches — if so, adjust the register write syntax to match the actual PAC method signatures. Common issues:
- `.wide_vco()` might be `.medium_vco()` or need different naming
- `.range8()` might need a different variant name
- `.enabled()` / `.disabled()` might be `.set_bit()` / `.clear_bit()`

Fix compile errors by checking the PAC source at `/home/hermes/.cargo/registry/src/*/stm32h7-0.15.*/src/stm32h753/rcc/` for exact method names.

- [ ] **Step 3: Commit**

```bash
git add chimera-stm32/src/audio.rs
git commit -m "feat(stm32): PLL3 + SAI1_A init for audio test tone

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Wire SAI pins and audio init into main.rs

**Files:**
- Modify: `chimera-stm32/src/main.rs`

- [ ] **Step 1: Add SAI pin configuration and audio init calls**

In `chimera-stm32/src/main.rs`, after the GPIOE split (line ~48) and before the display setup, add SAI pin configuration. The PE2/4/5/6 pins are on GPIOE which is already split.

Add after `let mut backlight = gpioe.pe11.into_push_pull_output();` (around line 55):

```rust
    // SAI1 pins (AF6)
    let _sai_mclk = gpioe.pe2.into_alternate::<6>();
    let _sai_fs = gpioe.pe4.into_alternate::<6>();
    let _sai_sck = gpioe.pe5.into_alternate::<6>();
    let _sai_sd_a = gpioe.pe6.into_alternate::<6>();
```

Add after `controls::enable();` (around line 72):

```rust
    // Audio init
    audio::init_pll3();
    audio::init_sai1a();
```

- [ ] **Step 2: Add sine generation to the main loop**

Add a phase accumulator variable before the loop (after `led.set_low();`):

```rust
    let mut audio_phase: f32 = 0.0;
```

Add at the TOP of the main loop body (before `controls.snapshot()`):

```rust
        // Feed SAI FIFO with 440 Hz sine — poll style
        while audio::sai_fifo_has_room() {
            let sample = libm::sinf(audio_phase * 2.0 * core::f32::consts::PI);
            let i32_sample = (sample * 0.5 * (i32::MAX as f32)) as i32;
            audio::write_sai_data(i32_sample); // left
            audio::write_sai_data(i32_sample); // right
            audio_phase += 440.0 / 47917.0;
            if audio_phase >= 1.0 { audio_phase -= 1.0; }
        }
```

Note: the `while` loop drains all available FIFO room each iteration. This ensures the FIFO stays as full as possible between display updates.

- [ ] **Step 3: Verify it compiles**

Run:
```bash
cargo clean -p chimera-stm32 --target thumbv7em-none-eabihf --release
cargo build --release --target thumbv7em-none-eabihf -p chimera-stm32 2>&1 | grep -E "Compiling chimera-stm32|Finished|^error"
```

Expected: Compiles successfully. If PE2/4/5/6 are already consumed by another part of the code, resolve the conflict (they shouldn't be — only PE1 and PE11 are used for LED and backlight).

- [ ] **Step 4: Commit**

```bash
git add chimera-stm32/src/main.rs
git commit -m "feat(stm32): wire SAI pins + sine tone into main loop

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Build, flash, and verify on hardware

**Files:** None (testing only)

- [ ] **Step 1: Build the release binary**

```bash
cargo clean -p chimera-stm32 --target thumbv7em-none-eabihf --release
cargo build --release --target thumbv7em-none-eabihf -p chimera-stm32
rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
```

Verify binary is valid ARM:
```bash
xxd chimera.bin | head -1
```
First 4 bytes should be `0000 0824` (stack pointer = 0x24080000).

- [ ] **Step 2: Flash via DFU**

```bash
dfu-util -a 0 -s 0x08020000:leave -D chimera.bin
```

- [ ] **Step 3: Verify behavior**

Checklist:
- [ ] Screen shows UI (display still works)
- [ ] Controls respond (buttons navigate, encoders change values)
- [ ] DAC 1 output produces audible tone
- [ ] Phone tuner app shows ~440 Hz

If no sound:
1. Check if the LED heartbeat still works (firmware not crashed)
2. If crashed: add LED checkpoints around `init_pll3()` and `init_sai1a()` to find which one fails
3. If running but no sound: the SAI clocking or I2S format may be wrong. Try changing `prtcfg` from 0b00 (I2S) to 0b01 (left-justified) in `init_sai1a()`
4. If sound but wrong pitch: the PLL3 or MCKDIV values need adjustment

- [ ] **Step 4: Commit working binary**

```bash
git add chimera.bin
git commit -m "feat: audio test tone — 440Hz sine out of SAI1_A DAC 1

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>"
```
