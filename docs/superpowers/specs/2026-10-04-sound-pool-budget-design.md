# Sound pool by memory budget: variable-size Sounds and sources, one meter

Status: draft for owner review (2026-10-04). Issue: private issue #337. ADR 0087 (Proposed) records the decision. It supersedes, in part, ADR 0043's fixed 32-slot `Pool`. TAPE (private issue #279) builds on it. The factory library (private issue #317) fills it.

## Intent

The owner, 2026-10-03: the project's sound pool is budgeted by **memory**, not by 32 slots, as on the Polyend Play. The rulings this spec builds on:

- **One byte budget** holds the project's Sounds and its TAPE sample data together. TAPE's separate sample pool from #279 merges into it.
- **There is one meter**, a single bar like the Play's.
- **Sounds are stored encoded and variable-size.** A Sound is decoded only when a Part loads it. Library → pool → Part stays a copy at each step (ADR 0043).
- **TAPE sources are stored S950-style:** 12-bit, with a sample rate per source.
- **NEW fills its pool with 32 curated built-in factory Sounds.** About 250 more live in the card library (#317).
- **No users yet.** Old projects must load safely; they need not sound identical.
- **Few controls.** Every MIX or hold gesture gets a contextual focus-band hint (ADR 0086).
- **Never snap parameters, and never allocate in the audio thread.**

## What exists today (measured 2026-10-04)

| Thing | Bytes | Where it comes from |
|---|---|---|
| `Option<Sound>` in RAM | **896** | `memory_budget_test::axi_counts_the_project`: 32 slots = 28 672 B |
| `Project` (6-Part `Performance` 5 672 B + the 32 slots + meta) | 34 464 | same test |
| A Sound as card records today (`encode_sound`, every block of both engines) | **1 127** records + 16 name = **1 143** | the `tests/fixtures/v1/*.snd` files, 1 159 B with the 28 B header and the 4 B CRC |
| The same Sounds, encoding only params that differ from `Sound::neutral` | **153–273, mean 211** (INIT: 58) | the 8 factory fixtures, re-encoded by hand (§ Sparse encoding) |
| Worst case, sparse (all 198 params, a full registry, 128 routes) | 1 823 | § Sparse encoding |
| AXI really used (`.data` + `.bss`) | 375 564 of 524 288; **148 724 free** | `rust-size -A` on the latest release ELF |
| AXI by the accounting model (`AXI_RESIDENT`) | 433 712; 90 576 spare (64 KB required) | `axi_residents_fit` |
| D2 used (`.ram_d2` + `.ram_d2_dma`) | 168 364 of 294 912; **126 548 free** | `rust-size`; earmarked for TAPE streaming and VA |
| `SymPool` (D2) | 112 200 | `sympathetic_pool_fits_d2` |
| Flash used (vectors + `.text` + `.rodata` + `.data`) | 747 144 of 917 504; **170 360 free** | `rust-size` |

Two findings shape the design:

1. **Today's encoding is bigger than the RAM struct.** `encode_sound` writes all 19 voice blocks, MODAL's on an ALGO Sound and ALGO's on a MODAL Sound, at 5 B a param. So "store the Sounds encoded" saves nothing until the encoding is sparse. Sparse encoding is the first step of the build (§ Build order).
2. **The fixed pool lives in `Project`, in AXI, behind `UiState`.** The audio thread never reads it: Parts carry decoded Sounds, which reach the audio through `AudioShared` (ADR 0021). Sources change that, because a TAPE voice must read sample bytes from the pool (§ How a Part plays).

## Prior art, and what we take

| Instrument | What it does | What we take |
|---|---|---|
| **Polyend Play** | "On the bottom of the Main menu Samples screen, you can see a Memory bar that indicates the remaining sample pool space percentage." Up to 255 samples per project, in about 6 minutes of 16-bit 44.1 kHz mono. "Delete Unused Samples" removes samples not placed on the grid. "A sample played back at half the sample rate will use half the Pool memory." (Play manual, § Samples, § Sample Pool.) | One bar with a percentage, in the browser. A count cap well above what the bytes allow. A purge of unused entries. A lower sample rate costs proportionally less. |
| **Polyend Tracker** | The Sample Loader's memory bar previews the highlighted file: a red segment for the memory it would add, green for what it would free. "Delete unused" purges instruments the project doesn't use. Per-project memory is about 133 s of mono; the low-quality import doubles it. (Tracker manual v1.5, § Sample Loader, § Instrument.) | **The preview segment:** the bar shows what the highlighted library Sound would cost before you load it. |
| **Elektron Digitakt** | 64 MB of sample RAM per project. SETTINGS › SAMPLES › VIEW RAM › SELECT UNUSED › UNLOAD removes samples no pattern uses from RAM. They stay on the +Drive. (Digitakt manual, § Samples.) | Purge is a selection, then a confirm. It never deletes from the card. |
| **Elektron Octatrack** | 85.5 MB of RAM, shared between Flex samples and the track recorders. Every sample in the slot list counts "whether they are used or not". PURGE SAMPLES drops the samples no trig uses from the slot lists, not from the card. A load that won't fit is refused with a memory-full error. (Octatrack manual, § Project › Samples, § Memory.) | A refusal names the shortfall. Purge leaves the card alone. Holding an unused entry costs the same as using it. |
| **Akai S950** | 12-bit samples, sample rate chosen per sample, from 7.5 to 48 kHz. Standard memory 750 KB, about 63 s at the lowest rate. (S950 specifications; S900 history.) | 12-bit storage with a rate per source. The rate is the user's quality-for-time trade. |

Sources:
- Polyend Play manual: https://polyend.com/manuals/play/
- Polyend Tracker manual v1.5: https://files.kraftmusic.com/media/ownersmanual/Polyend_Tracker_Manual_v1_5.pdf
- Digitakt purge: the Elektronauts threads "How to delete ONLY unused samples from +Drive?" and "How To: Remove Samples from Project?", which quote the manual's VIEW RAM › SELECT UNUSED › UNLOAD path
- Octatrack manual OS 1.25: http://www.elektron.co.jp/wp-content/uploads/2015/01/octatrack_manual_OS1.25.pdf, and the SampleStack guide: https://samplestack.app/news/octatrack-sample-management-guide/
- Akai S950: https://www.vintagedigital.com.au/akai-s950/ and https://en.wikipedia.org/wiki/Akai_S900

## The numbers

**The budget: `ARENA_BYTES` = 96 KiB (98 304 B), in AXI.** It replaces the 28 672 B of fixed slots.

| | Bytes |
|---|---|
| Arena | 98 304 |
| Index (128 Sound entries × 12 B + 32 source entries × 20 B) | 2 176 |
| `SourceTable` the audio reads (32 × 24 B) | 768 |
| `Project` after (Performance + index + meta) | ≈ 8 000, from 34 464 |
| AXI really free after | 148 724 − 98 304 − 768 + 26 464 ≈ **76 100** |
| `AXI_RESIDENT` spare after, with `UI_RESERVE` 64 → 16 KiB | ≈ **67 100** (rule: ≥ 64 KiB) |

`UI_RESERVE` can shrink because what it reserves for, `UiState` without its `Project`, measures 3 608 B, and `ui_state_fits_the_ui_reserve` already caps that at 4 KiB. A 128 KiB arena would leave about 34 KiB of model spare and break the 64 KiB rule (open question 1).

**What fits.** A Sound costs its sparse encoding plus its 16 B name, rounded up to 16 B. The curated set is budgeted at 320 B a Sound, above the measured 211 B mean, because curated Sounds touch more params than the 8 test Sounds. NEW's 32 then take **10 240 B, 10 %**, and leave **88 064 B** for sources:

| Source rate | 12-bit bytes/s | Seconds in 88 064 B | 16-bit, for comparison |
|---|---|---|---|
| 48 kHz | 72 000 | **1.2 s** | 0.9 s |
| 32 kHz | 48 000 | **1.8 s** | 1.4 s |
| 24 kHz | 36 000 | **2.4 s** | 1.8 s |
| 16 kHz | 24 000 | **3.7 s** | 2.8 s |
| 12 kHz | 18 000 | **4.9 s** | 3.7 s |
| 8 kHz | 12 000 | **7.3 s** | 5.5 s |

12-bit storage buys a third more time than 16-bit at every rate. With no Sounds in the pool at all, 24 kHz gives 2.7 s. In Sounds alone, 96 KiB holds about 300 curated Sounds, so the 128-slot cap (§ The model) binds before the bytes do, unless sources are loaded.

#279's estimate of 1–2 s per microsample source at 24 kHz fits: NEW plus one 2 s source at 24 kHz reads **84 %**.

## The model

```rust
pub const MAX_SOUNDS: usize = 128;   // Sound slots: was POOL_SIZE = 32
pub const MAX_SOURCES: usize = 32;   // source slots
pub const ARENA_BYTES: usize = 96 * 1024;
pub const GRANULE: usize = 16;

pub struct SlotId(u8);    // < MAX_SOUNDS; built only by new / ALL, as today
pub struct SourceId(u8);  // < MAX_SOURCES; the same pattern

/// Arena bytes, always a multiple of GRANULE: what the meter adds up.
pub struct Cost(u32);

pub struct Pool {
    sounds: [Option<SoundEntry>; MAX_SOUNDS],
    sound_gens: [u16; MAX_SOUNDS],
    sources: [Option<SourceEntry>; MAX_SOURCES],
    source_gens: [u16; MAX_SOURCES],
    arena: ArenaWriter,      // the only writer of the arena's bytes
}
struct SoundEntry  { offset: u32, len: u16, engine: EngineType, crc: u32 }       // 12 B
struct SourceEntry { offset: u32, frames: u32, rate: Hz, refs: u8, crc: u32 }    // 20 B
```

- **Slots stay numbered and stable**, as the projects spec required for AFX. There are now 128 of them. Empty slots are free rows. The bytes are the real limit, and the count cap only bounds the index. The Play caps at 255 samples and the Digitakt at 128 slots, so 128 is in line.
- **Generations are kept** exactly as ADR 0043 has them, one `u16` per Sound slot and per source slot. Every store and clear moves one.
- **`Pool::get(SlotId) -> Option<EncodedSound<'_>>`** replaces `Option<&Sound>`. An `EncodedSound` is a borrowed view: its name (the first 16 B), its engine, its `Cost`, its CRC, and a `decode_into(&mut Sound)`. Nothing outside `project` sees the arena's offsets.
- **`Cost` is a newtype.** `Pool::used() -> Cost`, `Pool::free() -> Cost`, and `Pool::fits(Cost) -> bool` are the only arithmetic. The meter, the refusals and the purge prompt all read them.

## Memory layout and allocator

```
 POOL_ARENA (96 KiB, AXI)
 0                                                              ARENA_BYTES
 | src 0 | src 3 | src 1 |  ......... free gap .........  | snd 7 | snd 0 | snd 2 |
 ^ sources grow up from 0                       Sounds grow down from the top ^
```

- **Two ends, one gap.** Sources are packed from offset 0 upward, Sounds from the top downward. Free space is exactly the gap between them.
- **No holes at rest.** Every delete compacts its own end at once, on the UI thread. So `free() == gap` always holds outside a running move, a load fits if and only if `cost ≤ gap`, and fragmentation never builds up. A test asserts the invariant after every operation (§ Test plan).
- **Why two ends.** Sounds and sources differ in who reads them:
  - The audio thread never reads a Sound's bytes. Compacting the Sound end is a plain `copy_within` on the UI thread, with no protocol.
  - The audio thread does read source bytes. Compacting the source end needs the move protocol below. Keeping the ends apart means a Sound delete never moves a source, and a source delete never moves a Sound.
- **The granule is 16 B.** Each entry starts on a 16 B boundary, so a source's data starts aligned, and the rounding wastes 8 B per entry on average.
- **Where the bytes live.** `POOL_ARENA` is one AXI static of `ARENA_BYTES`. The desktop and the tests take a boxed one. The UI holds the only `ArenaWriter`, built once from the static. The audio thread holds a `SourceReader`, which can read only through the `SourceTable` (§ How a Part plays). Both wrap a raw pointer. Their `// SAFETY:` argument is the move protocol: the audio thread reads only bytes the table publishes as live, and the UI writes those bytes only under the protocol.
- **Staged writes go into the gap.** An add writes its bytes into the gap, verifies them (a CRC for a source, a decode for a Sound), and only then takes an index entry. The meter doesn't move until then. A failed or cancelled add leaves the gap as it was.

### Moving a source the audio may be reading

A source delete slides every source above it down by the deleted cost `g`. That moves bytes a voice may be reading. Each source being moved has a `SourceTable` row that the UI updates:

```rust
pub struct SourceView {             // all AtomicU32; UI writes, audio reads
    base: AtomicU32,                // where the source now starts
    from: AtomicU32,                // where it started before this move
    done: AtomicU32,                // bytes already at `base`, a multiple of 3
    len:  AtomicU32,                // data bytes
    rate: AtomicU32,                // Hz
}
```

- **The UI copies front to back, in chunks of at most `g` bytes**, each a multiple of 3 (one 12-bit pair is 3 B, § 12-bit packing). After each chunk it stores `done` with `Release`.
- **The audio resolves a pair's address once per read:** byte `i` (pair-aligned) is at `base + i` if `i < done`, otherwise at `from + i`. It loads the row with `Acquire` at the start of each block, and the UI can't run inside a block.
- **Why it is safe, even if audio interrupts the UI mid-chunk.** Chunk `k` covers bytes `[k·c, (k+1)·c)`. Its writes land at `base + [k·c, (k+1)·c)`, which is `from + [k·c − g, (k+1)·c − g)`. With `c ≤ g`, every byte written has index below `k·c`, the `done` the audio last saw. The audio reads those bytes at `base`, where they are already complete. Bytes at or above `done` are read at `from`, and no write has reached them yet.
- **Cost.** Moving all 88 KB of sources is one `memmove`'s worth of work, well under a millisecond at 480 MHz. The UI does it in one pass, so there is no partial state across passes.
- `g ≥ GRANULE` (16 B), so each chunk is at least 15 B, and in practice 4 KiB.

### Retiring a source

A source a Part still uses can't be deleted (`InUse`, as `pool_clear` today). A source no Part uses can still be sounding: a ringing voice from before the Part's SOURCE changed. So a delete is two steps:

1. The UI marks the row `retired`. From then on, no new voice may start on it, and no Part can reach it, since no Part references it.
2. The audio publishes a per-block `sounding: u32`, one bit per source, beside `AudioStats`. The UI frees the bytes and compacts only after a block reports the bit clear, and after the audio acknowledges an epoch at or after the retirement (the `Swap::ack` pattern, ADR 0046).

If the note is held, the source stays, and PURGE reports `1 SOURCE STILL SOUNDING` (§ When the budget is full). The audio never cuts a note, and nothing snaps.

## Sparse encoding

`encode_sound` becomes **sparse**: a `Block` record carries only the params whose disk value (the `f32` bits, or the enum code) differs from `Sound::neutral(engine)`'s. A block with none is omitted. `Engine`, `Registry`, `ModDests` and `Routes` are written as today.

- **No format change, and today's decoder already reads it.** `decode_block` writes only the ids it is given, onto a `Sound::neutral` base, and an absent block keeps neutral's values. `neutral` is frozen (`neutral_v1.txt`). So a sparse file and a full file of the same Sound decode to the same bits.
- **Canonical.** The bytes depend only on the Sound's bits, so two Sounds are bit-equal exactly when their encodings are byte-equal (§ Test plan). `sound_crc` stays the CRC of the padded name and the records. Its values change once, which is safe, because no CRC is stored on the card (`Origin` stores the slot; ADR 0043 recomputes `Origin.crc` at load).
- **Sizes.** The 8 factory fixtures go from 1 143 B to 153–273 B. INIT goes to 58 B. The bound is 16 (name) + 5 (`Engine`) + 19 × 5 (block heads) + 198 × 5 (params) + 164 (`Registry`) + 37 (`ModDests`) + 516 (`Routes`) = **1 823 B**, which rounds to a `Cost` of 1 824. `MAX_SOUND_COST` is asserted against it.
- **The same encoder writes everywhere:** the arena, the project file's `Slot` and `Part` records, library `.snd` files and the factory blob. One encoder, one canonical form.

## How a Part plays from an encoded Sound

- **A Part decodes on load into its own `Sound`, as today.** `Performance` keeps six decoded `Sound`s (896 B each). `replace_part` decodes the slot's bytes into the Part, in place, on the UI thread. That takes about 200 B through `SoundCheck`, a few microseconds. Parts are then published through `AudioShared` as now. The audio thread never decodes anything.
- **Edits touch only the Part.** No edit reads or writes the arena, so editing can never fail on the budget.
- **Marks compare encodings.** `part_status` encodes the Part's Sound through a comparing sink and checks it against the slot's bytes: `Clean` when they are byte-equal. This replaces `bits_eq` against a decoded slot, and needs no buffer. `project_crc` hashes slot bytes directly, which is cheaper than re-encoding 32 Sounds.
- **A TAPE Part does not copy its source.** A TAPE Sound's records include `SourceRef { crc: u32, frames: u32, rate: u16 }`. When a Part loads it, the ref resolves to the pool's `SourceId` whose entry matches all three fields. The Part keeps that `SourceId` and its generation. `AudioShared` carries the `SourceId`, and the voice reads through the `SourceTable`.
  - **Sharing an immutable source can't be told apart from copying it,** so ADR 0043's copy-at-each-step rule holds for what a user can observe. The one observable difference, deletion, is guarded by `InUse` and by retirement.
  - **Sources are never edited in place.** #279's PROCESS step writes a new source, which matches its rule that a process never overwrites its input.
- **An unresolved ref is not an error.** The Part loads, and its TAPE voice plays silence. Its TAPE page shows `SOURCE MISSING`.
- **`refs` counts users.** It counts the pool Sounds and the Parts that resolve to a source. It is recomputed after every pool or Part change, never edited by hand: a `fn refs(&Project, SourceId)`, as `users(SlotId)` is today. Purge reads it.

## 12-bit packing

- **Two samples in three bytes**, little-endian: `b0 = s0[7:0]`, `b1 = s0[11:8] | s1[3:0] << 4`, `b2 = s1[11:4]`. The values are two's complement, 12 bits. An odd frame count pads the last pair with 0. A source of `n` frames takes `3·⌈n/2⌉` data bytes, after its 16 B name.
- **The audio reads** by sign-extending `(x << 20) >> 20` on an `i32` and scaling by `1/2048`. Unaligned byte loads are free on the Cortex-M7, so no wider alignment is needed.
- **Import quantises with TPDF dither**, offline, on the UI thread. That covers a WAV import, a RESAMPLE render and a PROCESS step (#279). Dither keeps 12-bit hiss benign; truncation gives the S900-style grit only on purpose, through a later AGE.
- **A rate per source:** `rate` in Hz, 4 000–48 000. The UI offers 48, 32, 24, 16, 12 and 8 kHz, with 24 kHz as the default, the S950-style quality-for-time choice. The importer band-limits to 0.45 × rate before decimating. The voice steps through the source at `rate / 48 000 × pitch ratio`.

## The meter, and the UI

**One bar.** It sits on the Sound browser's info line, where `2 INIT + 32 SLOTS` is now, and on SETTINGS › PROJECT › POOL.

```
 32 SOUNDS · 2 SRC                          64%
 ███████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░
 Sounds        sources      (preview)     free
```

- **Geometry.** The bar is 216 px wide (the screen less its margins, as ADR 0086 measures hints) and 4 px tall.
- **Colours.** Sounds fill in `INK`, sources in `MID`, and free is `BAR_REST`. Two tones keep it one bar while showing the breakdown, which the issue asked for.
- **What it counts:** arena bytes in use, `Pool::used()`, out of `ARENA_BYTES`. That is entry costs only. The index, the Parts and the `SourceTable` are fixed and never move, so the meter leaves them out.
- **The percentage** is `⌈used × 100 / ARENA_BYTES⌉`. Rounding up means the bar never reads less full than it is, and a pool with one byte left reads 100 %. An empty pool reads 0 %.
- **The preview.** On a library row, or on a source in a TAPE Part's SOURCE list, the bar adds that entry's cost as an `ACCENT` segment after the used part, as the Tracker previews a load. If the entry won't fit, the segment is drawn in `ALERT` from the used part to the end of the bar, and the focus band reads the shortfall (§ When the budget is full). A source the pool already holds (same CRC) previews as 0.
- **Rows show their cost** on the right, as `0.3K` or `41K`: kibibytes, one decimal place under 10 KiB, rounded up.
- **SETTINGS › PROJECT › POOL** is a leaf page (ADR 0066: one page). It has the bar, then two lines, `SOUNDS 12 · 9.8K` and `SOURCES 2 · 52K`, then `FREE 34K`. It is read-only.
- **SETTINGS › PROJECT › PURGE UNUSED** is an `Act` row. EDIT opens a prompt (ADR 0066): `PURGE 3 SOURCES · 27 SOUNDS? FREES 61K`, with PURGE / CANCEL: SEQ confirms and MENU cancels, as in every prompt (ADR 0081). It is not a MIX gesture, so it needs no hint.

**Hints (ADR 0086).** This design adds no new MIX or hold gesture. It amends the text of two existing browser hints so they say what a gesture does to the budget:

| Where | Touched | Hint |
|---|---|---|
| Sound browser, filled slot | always | `MIX + MINUS CLEARS · FREES 0.3K` |
| Sound browser, Part's own row | always | `MIX + EDIT SAVES · +0.1K` (the save's cost against the slot it replaces, or `WON'T FIT` in `ALERT`) |

## When the budget is full

- **An add that won't fit is refused before anything is read or written.** That covers library → pool, a save to a new slot, a save over a slot that grows, and a source import. The cost is known up front:
  - a library Sound's from its file size (sparse files are exact; a legacy full file over-estimates);
  - a save's from encoding the Part through a counting sink;
  - a source's from its frames.

  The focus band shows `POOL FULL · NEEDS 41K, 12K FREE`, in `ALERT`, until the next input (ADR 0017's convention). Nothing changes.
- **Purge unused** removes:
  - every source with `refs == 0`;
  - every Sound slot no Part's `Origin` names (`users(s).is_empty()`, as today), unless open question 2 rules otherwise.

  It deletes nothing from the card. A source still sounding is skipped, and the prompt's result says so. Purge marks the project `Modified`, as any pool change does.
- **Nothing else refuses.** Editing a Part, loading a slot into a Part, and changing a Part's SOURCE among the pool's sources cost no arena bytes.

## Interactions

### Part copies and ADR 0043's guards

- `Origin::Slot { slot, generation, crc }`, `part_status`, `part_actions`, `ReplaceGuard` and the witnesses are unchanged in meaning. `SlotId`'s range grows to 128.
- `NEW SLOT` is offered when a slot is free **and** the Part's Sound fits. `SAVE OVER SLOT` is offered when the growth fits. An offer that can't apply isn't made (ADR 0043's rule), so a full pool shows `POOL FULL` in place of the action.
- `ReplaceError::SlotEmpty` stays. A `Pending` whose slot was purged in the meantime is refused as today.

### Project format and versioning

- **`FORMAT_VERSION` stays 1.** The TLV records and the must-understand bit (ADR 0045) carry the change.
- **`Slot` (0x8007)** keeps its shape. Its index range becomes 0..128. Its Sound is written sparse.
- **`Part` (0x8008)** writes its Sound sparse. That is about 300 B in place of 1 143.
- **New records**, numbered after ORBIT's 0x0010–0x0014. The numbers are proposed here, checked against `RecordTag` at implementation time, and frozen once written:

  | Tag | Record | Payload |
  |---|---|---|
  | `0x8015` | `Source` | source `u8` (0..32), its name (16 B), `rate` `u16`, `frames` `u32`, `crc` `u32`; opens a context |
  | `0x0016` | `SourceData` | up to 504 B of packed pairs (168 pairs), in order; only inside a `Source` context |
  | `0x0017` | `SourceRef` | `crc` `u32`, `frames` `u32`, `rate` `u16`; inside a TAPE Sound |

- **Order:** `Fx`, then the `Source`s in slot order, then the `Slot`s, then the `Part`s, then ORBIT's records (ADR 0078). Sources come first, so a Sound's `SourceRef` resolves during the same pass.
- **CRC.** A `Source` whose data CRC mismatches its record is dropped, as if absent. The file's own CRC already covers bit rot, so this catches only an encoder bug.
- **Loading.** The load is two passes (ADR 0045/0046). Pass 1 adds up every entry's cost. Pass 2 runs after the epoch fade, so no voice reads the arena while it is rebuilt.
  - **Old projects** (32 full-encoded `Slot`s) load and re-encode sparse. They sound the same, with no extra work.
  - **A project bigger than the budget** loads in priority order: the sources a Part resolves to; then Sound slots in slot order, skipping any that won't fit; then the remaining sources in slot order. The note reads `LOADED · 4 SOUNDS, 1 SRC DIDN'T FIT`. The project reads `Modified`, because its CRC no longer matches the file's, and a later save drops what was skipped. The file on the card is untouched until then. Such a file can only come from a hand-made file or a future build with a bigger arena, so this is the "loads safely" rule, not a workflow.
- **A project load fails as today** (a torn file, a card pulled mid-pass 2), and falls back to NEW (`reset_new`).

### The card library

- **Library Sounds** (`/CHIMERA/SOUNDS`, #317) are written sparse. A row's cost is its file size − 28 (header) − 4 (CRC) + 16 (name), rounded up to 16. It is read from the listing, with no extra card I/O.
- **Library sources** live in a new `Dir::Tapes` (`/CHIMERA/TAPES`), as `FileKind` 6 `Source` files: a header, then one `Source` record and its `SourceData`. The 8.3 stem is the source's CRC in hex (`1A2B3C4D.A`), so a TAPE Sound's `SourceRef` finds its file by name, with no index. Identical sources share one file. ADR 0045's A/B pair is kept for uniformity.
- **Adding a library TAPE Sound** brings its source too, unless the pool already holds it (same `crc`, `frames` and `rate`). The preview and the refusal count both. A source missing from the card refuses the add: `SOURCE NOT ON CARD`.
- **Purge never touches the card.** That is the Octatrack and Digitakt behaviour.

### The factory set

- **NEW's 32 curated Sounds are one encoded blob in flash** (`factory.bin`, about 10 KB against 170 KB free). A desktop tool generates it from the authored Sounds (#317), and a test decodes every entry and checks its bits against the authored form. NEW copies the blob's entries into the arena **without decoding**, since they are already canonical, so `fill_new_pool` is a `memcpy` and 32 index writes.
- **NEW reads about 10 %** on the meter.
- **`TemplateCrc`** is computed at run time from the arena as filled, as today. No CRC is pinned.
- **ADR 0073's `NEW_PARTS`** (Parts 1–5 load named factory slots) carries over, with slot numbers from #317's curated list.
- **Factory TAPE Sounds** need their sources in flash or on the card (open question 3). Until that is ruled, the 32 are ALGO and MODAL only.

## Failure modes

| Case | What happens |
|---|---|
| **No card at boot** | NEW from flash, 32 Sounds, 10 %. The library rows read `NO CARD`. Every pool operation that needs no card still works. |
| **Card pulled during a library add** | The staged bytes in the gap are abandoned. No index entry is written, and the meter is unchanged. The note reads `CARD READ FAILED`. |
| **Card swapped during an add** | `Card::run`'s volume check refuses it (ADR 0043/0046), with the same outcome as a pulled card. |
| **Slow card (stalls)** | A source add reads in 4 KiB steps, one per UI pass, behind the busy indicator (`ui::busy`). At #279's measured 1.3 MB/s, 88 KB is about 70 ms of card time. Under full audio load, with UI passes of 22–26 ms (the 2026-10-02 stress test), it takes about 0.6 s on screen. MENU cancels the add, and the staged bytes are dropped. The audio is never involved. |
| **A project bigger than the budget** | It loads in priority order, with a note (§ Project format). |
| **A Part's source didn't load** | The Part loads. Its TAPE voice is silent, and its page reads `SOURCE MISSING`. |
| **A source still sounding at purge** | It is skipped and reported. Purge again once the note ends. |
| **A corrupt entry in a project file** | Handled as today: `Corrupt` or `Bounds` fails the load, which falls back to NEW. A `Source` with a bad data CRC is dropped alone. |

## Test plan

All host-side (`just test`), before any hardware. Hardware checks wait for the ship flash.

**Encoding**
- `sparse_round_trip`: for every factory Sound, both INITs and 1 000 seeded random Sounds (an xorshift over each param's spec range, routes and registry), `decode(encode(s))` is `bits_eq` to `s`.
- `sparse_is_canonical`: re-encoding a decoded full-encoded v1 fixture gives the same bytes as encoding the original. Two Sounds are byte-equal exactly when they are `bits_eq` (over the same random set, plus single-param mutations).
- `v1_fixtures_still_load`: every `fixtures/v1` file decodes to the same bits as before.
- `sparse_bound`: a Sound with every param off-neutral, a full registry and 128 routes encodes to at most `MAX_SOUND_COST`.

**Arena and allocator**
- A model test drives 10 000 seeded add, delete and replace operations on Sounds and sources. After each one it checks:
  - entries are disjoint and granule-aligned;
  - sources sit contiguously from 0, and Sounds contiguously to the top;
  - `free() == gap`;
  - every entry's bytes are unchanged by moves (a CRC per entry);
  - `used()` is the sum of costs.
- `move_protocol_reads_never_tear`: a source is moved with a simulated audio reader. The reader takes the `done`, `from` and `base` snapshot at every chunk boundary, and at every byte inside a chunk, by splitting the copy. Every read returns the original pair. It runs with `g` = 16, 48 and 4 096 and odd lengths.
- `retire_waits_for_silence`: a retired source's bytes are not reused while `sounding` has its bit set or before the ack.
- `memory_budget_test` gains `pool_arena_fits_axi` and updates `axi_residents_fit` (arena + index + `SourceTable`; `UI_RESERVE` 16 KiB; spare ≥ 64 KiB) and `axi_counts_the_project`.

**12-bit**
- `pack_unpack_all_values`: all 4 096 × 4 096 pairs round-trip, about 16 M iterations, in under a second in release, or a seeded 100 000 in debug.
- Odd frame counts pad, and the last real sample survives.
- Dither: a −60 dBFS sine quantised to 12 bits keeps its spectral peak, and its noise floor is flat within 3 dB across the band.

**Meter and UI**
- `meter_percent`: 0 B → 0 %, 1 B → 1 %, `ARENA_BYTES − 1` → 100 %, and exactly half → 50 %.
- Renderer snapshots (`docs/screens`): the browser bar at NEW, at 64 %, with a fitting preview and with a refused preview, and the POOL leaf.
- `hint_fits`: every amended hint within 216 px (ADR 0086's test).
- The refusal leaves the project byte-identical (`project_crc` unchanged).

**Project and library**
- An old 32-slot project file loads and sounds the same: the six Parts render bit-identical audio to before the change, through the existing golden harness.
- An over-budget fixture loads in priority order, and the note names the counts.
- A project with a source round-trips save → load byte-equal.
- A TAPE Sound's `SourceRef` resolves after a load; a missing one leaves the Part silent, with no panic.
- `purge_unused`: frees exactly the unreferenced entries and never touches the card (`MemStore` write count 0).
- NEW: 32 Sounds, about 10 %, `Pristine`, with every factory entry decode-equal to its authored form.

**Desktop QA** (the QA-before-UAT rule): sweep the browser and purge on the simulator, add and delete sources while a TAPE voice loops through them under the move protocol, and check by recording that the audio has no discontinuity at a move. The last waits for TAPE's first voice; until then the move test runs on a stub reader.

## Build order

Each step ships on its own, and the meter is right at each.

1. **Sparse `encode_sound`.** It is a writer change only. Files, projects and the library shrink. Add the encoding tests and `MAX_SOUND_COST`.
2. **The arena and the index, Sounds only.**
   - `Pool` changes over: 128 `SlotId`s, `EncodedSound` views, the Sound end of the arena with compaction, and marks compared by encoding.
   - The project codec's `Slot` range grows.
   - `UI_RESERVE` drops to 16 KiB, the AXI static and budget tests are updated, and the 28 KB of fixed slots go.
3. **The meter and the refusals:** the browser bar, row costs, the preview, the `POOL FULL` refusal, the amended hints, POOL and PURGE UNUSED under SETTINGS › PROJECT.
4. **NEW from the factory blob,** once #317's 32 are curated. Until then, the blob holds today's 8 and the two INITs, and the meter reads about 2 %.
5. **Sources,** with or just before the TAPE core (#279):
   - `SourceId`, the source end, the move and retire protocols, the `SourceTable`, 12-bit packing and the importer;
   - `Source` / `SourceData` / `SourceRef` records, `Dir::Tapes`;
   - the over-budget load order.

This follows the agreed roadmap: ORBIT unit QA → this → the TAPE core. Step 2 merges after ORBIT, since both touch the project codec and `template.rs` (ORBIT's ADR 0073 and 0078).

## Open questions for the owner

1. **Budget size: 96 KiB or 128 KiB?** 96 KiB keeps the 64 KiB AXI spare rule and gives 2.4 s of 24 kHz audio beside NEW's 32 Sounds. 128 KiB gives 3.4 s, and the spare drops to about 34 KiB.
2. **Does PURGE UNUSED take Sounds too, or sources only?** Sounds-too frees about 9 KB on NEW, but wipes the 27 curated Sounds no Part uses. Sources-only frees the bytes that matter and keeps the palette. This spec does Sounds too, behind the prompt. The Play, Digitakt and Octatrack purge samples only.
3. **Factory TAPE Sounds:** should any source live in flash (24 KB per second at 16 kHz, from 170 KB free), or should TAPE factory Sounds be card-only, so a unit with no card has none?
4. **Is 24 kHz the right default import rate?** 16 kHz gives half again as much time and matches #279's ~10 kHz "tape top".
5. **Meter text:** is `32 SOUNDS · 2 SRC   64%` what you want on the browser line, or the percentage alone, as on the Play?
