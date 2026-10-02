# 0068. Read the unit over a polled USB CDC-ACM console

- **Status:** Proposed (accepted at the ship flash, the plan's last task)
- **Deciders:** project owner (2026-10-02, the design in conversation, and
  the answers to the spec's five open questions the same day, and the ask
  to enter DFU from the firmware); firmware

## Context
Reading the unit's numbers and screen meant photographing the panel: AUDIO
LOAD, the bench rows B1 to B7, where the UI is. The owner wants them over
USB. The PreenFM3's USB connector is OTG_FS on PA11/PA12 (stock firmware
`usbd_conf.c`). PB14/PB15 are the SD card's SPI2 here, so OTG_HS's
internal PHY is out. The 48 MHz USB clock cannot come from a PLL: PLL1
follows the silicon revision, PLL2 is pinned for the SPIs, and PLL3 is
fractional for the SAI. The audio interrupt is level 0 and MIDI DIN level
4 (`priority.rs`). The UI loop is the framebuffer's only writer. The spec
is `docs/superpowers/specs/2026-10-02-usb-console-design.md`.

## Decision
- **Transport:** a CDC-ACM serial device on USB2 OTG_FS, PA11/PA12 AF10,
  from `stm32h7xx-hal` 0.16's `usb_hs` (`synopsys-usb-otg` 0.4), with
  `usb-device` 0.3 and `usbd-serial` 0.2. No hand-written driver.
- **Clock:** HSI48 to the USB kernel clock, trimmed by the CRS from USB2's
  SOF (`SYNCSRC = 0b11`).
- **Polled, never interrupt-driven:** `usb_dev.poll` runs at the top of the
  UI loop, and the OTG interrupt stays masked. Answers pump `poll` and
  `write` until done, or give up after 250 ms with no progress or 1000 ms
  for the whole answer, so a host that dribbles can't hold the UI. The loop
  answers at most one request per iteration.
- **Snapshot point:** the top of the UI loop, after the last flush. `shot`
  streams the framebuffer through the THEME palette from there with no copy;
  `shot raw` streams it in the canonical palette, as `docs/screens` shows
  it. The UI holds still while it streams (0.15 to 0.3 s; the owner,
  2026-10-02).
- **Protocol 1:** one ASCII request line, at most 64 bytes, terminated by
  LF or CR. One answer ends in `OK` or `ERR <reason>`. Text bodies are
  `key value` lines. A command takes at most one argument; only `shot`
  takes one (`raw`). `shot` sends `SHOT 240 320 rgb565be 153600`, then the
  body, then `OK`. The unit never speaks first.
- **One table, one enum:** a `commands!` row per command gives the
  `Command` enum, its name, its help line and its argument type, and the
  `Request` enum that carries the parsed argument (`Shot(Colours)`).
  `answer` matches exhaustively, so a new row does not build until it is
  answered. The commands are
  `help`, `status`, `stats`, `bench` and `shot`, all read-only, and `dfu`.
