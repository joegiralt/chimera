# 0326. Keep the display link at 50 MHz and re-init a panel that leaves its mode

- **Status:** Accepted (2026-11-22)
- **Deciders:** owner (keeps 50 MHz), firmware

## Context
On 2026-11-22, with the browser editor driving `ed part` and `ed set`, the
panel froze on a mix of the TAPE and VA pages over a purple-blue ground. It
stayed that way after the editor closed, while `shot` read a clean SETTINGS.
A register-level panel under the real flush stream
(`chimera-core/tests/panel_link_test.rs`) shows the UI and the flush keep the
panel equal to the framebuffer through that session. One flipped bit on one
command byte does not: a CASET (2Ah) read as DISPOFF (28h) or COLMOD (3Ah)
leaves the panel off or in the wrong pixel format, and nothing a flush sends
brings it back. `init` ran once at boot, a flush re-sends only its address
window, SPI errors were dropped, and nothing read the panel back.

SPI1 writes the ILI9341 at 50 MHz (PLL2_P 100 MHz / 2, commit 735e5201d). The
datasheet's serial write cycle is 100 ns at the least (V1.11, § 19.3, serial interface timing), so
10 MHz. Stock PreenFM3 runs the same panel at 32 MHz (PLL2 8 MHz × 40 / 5,
then /2) and ships TFT AUTO REINIT: every 500 ms it reads RDDPM and re-inits
after two bad reads in a row (`lib/Src/TftDisplay.cpp`, `TftDisplay::tic`).
That is a sign panels lose their mode in the field. Filed as #967.

## Decision
- SPI1 stays at 50 MHz for every write. The owner wants the screen as quick as
  it is.
- PA6 is SPI1's MISO (AF5), the panel's SDO, as stock wires it.
- At the top of each UI loop pass (between frames, through the card
  watch's door call), every 500 ms
  (`chimera_hal::panel::CHECK_MS`), the shell reads RDDPM, RDDMADCTL and
  RDDCOLMOD (0Ah, 0Bh, 0Ch): one byte each, the command then the answer, with no dummy bit in between. For the read
  only, SPI1 is slowed to 100 MHz / 16 = 6.25 MHz, inside the 150 ns read
  cycle, and then put back.
- Nothing re-inits until the panel has once answered healthy: RDDPM 0x9C,
  or 0xDE as stock also accepts; MADCTL 0x48; COLMOD 0x55. The answer is
  compared with these known values, never learned, so a noisy first read
  can't become the reference, and a link that can't read never re-inits in
  a storm. Once armed, two reads in a row that aren't healthy re-run
  `init`, push THEME's GAMMA as last set, and flush the whole framebuffer.
  A read that fails (an SPI error, or no answer within `POLLS`) counts as
  not healthy.
- Each re-init doubles the next wait, up to 16 s (`BACKOFF_MAX`). A healthy
  read puts it back to 500 ms. A fault that `init` can't clear holds the UI
  in init a handful of times a minute, not twice a second.
- CFG2.AFCNTR is set once at boot, so SPI1 keeps SCK and MOSI driven while
  it is disabled. The clock and duplex change only with CS high, between
  frames: the read switches SPI1 to /16 and full duplex, sends the command
  and a 0xFF dummy byte, raises CS, then switches back to /2 and
  transmit-only.
- SPI1 runs transmit-only (CFG2 COMM). Its writes go through one function,
  `display::put`, on the registers: TXP before each byte, CSTART once, and
  TXC only before CS rises. Nothing is drained from RX. This replaces the
  HAL's full-duplex write, which waited for every byte to come back on RX.
  A flag that never comes counts as an SPI error.
- `stats` prints `panel_armed`, `panel_reinits` and `spi_errors`. Bank 2
  writes those lines: the board's door hands `stats` a function pointer.
- The protocol (the init table, a flush's window, the check) is
  `chimera_hal::panel`, pure over `Link`, so the host test drives the same
  code the board runs. The check, the read and the `stats` lines are bank
  2's, an S4 group (`.bank2_panel`), reached through the board's door
  (`BoardDoor::panel`, `panel_stats`). `init` and the window stay in bank 1,
  where the boot needs them before the door opens. With the door closed there
  is no check.

## Alternatives considered
- **Slow the link to 25 or 32 MHz.** Fewer bad bytes, but still no recovery
  when one lands, and slower flushes. The owner declined it.
- **Re-send MADCTL, COLMOD, SLPOUT and DISPON before every flush.** This is
  blind. It would cost bytes on every flush, would still miss a SWRESET's
  power settings, and would never show that anything happened.
- **Read RDDPM only, as stock does.** This misses a garbled COLMOD or MADCTL,
  which leave RDDPM at 0x9C. The test's COLMOD case is exactly that.

## Consequences
- A panel knocked out of its mode comes back within about 1 s plus init's
  ~325 ms (the UI loop holds during init; the watchdog is kicked from the
  audio side).
- Each check costs three 2-byte reads at 6.25 MHz every 500 ms.
- With bank 2 closed there is no check, as for every other door.
- The link stays out of spec. The counters show how often that bites.

Measured (CHIMERA_STAMP_CLEAN release builds against 0.4 at c13ae2f07):
bank 1 default −128 B, orbit −56 B, bank2-gate −192 B (the transmit-only
`put` is smaller than the HAL's full-duplex write it replaces); bank 2
+640 B in default, orbit and bank2-gate, all of it in S4, their lines
raised under this ADR.

## Sources
- ILI9341 datasheet V1.11: § 8.2, the command list (RDDPM 0Ah, RDDMADCTL 0Bh, RDDCOLMOD 0Ch), and
  § 19.3 (serial interface timing).
- Stock PreenFM3: `lib/Src/TftDisplay.cpp` (`tic`, `reset`),
  `lib/Src/ili9341.c` (`ILI9341_ReadPowerMode`), `firmware/Src/main.c`
  (SPI1 and PLL2), and `stm32h7xx_hal_msp.c` (PA6 as SPI1_MISO). This is a
  local study clone, GPL-3.0; no code was copied.
- #967, and commit 735e5201d (50 MHz).
