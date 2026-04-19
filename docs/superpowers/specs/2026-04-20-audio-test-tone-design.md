# Audio Bringup — SAI Test Tone

## Goal

Get a clean 440 Hz sine wave out of DAC 1 (CS4344 on SAI1_A) to prove the entire audio chain: PLL3 clock → SAI1 master → I2S data → DAC → analog output.

## Architecture

No DMA for this step. The main loop polls the SAI FIFO status flag and writes stereo samples when there's room. This is a temporary bringup approach — the real engine will use DMA circular buffers in RAM_D2.

**Known limitation:** The main loop also services display rendering (~50-80ms per partial flush). The SAI FIFO is only 8 words deep (~167μs at 48kHz). Any main loop stall >167μs causes a FIFO underrun → audible click. This is expected and acceptable for a bringup test. The test proves "does the DAC produce 440Hz" not "is it glitch-free."

## Signal Chain

```
PLL3 (MCLK) → SAI1_A (master, I2S, 32-bit, stereo)
                  ↓
            SAI1_A FIFO ← main loop fills with sine samples
                  ↓
          PE6 (SD_A) → CS4344 DAC 1 → analog out
```

## Clock Configuration

After `rcc.freeze()`, configure PLL3 via direct register writes matching PreenFM3.

```
PLL3: M=1, N=46, P=3
Input = HSE 8 MHz / 1 = 8 MHz
VCO = 8 MHz × 46 = 368 MHz
PLL3_P = 368 / 3 = 122,666,667 Hz → SAI kernel clock
```

### Resulting sample rate

```
MCKDIV = 5
MCLK = 122,666,667 / (2 × 5) = 12,266,667 Hz
FS = MCLK / 256 = 47,917 Hz (actual sample rate)
SCK = FS × 64 = 3,066,688 Hz
```

This matches PreenFM3's actual rate of ~47,916 Hz.

### PLL3 Register Sequence

1. Enable SAI1 peripheral clock: `RCC.APB2ENR` set SAI1EN
2. Disable PLL3: `RCC.CR` clear PLL3ON
3. Wait for PLL3RDY = 0
4. Read-modify-write `RCC.PLLCKSELR`: set DIVM3 = 1 (preserve DIVM1/DIVM2 for PLL1/PLL2)
5. Set `RCC.PLL3DIVR`: DIVN3 = 45 (N-1), DIVP3 = 2 (P-1)
6. Set PLL3 VCO range in `RCC.PLLCFGR`: PLL3VCOSEL = 0 (wide, 1-16 MHz input), set DIVP3EN
7. Enable PLL3: `RCC.CR` set PLL3ON
8. Wait for PLL3RDY = 1
9. Set SAI1 clock source to PLL3_P: read-modify-write `RCC.D2CCIP1R`, SAI1SEL = 0b01

**Important:** Steps 4 and 9 are read-modify-write operations. These registers are shared with PLL1/PLL2 config that the HAL already set up. Writing the full register would clobber existing clock settings.

## SAI1_A Configuration

Direct register writes to SAI1 Block A (`SAI1.CHA`) control registers, matching PreenFM3:

| Register | Field | Value | Meaning |
|---|---|---|---|
| CR1 | MODE | 0b00 | Master TX |
| CR1 | PRTCFG | 0b00 | Free I2S protocol |
| CR1 | DS | 0b110 | 32-bit data size |
| CR1 | MCKDIV | 5 | MCLK = ker_ck / 10 |
| CR1 | MCKEN | 1 | MCLK output enabled |
| CR2 | FTH | 0b001 | FIFO threshold 1/4 |
| CR2 | FFLUSH | 1 | Flush FIFO |
| FRCR | FRL | 63 | Frame length = 64 bits |
| FRCR | FSALL | 31 | FS active for 32 bits |
| FRCR | FSDEF | 1 | FS is channel identification |
| FRCR | FSPOL | 0 | FS active low |
| FRCR | FSOFF | 1 | FS asserted one bit before first data |
| SLOTR | NBSLOT | 0b01 | 2 slots (value is N-1) |
| SLOTR | SLOTEN | 0b0011 | Slots 0 and 1 active |
| SLOTR | SLOTSZ | 0b10 | 32-bit slot size |

After configuration:
1. Enable SAI: CR1.SAIEN = 1
2. Wait until SAI is active (no specific ready flag — just start writing)

### CS4344 I2S Format Note

The CS4344 supports both I2S standard and left-justified formats. PreenFM3 uses I2S standard (PRTCFG = 0b00), with data clocked one SCK cycle after the FS edge. If the output sounds like noise, try PRTCFG = 0b01 (left-justified) as the PCB wiring may differ. Wrong format produces noise, not silence.

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
    let sample = libm::sinf(phase * 2.0 * core::f32::consts::PI);
    let i32_sample = (sample * 0.5 * (i32::MAX as f32)) as i32;
    write_sai_data(i32_sample); // left
    write_sai_data(i32_sample); // right (mono)
    phase += 440.0 / 47917.0;
    if phase >= 1.0 { phase -= 1.0; }
}
```

FIFO room is checked via SAI1 Block A status register `SAI1.CHA.SR`, field FLVL. FLVL < 5 means FIFO is not full (room for at least one stereo pair).

Data is written to `SAI1.CHA.DR` (SAI1 Block A data register). Each write pushes one 32-bit word into the FIFO. Two writes per sample (left then right).

The amplitude is scaled to 50% (`* 0.5`) to avoid clipping the DAC.

## What Lives Where

| File | Responsibility |
|---|---|
| `chimera-stm32/src/audio.rs` | `init_pll3()`, `init_sai1a()`, `sai_fifo_has_room() -> bool`, `write_sai_data(i32)` |
| `chimera-stm32/src/main.rs` | Configure PE2/4/5/6 as AF6, call audio init, add sine poll to main loop |

## What This Does NOT Do

- No DMA — FIFO polled from main loop (will glitch during display updates)
- No SAI1_B or SAI2_A — just one DAC
- No chimera-core DSP — hardcoded sine
- No interrupt-driven audio — main loop only
- No MIDI — fixed 440 Hz
- No MPU configuration for DMA coherence (not needed without DMA)

## Success Criteria

- Phone tuner app shows 440 Hz
- Clean sine between display updates (glitches during render/flush are expected)
- Doesn't hang or crash (display + controls still work)
- LED heartbeat continues running

## Future Steps (not in this spec)

1. Add DMA circular buffers in RAM_D2 for interrupt-driven audio
2. Wire up chimera-core Voice engine in DMA half-transfer callback
3. Add SAI1_B + SAI2_A slaves for 6-channel output
4. MIDI input via USART1
