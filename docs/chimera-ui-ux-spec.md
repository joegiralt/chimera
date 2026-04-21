# Chimera UI/UX Specification

## Overview

This document specifies the complete UI system for Chimera. Everything the user sees, touches, and navigates is defined here. The DSP layer implements what this spec requires — not the other way around.

## Hardware Surface

```
┌──────────────────────────────────────┐
│                                      │
│         ILI9341 240×320 TFT          │
│         (portrait, taller than wide) │
│                                      │
├──────────────────────────────────────┤
│                                      │
│  Encoders:  [A] [B] [C]             │
│             [D] [E] [F]  [MAIN]     │
│                                      │
│  Param buttons: [B1][B2][B3]        │
│                 [B4][B5][B6]        │
│                                      │
│  Nav buttons: [MENU] [-] [+]        │
│               [MIX] [EDIT] [SEQ]    │
│                                      │
└──────────────────────────────────────┘
```

- **6 encoders (A-F):** Edit the 6 parameters shown on the current page. Always 6. Every page shows exactly 6 values.
- **6 Part buttons (B1-B6):** Select Part/chain. B1 = Part 1's chain, B2 = Part 2's chain, etc. Up to 6 Parts.
- **Minus/Plus:** Navigate left/right through blocks in the selected chain.
- **Seq/Edit:** Navigate up/down through sub-pages at the current block.
- **MIX (hold):** Shift modifier. MIX + encoder = coarse snap. MIX + B1 = mixer page. MIX + B2-B6 = reserved for future functions.
- **MENU:** System/global settings page.

## Screen Layout

The 240×320 display is divided into three persistent zones:

```
┌────────────────────────────────┐ Y=0
│  HEADER                        │
│  Part name > Block name > Sub  │ 28px
├────────────────────────────────┤ Y=28
│                                │
│                                │
│  CONTENT ZONE                  │
│  (visualization + parameters)  │ 186px
│                                │
│                                │
├────────────────────────────────┤ Y=214
│                                │
│  DUNGEON MAP                   │
│  (navigation position)         │ 106px
│                                │
└────────────────────────────────┘ Y=320
```

### Header (Y: 0-28)

Always visible. Shows current context:

```
Part 1 > Filter > Cutoff                    2.1ms
```

- Left: Part number + current page name. Updates when Part or page changes.
- Right: Performance stats (render time, CPU load). Dim text, always present.

### Content Zone (Y: 28-214)

Two layout modes, determined by the current page:

**BigViz layout:** A single large visualization (Y: 28-144) with a 3×2 parameter grid below (Y: 152-214). Used for pages with meaningful visual feedback — filter response curves, FM algorithm diagrams, envelope ADSR shapes.

```
┌────────────────────────────────┐ Y=28
│                                │
│    [Visualization]             │
│    filter curve / FM algo /    │
│    envelope shape / etc.       │
│                                │
├────────────────────────────────┤ Y=144
│                                │
│  CUTOFF  72   RESO   45   MODE LP4  │
│  ▓▓▓▓▓░░░░   ▓▓▓░░░░░░   ▓▓▓▓▓░░  │
│  ENV    +64   LFO   +20   TRACK 50  │
│  ▓▓▓▓▓▓░░░   ▓▓░░░░░░░   ▓▓▓▓░░░  │
│                                │
└────────────────────────────────┘ Y=214
```

**CellGrid layout:** 3×2 grid of independent cells, each with a mini icon, label, value, and bar. Used for pages where each parameter is visually independent — drive, wavefolder, mixer, modal resonator.

```
┌────────────────────────────────┐ Y=28
│                                │
│  ┌──────┐ ┌──────┐ ┌──────┐  │
│  │~icon~│ │~icon~│ │~icon~│  │
│  │DRIVE │ │TONE  │ │MIX   │  │
│  │  72  │ │ +12  │ │  50  │  │
│  │▓▓▓▓░░│ │▓▓▓░░░│ │▓▓▓▓░│  │
│  └──────┘ └──────┘ └──────┘  │
│  ┌──────┐ ┌──────┐ ┌──────┐  │
│  │~icon~│ │~icon~│ │~icon~│  │
│  │E.AMT │ │L.AMT │ │ --   │  │
│  │ +40  │ │ +20  │ │  --  │  │
│  │▓▓▓░░░│ │▓▓░░░░│ │      │  │
│  └──────┘ └──────┘ └──────┘  │
│                                │
└────────────────────────────────┘ Y=214
```

