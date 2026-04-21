# Chimera UI/UX Specification

## Guiding Principles

1. **Everything is a chain.** Part sound design, mixer channels, system settings — all chains. No special pages. No exceptions.
2. **Every chain is a pipe of blocks.** Minus/Plus to traverse left/right. Seq/Edit for sub-pages up/down. The dungeon map shows your position.
3. **Every block is a page.** 6 encoders, 6 parameters. The page is complete — all parameters for that block, including modulation depths, are right there.
4. **B1-B6 select context.** Normally = Part chains. MIX + B1-B6 = mixer channel chains. MENU = system chain.
5. **One interaction model everywhere.** Learn one pattern, use it for everything — sound design, mixing, MIDI config, effects, system settings.
6. **Complexity is hidden, not absent.** Factory defaults work immediately. Chain structure is invisible to casual users. Power users can edit chains.

---

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
│             [D] [E] [F]             │
│                                      │
│  Part buttons:  [B1][B2][B3]        │
│                 [B4][B5][B6]        │
│                                      │
│  Nav buttons: [MENU] [-] [+]        │
│               [MIX] [EDIT] [SEQ]    │
│                                      │
└──────────────────────────────────────┘
```

| Control | Function |
|---------|----------|
| **Encoders A-F** | Edit the 6 parameters on the current page |
| **B1-B6** | Select Part chain (B1 = Part 1, B2 = Part 2, etc.) |
| **MIX + B1-B6** | Select mixer channel chain (MIX+B1 = CH1, MIX+B2 = CH2, etc.) |
| **MENU** | Enter system chain |
| **Minus / Plus** | Navigate left/right through blocks in the active chain |
| **Seq / Edit** | Navigate up/down through sub-pages at the current block |
| **MIX (hold) + Encoder** | Coarse snap (shift mode) |

---

## Screen Layout

The 240×320 display is divided into three persistent zones:

```
┌────────────────────────────────┐ Y=0
│  HEADER                        │
│  Context > Block > Sub-page    │ 28px
├────────────────────────────────┤ Y=28
│                                │
│  CONTENT ZONE                  │
│  (visualization + parameters)  │ 186px
│                                │
├────────────────────────────────┤ Y=214
│  DUNGEON MAP                   │
│  (chain position)              │ 106px
└────────────────────────────────┘ Y=320
```

### Header (Y: 0-28)

Always visible. Shows current context:

```
Part 1 > Filter                              2.1ms
```

- Left: Context (Part N / Mixer CH N / System) + current block name.
- Right: Performance stats (render time, CPU load). Dim text.

### Content Zone (Y: 28-214)

Two layout modes:

**BigViz:** Large visualization (Y: 28-144) + 3×2 parameter grid (Y: 152-214). For pages with meaningful visual feedback — filter curves, FM algorithm diagrams, ADSR shapes.

**CellGrid:** 3×2 grid of independent cells, each with mini icon + label + value + bar. For pages where parameters are visually independent.

### Dungeon Map (Y: 214-320)

Always visible. Shows the active chain's topology and current position. Nodes are 3-char boxes connected by lines. Active node is highlighted (filled cyan). Sub-pages appear below the active node.

The dungeon map is a navigation visualization. It shows where you are. That's all.

---

## Chain Types

There are three types of chains, all navigated identically:

### Part Chains (B1-B6)

Each Part owns a configurable DSP chain. Audio flows through the blocks in order. The chain defines the Part's sound.

```
Example — "FM Poly" on Part 1:

  B1 → [FM Osc] → [Drive] → [Filter] → [Wavefolder] → [VCA] → [Mod Matrix]
            │
            ├── sub-page 0: FM-A (algorithm, carrier)
            ├── sub-page 1: FM-B (modulator, op2)
            └── sub-page 2: FM-C (op3, op4)

Example — "Kick" on Part 4:

  B4 → [Noise Exciter] → [Tuned Resonator] → [Low Pass Gate] → [Mod Matrix]
```

The mod matrix is always the last block in every Part chain. Factory chain templates provide tasteful defaults — most users never modify the chain structure.

### Mixer Channel Chains (MIX + B1-B6)

Each Part has a mixer channel strip. MIX + B1 enters the channel strip chain for Part 1. MIX + B2 for Part 2, etc.

```
MIX + B1 → [Channel] → [MIDI] → [EQ] → [Sends]

