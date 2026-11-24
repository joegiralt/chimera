# Chimera

Chimera is a multi-engine, multitimbral synthesizer firmware, written in
Rust. Today it runs on the [PreenFM3](https://github.com/Ixox/preenfm3)
(STM32H753). It replaces the stock firmware and runs on the stock
hardware and bootloader.

Six Parts play at once, each on its own MIDI channel. Each Part plays one
engine through a voice chain: drive, a filter, an FX slot and a VCA with a
low-pass gate.

Chimera is in alpha: Sounds, projects and the card layout can still
change.

**Screenshots, the user guide, downloads and the browser editor are in
[chimera-releases](https://github.com/joegiralt/chimera-releases).**
This repository holds the code. Chimera will run on more than one machine,
and each machine's releases have their own version numbers, so the
releases live apart from the code.

## Engines

- **ALGO**: six-operator phase modulation. Two algorithms, ALG A and
  ALG B, with MORPH blending between them.
- **MODAL**: physical modelling. Struck, plucked and bowed resonators
  (STRING, BANK, BOWED, SYMP), with Mutable Instruments' Rings and
  Elements as the reference.
- **VA**: two oscillators combined by a MODE (MIX, SYNC, RING, FM, LOOPFM
  and more), with a SUB, a noise source and STABILITY drift.
- **TAPE**: plays sound sources from the SD card through four machines:
  WALK, STRETCH, SCRUB and WAVE.
- **DRUM**: drum machines (kick, snare, tom, clap, cowbell, cymbal, hats
  and more), one per pad of a KIT keymap.

**ORBIT** is a ring sequencer that plays Chimera by itself: a chord
progression round five rhythm rings.

**The mixer** takes the six Parts, with an EQ per Part, into three shared
effects (chorus with FLANGER and PHASER types, delay with TAPE and GRAIN
types, and reverb) and a master compressor.

## Workspace

| Crate | What it does |
|---|---|
| `chimera-engine` | `no_std` model and DSP: everything the audio runs |
| `chimera-core` | `no_std` UI, console and parameters; re-exports the engine |
| `chimera-hal` | Hardware traits: controls, display, MIDI in, storage |
| `chimera-board` | The unit off the audio path: boot, UI loop, drivers |
| `chimera-stm32` | The firmware: entry point, audio interrupt, MIDI DIN |
| `chimera-image` | Bank 2's image: header, seal and the door into it |
| `chimera-midi` | MIDI vocabulary and its decisions, as pure functions |
| `chimera-fat` | FAT16 and FAT32 over the storage trait |
| `chimera-theory` | Music theory as `const` data |
| `chimera-waves` | ALGO's wave-table recipes |
| `chimera-circuits` | Circuit models kept for reference |
| `chimera-screen` | The editor's glyph renderer (headless UI, WASM) |
| `chimera-desktop` | The desktop simulator and importers |
| `chimera-utils` | Host tools: A/B renders, budget measures, image seals |
| `chimera-bootloader` | A jump stub, never shipped |

`tools/` holds the Python checks and the USB tool (`chimera-usb.py`).
Design decisions are in `docs/adr/` (`docs/adr/README.md` is the index).
The specs and implementation plans behind them are in `docs/superpowers/`.

Issue numbers in the code and the ADRs (`#123`, "private issue #123")
point to the private tracker, `joegiralt/chimera-private`. Public bug
reports go to chimera-releases (see Contributing).

Chimera is written by AI agents directed by one engineer, who owns the
design, the specs and the testing. `CLAUDE.md` holds the rules the agents
follow.

## Building

Chimera needs **nightly Rust** (edition 2024). The engine uses
`#![feature(core_float_math)]` and `#![feature(optimize_attribute)]`, and
the firmware links with `-Z location-detail=none` (`.cargo/config.toml`).
`rust-toolchain.toml` pins the nightly, its components and the firmware
target, so rustup installs them on the first build, here and in CI. The
bank budgets are measured with that compiler. With rustup in place, two
tools remain:

```bash
cargo install cargo-binutils just
```

`chimera-stm32`, `chimera-board` and `chimera-bootloader` build only for
`thumbv7em-none-eabihf`; on the host, `cargo check` covers the rest.

On Linux the desktop simulator needs ALSA's development files
(`libasound2-dev` and `pkg-config`). The FAT tests need `dosfstools`.

| Task | Command |
|---|---|
| Desktop simulator | `just desktop` |
| Firmware (`target/chimera.bin` and `.bk2`) | `just firmware` |
| Firmware with ORBIT | `just firmware-orbit` |
| Tests | `just test` |
| Clippy | `just clippy` |
| The full check | `just check` |
| Flash a unit | `just flash` |

`just check` runs everything CI runs. It takes a while, so CI runs its
parts as separate jobs: `check-tools`, `check-host`, `check-firmware`,
`clippy-main`, `loom` and `cargo fmt` on every push and pull request, and
`check-bench`, `clippy-bench` and `check-release` nightly.

The firmware is split over the chip's two flash banks. Each feature set is
held to a byte budget for each bank (`tools/bank1-budget.txt`,
`tools/bank2-budget.txt`), and `just check-firmware` fails a build that
outgrows its line.

## Flashing

`just flash` builds the firmware and installs it with
`tools/chimera-usb.py update`: bank 2's `.bk2` goes onto the SD card over
USB, then the `.bin` goes to `0x08020000` over DFU. It needs `dfu-util`
and Python 3 (plus `pyserial` on macOS and Windows). The stock PreenFM3
bootloader (1.09) stays in place and is never written. `docs/recovery.md`
covers backing it up and restoring it.

To install a release instead, follow the guide in
[chimera-releases](https://github.com/joegiralt/chimera-releases).

## Licence

Copyright (C) 2026 Joe Giralt.

Chimera is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free
Software Foundation, either version 3 of the License, or (at your option)
any later version. See `LICENSE`. Code from others keeps its own notice;
`THIRD_PARTY.md` lists it.

## Contributing

This repository is read-only for now. It takes no outside pull requests
until a contributor licence agreement is in place, before 1.0.

Bug reports, questions and discussion go to
[chimera-releases' issues](https://github.com/joegiralt/chimera-releases/issues).
`CONTRIBUTING.md` has the details.
