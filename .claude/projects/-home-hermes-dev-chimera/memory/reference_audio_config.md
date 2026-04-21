---
name: Working SAI audio configuration
description: Proven register values for SAI1_A I2S output to CS4344 DAC on PreenFM3 hardware — 16-bit not 32-bit
type: reference
---

## Working SAI1_A Configuration (verified on hardware)

**Critical finding:** CS4344 on PreenFM3 PCB needs 16-bit I2S, NOT 32-bit. The PreenFM3 C firmware uses HAL with 32-bit protocol config, but the HAL likely handles the format translation internally.

### PLL3 (audio clock)
- HSE = 8 MHz, DIVM3=1, DIVN3=45(N-1), DIVP3=2(P-1)
- VCO = 8 * 46 = 368 MHz, PLL3_P = 368/3 = 122.67 MHz
- SAI1SEL = 0b010 (PLL3_P) — NOT 0b001 which is PLL2_P
- PLLCKSELR: read-modify-write (shared with PLL1/PLL2)

### SAI1_A registers
- CR1: MODE=0b00 (master TX), PRTCFG=0b00 (free/I2S), DS=0b100 (16-bit), MCKDIV=5, MCKEN=1 (bit 27, raw write)
- CR2: FTH=0b001 (1/4), FFLUSH=1
- FRCR: FRL=31 (32-bit frame), FSALL=15 (16-bit FS), FSDEF=1, FSPOL=0, FSOFF=1
- SLOTR: NBSLOT=1 (2 slots), SLOTEN=0b0011, SLOTSZ=0b01 (16-bit)

### Resulting clocks
- MCLK = 122.67M / (5×2) = 12.267 MHz
- FS = 47,917 Hz (actual sample rate)

### Pins
- PE2=MCLK, PE4=FS, PE5=SCK, PE6=SD_A (all AF6)

### FIFO
- 8 words deep, check FLVL < 4 for has_room
- write_sai_data takes i16, writes to DR as u32
