# ORBIT: Design

**Date:** 2026-09-30
**Status:** Draft; awaiting owner review
**Builds on:** main at 85d9ed3 (exclusive state merged: 8 voices, the sympathetic pool).
**Issue:** private issue #216
**Mockups:** an unpublished page (ORB, RING · RHYTHM, RING · VOICE, RING · NOTES, CHORDS, SCENES)
**Pure core:** branch `orbit-core` builds the `chimera-theory` crate and the pure `orbit` module test-first. This spec is the contract that branch implements; everything outside § 2 and § 3 lands on top of it.

## Intent

Chimera is a sound module; real sequencing stays external, over MIDI. ORBIT lets Chimera play itself while you shape sounds. It is a generative companion in the Marbles-into-Rings spirit, not a sequencer. You start it, it keeps playing while you edit Parts, the matrix and the envelopes, and it only ever sends notes into Parts.

A chord ring (key, scale, a progression over 1–8 bars) turns outside five rhythm rings. Each ring has its own step count per bar, so the rings run as polyrhythms. Each ring plays a Part, snaps its notes to the key or the current chord, and can drift on its own (MUTATE). Scenes hold whole setups and switch on the loop boundary. While ORBIT runs, notes you play on a keyboard snap too.

**Done when:**

- **Slice 1.** On the desktop, pressing PLAY on a fresh project gives five rings over a chord ring that sounds good within 10 seconds (the owner's milestone). RHYTHM, the CHRD page with custom scales, snapping and MUTATE work, the ORB page turns, the transport and navigation of § 7 work, and every Slice 1 test in § Tests passes.
- **Slice 2.** The VOICE and NOTES pages, STEP rings, the SCN page and scenes, note and chord capture, live-input snapping with STAY and RE-SNAP, and the ORBIT records in the project file. Every Slice 2 test passes.
- **Slice 3.** KIT targets, after #215.
- The firmware builds with and without the `orbit` feature. It stays off by default until the owner says it feels right; then one combined flash covers it.

## Principles (binding)

- **Functional core, imperative shell.** `chimera-theory` and `chimera-core::orbit` are pure: no hardware, no threads, no time source but their arguments. The shells (the audio callback, the UI pages, the desktop) are thin.
- **Types decide what's possible** (ADR 0012): `PitchClass`, `Degree`, `Quality`, `SnapTo`, `Scale`, `Fill`, `Pool`, `RingIdx`, `SceneIdx`, `Steps`, `HitMask` and the rest are enums or checked newtypes, so an out-of-range pitch, degree, ring, step, fill, pool or scene can't be built.
- **The audio thread never allocates or blocks.** ORBIT's per-block work is bounded: at most two events per ring per block (one note-off, one note-on), in a fixed array.
- **Parts can't tell ORBIT from a keyboard.** ORBIT's notes enter `Instrument::handle` like any MIDI note and take voices by the same rules.
- **Deterministic.** Every random choice comes from a seeded PRNG. The same scene, from the same Start, emits the same events, sample for sample, in tests and in desktop renders.
- **Never snap the UI** (CLAUDE.md). Values lerp as everywhere; the ORB sweep and dot flashes animate from the engine's reported position.

## What the code does today

| Area | Today | Where |
|---|---|---|
| Note path | Each note source owns a `NoteQueue` (SPSC, 64 events packed in `AtomicU32`); `NoteSources::drain` empties them in source order into `Instrument::handle`. | `chimera-core/src/note_queue.rs:94-190`, `:212-238`; ADR 0019 |
| MIDI in → Instrument | Firmware: DMA half interrupt drains, then renders one block. Desktop: the cpal callback drains once, then renders as many 64-sample blocks as the buffer needs. | `chimera-stm32/src/audio/engine.rs:92-94`; `chimera-desktop/src/audio.rs:91-96` |
| Routing | A note-on plays on every Part whose channel matches; a note-off releases every held voice started on that channel with that note. | `chimera-core/src/instrument.rs:396-458` (`:402`, `:449-455`) |
| MIDI clock | **Not parsed.** The parser drops every real-time byte (`>= 0xF8`), so Clock, Start, Continue and Stop never leave it. `MidiMessage` has no variants for them, and `NoteEvent::from_midi` drops CC and pitch bend. | `chimera-hal/src/midi.rs:31-33`; `chimera-hal/src/lib.rs:171-193`; `note_queue.rs:50-68` |
| Voice allocation | One pool of 8 voices shared by all Parts (`MAX_VOICES`). A Mono Part retriggers its voice; Poly takes a free voice round-robin, else a dying one, else steals the oldest released non-mono voice, then the oldest held; else refuses and counts it. | `chimera-core/src/voice_alloc.rs:1-21`, `:255-291`; `instrument.rs:407-410`; `hw.rs:9` |
| Refusals | Counted in `Allocator::refused`, shown nowhere yet. The AUDIO page shows queue drops. | `voice_alloc.rs:132-136`; `perf/load.rs:14-29` |
| Mid-note pitch | None: a voice's note is fixed from `note_on`; no retune or glide. | `chimera-core/src/dsp/voice.rs:320-382` |
| Buttons | `Controls` gives each button's level edge per frame (`Pressed`, `Held`, `Released`, `Up`). Nothing measures how long a button is held. | `chimera-hal/src/lib.rs:82-105` |
| SEQ and EDIT | SEQ `Pressed`: sub-page up. EDIT `Pressed`: sub-page down. In the browser SEQ saves. EDIT held + B<n> opens the browser. | `chimera-core/src/ui/chain.rs:116-127`; `ui/browser.rs:61`; `ui/mod.rs:533-548` |
| B1–B6, MIX, MENU | B<n>: Part n's chain; MIX + B<n>: its mixer (MIX + B6: Demo); MENU: System. | `chain.rs:75-99` |
| Chains and pages | `ChainId` names each chain; `ChainDef2` lists its `ChainBlock`s and sub-pages; `BlockDef` binds six cells. A UI-held block (THEME) binds cells with `ParamSlot::param(BlockRef::Theme, …)` and resolves through `UiBlocks`. | `chain.rs:6-12`; `ui/block_registry.rs:592-607`, `:646-655`, `:748-754`; `ui/mod.rs:1036-1058` |
| Header | Context, name, CPU %, and an accent dot while the instrument sounds. | `ui/components.rs:53-106` |
| Desktop | minifb window and keyboard; the same `UiState`, `Instrument` and `FxBus` as the chip; UI frame about every 33 ms. | `chimera-desktop/src/main.rs:60-123`, `controls.rs:36-72`, `audio.rs:77-116` |
| Storage | TLV records in A/B files, frozen block codes and idents, append-only (ADR 0045). Block codes 1–26 are taken; record tags 0x0001–0x0006. **Project files don't exist yet** (`FileKind` has Sound and System only); they are projects plan 2, whose spec reserves a tempo record. | `storage/codes.rs:11-78`; `storage/record.rs:14-48`; `storage/frame.rs:16-22`; projects spec § Format |
| Part params | CHANNEL … SEND_REVERB are ParamIds 0–7 of the PART block (code 25). | `chimera-core/src/part.rs:117-125` |

## 1. Architecture

```
MIDI in (DIN / USB / midir) ─┐                        UI thread
desktop keys ────────────────┤ NoteSources          ┌──────────────────────────┐
                             ▼ (notes + real-time)  │ ORB · RING · CHRD · SCN   │
                       ┌───────────────┐            │ scenes, live settings     │
 audio thread, per     │ OrbitRunner   │◀─AudioShared.orbit (settings, run, cue)─┤
 64-sample block:      │  clock ◀ Clock/Start/Stop  │                          │
                       │  capture ◀ live notes      │                          │
                       │  orbit::block() (pure)     ├─OrbitView (take-once)───▶│
                       └──────┬────────────────┘    └──────────────────────────┘
          live notes (Origin::Live) + ORBIT notes (Origin::Orbit)
                              ▼
                     Instrument::handle ─▶ voices ─▶ render
```

- **`chimera-theory`** (new crate, § 2): pitch classes, keys, scales, chords, snapping and naming. Pure, no_std, no dependencies.
- **`chimera-core::orbit`** (§ 3): the pure core. `euclid`, the pool walk, `mutate`, the clock's phase maths and `block()`, which turns (state, settings, span of time) into note events.
- **`OrbitRunner`** (§ 4): the audio-thread shell. It owns the engine's state, reads the clock input and the live notes, calls `orbit::block` once per block, and feeds everything to `Instrument::handle`.
- **UI** (§ 7): the ORB, RING, CHRD and SCN pages in `chimera-core::ui`, so the desktop shows the real 240×320 screens.
- **Storage** (§ 8): new block codes and records under ADR 0045's rules.

## 2. `chimera-theory`

A workspace crate beside `chimera-fat` and `chimera-waves`: `#![no_std]`, zero dependencies, const tables in flash (289 B, const-asserted ≤ 512 B), no heap. `chimera-core` depends on it. ORBIT, per-Part live snapping, KIT (#215) and possibly Modal's Sympathetic chords use it. It owns no disk codes; `chimera-core` implements `DiskCode` for its types. Built on `orbit-core` (c25126d, f107ff7, e725387).

Every value it hands out is valid by construction: a pitch class, a degree, a quality and a snap target are enums, and a scale always holds its root.

```rust
#[repr(u8)] pub enum PitchClass { C, Cs, D, Ds, E, F, Fs, G, Gs, A, As, B }  // + Interval wraps; − gives the upward Interval
pub struct Interval(u8);                  // mod 12; UNISON … MAJOR_SEVENTH
pub struct Note(u8);                      // 0..=127, C4 = 60; new -> Option, pc()

pub struct Scale(u16);                    // tonic-relative set; the root bit is always set
impl Scale {
    pub const MAJOR, MINOR /* natural */, DORIAN, PHRYGIAN, LYDIAN, MIXOLYDIAN, LOCRIAN,
              HARMONIC_MINOR, PENTATONIC_MAJOR, PENTATONIC_MINOR: Scale;   // ALL: [Scale; 10]
    pub const fn custom(mask: u16) -> Scale;       // forces the root on, drops bits above 11: total
    pub const fn toggle(self, i: Interval) -> Scale;   // the root is a no-op
    pub const fn contains(self, i: Interval) -> bool;
    pub const fn degree_count(self) -> usize;      // 1..=12
}
pub enum Degree { I, II, III, IV, V, VI, VII }
pub enum Stack { Triad, Seventh }
pub struct Key { pub tonic: PitchClass, pub scale: Scale }
impl Key {
    pub const fn chord(self, d: Degree, st: Stack) -> Chord;           // total
    pub const fn triad(self, d: Degree) -> Chord;
    pub const fn degree_label(self, d: Degree, st: Stack) -> DegreeLabel;
}
pub enum Quality { Maj, Min, Dim, Aug, Sus2, Sus4, Maj6, Min6, Dom7, Maj7, Min7,
                   HalfDim7, Dim7, MinMaj7, AugMaj7, Dom7Sus4 }          // ALL: 16; tones() derived
pub struct Chord { root: PitchClass, quality: Quality }                 // tones derived, never stored
impl Chord {
    pub const fn new(root: PitchClass, quality: Quality) -> Chord;
    pub fn tones(self) -> impl ExactSizeIterator<Item = PitchClass>;
    pub const fn contains(self, pc: PitchClass) -> bool;
    pub fn from_notes(notes: &[Note]) -> Option<Chord>;  // root = lowest note; None under 2 classes
}
pub enum SnapTo { Scale(Key), Chord(Chord) }
pub const fn snap(note: Note, to: SnapTo) -> Note;       // total; ties go down (§ 5.1)
```

Names come through `Display`, with no heap: `Note` ("A3", "C#4"), `PitchClass`, `Chord` ("Am", "E7", "Bm7b5") and `DegreeLabel` ("iv", "V7", "iio", "III+"). There is no unnamed chord, so no "?".

Compile-time checks: every named scale holds its root and has 5 or 7 notes; every named 7-note scale's degrees stack to an exact quality, triads and sevenths; every quality has its root and 3 or 4 tones, and no two share a set.

### 2.1 Custom scales (owner, 2026-09-30)

A scale is a `Scale(u16)`: a set of pitch classes counted from the tonic that always holds the root (`Scale::custom` sets bit 0 and `toggle` never clears it). The ten named scales are values of it; CUSTOM is any other set the user picks. Snapping, degree-to-chord and naming don't know the difference.

- **Degrees** are the scale's own notes, ascending from the root: degree n is the set's n-th note. `Degree` stops at VII; on a set of fewer notes a degree wraps round the set (degree VI of a 5-note set is its I).
- **A degree's chord** stacks every other note of the set: the degree's note and the notes two and four places up (and six for a seventh), wrapping an octave. For a 7-note set this is stacking thirds within the set; for fewer notes it is the owner's every-other-note rule; a set of more than 7 uses the same rule.
- The stack is then named as the nearest `Quality` (§ 2.2, choice 1).
- Microtuning (Scala `.scl`/`.kbm`, the MIDI Tuning Standard) is not this: it is private issue #217.

### 2.2 Choices made on `orbit-core`, adopted here

1. **Nearest quality.** `Chord::from_notes` and `Key::chord` name a set as the nearest `Quality`: 2 points per shared tone, −1 per missing or extra tone, ties to table order. **For the owner:** see § Conflicts.
2. Custom and pentatonic sets stack every other note, as the owner ruled: A minor pentatonic gives A7sus4, C6 and D7sus4 on I–III.
3. Names are ASCII, sharps only: "dim"/"o" for diminished, "m7b5"/"7b5" for half-diminished.
4. Octaves are named with C4 = 60.
5. RANDOM draws uniformly over the whole walk (pool × (RANGE + 1)); a note RANGE pushes past 127 folds down an octave at a time.
6. PING-PONG over a walk of one note stays put.
7. MUTATE's swap takes a random note **of the key** in `[C(OCTAVE), C(OCTAVE + RANGE + 1))`, capped at 127, stored unsnapped.
8. A nudge picks uniformly among the hits with a free neighbour, then uniformly between its free sides.
9. On every hit, the CHANCE draw comes first, then the walk's; CHANCE fires when `below(100) < p`.
10. A ring's `Rng` is xorshift32 seeded by SplitMix32 of `seed + (ring + 1) · 0x9E3779B9`.
11. `Ring::new` defaults: CHANCE 100 %, MUTATE 0, GATE 50 %, VEL 100, ACCENT 0, RANGE 0, SNAP SCALE, POOL, UP, a one-note pool at OCTAVE's C.
12. A STEP ring plays `notes[i % steps]` as stored; OCTAVE transposes the stored notes when turned (§ 3.4), not at play time.

## 3. The pure core: `chimera-core::orbit`

Built on `orbit-core` (1a53721, a6d3ac8, 48f0a9e): the fill, the pool, the walk, the ring's `play`, `mutate` and the dice. `Scene`, `ChordRing`, `harmony`, `segment_at` and `block` are not built yet; § 3.1's scene types and § 3.6 are this spec's.

### 3.1 Types

Built:

```rust
pub const RINGS: usize = 5; pub const MAX_STEPS: u8 = 32; pub const MAX_POOL: usize = 8;
pub struct RingIdx(u8);                   // 0..5
pub struct Steps(u8);                     // 1..=32 (the RHYTHM page's STEPS bar is drawn over 32)
pub struct Fill { .. }                    // STEPS, HITS ≤ STEPS, ROTATE mod STEPS: new -> Option
pub struct HitMask { bits: u32, steps: Steps }   // is_hit, hit_index, moved(from, to) -> Option
pub struct Pct(u8);                       // 0..=100
pub struct Rng(u32);                      // xorshift32; for_ring(seed, RingIdx)
pub struct Pool { .. }                    // 1..=8 notes, kept sorted
pub enum Walk { Up, Down, Random, PingPong }
pub struct Octave(u8);                    // 0..=8; low() = MIDI 12·(n+1)
pub enum Range { Zero, One, Two, Three }
pub enum SnapTo { Scale, Chord }          // the ring's setting; resolved to theory's SnapTo by Harmony
pub enum NoteMode { Step, Pool }
pub struct Harmony { pub key: Key, pub chord: Chord }
pub struct Ring {
    fill: Fill, hits_mask: HitMask,       // private, kept in step: with_fill re-lays the mask
    pub chance: Pct, pub mutate: Pct, pub mute: bool,
    pub gate: Pct, pub velocity: Velocity, pub accent: u8,
    pub octave: Octave, pub range: Range, pub snap: SnapTo,
    pub mode: NoteMode, pub walk: Walk,
    pub notes: [Note; 32],                // STEP: one per step
    pub pool: Pool,                       // POOL
}
pub enum Mutation { Swap { index: usize, note: Note }, Nudge { from: usize, to: usize } }
pub struct NoteOut { pub note: Note, pub velocity: Velocity, pub gate_fraction: Pct, pub accent: bool }
pub const ACCENT_BOOST: u8 = 30;          // the VOICE mockup's "+30 VEL"
```

This spec's (not built):

```rust
pub const SCENES: usize = 8;
pub const MAX_SEGS: usize = 8;
pub struct Scene { pub chords: ChordRing, pub rings: [SceneRing; RINGS], pub seed: u32 }
pub struct SceneRing {                    // the shell's view of a ring
    pub ring: Ring,
    pub target: Target,
    pub rhythm_rev: u8, pub notes_rev: u8,// bumped by the UI when it re-lays either
}
pub struct ChordRing {
    pub tonic: PitchClass,                // KEY
    pub scale: ScaleKind,                 // SCALE: one of the 10 named, or Custom
    pub custom: Scale,                    // the CUSTOM set, kept while a named scale is picked
    pub bars: Bars,                       // 1..=8: the loop
    pub segs: Segs,                       // 1..=8, split evenly over the loop
    pub degrees: [Degree; MAX_SEGS],
    pub played: [Option<Chord>; MAX_SEGS],// a played chord overrides the degree
}
pub enum ScaleKind { Major, Minor, Dorian, Phrygian, Lydian, Mixolydian, Locrian,
                      HarmonicMinor, PentatonicMajor, PentatonicMinor, Custom }  // scale(custom) -> Scale
pub enum Target { Part(PartId) /* Kit(PartId, KitKey): slice 3 */ }
```

Editing STEPS, HITS or ROTATE builds a new `Fill`, `Ring::with_fill` re-lays the mask, and `rhythm_rev` bumps, so any nudges MUTATE made are replaced. Capturing, picking or clearing notes bumps `notes_rev`.

### 3.2 Pure functions

| Function | Does | Built |
|---|---|---|
| `euclid(steps, hits, rotate) -> HitMask` | Bjorklund's even spread, rotated so step i plays step i + ROTATE (the mockup formula's direction). Total: HITS clamps, ROTATE wraps. E(3,8) = `x..x..x.`, E(5,8) = `x.xx.xx.`. | yes |
| `accents(mask, accent) -> HitMask` | The ACCENT fill: `euclid(k, accent, 0)` laid over the ring's k hits in order. | yes |
| `walk_next(ring, &mut WalkState, &mut Rng) -> Note` | The next pool note: § 3.4. | yes |
| `Ring::play(i, &mut WalkState, &Harmony, &mut Rng) -> Option<NoteOut>` | One step: § 3.3. | yes |
| `mutate(ring, &Harmony, &mut Rng) -> Option<Mutation>`, `Ring::apply` | At a loop boundary: § 3.5. | yes |
| `ChordRing::key() -> Key` | The tonic with the named scale or `custom`, by SCALE. | no |
| `segment_at(phase, segs) -> usize` | The chord segment at a loop phase. | no |
| `harmony(chords, seg) -> Harmony` | `{ key, chord: played[seg].unwrap_or(key.triad(degrees[seg])) }`; a degree past a small set wraps (§ 2.1). | no |
| `block(&mut OrbitState, &Scene, Span, &ClockOut) -> Events` | One block's events: § 3.6. | no |

One `Rng` per ring (§ 2.2, choice 10), reseeded from `(scene.seed, ring)` at Start and at a scene switch. A separate stream per ring means editing or muting ring 2 never changes ring 4's dice.

### 3.3 A step's note: `Ring::play`

On a hit, in order: the CHANCE draw; then the note, the step's own `notes[i % steps]` in STEP or `walk_next` in POOL. The draw and the walk happen whether or not the note sounds, so the pool's phase against the hits holds (a 3-note pool on 5 hits comes round in 15, as the NOTES mockup says). A miss or a muted ring returns `None`. Otherwise the note snaps to the ring's SNAP, resolved through `Harmony` to theory's `SnapTo::Scale(key)` or `SnapTo::Chord(chord)` (§ 5), and takes VEL, or `min(VEL + ACCENT_BOOST, 127)` on an accented hit.

### 3.4 Register and the walk

- A note is stored as a MIDI note. **OCTAVE** (0–8; octave n starts at MIDI 12·(n+1)) is the octave of the ring's lowest note. Turning OCTAVE transposes the stored notes by whole octaves; capturing notes sets it to the lowest captured note's octave.
- **RANGE** (0–3 octaves): a pool walk runs over the sorted pool repeated RANGE more octaves up, arpeggiator style. RANGE 0 is the pool as it is. Mutation picks new notes in `[C(OCTAVE), C(OCTAVE + RANGE + 1))`.
- **UP** climbs the walk, **DOWN** descends it, **PING-PONG** climbs then descends without repeating the ends, **RANDOM** picks uniformly over the whole walk.
- Walk positions reset at Start and at a scene switch, never at a loop boundary, so pools phase freely across loops. A walk survives its pool shrinking.
- The NOTES page's "COMES ROUND IN" figure is `lcm(pool.len() · (RANGE + 1), hits)`: 15 at RANGE 0 and 30 at RANGE 1 for the mockup's pool of 3 on 5 hits. The mockup pairs 15 with RANGE 1 OCT; the page follows this rule.

### 3.5 MUTATE

At each loop boundary, for each ring with MUTATE > 0, one draw: with probability MUTATE the ring changes once, else not at all. A second draw picks the kind, half and half:

- **Swap a note.** A POOL ring replaces one pool note (the pool re-sorts), a STEP ring one step's note, with a random note of the key in the ring's register (§ 2.2, choice 7). It is stored unsnapped and snaps when it plays, so it follows the chord.
- **Nudge a hit.** One hit with a free neighbour moves one step into it, wrapping (`HitMask::moved`). The hit count stays. A full ring has nowhere to nudge.

MUTATE at 0 draws nothing: the ring is locked, and back at 0 it keeps what it became. A scene's first loop after it is adopted is not mutated at that boundary: it plays once as saved.

### 3.6 `block`

Per block, for each ring in order, `block` finds the step boundaries in the block's window (§ 6.2). For each, the ring's pending note-off fires first, then `Ring::play`; a `NoteOut` becomes a note-on on the target Part's MIDI channel, and its note-off is scheduled `gate_fraction` × the step's length later.

Each block's events go out note-offs first, then note-ons, in ring order. `Events` is a fixed array of 2 × `RINGS` entries.

## 4. The audio shell: `OrbitRunner`

`OrbitRunner` lives beside the `Instrument` in the audio shells: the firmware's `Engine` (`chimera-stm32/src/audio/engine.rs`) behind the `orbit` feature, and `DesktopAudio`'s callback. Per block:

1. **Drain.** The note queues drain as today. Each event goes to the runner first: real-time messages (§ 6.1) go to the clock, and live notes go to any armed capture (§ 5.3), then on to `Instrument::handle` with `Origin::Live`.
2. **Settings.** Read `AudioShared.orbit` (§ 4.1).
3. **Clock.** Advance one block (§ 6).
4. **Engine.** `orbit::block`; each event goes to `Instrument::handle` with `Origin::Orbit`.
5. **Harmony.** Hand the current `Harmony` to the `Instrument` for live snapping (`None` while ORBIT is stopped), and re-snap held notes on a change (§ 5.2).
6. **Report.** Publish `OrbitView` (§ 4.2).
7. `Instrument::render`, as today.

On the desktop the ORBIT steps (2–7) run inside the per-block branch (`audio.rs:94-96`), not once per callback, so a block is always 64 samples of ORBIT time.

### 4.1 Settings flow (UI → audio)

`AudioShared` gains `orbit: OrbitShared { run: bool, bpm: Bpm, scene: SceneIdx, scene_rev: u16, settings: Scene, capture: Capture }`, published every frame through the existing triple buffer (ADR 0021), as the Parts are.

The engine keeps its own **working copy** of the playing scene, because MUTATE drifts it. Each block:

- If `scene_rev` is unchanged: the knob fields are copied from `settings`. A ring's `hits_mask` is copied only when its `rhythm_rev` changed, and its notes only when `notes_rev` changed; otherwise the working copy's drift stands.
- If `scene_rev` changed, a scene is queued (a switch, a load or a project load): the working copy is kept and `settings` ignored until the next loop boundary (at once if stopped). Then the whole scene is adopted, drift and all, the walks reset and the `Rng`s reseed.

Edits made while a switch is queued go to the queued scene, and apply when it lands.

### 4.2 Report (audio → UI)

`OrbitView`, through a take-once triple buffer (ADR 0021), as the scope and `AudioStats` are: running, clock source and measured BPM, loop phase, bar and beat counters, the playing scene and any queued one, the current segment and its chord, and per ring the current step, a fired counter (the dots flash on its change), and the working `hits_mask`, `notes` and `pool`. The pages draw the drift from here, and SAVE copies it.

Capture results come back here too: a counter and the captured notes (§ 5.3).

## 5. Snapping

### 5.1 The rule

`snap(note, to: SnapTo)`: a note whose pitch class `to` contains is unchanged. Otherwise it moves to the nearest note whose class it contains; **a tie goes down** (`chimera_theory::TIE`). A result outside 0..=127 takes the nearest member on the other side. `to` is `SnapTo::Scale(harmony.key)` for SNAP SCALE and `SnapTo::Chord(harmony.chord)` for SNAP CHORD; a chord's tones are derived from its root and quality.

ORBIT notes snap once, in the ring, to the ring's SNAP. A note already sounding keeps its pitch until its gate ends.

### 5.2 Live input

- The PART block gains **SNAP** (OFF, SCALE, CHORD; default OFF) and **HELD** (STAY, RE-SNAP; default STAY). OFF is the neutral default that reproduces every existing Part (ADR 0045).
- While ORBIT runs, `Instrument::handle` snaps each live note-on per Part, for each Part on the event's channel, to that Part's SNAP. While ORBIT is stopped, or with SNAP OFF, nothing snaps. KIT Parts (slice 3) never snap.
- **Note-off by origin and key.** The `Instrument` records, for each voice, the note-on's channel (as today), the key that started it (the raw live note, or ORBIT's note) and its `Origin`. A note-off releases only voices whose three match. So a live key-up releases its snapped note, and a ring's gate never releases a held key on the same note, nor a key-up an ORBIT note.
- **Chord changes.** At each segment boundary, and when a queued scene lands, a held live voice on a Part with HELD RE-SNAP glides to `snap(raw key, new target)`, over `RESNAP_GLIDE_MS` = 40 ms (a constant, not a knob). It snaps from the raw key, not from where it last was, so repeated changes never drift. HELD STAY keeps the pitch until the key is released. The glide needs a new `Voice::retune(note, glide)` on the voice's pitch path (ADR 0042), built in Slice 2.

