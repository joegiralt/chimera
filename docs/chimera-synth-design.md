# Chimera: Multi-Engine Digital Synthesizer

**Date:** 2026-04-18
**Status:** Draft
**Repo:** github.com/joegiralt/chimera (to be created)

---

## Overview

Chimera is a custom firmware for the PreenFM3 hardware platform, replacing the stock 6-operator FM synthesizer with a multi-engine, multitimbral digital synthesizer. Three swappable synthesis engines feed into a shared analog-modeled signal chain. The UI uses a chain-based spatial navigation model inspired by LSDJ, with contextual animated visualizations and a persistent dungeon-map position indicator.

**Tagline:** Three engines. One beast.

---

## Hardware Platform

**Target:** PreenFM3 (custom firmware, stock bootloader preserved)

### MCU
- STM32H750 Cortex-M7 @ 480 MHz (HSE, PLL: PLLN=120, PLLP=2)
- Hardware FPU, DSP instructions
- 128KB internal flash (firmware at 0x08020000, bootloader at 0x08000000)

### Memory Regions
| Address | Size | Region | Usage |
|---|---|---|---|
| 0x00000000 | 64 KB | ITCM | Fast instruction memory |
| 0x20000000 | 128 KB | DTCM | Zero-wait-state data: sine tables, audio stack, DSP lookup tables |
| 0x24000000 | 512 KB | D1 AXI-SRAM | Main working memory, framebuffers (~300KB), UI state, patch data |
| 0x30000000 | 128 KB | D2 SRAM1 | Audio DMA buffers (~6KB), MIDI buffers, remaining free |
| 0x30020000 | 128 KB | D2 SRAM2 | Voice/DSP working memory |
| 0x30040000 | 32 KB | D2 SRAM3 | Spare |
| 0x38000000 | 64 KB | D3 SRAM | Low-power accessible |

Note: D2 SRAM cacheability/bufferability is configured by MPU, not fixed by address. Audio DMA buffers (3 SAI streams x 256 samples x 2 channels x 4 bytes = ~6 KB total) are a small fraction of D2.

### Audio — SAI (Serial Audio Interface)
- SAI1 Block A: PE2(MCLK) PE4(FS) PE5(SCK) PE6(SD) — DAC pair 1
- SAI1 Block B: PE3(SD) — DAC pair 2
- SAI2 Block A: PD11(SD) — DAC pair 3
- 48 kHz, 32-bit, circular DMA (DMA1 streams 0-2)
- 3x CS4344 DACs = 6 mono outputs (3 stereo pairs)
- DMA buffer: 256 samples per half-transfer = ~5.3 ms latency
- Render block size: 128 samples (half-buffer, processed in DMA half-transfer ISR)

### Display
- ILI9341 240x320 color TFT, SPI1
- SPI1: PA5(SCK) PA6(MISO) PA7(MOSI), DMA2_Stream0
- Backlight: TIM1 CH2 (PE11), PWM
- DMA2D hardware accelerator available

### Storage
- SD card on TFT module, SPI2
- SPI2: PA9(SCK) PB14(MISO) PB15(MOSI), DMA2 streams 1-2

### MIDI
- USART1: PB6(TX) PB7(RX) @ 31250 baud
- USB MIDI (USB device mode)

### Controls
- 6 rotary encoders (a-f, 3x2 grid, no push) — continuous parameter adjustment
- 6 parameter buttons (1-6, 3x2 grid) — chain head jump
- 6 navigation buttons: MENU, -, +, MIX, EDIT, SEQ
- 1 main rotary encoder (no push)
- GPIO polled at 500 Hz

### Firmware Loading
- Stock bootloader reads firmware from SD card or USB DFU
- Normal update: copy .bin to SD card, hold Menu on boot, select, flash
- Recovery: bridge Boot0 to Vcc, USB DFU via `dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000`
- Revert to stock: flash original PreenFM3 .bin via same process

---

## Voice Architecture

### Signal Chain
```
[ENGINE] -> [DRIVE] -> [FILTER] -> [WAVEFOLDER] -> [VCA] -> [PER-VOICE EFX]
```

### Synthesis Engines (swappable per part)

