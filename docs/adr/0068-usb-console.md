# 0068. Read the unit over a polled USB CDC-ACM console

- **Status:** Proposed (accepted at the ship flash, the plan's last task)
- **Deciders:** project owner (2026-10-02, the design in conversation, and
  the answers to the spec's five open questions the same day); firmware

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
  `write` until done, or give up after 250 ms with no progress. The loop
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
  `help`, `status`, `stats`, `bench` and `shot`, all read-only.
- **Functional core, thin shells:** `chimera_core::console` turns bytes
  into answers through `Unit` and `Out` traits. `chimera-stm32/src/usb.rs`
  (feature `usb-console`, on by default) and the desktop's socket on
  `127.0.0.1:7341` are the shells.
- **Host side:** `tools/chimera-usb.py` (Python 3, standard library only,
  no dependencies; the owner, 2026-10-02),
  run by `just usb <cmd>`, `just shot`, `just stats` and `just status`. A
  udev rule keeps ModemManager off the port.
- **Identity:** the pid.codes test ID `1209:0001` for now (the owner,
  2026-10-02); a PID of Chimera's own is
  https://github.com/joegiralt/chimera/issues/299. Never ST's `0483:DF11`,
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

## Consequences
- About 20 KB of flash (about 2.2 points of 896 KB) and about 2.5 KB of
  AXI SRAM. A bench build adds a 6 KB report buffer.
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

## Sources
- `docs/superpowers/specs/2026-10-02-usb-console-design.md` (§ The owner's
  answers); `docs/superpowers/plans/2026-10-02-usb-console.md`.
- Stock PreenFM3 firmware (github.com/Ixox/preenfm3, `master`, GPL-3.0):
  `firmware/Src/usbd_conf.c`, `firmware/Src/main.c`,
  `firmware/preenfm3.ioc`, `bootloader/Src/usbd_storage_if.c`. Read only,
  no code taken.
- `stm32h7xx-hal` 0.16.0 `src/usb_hs.rs` and `examples/usb_serial.rs`;
  `synopsys-usb-otg` 0.4.0; `usb-device` 0.3.2; `usbd-serial` 0.2.2.
- ST RM0433 (CRS, OTG, unique device ID); ST AN2606 (STM32H74x/75x
  system-memory DFU on PA11/PA12).
- ADRs 0019 (note sources), 0066 (SETTINGS, USB CONFIG later); issues
  #203, #204, #269, #299.