### 5.3 Capturing notes and chords

- Arming: the NOTES page's LEARN cell arms capture into the selected ring; the CHRD page's PLAY cell arms it into the selected segment. The armed target rides in `OrbitShared.capture`.
- The runner watches every live note-on, from any source and channel. A capture takes the notes pressed from the first key-down until every key is up: at most 8 distinct notes, extra ones dropped. The notes still play through to the Parts as usual.
- **Chord:** `Chord::from_notes`, root = the lowest note. Fewer than two pitch classes is ignored, and capture stays armed.
- **POOL ring:** the notes, sorted, become the pool; OCTAVE becomes the lowest note's octave.
- **STEP ring:** notes fill the ring's hit steps in the order played, one per note-on, and capture ends when every hit is filled. Each step that is not a hit takes the note of the hit before it.
- The result goes back through `OrbitView`; the UI writes it into its settings, bumps `notes_rev` (a ring) and disarms.

## 6. Clock and time

### 6.1 Sources

- **Parser.** `MidiMessage` gains `Clock`, `Start`, `Continue` and `Stop`. `MidiParser::feed` returns them for 0xF8, 0xFA, 0xFB and 0xFC instead of dropping them (`midi.rs:31-33`). Real-time bytes still never disturb running status, and the other real-time bytes (0xFE, 0xFF) are still dropped.
- **Queue.** Real-time events ride the same per-source `NoteQueue`, packed with a kind field above the note bits, so they keep their order with the notes (ADR 0019). `NoteEvent::from_midi` becomes `InEvent::from_midi`. Song Position Pointer is not parsed.
- **Which source leads.** The clock follows the first source that sends a Clock or Start. Another source's real-time events are ignored until the leader has been silent for 500 ms.
- **Internal.** With no tick for 500 ms the clock falls back to the internal BPM (default 120). The fallback doesn't change the transport: a stopped ORBIT stays stopped.
- Chimera never sends clock.

