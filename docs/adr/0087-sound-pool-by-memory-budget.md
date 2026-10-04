# 0087. Budget the sound pool by memory: one arena for sparse Sounds and card-backed 12-bit sources, one meter (supersedes in part 0043 and 0014)

- **Status:** Proposed
- **Deciders:** owner (2026-10-03, private issue #337), firmware

## Context
ADR 0043's `Pool` is 32 `Option<Sound>` slots of 896 B each, 28 672 B in
AXI, whatever the Sounds hold. The owner ruled that the pool is budgeted by
memory, as on the Polyend Play:

- one budget for Sounds and TAPE's sample data, with #279's separate
  sample pool merged into it;
- one meter;
- Sounds stored encoded;
- sources 12-bit with a rate each, S950-style;
- NEW filled with 32 curated factory Sounds.

Measured on `orbit` (43d59110), which merges first:

- **`encode_sound` writes all 19 voice blocks of both engines,** 1 143 B a
  Sound, more than the struct. Writing only params that differ from
  `Sound::neutral` gives 153–273 B for the factory Sounds and 58 B for
  INIT. The worst case is 1 823 B.
- **AXI really free** is 125 732 B on the largest build (`bench,orbit`,
  `rust-size`). The paper model, `AXI_RESIDENT` with its 64 KiB `UI_RESERVE`
  and its `≥ 64 KiB` spare assert, binds well before the chip does. The
  reserve was sized (ADR 0014) for a stack that ADR 0025 has since moved to
  DTCM, and it now quietly covers `ENGINE`, the USB statics and `UiState`.
- **D2's 121 504 B free is TAPE's streaming rings and VA.**
- **Flash has 100 036 B free** on `bench,orbit`.

Even all the free AXI, with the delay line halved (ADR 0090), holds only
about 5.6 s of 24 kHz 12-bit audio. A RAM-only TAPE would be a one-source
engine.

## Decision
- **The linker enforces the AXI margin, not a paper sum.**
  - `memory.x` asserts at least 32 KiB between `__sheap` and the end of
    AXI, checked on every feature set `just check` builds.
  - `UI_RESERVE` and the 64 KiB const assert go. `axi_residents_fit`
    becomes a report.
  - This supersedes ADR 0014's AXI accounting in part.
- **One arena, `ARENA_BYTES` = 160 KiB, in AXI**, replacing the fixed
  slots.
  - It is sized after ADR 0090 stores the master delay line as `i16`
    (owner, 2026-10-04), which frees 48 128 B.
  - On `bench,orbit`: 398 556 − 48 128 − 28 672 + 2 176 (index) + 392
    (`SourceTable`) + 163 840 = 488 164 B used, leaving 36 124 B, above
    the 32 KiB margin.
  - Without ADR 0090 the same arithmetic stops at 112 KiB.
  - ADR 0090 is step 1 of the build, before the arena.
  - Sources pack up from 0, Sounds pack down from the top, and free space
    is the gap.
  - Entries are rounded to a 16 B granule.
  - Sound deletes compact at once on the UI thread.
- **Numbered slots and their generations stay:** 128 Sound slots and 32
  source slots. The bytes are the limit.
- **Sounds are stored sparse.** `encode_sound` writes only params whose
  disk value differs from `Sound::neutral(engine)`.
  - Today's decoder reads it, and the encoding is canonical.
  - `neutral` becomes a format contract: changing a default needs a
    migration.
  - One encoder writes the arena, project files, library files and the
    factory blob.
- **A Part decodes on load into its own `Sound`, as today.** The audio
  never decodes. Marks compare encodings.
- **Sources live on the card, write-once,** in `/CHIMERA/TAPES`, named by
  their CRC.
  - The arena holds a whole short source (the random-access TAPE models)
    or a streamed source's head (TRON). Both count on the meter.
  - Projects carry only a `SourceRef { crc, frames, rate }` (`PoolSource`
    0x8015 at the top level, `SourceRef` 0x0016 inside a Sound), never
    audio. Quick-save never writes sample data.
  - A Part shares its source, never copies it. Sources are immutable, so
    ADR 0043's copy rule holds.
- **Who uses a source is derived** (`source_users`), never stored.
- **Source bytes never move under a voice.**
  - A voice latches its `SourceId` at note-on.
  - A source hole compacts only when no source above it is sounding (an
    audio-written mask). It then closes a gate the audio acks within one
    block, moves the bytes, and reopens the gate with `Release`. Note-ons
    on the moving sources start after it, at most 2.7 ms late. That is
    far below the 10–20 ms flam threshold, and notes already land on
    1.33 ms block edges.
  - After the ack, the UI re-checks the mask, because a note-on in the
    block before the gate closed may already sound a moving source. If a
    bit is set, it reopens the gate and waits.
  - Until compaction, the hole is shown as pending and counted as used, so
    free always equals the gap.
  - A deleted source still sounding is retired, never cut.
  - `SourceTable` rows (12 B) carry no generation: a row is reused only
    after retirement, when no voice holds it.
- **12-bit packing:** two samples in three bytes, two's complement. The
  rate is set per source, 4–48 kHz, with 24 kHz the default (#279).
  Imports are TPDF-dithered offline.
- **One meter, a percentage only, as on the Play:**
  - a 216 × 4 px bar on the browser's info line and on SETTINGS ›
    PROJECT › POOL;
  - Sounds in `INK`, sources in `MID`, a pending hole in `FAINT`, free in
    `BAR_REST`;
  - the highlighted library entry previewed in `ACCENT`, or `ALERT` when
    it won't fit (after the Tracker);
  - each row shows its cost.
- **When it is full,** an add is refused up front: `POOL FULL · NEEDS 41K,
  12K FREE`.
- **SETTINGS › PROJECT › PURGE UNUSED SOURCES** removes samples only, as
  the Play, the Digitakt and the Octatrack do. It is recounted when it
  runs, and never touches the card.
- **Clearing a Sound that orphans a source** says so in its hint and its
  result (`CHOIR 41K UNUSED`).
- **No new gestures** (ADR 0086).
- **Projects keep `FORMAT_VERSION` 1.**
  - `Slot` ranges 0..128. An older build reads an index ≥ 32 or a
    `PoolSource` as a failed load, and falls back to NEW.
  - Old projects load and sound the same.
  - A project over the budget loads in priority order, with a note.
- **NEW copies a sparse factory blob** (about 10 KB) into the arena: 7 %
  on the meter. Factory sources are card-only.

## Alternatives considered
- **Keep 32 slots plus #279's 128 KB sample pool.** Two meters, and more RAM
  than AXI has.
- **RAM-only sources, embedded in project files.** That is about 4 s per
  project, and every save rewrites the audio. The Octatrack and the
  Digitakt reference card audio from the project instead.
- **Keep the paper rule and trim `UI_RESERVE` to 16 KiB.** The reserve
  already covers statics it was never sized for; a second guess is still a
  guess. The linker knows.
- **A 96 KiB arena under the paper rule.** It fails the const assert on
  `orbit` (about 59 KB spare against 64 KiB).
- **Move sources under a per-read address mapping** (the first draft). It
  needs a branch per sample read in every TAPE voice, and a subtle
  ordering proof. Deferring until silence and gating note-ons is simpler.
- **A stored reference count per source.** It goes stale between a Sound
  clear and a purge.
- **Purge Sounds too.** No precedent does it, and it would wipe the
  curated palette to save about 0.3 K a Sound.
- **Factory sources in flash.** At 24 KB a second at 16 kHz, against 100 KB
  free.
- **Decode Sounds into the arena,** resident as structs. That is 896 B a
  Sound, the fixed pool again.
- **16-bit sources.** A quarter less time at every rate.
- **Put the arena in D2.** D2 is TRON's rings and VA's.

## Consequences
- `Pool::get` returns an encoded view, so every `pool().get` caller
  changes once.
- NEW's 32 Sounds take 10 240 B, 7 %. That leaves 153 600 B: 4.3 s at
  24 kHz or 6.4 s at 16 kHz. In Sounds alone, it holds about 510 curated
  Sounds, so the 128-slot cap binds first.
- One 2 s, 24 kHz source beside NEW reads 51 %. A TRON instrument's heads
  read 27 %.
- The arena depends on ADR 0090. The delay's repeats carry 16-bit grain,
  and that is the price of the extra 48 KB.
- About 23 KB of flash across the build steps, measured at each.
- A project needs its card's `/CHIMERA/TAPES` to sound its TAPE Parts.
  Orphaned card sources wait for SETTINGS › SYSTEM › STORAGE (#267).

## Sources
- Spec: `docs/superpowers/specs/2026-10-04-sound-pool-budget-design.md`.
- Issues: #337, #279 (TAPE), #317 (factory library), #267 (storage).
- Code:
  - `chimera-core/src/project/` (`pool.rs`, `template.rs`, `codec.rs`,
    `marks.rs`);
  - `chimera-core/src/storage/` (`sound.rs`, `block_codec.rs`,
    `record.rs`);
  - `chimera-core/src/hw.rs`, `chimera-core/src/instrument.rs`,
    `chimera-core/src/dsp/delay.rs`;
  - `chimera-stm32/memory.x`;
  - `chimera-core/tests/memory_budget_test.rs`;
  - fixtures `chimera-core/tests/fixtures/v1/*.snd`.
- ADRs:
  - 0014 and 0025 (memory map, DTCM stack), 0028 (`i16` reverb), 0090
    (`i16` delay line);
  - 0043, 0045, 0046 (projects, card format, load);
  - 0066, 0073, 0078, 0081, 0086.
- Manuals:
  - Polyend Play (§ Samples: Memory bar, Delete Unused Samples);
  - Polyend Tracker v1.5 (§ Sample Loader preview, Delete unused);
  - Elektron Digitakt (§ Samples: VIEW RAM › SELECT UNUSED › UNLOAD);
  - Elektron Octatrack OS 1.25 (§ Memory, PURGE SAMPLES, Static
    streaming);
  - Akai S950 specifications.