MIX + B2 → [Channel] → [MIDI] → [EQ] → [Sends]
  ...
MIX + B6 → [Channel] → [MIDI] → [EQ] → [Sends]
```

**Channel Block**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | VOL | Uni | Channel volume |
| B | PAN | Bi | Stereo pan |
| C | OUT | Int(2) | Output assignment (DAC1 / DAC2 / DAC3) |
| D | VOICES | Int(5) | Max voice allocation for this Part |
| E | MODE | Int(2) | Voice mode (Poly / Mono / Legato) |
| F | GLIDE | Uni | Portamento time |

Layout: **CellGrid**

**MIDI Block**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | CH | Int(16) | MIDI receive channel (1-16, OFF) |
| B | PGM | Int(1) | Program change receive (On / Off) |
| C | CC.RX | Int(1) | CC receive (On / Off) |
| D | BEND | Int(12) | Pitch bend range (±1 to ±12 semitones) |
| E | TRNS | Bi | Transpose (±24 semitones) |
| F | — | — | — |

Layout: **CellGrid**

**EQ Block**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | LOW | Bi | Low shelf gain |
| B | L.FRQ | Uni | Low shelf frequency |
| C | MID | Bi | Mid band gain |
| D | M.FRQ | Uni | Mid band frequency |
| E | HIGH | Bi | High shelf gain |
| F | H.FRQ | Uni | High shelf frequency |

Layout: **BigViz** — frequency response curve.

**Sends Block**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | REV | Uni | Send 1 level (reverb) |
| B | DLY | Uni | Send 2 level (delay) |
| C | CHR | Uni | Send 3 level (chorus) |
| D | S4 | Uni | Send 4 level (spare) |
| E | — | — | — |
| F | — | — | — |

Layout: **CellGrid**

The mixer channel strip is a fixed chain (not user-configurable for now). Same navigation: Minus/Plus to traverse blocks, Seq/Edit for sub-pages.

### System Chain (MENU)

Global configuration. Fixed chain, not user-configurable.

```
MENU → [MIDI] → [Tuning] → [Theme] → [Updates] → [About]
```

**MIDI Block (sub-page 0: Channel Overview)**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | P1 CH | Int(16) | Part 1 MIDI channel |
| B | P2 CH | Int(16) | Part 2 MIDI channel |
| C | P3 CH | Int(16) | Part 3 MIDI channel |
| D | P4 CH | Int(16) | Part 4 MIDI channel |
| E | P5 CH | Int(16) | Part 5 MIDI channel |
| F | P6 CH | Int(16) | Part 6 MIDI channel |

**MIDI Block (sub-page 1: Global)**

| Encoder | Label | Format | Parameter |
|---------|-------|--------|-----------|
| A | CLOCK | Int(1) | Clock source (Int / Ext MIDI) |
| B | PGM | Int(1) | Program change global (On / Off) |
| C | CC | Int(1) | CC receive global (On / Off) |
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

Informational — firmware version, SD card status. No editable parameters.

Layout: **CellGrid**

**About Block**

Layout: **BigViz** — Chimera logo + credits.

---

## Part Chain Block Definitions

Every DSP block type that can appear in a Part chain. Each defines its page(s).

### FM Osc Block

3 sub-pages.

**FM-A: Algorithm + Carrier (sub-page 0)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | ALGO | Int(7) | — | Algorithm 0-7 |
| B | FDBK | Uni | — | Op4 self-feedback amount |
| C | RAT C | Uni | — | Carrier (op4) harmonic ratio |
| D | WAV C | Int(7) | — | Carrier waveform |
| E | LVL C | Uni | — | Carrier output level |
| F | DTN C | Bi | — | Carrier fine detune |

Layout: **BigViz** — algorithm routing diagram.

**FM-B: Modulator + Op2 (sub-page 1)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | RAT M | Uni | — | Modulator (op1) ratio |
| B | WAV M | Int(7) | — | Modulator waveform |
| C | LVL M | Uni | — | Modulator depth |
| D | DTN M | Bi | — | Modulator detune |
| E | RAT 2 | Uni | — | Op2 ratio |
| F | LVL 2 | Uni | — | Op2 level |

Layout: **BigViz** — algorithm diagram, op1/op2 highlighted.

**FM-C: Op3 + Remaining (sub-page 2)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | RAT 3 | Uni | — | Op3 ratio |
| B | WAV 3 | Int(7) | — | Op3 waveform |
| C | LVL 3 | Uni | — | Op3 level |
| D | WAV 2 | Int(7) | — | Op2 waveform |
| E | WAV 4 | Int(7) | — | Op4 waveform |
| F | DTN 4 | Bi | — | Op4 detune |

Layout: **BigViz** — algorithm diagram, op3/op4 highlighted.

### Modal Resonator Block

2 sub-pages.

**Modal-1: Core (sub-page 0)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | MODE | Int(3) | — | String / Modal / Bowed / Sympathetic |
| B | EXCITE | Uni | Burst | Excitation amount |
| C | DECAY | Uni | Ripple | Decay time |
| D | BRIGHT | Uni | Arc | Brightness |
| E | POS | Uni | Arc | Excitation position |
| F | INHARM | Uni | Arc | Inharmonicity |

Layout: **CellGrid**

**Modal-2: Extended (sub-page 1)**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | BODY | Uni | Arc | KS body resonance |
| B | STIFF | Uni | Arc | KS dispersion |
| C | FDBK | Uni | Arc | KS sustain feedback |
| D | E.DPT | Uni | Arc | Ensemble depth |
| E | E.RAT | Uni | Arc | Ensemble rate |
| F | E.MIX | Uni | DryWet | Ensemble mix |

Layout: **CellGrid**

### Noise Block

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | TYPE | Int(2) | — | White / Pink / Filtered |
| B | PITCH | Uni | Arc | Pitch sweep start |
| C | SWEEP | Uni | Arc | Pitch sweep time |
| D | LEVEL | Uni | LevelBar | Output level |
| E | E.AMT | Bi | Arc | Env amount |
| F | — | — | — | — |

Layout: **CellGrid**

### Drive Block

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | DRIVE | Uni | WaveClip | Drive amount |
| B | TONE | Bi | ToneTilt | Tilt EQ |
| C | MIX | Bi | DryWet | Dry/wet |
| D | E.AMT | Bi | Arc | Env → drive depth |
| E | L.AMT | Bi | Arc | LFO → drive depth |
| F | LEVEL | Uni | LevelBar | Output attenuation |

Layout: **CellGrid**

### Filter Block

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | CUTOFF | Uni | — | Cutoff frequency |
| B | RESO | Uni | — | Resonance |
| C | MODE | Int(7) | — | LP1/LP2/LP4/BP2/BP4/HP4/NT2/Phazor |
| D | E.AMT | Bi | — | Env → cutoff depth |
| E | L.AMT | Bi | — | LFO → cutoff depth |
| F | TRACK | Uni | — | Keyboard tracking |

Layout: **BigViz** — frequency response curve.

### Wavefolder Block

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | FOLD | Uni | WaveFold | Fold amount |
| B | SYM | Bi | Symmetry | Symmetry / bias |
| C | MIX | Bi | DryWet | Dry/wet |
| D | E.AMT | Bi | Arc | Env → fold depth |
| E | L.AMT | Bi | Arc | LFO → fold depth |
| F | LEVEL | Uni | LevelBar | Output attenuation |

Layout: **CellGrid**

### Low Pass Gate Block

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | CUTOFF | Uni | — | Gate cutoff |
| B | VACTRL | Uni | Arc | Vactrol response |
| C | DECAY | Uni | Ripple | Natural decay |
| D | LEVEL | Uni | LevelBar | Output level |
| E | E.AMT | Bi | Arc | Env → cutoff depth |
| F | VEL | Uni | Arc | Velocity → level |

Layout: **BigViz** — combined filter+VCA response.

### VCA Block

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | ATK | Uni | — | Attack |
| B | DEC | Uni | — | Decay |
| C | SUS | Uni | — | Sustain |
| D | REL | Uni | — | Release |
| E | LEVEL | Uni | — | Output level |
| F | VEL | Uni | — | Velocity sensitivity |

Layout: **BigViz** — ADSR envelope shape.

### Mod Matrix Block (always last in chain)

**Sub-page 0: Routing Grid**

Auto-generated from the chain's blocks. Shows all legal source → destination connections. Toggle cells on/off.

```
            FLT.Cut  DRV.Amt  FLD.Amt  VCA.Lv