### 6.2 Tempo resolution

- **A bar is four beats, 96 MIDI clocks.** MIDI clock carries no meter; 4/4 is what every DAW sends by default.
- **Position** is `(bar: u32, phase: u32)`, where `phase` is the fraction of a bar in units of 2⁻³². A ring with `n` steps is on step `⌊phase · n / 2³²⌋` (a `u64` multiply), so any count from 1 to 32 lands exactly, with no tie to 16ths or to 24 PPQN.
- **Internal clock:** a `u64` sample count since Start; `phase` is computed from it and the BPM each block, not accumulated, so it never drifts.
- **External clock:** each tick marks `phase = tick_in_bar / 96`. Between ticks the phase advances at the measured rate, a moving average of the last 24 tick intervals measured at block resolution. It never runs past the next tick's phase, and a late tick moves it forward, never back.
- **Block-accurate, centred.** Each block fires the steps whose time falls in the window `[t − 32, t + 32)` samples around its start `t`, so every note lands within ±32 samples (±0.67 ms) of its time. Voices start notes only at block boundaries (`Instrument::handle` runs before `render`: `instrument.rs:396`, `:608`). Sample accuracy would thread a start offset through every engine for ≤ 1.33 ms, below what a DIN note's own 0.96 ms transmission already smears, and inside what ADR 0019 already accepts as inaudible.
- **Gates** are counted in samples from the step's time, and fire in the block whose window holds them.