### Dungeon Map (Y: 214-320)

Always visible. Shows the chain topology and current position.

```
┌────────────────────────────────┐ Y=214
│                                │
│  ─[FM ]─[DRV]─[FLT]─[FLD]─[VCA]─  │
│                 ^^^              │
│            (you are here)       │
│                                │
│  Sub-pages: [CUT] [RES] [MOD] │
│              ^^^               │
└────────────────────────────────┘ Y=320
```

- Nodes are boxes with 3-char abbreviations connected by lines.
- The active node is highlighted (filled cyan background).
- If the active node has sub-pages, they appear below as a vertical list.
- The active sub-page is highlighted.

The dungeon map is a pure navigation visualization. It shows position. That's all.

---

## Page System

### What Is a Page

A page is a self-contained editing screen. It has:

1. **6 parameter slots** — label, value, format (unipolar/bipolar/integer), cell icon
2. **A layout mode** — BigViz or CellGrid
3. **A visualization** (BigViz mode) — visual representation of the block's behavior
4. **Read function** — extracts 6 normalized values from the parameter state
5. **Write function** — applies encoder deltas to the parameter state
6. **Dirty detection** — quantized value snapshots for partial redraw

Pages come in two kinds:

- **Block pages:** Backed by a DSP block in a chain. The page defines the block's user-facing parameters AND modulation depth controls. The DSP block's `process()` runs in the audio thread; the page's read/write functions run in the UI thread.
- **Menu pages:** Configuration screens — mixer, MIDI setup, patch management, system settings. No DSP block.

### How Pages Map to the Chain

Each block in a chain becomes one or more pages. B1-B6 buttons map to pages sequentially. The mod matrix is always the last page.

**Example: "FM Poly" chain on Part 1 (B1)**

```
Press B1 → enter Part 1's chain.
Navigate with Minus/Plus:

  [FM Osc] → [Drive] → [Filter] → [Wavefolder] → [VCA] → [Mod Matrix]
     │
     ├── sub-page 0: FM-A (algorithm, feedback, carrier)   ← Seq/Edit to move
     ├── sub-page 1: FM-B (modulator, op2)
     └── sub-page 2: FM-C (op3, op4)
```

**Example: "Kick" chain on Part 4 (B4)**

```
Press B4 → enter Part 4's chain.

  [Noise Exciter] → [Tuned Resonator] → [Low Pass Gate] → [Mod Matrix]
```

Pressing a Part button always lands on the first block (leftmost). Minus/Plus traverses the chain. The mod matrix is always the last node.

### Page Definitions for All Block Types

Each block type defines its page. Below is the complete specification for every block page in the MVP.

---

#### FM Osc Block

3 sub-pages (complex block with many parameters).

**FM-A: Algorithm + Carrier (sub-page 0)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | ALGO | Int(7) | — | Algorithm 0-7 |
| B | FDBK | Uni | — | Op4 self-feedback amount |
| C | RAT C | Uni | — | Carrier (op4) harmonic ratio |
| D | WAV C | Int(7) | — | Carrier waveform (sine, half, full, etc.) |
| E | LVL C | Uni | — | Carrier output level |
| F | DTN C | Bi | — | Carrier fine detune |

Layout: **BigViz** — visualization shows algorithm routing diagram (4 operator boxes with connections).

**FM-B: Modulator + Op2 (sub-page 1)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | RAT M | Uni | — | Modulator (op1) ratio |
| B | WAV M | Int(7) | — | Modulator waveform |
| C | LVL M | Uni | — | Modulator depth |
| D | DTN M | Bi | — | Modulator detune |
| E | RAT 2 | Uni | — | Op2 ratio |
| F | LVL 2 | Uni | — | Op2 level |

Layout: **BigViz** — same algorithm diagram, op1/op2 highlighted.