LFO 1      [  X  ]  [     ]  [  X  ]  [     ]
LFO 2      [     ]  [     ]  [     ]  [     ]
Env 1      [  X  ]  [     ]  [     ]  [  X  ]
Env 2      [     ]  [  X  ]  [     ]  [     ]
Vel        [     ]  [     ]  [     ]  [  X  ]
ModWhl     [  X  ]  [     ]  [     ]  [     ]
```

- Encoder A: Select source (row)
- Encoder B: Select destination (column)
- Encoder C: Toggle connection
- Minus/Plus: Scroll if grid exceeds screen

**Sub-pages 1+: Modulator Settings** (one per modulator)

**LFO:**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | RATE | Uni | Orbit | Rate |
| B | SHAPE | Int(5) | WaveShape | Sine/Tri/Saw/Sq/Rnd/S&H |
| C | SYNC | Int(1) | Arc | Free / Key sync |
| D | PHASE | Uni | Arc | Start phase |
| E | DEPTH | Uni | Breathe | Global depth |
| F | DELAY | Uni | Arc | Fade-in time |

Layout: **CellGrid**

**Envelope:**

| Encoder | Label | Format | Icon | Parameter |
|---------|-------|--------|------|-----------|
| A | ATK | Uni | — | Attack |
| B | DEC | Uni | — | Decay |
| C | SUS | Uni | — | Sustain |
| D | REL | Uni | — | Release |
| E | LEVEL | Uni | — | Depth |
| F | VEL | Uni | — | Velocity sensitivity |

Layout: **BigViz** — ADSR visualization.

---

## Navigation State Machine

### State

```rust
struct NavigationState {
    chain: ChainId,
    chain_node: usize,
    sub_page: usize,
}

