# Chimera Site Map

Complete topology of every chain, block, and sub-page in the instrument.

Placeholder blocks are marked with `[TBD]` — the slot exists, exact parameters to be defined later.

---

## Part Chains (B1-B6)

Each Part button enters that Part's DSP chain. Chain composition depends on the loaded template. The mod matrix is always the last block.

### Factory Chain Templates

**"FM Poly"** (default on Part 1)

```
[FM Osc] → [Drive] → [Filter] → [Wavefolder] → [VCA] → [Mod Matrix]
   │
   ├── FM-A: Algorithm, feedback, carrier ratio/wave/level/detune
   ├── FM-B: Modulator ratio/wave/level/detune, op2 ratio/level
   └── FM-C: Op3 ratio/wave/level, op2 wave, op4 wave/detune
```

**"FM Keys"**

```
[FM Osc] → [Filter] → [VCA] → [Mod Matrix]
   │
   ├── FM-A
   ├── FM-B
   └── FM-C
```

**"FM Bass"**

```
[FM Osc] → [Drive] → [Filter] → [VCA] → [Mod Matrix]
   │
   ├── FM-A
   ├── FM-B
   └── FM-C
```

**"FM Lead"**

```
[FM Osc] → [Drive] → [Filter] → [Wavefolder] → [VCA] → [Mod Matrix]
   │
   ├── FM-A
   ├── FM-B
   └── FM-C
```

**"FM Pad"**

```
[FM Osc] → [Filter] → [VCA] → [Mod Matrix]
   │
   ├── FM-A
   ├── FM-B
   └── FM-C
```

**"Modal Pluck"**

```
[Modal Resonator] → [Filter] → [VCA] → [Mod Matrix]
   │
   ├── Modal-1: Mode, excite, decay, bright, pos, inharm
   └── Modal-2: Body, stiff, fdbk, ensemble depth/rate/mix
```

**"Modal Bow"**

```
[Modal Resonator] → [Drive] → [VCA] → [Mod Matrix]
   │
   ├── Modal-1
   └── Modal-2
```

**"Kick"**

```
[Noise] → [TBD: Tuned Resonator] → [Low Pass Gate] → [Mod Matrix]
```

**"Snare"**

```
[Noise] → [Filter] → [VCA] → [Mod Matrix]
```

**"Hat"**

```
[Noise] → [Filter] → [VCA] → [Mod Matrix]
```

---

## Part Chain Block Inventory

Every block type that can appear in a Part chain:

| Block | Sub-pages | Layout | Viz | Status |
|-------|-----------|--------|-----|--------|
| FM Osc | 3 (FM-A, FM-B, FM-C) | BigViz | AlgorithmDiagram | Defined |
| Modal Resonator | 2 (Core, Extended) | CellGrid | None | Defined |
| Noise | 0 | CellGrid | None | Defined |
| Drive | 0 | CellGrid | None | Defined |
| Filter | 0 | BigViz | FilterResponse | Defined |
| Wavefolder | 0 | CellGrid | None | Defined |
| Low Pass Gate | 0 | BigViz | LpgResponse | Defined |
| VCA | 0 | BigViz | Adsr | Defined |
| Tuned Resonator | TBD | TBD | TBD | Placeholder |
| Mod Matrix | N (grid + 1 per modulator) | CellGrid | None | Defined |

---

## Mixer Channel Chains (MIX + B1-B6)

Each Part has a mixer channel strip chain. MIX + B1 = CH1, MIX + B2 = CH2, etc. All 6 channel strips have the same topology:

```
MIX + Bn → [Channel] → [MIDI] → [EQ] → [Sends] → [TBD: Insert?]
```

| Block | Sub-pages | Layout | Viz | Status |
|-------|-----------|--------|-----|--------|
| Channel | 0 | CellGrid | None | Defined — vol, pan, output, voices, mode, glide |
| MIDI | 0 | CellGrid | None | Defined — channel, pgm, cc, bend range, transpose |
| EQ | 0 | BigViz | EqResponse | Defined — low/mid/high gain + freq |
| Sends | 0 | CellGrid | None | Defined — rev, dly, chr, s4 levels |
| Insert | TBD | TBD | TBD | Placeholder — future per-channel insert effect |

---

## System Chain (MENU)

Global configuration. Fixed chain, same for everyone.

```
MENU → [MIDI] → [Tuning] → [Theme] → [Patch] → [Updates] → [About]
```