**4-op FM (TX81Z style)**
- 4 operators, 8 algorithms
- Selectable waveforms per operator (sine, saw, square, etc.)
- Per-operator: ratio, feedback, detune, ADSR envelope
- Digitone-style ergonomics

**Physical Modeling (MI Elements/Rings style)**
- Karplus-Strong strings
- Modal resonators
- Waveguide tubes
- Exciter + resonator model

**VA Polymod (Prophet-5 style)**
- Classic oscillators: saw, square, triangle
- Oscillator sync, PWM
- Cross-modulation (polymod routing)

### Shared Signal Chain

**Drive**
- Pre-filter saturation
- Asymmetric soft clipping

**Filter (Cascadia/Polaris style)**
- 8 selectable modes: LP1, LP2, LP4, BP2, BP4, HP4, NT2, Phazor
- Self-oscillating resonance
- Filter FM from engine output
- Envelope amount, key tracking
- Input drive

**Wavefolder (post-filter)**
- Fold amount
- Symmetry / bias
- Dry/wet mix

**VCA**
- Amp envelope (ADSR)
- Velocity sensitivity

**Per-voice Effects**
- Delay, reverb, chorus sends (V1 scope TBD)

### Voice Allocation
- 4-6 voices polyphonic
- Multitimbral: 2-4 parts
- Dynamic allocation — cheaper engines get more voices
- Per-part MIDI channel routing
- 3 DAC pairs available for per-part stereo outputs

### CPU Budget (estimated cycles per sample at 480 MHz, 48 kHz = 10,000 cycles/sample)

| Stage | Est. cycles/sample |
|---|---|
| 4-op FM engine | ~200 |
| Physical modeling (modal, 8 modes) | ~800 |
| VA Polymod (2 osc + sync + PWM) | ~300 |
| Drive (soft clip) | ~20 |
| Filter (SVF, 1 mode) | ~80 |
| Filter FM (per-sample coeff update) | ~60 |
| Wavefolder | ~40 |
| VCA + amp envelope | ~50 |
| 3 envelopes total | ~90 |
| 2 LFOs | ~40 |
| Mod matrix (6 slots) | ~30 |
| **Total per voice (FM)** | **~610** |
| **Total per voice (Modal)** | **~1,210** |
| **Total per voice (VA)** | **~710** |

Budget for 6 FM voices: ~3,660 cycles (36% of budget). Comfortable.
Budget for 4 Modal voices: ~4,840 cycles (48% of budget). Tight but feasible.
Remaining budget: UI rendering, MIDI processing, effects, overhead.

Note: filter FM (audio-rate cutoff modulation) requires per-sample coefficient recalculation. If this proves too expensive, fallback to per-block smoothing with interpolation.

### Modulation
- 2 LFOs (rate, shape, depth)
- 3 envelopes: amp, filter, aux
- Mod matrix: 4-6 assignable slots (source -> destination -> amount)

---

## Navigation Model — Chain Architecture

Inspired by LSDJ. No menus, no trees. Pure 2D spatial navigation.

### Controls
| Control | Function |
|---|---|
| - | Left (previous node in chain) |
| + | Right (next node in chain) |
| SEQ | Up (previous sub-page) |
| EDIT | Down (next sub-page) |
| MIX | Shift modifier (hold) |
| MENU | System chain |
| Buttons 1-6 | Jump to chain head |
| Main encoder | Patch select / fine-tune |

### Chain Definitions

**Button 1 — VOICE chain (matches DSP signal path):**
```
[ENGINE] -> [DRIVE] -> [FILTER] -> [FOLDER] -> [VCA] -> [EFX]
    |
    v (up/down)
  [FM]
  [Modal]
  [VA]
```

**Button 2 — MIX chain:**
```
[MIXER] -> [ROUTING] -> [DRIVE] -> [COMP] -> [GLOBAL EFX]
    |
    v (up/down)
  [Ch 1]
  [Ch 2]
  [Ch 3]
  [Ch 4]
```

**Button 3 — ENVELOPE chain:**
```
[AMP] -> [FILTER] -> [AUX]
```