- **Enter DFU from the firmware** (the owner, 2026-10-02: no BOOT0
  jumper to flash). SETTINGS › SYSTEM › OS UPGRADE becomes an action
  row. Its prompt reads `ENTER DFU?` / `PLAY STOPS UNTIL FLASHED OR
  POWER-CYCLED`, with the pills `ENTER DFU` and `CANCEL`, sealed by
  `Said<RomDfu>` through `commits!` and `replace::said`. The console's
  `dfu` answers `OK`, flushes it and keeps polling for 20 ms so the host
  collects it, then does the same; the desktop answers `ERR dfu is not in
  this build`. Only the shell's `service` makes the `DfuAsked` the
  console's way in needs, and only after that drain; a host that still
  misses the `OK` sees the port vanish and DF11 appear, which `to-dfu`
  counts as success. Both paths sync SYSTEM to the card first
  (`sync_system_now`; the menu's through `take_dfu_synced`), so a THEME
  change made in the same visit is kept. Either way the firmware writes `DFU_MAGIC` to
  RTC_BKP0R and calls `SCB::sys_reset()`. The stock bootloader runs and
  jumps to Chimera. At the top of `main`, before `boot()` touches a clock
  or peripheral, Chimera reads the register, always clears it and reads
  the clear back, and asks the pure `boot::after_reset(marker,
  readback)`: only `DFU_MAGIC` that read back 0 jumps, and a stuck marker
  boots the synth (a transient stuck may enter DFU on a later reset,
  which is accepted). `after_reset` returns `dfu::Checked`,
  which `boot` takes and pairs with the owned RTC into the `dfu::Marker`
  that `dfu::enter` needs, so nothing writes the marker before the check
  ran. On the jump, it stops SysTick, disables and unpends every NVIC
  line, sets VTOR and MSP
  from the ROM's vector table at 0x1FF0_9800 (ST AN2606) and branches to
  its reset vector. That jump is this feature's one `unsafe`. The empty
  UPDATES page goes, since ABOUT already shows everything a version page
  would. `just flash` and `just flash-bench` send `dfu` when the console is
  there, wait up to 10 s for `0483:DF11`, then flash. Without the console,
  they print the jumper instruction and flash as before.
- **Functional core, thin shells:** `chimera_core::console` turns bytes
  into answers through `Unit` and `Out` traits. `chimera-stm32/src/usb.rs`
  (feature `usb-console`, on by default) and the desktop's socket on
  `127.0.0.1:7341` are the shells.
- **Host side:** `tools/chimera-usb.py` (Python 3, standard library only,
  no dependencies; the owner, 2026-10-02),
  run by `just usb <cmd>`, `just shot`, `just stats` and `just status`. A
  udev rule keeps ModemManager off the port.
- **Identity:** `0483:5740`, the identity the stock PreenFM3 firmware uses
  (the owner, 2026-10-02; github.com/Ixox/preenfm3
  `firmware/Src/usbd_desc.c`: `USBD_VID 1155`, `USBD_PID_FS 22336`).
  Chimera is software on someone else's hardware. 0x5740 is ST's example
  VCP PID. Strings stay `Chimera` / `Chimera console`. Never ST's `0483:DF11`,
  so `dfu-util` for `just flash` never matches the running synth.
- **When:** the port appears once the UI loop starts, after the splash
  (the owner, 2026-10-02).

## Alternatives considered
- **OTG interrupt at a level below audio:** it can't read the framebuffer
  or `UiState` without a 150 KB copy or a handshake, and it needs queues
  both ways, all for latency a console doesn't need. USB MIDI will need
  it, and that gets its own ADR.
- **OTG_HS with its internal FS PHY on PB14/PB15:** those pins are the SD
  card's.
- **A PLL for 48 MHz:** no PLL is free to give it exactly (Context).
- **A binary or framed protocol (SLIP, COBS, protobuf):** it can't be typed
  in a terminal, and text with one binary body covers every request.
- **A PTY for the desktop simulator:** it needs libc or `nix`. A socket is
  in `std`.
- **A Rust host tool:** it adds a serial-port crate and a PNG crate for
  about 120 lines of glue, with no constraint for types to guard.
- **A shot that keeps the UI moving** (a 150 KB copy streamed a slice per
  frame) or **an RLE-compressed shot**: the owner chose the 0.15 to 0.3 s
  hold instead.
- **The shared V-USB CDC-ACM ID `16C0:27DD`:** not ours alone; the test ID
  serves until Chimera has its own.
- **A port from the start of boot:** it would need polling from the boot
  steps, for hangs the splash already shows.
- **DFU through Chimera's own USB DFU class, or by writing BOOT0's option
  bytes:** the first is a flash writer in the firmware and the second
  can leave the unit booting the ROM for good. The ROM loader already
  does the job, and neither would let it be reached more safely.
- **The DFU check in `#[pre_init]`:** cortex-m-rt 0.7.5 calls a Rust
  `pre_init` unsound. The top of `main` runs before any of Chimera's
  clock or peripheral setup, which is all the ROM needs.
- **The HAL's `rtc::Rtc` for the marker:** `init` resets the backup
  registers, and the RTC itself is unused. The PAC's `bkpr[0]` is enough.

## Consequences
- 26,040 B of flash measured (about 2.8 points of 896 KB; budget
  raised to 28 KB by the owner, 2026-10-02) and about 1.5 KB of AXI SRAM. A bench build adds a 6 KB report buffer.
- A `shot` freezes the UI for 0.15 to 0.3 s. Audio, MIDI and key latching
  go on.
- The port appears once the UI loop starts, after the boot splash, so
  boot hangs are not readable over it.
- B4 to B7 come from `stats` (overruns, UI loop time) and B1 to B3 from
  `bench`, with no camera.
- USB MIDI (#203) and USB audio (#204) make the device composite and move
  USB to an interrupt, which supersedes the "polled" part of this ADR. The
  console core is unchanged by that.
- Write commands extend the same grammar with typed arguments.
- Flashing needs no jumper while a working Chimera with the console is
  on the unit: `just flash` reaches the ROM loader through `dfu`. The
  jumper stays the way in for a unit that doesn't boot.
- **The unit cannot be bricked by DFU entry.** The ROM loader is in
  system memory and cannot be written. Chimera never writes flash, option
  bytes or BOOT0. The marker is cleared before the jump, so the next reset
  or power-cycle always plays. And the BOOT0 jumper reaches the ROM loader
  whatever is in flash.
- It rests on two facts checked only on the unit (the plan's U8 to U10):
  the stock bootloader leaves RTC_BKP0R alone, and the ROM loader runs
  from the state that bootloader leaves. If the marker is clobbered, an
  issue is filed. A possible future alternative is a word of D3 SRAM4
  (0x3800_FFFC), which neither image maps; it is not built. It carries two
  caveats: SRAM is ECC-protected, so reading a word never written since
  power-on can raise an ECC error on a cold boot, and the marker write
  must reach SRAM past the D-cache before the reset.
- OS UPGRADE no longer opens a page. Block id 34 (`SYS_UPDATES`) is
  retired, not reused.
- Well under 1 KB of flash, and no RAM.

## Sources
- `docs/superpowers/specs/2026-10-02-usb-console-design.md` (§ The owner's
  answers); `docs/superpowers/plans/2026-10-02-usb-console.md`.
- Stock PreenFM3 firmware (github.com/Ixox/preenfm3, `master`, GPL-3.0):
  `firmware/Src/usbd_conf.c`, `firmware/Src/main.c`,
  `firmware/preenfm3.ioc`, `bootloader/Src/usbd_storage_if.c`,
  `bootloader/Src/main.c`, `bootloader/STM32H753VITX_FLASH.ld`. Read only,
  no code taken.
- `cortex-m` 0.7.7 `asm::bootload`, `SCB::sys_reset`; `cortex-m-rt` 0.7.5
  (`pre_init`); `stm32h7xx-hal` 0.16.0 `rtc.rs`, `pwr.rs` (DBP).
- `stm32h7xx-hal` 0.16.0 `src/usb_hs.rs` and `examples/usb_serial.rs`;
  `synopsys-usb-otg` 0.4.0; `usb-device` 0.3.2; `usbd-serial` 0.2.2.
- ST RM0433 (CRS, OTG, unique device ID); ST AN2606 (STM32H74x/75x
  system-memory DFU on PA11/PA12).
- ADRs 0019 (note sources), 0066 (SETTINGS, USB CONFIG later); issues
  #203, #204, #269.
