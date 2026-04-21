# Chimera — Product Architecture

## 1. Product Definition

**Chimera is a multitimbral hardware synthesis module with per-part outputs, built for external sequencing.**

It is a compact, screen-driven sound module that accepts MIDI and produces audio across three stereo output pairs. It does not sequence. It does not try to be a groovebox. It is an instrument designed to be the sound engine for your sequencer — whether that's an Elektron box, a tracker, a DAW, or a modular rig.

**Who it is for:**

- Studio musicians who sequence externally and want a dedicated sound source with real outputs
- Live performers who pair hardware sequencers with sound modules
- Sound designers who want deep FM and physical modeling in a compact, standalone unit
- Producers who want a multitimbral rack module that replaces multiple mono synths

**What problem it solves:**

Most hardware synths are either (a) stereo-only instruments with deep synthesis but no output routing, or (b) grooveboxes that bundle mediocre sequencing with mediocre sound. Chimera takes the position that sequencing is a solved problem — Elektron, M8, Polyend, Dirtywave, your DAW — and that what's missing is a serious sound module designed from the ground up for multi-output, multitimbral operation with deep synthesis.

**What makes it distinct:**

1. **Three stereo output pairs from a single module.** This is the hardware truth. Three CS4344 DACs. Six channels of audio. Not decorative — architecturally central.
2. **Part-based routing.** Each Part (up to 4) owns a sound, a voice allocation policy, a MIDI channel, and an output assignment. You configure what you need.
3. **Two serious engines.** 4-operator FM (TX81Z lineage with modern ergonomics) and modal/physical modeling (string, resonator, bowed). Not toy oscillators.
4. **No sequencer, no compromise.** The entire DSP budget and UI surface goes to synthesis, effects, and routing. Nothing wasted on step sequencing or pattern management.

---

## 2. Core Use Cases

### 2.1 Polyphonic Synth Module

A single Part receives MIDI on one channel, allocates 6 voices polyphonically, outputs stereo on the main pair. This is the simplest and most common use: plug in MIDI, play chords.

**Why it matters:** This is the baseline. If this doesn't feel good, nothing else matters. The instrument must be a excellent poly synth before it's anything else.

**Architectural demands:** Voice allocator with steal policies (oldest, quietest). Fast note-on response (< 3ms). Clean voice stealing without clicks. Stereo panning per voice.

**UX implications:** Sound design must be fast. One Part, one sound, deep editing. This should feel like a dedicated synth, not a submenu of a multi-engine.

### 2.2 Multitimbral Module

2-4 Parts, each on a different MIDI channel, each with its own sound and output assignment. Part 1 plays pads on DAC1, Part 2 plays bass on DAC2, Part 3 plays leads on DAC3. One box replaces three synths.

**Why it matters:** This is the killer use case for anyone with a multi-track sequencer. One Chimera module gives you three independent synth voices routed to three separate mixer channels. This is where the 3-DAC architecture pays for itself.

**Architectural demands:** Per-Part voice pools or shared pool with priority. Independent parameter snapshots per Part. Per-Part MIDI channel filtering. Per-Part output routing.

**UX implications:** Switching between Parts must be instant (one button press). Each Part's sound must be editable without affecting others. The UI must clearly indicate which Part is selected.

### 2.3 Drum / Percussion Module

4-6 Parts, each monophonic, each triggered by a different MIDI note or note range. One Part per drum sound. Output routing gives individual outs for kick, snare, hats, etc.

**Why it matters:** FM synthesis is exceptional for percussion (the Digitone proved this). Physical modeling is exceptional for metallic and tuned percussion. Chimera's engines already cover both. The 3-DAC architecture means individual outputs for mixing — something most drum machines lack.

**Architectural demands:** Monophonic Parts with fixed voice allocation (one voice per Part, no stealing across Parts). MIDI note filtering per Part (e.g., Part 1 responds only to C1, Part 2 to D1). Fast envelope response. No long release tails consuming voices.

**UX implications:** Drum mode is not a separate mode — it's a multitimbral configuration where each Part is monophonic. The UI should make it easy to set up but shouldn't require a "drum mode" toggle. A drum kit is just a preset that configures 4-6 mono Parts with appropriate note mappings.

### 2.4 Layered / Split Performance Module

2 Parts sharing the same MIDI channel, playing simultaneously for layered sounds (FM pad + modal string attack). Or 2 Parts split by note range (bass below C3, lead above). Or binaural: Part 1 hard-left on DAC1, Part 2 hard-right on DAC2, slightly detuned.

**Why it matters:** Layering two different engines (FM + Modal) is something very few hardware synths can do. The dual-engine layer is a Chimera-specific capability that creates sounds no single-engine synth can produce.

**Architectural demands:** Two Parts sharing one MIDI channel with full voice allocation. Optional note-range splits. Per-Part output assignment for spatial separation.

**UX implications:** Layer configuration should be a routing feature, not a separate mode. The user assigns two Parts to the same MIDI channel and chooses output routing. Simple.

### 2.5 Studio Sound Module with Assignable Outs

All three stereo pairs used: main mix on DAC1, dry/effect split on DAC2/DAC3, or Part stems on separate pairs for DAW multitrack recording.

**Why it matters:** Studio users want stems. A synth module that can output separate Part audio for independent mixing and processing in a DAW is significantly more useful than one that sums everything to stereo.

**Architectural demands:** Flexible output routing matrix (Part → DAC assignment). Optional "main mix" bus that sums all Parts to DAC1 while still sending individual Parts to DAC2/DAC3.

**UX implications:** Output routing must be visible and editable without deep menu diving. A mixer page with clear DAC assignment per Part.

---

## 3. Voice + Output Architecture Options

### Hardware Truth

Before evaluating options, the hardware constraints:

- **3 × CS4344 DAC** via SAI: 3 stereo pairs, 6 channels total. This is fixed.
- **STM32H750 @ 480 MHz**: ~10,000 cycles per sample at 48 kHz.
- **FM voice cost**: ~610 cycles/sample. 6 voices = 36%, 8 voices = 49%.
- **Modal voice cost**: ~1,200 cycles/sample. 4 voices = 48%.
- **Effects budget**: Plate reverb ~200 cycles/sample, tape delay ~100, chorus ~80.
- **Memory**: 512K D1 SRAM + 288K D2 SRAM + 64K DTCM. Modal voices are ~40KB each (delay lines).

### Option A: 6 Voices, 3 Stereo Pairs (Recommended)

6 voices sharing a pool across up to 4 Parts. Each Part assignable to any of 3 stereo output pairs.

| Aspect | Analysis |
|---|---|
| **CPU** | 6 FM = 36%. Leaves 64% for effects, mixing, UI, MIDI. Comfortable. |
| **Memory** | 6 FM voices: ~3KB. 6 Modal: ~240KB (tight but fits in D2). Mixed: fine. |
| **Outputs** | 3 stereo pairs. Natural: 1 Part per DAC, or all to main + aux sends. |
| **Polyphony** | 6 voices is adequate for most synthesis. Not luxurious. Honest. |
| **Pros** | Best CPU headroom for effects. Clean hardware mapping. Enough voices for real music. |
| **Cons** | 6-voice poly feels limiting for dense pad work. |
| **Verdict** | **Best balance of polyphony, effects quality, and routing utility.** |

### Option B: 8 Voices, Main Stereo + 2 Aux Pairs

8 voices, all summed to main stereo on DAC1. DAC2/DAC3 used as effect sends or aux buses.

| Aspect | Analysis |
|---|---|
| **CPU** | 8 FM = 49%. Leaves 51% — enough for effects but tight. |
| **Memory** | 8 Modal voices = 320KB. Won't fit. Limits Modal to 4 voices. |
| **Outputs** | Main stereo only. Aux pairs are effect returns, not Part stems. |
| **Polyphony** | 8 voices is comfortable for poly. |
| **Pros** | Higher voice count. More conventional poly synth feel. |
| **Cons** | Wastes the 3-DAC architecture. Aux outputs are less useful than Part stems. Effects budget squeezed. |
| **Verdict** | **More voices, but sacrifices the thing that makes Chimera unique (individual outputs).** |

### Option C: 4 Voices, Heavy Per-Voice Processing

4 voices with generous DSP per voice. Room for per-voice effects or higher-quality engines.

| Aspect | Analysis |
|---|---|
| **CPU** | 4 Modal = 48%, or 4 FM = 24% leaving huge effects budget. |
| **Memory** | 4 Modal = 160KB. Comfortable. |
| **Outputs** | 3 stereo pairs. Could do 1 voice per DAC + spare, or 2+2 split. |
| **Polyphony** | 4 voices is limiting. Duophonic per Part in a 2-Part setup. |
| **Pros** | Best sound quality per voice. Most effects headroom. |
| **Cons** | 4-voice poly is genuinely restrictive. Feels like a boutique instrument, not a workhorse. |
| **Verdict** | **Interesting for a premium sound-design tool, but too limiting for a general-purpose module.** |

### Option D: Dynamic Voice Count by Engine

Voice count adapts to engine: 8 FM, 6 mixed, 4 Modal. Pool resizes when engine configuration changes.

| Aspect | Analysis |
|---|---|
| **CPU** | Maximizes utilization per engine type. |
| **Polyphony** | Best possible per engine. |
| **Pros** | Most flexible. Gets the most out of the hardware. |
| **Cons** | User can't predict voice count. "How many notes can I play?" depends on configuration. Hard to document, hard to test. Voice count changes when you switch engines mid-performance. |
| **Verdict** | **Technically optimal, but UX disaster. If pursued, it must be presented as fixed configurations the user selects, not dynamic runtime behavior.** |

### Recommendation

**Option A (6 voices, 3 stereo pairs) for MVP.** The 3-DAC output architecture is the hardware's defining feature. Wasting it on aux sends (Option B) misses the point. 6 voices with good effects is more musically useful than 8 voices with thin effects. Option D is a future upgrade, presented as selectable "performance profiles" (e.g., "6-voice FM", "4-voice Modal", "8-voice FM Lite") — not runtime dynamic allocation.

---

## 4. Chain Architecture

### Core Concept: Chains, Not Fixed Signal Paths

A **chain** is an ordered sequence of DSP blocks. Audio flows in one direction — a pipe, not a graph. No splits, no parallel paths, no feedback routing between blocks. Each block processes a buffer in-place and passes it to the next.

A **project** is a collection of chains and how they route to the mixer. That's it.

Different chains can have completely different compositions. An FM pad chain looks nothing like a kick drum chain:

```
FM Poly:    [FM Osc] → [Drive] → [Filter] → [Wavefolder] → [VCA]
Kick:       [Noise Exciter] → [Tuned Resonator] → [Low Pass Gate]
Pluck:      [Modal Resonator] → [Filter] → [VCA]
Pad:        [FM Osc] → [Filter] → [VCA]
Bass:       [FM Osc] → [Drive] → [Filter] → [VCA]
Snare:      [Noise] → [Tuned Resonator] → [Filter] → [VCA]
```

This is not a modular system exposed to the user. Factory chains ship as complete instruments with tasteful defaults. Most users never see or modify the chain structure — they just see pages of parameters. Power users can edit chains; everyone else doesn't know chains exist.

### Blocks

A block is a self-contained DSP unit with:
- A `process(buf: &mut [f32; BLOCK_SIZE])` method that modifies audio in-place
- Its own parameters (visible as a page in the UI)
- Its own modulation depth controls (env_amount, lfo_amount, velocity_amount, etc.)
- Its own output attenuation — every block controls its own output level

No separate VCA block is needed between stages unless the chain design explicitly includes one. Each block is responsible for its own output gain.

**Block vocabulary (MVP):**