**FM-C: Op3 + Remaining (sub-page 2)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | RAT 3 | Uni | — | Op3 ratio |
| B | WAV 3 | Int(7) | — | Op3 waveform |
| C | LVL 3 | Uni | — | Op3 level |
| D | WAV 2 | Int(7) | — | Op2 waveform |
| E | WAV 4 | Int(7) | — | Op4 waveform |
| F | DTN 4 | Bi | — | Op4 detune |

Layout: **BigViz** — same algorithm diagram, op3/op4 highlighted.

---

#### Modal Resonator Block

2 sub-pages.

**Modal-1: Core Parameters (sub-page 0)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | MODE | Int(3) | — | Resonator mode (String/Modal/Bowed/Sympathetic) |
| B | EXCITE | Uni | Burst | Excitation amount |
| C | DECAY | Uni | Ripple | Resonance decay time |
| D | BRIGHT | Uni | Arc | Brightness/timbre |
| E | POS | Uni | Arc | Pluck/excitation position |
| F | INHARM | Uni | Arc | Inharmonicity |

Layout: **CellGrid**

**Modal-2: Extended Parameters (sub-page 1)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | BODY | Uni | Arc | KS body resonance |
| B | STIFF | Uni | Arc | KS allpass dispersion |
| C | FDBK | Uni | Arc | KS sustain feedback |
| D | E.DPT | Uni | Arc | Ensemble depth |
| E | E.RAT | Uni | Arc | Ensemble rate |
| F | E.MIX | Uni | DryWet | Ensemble mix |

Layout: **CellGrid**

---

#### Noise Block

1 page.

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | TYPE | Int(2) | — | White / Pink / Filtered |
| B | PITCH | Uni | Arc | Pitch sweep start |
| C | SWEEP | Uni | Arc | Pitch sweep time |
| D | LEVEL | Uni | LevelBar | Output level |
| E | E.AMT | Bi | Arc | Env amount |
| F | — | — | — | Unused |

Layout: **CellGrid**

---

#### Drive Block

1 page.

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | DRIVE | Uni | WaveClip | Drive amount |
| B | TONE | Bi | ToneTilt | Pre/post tilt EQ |
| C | MIX | Bi | DryWet | Dry/wet blend |
| D | E.AMT | Bi | Arc | Env → drive depth |
| E | L.AMT | Bi | Arc | LFO → drive depth |
| F | LEVEL | Uni | LevelBar | Output attenuation |

Layout: **CellGrid**

---

#### Filter Block

1 page.

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | CUTOFF | Uni | — | Filter cutoff frequency |
| B | RESO | Uni | — | Resonance / Q |
| C | MODE | Int(7) | — | Filter mode (LP1/LP2/LP4/BP2/BP4/HP4/NT2/Phazor) |
| D | E.AMT | Bi | — | Env → cutoff depth |
| E | L.AMT | Bi | — | LFO → cutoff depth |
| F | TRACK | Uni | — | Keyboard tracking amount |

Layout: **BigViz** — visualization shows frequency response curve that responds to cutoff/resonance/mode.

---

#### Wavefolder Block

1 page.

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | FOLD | Uni | WaveFold | Fold amount |
| B | SYM | Bi | Symmetry | Symmetry / bias |
| C | MIX | Bi | DryWet | Dry/wet blend |
| D | E.AMT | Bi | Arc | Env → fold depth |
| E | L.AMT | Bi | Arc | LFO → fold depth |
| F | LEVEL | Uni | LevelBar | Output attenuation |

Layout: **CellGrid**

---

#### Low Pass Gate Block

1 page.

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | CUTOFF | Uni | — | Gate cutoff frequency |
| B | VACTRL | Uni | Arc | Vactrol response (fast → slow) |
| C | DECAY | Uni | Ripple | Natural decay time |
| D | LEVEL | Uni | LevelBar | Output level |
| E | E.AMT | Bi | Arc | Env → cutoff depth |
| F | VEL | Uni | Arc | Velocity → level depth |

Layout: **BigViz** — visualization shows combined filter+VCA response curve.

---

#### VCA Block