enum ChainId {
    Part(usize),          // 0-5, selected by B1-B6
    Mixer(usize),         // 0-5, selected by MIX + B1-B6
    System,               // selected by MENU
}
```

### Transitions

| Input | Action |
|-------|--------|
| **B1-B6** | Enter Part chain N, first block. Same button again = snap home. |
| **MIX + B1-B6** | Enter mixer channel chain N, first block. Same combo again = snap home. |
| **MENU** | Enter system chain, first block. Again = snap home. |
| **Minus** | Move left one block. Reset sub_page to 0. |
| **Plus** | Move right one block. Reset sub_page to 0. |
| **Seq** | Sub-page up (decrement). |
| **Edit** | Sub-page down (increment). |
| **Encoder A-F** | Edit parameter at that index on current page. |
| **MIX + Encoder** | Coarse snap (shift mode). |

### Invariants

1. **B1-B6 always select Part chains.** From anywhere — mixer, system, another Part.
2. **MIX + B1-B6 always select mixer channel chains.** From anywhere.
3. **MENU always enters the system chain.** From anywhere.
4. **Everything is a chain.** Part chains, mixer channel chains, system chain — all navigated identically.
5. **One page active at a time.** No overlays, no popups, no modals.
6. **Dungeon map always shows the active chain.** The topology updates when you switch chains.
7. **Same button = snap home.** Pressing B2 while on Part 2 returns to node 0, sub_page 0.
8. **MIX is only a modifier.** Does nothing alone.

---

## Mixer Signal Flow

The mixer is not a single page — it's 6 channel strip chains (one per Part) plus a global main bus. The channel strips are accessed via MIX + B1-B6.

```
Part 1 ──[CH1 Strip]──┬──► Main Bus ──► [Compressor] ──► [Limiter] ──► DAC1
Part 2 ──[CH2 Strip]──┤       ▲
Part 3 ──[CH3 Strip]──┤       │
Part 4 ──[CH4 Strip]──┤   Send Returns
Part 5 ──[CH5 Strip]──┤       ▲
Part 6 ──[CH6 Strip]──┘       │
                       │   ┌───┴────┐
                       ├──►│ Send 1  │  Reverb
                       ├──►│ Send 2  │  Delay
                       ├──►│ Send 3  │  Chorus
                       └──►│ Send 4  │  (spare)
                           └─────────┘
