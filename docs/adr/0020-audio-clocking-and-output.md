# 0020. Clock the chip by silicon revision; derive the cycle budget from it

- **Status:** Accepted (2026-09-26)
- **Deciders:** project owner

## Context
The instrument moves onto the PreenFM3 (STM32H750). Two silicon revisions
exist: rev V runs 480 MHz at VOS0 (stm32h7xx-hal's `revision_v` feature),
rev Y tops out at 400 MHz, and their SAI master-clock dividers differ (rev Y
halves MCLK, and only rev B and later have MCKEN). The firmware ran a fixed
400 MHz with MCKDIV = 5, which by the rev V formula plays 95.8 kHz — an
octave high. ADR 0013 made the cycle budget a compile-time constant for
480 MHz, which would over-budget a rev Y chip by 20 %. The SAI data
register is right-aligned, so a 24-bit left-justified sample needs 32-bit
data. The stack shared AXI with no guard, beside a 150 KB framebuffer.

## Decision
- **CPU speed by revision.** DBGMCU REV_ID `0x2003` (V) runs 480 MHz, VOS0;
  `0x1003` (Y) and anything unknown run 400 MHz. HCLK is half the core.
- **Runtime budget.** `SampleBudget::for_cpu(cpu_hz)` (70 % of
  `cpu_hz / 48 kHz`, the allocator's unit) and `BlockBudget::for_cpu`
  (the 64-sample deadline, the probe's) are built from the real clock and
  have no `Default`. This **supersedes ADR 0013's clause** that the cycle
  budget is a shared compile-time constant in `hw.rs`; the rest of 0013
  stands (the desktop still enforces the 480 MHz chip's budget).
- **48 kHz ± 10 ppm** from an 8 MHz HSE through fractional PLL3
  (`clock_plan::pll3_for`: 49.152 MHz SAI kernel, M 1, N 49, FRACN 1245,
  P 8, −0.46 ppm) and MCKDIV by revision: FS = ker / (MCKDIV × 256) from
  rev B (REV_ID ≥ `0x2000`, MCKDIV 4, MCKEN set), MCLK = ker / (2 × MCKDIV)
  on rev Y (MCKDIV 2, no MCKEN).
- **32-bit SAI slots carrying left-justified 24-bit samples** (`DacSample`),
  I2S, MCLK 256 × FS.
- **Three SAI blocks on one clock:** SAI1 A master exporting its sync,
  SAI1 B internal slave, SAI2 A external slave; three circular DMA streams
  started before the slaves, the master last; only stream 0 interrupts.
- **The stack in DTCM** (128 KB, zero-wait, CPU-only), with a painted
  high-water mark on the AUDIO page.

## Alternatives considered
- **480 MHz unconditionally** (stock PreenFM3 does this) — out of spec on a
  rev Y part; the REV_ID read costs nothing.
- **Keep a 480 MHz constant budget** — a rev Y chip would accept voices it
  can't render in time.
- **DS = 24 with right-aligned sign-extended samples** — works, but diverges
  from stock and gains nothing; the CS4344 reads the top 24 bits of a
  32-bit slot either way.
- **Integer PLL3 (stock's 122.67 MHz, MCKDIV 10)** — 47,917 Hz, 3 cents flat.
- **SAI2 as a second master** — breaks the three-stream DMA lockstep.

## Consequences
Pitch is exact on both revisions; a rev Y chip gets fewer heavy voices, by
the same allocator code. `Instrument::new` and `Allocator::new` take the
budget. The display SPI kernel (PLL1 Q) is 192 MHz on rev V (the 960 MHz
VCO has no 200 MHz divisor) and pclk2 is 120 MHz; the SPI and USART
dividers are computed from the real clocks. DTCM holds `main`'s frame
(`UiState`), so a debug firmware is only link-checked.

## Sources
`docs/superpowers/specs/2026-09-26-instrument-on-chip-design.md`;
`docs/superpowers/plans/2026-09-26-instrument-on-chip.md`; stock PreenFM3
firmware (Ixox/preenfm3, `firmware/Src/main.c`: SAI setup and pins, VOS0);
ST STM32H7 HAL (`stm32h7xx_hal_sai.c`: MCKDIV formula, MCKEN from rev B);
stm32h7xx-hal 0.16 (`pwr.rs`, `rcc/pll.rs`); RM0433.