### 6.3 Transport and the loop

- **PLAY (B6, or MIDI Start)** starts from the loop's start: bar 0, phase 0, the walks reset, the `Rng`s reseeded from the scene's seed. MIDI Continue resumes where the clock stopped.
- **B6 under an external clock** toggles ORBIT's own run flag. ORBIT sounds only while it runs and the external transport plays. Its position always follows the external clock, so pressing PLAY joins in phase.
- **STOP (B6, or MIDI Stop)** sends a note-off, in the same block, for every note ORBIT holds (at most one per ring), and nothing else. Their release tails ring out. Live notes are untouched.
- **The loop** is BARS bars of the playing scene, counted from Start. Under an external clock, bar 0 is the last Start.
- **At a loop boundary,** in order: a queued scene is adopted (§ 4.1); otherwise MUTATE runs for the new loop (§ 3.5). Then the boundary's steps fire.
- **Scene switch.** It lands on the next loop boundary of the playing scene, at once when stopped. The old scene's sounding notes end on their own gates, or when their ring next fires, whichever comes first. There is no morph.

## 7. UI

### 7.1 The ORBIT chain

`ChainId::Orbit` joins `ChainId`, with its `ChainDef2` in the block registry and in `ALL_CHAINS`:

```
ORB ── RING ── CHRD ── SCN
        │        │
      RHYTHM   CUSTOM (while SCALE is CUSTOM)
      VOICE
      NOTES
```

- **ORB** (home, BigViz): the rings animated, as mocked. The outer band is the progression with the current segment lit; inside, the five rings, a dot per step, filled for a hit, an orange ring for an accent, and a sweep hand for the clock. Dots flash as they fire; a muted ring is dark. The header reads `SCENE n ORBIT` over `KEY SCALE · n BAR · BPM SOURCE`.
- **RING · RHYTHM:** STEPS, HITS, ROTATE, CHANCE, MUTATE, MUTE; the pattern unrolled; the focus band shows `HITS/STEPS`.
- **RING · VOICE:** GATE, VEL, ACCENT, OCTAVE, RANGE, SNAP; gates drawn as bars, accents taller.
- **RING · NOTES:** MODE, WALK, TARGET, LEARN, CLEAR; the pool as chips (or one note per step), and `POOL p · HITS h · COMES ROUND IN n`.
- **CHRD:** KEY, SCALE, BARS, SEGS, DEGREE, PLAY; the segments as chips, a played chord shown by name with a dot. SCALE offers MAJOR, MINOR and CUSTOM.
- **CHRD › CUSTOM** (a sub-page, shown while SCALE is CUSTOM): a 12-dot circle, one dot per pitch class from the tonic, lit when in the scale; the root's dot is locked on. Turning SCALE to CUSTOM opens it. Encoder A moves the cursor round the circle; a turn of encoder B, or EDIT, toggles the note under it.
- **SCN:** SCENE, SAVE, SWITCH; eight slots, the playing one outlined, a queued one filling as the loop runs out.

