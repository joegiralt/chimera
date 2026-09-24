# Instrument Core Design

**Date:** 2026-09-24
**Status:** Draft, awaiting review
**Phase:** Desktop-first core instrument (decided 2026-09-24). Sub-project 1 of 3:
instrument audio core → MIDI input → patch persistence. Fixed chains (per
`ChainType`) stay; composable chains, the chain editor, the web builder and
all DSP quality work (e.g. #10) come later.
**Builds on:** `engine-pivot` (PR #9) — `Engines`, `ParamAddr`, per-block specs.
**ADR:** [0013 Hardware parity](../../adr/0013-hardware-parity-budgets.md).

## Intent

Turn the single-voice, single-track engine into a playable multitimbral
instrument in the desktop simulator, designed so that it fits the STM32H750
unchanged:

- a **Performance** of `MAX_PARTS` **Parts**, each holding a **Sound**, a MIDI
  channel, Mono/Poly mode, a DAC-pair output, level, pan and FX sends;
- a shared pool of `MAX_VOICES` voices, allocated Digitone-style;
- a mixer and the shared FX bus in the audio path;
- 3 stereo outputs internally;
- the chip's memory and CPU limits enforced on desktop (ADR 0013).

## Vocabulary

| Term | Meaning | Today's code |
|---|---|---|
| **Sound** | One instrument: chain type + `ParamSnapshot` + mod routes | `Patch` |
| **SoundPool** | Saved Sounds that can be loaded into a Part | `SoundPool` (32 slots) |
| **Part** | A slot playing one Sound, with channel, mode, output, level, pan, sends | `Track` |
| **Performance** | All Parts + FX: the whole setup you play and save | `Project` |
| **Voice** | One sounding note: engines + chain + amp env + LFO | `Voice` |

Renames: `Patch` → `Sound`, `Track` → `Part`, `Project` → `Performance`.
`MixerState` is folded into `Part` (level/pan/sends) and removed.

## Hardware parity (from ADR 0013)

New module `chimera-core/src/hw.rs` holds the target's limits as constants,
shared by both builds:

```rust
pub const SAMPLE_RATE: u32 = 48_000;          // re-exported from chimera-hal
pub const BLOCK_SIZE: usize = 64;             // re-exported from chimera-hal
pub const MAX_VOICES: usize = 6;
pub const MAX_PARTS: usize = 6;
pub const DAC_PAIRS: usize = 3;
pub const CPU_HZ: u32 = 480_000_000;
pub const CYCLES_PER_SAMPLE: u32 = CPU_HZ / SAMPLE_RATE;      // 10_000
pub const AUDIO_CYCLE_BUDGET: u32 = CYCLES_PER_SAMPLE * 70 / 100; // 30% left for UI/MIDI/ISR overhead

/// Memory regions the audio core may use, in bytes (STM32H750 map).
pub const AXI_SRAM: usize = 512 * 1024;   // D1: framebuffer, UI, Performance
pub const D2_SRAM: usize = 288 * 1024;    // D2 SRAM1+2+3: voices, DMA buffers
pub const DTCM: usize = 128 * 1024;       // tables, audio stack
```

- `const` assertions next to the types they guard fail the build on **both**
  targets when a layout stops fitting:
  - `size_of::<[Voice; MAX_VOICES]>() <= VOICE_RAM_BUDGET` (D2 minus DMA buffers)
  - `size_of::<Performance>() + size_of::<SoundPool>() + FB_BYTES + UI_RESERVE <= AXI_SRAM`
  - `size_of::<AudioShared>() * 2` fits its region
- `chimera-stm32/memory.x` maps the real D2 SRAM (288 KB, not 32 KB) and
  places `[Voice; MAX_VOICES]` there via `#[link_section]`.
- If `[Voice; 6]` does not fit (a `Voice` is ~68 KB today, mostly Modal's
  8 × `[f32; 2048]` string buffers → ~408 KB), the build fails and the fix is
  chosen in the plan, in this order of preference: (1) shrink Modal's string
  buffers to what its lowest supported note needs, (2) lower `MAX_VOICES`,
  (3) move Modal buffers to a shared pool. No `#[cfg]` escape for desktop.
- CPU: each engine declares `const COST: Cost` (cycles/sample/voice). Values
  are **estimates** until measured on hardware with the DWT cycle counter,
  and are marked `// estimate` until then. The allocator refuses a note whose
  engine cost would push the sounding total over `AUDIO_CYCLE_BUDGET`
  (refused note = dropped, counted in a debug counter). Desktop and firmware
  run the same allocator, so an over-budget combination fails identically.
- `just check` builds `chimera-stm32` for `thumbv7em-none-eabihf`; a firmware
  that does not link into its flash region fails the check. (Add the
  `justfile`; it is referenced in CLAUDE.md but missing.)

## Data model

```rust
pub struct Part {
    pub sound: Sound,
    pub loaded_from: Option<u8>,   // SoundPool slot (as Track today)
    pub channel: MidiChannel,      // 0..=15, newtype
    pub mode: PartMode,            // Mono | Poly
    pub output: DacPair,           // enum { P1, P2, P3 }
    pub level: f32,                // 0..1
    pub pan: f32,                  // -1..1
    pub sends: [f32; 3],           // chorus, delay, reverb
}

pub struct Performance {
    pub name: [u8; NAME_LEN],
    pub parts: [Part; MAX_PARTS],
    pub fx: FxParams,              // chorus + delay + reverb params (moved out of ParamSnapshot)
}
```

- Defaults: part *n* listens on channel *n* (0-based), Poly, output P1,
  level 0.8, pan 0, sends 0.
- `PartParams` (channel/mode/output/level/pan/sends) implement `Block` with
  specs, so they get UI pages, snap and (for level/pan) modulation for free.
- Chorus/delay/reverb move from `ParamSnapshot` to `Performance.fx`: they are
  shared, not per Sound. `ParamSnapshot` shrinks accordingly.
- `SoundPool` stays in the UI/Performance side; only the Performance state
  the audio needs crosses to the audio thread.

## Voice allocation

`Allocator` — pure logic, no DSP, `no_std`, fixed-size:

```rust
pub struct VoiceSlot { part: Option<u8>, note: Option<MidiNote>, age: u32, held: bool }
pub struct Allocator { slots: [VoiceSlot; MAX_VOICES], clock: u32, rr: usize }

pub enum Alloc { Voice(usize), Refused }
impl Allocator {
    pub fn note_on(&mut self, part: u8, mode: PartMode, note: MidiNote,
                   cost: Cost, sounding_cost: Cost) -> Alloc;
    pub fn note_off(&mut self, part: u8, note: MidiNote) -> Option<usize>;
    pub fn release_finished(&mut self, voice: usize); // voice went silent
}
```

Rules:
1. **Mono part:** owns exactly one voice while it has a sound. A new note
   retriggers that voice (legato/glide is a later Sound param). Mono voices
   are never stolen.
2. **Poly part:** take a free voice, round-robin from `rr`.
3. **Pool full:** steal the oldest (lowest `age`) non-mono voice, from any part. *(Superseded by ADR 0015: released voices are stolen before held ones.)*
   If every voice is mono, refuse.
4. **CPU budget:** if `sounding_cost + cost > AUDIO_CYCLE_BUDGET`, first try to
   steal as in rule 3; refuse if that doesn't free enough.
5. **Note-off:** marks the matching `(part, note)` voice released; the voice
   is free again when its engine reports inactive (existing
   `Engines::is_active` rules), so tails ring out.
6. A voice renders with **its part's** Sound snapshot and mod state.

## Audio path

```rust
pub struct Instrument {
    voices: [Voice; MAX_VOICES],     // placed in D2 on hardware
    alloc: Allocator,
    fx: FxBus,                       // chorus, delay, reverb (stereo)
}

impl Instrument {
    pub fn handle(&mut self, ev: NoteEvent, shared: &AudioShared);
    pub fn render(&mut self, out: &mut [[f32; BLOCK_SIZE * 2]; DAC_PAIRS], shared: &AudioShared);
}
```

Per 64-sample block:
1. Drain the note queue (see Threading) into `handle`.
2. For each voice with a part: `voice.render(mono_block, &parts[p].params, &parts[p].mod_state)`.
3. Pan (constant-power) and scale by part level into `out[part.output]`;
   accumulate `mono × send[i]` into the FX bus inputs.
4. Run the FX bus once; add its stereo return into DAC pair 1.
5. Free voices whose engines went inactive after note-off.

Desktop sums the 3 pairs to stereo for the speakers (and can solo a pair);
the STM32 writes each pair to its CS4344 later (port sub-project).

## Threading

- `AudioShared { parts: [PartAudio; MAX_PARTS], fx: FxParams }` where
  `PartAudio { params: ParamSnapshot, mod_state: ModState, mode, output, level, pan, sends }`.
  Double-buffered, one `AtomicPtr` swap per UI frame (generalizes today's
  desktop `AudioShared`). Both copies count toward the memory budget.
- Notes do not ride the snapshot: a fixed-capacity SPSC queue
  (`NoteQueue`, 64 events, lock-free, no allocation) carries
  `NoteEvent { channel, note, vel | off }` from the UI/input thread to audio.
  Full queue → event dropped and counted.
- Part routing by channel happens on the audio side at dequeue (the snapshot
  holds each part's channel), so a channel change applies atomically with the
  next snapshot.
- The STM32 keeps its raw-pointer params until the port sub-project; this
  design is what that port adopts.

## UI

- B1–B6 select the Part being edited (as tracks today).
- Mixer chain: a PART page per Part with channel, mode, output, level, pan,
  and a SENDS page — bound through `SlotBinding` to `PartParams`. This
  replaces the mis-bound Mixer pages found in the refactor review.
- Desktop keyboard piano plays the **selected** Part's channel.

## Testing

- **Allocator** (pure logic): round-robin order; steal-oldest across parts;
  mono never stolen; mono retrigger reuses its voice; note-off frees only the
  matching `(part, note)`; tails keep the voice until inactive; budget refusal;
  a seeded property test (random note on/off across parts and modes) asserting
  no slot ever has two notes, no held note is lost without a steal, and the
  pool invariants hold.
- **Budget:** `const` size assertions (compile-time); a test that
  `sum(COST of sounding voices) <= AUDIO_CYCLE_BUDGET` under the property test.
- **Goldens:** part 1 with one voice, other parts silent, FX sends 0 → the
  part's **mono bus before pan and level** (the sum of its voices) must match
  the existing goldens bit-for-bit through the new path. Pan/level are tested
  separately (constant-power law: centre = −3 dB per side, hard left/right =
  unity on one side).
- **New goldens:** 4-note Poly chord on one part; two parts on different DAC
  pairs; reverb send on vs off.
- **Queue:** SPSC full/empty/wrap; dropped-event counter.
- `just check`: core tests, desktop type-check, firmware build + link.

## Out of scope

- MIDI input from hardware/USB/desktop controllers (sub-project 2).
- Sound/Performance files and SD (sub-project 3).
- STM32 port of `Instrument`, `AudioShared`, DMA output to 3 DACs.
- Measuring engine CPU costs on hardware (estimates until then).
- Glide/legato, voice priority settings, per-part voice limits.
- Composable chains, chain editor, web builder, DSP quality (#10).
