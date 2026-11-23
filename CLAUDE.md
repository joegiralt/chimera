# CLAUDE.md — Chimera Synth Firmware

## What this project is

Chimera is a custom Rust firmware for the PreenFM3 hardware (STM32H753). It replaces the stock FM synthesizer with a multi-engine, multitimbral digital synth with chain-based LSDJ-style navigation.

Decisions: `docs/adr/README.md` (the index; every later decision lives in an ADR). Current specs: `docs/chimera-product-architecture.md`, `docs/chimera-sitemap.md`, `docs/chimera-modulation-spec.md`, and the feature specs in `docs/superpowers/specs/`. `docs/chimera-synth-design.md` is the 2026-04 first draft, superseded in most parts.

## Workspace layout

```
chimera-engine/      — no_std model and DSP, everything the audio runs (opt 2)
chimera-core/        — no_std UI, console and params; re-exports the engine (opt z)
chimera-hal/         — trait definitions (Controls, Display, MidiIn, store)
chimera-board/       — the unit off the audio path: boot, UI loop, drivers (opt z)
chimera-stm32/       — hardware target: main, audio ISR, MIDI DIN, bench (opt 2)
chimera-image/       — bank 2's image: header, seal, the door (ADR 0137)
chimera-midi/        — MIDI vocabulary and pure decisions
chimera-fat/         — FAT16/FAT32 over chimera-hal's store
chimera-theory/      — music theory as const data
chimera-waves/       — ALGO's wave-table recipes (ADR 0023)
chimera-circuits/    — circuit models kept for reference (ADR 0196)
chimera-screen/      — the browser editor's glyphs (headless UiState, WASM)
chimera-desktop/     — simulator (minifb window, cpal audio) and importers
chimera-utils/       — host tools: A/B renders, clips, budget measures
chimera-bootloader/  — Chimera's own jump stub (bootloader.bin), never shipped
(editor)             — the Elm browser editor and site live in joegiralt/chimera-releases (`editor/`)
tools/               — Python checks and the USB tool (chimera-usb.py)
```

## Task runner

| Task | Command |
|---|---|
| Desktop simulator | `just desktop` |
| Build firmware | `just firmware` |
| Check all | `just check` |
| Tests | `just test` |
| Clippy | `just clippy` |
| Flash to hardware | `just flash` |

## Hardware

- STM32H753 @ 480 MHz, 2 MB flash in two 1 MB banks; firmware at 0x08020000 (bank 1, 917,504 B); bank 2 per #460
- ILI9341 240x320 TFT via SPI1
- 3x CS4344 DAC via SAI (48 kHz, 32-bit)
- HC165 shift registers for encoders/buttons (3 GPIO pins: DATA, LOAD, CLK)
- USART1 MIDI @ 31250 baud + USB MIDI
- SD card via SPI2

## Rules

- No `unsafe` without `// SAFETY:` comment
- No heap allocation in audio callback
- No libc
- Audio thread never blocks, never allocates
- All parameter changes lerped in UI — never snap
- Type-driven first: if the type system can solve it, it must. Encode invariants in types — newtypes for units and ranges (`Hz`, `MidiNote`), enums over magic numbers, illegal states unrepresentable, `const` validation of statics — so the compiler rejects misuse. Use a runtime check or test only for what a type can't express; reviews flag any check or test a type could replace.
- Bugs, known issues and follow-ups go in the private tracker (joegiralt/chimera-private), not in repo files (public bug reports come in at joegiralt/chimera-releases' issues); reference them by URL.
- Decisions that constrain future work (architecture, algorithm choice and provenance, UX behavior, licensing) get an ADR in `docs/adr/` (template `0000-template.md`, add it to `docs/adr/README.md`). Check existing ADRs before re-deciding something; never edit an accepted ADR — supersede it with a new one.