| Block | Sub-pages | Layout | Viz | Status |
|-------|-----------|--------|-----|--------|
| MIDI | 2 (channels, global) | CellGrid | None | Defined |
| Tuning | 0 | CellGrid | None | Defined — master tune, scale |
| Theme | 0 | CellGrid | None | Defined — brightness, accent color |
| Patch | TBD | TBD | TBD | Placeholder — save/load/copy/init workflow |
| Updates | 0 | CellGrid | None | Placeholder — firmware version, SD status |
| About | 0 | BigViz | Logo | Placeholder — credits |

---

## Send Effects Chain (TBD)

The send effect parameters need a home. Options under consideration:

```
Option A: Accessible from mixer — MIX + MENU → [Reverb] → [Delay] → [Chorus] → [Send 4] → [Master]
Option B: Sub-pages on the Sends block in each mixer channel strip
Option C: Dedicated chain on a button combo
```

Regardless of where they live, these blocks exist:

| Block | Sub-pages | Layout | Viz | Status |
|-------|-----------|--------|-----|--------|
| Reverb | 0 | CellGrid | None | Placeholder — type, time, damp, size, mix, ... |
| Delay | 0 | CellGrid | None | Placeholder — time, fdbk, wow, sat, tone, mix |
| Chorus | 0 | CellGrid | None | Placeholder — mode, rate, depth, mix, ... |
| Send 4 | TBD | TBD | TBD | Placeholder — future |
| Master Bus | TBD | TBD | TBD | Placeholder — compressor, limiter, master vol |

---

## Chain Editor (MIX + MENU)

Not a chain — a special editor mode for power users. Edits the active Part's chain.

| Element | Status |
|---------|--------|
| Block list with CPU costs | Defined |
| Add/delete/reorder | Defined |
| Budget display | Defined |

---

## Complete Navigation Map

```
┌─────────────────────────────────────────────────────────────────┐
│                                                                 │
│  B1 → Part 1 chain    [block] → [block] → ... → [Mod Matrix]  │
│  B2 → Part 2 chain    [block] → [block] → ... → [Mod Matrix]  │
│  B3 → Part 3 chain    [block] → [block] → ... → [Mod Matrix]  │
│  B4 → Part 4 chain    [block] → [block] → ... → [Mod Matrix]  │
│  B5 → Part 5 chain    [block] → [block] → ... → [Mod Matrix]  │
│  B6 → Part 6 chain    [block] → [block] → ... → [Mod Matrix]  │
│                                                                 │
│  MIX+B1 → CH1 strip   [Channel]→[MIDI]→[EQ]→[Sends]→[TBD]    │
│  MIX+B2 → CH2 strip   [Channel]→[MIDI]→[EQ]→[Sends]→[TBD]    │
│  MIX+B3 → CH3 strip   [Channel]→[MIDI]→[EQ]→[Sends]→[TBD]    │
│  MIX+B4 → CH4 strip   [Channel]→[MIDI]→[EQ]→[Sends]→[TBD]    │
│  MIX+B5 → CH5 strip   [Channel]→[MIDI]→[EQ]→[Sends]→[TBD]    │
│  MIX+B6 → CH6 strip   [Channel]→[MIDI]→[EQ]→[Sends]→[TBD]    │
│                                                                 │
│  MENU → System chain   [MIDI]→[Tuning]→[Theme]→[Patch]→[Upd]→[About] │
│                                                                 │
│  MIX+MENU → Chain Editor (active Part)                         │
│                                                                 │
│  Within any chain:                                              │
│    Minus/Plus = left/right through blocks                       │
│    Seq/Edit = up/down through sub-pages                         │
│    Encoders A-F = edit 6 params on current page                 │
│    MIX + Encoder = coarse snap                                  │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

---

## Block Count Summary

| Category | Blocks | Status |
|----------|--------|--------|
| DSP blocks (Part chains) | 10 | 9 defined, 1 placeholder (Tuned Resonator) |
| Mixer blocks (channel strips) | 5 | 4 defined, 1 placeholder (Insert) |
| System blocks | 6 | 3 defined, 3 placeholder (Patch, Updates, About) |
| Send/Master blocks | 5 | All placeholder |
| **Total unique block types** | **26** | **16 defined, 10 placeholder** |

---

## Placeholder Resolution Priorities

Blocks to define before building:

1. **Send effects + Master bus** — need to decide where they live
2. **Patch management** — save/load/copy workflow
3. **Tuned Resonator** — kick/percussion exciter block

Blocks that can wait:

4. Insert effects (mixer channel)
5. Send 4 (spare)
6. Updates page (firmware update flow)
7. About page (just credits)