1 page.

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | ATK | Uni | — | Amp envelope attack |
| B | DEC | Uni | — | Amp envelope decay |
| C | SUS | Uni | — | Amp envelope sustain |
| D | REL | Uni | — | Amp envelope release |
| E | LEVEL | Uni | — | Output level |
| F | VEL | Uni | — | Velocity sensitivity |

Layout: **BigViz** — visualization shows ADSR envelope shape with breakpoints.

---

#### Mod Matrix (always last page in every chain)

Multi-sub-page block.

**Sub-page 0: Routing Grid**

The grid shows all legal source→destination connections for this chain. Each cell is toggleable (on/off). The grid auto-generates from the chain's blocks — only parameters that exist in the chain appear as columns.

```
┌─────────────────────────────────────────────┐
│  MOD MATRIX                                  │
│                                              │
│            FLT.Cut  DRV.Amt  FLD.Amt  VCA.Lv│
│  LFO 1    [  X  ]  [     ]  [  X  ]  [     ]│
│  LFO 2    [     ]  [     ]  [     ]  [     ]│
│  Env 1    [  X  ]  [     ]  [     ]  [  X  ]│
│  Env 2    [     ]  [  X  ]  [     ]  [     ]│
│  Vel      [     ]  [     ]  [     ]  [  X  ]│
│  ModWhl   [  X  ]  [     ]  [     ]  [     ]│
│                                              │
└─────────────────────────────────────────────┘
```

Navigation within the grid:
- Encoders A-F scroll/select rows and columns
- Encoder A: Select source (row)
- Encoder B: Select destination (column)  
- Encoder C: Toggle connection on/off
- Minus/Plus: Scroll the grid if it exceeds screen

Layout: **CellGrid** (custom rendering — the grid replaces the standard cell layout)

**Sub-pages 1+: Modulator Settings**

One sub-page per modulator. Navigate via Seq/Edit (up/down) from the grid page.

**LFO Page:**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | RATE | Uni | Orbit | LFO rate |
| B | SHAPE | Int(5) | WaveShape | Sine/Tri/Saw/Square/Random/S&H |
| C | SYNC | Int(1) | Arc | Free / Key sync |
| D | PHASE | Uni | Arc | Start phase |
| E | DEPTH | Uni | Breathe | Global depth multiplier |
| F | DELAY | Uni | Arc | Fade-in time |

Layout: **CellGrid**

**Envelope Page:**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | ATK | Uni | — | Attack time |
| B | DEC | Uni | — | Decay time |
| C | SUS | Uni | — | Sustain level |
| D | REL | Uni | — | Release time |
| E | LEVEL | Uni | — | Envelope depth |
| F | VEL | Uni | — | Velocity sensitivity |

Layout: **BigViz** — ADSR envelope visualization.

---

### Menu Pages (Non-Block)

Accessed via navigation buttons. Not part of any chain.

#### Mixer Page (MIX button)

Shows all Parts with levels, pans, output assignments, and send levels.

```
┌────────────────────────────────┐
│ MIXER                          │
│                                │
│ P1 [FM Poly]  ████░░  DAC1    │
│ P2 [Kick]     ███░░░  DAC2    │
│ P3 [Pluck]    █████░  DAC3    │
│ P4 [Snare]    ██░░░░  DAC2    │
│ P5 [--]       ░░░░░░  --      │
│ P6 [--]       ░░░░░░  --      │
│                                │
│ Master:       ████████░  -2dB  │
└────────────────────────────────┘
```

Sub-pages (Seq/Edit):

**Sub-page 0: Parts 1-3 volume + master**

| Encoder | Label | Parameter |
|---------|-------|-----------|
| A | P1 VOL | Part 1 volume |
| B | P2 VOL | Part 2 volume |
| C | P3 VOL | Part 3 volume |
| D | MASTER | Master volume |
| E | SEND 1 | Last-selected Part → Send 1 level |
| F | SEND 2 | Last-selected Part → Send 2 level |

**Sub-page 1: Parts 4-6 volume + output**

| Encoder | Label | Parameter |
|---------|-------|-----------|
| A | P4 VOL | Part 4 volume |
| B | P5 VOL | Part 5 volume |
| C | P6 VOL | Part 6 volume |
| D | OUTPUT | Last-selected Part → DAC assignment |
| E | PAN | Last-selected Part → Pan |
| F | — | — |