The ring cells bind through a new `SlotBinding::SelectedRing(ParamId)`, as `SelectedOp` binds an operator's (`block_def.rs:37-59`). The chord ring and the rings are UI-held blocks resolved in `UiBlocks`, as THEME is (`ui/mod.rs:1036-1058`). The per-ring pages edit the *live* scene; the drift they draw comes from `OrbitView`.

### 7.2 Buttons (owner, 2026-09-30)

| Where | Input | Does |
|---|---|---|
| Anywhere but the browser | **hold SEQ** | Enter ORBIT. Inside ORBIT it goes to ORB, as a chain button pressed again goes home (design doc § Navigation Behavior). |
| Anywhere | tap SEQ | As today: sub-page up; in the browser, save. |
| ORBIT | B1–B5 | Pick ring 1–5 and open its RING page. |
| ORBIT | MIX + B1–B5 | Mute or unmute that ring. |
| ORBIT | B6 | PLAY / STOP (§ 6.3). |
| ORB | EDIT | Jump to the pages of the Part the selected ring plays (`ChainId::Part`). |
| RING | EDIT, tap SEQ | Down and up through RHYTHM, VOICE, NOTES, as elsewhere. |
| CHRD | EDIT, tap SEQ | Down to CUSTOM and back, while SCALE is CUSTOM. On CUSTOM, EDIT toggles the note under the cursor. |
| ORBIT | MENU | Leave, to System, as today. |
| ORBIT | − / + | Move along ORB · RING · CHRD · SCN, as elsewhere. |

- **Hold threshold: 500 ms.** A tap is a press released before 500 ms; a hold fires once when 500 ms is reached, before release. 500 ms is the usual long-press time and well above a deliberate tap (100–200 ms).
- **A tap acts on release.** SEQ's sub-page-up and the browser's save move from `Pressed` to the release of a tap, so a hold never also moves a sub-page or saves a sound. The delay is the tap's own length.
- A pure `ui::hold::HoldGate` takes each frame's `ButtonState` and elapsed milliseconds and returns `Tap`, `Hold` or nothing. `UiState::handle_input` gains the frame's milliseconds: the desktop already measures them (`main.rs:62`), and the firmware's UI loop does the same.
- **EDIT: jump or sub-page.** EDIT jumps to the Part only from the ORB page. On RING pages, and on CHRD while SCALE is CUSTOM, it moves down the sub-pages; on CHRD › CUSTOM it toggles a note; on CHRD with a named scale it does nothing; on SCN it commits SAVE (§ Design choices). ORB has no sub-pages, so the two meanings never meet.
- In ORBIT, EDIT acts on its press, so EDIT + B<n> (the browser) is not available there; the browser is reached from the Part pages as before. MIX + B6 keeps its job (Demo).
- **Leaving.** ORBIT keeps playing when you leave. Holding SEQ returns to the ORBIT page you left.

### 7.3 The header dot

While ORBIT runs, every page's header shows a small outlined ORBIT dot left of the sounding dot (`components.rs:97-105`). It fills on each beat and fades back over about 150 ms, lerped. The CPU label moves left by one dot. The dot's region is redrawn only when the beat counter in `OrbitView` changes or the fade moves.

## 8. Saving

Settings save with the project through the existing storage layer (ADR 0045). The project file itself is projects plan 2, not built yet. This spec defines ORBIT's codes and records, their encoder, decoder and golden tests. Plan 2 writes and reads them in the project file.

**Block codes** (append-only, `codes.rs`), with their frozen idents:

| Code | Ident | Block |
|---|---|---|
| 27 | `ORBIT` | `BlockRef::OrbitChords`: KEY, SCALE (MAJOR, MINOR, CUSTOM), BARS, SEGS |
| 28–32 | `RING1`–`RING5` | `BlockRef::OrbitRing(RingIdx)`: every scalar and enum field of `Ring` |