| Block | Description | CPU (cycles/sample) |
|---|---|---|
| FM Osc | 4-operator FM synthesis, 8 algorithms | ~200 |
| Modal Resonator | Physical modeling (string, modal, bowed, sympathetic) | ~600 |
| Noise | White/pink/filtered noise source | ~10 |
| Drive | Pre-filter saturation, tanh soft clip with tone tilt | ~20 |
| Filter | 2-pole SVF, 8 modes, nonlinear feedback, self-oscillating | ~80 |
| Wavefolder | Triangle fold with symmetry and bias | ~40 |
| Low Pass Gate | Combined filter + VCA with vactrol-style response | ~60 |
| VCA | Amplitude envelope (ADSR) | ~50 |

Future blocks: VA oscillator, ring modulator, comb filter, bitcrusher, tuned resonator bank.

### Implementation

The chain is a fixed-size array of block enums. No heap allocation. No interpreter overhead.

```rust
enum Block {
    FmOsc(FmEngine),
    ModalResonator(ModalEngine),
    Noise(NoiseGen),
    Drive(Drive),
    Filter(SvfFilter),
    Wavefolder(Wavefolder),
    LowPassGate(Lpg),
    Vca(Vca),
}

struct Chain {
    blocks: [Option<Block>; MAX_BLOCKS],  // e.g., MAX_BLOCKS = 8
    count: usize,
}

fn render(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &ChainParams) {
    for block in &mut self.blocks[..self.count] {
        block.process(buf, params);
    }
}
```

The cost of this abstraction over a hardcoded chain is one enum match per block per render call — effectively zero. The DSP math inside each block is identical whether the chain is hardcoded or configurable.

### CPU Budget Reality

At 480 MHz / 48 kHz = ~10,000 cycles per sample:

```
6-voice FM Poly chain (FM Osc + Drive + Filter + Wavefolder + VCA):
  Per voice: 200 + 20 + 80 + 40 + 50 = 390 cycles
  × 6 voices = 2,340 cycles (23%)

6-voice FM Poly + 2 LFOs + 2 envelopes per voice:
  Modulators: 6 × (5 + 5 + 50 + 50) = 660 cycles (7%)

2 send effects (reverb + delay):
  300 cycles (3%)

Mixing + output conversion + overhead:
  ~500 cycles (5%)

Total: ~3,800 cycles (38%)
Remaining: 62% — comfortable headroom
```

A simpler chain (kick: 3 blocks, 1 voice) costs ~270 cycles total. A denser chain (6 blocks × 4 Modal voices) costs ~4,800 cycles (48%). The system has real room.

### Modulation in the Chain Architecture

Modulation has three layers, each with a clear job:

**Layer 1: Modulator definitions (what the modulators do)**

Each chain has a pool of modulators — LFOs, envelopes, velocity, aftertouch, mod wheel, note number, random. The number of modulators is determined by the chain template (factory chains have pre-set modulator counts tested within budget; custom chains can add modulators until CPU budget runs out).

Each modulator gets its own page in the UI:
- LFO page: Rate, Shape, Sync, Free/Triggered
- Envelope page: Attack, Decay, Sustain, Release, Velocity sensitivity

