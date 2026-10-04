# 0087. Budget the sound pool by memory: one arena for sparse Sounds and 12-bit sources, one meter (supersedes in part 0043)

- **Status:** Proposed
- **Deciders:** owner (2026-10-03, private issue #337), firmware

## Context
ADR 0043's `Pool` is 32 `Option<Sound>` slots of 896 B each, 28 672 B in
AXI, whatever the Sounds are. The owner ruled that the pool is budgeted
by memory, as on the Polyend Play, with these rules:

- one budget for Sounds and TAPE's sample data, with #279's separate
  sample pool merged into it;
- one meter;
- Sounds stored encoded, so each costs only what it is;
- sources stored S950-style, 12-bit, with a rate per source;
- NEW filled with 32 curated factory Sounds.

Measured on 2026-10-04:

- **Today's encoding is no help.** `encode_sound` writes all 19 voice
  blocks of both engines, 1 143 B a Sound, which is more than the struct.
  Writing only the params that differ from `Sound::neutral` gives
  153–273 B for the factory Sounds and 58 B for INIT. The worst case is
  1 823 B.
- **AXI has 148 724 B really free**, by `rust-size`. The accounting model
  (`AXI_RESIDENT`) has 90 576 B spare against a 64 KiB rule.
- **D2's 126 548 B free is earmarked for TAPE streaming and VA.**

The audio thread never reads the pool today, but a TAPE voice will read
its source's bytes.

## Decision
- **One arena, `ARENA_BYTES` = 96 KiB, in AXI**, replacing the fixed slots.
  - Sources pack up from offset 0, Sounds pack down from the top, and
    free space is the gap between them.
  - Each entry is rounded to a 16 B granule.
  - Every delete compacts its own end at once, on the UI thread, so no
    hole survives an operation and `free == gap`.
  - `UI_RESERVE` drops from 64 KiB to 16 KiB (`UiState` less its
    `Project` measures 3 608 B), which keeps the 64 KiB AXI spare.
- **Numbered slots stay, and their generations stay:** 128 Sound slots
  (`SlotId`) and 32 source slots (`SourceId`), each with a `u16`
  generation. The bytes are the limit; the counts only bound the index.
- **Sounds are stored sparse.** `encode_sound` writes only the params
  whose disk value differs from `Sound::neutral(engine)`, and drops empty
  blocks.
  - Today's decoder already reads it, so the card format doesn't change.
  - The encoding is canonical: bit-equal Sounds are byte-equal.
  - The same encoder writes the arena, project files, library files and
    the factory blob.
- **A Part decodes on load into its own `Sound`, as today.** The audio
  thread never decodes. Marks compare the Part's encoding with the slot's
  bytes.
- **A TAPE Part shares its source, never copies it.** The Sound carries
  `SourceRef { crc, frames, rate }`, which resolves to a `SourceId` at
  load. The voice reads through a `SourceTable` of atomics.
  - Sources are immutable, so sharing one is indistinguishable from
    copying it, and ADR 0043's copy rule holds.
  - A source in use can't be deleted (`InUse`). A source no Part uses but
    a voice still sounds is retired: freed only after the audio reports
    it silent and acks the epoch.
- **Moving a source under audio.** A compaction slides each source down
  by the deleted cost `g`. It copies front to back in chunks of at most
  `g` bytes, each a multiple of 3, and publishes `done` after each chunk.
  The audio reads byte `i` at `base + i` below `done`, else at `from + i`.
  With chunks of at most `g`, no write reaches a byte the audio reads at
  `from`.
- **12-bit packing:** two samples in three bytes, little-endian, two's
  complement. The rate is set per source, 4–48 kHz, offered as
  48/32/24/16/12/8 kHz with 24 kHz the default. Imports are TPDF-dithered
  offline.
- **One meter:**
  - a 216 × 4 px bar on the browser's info line and on SETTINGS ›
    PROJECT › POOL;
  - Sounds in `INK`, sources in `MID`, free in `BAR_REST`;
  - `⌈used × 100 / ARENA_BYTES⌉ %`;
  - the highlighted library entry previewed as an `ACCENT` segment, or
    `ALERT` when it won't fit (after the Polyend Tracker);
  - each row shows its cost.
- **When it is full,** an add is refused up front and nothing changes:
  `POOL FULL · NEEDS 41K, 12K FREE`. SETTINGS › PROJECT › PURGE UNUSED,
  behind a prompt, removes unreferenced sources and Sound slots no Part
  names. It never touches the card (after the Digitakt and Octatrack).
  Editing a Part never costs arena bytes.
- **No new gestures.** The browser's existing `MIX + MINUS` and
  `MIX + EDIT` hints gain their cost (ADR 0086).
- **Project files** keep `FORMAT_VERSION` 1:
  - `Slot` ranges 0..128;
  - Sounds are written sparse;
  - `Source` (0x8015, critical), `SourceData` (0x0016) and `SourceRef`
    (0x0017) are added after ORBIT's tags, frozen once written.

  Pass 1 of a load adds up the costs. Old projects load and sound the
  same. A project over the budget loads in priority order: Part-used
  sources, then slots in order, then other sources. It ends with a note
  of what didn't fit and reads `Modified`.
- **Library sources** live in `/CHIMERA/TAPES`, named by their CRC in
  hex, as `FileKind` 6.
- **NEW copies a sparse factory blob from flash into the arena** without
  decoding it: about 10 KB, 10 % of the meter.

## Alternatives considered
- **Keep 32 fixed slots and add a separate sample pool (#279's 128 KB).**
  It means two meters and two "full" states, and the 28 KB of slots stays
  reserved however small the Sounds are. The owner ruled for one budget.
- **One interleaved arena,** entries in insertion order. A Sound delete
  would then move sources, and every compaction would need the audio
  protocol. Two ends keep Sound moves trivial.
- **A free list with holes, no compaction.** The meter would read free
  bytes that a load can't use. The Play and the Octatrack meters count
  bytes, so free must mean usable.
- **Decode Sounds into the arena,** resident as structs. That is 896 B a
  Sound, the fixed pool again.
- **Copy each TAPE Part's source into the Part.** That doubles the bytes
  of every shared source, and #279's sources are 48–96 KB.
- **16-bit sources.** They give a third less time at every rate, and the
  owner ruled for 12-bit.
- **Compress Sounds further,** for example as a delta against INIT, or as
  `u16` param values. Neutral is frozen and INIT is not, and a `u16` form
  is a format change. Sparse against neutral already brings a Sound to
  about a quarter of the struct.
- **A 128 KiB arena.** 3.4 s at 24 kHz in place of 2.4 s, but AXI's
  model spare falls to about 34 KiB. That is open question 1 in the spec.
- **Put the arena in D2.** D2's free 126 KB is TAPE's streaming buffers
  (#279: about 58 KB of first slices and about 60 KB of rings).

## Consequences
- `Pool::get` returns an encoded view, not `&Sound`. `replace_part`,
  `part_status` and `project_crc` read bytes. Every caller of
  `pool().get` changes once.
- NEW reads about 10 %. One 2 s, 24 kHz source beside it reads 84 %.
- `memory_budget_test` gains the arena, the index and the `SourceTable`.
  `AXI_RESIDENT` keeps 64 KiB spare at about 67 KiB.
- The audio thread gains one branch per source pair read (the move
  mapping), and a `sounding` mask per block.
- Project, library and Part records shrink about fivefold. The 20.6 KB
  v1 project fixture's Sounds go from 1 143 B to about 200 B each.
- A project from a build with a bigger arena loads partially, with a note.
- Factory TAPE Sounds wait on where their sources live (flash or card).

## Sources
- Spec: `docs/superpowers/specs/2026-10-04-sound-pool-budget-design.md`.
- Issues: #337, #279 (TAPE), #317 (factory library).
- Code:
  - `chimera-core/src/project/pool.rs`, `template.rs`, `codec.rs`,
    `marks.rs`;
  - `chimera-core/src/storage/sound.rs`, `block_codec.rs`, `record.rs`;
  - `chimera-core/src/hw.rs`, `chimera-core/src/instrument.rs`
    (`AXI_RESIDENT`);
  - `chimera-core/tests/memory_budget_test.rs`;
  - fixtures `chimera-core/tests/fixtures/v1/*.snd`.
- ADRs:
  - 0043 (projects own their sounds), 0045 (card format), 0046 (load
    protocol);
  - 0021 (triple buffer), 0066 (SETTINGS), 0073 and 0078 (ORBIT's NEW
    Parts and tags), 0081 (D-pad), 0086 (hints).
- Manuals:
  - Polyend Play manual (§ Samples: Memory bar, Delete Unused Samples);
  - Polyend Tracker manual v1.5 (§ Sample Loader memory preview, Delete
    unused);
  - Elektron Digitakt manual (§ Samples: VIEW RAM › SELECT UNUSED ›
    UNLOAD);
  - Elektron Octatrack manual OS 1.25 (§ Memory, PURGE SAMPLES);
  - Akai S950 specifications (12-bit, 7.5–48 kHz, 750 KB).