The PART block (25) gains **SNAP** (ParamId 8) and **HELD** (ParamId 9). `ScaleKind` (codes 0–10 in the enum's order, idents `MAJOR` … `PENT_MINOR`, `CUSTOM`), `PitchClass`, `Degree`, `Quality` (16), `Range`, `SnapTo`, `NoteMode`, `Walk`, `Target`'s kind and the PART block's SNAP and HELD get exhaustive `DiskCode` tables, pinned in `tests/fixtures/disk_codes_v1.txt`.

**Records** (all non-critical, so a reader without them still loads the Sounds; each well under `MAX_RECORD_LEN` = 512 B):

| Tag | Name | Payload |
|---|---|---|
| 0x0007 | `Tempo` | BPM as `f32` LE: the tempo record the projects spec reserves |
| 0x0008 | `OrbitBlock` | scene `u8`, then a `Block` payload (block code, (ParamId, value)*) |
| 0x0009 | `OrbitNotes` | scene `u8`, ring `u8`, `hits_mask` `u32` LE, pool length `u8` (1–8), the pool's notes, then 32 step notes; a reader rebuilds `Pool` and `HitMask` through their checked constructors and takes the default on a bad one |
| 0x000A | `OrbitChord` | scene `u8`, segment `u8`, degree code `u8`, played root code `u8` (0xFF: none), played `Quality` code `u8` |
| 0x000B | `Orbit` | current scene `u8`, then each scene's seed as `u32` LE (9 of them) |
| 0x000C | `OrbitScale` | scene `u8`, the CUSTOM set as `u16` LE (bit 0 the root; a reader sets it whatever the file says) |

- Scene 0–7 are the saved slots; **scene 8 is the live scene**, drift included, so a project reloads exactly where it was saved (projects spec, "comes back bit-identical").
- The transport is never saved: a loaded project is stopped.
- The model and codec compile without the `orbit` feature, so a firmware with ORBIT off still loads and re-saves ORBIT's records rather than dropping them.
- A decoded scene starts from the neutral scene (every ring muted, the chord ring on I), not from the init scene, whose tuning may change.

## 9. Voices

- ORBIT and live input share the one voice pool by the allocator's existing rules (`voice_alloc.rs:1-21`), with no priority and no reserved voices. A ring's short gates leave released tails, which the allocator steals first, so a held key survives a busy ring for as long as there are tails to take.
- A Mono Part played by both retriggers its one voice: the last note wins.
- A refused note (every voice mono, or the CPU budget) is dropped and counted in `Allocator::refused`, as any note is. Its pending note-off then finds no voice and does nothing.
- **Neither the ring nor any page shows a steal or a refusal.** They are the allocator's normal behaviour, and a per-hit mark would need a per-event report from the audio thread for something the ear already tells you.

## Memory

Estimates for `thumbv7em`; the plan measures them and asserts the engine's size.

| What | Where | Bytes |
|---|---|---|
| `SceneRing` (`Ring`: fill, mask, knobs, 32 step notes, `Pool`; target, 2 revs) | | ≈ 64 |
| `ChordRing` (tonic, scale, custom set, bars, segs, 8 degrees, 8 × `Option<Chord>`) | | ≈ 46 |
| `Scene` (chord ring, 5 rings, seed) | | ≈ 370 |
| **Engine** (`OrbitRunner`: working `Scene` 370; per ring `Rng`, walk, pending note-off ≈ 16 × 5; clock ≈ 40; capture ≈ 28) | audio shell | **≈ 520** |
| `Instrument`: each voice's key and `Origin` | D2 | 16 |
| `OrbitShared` in `TripleBuffer<AudioShared>` (≈ 380 × 3) | AXI | ≈ 1,140 |
| `TripleBuffer<OrbitView>` (≈ 224 × 3) | AXI | ≈ 672 |
| UI: 8 scenes, the live scene, nav and hold state | AXI | ≈ 3,350 |
| **Total** | | **≈ 5.7 KB** |

- The engine, all of ORBIT's audio-thread state, is about 520 B: well under 1 KB. `orbit-core` measures its part, `STATE_BYTES` (5 rings, walks, `Rng`s and a `Harmony`), at 356 B on the host and asserts it under 1,024. The runner asserts `size_of::<OrbitRunner>() <= 1024`.
- Most of the rest is the eight saved scenes, which are project data like the Sound pool. AXI had 88,892 B free at ADR 0045's measurement; ORBIT takes about 5.2 KB of it. Every new static is added to `AXI_RESIDENT` (`instrument.rs:35-45`).
- `chimera-theory`'s tables are a few hundred bytes of flash.

## CPU

Per block: a clock step (a `u64` multiply and divide), five step lookups (a `u64` multiply each), and on the rare block with an event, a walk, a snap (at most 6 steps each way over a 12-bit set) and one `Instrument::handle`. Estimated under 500 cycles a block, about 8 cycles a sample, against a budget of thousands. The bench gains an ORBIT row: five rings of 32 steps, all hits, MUTATE 100%, measured on the chip in the combined flash.

## Tests

| Test | Asserts |
|---|---|
| **Slice 1** | |
| `chimera-theory/tests/theory_test.rs` (23, built) | `note_is_checked`, `pitch_class_arithmetic_wraps`, `note_names_c4_is_60`, `scales`, `major_diatonic_triads_and_sevenths`, `harmonic_minor_degrees`, `heptatonic_degrees_name_exact_chords`, `numerals_follow_quality`, `chord_tones_are_derived`, `played_chords_map_to_the_nearest_quality` (root on the lowest note, `None` under two classes), `snap_to_scale`, `snap_to_chord`, `snap_is_pinned_to_brute_force` (every note × 12 roots × 10 scales, 16 qualities and 5 custom sets), `tables_stay_small`. |
| `scale_always_holds_its_root`, `custom_forces_the_root_and_drops_high_bits` (built) | `Scale::custom(0)` holds the root; `toggle` of the root is a no-op; bits above 11 are dropped; every other interval toggles. |
| `named_scales_are_scale_values` | The ten named scales are the expected masks; a CUSTOM set equal to MAJOR gives the same degrees, chords, names and snaps. |
| `pentatonic_stacks_every_other_note`, `custom_pentatonic_sized_set`, `custom_two_note_set`, `custom_chromatic_set` (built) | A 5-note set's degrees are its notes and wrap past V; its chords stack every other note and are named by the nearest quality (A minor pentatonic: A7sus4, C6, D7sus4); 2- and 12-note sets stay total. |
| `snap_to_a_custom_set` (built) | Snapping to a custom key, pinned against the brute-force search; the 12-note set never moves a note. |
| `chrd_custom_circle` | Turning SCALE to CUSTOM opens the circle; encoder A moves the cursor, encoder B and EDIT toggle, the root can't be cleared; screen golden. |
| `custom_scale_round_trips` | `OrbitScale` encodes and decodes every set; a file with bit 0 clear loads with the root set; SCALE's codes are pinned. |
| `snap_ties_go_down` (built) | D against C and E snaps to C. |
| `snap_stays_in_midi_range` (built) | A note whose nearest member is past 127 (or below 0) takes the one on the other side. |
| `known_euclidean_rhythms`, `every_fill_is_even`, `rotate_shifts_like_the_spec_formula`, `steps_past_the_bar_wrap`, `hit_index_counts_hits`, `ranges_are_types` (built) | Tresillo `x..x..x.`, cinquillo `x.xx.xx.`, 4/16, 5/12, 5/13, 0 and full; every n ≤ 32 and k is even; ROTATE's direction; `Fill::new` refuses HITS > STEPS. |
| `accents_ride_the_hits`, `accents_play_louder` (built) | ACCENT 2 over 5 hits marks hits 0 and 2 in order, whatever the mask's nudges; an accented hit plays `min(VEL + 30, 127)`. |
| `step_index_is_exact_for_any_count` | For every count 1–32, step boundaries from `phase` land on `k/n` of a bar to within one 2⁻³² unit, and a bar holds exactly n steps. |
| `internal_clock_never_drifts` | One hour at 97.3 BPM: bar `n` starts at sample `round(n · bar_len)` ± 0. |
| `steps_fire_within_half_a_block` | Every event's block start is within ±32 samples of its exact time, for 13 steps at 137 BPM over 100 bars. |
| `pool_is_one_to_eight_sorted_notes`, `walk_modes`, `range_walks_the_pool_up_octaves`, `pool_walks_phase`, `walk_survives_a_shrinking_pool` (built) | `Pool::new` refuses 0 or 9 notes and sorts; UP, DOWN, PING-PONG (no repeated ends), RANDOM over the whole walk; a 3-note pool on 5 hits repeats every 15 hits, 30 at RANGE 1. |
| `the_walk_advances_on_a_missed_hit`, `chance_full_always_fires_and_zero_never`, `step_rings_play_each_steps_note`, `notes_snap_to_the_ring_target` (built) | The walk and the draw run on every hit; CHANCE 100 % always and 0 % never; a STEP ring plays each step's note; `play` snaps to the key or the chord. |
| `mute_sounds_nothing_and_keeps_the_dice` (built); `mute_changes_nothing_else` | A muted ring draws and walks as unmuted; two runs, ring 2 muted in one, give identical events on the other rings, and unmuting ring 2 gives it the unmuted run's notes from that step. |
| `rings_have_their_own_dice` | Changing ring 2's CHANCE leaves ring 4's events unchanged. |
| `mutate_zero_locks` (built) | 10,000 calls at MUTATE 0 draw nothing; 100 loops leave the ring unchanged. |
| `mutate_keeps_count_and_register`, `a_full_ring_has_nowhere_to_nudge` (built) | 1,000 loops in POOL and STEP, OCTAVE 0, 3 and 8, RANGE 0, 1 and 3: the hit count holds, every swapped note is in the key and the register; a full ring never nudges. |
| `mutate_back_to_zero_freezes`, `a_fixed_seed_replays_exactly`, `rng_is_seeded`, `rng_below_and_chance`, `state_fits_in_a_kilobyte` (built) | Back at 0 the ring keeps what it became; a seed replays exactly; `STATE_BYTES < 1024`. |
| `gate_off_precedes_the_next_on` | GATE 100% on a ring whose notes repeat: each block orders the off before the on, and no voice is released by the wrong event. |
| `orbit_events_reach_parts_like_midi` | An ORBIT note-on for Part 2 renders bit-identically to the same `NoteEvent` from a keyboard. |
| `orbit_off_never_releases_a_held_key` | A live key held on note 60, ring on Part 1 playing 60: the ring's note-offs leave the live voice held. |
| `orbit_shares_voices` | A held live chord of 4 and five rings on one Poly Part: ORBIT's tails are stolen before held keys; with no tails left, the oldest held note goes, as the allocator's rule 3 says. |
| `stop_releases_only_orbit` | STOP with 3 ORBIT notes and 2 live keys held: 3 note-offs in that block; the live voices stay held. |
| `play_starts_at_the_loop_start` | PLAY after a STOP mid-loop: the first events are the loop's step 0s, identical to the first PLAY. |
| `scene_replays_identically` | The same scene from Start, twice (MUTATE 60%, CHANCE 70%, RANDOM walks): every event, block and velocity equal for 64 loops. |
| `desktop_render_is_repeatable` | The desktop's `Instrument` + `OrbitRunner` rendered offline twice from one project: the DAC blocks are bit-identical. |
| `hold_gate_*` (pure) | A 499 ms press is a Tap on release; 500 ms fires Hold once, and its release is nothing; bounces under a frame are one press. |
| `seq_hold_enters_orbit` / `seq_tap_moves_up` | Hold SEQ from a Part page lands on ORB; a tap moves one sub-page up on release and never enters ORBIT; a tap in the browser saves once. |
| `orbit_buttons` | In ORBIT: B3 opens RING 3; MIX + B4 toggles ring 4's mute; B6 toggles run; EDIT on ORB jumps to the target Part's chain; EDIT on RING moves down a sub-page; MENU opens System and ORBIT keeps running. |
| `orb_page_golden` and `ring_rhythm_golden` | Screen goldens of ORB and RHYTHM at a fixed clock phase. |
| `header_dot_pulses` | The dot is filled on a beat's first frame and back to outline by 150 ms, lerped; absent when stopped. |
| `sounds_good_in_ten_seconds` (listening, not automatic) | The init scene, played on the desktop, with the owner. |
| **Slice 2** | |
| `midi_parser_real_time` | 0xF8, 0xFA, 0xFB, 0xFC come out as `Clock`, `Start`, `Continue`, `Stop`, between a note's data bytes too, without breaking running status; 0xFE is dropped. |
| `clock_locks_to_midi` | 24-PPQN ticks at 120 BPM with ±1 ms jitter: steps land within ±1 block of the ideal; the phase never runs past the next tick nor moves back. |
| `clock_falls_back_after_500_ms` | Ticks stop: after 500 ms the internal BPM drives the clock, and the transport is unchanged. |
| `one_clock_leads` | Ticks from two sources: only the first source's move the clock until it is silent 500 ms. |
| `scene_switch_lands_on_the_loop` | A switch queued mid-loop: the old scene plays to the boundary, the new one's step 0s fire at it, its first loop is unmutated; stopped, it lands at once. |
| `edits_while_queued_go_to_the_queued_scene` | An edit between the cue and the boundary applies from the boundary and does not change the old scene's last loop. |
| `save_captures_the_drift` | MUTATE for 20 loops, SAVE into slot 5, switch away and back: slot 5 plays the drifted mask and notes. |
| `live_snaps_per_part` | Two Parts on one channel, SNAP CHORD and OFF: the first sounds the snapped note, the second the raw one; a key-up releases both. |
| `live_does_not_snap_when_stopped` | ORBIT stopped: SNAP CHORD Parts play raw notes. |
| `held_stay_and_resnap` | A chord change under two held keys: HELD STAY keeps the pitch; RE-SNAP glides over 40 ms to `snap(raw key, new chord)`; five changes in a row never drift from the raw key. |
| `capture_*` | A chord pressed over 300 ms and released becomes the segment's override, rooted on its lowest note; a single note is ignored and capture stays armed; a 9th note is dropped; a POOL capture sets OCTAVE; a STEP capture fills the hits in order; the notes still sound during capture. |
| `voice_and_notes_pages` / `chrd_and_scn_pages` | Screen goldens; every cell's range and format. |
| `orbit_codes_pinned` | Block codes 27–32, idents, the new PART ParamIds and every ORBIT enum's codes are in `disk_codes_v1.txt`. |
| `orbit_records_round_trip` | Every scene and the live scene encode and decode bit-identically, drift and seeds included; each record is under 512 B. |
| `orbit_off_keeps_orbit_records` | A build without `orbit` loads and re-saves a project with ORBIT records unchanged. |
| `part_snap_default_is_neutral` | A PART block without SNAP and HELD decodes to OFF and STAY. |
| **Every slice** | |
| Size assert | `STATE_BYTES < 1024` (built) and `OrbitRunner <= 1024` B, printed in `memory_budget_test`; `AXI_RESIDENT` still fits. |
| Existing suites | Goldens, instrument, allocator, codec and page tests pass unchanged; the SEQ tests move from press to tap. |

## Slices and plan order

Each task leaves the tree green. ADRs are written where the decision lands, at the next free numbers.

**Slice 1** (the owner's milestone)

1. `chimera-theory` (on `orbit-core`): the crate, `Scale(u16)` with the named and custom scales, degrees and chords, `snap`, naming. ADR: theory is a shared crate.
2. The pure `orbit` core (on `orbit-core`): settings types, `euclid`, `accents`, the walk, `mutate`, the clock's phase maths, `block`, and their tests.
3. `ui::hold::HoldGate`; SEQ tap on release; SEQ hold enters ORBIT; `ChainId::Orbit` with ORB, RING · RHYTHM, and CHRD (KEY, SCALE with CUSTOM and its 12-dot circle, BARS, SEGS, DEGREE). ADR: SEQ hold enters ORBIT; the hold threshold.
   The custom set's `OrbitScale` record and SCALE's codes land here with their tests, so the codec for the rest follows in task 9.
4. `OrbitRunner` on the desktop: the internal clock, B6 transport, `Origin` in `Instrument::handle`, the header dot. The init scene is tuned with the owner until it sounds good in 10 seconds, then pinned by an event golden. ADR: ORBIT runs on the audio thread, block-accurate, the engine owning the drift.

**Slice 2**

5. Real-time MIDI through the parser and the queues; the MIDI clock and its fallback.
6. RING · VOICE and RING · NOTES; STEP rings; capture of notes and chords.
7. Scenes: SCN, SAVE, switching on the loop boundary.
8. Live snapping: PART SNAP and HELD; `Voice::retune` for RE-SNAP.
9. The ORBIT codes, records and codec tests. Project plan 2 wires them into the project file.
10. Firmware: the runner behind `orbit`; the bench's ORBIT row; the memory figures. Built, not flashed, until the owner says it feels right.

**Slice 3**

11. KIT targets (`Target::Kit`), unsnapped, after #215.

## Interactions with in-flight work

- **`orbit-core`** builds tasks 1 and 2. This spec is its contract; names may change there, behaviour may not.
- **Projects plan 2** (project files) writes and reads § 8's records. It takes the tempo record defined here.
- **#214 / #215** (DRUM and KIT). ORBIT comes first (owner, 2026-09-30); slice 3 waits on #215.
- **#207** (per-Part VOICES) would cap a Part's share of the pool; ORBIT uses whatever cap exists and adds none.
- **`Instrument::handle`** gains an `Origin` argument. The bench (`chimera-stm32/src/bench.rs:566`, `:605-609`) and the test rigs pass `Origin::Live`.

## Out of scope

- Per-step probability: floated, not agreed.
- Named scales beyond the ten built (a CUSTOM set covers any other), and progression chords beyond triads (`Stack::Seventh` exists in theory; no page offers it). Filed as a follow-up when the owner asks.
- Microtuning: Scala `.scl`/`.kbm` files and the MIDI Tuning Standard, private issue #217.
- Morphing between scenes (the owner chose no morph).
- Sending MIDI clock, and Song Position Pointer.
- Marbles-style macro knobs per ring: considered and not chosen by the owner.
- KIT targets until #215.

## Decisions made (owner, 2026-09-30)

- ORBIT is a generative companion in the Marbles-into-Rings spirit, not a sequencer. Real sequencing stays external over MIDI. You start it while doing sound design and it plays in the background while you edit Parts, the matrix and the envelopes.
- **Chord ring (outer):** a key, a scale and a progression of degrees, whose qualities come from the scale, split evenly over a loop of 1–8 bars. A chord played in over MIDI overrides a segment, and snapping then uses its tones.
- **Five rhythm rings inside,** each with its own count of evenly spaced steps per bar (3, 5, 13 …), not tied to 16ths, so they run as polyrhythms.
- **Rhythm per ring:** a Euclidean fill: STEPS, HITS spread evenly, ROTATE; with CHANCE (probability) on top, and MUTE.
- **Notes per ring:** played in, picked by hand, or random. A ring is a STEP ring (one note per step) or a POOL ring (1–8 notes walked UP, DOWN, RANDOM or PING-PONG; pool length ≠ step count, so it phases).
- **Snapping:** a note outside the scale or chord moves up or down to the nearest note that fits. Per ring, the target is the scale (free melody) or the current chord's tones (follows the progression).
- **MUTATE per ring,** Turing Machine style: 0 locks the ring; higher values give each loop a growing chance to swap a pool note or nudge a hit, staying in key through snapping; back to 0 freezes what it became.
- **Expression:** GATE and VELOCITY per ring, plus an ACCENT fill: a second Euclidean pattern over the ring's hits. Accented hits play louder and drive the Part's velocity routes.
- **Register:** OCTAVE (base register) and RANGE (0–3 octaves, for pool walks and mutation) per ring; snapping keeps it in key.
- **Targets, per ring:** a Part, or a key in a KIT Part (#215).
- **Clock:** internal BPM when nothing is connected; locks to incoming MIDI clock and start/stop when present. Chimera does not send clock.
- **Scenes:** about eight per project, each the whole setup (chords, rings, notes, mutation state). SAVE captures the current drift. Switching lands on the next loop boundary. No morph.
- **Pages:** ORB (home, the rings animated), RING (RHYTHM, VOICE, NOTES), CHRD, SCN, as mocked. The full per-ring pages stay the main view.
- **Buttons in ORBIT** become transport (play/stop) and ring mutes; from a ring you can jump straight to its Part's pages.
- **Saved with the project.**
- **Live input snapping:** while ORBIT runs, notes played on an attached keyboard go through the same snapper. A SNAP switch per Part says whether its live notes snap, and to the scale or the chord. KIT Parts stay unsnapped.
- **Held notes on a chord change,** per Part: STAY (keep the pitch until released) or RE-SNAP (glide to the nearest tone of the new chord). Leads stay; pads follow the progression.
- **The screen:** concentric rings turning, dots lighting as they fire.
- **Order:** prototype ORBIT completely on the desktop simulator first, ahead of #214 and #215, as a sound-design tool; firmware wiring once it feels right. KIT targets wait for #215. It only emits notes into Parts and costs almost no CPU.
- **Architecture (approved):** a pure `orbit` module; ordinary note events on the MIDI path, live notes through the per-Part snapper on it; a clock that follows MIDI clock and start/stop, with an internal BPM fallback; a seeded PRNG; the pages in the shared UI code; saved through the storage layer's frozen codes; desktop first, firmware behind a cargo feature; the three slices above.
- **Navigation:** holding SEQ enters ORBIT, a new chain; a short SEQ press keeps its job (sub-page up; save in the browser). In ORBIT: B1–B5 pick a ring and open its RING page; MIX + B1–B5 mutes or unmutes it; B6 is PLAY/STOP; EDIT jumps to the selected ring's Part; a short SEQ, or EDIT on a page with sub-pages, moves through RHYTHM, VOICE and NOTES; MENU leaves as today. ORBIT keeps playing after you leave; a small ORBIT dot in every page's header shows it runs and pulses on the beat. Holding SEQ again returns to ORBIT.
- **Custom 12-note scales (slice 1):** a scale is a user-picked pitch-class set, a `chimera-theory` `Scale(u16)` that always contains the root. On CHRD, SCALE offers the named scales plus CUSTOM, which opens a 12-dot circle where each note is toggled with the encoder or buttons; the root stays locked on, enforced by the type. Saved with the project under a frozen code and ident. Snapping, degree-to-chord and naming work unchanged: degrees stack thirds within the set, and a set of fewer than 7 notes has its own notes as degrees, each chord stacking every other note. Microtuning is #217.
- **Music theory** (pitch classes, keys, scales, chord qualities, degrees → chords, `Chord`, `snap`, naming) is its own crate, `chimera-theory`: no_std, zero dependencies, const tables, no heap, a few hundred bytes, beside `chimera-fat` and `chimera-waves`. `chimera-core` depends on it. ORBIT, live snapping, and later KIT and possibly Sympathetic's chords use it.

## Technical decisions (this spec)

- **Block-accurate timing, centred ±32 samples:** voices start only at block boundaries, and ±0.67 ms is below a DIN note's own transmission time (§ 6.2).
- **Position as a 32-bit fraction of a bar:** any step count from 1 to 32 lands exactly, independent of 24 PPQN (§ 6.2).
- **External clock interpolated, clamped at the next tick:** smooth between ticks, never ahead of the master, never backwards (§ 6.2).
- **A bar is 96 clocks (4/4):** MIDI clock has no meter, and 4/4 is every DAW's default (§ 6.2).
- **Fallback to internal after 500 ms without a tick,** transport unchanged: a stopped DAW doesn't start ORBIT (§ 6.1).
- **Real-time events share each source's note queue:** they keep their order with the notes, with no new queue or thread (§ 6.1).
- **Voices: shared, no priority, the allocator's rules:** the owner's "both play, sharing voices", and ORBIT's tails go first (§ 9).
- **Steals and refusals aren't shown on the ring:** normal allocator behaviour, and marking them needs a per-event report (§ 9).
- **Note-offs match origin, channel and key:** a ring never releases a held key, and a snapped live note releases on its own key-up (§ 5.2).
- **STOP releases only ORBIT's notes, at once, with tails:** ORBIT releases what it started and nothing else (§ 6.3).
- **PLAY starts at the loop start; Continue resumes:** the rings and progression realign audibly (§ 6.3).
- **Scene switch at the playing scene's loop boundary; old notes finish their gates:** no cut notes, no morph (§ 6.3).
- **Snap ties go down:** one fixed rule keeps `snap` a pure function (§ 5.1).
- **RE-SNAP from the raw key, over a 40 ms glide:** repeated chord changes never drift, and 40 ms follows the chord without a click (§ 5.2).
- **Capture: first key-down to last key-up, any channel, ≤ 8 notes, root = lowest:** a chord is the notes you held together, and the root is what you played at the bottom (§ 5.3).
- **The engine owns the drift; the UI owns the knobs; revs say who wins:** MUTATE is sample-exact on the audio thread, and edits still apply at once (§ 4.1).
- **One `Rng` per ring:** editing one ring never reshuffles another (§ 3.2).
- **A missed or muted hit still walks and draws:** the phase between rings and pools is kept (§ 3.6).
- **Memory: engine ≈ 520 B, asserted under 1 KB; about 5.7 KB in all, mostly saved scenes in AXI** (§ Memory).
- **The live scene saves as scene 8:** a project reloads exactly where it was (§ 8).
- **Codes and codec outside the `orbit` feature:** a firmware with ORBIT off never drops a project's ORBIT data (§ 8).

## Design choices for the owner's review

These fill gaps in the mockups and the owner's decisions. Each is the smallest choice that makes the pages work; any can change without touching the rest.

- **Named scales:** the ten built on `orbit-core` (MAJOR, MINOR, the five other modes, HARMONIC MINOR, the two pentatonics), beside CUSTOM; progression chords are triads, as the CHORDS mockup shows (Am, Dm).
- **Custom sets of more than 7 notes** use the every-other-note rule too, so an 8-note diminished set gives diminished triads and a 12-note set gives clusters.
- **The CUSTOM circle:** turning SCALE to CUSTOM opens it as a CHRD sub-page; encoder A moves the cursor and encoder B or EDIT toggles. A fresh CUSTOM set starts as the named scale picked before it.
- **Ranges:** STEPS 1–32 (the mockup's bar is drawn over 32); SEGS 1–8 (drawn over 8); OCTAVE 0–8 (drawn over 8, with C4 = 60); internal BPM 30–300 in steps of 1, default 120.
- **Accent:** +30 velocity, capped at 127, from the VOICE mockup's "+30 VEL"; the accent fill has no rotation.
- **MUTATE:** one change per ring per loop at most, with probability MUTATE; half swaps a note, half nudges a hit by one step.
- **CHRD segment select:** DEGREE edits the selected segment; MIX + DEGREE's encoder picks the segment.
- **CHRD PLAY:** `—` (no override), turned up arms it (`HOLD`), a capture shows the chord's name; turned down from a chord clears the override.
- **NOTES:** LEARN turned up arms (`HOLD`), down disarms; CLEAR clears on a turn up. Picking notes by hand and random fill take NOTES' sixth cell, designed in slice 2's page task.
- **SCN:** turning SCENE queues that scene; SAVE's encoder picks the slot (`→ n`, default the playing one) and EDIT on SCN commits it; SWITCH shows LOOP, read-only, the owner's one switch point.
- **ORB encoders:** A picks the selected ring; F sets the internal BPM (dimmed under an external clock).
- **B1–B5** open RING on the sub-page last shown, so you can compare ring 2's VOICE with ring 3's.
- **Holding SEQ back into ORBIT** returns to the page you left; inside ORBIT it goes to ORB.
- **PART SNAP and HELD** sit on the Part's mixer page with its other per-Part settings; HELD's values read STAY and RE-SNAP.
- **The init scene** starts from the ORB mockup's rings (4/4, 3/3, 5 in 13 rotated 2, 3 in 8 muted, 9 in 16 rotated 1) in A minor, i–iv–v–i over one bar; ring n targets Part n; pools are the tonic triad in each ring's octave. It is tuned with the owner in slice 1, task 4.

## Conflicts for the owner

- **Nearest-quality naming changes a chord's tones** (§ 2.2, choice 1). The owner ruled that a custom set's chord stacks every other note, and that a played chord's tones are what snapping uses. `orbit-core` stores a chord as a root and a `Quality`, so a stack or a played chord that names no quality becomes the nearest named one, with different tones: C D E G A's stack C E A becomes C6 (C E G A), adding G; a played C E A snaps to C E G A too. The alternatives: keep it (names always exist, one more tone), or give `Chord` a third form holding the exact set, unnamed. The spec keeps the built behaviour until the owner rules.

## Open questions

None beyond § Conflicts.