**Layer 2: Mod matrix grid (what's connected to what)**

The first page of the mod section is a grid showing all legal source → destination connections for this specific chain. If the chain has no wavefolder block, there's no wavefolder column. The grid is auto-generated from the chain's actual blocks.

```
              Filter  Drive  WaveFold  VCA   FM.Op1  FM.Op2
             Cutoff   Amt    Amount    Level  Level   Level
LFO 1       [  X  ] [     ] [      ] [     ] [     ] [     ]
LFO 2       [     ] [     ] [      ] [     ] [  X  ] [     ]
Env 1       [  X  ] [     ] [      ] [  X  ] [     ] [     ]
Env 2       [     ] [     ] [      ] [     ] [     ] [  X  ]
Velocity    [     ] [  X  ] [      ] [     ] [     ] [     ]
Mod Wheel   [  X  ] [     ] [      ] [     ] [     ] [     ]
```

Toggle cells on/off. The grid defines routing — which source connects to which destination.

**Layer 3: Depth controls (how much modulation affects each destination)**

The depth/amount for each modulation connection lives on the destination block's own page. When the user edits the Filter page, they see:

```
Cutoff    Resonance    Env Amt    LFO Amt    Key Track    Drive
```

Everything about the filter — including how much modulation affects it — is on one page. The user doesn't need to visit the mod matrix to understand what's modulating the filter. The page is complete.

This three-layer separation means:
- Block pages are self-contained (all parameters + modulation depths)
- The mod matrix grid is an overview/routing tool
- Modulator pages define modulator behavior independent of routing

### Factory Chain Templates

The product ships with 8-10 factory chains. These are pre-configured with tested block sequences, modulator assignments, and mod matrix routings. The user selects a chain template and sees familiar pages — not a block editor.

| Template | Blocks | Modulators | Character |
|---|---|---|---|
| **FM Poly** | FM Osc → Drive → Filter → Wavefolder → VCA | 2 LFO, 2 Env | Classic FM with analog-style shaping |
| **FM Keys** | FM Osc → Filter → VCA | 1 LFO, 2 Env | Clean electric piano / organ |
| **Modal Pluck** | Modal Resonator → Filter → VCA | 1 LFO, 1 Env | Plucked strings, metallic percussion |
| **Modal Bow** | Modal Resonator → Drive → VCA | 1 LFO, 1 Env | Bowed strings, drones |
| **Kick** | Noise Exciter → Tuned Resonator → Low Pass Gate | 1 Env | Analog-style kick drum |
| **Snare** | Noise → Filter → VCA | 2 Env | Tuned or noise snare |
| **Hat** | Noise → Filter → VCA | 1 Env | Hi-hats, cymbals |
| **Bass** | FM Osc → Drive → Filter → VCA | 1 LFO, 2 Env | Aggressive bass |
| **Pad** | FM Osc → Filter → VCA | 2 LFO, 1 Env | Slow-attack pads |
| **Lead** | FM Osc → Drive → Filter → Wavefolder → VCA | 2 LFO, 2 Env | Expressive mono lead |

### The UX Boundary

**What most users see:** Pages. A "Kick" preset has a SOUND page (exciter controls), a FILTER page (resonator tuning), an AMP page (decay shape). These feel like pages of a dedicated kick drum synth. The user doesn't know they're editing blocks in a chain.

**What power users can access:** The chain editor. Accessed via a deliberate action (e.g., hold Edit + press a specific button). Shows the block sequence, allows inserting, removing, and reordering blocks. This is the "dungeon map" — a visual representation of the chain that the user can modify.

**The Percussa lesson:** Percussa exposed the modular graph to everyone and it was overwhelming. Chimera hides the graph behind instrument-like pages. The chain editor exists but is never required. A user who never opens it has a fully functional, deep synthesizer.

### Per-Part Processing

```
[Voice Sum] → [Part Bus: Volume, Pan] → [Send 1 Level] → [Send 2 Level] → [Output Assignment]
```

Each Part sums its active voices into a stereo bus. The Part bus applies:
- **Volume**: Part-level gain (0 to +6 dB)
- **Pan**: Stereo position
- **Send levels**: How much of this Part goes to each send effect (0-100%)
- **Output assignment**: Which DAC pair this Part routes to

### Send Effects (Global)

```
Send 1: [Reverb]        → returns to Main Bus
Send 2: [Delay/Chorus]  → returns to Main Bus
```

Two global send effects, shared across all Parts. Each Part has independent send levels. Effect returns are summed into the main stereo bus (DAC1).

**Why sends, not inserts?** Insert effects per Part would cost 3× the CPU (one reverb per Part). Send effects cost 1× regardless of Part count. At our DSP budget, sends are the only honest option.

**Effect selection:**
- Send 1: Reverb (plate, FDN, or MidiVerb II — user selectable). All three algorithms already exist.
- Send 2: Switchable between tape delay and Juno chorus. Both already exist.

**Budget:** Plate reverb ~200 cycles, tape delay ~100 cycles, chorus ~80 cycles. Two sends = ~300 cycles max = 3% CPU. Affordable.

### Master Bus

```
[Part Buses + Effect Returns] → [Master Compressor] → [Limiter] → DAC1 (Main Stereo)
```

The master bus sums all Parts routed to the main output plus both send effect returns.

- **Compressor**: Optional, gentle bus compression.
- **Limiter**: Always on. Prevents clipping.

**DAC2 and DAC3** receive their assigned Part buses directly, bypassing master processing. Individual outputs should be clean for external mixing.

### Complete Signal Flow

```
                    ┌──────────────────────────────────────────┐
                    │  VOICE POOL (6 voices)                   │
                    │  ┌─────────────────────────────────────┐ │
                    │  │  Chain: [Block] → [Block] → [Block] │ │
                    │  │  (configurable per Part)             │ │
                    │  └─────────────────────────────────────┘ │
                    └──────────┬───────────────────────────────┘
                               │ (voices assigned to Parts)
              ┌────────────────┼────────────────┐
              ▼                ▼                 ▼
         ┌─────────┐    ┌─────────┐       ┌─────────┐
         │ Part 1  │    │ Part 2  │       │ Part 3  │
         │ Vol/Pan │    │ Vol/Pan │       │ Vol/Pan │
         │ Send 1  │    │ Send 1  │       │ Send 1  │
         │ Send 2  │    │ Send 2  │       │ Send 2  │
         └────┬────┘    └────┬────┘       └────┬────┘
              │              │                  │
              │   ┌──────────┘                  │
              │   │   ┌─────────────────────────┘
              ▼   ▼   ▼
         ┌──────────────┐
         │ Output Router │
         │ Part→DAC map  │
         └──┬─────┬────┬─┘
            │     │    │         ┌─────────┐
            │     │    │    ┌───►│ Send 1  │──┐
            │     │    │    │    │ Reverb   │  │
            ▼     ▼    ▼    │    └─────────┘  │
         ┌─────┐┌────┐┌────┐│   ┌─────────┐  │
         │DAC1 ││DAC2││DAC3││┌─►│ Send 2  │──┤
         │Main ││Aux1││Aux2│││  │ Dly/Chr │  │
         └──┬──┘└────┘└────┘││  └─────────┘  │
            │               ││                │
            │◄──────────────┘│                │
            │◄───────────────┘                │
            │◄────────────────────────────────┘
            ▼
    ┌───────────────┐
    │ Master Bus    │
    │ Comp → Limit  │
    └───────┬───────┘
            ▼
         DAC1 Out
```

---

## 5. Engine Strategy

### Engines Are Blocks, Not a Separate Layer

In the chain architecture, "engines" are just source blocks — the first block in the chain that generates audio. There is no separate engine layer or engine selection concept. The chain template determines which source block is used.

**Source blocks:**
- **FM Osc**: 4-operator FM synthesis, 8 algorithms, TX81Z waveforms
- **Modal Resonator**: Physical modeling with 4 resonator types
- **Noise**: White/pink/filtered noise (excitation source for percussion)
- **VA Osc** (future): Virtual analog with saw/pulse/triangle, sync, PWM

**Processing blocks:**
- **Drive**, **Filter**, **Wavefolder**, **Low Pass Gate**, **VCA**
- These are engine-agnostic. A filter is a filter regardless of what source precedes it.

### Why This Is Better Than Swappable Engines

The previous architecture had "engines" as a special first stage with a shared fixed chain after it. The chain architecture eliminates this distinction:

- A kick drum doesn't need a traditional "engine" — its source is a noise exciter, not an oscillator.
- A Modal pluck doesn't need a wavefolder — removing it saves CPU for more voices.
- An FM pad might want two filters in series — the chain allows it.

The engine concept was a constraint disguised as a feature. Chains are more honest: every sound is a sequence of blocks. Some sequences start with FM. Some start with noise. The architecture doesn't care.

### Chain Selection Is Per Part

Each Part has a chain. All voices in a Part use the same chain. This means:
- Part 1 can use the "FM Poly" chain (5 blocks, 4 voices)
- Part 2 can use the "Kick" chain (3 blocks, 1 voice, monophonic)
- Both share the voice pool, output routing, and send effects

Changing a Part's chain is equivalent to loading a different instrument. The UI presents it as selecting a sound type, not as "configuring a DSP graph."

---

## 6. UI/UX Architecture

### Hardware Constraints

- **Display**: ILI9341 240×320 TFT (portrait orientation, taller than wide)
- **Encoders**: 6 parameter encoders + 1 main encoder
- **Buttons**: 6 parameter buttons + 6 navigation buttons (Menu, Minus, Plus, Mix, Edit, Seq)
- **No velocity-sensitive keys, no pads, no faders**

This is a screen-driven instrument. The screen does all the heavy lifting. The encoders provide direct manipulation. The buttons provide navigation and mode switching.

### Design Principles

**From Digitone II:**
- **Part-centric workflow.** The instrument is organized around Parts, not voices or global settings. You select a Part, edit its sound, and everything is scoped to that Part.
- **Clear sound/filter/amp/fx page separation.** Each domain of sound design gets its own page with dedicated parameters.
- **Performance + sound design are separate workflows.** You don't accidentally edit a patch while performing.

**From M8 Tracker:**
- **Screen density.** Show maximum useful information per screen. No wasted space. No decorative elements. Every pixel earns its place.
- **Minimal button presses to any destination.** Deep features are reachable in 2-3 presses, never more.
- **Instrument-first mentality.** Each sound is a self-contained instrument definition. Changing instruments doesn't require navigating away from the current context.

**Chimera-specific:**
- **6 encoders = 6 parameters per page.** Every page shows exactly 6 editable parameters, one per encoder. This creates a consistent interaction pattern.
- **Parameter buttons = page selection.** The 6 parameter buttons (B1-B6) select pages within the current context.
- **Nav buttons = context switching.** Menu, Mix, Edit navigate between top-level modes.

### Page Structure — Chain-Driven

Pages are generated from the chain. Each block in the chain becomes a page. The mod matrix is always the last page. The user sees instrument pages, not DSP blocks.

**Example: "FM Poly" chain (FM Osc → Drive → Filter → Wavefolder → VCA)**
```
[PART SELECT]  ← Main encoder selects active Part (1-4)
     │
     ├── (B1) [FM OSC]      Operator config: algorithm, ratios, levels, waveforms
     ├── (B2) [DRIVE]       Drive amount, tone, mix, env amt, lfo amt
     ├── (B3) [FILTER]      Cutoff, resonance, mode, env amt, lfo amt, key track
     ├── (B4) [WAVEFOLD]    Fold amount, symmetry, mix, env amt, lfo amt
     ├── (B5) [VCA]         Volume, pan, env ADSR, velocity sensitivity
     └── (B6) [MOD]         Mod matrix grid (page 1) + modulator settings (pages 2+)
```

**Example: "Kick" chain (Noise Exciter → Tuned Resonator → Low Pass Gate)**
```
     ├── (B1) [EXCITER]     Noise type, pitch sweep, sweep time
     ├── (B2) [RESONATOR]   Tuning, decay, tone
     ├── (B3) [LPG]         Cutoff, response, decay
     ├── (B4) [MOD]         Mod matrix grid + modulators
     ├── (B5) —             (unused — fewer blocks = fewer pages)
     └── (B6) —
```

The page labels change depending on the chain. A user on the "Kick" preset sees Exciter / Resonator / LPG / Mod. A user on "FM Poly" sees FM Osc / Drive / Filter / Wavefold / VCA / Mod. Each page is complete — all parameters for that block, including modulation depth controls.

**Global pages** (accessed via nav buttons):
```
[MIX]    ← Mixer: Part levels, pans, output assignments, send levels
[EDIT]   ← Patch management: save, load, copy, init
[MENU]   ← System: MIDI config, tuning, calibration, about
```

### Navigation Model

1. **Main encoder** always selects the active Part (top-level context).
2. **B1-B6** select block pages within the chain (auto-mapped to the chain's blocks + mod matrix).
3. **Param encoders (A-F)** edit the 6 parameters shown on the current page.
4. **Minus/Plus** scroll sub-pages when a block has more than 6 parameters (e.g., FM operator editing has 4 operators).
5. **Mix button** jumps to the mixer view.

### Multi-Part Workflow

Switching Parts is one turn of the main encoder. The selected Part is always visible at the top of the screen. All B1-B6 pages are scoped to the selected Part — and the page labels update to reflect that Part's chain.

- Turn main encoder to Part 1 (FM Poly) → B1 shows "FM OSC"
- Turn main encoder to Part 2 (Kick) → B1 shows "EXCITER"

No mode switching. No "enter Part edit mode." Just select and edit.

### Block Pages Are Complete

Each block page shows everything about that block. For a Filter block:

```
┌─────────────────────────┐
│ Part 1 > FILTER         │
│                         │
│ A: Cutoff        72     │
│ B: Resonance     45     │
│ C: Mode          LP4    │
│ D: Env Amount   +64     │  ← modulation depth: how much Env 1 affects cutoff
│ E: LFO Amount   +20     │  ← modulation depth: how much LFO 1 affects cutoff
│ F: Key Track     50     │
└─────────────────────────┘
```

The user never needs to visit the mod matrix to understand what's modulating the filter. The depth controls are right here. The mod matrix exists as an overview and routing tool — it shows which sources are connected to which destinations — but the amounts are set on the block pages.

### Mod Matrix Page

Always the last page in the chain. Two sub-page levels:

**Sub-page 1: Routing grid.** A grid of all legal source → destination connections for this chain. Toggle cells on/off. The grid auto-generates from the chain's blocks — if there's no wavefolder in the chain, there's no wavefolder column.

**Sub-pages 2+: Modulator settings.** One sub-page per modulator (LFO 1, LFO 2, Env 1, Env 2, etc.). Each shows the modulator's own parameters — rate, shape, ADSR, sync — independent of where it's routed.

### Mixer Page (MIX Button)

```
┌─────────────────────────┐
│ MIXER                   │
│                         │
│ Part 1 [FM Poly] ████░ L│
│   Out: DAC1  S1:40 S2:0 │
│                         │
│ Part 2 [Kick]  ███░░░ C │
│   Out: DAC2  S1:0  S2:0 │
│                         │
│ Part 3 [Pluck] █████░ R │
│   Out: DAC3  S1:60 S2:30│
│                         │
│ Master: ████████░░  -2dB│
└─────────────────────────┘
```

6 encoders: Part 1/2/3 volume, Master volume, selected Part send 1, selected Part send 2.

### Chain Editor (Power Users Only)

Accessed via a deliberate action (e.g., hold Edit + Menu). Shows the dungeon map — a visual representation of the block sequence. The user can:
- Insert a block at any position
- Remove a block
- Reorder blocks (move up/down)
- See CPU budget remaining

Most users never open this. The factory chain templates + parameter editing covers 95% of use cases.

---

## 7. MVP Recommendation

Be ruthless. Here's what ships first.

### MVP Scope

| Aspect | MVP Specification |
|---|---|
| **Voice count** | 6 |
| **Parts** | Up to 4 (configurable 1-4) |
| **Block vocabulary** | FM Osc, Modal Resonator, Noise, Drive, Filter, Wavefolder, Low Pass Gate, VCA |
| **Factory chains** | 8-10 templates (FM Poly, FM Keys, Modal Pluck, Kick, Snare, Hat, Bass, Pad, Lead) |
| **Effects** | 1 send effect: Reverb (plate algorithm only) |
| **Outputs** | 3 stereo DAC pairs with per-Part assignment |
| **MIDI** | USART1 hardware MIDI in. 1 channel per Part. Note on/off, CC, pitch bend. |
| **Modulation** | 1 LFO + velocity per Part. 4 mod slots. |
| **Polyphony** | Shared pool, per-Part allocation (poly, mono, legato) |
| **Patches** | Save/load to SD card. 128 patch slots. |
| **UI** | 6 pages (Play, Sound, Filter, Amp, Mod, FX) + Mixer + Menu |

### What Gets Cut for V1

- **USB MIDI**: USART only for MVP. USB adds driver complexity.
- **Second send effect**: One reverb is enough. Delay/chorus in v1.1.
- **Per-Part EQ/compression**: Too much CPU. Global master limiter only.
- **Drum mode presets**: Drums work (monophonic Parts with note mapping), but no dedicated drum UI or factory drum kits.
- **Advanced modulation**: No envelope followers, no audio-rate mod, no mod matrix beyond 4 slots.
- **VA engine**: FM + Modal is enough. VA (virtual analog) is v1.1+.
- **Binaural/layer presets**: Layering works (two Parts, same MIDI channel), but no dedicated layer UI.
- **Performance macros**: No macro knob assignments in v1.

### Why This MVP is Compelling

1. **It's a 6-voice, 3-output multitimbral FM/Modal synth.** That sentence alone is a product.
2. **The 3-output architecture is immediately useful.** Plug in three cables, sequence three Parts from an Elektron, have three independent synth voices on your mixer.
3. **FM + Modal covers enormous sonic territory.** Classic DX-style electric pianos, metallic percussion, plucked strings, bowed drones, aggressive bass, lush pads.
4. **It works as both a poly synth and a multitimbral module out of the box.** No firmware update needed.

---

## 8. Product Roadmap

### v1.0 — MVP (as above)

Ship it. Get it into hands. Collect feedback.

### v1.1 — Effects + Modulation

- Add second send effect: Tape delay (switchable with chorus)
- Expand mod matrix to 8 slots per Part
- Add LFO 2 per Part
- Add USB MIDI (device class, no drivers needed)
- Factory preset bank (64 patches covering key use cases)

### v1.2 — Drum Kit Support

- Drum kit configuration UI: assign note ranges to Parts, quick mono setup
- Factory drum kit presets (FM kicks, snares, hats, metallic percussion)
- Per-Part note mapping page
- Velocity curves per Part

### v1.3 — VA Engine + Performance

- Virtual analog engine: 2 oscillators with saw/pulse/triangle, sync, PWM, unison
- Performance macro page: 4 assignable macro knobs mapped to any parameter
- Scene morphing: save two parameter states, crossfade between them

### v2.0 — Advanced (Hardware Revision Possible)

- Master compressor/limiter with sidechain
- Audio-rate modulation (FM of filter cutoff, etc.)
- MPE support
- Per-Part arpeggiator (simple — not a sequencer)
- Expanded memory for longer delay lines and larger reverbs

### What About Dynamic Voice Count?

Defer to v1.3 or later. Present as "Performance Profiles":
- **Profile A:** 6-voice FM (default)
- **Profile B:** 4-voice Modal
- **Profile C:** 8-voice FM Lite (reduced effects)
- **Profile D:** 4 FM + 2 Modal (mixed)

User selects a profile. Voice count is fixed within the profile. No runtime surprises.

---

## 9. Risks and Failure Modes

### Risk 1: Output Architecture Becomes Decorative

**The danger:** We ship 3 DAC pairs but the UI makes output routing so buried that nobody uses it. Users just run stereo from DAC1 and ignore the rest.

**Mitigation:** Output routing must be front-and-center in the mixer page. The factory presets must include multitimbral configurations. The quick-start guide must show the 3-cable setup. If the outputs aren't useful by default, they don't exist.

### Risk 2: Effects Budget Collapses

**The danger:** Reverb + delay + chorus + per-voice processing + voice rendering exceeds the CPU budget, forcing ugly compromises (reduced quality, reduced voice count, dropped frames).

**Mitigation:** Budget conservatively. MVP has ONE send effect. Measure actual CPU usage on hardware before adding more. The effects that exist must sound good. One great reverb is better than three mediocre effects.

### Risk 3: Drum Mode Makes the Product Incoherent

**The danger:** Adding drum-specific UI, drum kits, and percussion presets makes Chimera feel like it doesn't know what it is. "Is it a synth? Is it a drum machine? Is it a groovebox?" Identity crisis.

**Mitigation:** Drums are a **configuration**, not a mode. The same Part/Voice/Output architecture that serves poly synthesis also serves drums. No "drum page." No "drum mode." Just monophonic Parts with note mapping. The product is always a synth module; drums are one thing it can do.

### Risk 4: Too Much Flexibility, Not Enough Identity

**The danger:** 4 Parts × 2 engines × 3 outputs × modulation matrix × effect routing = combinatorial explosion. The user opens the box and has no idea where to start.

**Mitigation:** Strong factory presets. Clear defaults. When you power on, it's a 6-voice FM poly synth on the main stereo output. One Part, one sound, immediately playable. Complexity is available but not required.

### Risk 5: Modal Engine Memory Pressure

**The danger:** Modal engine uses ~40KB per voice (Karplus-Strong delay lines). 6 Modal voices = 240KB. This fits in D2 SRAM but leaves little room for effects buffers (reverb plate needs ~100KB).

**Mitigation:** Cap Modal voice count at 4 in mixed configurations. Use the "Performance Profile" system to enforce: if you choose Modal, you get 4 voices. Memory allocation is known at profile selection time, not runtime.

### Risk 6: Parameter Passing Bottleneck

**The danger:** The UI thread and audio ISR need to share parameter data without locks. Double-buffered ParamSnapshot is the plan, but if ParamSnapshot grows too large (4 Parts × full param sets), the atomic swap becomes expensive or the snapshot doesn't fit in cache.

**Mitigation:** Keep ParamSnapshot compact. Use quantized parameters (u16 instead of f32 where possible). One snapshot per Part, not one global mega-snapshot. The audio ISR reads Part snapshots independently.

---

## 10. External Sequencer Integration

### The Problem

Chimera is a sound module. It has no sequencer. That means every note it plays, every parameter change, every patch recall comes from an external device over MIDI. If this integration is even slightly janky — wrong channel, missed CC, confusing setup — the product fails. The MIDI story must be seamless with every major sequencer: Elektron, Cirklon, M8, OP-XY, Polyend, DAWs.

### How Popular Sequencers Actually Work

**Elektron (Digitone II, Digitakt II, Syntakt):** Any track can become a MIDI track. Each MIDI track sends notes (4-voice polyphony), velocity, pitch bend, aftertouch, and 8-16 assignable CCs on a configurable channel. P-locking (per-step parameter automation) applies to CCs identically to internal parameters. Program change + bank select sent per pattern. Two LFOs per track can modulate CCs. This is the gold standard for hardware MIDI sequencing.

**Sequentix Cirklon:** Up to 64 tracks across 5 MIDI ports. "Instrument definitions" create named profiles with labeled CC mappings per synth. Four aux rows per track for per-step CC values. NRPN support. Program change + bank select per scene. The Cirklon user expects to build a detailed instrument profile once, then control everything by name.

**Dirtywave M8:** Tracker-style. Any of 8 tracks can be a MIDI instrument. 10 user-assignable CCs per instrument plus program change via tracker commands. Per-row CC changes are natural — every row in a phrase can have a different CC value. USB MIDI + DIN MIDI.

**Teenage Engineering OP-XY:** 8 aux tracks for external MIDI. 8 assignable CCs per track. Per-step values. Clock. Limited program change support. The multi-out port shares protocols (CV or MIDI, not both), so users often need a USB MIDI host adapter.

**Polyend Play:** 8 dedicated MIDI tracks, each with CC knob mapping, program change, bank select. Note: track mutes don't mute CCs — only notes. 24 PPQN external clock.

### Design Requirements

#### 10.1 Zero-Config Basics

When you plug a MIDI cable into Chimera and send notes on channel 1, it should make sound immediately. No menu diving. No channel configuration. Power on → receive notes → play.

**Default state:**
- Part 1 active, receiving on MIDI channel 1
- All voices allocated to Part 1
- Output on DAC1 (main stereo)
- Velocity-sensitive, pitch bend active (±2 semitones)

This is the "plug and play" experience. A user with any sequencer should hear sound within 10 seconds of connecting MIDI.

#### 10.2 Multitimbral Channel Assignment

Each Part has a MIDI channel (1-16). Configuration is on the Mixer page — visible, not buried.

```
Part 1: CH 1   [FM]     → DAC1
Part 2: CH 2   [Modal]  → DAC2
Part 3: CH 3   [FM]     → DAC3
Part 4: CH 10  [FM]     → DAC1  (drums on channel 10, summed to main)
```

**Elektron workflow:** User creates 3 MIDI tracks on the Digitone II, assigns channels 1, 2, 3. Powers on Chimera. Each Elektron MIDI track controls one Chimera Part. Each Part comes out a separate DAC. Done.

**Cirklon workflow:** User creates 3 instrument definitions for Chimera Part 1/2/3, assigns channels 1/2/3. Maps CCs to named parameters. Everything works via the Cirklon's instrument profile system.

**M8 workflow:** User creates 3 MIDI instruments on 3 tracks, channels 1/2/3. Uses tracker CC column for parameter automation.

#### 10.3 CC-to-Parameter Mapping

This is the critical integration point. Every sequencer automates via CCs. Chimera must respond to CCs predictably.

**Default CC Map (per Part, matches conventions):**

| CC | Parameter | Notes |
|---|---|---|
| 1 | Mod wheel (routed via mod matrix) | Universal standard |
| 5 | Portamento time | |
| 7 | Part volume | |
| 10 | Part pan | |
| 64 | Sustain pedal | ≥64 = on |
| 70 | Engine select | 0-42=FM, 43-84=Modal, 85-127=VA |
| 71 | Filter resonance | Standard (GM2/MPE) |
| 72 | Amp envelope release | Standard |
| 73 | Amp envelope attack | Standard |
| 74 | Filter cutoff | Standard (GM2/MPE) — most commonly automated CC |
| 75 | Amp envelope decay | |
| 76 | Wavefolder amount | Chimera-specific |
| 77 | Drive amount | Chimera-specific |
| 78 | Filter envelope amount | Chimera-specific |
| 85-90 | Engine-specific params | FM: op levels, ratios. Modal: excite, decay, brightness |
| 91 | Reverb send level | Standard |
| 93 | Chorus/delay send level | Standard |
| 102-109 | User-assignable slots | Mapped via menu to any parameter |

**Design rules:**
- CCs 71, 74 (filter cutoff/resonance) must always work. These are the two most commonly p-locked parameters on every sequencer.
- CCs 7, 10 (volume, pan) must always work. Universal.
- CCs 91, 93 (reverb/chorus send) must always work. Standard.
- 8 user-assignable CC slots (102-109) for Cirklon/M8 users who want custom mappings.
- All CC reception is 7-bit (0-127). 14-bit CC pairs (MSB+LSB) are not worth the complexity — no sequencer in our target market sends them.

**Smooth CC response:** CC changes must be interpolated (smoothed/slewed) to avoid zipper noise. A received CC value becomes a target; the parameter ramps to it over ~5ms. This is essential for p-lock-style per-step automation, which sends abrupt value changes.

#### 10.4 Program Change + Bank Select

Used by every sequencer for patch recall. Elektron sends program change per pattern. M8 via tracker PGM command. Cirklon per scene.

**Implementation:**
- Program Change (0-127) selects patch within current bank
- CC0 (Bank Select MSB) selects bank
- CC32 (Bank Select LSB) ignored for MVP (128 patches × 128 banks = 16K patches is enough)
- Part-specific: program change on Part 1's MIDI channel changes Part 1's patch only

**Timing:** Bank select (CC0) must be received before program change. This is the standard MIDI order. Buffer the bank select value and apply it when program change arrives.

**Response time:** Patch loading must complete within 50ms. No audible gap. Preload the next patch into an inactive buffer if possible.

#### 10.5 Pitch Bend

- Default range: ±2 semitones (standard)
- Configurable per Part: ±1 to ±12 semitones
- 14-bit resolution (pitch bend is always 14-bit per MIDI spec)
- Smooth interpolation — pitch bend must never step

#### 10.6 Clock and Transport

Chimera has no sequencer, so clock reception is minimal:

- **MIDI Clock:** Receive and use for LFO sync (LFO tempo can lock to incoming clock). Also useful for tempo-synced delay times in future versions.
- **Start/Stop/Continue:** Could be used to reset LFO phase on Start. Optional for MVP.
- **Chimera does not send clock.** It is a receiver only.

#### 10.7 MIDI Channel Modes

- **Omni mode (default at power-on for Part 1):** Responds to all channels. Good for quick testing — plug in anything and it works.
- **Poly mode (normal operation):** Each Part on its own channel.
- **MPE:** Deferred to v2.0. MPE requires per-note pitch bend and CC74 (slide), which conflicts with our per-Part parameter model. It's valuable but architecturally complex.

#### 10.8 MIDI Implementation Chart

Chimera must ship with a published MIDI implementation chart in the manual and on the website. This is what Cirklon users look at first. It's what M8 users reference when setting up CC mappings. It should list:

- Every received CC and its destination
- Program change behavior
- Pitch bend range
- Channel assignment per Part
- Clock reception behavior
- What is NOT supported (SysEx, NRPN, MPE, active sensing)

#### 10.9 The "First 5 Minutes" Test

For each target sequencer, this experience must work:

**Elektron test:**
1. Create MIDI track on Digitone II, set channel 1
2. Plug MIDI cable into Chimera
3. Enter notes on Elektron → hear sound immediately
4. P-lock CC74 (filter cutoff) on step 5 → hear filter sweep on that step
5. Set pattern program change → Chimera loads correct patch

**M8 test:**
1. Create MIDI instrument on M8, set channel 1
2. Connect USB MIDI (or DIN via adapter)
3. Enter notes in phrase → hear sound immediately
4. Add CC74 in effects column → hear filter change per row
5. Add PGM command → Chimera switches patch

**Cirklon test:**
1. Create instrument definition for Chimera, channel 1
2. Map CCs 74, 71, 91 to "Cutoff", "Resonance", "Reverb" knobs
3. Sequence notes → hear sound immediately
4. Automate CCs via aux rows → hear parameter changes per step

If any of these workflows require menu diving on Chimera, the design has failed.

---

## 11. Final Recommendation

### Best Product Shape

**A 6-voice multitimbral synthesis module with 3 stereo output pairs, dual engines (FM + Modal), and send effects.** Not a groovebox. Not a drum machine. A synth module that happens to be capable of drums, layers, and splits because of its multitimbral Part architecture.

### Best Architecture

- **6 voices** in a shared pool
- **Up to 4 Parts**, each with engine selection, voice allocation, MIDI channel, and output routing
- **Per-voice:** Engine → Drive → Filter → Wavefolder → VCA
- **Per-Part:** Volume, Pan, Send levels, Output assignment
- **Global:** 1-2 send effects (reverb, delay), master bus with limiter
- **3 stereo DAC pairs** with per-Part assignment
- **Double-buffered ParamSnapshot** per Part for lock-free audio/UI communication

### Best MVP

Ship with 6 voices, FM + Modal, 1 send effect (reverb), 3 output pairs, 4 Parts, hardware MIDI. Cut everything else. This is already a product nobody else makes: a compact multitimbral module with individual outputs and two serious synthesis engines.

### Biggest Thing to Avoid

**Do not add a sequencer.** Not even a simple one. Not even an arpeggiator in v1. The moment you add any note generation, the product identity fractures. "It sequences too, but not very well" is worse than "it doesn't sequence at all — bring your own." Chimera is a sound module. Sequencing is someone else's job. Protect this boundary ruthlessly.