**Buttons 4-6 — TBD** (reserved for future chains: modulation, performance, patch management, etc.)

### Navigation Behavior
- Left/right (-/+) moves through chain nodes horizontally (follows signal path)
- Up/down (SEQ/EDIT) moves through sub-pages vertically at current node
- Pressing a chain button jumps to that chain's head
- Pressing the same button again snaps home from anywhere in the chain
- Every chain node maps 6 encoders to its parameters
- The chain model is extensible: new feature = new node or new chain

### Encoder Mapping Per Node (6 encoders: a-f in 3x2 grid)

**VOICE chain — ENGINE node (FM sub-page):**
```
a: Algorithm   b: Ratio      c: Waveform
d: Feedback    e: Depth      f: Detune
```

**VOICE chain — FILTER node:**
```
a: Cutoff      b: Resonance  c: Drive
d: FM Amount   e: Env Amount f: Key Track
```

**VOICE chain — FOLDER node:**
```
a: Fold Amount b: Symmetry   c: Dry/Wet
d: (reserved)  e: (reserved) f: (reserved)
```

**ENVELOPE chain — AMP node:**
```
a: Attack      b: Decay      c: Sustain
d: Release     e: Level      f: Vel Sens
```

**MIX chain — MIXER node:**
```
a: Volume      b: Pan        c: Voices
d: MIDI Ch     e: Pitch      f: Glide
```

---

## Screen Layout & Visual Design

### Display: 240x320 color TFT (ILI9341)

### Layout: Two Zones
```
+----------------------------------+
|                                  |
|        ENCODER ZONE              |
|        (top 2/3 ~213px)          |
|                                  |
|   Contextual visualization +     |
|   parameter values/labels        |
|                                  |
|                                  |
+----------------------------------+
|   DUNGEON MAP (~107px)           |
|                                  |
|   2D chain position indicator    |
|   with vertical branch display   |
+----------------------------------+
```

### Encoder Zone — Contextual Visualizations
Each page renders a visualization specific to its function:
- **Filter:** live frequency response curve, reshapes as you turn cutoff/resonance
- **FM Engine:** algorithm routing diagram (operator boxes + connections)
- **Envelope:** animated ADSR curve, slopes tilt in real-time
- **VA Engine:** waveform display (saw/square/tri with PWM/sync visualization)
- **Mixer:** level bars per channel
- **Wavefolder:** folded waveform shape

Parameter values and labels flank the visualization.

### Dungeon Map — Persistent Position Indicator
Always visible at bottom of screen. Shows:
- Full chain as horizontal node sequence
- Current node highlighted (filled/bright)
- Inactive nodes dimmed but visible
- Vertical branches appear when sub-pages exist at current node:
```
  [ENG]--[VCA]--[FLT]--[FLD]--[EFX]
    |-- * FM
    |-- Modal
    '-- VA
```
- Smooth scrolling if chain wider than screen

### Rendering
- Custom framebuffer rendering — no UI framework
- embedded-graphics primitives + hand-rolled animation/drawing layer
- Double-buffered, 30fps, decoupled from audio thread
- All parameter changes lerped — never snap, always glide
- Consistent easing curves across every screen
- DMA2D hardware accelerator for fills/blits
- Dithering for gradient/depth effects

---

## Software Architecture

### Split Architecture: Portable Core + Platform HAL

```
chimera/
  chimera-core/          (no_std, runs anywhere)
    src/
      dsp/
        engine_fm.rs
        engine_modal.rs
        engine_va.rs
        filter.rs        (8-mode SVF)
        wavefolder.rs
        drive.rs
        envelope.rs
        lfo.rs
        voice.rs         (engine -> chain -> output)
      ui/
        chain.rs         (navigation state machine)
        page.rs          (encoder mapping trait)
        dungeon_map.rs
        renderer.rs      (compositor)
        animation.rs     (lerp, easing)
        pages/
          engine_fm.rs
          engine_modal.rs
          engine_va.rs
          filter.rs
          envelope.rs
          mixer.rs
          ...
      params.rs          (parameter model)
      mod_matrix.rs
      patch.rs           (save/load format)

  chimera-hal/           (platform trait definitions)
    src/
      lib.rs             (AudioOut, Display, Controls, Midi traits)

  chimera-stm32/         (hardware target)
    src/
      audio.rs           (SAI circular DMA)
      display.rs         (ILI9341 SPI + DMA2D)
      controls.rs        (GPIO encoders + buttons)
      midi.rs            (USART1 + USB MIDI)
      main.rs

  chimera-desktop/       (simulator target)
    src/
      audio.rs           (cpal)
      display.rs         (minifb, 240x320 window)
      controls.rs        (keyboard mapping)
      midi.rs            (midir)
      main.rs
```

