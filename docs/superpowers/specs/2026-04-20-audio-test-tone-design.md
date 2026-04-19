# Audio Bringup — SAI Test Tone

## Goal

Get a clean 440 Hz sine wave out of DAC 1 (CS4344 on SAI1_A) to prove the entire audio chain: PLL3 clock → SAI1 master → I2S data → DAC → analog output.

## Architecture

No DMA for this step. The main loop polls the SAI FIFO status flag and writes stereo samples when there's room. This is a temporary bringup approach — the real engine will use DMA circular buffers in RAM_D2.

## Signal Chain

```
PLL3 (MCLK) → SAI1_A (master, I2S, 32-bit, stereo)
                  ↓
            SAI1_A FIFO ← main loop fills with sine samples
                  ↓
          PE6 (SD_A) → CS4344 DAC 1 → analog out
```

## Clock Configuration

After `rcc.freeze()`, configure PLL3 via direct register writes matching PreenFM3:

```
PLL3: M=1, N=46, P=3
Input = HSE 8 MHz / 1 = 8 MHz
VCO = 8 MHz × 46 = 368 MHz
PLL3_P = 368 / 3 ≈ 122.67 MHz → SAI kernel clock
```

SAI1_A divides this down internally to produce MCLK, SCK, and FS for ~48 kHz sample rate. The actual rate will be 47,916 Hz (same as PreenFM3) due to integer divider constraints.

### PLL3 Register Sequence

1. Disable PLL3: `RCC.CR` clear PLLR3ON
2. Wait for PLL3RDY = 0
3. Set `RCC.PLLCKSELR`: PLL3 source = HSE, DIVM3 = 1
4. Set `RCC.PLL3DIVR`: DIVN3 = 45 (N-1), DIVP3 = 2 (P-1)
5. Enable PLL3P output: `RCC.PLLCFGR` set DIVP3EN
6. Set PLL3 VCO range: wide (1-16 MHz input)
7. Enable PLL3: `RCC.CR` set PLL3ON
8. Wait for PLL3RDY = 1
9. Set SAI1 clock source to PLL3_P: `RCC.D2CCIP1R` SAI1SEL = 0b01

## SAI1_A Configuration

Direct register writes to SAI1 Block A control registers, matching PreenFM3:

| Register | Value | Meaning |
|---|---|---|
| CR1 | MODE=0 (master TX), PRTCFG=0 (free I2S), DS=0b110 (32-bit), MCKDIV=TBD | Master TX, 32-bit |
| CR2 | FTH=0b001 (FIFO 1/4), FFLUSH=1 | FIFO threshold |
| FRCR | FSALL=31, FRL=63, FSDEF=1, FSPOL=0, FSOFF=1 | 64-bit frame, FS active low |
| SLOTR | NBSLOT=1, SLOTEN=0b11, SLOTSZ=0b10 (32-bit) | 2 slots, both active |

MCKDIV calculation:
- SAI kernel clock = PLL3_P ≈ 122.67 MHz
- MCLK = kernel / MCKDIV
- For 48 kHz with 64-bit frame (32-bit × 2 channels): SCK = 48000 × 64 = 3.072 MHz
- MCLK is typically 256×FS = 12.288 MHz
- MCKDIV = 122.67 / (2 × 12.288) ≈ 5 → MCKDIV=5

After configuration:
1. Enable MCLK output (CR1.MCKEN = 1)
2. Enable SAI (CR1.SAIEN = 1)
3. Wait for SAI ready

## Pin Setup

All SAI1 pins via the HAL's `into_alternate::<6>()` (AF6 for SAI1):

| Pin | Function |
|---|---|
| PE2 | SAI1_MCLK_A |
| PE4 | SAI1_FS_A |
| PE5 | SAI1_SCK_A |
| PE6 | SAI1_SD_A |

## Sine Generation

Phase accumulator polled from main loop:

```rust
// Main loop alongside controls + display:
if sai_fifo_has_room() {
    let sample = libm::sinf(phase * 2.0 * PI);
    let i32_sample = (sample * 0.5 * (i32::MAX as f32)) as i32;
    write_sai_data(i32_sample); // left
    write_sai_data(i32_sample); // right (mono)
    phase += 440.0 / 48000.0;
    if phase >= 1.0 { phase -= 1.0; }
}
```

FIFO room is checked via SAI1 Block A status register (SAI_xSR.FLVL < 5, meaning FIFO not full).

The amplitude is scaled to 50% (`* 0.5`) to avoid clipping the DAC.

## What Lives Where

| File | Responsibility |
|---|---|
| `chimera-stm32/src/audio.rs` | `init_pll3()`, `init_sai1a(pins)`, `sai_fifo_has_room() -> bool`, `write_sai_data(i32)` |
| `chimera-stm32/src/main.rs` | Configure PE2/4/5/6 as AF6, call audio init, add sine poll to main loop |

## What This Does NOT Do

- No DMA — FIFO polled from main loop
- No SAI1_B or SAI2_A — just one DAC
- No chimera-core DSP — hardcoded sine
- No interrupt-driven audio — main loop only
- No MIDI — fixed 440 Hz
- No MPU configuration for DMA coherence (not needed without DMA)

## Success Criteria

- Phone tuner app shows 440 Hz
- Clean sine, no clicks/pops/distortion
- Doesn't interfere with display or controls (main loop still services both)
- LED heartbeat continues running (no hangs)

## Future Steps (not in this spec)

1. Add DMA circular buffers in RAM_D2 for interrupt-driven audio
2. Wire up chimera-core Voice engine in DMA half-transfer callback
3. Add SAI1_B + SAI2_A slaves for 6-channel output
4. MIDI input via USART1
