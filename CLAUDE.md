# CLAUDE.md — Chimera Synth Firmware

## What this project is

Chimera is a custom Rust firmware for the PreenFM3 hardware (STM32H750). It replaces the stock FM synthesizer with a multi-engine, multitimbral digital synth with chain-based LSDJ-style navigation.

Full design spec: `docs/chimera-synth-design.md`

## Workspace layout

```
chimera-core/     — no_std portable logic (DSP, UI, params)
chimera-hal/      — trait definitions (Controls, Display, MidiIn)
chimera-stm32/    — hardware target (SAI, SPI, GPIO, DMA)
chimera-desktop/  — simulator (minifb window, cpal audio)
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

- STM32H750 @ 480 MHz, 128KB flash, firmware at 0x08020000
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
- Decisions that constrain future work (architecture, algorithm choice and provenance, UX behavior, licensing) get an ADR in `docs/adr/` (template `0000-template.md`, add it to `docs/adr/README.md`). Check existing ADRs before re-deciding something; never edit an accepted ADR — supersede it with a new one.