### Key Traits
```rust
/// Block-based rendering. BLOCK_SIZE = 128 samples (matches DMA half-buffer).
/// Output is mono f32; stereo panning happens at the mixer stage.
trait SynthEngine {
    fn render(&mut self, output: &mut [f32; BLOCK_SIZE], params: &EngineParams, sample_rate: u32);
    fn note_on(&mut self, note: u8, vel: u8);
    fn note_off(&mut self);
}

/// Display abstraction. Implementors own the framebuffer.
/// Desktop: minifb window. Hardware: ILI9341 via SPI+DMA.
/// Uses embedded-graphics DrawTarget for geometry, raw pixel access for blitting.
trait Display: embedded_graphics::draw_target::DrawTarget<Color = Rgb565> {
    fn flush(&mut self);
    fn width(&self) -> u16;  // 240
    fn height(&self) -> u16; // 320
}

trait Controls {
    fn encoder_delta(&self, id: EncoderId) -> i8;
    fn button_state(&self, id: ButtonId) -> ButtonState;
}

trait AudioOut {
    fn sample_rate(&self) -> u32;
}

trait MidiIn {
    fn read(&mut self) -> Option<MidiMessage>;
}
```

### Audio/UI Separation
- Audio callback at 48 kHz (128-sample blocks via DMA half-transfer ISR) — never blocks, never allocates
- UI loop at 30 fps — reads controls, updates chain state machine, renders framebuffer
- Audio thread has absolute priority; UI is best-effort

### Parameter Sharing (Audio <-> UI)
Double-buffered parameter snapshots, not per-parameter atomics. The UI writes to an inactive parameter buffer, then atomically swaps a pointer. The audio thread reads from the active buffer. This avoids torn state when multiple related parameters change together (e.g., algorithm switch changes all operator routings). Single `AtomicPtr` swap per frame.

```rust
struct ParamSnapshot {
    engine: EngineParams,
    filter: FilterParams,
    fold: FolderParams,
    drive: DriveParams,
    envelopes: [EnvParams; 3],
    lfos: [LfoParams; 2],
    mod_matrix: [ModSlot; 6],
}
// UI writes to &mut inactive, then swaps
// Audio reads from &active (immutable borrow, no contention)
```

### Development Workflow
- Daily development: `cargo run -p chimera-desktop` — window pops up, keyboard simulates buttons, audio out via soundcard
- Hardware verification: `cargo build --release -p chimera-stm32` then flash via SD card or `dfu-util`
- Revert to stock PreenFM3: flash original .bin anytime

---

## V1 Phases

### Phase 0 — Hardware Bringup
- Rust `no_std` project skeleton, custom linker script for H750 memory layout
- Replicate clock config (PLL, SAI, SPI, USART clocks)
- GPIO init for all buttons/encoders (map pins from preenfm3LibInitGpio)
- ILI9341 display driver — get pixels on screen via SPI1 + DMA
- SAI circular DMA — get a sine wave out of the CS4344
- MIDI UART rx at 31250 baud
- **Exit criteria:** sine wave plays, screen shows something, MIDI note-on triggers it

### Phase 1 — Desktop Simulator
- Portable core crate (no_std compatible)
- HAL traits: AudioOut, Display, Controls, Midi
- Desktop backend: minifb window (240x320), cpal audio, keyboard -> buttons
- **Exit criteria:** same code runs on desktop and hardware

