# TAPE as a sampler: PREP, then PLAY

- **Status:** Draft for owner review (2026-10-15)
- **Issue:** private issue #476 (the TAPE release headline); #215 (KIT Part, choke groups); #398 (clean-up)
- **Builds on:** the TAPE core (`2026-10-04-tape-core-design.md`, ADR 0093), the pool (ADR 0087), `put` for sources (ADR 0106), bank 2 (ADR 0137, 0141, 0142), the card format (ADR 0045, 0048, 0102)
- **Research:** (private notes) (the plan; its § 4 holds the owner's answers), `feature-table.md` (streaming feasibility, the M1 card run), `tape-rework/adversarial-review-streaming.md` (the bench plan B1–B10)
- **ADRs:** 0145 (the machine lineup), 0146 (PREP and the `.PRP`), 0147 (streaming by DMA behind heads). All Proposed.
- **First slice in detail:** `2026-10-15-tape-slice1-streaming-design.md`

This is the umbrella. It fixes what every slice shares: the machines, the prep types and their file, PREP's screens, the streaming architecture, budgets, and the slice list. Each slice gets its own plan; slice 1 also gets its own spec.

## Owner's decisions (2026-10-15, binding)

1. **Machines:** PLAY (one-shot, loop, ping-pong; takes RAW or ZONES; release FADE; streams automatically when the source is too big for RAM), KIT (64 slices from C1, black keys CUT in one choke group, white keys STACK), TRON (provisional: a thin layer over PLAY, its own page and defaults RUN 8 s, FADE, WOW; it must earn its keep by the release or fold into factory Sounds on PLAY), REEL, SCRUB, STRETCH, WAVE. GRAIN later.
2. **PREP is its own mode** with its own UI and UI language, as SETTINGS has. Its chain: TRIM · CUT · LOOP · MAP · REEL · MAKE.
3. **A `.PRP` beside the `.SRC`** holds the prep. Tools that change the sound render a new file and never overwrite.
4. **KIT defaults** as above. Cut and choke are shared with #215's KIT Part, tentatively: designed shared, with a seam that is cheap to split.
5. **TRON:** zones now (two per octave, 8 s, 12-bit/24 kHz mono, 100 ms heads). Per-key tapes later.
6. **REEL programs:** up to 8 steps (splice, direction, repeats, speed), each with a probability, as ORBIT's hits have.
7. **RESAMPLE** is in the first TAPE release.
8. **Writes fade streams:** a PREP render or a MENU-hold save fades streamed notes out and back in.

Build order: bench → DMA card layer and streaming core (proved by PLAY streaming) → TRON → PREP and KIT → REEL. RESAMPLE is in the release too.

---

## 1. The instrument in one page

TAPE stays one engine (`EngineType::Tape`, `BlockRef::TapeEngine`, ADR 0093). It has two stages:

- **PREP** turns a sealed `.SRC` into a **prepared sample**: trimmed, sliced, looped, mapped or spliced. The marks go in a `.PRP`. Audio changes render a new `.SRC`.
- **PLAY** is the Part. MODEL picks a **machine**, and each machine takes certain **prep types**. SOURCE picks the sample; the Sound copies its prep in, as it copies ROOT and PERIOD today (ADR 0093). A project therefore plays the same on any card.

Long sources stream from the card behind RAM **heads**. Which sources stream is automatic. The player sees `STREAMED` in SOURCE's focus band, and nothing else changes.

```
 .SRC (sealed audio) ──► PREP ──► .PRP (sealed marks) ──► SOURCE cell ──► Sound (prep copied in)
        ▲                  │                                                   │
        └── new .SRC ◄─────┘ RENDER / RESAMPLE                    machine plays it, RAM or streamed
```

## 2. Machines

### 2.1 The lineup

MODEL's screen order and disk codes. Codes are frozen once written; the ones marked "new" are proposed and fixed by their slice.

| MODEL | Code | Takes | RAM or stream | Bill (cycles a sample) | Slice |
|---|---|---|---|---|---|
| **PLAY** | 0 (LOOP's) | RAW, ZONES | RAM if it fits the arena's free space, else streamed | ONE 230, LOOP 450, PING 490 (as built), + the stream term (§ 5.8) | 1 (ZONES: 2) |
| **KIT** | 8, new | KIT | RAM only | 230 (ONESHOT's path) | 4 |
| **TRON** (on probation) | 6 | ZONES, RAW | as PLAY | 230 + the stream term | 2 |
| **REEL** | 9, new | REEL | streamed, or RAM if it fits | 450 + program stepping, measured | 5 |
| **SCRUB** | 4 | RAW | RAM only | 480 | as built |
| **STRETCH** | 5 | RAW | RAM only | 450 | as built |
| **WAVE** | 3 | RAW (transwave) | RAM only | 400 / 770 | as built |
| GRAIN | 10, reserved | RAW | RAM only | — | later |

**Folded into PLAY (slice 1):** ONESHOT (code 7) loads as PLAY with MODE ONE and FADE HOLD; PING-PONG (code 2) loads as PLAY with MODE PING. ONESHOT's SLICE values load and are ignored: KIT does slicing now. This loads old Sounds safely; it doesn't promise they sound the same (no-legacy-compat).

**MODEL refuses a source of the wrong prep type** before allocation, as TRANSWAVE refuses a plain source today (`TapeDeck::refuses`). The focus band says why: `KIT NEEDS A KIT PREP`.

### 2.2 Pages and labelled cells

The chain stays `SRC · DECK · <machine> · DRV · FLT · AMP · MOD`, opening on DECK (ADR 0093, 0120). Each machine labels the shared params its own way and dims what it ignores, from the one table (`tape_engine::machine::MACHINES`). Six cells a page at most.

**SRC (every machine):** SOURCE, START, END, VOL, PREP, –.
- PREP shows the prep's name and type (`AMEN 64 · KIT`), or `RAW`. Turning it steps through the `.PRP`s made from this source. **MIX + EDIT on SOURCE or PREP opens PREP** on the sample, with the hint `MIX + EDIT · PREP THIS SAMPLE` (ADR 0086).
- When streamed, SOURCE's focus band reads `STREAMED · 41 KB/S · 7 KB HEADS`.

**DECK (home):** MODEL, then two machine-labelled cells (POSITION and MOTION, or dimmed), TAPE, AGE, –.

| Machine | DECK cells 2–3 | Machine page (≤ 6 cells) |
|---|---|---|
| PLAY | POSITION, WALK (MOTION) | MODE (ONE · LOOP · PING), LENGTH (SIZE), XFADE, FADE (DRAG), WOW |
| KIT | SHIFT, – | FADE, CHOKE (ON · OFF), REVERSE (OFF · ALL), WOW |
| TRON | –, – | RUN, FADE, WOW, MOTOR (candidate, § 2.5) |
| REEL | SPLICE (POSITION), SPEED (MOTION, bipolar) | PROGRAM (OFF · 1–8), GAP, XFADE, FADE, WOW |
| SCRUB, STRETCH, WAVE | as built | as built |

New params on `TapeEngineParams`, ids appended (frozen once written, each fixed by its slice): **MODE 15** (note-on), **RUN 16** (0.25–8 s, note-on), **SHIFT 17** (−63…+63 slices, note-on), **CHOKE 18** (note-on), **REVERSE 19** (note-on), **PROGRAM 20** (note-on), **GAP 21** (0–500 ms, block). None is modulatable but SPLICE and SPEED, which reuse POSITION and MOTION (already modulatable, eased).

- **FADE** is DRAG relabelled (10–500 ms, exponential), the release through the pressure-pad one-pole TRON has today. On PLAY its last step reads **HOLD**: note-off is ignored and the note plays to END. HOLD is offered only in MODE ONE, so no loop can sound forever. ONE's default is HOLD (a drum one-shot), LOOP's and PING's 32 ms.
- **WOW** keeps CAPSTAN and WARBLE.

### 2.3 PLAY

PLAY is ONESHOT, LOOP and PING-PONG as one machine. Their code stays as it is (`oneshot.rs`, `heads.rs`); MODE picks the path at note-on. END before START reverses ONE and LOOP (ADR 0093). PLAY takes a ZONES prep from slice 2: the note picks its zone from the layout, as TRON's spec already defines (`SourceZones`, `ZoneTune`).

**Streaming is automatic.** A source that fits the arena's free space is held whole. One that doesn't is held as heads and streamed (§ 5). In slices 1–4 streaming is **forward only**: MODE ONE and forward LOOP. PING and END before START on a streamed source are refused with `PING NEEDS RAM` / `REVERSE NEEDS RAM` until slice 5 brings backward reads. A streamed LOOP's loop-start head begins XFADE before the loop, so the crossfade reads RAM; XFADE is capped at 100 ms of source, and the cell reads the cap in its focus band.

### 2.4 KIT

The owner's example, built on ONESHOT's one-head path.

- **Up to 64 slices** (the Octatrack's ceiling), each a `(start, end)` window of the source in frames; end before start plays it reversed. Slice `n` sits on key `BASE + n`; BASE is C1 (36) by default, so 64 slices fill 36–99. SHIFT moves the whole map by slices.
- **Per slice:** `Rehit` (CUT or STACK), a choke group (0 none, 1–4), LEVEL (−24…+6 dB), TUNE (−24…+24 semitones), REV.
- **Defaults:** black keys CUT in choke group 1, white keys STACK with no group. Every slice can change in PREP's MAP page.
- **Whole source in RAM.** 64 streamed slices would need 64 heads (230 KB); not in this release.
- CHOKE OFF on the machine page ignores the groups (every key STACKs or CUTs only itself); REVERSE ALL flips every slice.

**The seam with #215 (decision 4).** Cut and choke live in one small module that both KITs call, and nothing else couples them.

```rust
// chimera-engine/src/choke.rs (bank 1: the allocator runs it)
pub enum Rehit { Cut, Stack }
pub struct ChokeGroup(NonZeroU8);          // 1..=4
pub struct KeyPolicy { pub rehit: Rehit, pub choke: Option<ChokeGroup> }

/// The voices of `part` a note-on of `note` under `policy` must cut:
/// the same key's voice if `Cut`, and every voice in the same group.
pub fn victims(slots: &[VoiceSlot; MAX_VOICES], part: u8, note: MidiNote,
               policy: KeyPolicy, policy_of: impl Fn(MidiNote) -> KeyPolicy) -> VoiceMask;
```

- The Instrument asks the Part for a `KeyPolicy` at note-on. A TAPE KIT Part answers from its prep's slice table (§ 3.3); #215's KIT Part will answer from its keymap; every other Part answers `Stack` with no group, and `victims` returns nothing.
- Cut voices leave through the allocator's existing fade (`Allocator::fade_from`, 2.7 ms). No new voice state.
- **Splitting later costs a copy of `choke.rs`** (about 60 lines and its tests). The policy tables stay with their owners either way.

### 2.5 TRON, on probation

TRON is PLAY's voice and PLAY's streaming with its own page and defaults: MODE ONE, **RUN** 8 s (the tape runs out in output time at every pitch, as built), **FADE** 32 ms, **WOW** WARBLE. It takes ZONES (two per octave, 12/24 mono, 100 ms heads) and RAW.

It must earn its keep by the release (slice 7) with something PLAY can't do. The candidates, with their costs:

| Candidate | What the player gets | Cost |
|---|---|---|
| **MOTOR** (recommended) | One tape motor per Part: every held note drifts together, as the M400's one capstan does, against PLAY's per-voice wow | One `Transport` per Part instead of per voice: cheaper than today. About 1 KB of flash. |
| **REWIND** | A quick replay starts mid-tape, where the spring has rewound it | Every such start is a cold read (no head there): up to ~60 ms late, fading in. The voice's ring holds material ahead of the head, not behind. Slice 2 measures whether it feels like a Mellotron or like a fault. |

If neither earns its place, TRON folds into PLAY: factory TRON Sounds become PLAY Sounds with RUN, FADE and WARBLE set, and code 6 loads as PLAY with the same settings.

### 2.6 REEL

A Morphagene-like reel.

- **Splices:** up to 32 marks over one source; splice `n` runs from its mark to the next.
- **SPLICE** (DECK, modulatable) picks a splice live: ORGANIZE. Keys from C1 (36) also pick splices 1–16 (**key-reachable**).
- **SPEED** is bipolar vari-speed through zero, ±2× (MOTION's law on SCRUB).
- **PROGRAM** plays one of up to 8 programs. A program is up to 8 steps, then an end (LOOP, STOP or NEXT program). Each step holds:

| Field | Range |
|---|---|
| SPLICE | 1–32 |
| DIR | → · ← · ↔ (ping-pong within the splice) |
| REPEATS | 1–16 |
| SPEED | −2…+2× in 25 cent steps near 1×, as a ratio |
| CHANCE | 0–100 %, default 100 % |

- **CHANCE** (decision 6) is rolled once per step, at the start of the step before it, from a per-Part seed, as ORBIT's hits are. A failed roll skips the step. Rolling a step early means the next splice is known a whole step ahead, so streaming can prefetch it (§ 5.6).
- **GAP** puts silence between steps, eased in and out over 3 ms. XFADE crossfades step to step when GAP is 0.
- The owner's example is one program: `A → ×3 · A ← ×3 · LOOP`.

### 2.7 SCRUB, STRETCH, WAVE

Unchanged from the core (ADR 0093, 0103). RAM only: their reads jump, and a jump is a seek.

### 2.8 Everything every machine keeps

The DRV, FLT, AMP and MOD pages; the TAPE and AGE macros (the "WEAR" layer: wow, flutter, hiss, band limit, drop-outs); ORBIT (rings target a TAPE Part, or a KIT key once #215's key targets exist); the per-Part MIDI map (ADR 0139).

## 3. Prepared samples and the `.PRP`

### 3.1 Prep types

| Type | What it holds | Record bytes | Played by |
|---|---|---|---|
| **RAW** | START, END, loop start and length, XFADE, ROOT | 22 | PLAY, TRON, SCRUB, STRETCH, WAVE |
| **KIT** | BASE; up to 64 slices: start u32, end u32, `KeyPolicy` u8, LEVEL i8, TUNE i8, flags u8 | 2 + 12 a slice: 770 at 64, so two records | KIT |
| **ZONES** | Per zone: low key, high key, root (cents). The zone layout itself is in the `.SRC` (CRC'd, ADR 0093); this overrides its keys and roots without new audio | 1 + 4 a zone: ≤ 97 | PLAY, TRON |
| **REEL** | Up to 32 splice marks; up to 8 programs of up to 8 steps (5 B a step) and an end | ≤ 466 | REEL |

A record holds at most 640 B (ADR 0102), so KIT's slices take two `PrepSlices` records of up to 32 slices, each carrying its first slice's index.

### 3.2 The `.PRP` file

- **Where:** `/CHIMERA/TAPES/<PRPCRC>.PRP`, beside the sources.
- **Kind:** `FileKind::Prep` = 7 (ADR 0045's header; kinds 4 and 5 stay reserved).
- **Records:** `PrepHead` first, then the type's records.

| Record | Critical | Payload |
|---|---|---|
| `PrepHead` | yes | the source's `SourceRef` (10 B), type u8, name 16 B |
| `PrepRaw` | yes | as § 3.1 |
| `PrepSlices` | yes | first index u8, BASE u8, then slices |
| `PrepZones` | yes | as § 3.1 |
| `PrepReel` | yes | splices, then programs |
| `PrepView` | no | the waveform overview, 216 min/max pairs (432 B), so PREP reopens instantly |

  Tag codes are the next free ones when slices 3–5 build them (0x001F upward on main today), checked against open branches as TAPE's were.
- **Sealed and write-once, named by content.** The name is a CRC over `PrepHead`'s ref and type and the type's records. The name and `PrepView` are labels, outside it, as a source's root and name are. It is written by `write_source`'s rules: write, read back, verify, seal (ADR 0087, 0106). A sealed file that fails reads as missing and is never rewritten.
- **Editing a prep makes a new file.** MAKE writes the new `.PRP`, seals it, then deletes the one it replaces (if the user chose REPLACE, not SAVE AS). A cut between the two leaves both: harmless, and SAMPLES lists both.
- **Several preps per source** are allowed: a KIT and a REEL of the same break. SAMPLES finds a source's preps by the ref in each head.
- **The Sound copies the prep in** when SOURCE or PREP picks it: the same records, inside the Sound (Sound records may hold them; a `SourceRef` names the source). The audio never reads a `.PRP`. A project keeps working with the `.PRP` deleted.
- **Clean-up (#398)** gains one rule: a `.PRP` whose source is gone is an orphan, and is deleted. A `.PRP` is otherwise a library item; the user deletes it in SAMPLES.

**Why content-named, not named after its source:** a source may have several preps, and a file named after its source would have to be rewritten in place. That needs an A/B pair (ADR 0045) on a card the owner power-pulls. Write-once files need no A/B, reuse the sealing code `put` already has, and match the `.SRC` beside them (ADR 0146).

### 3.3 What the audio reads

A machine needs its prep at note-on: KIT the key's slice and policy, REEL its splices and programs, PLAY its heads.

- **RAW** values are Sound params (START, END, POSITION, SIZE, XFADE, ROOT), written when the prep is picked. Nothing new reaches the audio.
- **KIT and REEL tables** are published as **prep rows** in the pool's `SourceTable`: bytes in the arena, a row with `base` and `held`, latched at note-on, counted on the meter and in `sounding`, moved only under the gate (ADR 0087). They share `MAX_SOURCES` (32): a KIT Sound costs its source plus one prep row. Slice 4 fixes the encoding.
- **ZONES** overrides go into the Sound's `ZoneTune` and a key table beside it; slice 2 fixes it.

### 3.4 What renders new audio

Only these write a new `.SRC`: FADE IN/OUT, GAIN, NORMALISE, REVERSE (baked), CRUSH (bits and rate), a STRETCH bake, and RESAMPLE.

- A render is pure. It reads the old `.SRC` in 4 KB chunks and writes a new one under `write_source`'s rules. The file is named by its CRC and the FAT layer has no rename, so it **renders twice**: once for the CRC, once to write (ADR 0093 § 8).
- **Requantising on the device** (GAIN, fades, RESAMPLE) uses TPDF dither from a seeded xorshift: about 20 instructions a sample. This amends ADR 0093's "dither is host code" for on-device renders only. WAV decode and sample-rate conversion stay on the host.
- The new `.SRC` gets its own fresh `.PRP` (the old marks, re-timed if the render moved them).

## 4. PREP: the mode

### 4.1 Doors

- **From a TAPE Part:** MIX + EDIT on SOURCE or PREP (SRC page). MENU returns there.
- **From SETTINGS › SAMPLES**, the library in SETTINGS' list language (§ 4.6). EDIT on a row opens PREP on it. MENU returns to the list.
- **From RESAMPLE:** a finished take opens in PREP's TRIM.

PREP never changes a Sound by itself. It writes a `.PRP` (and maybe a `.SRC`). Back on the Part, PREP's cell offers the new prep. Opened from a Part, MAKE ends with `USE ON PART 2? YES / NO`.

### 4.2 Its own language

SETTINGS is lists, a breadcrumb and a footer. Part pages are the focus band, the arc and six cells under a chain map. PREP is a **workbench**: one big waveform with numbered marks, a leader strip of its chain, a readout line, six compact dials, and a **tool bar that labels B1–B6**. No other mode labels those buttons, so you always know you are in PREP.

| Element | Size (240 × 320 portrait) | Look |
|---|---|---|
| Header | 0–23 | A filled `PREP` tag in `ACCENT`, the source's name, the prep type at the right in a pill |
| Leader | 24–39 | `TRIM · CUT · LOOP · MAP · REEL · MAKE`, the current page in a box, the rest `MID`; pages that don't apply to the type are `FAINT` |
| Overview | 40–55 | The whole file, min/max in `FAINT`, the zoom window bracketed in `INK2` |
| Waveform | 56–175 | 216 × 120, min/max a column in `MID`, the selection shaded `ACCENT_SOFT`, marks as `WARN` lines with numbered flags along the top, the audition head in `INK` |
| Ruler | 176–187 | Time at the window's ends and the cursor, in seconds to 3 places |
| Readout | 188–215 | What the focused dial does, large: `SLICE 07 / 24 · 1.214 S`. It is also where MIX hints show (ADR 0086). |
| Dials | 216–287 | Six cells, two rows of three, label and value only (no arcs), eased as every value (ADR 0143) |
| Tool bar | 288–319 | Six boxes in the order of B1–B6, each a page tool's name |

Marks are amber (`WARN`) only in PREP. The Part pages never draw `WARN` but for warnings, so the colour also says where you are.

### 4.3 Controls

| Control | In PREP |
|---|---|
| MINUS / PLUS | Along the chain (D-pad rule) |
| EDIT / SEQ | Down into a page's sub-page and back up (CUT › GRID / TRANSIENT / HAND); SEQ at the top does nothing |
| Encoders 1–6 | The six dials |
| B1 | Audition the selection: play, stop |
| B2–B6 | The page's tools, labelled in the tool bar |
| A played key (MIDI or the Part's keys) | Auditions the prep as mapped: on KIT that key's slice; on ZONES that zone; on REEL that key-reachable splice |
| MIX + a button | Big or destructive actions, each with its readout hint |
| MIX + PLUS | MAKE · SAVE (any page) |
| MENU | Leave. Unsaved marks ask `KEEP EDITING / DISCARD` |

**Audition** plays through a neutral TAPE Sound of the right machine on the Part PREP came from (from SAMPLES, the last focused Part). The Part's own Sound is untouched. Audition streams if the source is big, like any note.

### 4.4 The pages

Each mockup is the 240 × 320 screen, one character about 6 px.

**TRIM** (RAW and every type: the window the prep keeps)

```
┌PREP┐ RAIN ROOF            ( RAW ) 
 [TRIM] · CUT · LOOP · MAP · REEL · MAKE
 ▕▁▂▃▅▆▅▃▂▁▂▃[▅▆▇▆▅▃]▂▁▂▃▅▃▂▁▁▂▃▅▆▅▃▂▕
┌──────────────────────────────────────┐
│ S                                  E │
│ ▏  ▂▃▅▆▇▆▅▃▂ ▂▃▅▇█▇▅▃▂ ▂▃▅▆▅▃▂  ▂▃ ▏ │
│ ▏ ▅██████████████████████████████▅▏ │
│ ▏  ▂▃▅▆▇▆▅▃▂ ▂▃▅▇█▇▅▃▂ ▂▃▅▆▅▃▂  ▂▃ ▏ │
└──────────────────────────────────────┘
 0:00.412          ▲ 0:06.020    1:54.900
 START  0:00.412 · ZERO CROSS
 ┌START──┐┌END────┐┌ZOOM───┐
 │0.412 S││114.9 S││  ×4   │
 └───────┘└───────┘└───────┘
 ┌GAIN───┐┌FADE IN┐┌FADE OU┐
 │+0.0 DB││  0 MS ││ 20 MS │
 └───────┘└───────┘└───────┘
 [▶ PLAY][TO S  ][TO E  ][REVERS][NORMAL][ SNAP ]
```

- Dials: START, END (snapping to zero crossings when SNAP is on), ZOOM, GAIN, FADE IN, FADE OUT.
- Tools: B1 PLAY, B2 jump the view TO START, B3 TO END, B4 REVERSE (toggles a render mark), B5 NORMALISE (a render mark), B6 SNAP on/off.
- GAIN, fades, REVERSE and NORMALISE are marks until MAKE, and are heard in audition (the audition applies them on the fly). MAKE renders them.

**CUT** (KIT and REEL: where the marks go). Sub-pages GRID, TRANSIENT, HAND (EDIT / SEQ).

```
┌PREP┐ AMEN BREAK           ( KIT ) 
 TRIM · [CUT] · LOOP · MAP · REEL · MAKE
 ▕▃▆▃▂▆▃▂▅▃▂▆▃▂▅▃▂▆▃▂▅▃▂▆▃▂▅▃▂▆▃▂▅▃▂▕
┌1───2────3───4────5───6────7───8────┐
│▏   ▏    ▏   ▏    ▏   ▏    ▏   ▏    │
│█▅▃▂█▅▃▂▁█▅▃▂█▅▃▂▁█▅▃▂█▅▃▂▁█▅▃▂█▅▃▂ │
│▏   ▏    ▏   ▏    ▏   ▏    ▏   ▏    │
└────────────────────────────────────┘
 0:00.000     ▲ 0:00.402       0:01.851
 TRANSIENT · 16 SLICES · MIN GAP 40 MS
 ┌SLICE──┐┌POS────┐┌ZOOM───┐
 │ 3 / 16││0.402 S││  ×1   │
 └───────┘└───────┘└───────┘
 ┌THRESH─┐┌MIN GAP┐┌COUNT──┐
 │ -24 DB││ 40 MS ││  16   │
 └───────┘└───────┘└───────┘
 [▶ PLAY][ ADD  ][DELETE][◂ PREV][NEXT ▸][ SNAP ]
```

- GRID: COUNT (2–64 equal). TRANSIENT: THRESH and MIN GAP (an envelope follower over one read of the file). HAND: ADD a mark at the audition head, DELETE, PREV, NEXT, POS moves the focused mark (snapping to a zero crossing).
- MIX + B3 clears every mark (`MIX + DELETE · CLEAR ALL MARKS`).
- Up to 64 marks for KIT and 32 for REEL. The 65th ADD says `64 SLICES · KIT IS FULL`.

**LOOP** (RAW)

- Dials: LOOP START, LENGTH, XFADE, ZOOM. Tools: B1 PLAY (loops), B2 FIND, B5 SNAP.
- FIND (B2) nudges the loop end to the best match near the marks: autocorrelation over at most 4,096 frames around them, a few ms.
- These become the Sound's POSITION, SIZE and XFADE when the prep is picked.

**MAP** (KIT and ZONES)

KIT: a keyboard of 64 keys from BASE, one slice a key, drawn as the slice's policy.

```
┌PREP┐ AMEN BREAK           ( KIT ) 
 TRIM · CUT · LOOP · [MAP] · REEL · MAKE
┌C1─────────────C2─────────────C3──────┐
│▌▐▌▐▌ ▐▌▐▌▐▌ ▐▌▐▌ ▐▌▐▌▐▌ ▐▌▐▌ ▐▌▐▌▐▌│ black keys: CUT, group 1 (amber)
│1 2 3 4 5 6 7 8 9 ...                 │ white keys: STACK
└──────────────────────────────────────┘
 ▕█▅▃▂▁▁ slice 07 waveform ▁▁▂▃▅█▕
 KEY F#1 · SLICE 07 · CUT · GROUP 1
 ┌KEY────┐┌REHIT──┐┌GROUP──┐
 │  F#1  ││  CUT  ││   1   │
 └───────┘└───────┘└───────┘
 ┌LEVEL──┐┌TUNE───┐┌BASE───┐
 │+0.0 DB││  +0   ││  C1   │
 └───────┘└───────┘└───────┘
 [▶ PLAY][ REV  ][ COPY ][PASTE ][◂ KEY ][ KEY ▸]
```

- MIX + B2 resets every key to the default (black CUT in group 1, white STACK).
- ZONES: zone count, each zone's low and high key and root; AUTO (B2) spreads them two an octave.

**REEL** (REEL): splices come from CUT. This page edits programs.

```
┌PREP┐ TAPE LOOPS 3         ( REEL ) 
 TRIM · CUT · LOOP · MAP · [REEL] · MAKE
 ▕A▁▂▃▅▆│B▅▃▂▁▂│C▃▅▆▅▃│D▂▃▅▃▂▁│E▁▂▃▅▕
 PROGRAM 1                    END: LOOP
  1  A  →   ×3   1.00×  100 %
  2  A  ←   ×3   1.00×  100 %
 [3] C  ↔   ×2   0.50×   50 %    ◂ focused
  4  —
 C ↔ ×2 · HALF SPEED · PLAYS HALF THE TIME
 ┌STEP───┐┌SPLICE─┐┌DIR────┐
 │ 3 / 8 ││   C   ││   ↔   │
 └───────┘└───────┘└───────┘
 ┌REPEATS┐┌SPEED──┐┌CHANCE─┐
 │  ×2   ││ 0.50× ││  50 % │
 └───────┘└───────┘└───────┘
 [▶ PLAY][ ADD  ][DELETE][◂ PROG][PROG ▸][ END  ]
```

- B6 cycles the program's end: LOOP, STOP, NEXT.
- The splice strip at the top lights the step playing during audition.

**MAKE**

```
┌PREP┐ AMEN BREAK           ( KIT ) 
 TRIM · CUT · LOOP · MAP · REEL · [MAKE]
   TYPE      KIT
   NAME      AMEN 64
   MARKS     16 SLICES · 1 GROUP
   AUDIO     NO CHANGE  (.PRP ONLY, 1 KB)
   REPLACES  AMEN 16  (SAVE AS KEEPS IT)
 MIX + PLUS · SAVE
 ┌TYPE───┐┌NAME───┐┌       ┐
 │  KIT  ││ EDIT ▸││       │
 └───────┘└───────┘└───────┘
 [▶ PLAY][SAVE AS][REPLACE][      ][      ][      ]
```

- With audio marks, AUDIO reads `RENDER · NEW .SRC · 288 KB · ~1 S` and the save renders first, behind a progress bar, then writes the `.PRP`.
- NAME opens the house NAMING screen (`settings::naming`).

### 4.5 What runs on the unit and what on the host

| Tool | Where | Cost on the unit |
|---|---|---|
| Overview (min/max a column) | Unit | One read of the file: 4 MB in 2–3 s, once, then kept in `PrepView` |
| Trim, gain, fades, reverse, normalise, crush | Unit, a render | Two passes (CRC, then write). A write runs at 363 KB/s: 288 KB in ~0.8 s; a 4 MB reel ~12 s. 16 KB of scratch. |
| Grid, transient and hand slicing; splices; programs; key and zone maps | Unit | Marks only. Transient detection is one read and an envelope follower. |
| Loop FIND | Unit | ≤ 4,096-frame autocorrelation: a few ms |
| STRETCH bake | Unit | STRETCH run offline: 8 s ≈ 1 s of UI time |
| RESAMPLE | Unit (§ 6) | 4 s of render ≈ 2–3 s of UI time, twice, then the write |
| WAV/AIFF decode, sample-rate conversion | Host (`just tape`) | Not linked (ADR 0093) |
| Multisample folder → zone set | Host | YIN and layout |
| Transwave building | Host | FFT, mip levels |

Every unit tool runs in the UI loop in chunks behind `ui::busy` with a progress bar; audio never waits (ADR 0087's rule). MENU cancels a render, deleting its unsealed file.

**PREP's RAM:** 16 KB of chunk scratch, a static in D2; the overview (432 B) and the marks being edited (≤ 1 KB) in AXI. **Card writes in PREP** fade streams on other Parts (decision 8, § 5.7).

### 4.6 SETTINGS › SAMPLES

The library, in SETTINGS' own language: breadcrumb, list rows, footer (ADR 0066).

```
 SETTINGS › SAMPLES
 ─────────────────────────────────────
  ▌BEDS                              ›
   KEYS                              ›
   DRUMS                             ›
   MINE                              ›
   RESAMPLE                          ›
   CARD TEST            8 STREAMS    ›
   CLEAN UP             3 UNUSED     ›
 ─────────────────────────────────────
 PROJ: RAINY DAY *          CARD 41 %
```

```
 SETTINGS › SAMPLES › BEDS
 ─────────────────────────────────────
  ▌RAIN ROOF        RAW   1:54  4.1 MB
   CRACKLE          RAW   0:58  2.1 MB
   ROOM TONE        LOOP  0:30  1.0 MB
 ─────────────────────────────────────
  EDIT ▸ PREP     MIX+MINUS · DELETE
```

- A row is a `.SRC` or a `.PRP` (indented under its source), with type, length and size.
- **EDIT** opens PREP. **MIX + MINUS** deletes behind a prompt. A source a saved Sound uses can't be deleted; the prompt says which.
- **CARD TEST** runs the slice-0 stream test on this card and stores the stream budget in `SystemSettings` (§ 5.5).
- **CLEAN UP** is #398's, with the `.PRP` rule of § 3.2.
- Categories come from a catalog file (`/CHIMERA/TAPES/CATALOG.IDX`) that raises SOURCE's 64-file cap (`MAX_TAPES`). Its size limit comes from bench B10 (a 2,000-file listing).

## 5. Streaming

The full design of slice 1 is in its own spec. Here is what every slice builds on. ADR 0147 records it.

### 5.1 Why and how much

M1 (2026-10-15, owner's 16 GB SDHC, 62 % audio load): random 4 KB read median 5.3 ms, p95 5.4–6.3 ms, p99 6.3 ms, max 6.66 ms; interleaved 778 KB/s over 10 streams; 0 overruns. Load barely moves latency. **DMA is needed for CPU time, not latency:** a polled read busy-waits the UI thread about 5 ms each, 380 ms a second at 8 streams.

Streams are 12-bit mono, any rate the source has. At 24 kHz a stream needs 36 KB/s of samples, **41.1 KB/s of card reads** (record framing and block overshoot, § 5.3). Eight streams at 1× take 329 KB/s, 42 % of the measured rate.

### 5.2 The card layer

- **One owner of SPI2:** a bus word (`Free`, `Ui`, `Stream`). The UI's `FatStore` keeps working, through a `SharedCard` that takes the bus for one `BlockDevice` call at a time. The stream side takes it for one DMA transfer at a time.
- **DMA2** moves the data (DMA1 streams 0–2 are the audio's). SPI2 RX and TX each get a stream through DMAMUX1.
- **The card interrupt** (the RX stream's, priority level 12: under audio 0 and MIDI 4, over SysTick 15) runs the transfer state machine and the scheduler. The audio wakes it by pending that interrupt when it asks for a chunk and the layer is idle.
- **Reads only.** The stream side never writes. Writes are the UI's, and they quiet the streams first (§ 5.7).
- Pure SPI-mode protocol bits (command frames, CRC7, data-token scan, CRC16) live in `chimera-fat`, host-tested; the shell in `chimera-board`. Both are bank 1: the card must work while bank 2 is closed, or bank 2 could never be installed from it.

### 5.3 Heads, rings and chunks

- **A head** is the first stretch of material at each place a note can start: PLAY's START and loop start, each zone's start, each key-reachable splice. It lives in the arena, in the source's `held` after the descriptor, so it counts on the meter and moves only under the gate (ADR 0087). **Default 100 ms** (2,400 frames, 3,600 B at 24 kHz). The descriptor's `head_frames` makes it data (ADR 0093).
- **A chunk** is 8 `SourceData` records: 4,032 B of samples, 2,688 frames, 112 ms at 24 kHz. Records are 508 B on the card (4 B head, 504 B of samples, 168 whole pairs), so a chunk is 4,064 B and spans 9 blocks (4,608 B) unless it starts in a block's first 32 B. The card layer reads the blocks and copies out the samples.
- **A ring** per voice: 2 slots of a chunk each, plus 4 guard frames copied from the end of the slot before, so every 4-tap Hermite read lands inside one slot. 8 rings of 8,128 B: **65,024 B in SRAM4** (64 KB, unused today). Voice `n` owns ring `n`, so streams ≤ voices = 8.
- **Staging:** the DMA reads into 2 × 4,608 B in D2's non-cacheable DMA region (grown from 4 KB to 16 KB), and the card interrupt copies samples into the ring. The rings stay cacheable for the audio's reads, and nothing needs cache maintenance: only the CPU writes them.
- **Files are read by raw block address.** When the pool takes a streamed source, the UI resolves its cluster chain once (`FatStore::runs`) into at most 32 runs (the bench's `MAX_RUNS`) and registers them. A file in more runs is refused: `FRAGMENTED · COPY IT AGAIN`.

### 5.4 Scheduling

- The audio asks per ring slot: `Empty → Wanted(chunk, deadline)`. The card layer takes `Wanted → Filling → Ready`. The audio frees a slot when the head leaves it. One atomic word a slot, with a generation so a cancelled fill is dropped.
- The **deadline** is in audio blocks: when the head will reach that chunk at the note's fastest possible speed.
- **Earliest deadline first.** UI calls get the bus between transfers unless a stream's deadline is within 15 ms (urgent).
- **A chord's worst case:** 8 note-ons queue 8 first chunks behind one in flight: 9 × 6.66 ms = 60 ms at M1's max. A 100 ms head lasts 82 ms at TRON's worst speed (+3 semitones and 33 cents of wow, ×1.214). The second chunks are due 92 ms later.

### 5.5 Admission

A streamed note is billed in bytes a second at the **fastest speed it can reach**: `rate × 1.5 × 4,608 / 4,032 × ratio_max`. Here `ratio_max` covers the note against ROOT, the PITCH and FINE routes' depth, the bend range and 33 cents of wow.

- The budget defaults to 466 KB/s (60 % of M1's 778 KB/s) until bench B4 measures it; then CARD TEST sets it per card.
- At that default, 8 TRON notes at +3 semitones (399 KB/s) fit, and so do 8 beds at 1× (329 KB/s).
- **Over budget, the new note steals the oldest streamed voice** (released first, then held, through the allocator's fade). A note is never silently late. If nothing can be stolen, it is refused before allocation.
- The CPU bill gets a stream term for the span switching and slot checks, measured in slice 1 (estimated +20 cycles a sample).

### 5.6 Prefetch

- PLAY ONE and LOOP read forward; a LOOP's wrap jumps to the loop-start head, so nothing waits.
- TRON reads forward and stops at RUN.
- REEL knows the next step a whole step ahead (CHANCE rolled early, § 2.6) and asks for its first chunk with the current step's end as the deadline, so a program jump needs no head. Live SPLICE moves to a splice with no head are cold starts (§ 5.7). Backward steps read chunks in reverse; bench B7 sets their bill (expected 2×).

### 5.7 Failure behaviour

| What happens | What the player hears and sees |
|---|---|
| **Underrun** (a slot isn't ready when the head needs it) | A designed drop-out. The voice fades out over 3 ms before the missing material, keeps its tape moving in time, asks for the chunk at the head's new place, and fades in over 3 ms when it lands. `stats` counts `stream_underruns`. |
| **Cold start** (a start with no head: REWIND, a live SPLICE jump, a fill after a quiet) | Silent until its first chunk lands (≤ 60 ms at M1's worst), then a 3 ms fade in. Counted as `stream_cold`. |
| **A card write** (a save, MAKE, a render, RESAMPLE, `put`, CLEAN UP) while streams play | Decision 8, applied to every write. The UI asks for quiet; the audio fades every streamed voice out over 10 ms and keeps their tape moving; the card layer finishes its transfer and stops; the write runs; on release, every stream asks again from where it now is and fades in when its chunk lands. RAM voices and the other engines play on. With no streams playing, nothing fades. |
| **Read error** (CRC16, a token timeout of 100 ms, an R1 error) | One retry. A second failure faults the layer: every stream drops out as an underrun; new streamed notes are refused; TAPE pages read `CARD READ FAILED`. The UI re-inits the card on its next pass (`Medium::reinit`). On success the streams resume; if the volume changed (`CardChanged`), streamed sources read `SOURCE MISSING` until the project resolves them again. |
| **Card gone** (no detect pin; the card is inside the case) | As a read error. |
| **Bank 2 closed** | TAPE is silent and labelled (ADR 0141). The card layer still serves the UI. |

### 5.8 Not streamed in this release

SCRUB, STRETCH, WAVE, GRAIN and KIT (seeks); stereo or 16-bit sources (no such format); recording to the card while playing (writes quiet reads; no RAM for a write buffer).

## 6. RESAMPLE

Chimera feeds itself (decision 7): render a Part's Sound offline to a new `.SRC`, then PREP it.

- **Where:** SETTINGS › SAMPLES › RESAMPLE, a leaf page: PART, NOTE (default C3), VELOCITY, HOLD (0.1–8 s, the key held), TAIL (until silent, at most +4 s), RATE (48, 24 or 12 kHz). `MIX + PLUS · RENDER`.
- **What it renders:** one note through a scratch `Voice` built from the Part's Sound: engine, drive, filter, folder, VCA, the Sound's own modulation. Mono, pre-pan, no sends, no master. The render is deterministic (the voice is seeded as voice 0), so the second pass for writing matches the CRC pass bit for bit.
- **Decimation** to 24 or 12 kHz uses the existing half-bands (`dsp/halfband.rs`), once or twice. The take is normalised to −0.5 dBFS (the importer's rule), dithered (§ 3.4) and written. Then PREP opens on it, in TRIM.
- **Limits:** a TAPE Sound renders only if its source is held whole (no card reads during the render). A MODAL Sound renders with SYM off unless a scratch `SymPool` fits; slice 6 measures the scratch voice and pool against D2's free space.
- **Cost:** a 4 s take is about 2–3 s of UI time a pass, two passes, then the write (streams quiet for it).

## 7. Budgets

### 7.1 RAM

| Where | Today free | TAPE takes | Notes |
|---|---|---|---|
| SRAM4 (64 KiB, D3) | 65,536 (unused, not in `memory.x`) | 65,024: 8 rings | Slice 1 adds the region. CPU-only access, so DMA reach doesn't matter. |
| D2 SRAM1–3 | 86,240 | +8,192 for the larger DMA region (staging 9,216 inside it); ~2 KB stream registry; 16 KB PREP scratch; ~20–25 KB RESAMPLE voice (measured in slice 6) | ~35 KB left at the end |
| AXI | ~27 KB spendable | ~1 KB: per-voice stream state, counters, PREP's overview and marks | The linker's 32 KiB margin stays |
| Arena (153,600 B for sources) | per project | Heads: PLAY 7.2 KB a streamed source (two heads), a TRON set 43 KB (12 heads), a REEL up to 58 KB (16); KIT and REEL prep rows ≤ 1 KB each | On the meter |
| DTCM, ITCM | — | nothing | |

### 7.2 Flash

Bank 1 is nearly full; bank 2 holds TAPE's DSP and the reverb behind the door (ADR 0137, 0142) with about 390 KB free. Bank 1's free space is measured at each slice's start (`just size`); the ledgers put it at 51–76 KB.

| Part | Bank | Estimate |
|---|---|---|
| Card layer: protocol, DMA shell, arbiter, scheduler | 1 (must serve the UI with bank 2 closed) | ~5 KB |
| Stream registry, admission, slot protocol, new records' codecs | 1 | ~3 KB |
| Streamed read path, PLAY's MODE, KIT, REEL, TRON's page | 2 (TapeDeck) | ~12 KB |
| PREP mode, SAMPLES pages, transient detection, loop FIND, renders | 2 | ~35–45 KB |
| RESAMPLE | 2 | ~5 KB |

**PREP's UI in bank 2 needs its own ADR**, owed by slice 3: ADR 0137's door holds DSP today. The door gains a `PrepDoor` (open, draw, input, step). Closed, SAMPLES and PREP's doors read `NEEDS UPDATE` (ADR 0141's rule).

## 8. What stays from the TAPE core, and what this supersedes

**Stays (2026-10-04 core spec, ADR 0093):** the `.SRC` format, `SourceHead`, the CRC rule, 12-bit packing, the Hermite read and its 0.35 × rate cut-off, folding on pitch-up; the pool and the gate; ROOT and PERIOD copied into the Sound; `ZoneSet`, `SourceZones`, `ZoneTune`; SCRUB, STRETCH, TRANSWAVE as built; TAPE and AGE; WOW; END before START; the billing method and the bench rows; the host importer; `put` (ADR 0106); the factory set's rules; the names (`TapeEngine`, DECK, the chain).

**Superseded:**
- **The machine lineup** (ADR 0093 § Machines): ONESHOT, LOOP and PING-PONG fold into PLAY; KIT and REEL are new; ONESHOT's SLICE goes to KIT. (ADR 0145)
- **TRON plays `Plain` sources held whole** (core § 2 Machines): TRON now streams zone sets; RAM TRON stays possible where the source fits. (ADR 0145, 0147)
- **"Refill stays in the UI loop; rings in D2; 35 ms heads"** (core § 8): the DMA layer, rings in SRAM4, 100 ms heads by default. (ADR 0147)
- **"Dither is host code"** (core § 6): on-device renders dither too. WAV decode and sample-rate conversion stay on the host.
- **"No destructive editing on the device"** (feature table): still true. Renders write new files; nothing is overwritten.
- **The descriptor's rule** "`head_frames` only on a `ZoneSet`": a streamed `Plain` carries heads too (slice 1). The descriptor is RAM only, so no file changes.

**The master tape stage is renamed MTAPE** (owner, 2026-10-15). It reads **REEL** on screen today (ADR 0093: page short `REEL`, matrix `REL`); it becomes page short `MTAPE`, matrix `MTP`, so the REEL machine keeps its word. Labels only: no param ids or disk codes change. Done by slice 5 at the latest.

## 9. Slices

Each slice is its own plan. Each ends on the unit (hardware-checks-only-at-ship: host first, then Claude flashes and reads the unit over the console). Owner UAT is by ear.

| # | Slice | Builds | Done when (on the unit) |
|---|---|---|---|
| 0 | **Bench** | The real DMA read path and protocol (slice 1's driver, not a throwaway), driven by the console's `card` in `bench-stress,bank2`: B2 (access time vs chunk), B3 (8-read burst), B4 (sustained EDF streams, 10 min), B5 (write stall), B6 (UI under streams), B7 (reverse) | Raw console text for every bench is in the ledger. It fixes CHUNK_RECORDS, HEAD_MS and the stream budget, or shows a number in § 5 is wrong. |
| 1 | **Streaming core + PLAY** | The card layer in product code; `SharedCard`; SRAM4 rings; the slot protocol; EDF; admission; drop-outs; quiet for writes; fault handling; PLAY (the fold, MODE, FADE) streaming forward ONE and LOOP | 4 Parts each loop a 25–28 s bed (under `put`'s 1 MiB; longer files wait for slice 2) while Part 5 holds a 4-voice chord at ≥ 70 % load for 10 min: `stream_underruns` 0, `overruns` 0. A MENU-hold save in the middle fades the beds out and back. An injected read fault drops out, reads `CARD READ FAILED`, and recovers. |
| 2 | **TRON on zones** | PLAY takes ZONES; TRON's page and RUN; resumable `put` (its ADR, as ADR 0093 requires) or disk-mode copy for files over 1 MiB; host zone-set builder; 2–4 CC0 factory TRON sets; MOTOR and REWIND prototypes | 8-note chords of 8 s TRON notes over every zone, 10 min, 0 underruns; owner A/B of MOTOR and REWIND |
| 3 | **PREP + SAMPLES** | PREP mode (TRIM, LOOP, MAKE), its bank-2 door and ADR, the `.PRP` (RAW), renders, SETTINGS › SAMPLES, the catalog, CARD TEST, CLEAN UP (#398) | Trim and loop a 2-minute bed on the unit, MAKE a `.PRP` and a rendered `.SRC`, pick both on a Part, power-pull during a render and boot clean |
| 4 | **CUT + MAP + KIT** | CUT (grid, transient, hand), MAP (KIT and ZONES), the KIT machine, prep rows, `choke.rs` | A 64-slice break plays from C1–D#6 with black keys choking each other and white keys stacking, retriggered by ORBIT at 16ths for 10 min |
| 5 | **REEL** | Splices, programs with CHANCE, PREP's REEL page, backward and ping-pong streaming (PLAY's PING and reverse too) | The owner's `A → ×3 · A ← ×3 · LOOP` on a streamed 2-minute reel on 4 Parts, 0 underruns; a 50 % step skips about half the time |
| 6 | **RESAMPLE** | The scratch voice, decimation, dither, the two-pass write, opening PREP | Resample ALGO, VA and MODAL Sounds to 24 kHz takes that null against a desktop render of the same Sound (bit-exact before dither, same seed) |
| 7 | **Release gate** | TRON's verdict (MOTOR or fold), factory KITs and REELs, desktop QA of every machine, fixes #381, #423, #524 | Owner UAT; every bench row under its bill; the release notes |

Slices 0–7 are the release (release-needs-features).

## 10. Open questions for the owner

1. ~~The master tape stage reads REEL on screen.~~ Settled: the stage becomes `MTAPE` (matrix `MTP`).
2. ~~Over the stream budget.~~ Settled: a new note steals the oldest streamed voice.
3. ~~Fade on every write.~~ Settled: every card write while streams play fades them (save, library save, `put`, CLEAN UP, renders).
4. TRON's keep: MOTOR (cheap, recommended) or REWIND (each replay is a cold start, up to ~60 ms late)?
5. RESAMPLE renders one note of a Part's Sound. Do you also want a held chord, or an ORBIT phrase, in this release?

Settled here by precedent or the code, not asked: the `.PRP` is content-named and write-once (§ 3.2); KIT has 4 choke groups; REEL's CHANCE is rolled a step early; streams are 12-bit mono only; streaming is forward-only until REEL.
