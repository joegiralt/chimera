# Chimera (Work in progress)

Multi-engine digital synthesizer firmware for PreenFM3.

Two engines, six Parts, one screen-driven chain UI.

## Screens

Rendered by the firmware's own renderer (`chimera-core`) at the display's native 240×320, shown at 2×. Regenerate with `just screens` after any UI change.

| Algorithm | Operator waves | Filter |
|---|---|---|
| ![Algorithm page](docs/screens/algo_alg.png) | ![Wave page](docs/screens/algo_wave.png) | ![Filter page](docs/screens/bigviz_filter.png) |

| Mod matrix | Mixer · Part | Sound browser |
|---|---|---|
| ![Mod matrix page](docs/screens/mod_matrix.png) | ![Mixer part page](docs/screens/mixer_part.png) | ![Sound browser](docs/screens/sound_browser.png) |

## Engines

- **Algo** — one algorithmic six-operator engine, two algorithms morphed (ADR 0022)
- **Modal** — physical modelling: Karplus-Strong strings and a modal resonator bank, Rings/Elements as the reference (ADR 0004)

## Architecture

Custom Rust firmware replacing stock PreenFM3 firmware. Swappable synthesis engines feed a shared signal chain (drive -> filter -> wavefolder -> VCA). LSDJ-inspired chain navigation with dungeon-map position display.

## Building

Stable Rust (edition 2024) for everything; add the `thumbv7em-none-eabihf` target for the firmware and the `llvm-tools` component for `just stack-check`.

```bash
# Desktop simulator
just desktop

# STM32 firmware
just firmware

# Flash to PreenFM3
just flash
```

## Status

Runs on the PreenFM3: audio on all three DAC pairs, display, encoders and buttons, MIDI DIN. Design decisions live in `docs/adr/`; open work in the GitHub issue tracker.