**Sub-page 2: Send effect parameters**

| Encoder | Label | Parameter |
|---------|-------|-----------|
| A | R.TYPE | Reverb type (Plate/FDN/MidiVerb) |
| B | R.TIME | Reverb time |
| C | R.DAMP | Reverb damping |
| D | R.MIX | Reverb mix |
| E | D.TIME | Delay time |
| F | D.FDBK | Delay feedback |

Layout: **CellGrid**

#### Patch Page (EDIT button)

Patch save/load/copy/init.

```
┌────────────────────────────────┐
│ PATCH                          │
│                                │
│ Current: 001 - Init FM         │
│                                │
│ [SAVE] [LOAD] [COPY] [INIT]   │
│                                │
│ Bank: A   Slot: 001            │
│                                │
│ Name: ________________________ │
└────────────────────────────────┘
```

| Encoder | Label | Parameter |
|---------|-------|-----------|
| A | BANK | Bank select (A-H) |
| B | SLOT | Patch number (001-128) |
| C | ACTION | Save / Load / Copy / Init |
| D | — | — |
| E | — | — |
| F | CONFIRM | Confirm action |

Main encoder scrolls through patches for preview. B1-B4 buttons trigger Save/Load/Copy/Init directly.

Layout: **CellGrid** (custom rendering)

#### System Chain (MENU button)

MENU enters a fixed system chain. Same navigation as Part chains — Minus/Plus to traverse blocks, Seq/Edit for sub-pages. The dungeon map shows:

```
MENU: [MIDI] → [Tuning] → [Theme] → [Updates] → [About]
```

**MIDI Block (sub-page 0: Channel Assignment)**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | P1 CH | Int(16) | Part 1 MIDI channel (1-16) |
| B | P2 CH | Int(16) | Part 2 MIDI channel (1-16, OFF) |
| C | P3 CH | Int(16) | Part 3 MIDI channel (1-16, OFF) |
| D | P4 CH | Int(16) | Part 4 MIDI channel (1-16, OFF) |
| E | P5 CH | Int(16) | Part 5 MIDI channel (1-16, OFF) |
| F | P6 CH | Int(16) | Part 6 MIDI channel (1-16, OFF) |

**MIDI Block (sub-page 1: Global MIDI)**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | CLOCK | Int(1) | Clock source (Int / Ext MIDI) |
| B | PGM.CH | Int(1) | Program change receive (On / Off) |
| C | CC.RX | Int(1) | CC receive (On / Off) |
| D | — | — | — |
| E | — | — | — |
| F | — | — | — |

Layout: **CellGrid**

**Tuning Block**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | TUNE | Bi | Master tuning (A=435-445 Hz) |
| B | SCALE | Int(2) | Scale (Equal / Just / Pythagorean) |
| C | — | — | — |
| D | — | — | — |
| E | — | — | — |
| F | — | — | — |

Layout: **CellGrid**

**Theme Block**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | BRIGHT | Uni | Screen brightness |
| B | ACCENT | Int(4) | Accent color preset |
| C | — | — | — |
| D | — | — | — |
| E | — | — | — |
| F | — | — | — |

Layout: **CellGrid**

**Updates Block**

Informational — shows current firmware version, SD card status. No editable parameters.

Layout: **CellGrid**

**About Block**

Layout: **BigViz** — visualization shows Chimera logo + credits. No editable parameters.

---

## Navigation State Machine

### State

```rust
struct NavigationState {
    chain: ChainId,        // Which chain is active
    chain_node: usize,     // Horizontal position in chain
    sub_page: usize,       // Vertical sub-page at current node
}

enum ChainId {
    Part(usize),           // 0-5 (B1-B6)
    Menu,                  // MENU button
    Mixer,                 // MIX + B1 (special — single page, not a chain)
}
```

### Transitions

