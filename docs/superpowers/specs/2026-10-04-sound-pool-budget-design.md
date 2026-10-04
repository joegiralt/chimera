# Sound pool by memory budget: sparse Sounds, card-backed sources, one meter

Status: draft for owner review (2026-10-04), revised after an adversarial review the same day. The arena size depends on ADR 0090 (the master delay line as `i16`, owner ruling 2026-10-04). Issue: private issue #337. ADR 0087 (Proposed) records the decision. It supersedes ADR 0043's fixed 32-slot `Pool` in part, and ADR 0014's AXI accounting in part. TAPE (private issue #279) builds on it. The factory library (private issue #317) fills it.

## Intent

The owner, 2026-10-03: the project's sound pool is budgeted by **memory**, not by 32 slots, as on the Polyend Play. The rulings this spec builds on:

- **One byte budget** covers the project's Sounds and the TAPE sample data held in RAM. TAPE's separate sample pool from #279 merges into it.
- **One meter**, a single bar like the Play's.
- **Sounds are stored encoded and variable-size.** A Sound is decoded only when a Part loads it. Library → pool → Part stays a copy at each step (ADR 0043).
- **TAPE sources are stored S950-style:** 12-bit, with a rate per source.
- **NEW fills its pool with 32 curated built-in factory Sounds.** About 250 more live in the card library (#317).
- **No users yet.** Old projects must load safely; they need not sound identical.
- **Few controls.** Every MIX or hold gesture gets a contextual focus-band hint (ADR 0086).
- **Never snap parameters, and never allocate in the audio thread.**

**The frame this revision adds.** RAM can't hold TAPE's sources; the card can. Even with the master delay line moved to `i16` (owner ruling 2026-10-04, ADR 0090), the honest ceiling for the arena is about 214 KB (209 KiB) on the shipping build. That is 5.9 s of 24 kHz 12-bit audio for the whole project, which would make a RAM-only TAPE a one-source engine. So:

- **Every source lives on the card**, written once, under `/CHIMERA/TAPES`.
- **The arena holds, per source, either the whole of a short source** (a microsample, for the random-access models: TRANSWAVE, LOOP, GRAIN, SCRUB and the rest) **or the first slice of a streamed one** (TRON, #279). Both count on the one meter.
- **Projects carry only a `SourceRef` to each source, never audio.** That is the Octatrack's and the Digitakt's model: a project's sample list points into the card's audio pool. Quick-save never rewrites sample data.

## What exists (measured 2026-10-04 on `main` at cf50ee74)

These figures were first measured on `orbit` (43d59110), then re-measured on `main` once ORBIT, fat LTO and the console's `card` command merged. The table is `main`'s.

| Thing | Bytes | Source of the figure |
|---|---|---|
| `Option<Sound>` in RAM | **896** | `memory_budget_test::axi_counts_the_project`: 32 slots = 28 672 B |
| `Project` on orbit (Performance 5 720 + slots 28 672 + ORBIT 3 912 + meta) | 38 424 | same test |
| A Sound as card records today (`encode_sound`, every block of both engines) | **1 143** (1 127 records + 16 name) | `tests/fixtures/v1/*.snd`, 1 159 B with the 28 B header and 4 B CRC |
| The same Sounds, sparse (§ Sparse encoding) | **153–273, mean 211**; INIT 128 | the 8 factory fixtures, re-encoded by hand |
| Worst case, sparse | 1 858 | § Sparse encoding |
| AXI by the paper model (`AXI_RESIDENT`, with `orbit`) | 442 084 (64 KiB spare required, 82 204 spare) | `axi_residents_fit` |
| AXI really used, default features | 382 496; 141 792 free | `rust-size -A`, release ELF |
| AXI really used, `--features orbit` (the largest shipping build) | 384 264; **140 024 free** | same |
| AXI really used, `--features bench,orbit` | 419 332; 104 956 free | same |
| AXI really used, `--features bench,master-tape` (the largest build `just check` makes) | 420 100; **104 188 free**. The bench builds hold 31 KB of diagnostics statics, 20 480 B of it the `card` command's transfer buffers (`card_speed::BUFS`) | same, and `rust-nm -S` |
| What `UI_RESERVE` (65 536) actually covers | `UiState` less its `Project` 3 840, `ENGINE` 5 864, the USB console's statics 1 456, plus others it was never sized for | `ui_state_fits_the_ui_reserve`; `rust-nm -S` |
| Master delay line (`[f32; 24 064]`, `dsp/delay.rs`) | 96 256; as `i16` (ADR 0090), 48 128, freeing 48 128 | `MAX_DELAY_SAMPLES` |
| D2 free | **121 504** of 286 720 | `instrument_fits_d2` |
| `SymPool` (D2) | 112 200: Sympathetic's string state, DSP not Sounds, so encoding Sounds frees none of it | `sympathetic_pool_fits_d2` |
| Flash free, `--features orbit` | 194 168 of 917 504 (fat LTO) | `rust-size` |
| Flash free, `--features bench,orbit` (the largest flash image) | **120 304** | same |

Three findings shape the design:

1. **Today's encoding is bigger than the RAM struct.** `encode_sound` writes all 19 voice blocks, MODAL's on an ALGO Sound and ALGO's on a MODAL Sound, at 5 B a param. "Store Sounds encoded" saves nothing until the encoding is sparse, which is step 1 of the build.
2. **The paper AXI rule binds before the chip does.** `AXI_RESIDENT` counts `UI_RESERVE` at 64 KiB. ADR 0014 sized that reserve from `main`'s measured stack, and ADR 0025 has since moved the stack to DTCM. The reserve now quietly covers statics no one budgeted (the row above). On the paper model, a 96 KiB arena would fail the `≥ 64 KiB` const assert (about 59 KB spare), even with the chip holding 140 KB free. The rule is replaced (§ The budget).
3. **D2 is TAPE's streaming memory.** Its 121.5 KB free is the TRON ring buffers (#279: about 10 KB a note, about 60 KB for six) and VA's state. The arena does not go there.

## Prior art, and what we take

| Instrument | What it does | What we take |
|---|---|---|
| **Polyend Play** | "On the bottom of the Main menu Samples screen, you can see a Memory bar that indicates the remaining sample pool space percentage." Up to 255 samples a project. "Delete Unused Samples" removes samples not placed on the grid. "A sample played back at half the sample rate will use half the Pool memory." (Play manual, § Samples, § Sample Pool.) | **One bar, labelled with a percentage only.** A count cap above what the bytes allow. Purge removes unused samples. A lower rate costs proportionally less. |
| **Polyend Tracker** | The Sample Loader's memory bar previews the highlighted file: a red segment for what it would add. "Delete unused" purges instruments the project doesn't use. (Tracker manual v1.5, § Sample Loader, § Instrument.) | **The preview segment** for the highlighted library entry. |
| **Elektron Digitakt** | 64 MB of sample RAM a project. SETTINGS › SAMPLES › VIEW RAM › SELECT UNUSED › UNLOAD removes samples no pattern uses from RAM. They stay on the +Drive. The project references samples on the +Drive. (Digitakt manual, § Samples.) | Purge is a selection, then a confirm. It removes samples only, and never touches the card. Projects reference card audio. |
| **Elektron Octatrack** | 85.5 MB of RAM, shared by Flex samples and the track recorders. Every sample in the slot list counts "whether they are used or not". PURGE SAMPLES drops unused samples from the slot lists, "not … from the audio pool" on the card. Static machines stream from the card. A load that won't fit is refused with a memory-full error. (Octatrack manual OS 1.25, § Memory, § Samples.) | **RAM-resident and streamed samples side by side** (Flex and Static). Projects point at card files. A refusal names the shortfall. Purge leaves the card alone. |
| **Akai S950** | 12-bit, rate chosen per sample, 7.5–48 kHz. 750 KB is about 63 s at the lowest rate. (S950 specifications.) | 12-bit storage with a rate per source, as the quality-for-time trade. |

Sources:
- Polyend Play manual: https://polyend.com/manuals/play/
- Polyend Tracker manual v1.5: https://files.kraftmusic.com/media/ownersmanual/Polyend_Tracker_Manual_v1_5.pdf
- Digitakt purge: Elektronauts, "How To: Remove Samples from Project?" and "How to delete ONLY unused samples from +Drive?", which quote the manual's path
- Octatrack manual OS 1.25: http://www.elektron.co.jp/wp-content/uploads/2015/01/octatrack_manual_OS1.25.pdf; the SampleStack guide: https://samplestack.app/news/octatrack-sample-management-guide/
- Akai S950: https://www.vintagedigital.com.au/akai-s950/ and https://en.wikipedia.org/wiki/Akai_S900

## The budget

**The rule changes from a paper sum to the linker.**

- `chimera-stm32/build.rs` writes `axi_margin.x` beside `ram_d2.x`: `ASSERT(ORIGIN(RAM) + LENGTH(RAM) - __sheap >= AXI_MARGIN, "AXI: under the margin")`. `__sheap` is cortex-m-rt's end of `.bss` and `.uninit`; there is no heap, so it is the real end of AXI statics.
- **`AXI_MARGIN` is 32 KiB on shipping builds and 8 KiB on bench builds** (`CARGO_FEATURE_BENCH`). The bench builds carry 31 KB of diagnostics, the `card` command's buffers among them, and are never what a user runs. The margin is growth room for product features. A diagnostics build needs only to link and run.
- Every feature set `just check` builds is checked. `orbit` binds the 32 KiB rule, and `bench,master-tape` binds the 8 KiB one.
- `UI_RESERVE` and the const `≥ 64 KiB` assert go. `axi_residents_fit` stays as a host-side report: it prints the parts, but it no longer asserts spare.
- **32 KiB is the stated margin.** It is room for the next features' UI state (TAPE's pages, VA's) without re-deciding the arena.

**`ARENA_BYTES` = 160 KiB (163 840 B), in AXI.** It is sized on `main` (cf50ee74), after ADR 0090 halves the delay line. The pool's own change is −48 128 (delay) − 28 672 (the fixed slots) + 2 176 (index: 128 × 12 B + 32 × 20 B) + 392 (`SourceTable`, 32 × 12 B, the gate and `sounding`) + the arena:

| Build | Used today | Used after | Free after | Margin | Largest arena it allows |
|---|---|---|---|---|---|
| `orbit` (shipping) | 384 264 | 473 872 | **50 416** | 32 KiB | 181 488 B (177 KiB) |
| `bench,master-tape` | 420 100 | 509 708 | **14 580** | 8 KiB | 170 228 B (166 KiB) |

160 KiB is the largest 16 KiB multiple that both rows allow. With no margin at all, the shipping build's ceiling is 524 288 − 310 032 = **214 256 B** (209 KiB). Without ADR 0090, the same arithmetic gives 112 KiB.

**What fits.** A Sound costs its sparse encoding plus its 16 B name, rounded up to 16 B. The curated set is budgeted at 320 B a Sound, above the measured 211 B mean. NEW's 32 then take **10 240 B, 7 %**, and leave **153 600 B** for sources:

| Source rate | 12-bit bytes/s | Seconds in 153 600 B, after NEW |
|---|---|---|
| 48 kHz | 72 000 | 2.1 s |
| 32 kHz | 48 000 | 3.2 s |
| **24 kHz** (default) | 36 000 | **4.3 s** |
| **16 kHz** | 24 000 | **6.4 s** |
| 8 kHz | 12 000 | 12.8 s |

As 16-bit, each figure would be a quarter less.

**What sources cost.**
- **A TRON instrument's heads:** 12 zones × 100 ms at 24 kHz is 43 200 B, **27 %**. The rest of its 4–5 MB streams from the card into D2's rings.
- **A 2 s microsample at 24 kHz:** 72 000 B, 44 %. NEW plus one reads **51 %**.

In Sounds alone, 160 KiB holds about 510 curated Sounds, so the 128-slot cap binds first unless sources are loaded.

**The delay line goes to `i16` first** (owner ruling 2026-10-04). ADR 0090 records it: the sound cost (grain on the repeats, SAT's loop bound against the ±2.0 full scale) and the CPU cost (a convert on each write and read). It is step 1 of the build, because the arena can grow past 112 KiB only once those 48 128 B are free.

## The model

```rust
pub const MAX_SOUNDS: usize = 128;   // Sound slots: was POOL_SIZE = 32
pub const MAX_SOURCES: usize = 32;   // source slots
pub const ARENA_BYTES: usize = 160 * 1024;
pub const GRANULE: usize = 16;

pub struct SlotId(u8);    // < MAX_SOUNDS; built only by new / ALL, as today
pub struct SourceId(u8);  // < MAX_SOURCES; the same pattern

/// Arena bytes, a multiple of GRANULE: what the meter adds up.
pub struct Cost(u32);

pub struct Pool {
    sounds: [Option<SoundEntry>; MAX_SOUNDS],
    sound_gens: [u16; MAX_SOUNDS],
    sources: [Option<SourceEntry>; MAX_SOURCES],
    source_gens: [u16; MAX_SOURCES],
    arena: ArenaWriter,          // the only writer of the arena's bytes
}
struct SoundEntry  { offset: u32, len: u16, engine: EngineType, crc: u32 }   // 12 B
struct SourceEntry { offset: u32, held: u32, r: SourceRef }                  // 20 B
pub struct SourceRef { crc: u32, frames: u32, rate: Hz }   // rate: u16 Hz; together, the source's identity
```

- **Slots stay numbered and stable**, now 128 of them; empty slots are free rows. The bytes are the real limit, and the count cap only bounds the index. The Play caps at 255 samples and the Digitakt at 128 slots.
- **Generations stay** exactly as ADR 0043 has them, one `u16` per Sound slot and per source slot.
- **`Pool::get(SlotId) -> Option<EncodedSound<'_>>`** replaces `Option<&Sound>`. An `EncodedSound` is a borrowed view: its name (its first 16 B), its engine, its `Cost`, its CRC, and `decode_into(&mut Sound)`.
- **`held` is how many bytes of the source the arena holds:** all of it (`3·⌈frames/2⌉`), or a head for a streamed source. A source's cost is `16 + held`, rounded up. The engine model decides which (§ How a Part plays).
- **`Cost` is a newtype.** `Pool::used()`, `Pool::free()` and `Pool::fits(Cost)` are the only arithmetic. The meter, the refusals and the purge read them.
- **References are derived, never stored.** `fn source_users(&Project, SourceId) -> Users` returns which pool Sounds and which Parts resolve to that source. It is computed when asked, the same way `users(SlotId)` is, so no count can drift. (The first draft's stored `refs` field could go stale between a Sound clear and a purge.)

## Memory layout and allocator

```
 POOL_ARENA (160 KiB, AXI)
 0                                                              ARENA_BYTES
 | src 0 | src 3 | src 1 |  ......... free gap .........  | snd 7 | snd 0 | snd 2 |
 ^ sources grow up from 0                       Sounds grow down from the top ^
```

- **Two ends, one gap.** Sources pack upward from offset 0, and Sounds pack downward from the top.
- **Sounds compact at once.** The audio never reads Sound bytes, so a Sound delete is a `copy_within` on the UI thread, done immediately.
- **Sources compact when nothing above the hole is sounding** (§ Compacting sources). Until then the hole is **pending**: the meter counts it as used, and draws it in `FAINT` (§ The meter). That keeps `free() == gap` true at every moment, so an add fits if and only if `cost ≤ gap`, and the meter never shows bytes a load can't use.
- **The granule is 16 B.** Each entry starts on a 16 B boundary, and rounding wastes 8 B an entry on average.
- **Where the bytes live.** `POOL_ARENA` is one AXI static. The desktop and the tests box one.
  - The UI holds the only `ArenaWriter`. The audio holds a `SourceReader`, which reads only through the `SourceTable`. Both wrap a raw pointer.
  - Their `// SAFETY:` argument: the audio reads only rows the table publishes as live, and the UI writes a live row's bytes only inside the gate below.
- **Staged writes go into the gap.** An add writes its bytes into the gap and verifies them (a decode for a Sound, the CRC for a source). Only then does it take an index entry. Until then the meter doesn't move. A failed or cancelled add leaves the gap as it was.

### The audio's view

```rust
pub struct SourceRow { base: AtomicU32, held: AtomicU32, rate: AtomicU32 }  // 12 B
pub struct SourceTable {
    rows: [SourceRow; MAX_SOURCES],
    gate: AtomicU32,       // nonzero while the UI moves source bytes
    sounding: AtomicU32,   // written by audio: bit n set if a voice reads source n
}
```

- **A voice latches its `SourceId` at note-on**, and its row's `base` and `held` at the start of each block. It never re-resolves the Part's ref mid-note, so a Part changing SOURCE doesn't touch a ringing note.
- **No generation in a row.** A row is reused only after its source is retired, and retirement waits until no voice reads it (§ Deleting a source). So no latched id can outlive its source. A generation would guard against something the protocol already rules out.
- **Writes by the UI** (row fields `Relaxed`, then the gate):
  - **publish a new source:** write `base`, `held` and `rate`, then `fence(Release)`. Only after that is the `SourceId` written into a Part's `AudioShared` state, which is itself published by the triple buffer's `Release`.
  - **audio reads:** `gate.load(Acquire)` at block start, then the rows.

### Compacting sources

A source delete leaves a hole below the sources above it. Compaction slides them down. The design is the simple one: **never move bytes a voice is reading.**

1. **Wait.** Compaction starts only when `sounding & above == 0`, where `above` is the mask of the sources above the hole. The UI checks on each pass. While a drone holds one of them, the hole stays pending, and the meter shows it.
2. **Close the gate.** The UI stores `gate = 1` (`Release`), then waits until the audio acknowledges a block that began after the store. This is the epoch-ack pattern of `Swap::ack` (ADR 0046), at most one block, 1.33 ms. While the gate is closed:
   - a note-on whose source is among `above` is **held for the block**: the voice is allocated and latched, its envelope waits, and it starts on the first block after the gate opens;
   - every other source plays normally.
3. **Re-check after the ack.** A note-on in the block just before the gate closed may have started a voice on a source in `above`: that block's `sounding` already has its bit. The UI re-reads `sounding & above` from a block that began after the ack. If any bit is set, it reopens the gate (`gate = 0`, `Release`) and goes back to step 1. Only with the mask clear, under a closed and acknowledged gate, does it move. No voice can start on `above` after that, so nothing reads a moving byte.
4. **Move.** `copy_within` the sources above the hole down, and rewrite their rows' `base` (`Relaxed`). The largest possible move, every source byte, is 154 KB, about 0.3 ms at 480 MHz.
5. **Open the gate:** `gate = 0` with `Release`. The audio's next `Acquire` sees the new bases.

**The cost** is that a note on a moving source can start up to two blocks (2.7 ms) late, once per source delete, which is a deliberate user action. The audio never reads a byte mid-move, and never takes a branch per sample. There is no per-read mapping The latency is accepted, not asked: 2.7 ms is far below the 10–20 ms at which two onsets are heard as a flam, and every note already lands on a 1.33 ms block edge.

### Deleting a source

- **A source a Part or a pool Sound resolves to can't be deleted** (`InUse`, as `pool_clear` is today). It can still be sounding after its last user left: a ringing note from before a Part changed SOURCE.
- **So a delete retires it.** Its index entry goes at once, and it is out of the SOURCE lists. Its row stays live until its `sounding` bit is clear, and then the hole compacts as above.
- **No note is ever cut.**

## Sparse encoding

`encode_sound` becomes **sparse**. A `Block` record carries only the params whose disk value (the `f32` bits, or the enum's code) differs from `Sound::neutral(engine)`'s, and a block with none is omitted. `Engine`, `Registry`, `ModDests` and `Routes` are written as today.

- **Today's decoder already reads it.** `decode_block` writes only the ids it is given, onto a `Sound::neutral` base, and an absent block keeps neutral's values.
- **`neutral` becomes a format contract.** A full-encoded file never depended on neutral's values; a sparse file does, for every param it omits. So:
  - changing any value in `Sound::neutral` changes the meaning of every sparse file on every card;
  - it needs a migration (`codes.rs`'s `TRANSLATIONS`, keyed on a format bump) in the same commit;
  - `neutral_v1.txt` already pins it, and its test comment says so from now on.

  A param added later takes its neutral default at addition. Old sparse files omit it and get that default, just as old full files lacked it.
- **Canonical.** The bytes depend only on the Sound's bits, so two Sounds are byte-equal exactly when they are bit-equal. `sound_crc` keeps its definition, and its values change once. No CRC is stored on the card: `Origin` stores the slot, and ADR 0043 recomputes `Origin.crc` at load.
- **Sizes.** The 8 factory fixtures go from 1 143 B to 153–273 B, and INIT to 128 B (its RR and OUT LEVEL sit off neutral, ADR 0063). The bound is 16 (name) + 5 (`Engine`) + 19 × 5 (block heads) + 205 × 5 (params) + 164 (`Registry`) + 37 (`ModDests`) + 516 (`Routes`) = **1 858 B**. `MAX_SOUND_COST` (1 872) is asserted.
- **A legacy file's previewed cost differs from its loaded cost.** A library `.snd` written full (before step 1) previews at about 1.1 K from its file size, and costs its sparse size, around 0.2 K, once loaded. The preview over-estimates, never under: a fitting load is never refused late. After a load, the meter shows the true cost.
- **One encoder writes everywhere:** the arena, project `Slot` and `Part` records, library `.snd` files and the factory blob.

## How a Part plays from an encoded Sound

- **A Part decodes on load into its own `Sound`, as today.** `Performance` keeps six decoded `Sound`s (896 B each).
  - `replace_part` decodes the slot's bytes into the Part in place, on the UI thread. That is about 200 B through `SoundCheck`, a few microseconds.
  - Parts reach the audio through `AudioShared`, as now. The audio thread never decodes a Sound.
- **Edits touch only the Part**, so editing can never fail on the budget.
- **Marks compare encodings.** `part_status` encodes the Part's Sound through a comparing sink against the slot's bytes, and reads `Clean` when they are equal. It needs no buffer. `project_crc` hashes the slot bytes directly.
- **A TAPE Sound carries a `SourceRef`, never audio.** When a Part loads it, the ref resolves to the pool `SourceId` whose entry has the same `crc`, `frames` and `rate`. The Part keeps that id and its generation, and `AudioShared` carries the id. Sharing an immutable source can't be told apart from copying it, so ADR 0043's copy rule holds for what a user can observe. Deletion, the one observable difference, is guarded by `InUse` and retirement. A source is never edited in place: #279's PROCESS writes a new card file.
- **Whole or head.** The TAPE model decides how much of a source the arena holds:
  - random-access models (TRANSWAVE, LOOP, GRAIN, SCRUB, PING-PONG, REVERSE, SPECTRAL, FORMANT) hold the whole source;
  - TRON holds a 100 ms head per zone and streams the rest from the card into D2 rings (#279).

  Both are the same pool entry with a different `held`.
- **An unresolved ref is not an error.** The Part loads, its TAPE voice is silent, and its page reads `SOURCE MISSING`.

## 12-bit packing

- **Two samples in three bytes**, little-endian: `b0 = s0[7:0]`, `b1 = s0[11:8] | s1[3:0] << 4`, `b2 = s1[11:4]`. Samples are 12-bit two's complement. An odd frame count pads the last pair with 0. A card source file and the arena hold the same bytes, so a load is a copy.
- **The audio reads** by sign-extending `(x << 20) >> 20` on an `i32` and scaling by `1/2048`. Unaligned byte loads are free on the Cortex-M7.
- **Conversion is host-only** (ADR 0093, the TAPE spec). The host tool resamples a WAV, band-limits it, dithers it to 12 bits and writes the `.SRC` file. The unit only stores and plays verified `.SRC` files. `chimera-core` carries no importer, decimator or dither.
- **The rate is per source,** `rate` in Hz, 4 000–48 000. The UI offers 48, 32, 24, 16, 12 and 8 kHz, and **24 kHz is the default, per #279** (honest to tape's ~10 kHz top). The host tool band-limits to 0.45 × rate before decimating. The voice steps through the source at `rate / 48 000 × pitch ratio`.

## The meter, and the UI

**One bar, labelled with a percentage only, as on the Play.** It sits on the Sound browser's info line, where `2 INIT + 32 SLOTS` is now, and on SETTINGS › PROJECT › POOL.

```
                                             64%
 ███████████▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒░░░░░░░░░░░░░░░░
 Sounds       sources    pending      free
```

- **Geometry.** The bar is 216 px wide (the screen less its margins, ADR 0086's measure) and 4 px tall.
- **Colours.**
  - Sounds are filled in `INK`, sources in `MID`, and free space is `BAR_REST`.
  - A pending hole (§ Memory layout) is `FAINT`.
  - The preview segment is `ACCENT`, or `ALERT` when it won't fit.
- **It counts arena bytes in use** (`Pool::used()`, pending holes included) out of `ARENA_BYTES`. The index, the Parts and the `SourceTable` are fixed and never move, so it leaves them out.
- **The percentage** is `⌈used × 100 / ARENA_BYTES⌉`. The bar never reads emptier than it is, and an empty pool reads 0 %.
- **The preview.** On a library row, or on a card source in a TAPE Part's SOURCE list, the bar adds that entry's cost after the used part, as the Tracker does. If it won't fit, `ALERT` runs from the used part to the end, and the focus band reads the shortfall. A source the pool already holds previews as 0.
- **Rows show their cost** on the right, as `0.3K` or `41K`: kibibytes, one decimal under 10, rounded up.
- **SETTINGS › PROJECT › POOL** is a leaf (ADR 0066: one page). It shows the bar, `SOUNDS 12 · 9.8K`, `SOURCES 2 · 52K`, `PENDING 0K` (only while one is pending) and `FREE 34K`. It is read-only, and it is the breakdown the issue asked for.
- **SETTINGS › PROJECT › PURGE UNUSED SOURCES** is an `Act` row. It opens a prompt: `PURGE 3 SOURCES? FREES 61K`. SEQ confirms and MENU cancels, as in every prompt (ADR 0081). It is reached by EDIT, not a MIX gesture, so it needs no hint.

**Hints (ADR 0086).** No new MIX or hold gesture. Two existing browser hints gain the bytes their gesture moves:

| Where | Touched | Hint |
|---|---|---|
| Sound browser, a filled slot | always | `MIX + MINUS CLEARS · FREES 0.3K`. If clearing it leaves a source with no user: `MIX + MINUS CLEARS · CHOIR 41K UNUSED` |
| Sound browser, the Part's own row | always | `MIX + EDIT SAVES · +0.1K` (the save's cost against the slot it replaces), or `WON'T FIT` in `ALERT` |

**After a clear that orphans a source,** the focus band reads `CHOIR 41K UNUSED · PURGE FREES IT` until the next input (ADR 0017's convention). The clear frees the Sound's bytes only. The source stays until a purge, as a sample stays in the Play's pool when its last step is erased.

## When the budget is full

- **An add that won't fit is refused before anything is read or written.** That covers library → pool, a save to a new slot, a save over a slot that grows, a source added from the card, and a TAPE Sound whose source isn't in the pool yet.
  - Every cost is known up front: from a library file's size, from encoding the Part through a counting sink, or from a `SourceRef`'s frames and the model's `held`.
  - The focus band reads `POOL FULL · NEEDS 41K, 12K FREE` in `ALERT` until the next input. Nothing changes.
- **PURGE UNUSED SOURCES removes samples only**, as the Play, the Digitakt and the Octatrack do. It takes every source for which `source_users` is empty, recounted from the project when the purge runs. Never a cached count.
  - A source still sounding is retired (§ Deleting a source). Its bytes come back when the note ends, shown as pending meanwhile.
  - It deletes nothing from the card, and it marks the project `Modified`, as any pool change does.
  - Sounds are never purged. A Sound costs about 0.3 K. Clearing one is the browser's `MIX + MINUS`.
- **Nothing else refuses.** Editing a Part, loading a slot into a Part, and switching a Part's SOURCE among the pool's sources cost no arena bytes.

## Interactions

### Part copies and ADR 0043's guards

- `Origin`, `part_status`, `part_actions`, `ReplaceGuard` and the witnesses keep their meaning. `SlotId`'s range grows to 128.
- `NEW SLOT` is offered when a slot is free **and** the Part's Sound fits. `SAVE OVER SLOT` is offered when the growth fits. An offer that can't apply isn't made (ADR 0043's rule), so a full pool shows `POOL FULL` in its place.
- `ReplaceError::SlotEmpty` stays.

### Project format and versioning

- **`FORMAT_VERSION` stays 1.** The TLV records and the must-understand bit (ADR 0045) carry the change.
- **`Slot` (0x8007)** keeps its shape. Its index range becomes 0..128. Its Sound is written sparse.
- **`Part` (0x8008)** writes its Sound sparse: about 300 B where it was 1 143.
- **New records,** after ORBIT's 0x0010–0x0014. The numbers are proposed here, checked against `RecordTag` when they are built, and frozen once written:

  | Tag | Record | Payload |
  |---|---|---|
  | `0x8015` | `PoolSource` | source slot `u8` (0..32), then the `SourceRef` (crc `u32`, frames `u32`, rate `u16`): 11 B |
  | `0x0016` | `SourceRef` | the `SourceRef`, 10 B; inside a TAPE Sound's records |

  There is no audio in a project file, ever. Quick-save (MENU hold) writes records of a few KB and never sample data.
- **Order:** `Fx`, then `PoolSource`s in slot order, then `Slot`s, then `Part`s, then ORBIT's records (ADR 0078).
- **An older build reading a newer project:**
  - a `Slot` index ≥ 32 is `Bounds`;
  - a `PoolSource` is an unknown critical tag, so `NeedsNewerFirmware`.

  Either fails the whole load, and the build falls back to NEW (`reset_new`), leaving the card file untouched. Nothing loads half. No users exist yet, so a down-flash losing its project view is acceptable.
- **Loading.** The load runs in two passes (ADR 0045/0046).
  - **Pass 1** adds up every Sound's cost and every `PoolSource`'s cost (its `held`, from the frames and the engine model, with no card read).
  - **Pass 2** runs after the epoch fade, so no voice reads the arena while it is rebuilt. It reads each source's bytes, or its head, from `/CHIMERA/TAPES`.
- **Old projects** (32 full-encoded `Slot`s) load and re-encode sparse. They sound the same.
- **A project over the budget** loads in priority order: the sources a Part resolves to, then Sound slots in slot order (skipping any that won't fit), then the other sources. The note reads `LOADED · 4 SOUNDS, 1 SRC DIDN'T FIT`. The project reads `Modified`, and a later save drops the skipped entries. Until then the file on the card is untouched. Only a hand-made file or a future build with a bigger arena can produce one.

### The card library

- **Library Sounds** (`/CHIMERA/SOUNDS`, #317) are written sparse. A row's cost is the file size − 28 (header) − 4 (CRC) + 16 (name), rounded up to 16, read from the listing with no extra card I/O.
- **Sources live only on the card,** in a new `Dir::Tapes` (`/CHIMERA/TAPES`), as `FileKind` 6 `Source` files: a header, a `SourceHead` record (`0x8017`: the `SourceRef` and a 16 B name), then `SourceData` records (`0x0018`) of up to 504 B of packed pairs (168 pairs). Both are refused (`Corrupt`) outside a `Source` file.
  - **The 8.3 name is the CRC in hex, with `.SRC`** (`1A2B3C4D.SRC`), so a `SourceRef` finds its file by name, with no index. Identical sources share one file. It is a single file, not an A/B pair: it never changes once written.
  - **Write-once, by sealing.** A file arriving from the host tool (ADR 0093) is written once. The writer reads it back, checks the header and CRC, and only then sets the FAT read-only attribute. From then on, the FAT layer's `writable()` check (console-put, #359) refuses any rewrite or delete.
  - **A torn file is replaced.** An unsealed `<CRC>.SRC` is a write a power cut interrupted. The next write of the same source deletes it and writes it again. A sealed file whose check fails reads as missing, and is never rewritten in place; card cleanup is #267.
  - The `SourceRef`'s `frames` and `rate` are checked against the header, and a mismatch reads as missing.
- **Adding a library TAPE Sound** brings its source's `held` bytes into the pool too, unless the pool already holds that source. The preview and the refusal count both. A source not on the card refuses the add: `SOURCE NOT ON CARD`.
- **Purge never touches the card.** Orphaned `/CHIMERA/TAPES` files are a card-cleanup job for SETTINGS › SYSTEM › STORAGE (#267), outside this spec.

### The factory set

- **NEW's 32 curated Sounds are one sparse blob in flash** (`factory.bin`, about 10 KB). A desktop tool generates it from the authored Sounds (#317), and a test decodes every entry against its authored form. NEW copies the entries into the arena without decoding, since they are already canonical.
- **Factory sources are card-only.** Flash has 120 KB free on the largest build, and one second of 16 kHz 12-bit audio is 24 KB. Until a card is present, the 32 are ALGO and MODAL Sounds. A factory TAPE Sound on the card library brings its source from `/CHIMERA/TAPES` like any other.
- **NEW reads 7 %.**
- **`TemplateCrc`** is computed at run time from the arena as filled.
- **ADR 0073's `NEW_PARTS`** carries over, with slot numbers from #317's curated list.

### Flash cost

These are estimates, to be measured with `rust-size` on `bench,orbit` (the largest flash image) at each step, against 120 304 B free.

| Step | Adds | Removes | Net |
|---|---|---|---|
| Sparse encoder | ~0.5 KB | | +0.5 KB |
| Arena, index, Sound compaction, `EncodedSound`, encoded marks | ~4 KB | | +4 KB |
| Meter, row costs, preview, POOL leaf, PURGE row and prompt, hint text | ~3 KB | | +3 KB |
| Factory blob, 32 Sounds | ~10 KB | today's code-built 8 Sounds (`factory.rs`), ~3 KB | +7 KB |
| Sources: table, gate, retire, 12-bit read, seal and verify, `Dir::Tapes`, records | ~5 KB | | +5 KB |
| **Total** | | | **≈ 20 KB**, leaving ≈ 100 KB |

## Failure modes

| Case | What happens |
|---|---|
| **No card at boot** | NEW from flash: 32 Sounds, 7 %. Library rows read `NO CARD`. Sound-only pool work goes on. |
| **No card, project with TAPE Parts** | It can't be loaded without a card, since projects live on the card. A card pulled after load leaves whole sources playing. Streamed (TRON) notes play their heads, then fall silent per note, which is #279's rule. |
| **Card pulled during an add or a project load's pass 2** | The add abandons its staged bytes, with no index entry and the meter unchanged: `CARD READ FAILED`. A load fails and falls back to NEW, as today. |
| **Card swapped** | `Card::run`'s volume check refuses it (ADR 0043/0046), with the same outcome. |
| **Slow card** | A source read is 4 KiB a UI pass, behind `ui::busy`. 154 KB at 1.3 MB/s (#279's measurement) is 120 ms of card time. At full audio load, with UI passes of 22–26 ms (stress test 2026-10-02), it takes about 1 s on screen. MENU cancels an add. A project load's sources read inside its existing fade. |
| **A source missing from the card at load** | Its `PoolSource` loads as an **absent** entry: it keeps its `SourceRef`, costs 0 B, and its row reads `MISSING`. A Part that resolves to it loads silent, with `SOURCE MISSING`, and the note names the count. A save writes the ref back unchanged, so a card missing a file for a while never loses the project's reference to it. PURGE removes an absent entry only when nothing uses it. |
| **Project over the budget** | It loads in priority order, with a note. |
| **A purged or cleared source still sounding** | It is retired, its bytes stay pending until the note ends, and the meter shows them in `FAINT`. |

## Test plan

All host-side (`just test`). Hardware checks wait for the ship flash.

**Encoding**
- `sparse_round_trip`: for every factory Sound, both INITs and 1 000 seeded random Sounds (an xorshift over each param's spec range, the routes and the registry), `decode(encode(s))` is `bits_eq` to `s`.
- `sparse_byte_identity`: `encode(decode(encode(s))) == encode(s)`, byte for byte, over the same set. Re-encoding a full-encoded v1 fixture gives the sparse bytes of the original.
- `sparse_is_canonical`: two Sounds are byte-equal exactly when `bits_eq` (the random set, plus single-param mutations).
- `v1_fixtures_still_load`: every `fixtures/v1` file decodes to the same bits as before.
- `neutral_is_a_contract`: `neutral_v1.txt` matches `Sound::neutral`, and the test's message says that a change needs a migration.
- `sparse_bound`: a Sound with every param off neutral, a full registry and 128 routes is at most `MAX_SOUND_COST`.

**Arena and allocator**
- **A seeded model test,** 10 000 operations (add, delete, replace, retire, compact), with a fake `sounding` mask flipping. After each operation:
  - entries are disjoint and granule-aligned;
  - the sources are contiguous from 0, apart from pending holes;
  - the Sounds are contiguous to the top;
  - `free() == gap`;
  - a per-entry CRC is unchanged by every move;
  - `used()` is the sum of costs and pending holes.
- `compaction_waits_for_silence`: no source byte moves while its `sounding` bit is set. The hole stays pending and shows on the meter.
- `gate_recheck_catches_late_note_on`: a note-on on a source in `above` arrives in the block just before the gate closes. The re-check after the ack sees its `sounding` bit, reopens the gate, and moves nothing until the note ends. Then compaction runs, and the voice's samples match the unmoved source throughout.
- `gate_holds_note_on`: a note-on on a moving source during a closed gate starts on the first block after the gate opens, with its first sample unchanged. A note-on on any other source starts at once.
- `retire_never_cuts`: a retired source's row stays live until its bit clears.
- **`memory_budget_test`:**
  - `axi_residents_fit` becomes a report;
  - `pool_arena_fits_axi` prints the arena, the index and the table;
  - the binding check is the linker `ASSERT`, exercised by every firmware build in `just check`.

**Sources and `source_users`**
- `choir_orphan`, the review's scenario:
  1. Slot 5 holds CHOIR, a TAPE Sound on the CHOIR source.
  2. Part 2 loads it, then loads another Sound.
  3. The clear hint for slot 5 names `CHOIR 41K UNUSED`.
  4. After the clear, `source_users(CHOIR)` is empty, and PURGE frees exactly CHOIR's bytes.
  5. Before the clear, PURGE frees nothing.
  6. With Part 2 still on CHOIR, the clear frees the Sound, PURGE keeps the source, and the hint doesn't say UNUSED.
- `purge_recounts`: a purge after any sequence of clears and Part loads frees exactly the sources the model says are unreferenced at that moment.
- `purge_never_writes_card`: the `MemStore` write count stays 0.

**12-bit**
- `pack_unpack_all_values`: all 4 096 × 4 096 pairs round-trip in release, or a seeded 100 000 in debug. Odd frame counts pad.

**Meter and UI**
- `meter_percent`: 0 B is 0 %, 1 B is 1 %, `ARENA_BYTES − 1` is 100 %, and half is 50 %.
- Renderer snapshots (`docs/screens`): the bar at NEW, at 64 %, with a pending hole, with a fitting preview and with a refused one, and the POOL leaf.
- `hint_fits`: every amended hint within 216 px.
- A refusal leaves `project_crc` unchanged.

**Project and library**
- An old 32-slot project loads, and its Parts render bit-identical audio through the golden harness.
- The over-budget fixture loads in priority order, with the counts in the note.
- A project with `PoolSource`s round-trips save → load byte-equal. Its file holds no `SourceData`. A quick-save's bytes written are independent of source sizes.
- A missing `/CHIMERA/TAPES` file leaves its Part silent, with no panic.
- NEW: 32 Sounds, 7 %, `Pristine`, every factory entry decode-equal to its authored form.

**Desktop QA** (QA before UAT): browse, clear and purge on the simulator. Once TAPE's first voice exists, delete sources while other sources loop, and check the recordings for discontinuities.

## Build order

Each step ships on its own, and the meter is right at each.

1. **The delay line as `i16` (ADR 0090).** It frees 48 128 B of AXI, re-blesses the delay goldens, and comes with ADR 0090's tests. It lands first, on its own, because the 160 KiB arena can't link without it.
2. **Sparse `encode_sound`,** with the encoding tests, `MAX_SOUND_COST` and the neutral contract note. It is a writer change only.
3. **The linker assert in place of the paper rule.** `UI_RESERVE` and the 64 KiB const go, and `axi_residents_fit` becomes a report. It lands before step 4, on its own, because step 4 can't link under the paper rule.
4. **The arena and the index, Sounds only.** `Pool` changes over to 128 `SlotId`s, `EncodedSound`, encoded marks and Sound compaction. The `Slot` range grows, and the 28 KB of fixed slots go.
5. **The meter and the refusals:** the bar, row costs, the preview, `POOL FULL`, the amended hints, and SETTINGS › PROJECT › POOL. PURGE waits for sources.
6. **NEW from the factory blob,** once #317's 32 are curated. Until then, the blob holds today's 8 and the two INITs: about 2 %.
7. **Sources,** with the TAPE core (#279):
   - `SourceId`, the source end, the table, the gate, retirement, `source_users`;
   - PURGE UNUSED SOURCES;
   - 12-bit packing and `Dir::Tapes` (the importer is host-only, ADR 0093);
   - `PoolSource` and `SourceRef`;
   - the over-budget load order.

Step 4 merges after ORBIT, since both touch the project codec and `template.rs` (ADR 0073, 0078).

## Owner questions

None open. Settled by the owner, by precedent or by the code:
- **The delay line goes to `i16`** (owner, 2026-10-04): ADR 0090, giving a 160 KiB arena.
- **The budget size** comes from the arithmetic (§ The budget).
- **PURGE takes samples only,** as on the Play, the Digitakt and the Octatrack.
- **Factory sources are card-only,** because of flash.
- **24 kHz is the default import rate,** per #279.
- **The meter shows a percentage only,** as on the Play.
- **The compaction gate's 2.7 ms worst-case note-on delay is accepted.** It is far below the 10–20 ms flam threshold, and notes already land on 1.33 ms block edges.
