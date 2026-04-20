# DMA Circular Buffer Audio for SAI1_A

## Goal

Replace FIFO polling with DMA1 circular transfer so SAI1_A plays continuously regardless of what the main loop is doing. Audio should not glitch during display rendering or control handling.

## Architecture

DMA1_Stream0 continuously reads from a double-buffered static array in RAM_D2 (0x30000000) and writes to the SAI1_A data register. Half-transfer and transfer-complete ISRs fill the inactive half of the buffer with sine samples. The main loop has no audio responsibilities.

## Memory Layout Change

Add RAM_D2 to `memory.x`:

```
FLASH  (rx)  : ORIGIN = 0x08020000, LENGTH = 896K
RAM    (rwx) : ORIGIN = 0x24000000, LENGTH = 512K
RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 32K
```

32K is sufficient for the audio buffer (512 bytes) with room for future SAI1_B and SAI2_A buffers.

The linker script needs a `.ram_d2` output section that maps to the RAM_D2 region. Add to `build.rs` or use a custom `link.x` snippet.

## DMA Buffer

```rust
#[link_section = ".ram_d2"]
static mut AUDIO_BUF: [i16; 256] = [0; 256];
```

- 256 × i16 = 512 bytes
- 128 stereo pairs total (64 per half)
- Half-transfer ISR fills `[0..128]` (64 stereo pairs)
- Transfer-complete ISR fills `[128..256]` (64 stereo pairs)
- At 47917 Hz, each half = 64/47917 = 1.34ms — ISR fires at 750 Hz

## Data Flow

```
Static AUDIO_BUF in RAM_D2 (0x30000000)
  [half A: 128 i16] [half B: 128 i16]
       ↑                    ↑
   ISR fills A          ISR fills B
   while DMA            while DMA
   reads B              reads A
       │                    │
       └────── DMA1_Stream0 ──────→ SAI1_A DR (16-bit)
                (circular)              │
                                   CS4344 DAC 1
```

## DMA1_Stream0 Configuration

Direct register writes to DMA1 Stream 0:

| Register/Field | Value | Meaning |
|---|---|---|
| CR.DIR | 01 | Memory to peripheral |
| CR.CIRC | 1 | Circular mode |
| CR.MINC | 1 | Memory address increment |
| CR.PINC | 0 | Peripheral address fixed |
| CR.MSIZE | 01 | Memory data size = 16-bit |
| CR.PSIZE | 01 | Peripheral data size = 16-bit |
| CR.PL | 11 | Priority = Very High |
| CR.HTIE | 1 | Half-transfer interrupt enable |
| CR.TCIE | 1 | Transfer-complete interrupt enable |
| NDTR | 256 | Total number of data items |
| PAR | SAI1_CHA_DR address (0x40015824) | Peripheral address |
| M0AR | &AUDIO_BUF | Memory base address |

DMAMUX: DMA1_Stream0 request must be mapped to SAI1_A. On STM32H7, the DMAMUX channel request ID for SAI1_A is 87 (from reference manual Table 121).

### DMAMUX Configuration

```
DMAMUX1_Channel0.CCR = 87  (SAI1_A request)
```

### DMA FIFO

Disabled (direct mode). For 16-bit transfers at audio rates, direct mode is fine. No burst.

## ISR Handler

```rust
#[interrupt]
fn DMA1_STR0() {
    let dma1 = unsafe { &*pac::DMA1::ptr() };

    if dma1.lisr.read().htif0().bit_is_set() {
        // Half-transfer: fill first half [0..128]
        dma1.lifcr.write(|w| w.chtif0().set_bit()); // clear flag
        fill_sine_buffer(0);
    }

    if dma1.lisr.read().tcif0().bit_is_set() {
        // Transfer-complete: fill second half [128..256]
        dma1.lifcr.write(|w| w.ctcif0().set_bit()); // clear flag
        fill_sine_buffer(128);
    }
}
```

The sine phase accumulator is a `static mut` variable accessed only from this ISR (safe — single interrupt, not re-entrant).

```rust
static mut SINE_PHASE: u32 = 0;

fn fill_sine_buffer(offset: usize) {
    let buf = unsafe { &mut AUDIO_BUF };
    let phase_inc: u32 = 39_472_883; // 440 Hz at 47917 Hz
    unsafe {
        for i in (0..128).step_by(2) {
            let idx = (SINE_PHASE >> 24) as usize;
            let sample = (SINE_TABLE[idx] >> 16) as i16;
            buf[offset + i] = sample;     // left
            buf[offset + i + 1] = sample; // right
            SINE_PHASE = SINE_PHASE.wrapping_add(phase_inc);
        }
    }
}
```

Uses the same 256-entry sine lookup table from the test tone implementation.

## SAI1_A Changes

The existing `init_sai1a()` stays the same (16-bit I2S, MCKDIV=5, etc). Two additions:

1. Set DMAEN bit in CR1 (enable DMA request)
2. Do NOT enable SAI until DMA is configured and buffer is pre-filled

Startup sequence:
1. `init_pll3()` — configure audio clock
2. `init_sai1a()` — configure SAI registers (SAI disabled)
3. Pre-fill entire `AUDIO_BUF` with sine data
4. Configure DMA1_Stream0
5. Enable DMA stream
6. Enable SAI (CR1.SAIEN = 1) — DMA starts transferring

## NVIC Configuration

DMA1_Stream0 interrupt must be enabled in NVIC with appropriate priority:
- Priority 3 (same as PreenFM3) — below display SPI (2) and MIDI USART (1)
- The `#[interrupt] fn DMA1_STR0()` handler is registered via cortex-m-rt

## Main Loop Change

Remove the `while audio::sai_fifo_has_room() { ... }` block and the sine table from main.rs. The main loop only does controls + display:

```rust
loop {
    controls.snapshot();
    if controls.has_activity() {
        ui.handle_input(&controls);
    }
    ui.update();
    let flush_list = ui.render_dirty(&mut display, &perf.stats);
    for &(ys, ye) in &flush_list {
        if ys != ye { display.flush_region(ys, ye); }
    }
}
```

## What Lives Where

| File | Change |
|---|---|
| `chimera-stm32/memory.x` | Add `RAM_D2 (rwx) : ORIGIN = 0x30000000, LENGTH = 32K` |
| `chimera-stm32/build.rs` | Add `.ram_d2` section to linker script output |
| `chimera-stm32/src/audio.rs` | Add DMA init, ISR, sine fill, buffer declaration. Move sine table here. |
| `chimera-stm32/src/main.rs` | Remove FIFO polling + sine table. Call `audio::start_dma()` after SAI init. |

## MPU Configuration

RAM_D2 should ideally be configured as write-through cacheable for DMA coherence. For the test tone this is not critical (the DMA reads are small and infrequent relative to the write rate). MPU config can be added when DSP performance matters.

Default without MPU: RAM_D2 is uncached on Cortex-M7, which means DMA sees writes immediately. This is correct but slower for DSP computation. Fine for test tone.

## What This Does NOT Do

- No Voice engine — still hardcoded sine
- No SAI1_B or SAI2_A — single DAC only
- No MIDI — fixed 440 Hz
- No MPU tuning — default uncached RAM_D2
- No error handling for DMA underrun (FIFO error flags)

## Success Criteria

- Continuous 440 Hz sine with zero dropouts
- Turning encoders and navigating pages produces NO audio glitches
- Display and controls remain fully responsive
- ISR at 750 Hz doesn't interfere with SysTick at 500 Hz