| Input | Current State | Action |
|-------|---------------|--------|
| **B1-B6 pressed** | Any | Switch to that Part's chain, first block. If already on that Part, snap home (node 0, sub_page 0). |
| **MENU pressed** | Any | Switch to system chain, first block. If already on system chain, snap home. |
| **MIX + B1** | Any | Switch to Mixer page. |
| **MIX + B2-B6** | Any | Reserved for future functions. |
| **Minus pressed** | Any chain | Move left one node (decrement chain_node). Reset sub_page to 0. |
| **Plus pressed** | Any chain | Move right one node (increment chain_node). Reset sub_page to 0. |
| **Seq pressed** | Node with sub-pages | Move up one sub-page (decrement sub_page). |
| **Edit pressed** | Node with sub-pages | Move down one sub-page (increment sub_page). |
| **Encoder A-F turn** | Any | Apply delta to parameter at encoder index on current page. |
| **MIX (hold) + Encoder** | Any | Shift mode: coarse snap instead of fine adjustment. |

### Key Invariants

1. **B1-B6 always select a Part's chain.** Pressing B3 takes you to Part 3's chain, first block. Even from Mixer or Menu.
2. **MENU always enters the system chain.** Same navigation model as Part chains — Minus/Plus to traverse, Seq/Edit for sub-pages.
3. **Everything is a chain except the mixer.** The mixer is the one global page (with sub-pages) accessed via MIX + B1.
4. **Only one page is active at a time.** No overlays, no popups, no modal dialogs.
5. **The dungeon map always shows the active chain.** Part chain, system chain, or dimmed when on the mixer.
6. **Pressing the same button again snaps home.** B2 while on Part 2 = back to node 0. MENU while on system chain = back to MIDI block.
7. **MIX is only a modifier.** It does nothing on its own.

---

## Dirty Region Tracking

### How It Works

Each frame (30fps target):

1. **Quantize** current animated parameter values to u16 (prevents float jitter triggering redraws).
2. **Compare** each screen region's current data snapshot against its previous snapshot.
3. **If different:** Clear the region via direct framebuffer write. Redraw. Add to flush list.
4. **Flush** only the changed Y-ranges to the display hardware (partial SPI transfer).

### Regions by Layout

**BigViz layout:** 4 regions

| Region | Y Range | Dirty Trigger |
|--------|---------|---------------|
| Header | 0-28 | Part/page/sub-page change, perf stats change |
| Viz | 28-144 | Any of 6 parameter values changed (affects visualization) |
| Params | 144-214 | Any of 6 parameter values changed (bar/number update) |
| Nav | 214-320 | Chain position changed |

**CellGrid layout:** 3 regions

| Region | Y Range | Dirty Trigger |
|--------|---------|---------------|
| Header | 0-28 | Part/page/sub-page change |
| Cells | 28-214 | Any of 6 parameter values changed |
| Nav | 214-320 | Chain position changed |

### Optimization

- Parameter values are quantized to 1/1000th resolution. Changes smaller than 0.001 don't trigger redraws.
- Animated values interpolate smoothly (ease-out cubic at 15% per frame). This means a parameter change triggers ~5-7 frames of redraws, then settles.
- The dungeon map only redraws when navigation position changes — not on parameter edits.
- Header only redraws when Part/page changes or every ~8 frames (perf stats update).

---

## Animation System

### AnimatedValue

Each of the 6 encoder parameters has an `AnimatedValue` that smoothly interpolates from current to target.

```
Speed:          0.15 per frame (15% of remaining gap)
Snap threshold: 0.001 (values closer than this jump instantly)
Frame rate:     30fps target
Settling time:  ~5-7 frames (~170-230ms) for full travel
```

When navigation changes (page switch), all 6 animated values **snap** instantly to the new page's values. No animation on page transitions — only on parameter edits within a page.

---

## Visualization Dispatch

Each block type defines its visualization for BigViz layout. CellGrid pages use per-cell mini icons instead.

| Block Type | Visualization | Description |
|------------|---------------|-------------|
| FM Osc | Algorithm diagram | 4 operator boxes with routing arrows per algorithm |
| Modal Resonator | — | CellGrid with per-param icons |
| Noise | — | CellGrid |
| Drive | — | CellGrid with WaveClip icons |
| Filter | Frequency response | Curve showing cutoff, resonance peak, rolloff slope |
| Wavefolder | — | CellGrid with WaveFold icons |
| Low Pass Gate | Combined response | Filter curve that also shows VCA decay |
| VCA | ADSR envelope | Breakpoint diagram with A/D/S/R segments |
| Envelope (mod) | ADSR envelope | Same as VCA |
| Mod Matrix | Routing grid | Custom grid rendering |