```

Parts assigned to DAC2/DAC3 go direct (bypass main bus processing). Send effect returns sum into the main bus.

---

## Dirty Region Tracking

Each frame (30fps target):

1. **Quantize** animated parameter values to u16 (prevents float jitter).
2. **Compare** each region's data snapshot against previous.
3. **If changed:** Clear region, redraw, add to flush list.
4. **Flush** only changed Y-ranges (partial SPI transfer).

**BigViz layout (4 regions):**

| Region | Y Range | Dirty Trigger |
|--------|---------|---------------|
| Header | 0-28 | Context/page change, perf stats |
| Viz | 28-144 | Parameter values changed |
| Params | 144-214 | Parameter values changed |
| Nav | 214-320 | Chain position changed |

**CellGrid layout (3 regions):**

| Region | Y Range | Dirty Trigger |
|--------|---------|---------------|
| Header | 0-28 | Context/page change |
| Cells | 28-214 | Parameter values changed |
| Nav | 214-320 | Chain position changed |

---

## Animation

Each of 6 parameters has an `AnimatedValue`:

- Speed: 0.15 per frame (15% of remaining gap)
- Snap threshold: 0.001
- Settling: ~5-7 frames (~200ms)
- Page transitions: instant snap, no animation

---

## Visualization Dispatch

| Block Type | Layout | Visualization |
|------------|--------|---------------|
| FM Osc | BigViz | Algorithm routing diagram |
| Modal Resonator | CellGrid | Per-param icons |
| Noise | CellGrid | Per-param icons |
| Drive | CellGrid | WaveClip icons |
| Filter | BigViz | Frequency response curve |
| Wavefolder | CellGrid | WaveFold icons |
| Low Pass Gate | BigViz | Combined filter+VCA curve |
| VCA | BigViz | ADSR envelope |
| Envelope (mod) | BigViz | ADSR envelope |
| Mod Matrix | CellGrid | Routing grid (custom) |
| Mixer Channel | CellGrid | Level/pan indicators |
| Mixer EQ | BigViz | EQ frequency response |

---

## Chain Editor (Power User)

Accessed via **MIX + MENU**.

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

- Encoder A: Change block type at cursor
- B1: Add block below cursor
- B2: Delete selected block
- Minus/Plus: Move selected block up/down
- MENU: Exit chain editor

Most users never open this.

---

## First Boot

1. Power on → Chimera logo + version (1 second).
2. Default project: Part 1 = "FM Poly" chain, Parts 2-6 empty.
3. Screen shows FM-A page. MIDI channel 1 active.
4. Send notes → hear sound immediately. No setup required.

---

## Theme

```
Background:     Black
Text:           White (values, active labels)
Text Dim:       25% gray (inactive labels)
Text Mid:       45% gray (header labels)
Accent:         Cool cyan (active elements, bars, viz)
Accent Dim:     Muted cyan (selected but secondary)
Accent Bright:  Highlight cyan (momentary feedback)
Separator:      Very dark gray
```

Two font sizes: 6pt (labels, map) and 8pt (values, names). 8px grid alignment. No borders — negative space separates elements.

---

## Implementation Delta

### What Changes from Current Code

1. **Pages defined by blocks, not a fixed enum.** Each block type provides labels, formats, icons, read/write, visualization. No more `PageId` with 21 hardcoded variants.
2. **Chains are per-Part, not global constants.** Each Part owns a chain. The dungeon map renders from the active Part's chain.
3. **B1-B6 = Part select** (like Digitone T1-T4). Not page select within a chain.
4. **Mixer is 6 channel strip chains**, not a single page.
5. **System settings are a chain**, not a flat settings page.
6. **Mod matrix auto-generates from chain blocks.**

### What Stays the Same

- Screen layout zones (header, content, dungeon map) — Y ranges unchanged
- BigViz vs CellGrid layout modes
- 6 parameters per page (6 encoders)
- Dirty region tracking (quantize-compare-redraw)
- Animation system (AnimatedValue, ease-out cubic)
- Theme colors
- Dungeon map rendering (node boxes with connectors)
- Parameter editing mechanics (nudge/snap)