### Phase 2 — UI Framework
- Framebuffer renderer (embedded-graphics primitives + custom drawing)
- Animation layer (lerp, easing, 30fps render loop)
- Chain navigation state machine
- Dungeon map renderer with vertical branch display
- Page trait — each page declares its 6 encoder mappings + visualization
- **Exit criteria:** navigate chains with buttons, see dungeon map, encoders twiddle placeholder values

### Phase 3 — First Engine (4-op FM)
- TX81Z-style: 4 operators, 8 algorithms, waveform selection per op
- Operator envelopes (ADSR per op)
- Per-operator ratio, feedback, detune
- Engine page with algorithm visualization
- **Exit criteria:** playable FM synth with MIDI, one voice

### Phase 4 — Shared Signal Chain
- Drive (pre-filter soft clipping)
- Filter (8-mode SVF: LP1/LP2/LP4/BP2/BP4/HP4/NT2/Phazor)
- Filter FM from engine output, resonance, key tracking
- Wavefolder (post-filter, fold amount, symmetry, dry/wet)
- VCA + amp envelope
- Filter page with live frequency response visualization
- Envelope page with animated ADSR curve
- **Exit criteria:** full voice chain working, filter sounds good, animations smooth

### Phase 5 — Polyphony & Multitimbral
- Voice allocator (4-6 voices, dynamic based on engine cost)
- Part management (2-4 parts)
- MIDI channel routing per part
- Mix chain — levels, pan per part
- Output routing across 3 DAC pairs
- **Exit criteria:** play chords, multiple parts on different MIDI channels

### Phase 6 — Modulation
- 2 LFOs (rate, shape, depth)
- Mod matrix (4-6 slots, source -> destination -> amount)
- Mod page with visualization
- **Exit criteria:** LFO modulating filter cutoff, matrix assignable

### Phase 7 — Remaining Engines
- Physical modeling (Karplus-Strong, modal resonator, waveguide)
- VA Polymod (saw/square/tri, sync, PWM, cross-mod)
- Engine-specific pages and visualizations
- **Exit criteria:** all 3 engines playable, swappable per part

### Phase 8 — Patch Management & Effects
- Save/load patches to SD card
- Per-voice effects (delay, reverb, chorus sends)
- Global effects bus
- MENU chain (system settings, patch browser)
- **Exit criteria:** save a patch, reload it, effects sound decent

### Phase 9 — Polish
- Animation tuning — consistent easing across all pages
- Pixel-level visual refinement
- USB MIDI support
- Fill chains 4-6 based on user needs
- CPU budget verification per voice
- **Exit criteria:** feels like a finished instrument

---

## Implementation Language

**Rust** (`no_std`, embedded)

Key crates:
- `cortex-m-rt` — runtime, interrupt vectors
- `stm32h7xx-hal` — H750 peripheral access
- `embedded-graphics` — drawing primitives
- `cpal` — desktop audio (simulator)
- `minifb` — desktop window (simulator)
- `midir` — desktop MIDI (simulator)
- `heapless` — fixed-size collections for no_std
- `atomic-polyfill` or `core::sync::atomic` — lock-free parameter sharing

No `unsafe` without explicit `// SAFETY:` comment.
No heap allocation in the audio callback.
No libc.

---

## Specified Defaults

- **SD card filesystem:** FAT32 (via `embedded-sdmmc` crate)
- **Patch format:** Binary, versioned header, forward-compatible
- **Filter mode "Phazor":** Allpass cascade producing phaser effect (per Cascadia/Polaris naming)
- **Framebuffer strategy:** Double-buffered RGB565 in D1 AXI-SRAM (~300 KB). Use DMA2D for fills/blits to reduce CPU cost. If memory proves tight, fall back to single-buffered with dirty-rectangle partial updates.
- **Encoder behavior:** Clamped at min/max (no wrap). Acceleration curve for fast turns. ~128 ticks full range by default, configurable per parameter. Main encoder provides fine-tune (1:1 resolution, no acceleration).

---

## Open Questions (post-V1)
- Chains 4-6: modulation chain? performance chain? arpeggiator?
- MIX+button shift combos: what do they access?
- Main encoder role: patch browsing? value fine-tune? context-dependent?
- Additional engines: wavetable, additive, noise/texture, granular
- Microtuning support
- MPE support
- Preset sharing format