---

## Chain Editor (Power User)

Accessed via: **MIX + MENU** (deliberate shift combo).

Shows the full chain as an editable list of blocks:

```
┌────────────────────────────────┐
│ CHAIN EDITOR — Part 1          │
│                                │
│  1. [FM Osc      ]  200 cy    │
│  2. [Drive       ]   20 cy    │
│  3. [Filter      ]   80 cy    │
│  4. [Wavefolder  ]   40 cy    │
│  5. [VCA         ]   50 cy    │
│                                │
│  Budget: 390 / 1600 cy  (24%) │
│                                │
│  [+ADD]  [-DEL]  [↑↓ MOVE]    │
└────────────────────────────────┘
```

- Main encoder: select block in list
- Encoder A: change block type (scrolls through block vocabulary)
- B1: Add block below cursor
- B2: Delete selected block
- Minus/Plus: Move selected block up/down
- MENU: Exit chain editor

The CPU budget display shows cycles per sample for the current chain and the per-voice budget ceiling. Blocks that would exceed the budget are shown in red.

Most users never see this screen.

---

## First Boot Experience

1. Power on → display shows Chimera logo + firmware version for 1 second.
2. Default project loads: 1 Part, "FM Poly" chain, default parameters.
3. Screen shows FM-A page (algorithm diagram + carrier parameters).
4. MIDI channel 1 active. Notes on channel 1 produce sound immediately.
5. LED is on during boot, off when ready.

No setup wizard. No configuration required. Play immediately.

---

## Theme

### Color Palette

```
Background:     Black (#000000)
Text:           White (#FFFFFF) — parameter values, active labels
Text Dim:       25% gray — inactive labels, secondary info
Text Mid:       45% gray — header labels
Accent:         Cool cyan — active nodes, bars, visualizations
Accent Dim:     Muted cyan — selected but not primary
Accent Bright:  Highlight cyan — momentary feedback
Separator:      Very dark gray — subtle dividers
```

### Typography

Two sizes only:
- **6pt:** Parameter labels, header text, dungeon map abbreviations
- **8pt:** Parameter values (numeric), page names

No decorative fonts. No variable weight. Monospace-adjacent for alignment.

### Spacing Principles

- **8px grid.** All vertical positions align to 8px multiples where possible.
- **Generous padding in content zone.** Parameters don't crowd each other.
- **Tight packing in dungeon map.** Maximum information density in the nav zone.
- **No borders on cells.** Negative space separates elements, not lines.

---

## Implementation Notes

### What Changes from Current Code

The current codebase has a hardcoded `PageId` enum with 21 variants and hardcoded chain definitions. The new architecture requires:

1. **Pages are defined by blocks, not a fixed enum.** `PageId` becomes a block type + sub-page index, not a flat enum. Each block type provides its own labels, formats, icons, read/write functions, and visualization.

2. **Chain definitions are per-Part, not global constants.** Each Part owns a chain (list of blocks). The dungeon map renders from the Part's chain, not from `static VOICE_CHAIN`.

3. **Navigation is Part-aware.** B1-B6 select Parts directly (like Digitone T1-T4). Switching Parts changes the chain topology shown in the dungeon map. Minus/Plus navigate within the chain.

4. **The mod matrix page is auto-generated from the chain.** Its routing grid columns come from the chain's blocks. Adding or removing a block updates the grid.

### What Stays the Same

- Screen layout zones (header, content, dungeon map) — Y ranges unchanged
- BigViz vs CellGrid layout modes — same rendering paths
- 6 parameters per page — fundamental constraint from 6 encoders
- Dirty region tracking — same quantize-compare-redraw pipeline
- Animation system — same AnimatedValue with ease-out interpolation
- Theme colors — same palette
- Dungeon map rendering — same node boxes with connectors
- Parameter editing — same nudge/snap mechanics
