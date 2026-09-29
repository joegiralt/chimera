# Storage Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Chimera reads and writes its SD card. The card works on the chip over SPI2, sits behind a `chimera-hal` `Store` trait with a desktop directory twin, and holds versioned, CRC-checked, A/B-generation files. The first of those files is SYSTEM, which brings THEME back at power-on.

**Architecture:**
- **Functional core** (`chimera-core/src/storage/`), pure and host-tested:
  - the file format: header, TLV records, the must-understand bit and the CRC trailer;
  - the frozen disk-code tables;
  - the block and Sound codecs;
  - the A/B generation rules (`pick`, `write_target`, `delete_order`);
  - the `Card` state machine;
  - SYSTEM.

  All of it sees only bytes in and bytes out through the `Store` trait.
- **Portable shell** (`chimera-fat`, a new `no_std` crate): `embedded-sdmmc` 0.10 behind `Store`, plus a pure MBR and boot-sector parser that yields the volume serial and rejects anything the library would panic on. It is host-tested on a RAM disk.
- **Hardware shell** (`chimera-stm32/src/sd.rs`): SPI2 polled, with embedded-hal 1.0 adapters over `stm32h7xx-hal` 0.16's embedded-hal 0.2 SPI.
- **Desktop shell** (`chimera-desktop/src/store.rs`): a directory.

**Tech Stack:** Rust 2024 (`no_std` core), `embedded-sdmmc` 0.10 (`default-features = false`), `embedded-hal` 1.0 (`chimera-fat`'s `Medium` impl and the chip adapter), `stm32h7xx-hal` 0.16, `just`. No new dependency in `chimera-core`.

**Spec:** `docs/superpowers/specs/2026-09-28-projects-storage-design.md` (binding) at 12fa052, amended with this revision: `Card::Failed { err, last }` and the lent `&Ready`, the A/B pick rule, and the atomic-512 B-block assumption. Sections: § Plans item 1, § Types, § Storage, § Boot step 1, § Tests. The review it answers: `projects-spec-review.md` (C1, C2, H2, H6, H7, M1, M3, M6, M9). This revision answers the plan's adversarial review (see § Review response).

> **Out of scope.** These belong to plan 2 (projects and the ladder):
> - `Project`, `Pool`, `SlotId`, `PartId`, `Origin`, the project records and the load protocol;
> - `LOAD_EPOCH`, `Location`, button edge latching and the NO CARD rung.
>
> These belong to plan 3: library sounds, `SNDINDEX`, `TAGS`, the fresh-card factory library, `TagSet` and the naming screen.
>
> This plan builds only the interfaces those plans consume: `Store`, `Card`/`Ready`, `Decode`, `save_ab`/`load_ab`, the block codec, `ProjectId`, `Name<N>` and `Sound::bits_eq`.

## Global Constraints

Copied from the spec. Every task's requirements include this section.

**Card**
- SD runs in SPI mode on SPI2: SCK PA9, MISO PB14 (pulled up), MOSI PB15. CS is PE12 and there is no card-detect line, **both unconfirmed until the Task 2 STOP**.
- Wake the card with ≥ 74 clocks, CS high, before CMD0. Init at ≤ 400 kHz, then switch to the fast clock.
- Transfers are **polled**, with no DMA. The stack is in DTCM, which DMA1 and DMA2 can't reach, and D2 is full. This supersedes the design doc's DMA2 note.
- Card I/O runs in the UI loop, never on the audio path. A BUSY/SAVING overlay is drawn before any card operation, at boot too. Every operation has a timeout.
- States are `Absent`, `Ready(VolumeId)` and `Failed { err: CardError, last: Option<VolumeId> }`.
  - The card is mounted per operation. Every handle is closed on success and on error. Any card error sets `Failed`; NO CARD sets `Absent`; a file error leaves the state alone.
  - `Failed` or `Absent` becomes `Ready` only after a fresh init and mount.
  - Each mount compares the volume serial and label with the cached `VolumeId`, which `Failed` keeps.
  - Store operations take a `&Ready`, lent only inside `Card::run`.
- FAT16 and FAT32 only, classified by cluster count as `embedded-sdmmc` does. exFAT shows "CARD IS EXFAT: FORMAT FAT32". A boot sector the library can't read safely never reaches the library.

**Files**
- Layout: `/CHIMERA/SYSTEM.A` and `.B`, with global settings; `/CHIMERA/PROJECTS/P0000001.A` and `.B`, a project. The names are 8.3.
- A/B:
  - Every file is a pair. The header carries a `u32` generation.
  - A save truncates and rewrites the side the reader wouldn't take (the older, missing or broken one) with generation + 1, then flushes.
  - The CRC32 is a trailer. The reader takes the valid file with the highest generation. A torn or invalid file is ignored, except one that needs newer firmware, which an older file never shadows.
  - A delete removes the older file first.
  - A 512 B block write is assumed atomic (ADR 0045). Tests characterise the torn case.

**Format**
- The header gives the magic, format version, kind, generation and display name. TLV records follow (`u16` tag, `u16` length, the bytes), then the CRC trailer.
- The must-understand bit:
  - Tag bit 15 means must-understand. An unknown critical record greys the file as "NEEDS NEWER FIRMWARE".
  - An unknown non-critical record is skipped by its length.
- Code tables: `BlockRef::disk_code()`, `ModSource::disk_code()` and a `disk_code` for every stored enum are exhaustive `match` tables. Golden tests pin them. A retired-ids list stops any id from being reused.
- Values:
  - Enums are stored by frozen code, never by UI position.
  - Continuous values are stored as `f32` in spec units.
  - Decode a whole block, then write it through `Block::write` in canonical spec order, KIND before MODE.
  - Every value is clamped and quantised through its `ParamSpec`.
  - A NaN or infinite value is rejected, and its record takes the default.
  - An enum code out of range takes the default in a non-critical record, and greys the file in a critical one.
- A rescaled param gets a new `ParamId` and a pure `migrate(old) -> new`.
- Decoding starts from a frozen base: every param at its neutral default, no routes and an empty registry. It does **not** start from `Sound::init`.
- Routes:
  - Every present route is written, zero amounts included.
  - A route is keyed by (source code, dest block code, dest param id).
  - Routes are bounded by `MAX_MOD_DESTS` and the registry by `MAX_REGISTRY_DESTS`. Record lengths are bounded by the remaining file size.
- Fixtures under `chimera-core/tests/fixtures/v1/` load and render identically in every later version.
- The parser never panics. Neither does anything a card's bytes can reach.

**RAM**
- There is no staging copy and no serialised buffer. Saves stream from live state, and loads use two passes. Nothing new goes in D2.
- Every new static is added to `AXI_RESIDENT`. Fix the stale stack comment on `UI_RESERVE` (ADR 0025).
- No stack frame of 8 KB or more (`just stack-check`).

**SYSTEM**
- It holds `ThemeSettings` (BRIGHT, GAMMA, ACCENT, BLACK) and the last project id, and nothing else.
- It is written on leaving System, only if the bytes changed. It uses A/B.
- Boot step 1: mount the card and read SYSTEM, then apply THEME. With no card, no file or an error, the built-in defaults apply.

**Repo rules (CLAUDE.md, owner)**
- Every `unsafe` has a `// SAFETY:` comment. No heap and no blocking on the audio path. No libc.
- Invariants live in types (newtypes, enums, private fields). Pure core, thin shells.
- Every decision that constrains later work gets an ADR.
- Commits are terse, with no type prefix and no Claude or AI attribution line. Never stage `docs/chimera-ui-ux-spec.md`.
- `just check` (tests, every feature set, clippy `-D warnings`, fmt, stack check) passes before every commit. No task ends red.

## Review Focus

These are the failure modes the spec implies but no spec'd test exercises, most likely first. Each has a test in the task named.

1. **A corrupt or odd boot sector.** Zero sectors per cluster, a total below the metadata, a root cluster of 0, a FAT32 type byte over a FAT16 layout, an exFAT card with no MBR. The parser rejects or classifies it exactly as `embedded-sdmmc` would, before the library can divide by zero or underflow; the unit never halts on a card. Task 1: `corrupt_boot_sector_is_rejected_not_panic`, `fat32_type_byte_with_fat16_layout_is_fat16`, `exfat_superfloppy_is_exfat`. Task 4: `mutated_images_never_panic`.
2. **A card pulled mid-save, then put back, on the truncate path as well as the create path.** A cut between the FAT truncation and the size-0 entry write leaves an old length over a short chain; the reader sees that as a torn side, not a card fault. The next operation re-inits; the event is `Same`; a generation loads and the next save succeeds. Task 11: `cut_at_every_block_write_keeps_a_generation`, `repeated_cuts_keep_a_generation`, `cut_then_reinsert_loads_previous_generation`.
3. **No card.** The mount fails with `NoCard` within `SD_ACQUIRE_MS`, not `Io` after seconds. Leaving System tries exactly once, and the next exit tries again. Task 4: `no_card_is_no_card`; Task 12: `no_card_exit_tries_once`; Task 13 STOP check 2.
4. **A card swapped while idle.** The first operation on the new card succeeds: the mount re-inits and retries once. Task 4: `mount_retries_once_after_reinit`; Task 13 STOP check 4.
5. **A card that fills mid-save,** and a device error during allocation that the library reports as full. `save_ab` returns `Full` and the older generation loads; a pulled card during allocation reads as the fault it is. Task 4: `full_maps_both_library_errors`, `alloc_device_error_is_not_full`; Task 11: `full_card_keeps_previous_generation`.
6. **A torn 512 B write in the directory or FAT sector the pair shares.** It pins the atomic-block assumption: a torn data or entry block never loses the kept side, and no torn block ever loads wrong data. Task 11: `torn_block_breaks_only_shared_sectors`.
7. **Both files of a pair at the same generation** (a user copied `.A` over `.B` on a computer). The newer is chosen deterministically (A), and the next save goes to B with generation + 1. Task 11: `tied_generations_prefer_a_then_write_b`.
8. **A card with no partition table** ("superfloppy" FAT). The clear error `Unsupported::NoPartitionTable`, not `Io` or a panic. Task 1: `superfloppy_is_no_partition_table`.

## Decisions this plan makes where the spec is silent

- **embedded-hal versions.** `stm32h7xx-hal` 0.16 implements only embedded-hal **0.2.7**. `embedded-sdmmc` 0.10 needs 1.0 `SpiDevice<u8>` and `DelayNs`. `chimera-stm32/src/sd.rs` wraps the HAL's blocking `Transfer`/`Write` (0.2) in a 1.0 `SpiDevice` that drives CS itself; `set_hz` rebuilds with `spi_unchecked` because `spi()` consumes the pins. Both embedded-hal majors coexist in the lock. `chimera-fat` keeps its `embedded-hal = "1.0"` dependency: Task 4's `impl Medium for SdCard<S: SpiDevice<u8> + SdBus, D: DelayNs>` names it.
- **SPI kernel clock.** SPI1/2/3 share one kernel clock (`SPI123SEL`, one mux for the group), today `pll1_q_ck` at 200 MHz. The largest SPI divider is 256, so 200 MHz can't go below 781 kHz, which breaks the ≤ 400 kHz init. Move `SPI123SEL` to **PLL2_P at 100 MHz**:
  - the display gets exactly 50 MHz (100 / 2) on both silicon revisions;
  - SD init runs at 390.6 kHz (100 / 256);
  - the fast clock is 12.5 MHz (100 / 8) or 25 MHz (100 / 4), chosen at the Task 2 STOP.

  Precedent: the stock firmware runs SPI1, SPI2 and USART1 from PLL2 (M 1, N 40, P 5 → 64 MHz). Nothing else in Chimera uses PLL2 (SAI is on PLL3; the ADC, PLL2_P's only other reset consumer, is unused). The rejected options: HSI (display 32 MHz), PLL1_Q at 100 MHz (not an integer divide of rev V's 960 MHz VCO), PLL3 (audio's), init above 400 kHz, bit-banging.
- **CS and card-detect: unconfirmed.** Nothing in the repo's docs or code gives them. The only source is `.claude/projects/-home-hermes-dev-chimera/memory/reference_preenfm3_firmware.md` (transcribed from Ixox/preenfm3), which gives **CS = PE12**, AF5 on SPI2, and no card-detect line.
  - The plan builds on PE12 and on "no card-detect": `Absent` is inferred from an acquire failure (`StoreError::NoCard`).
  - The owner confirms both at the Task 2 STOP, against `MX_SPI2_Init` and `MX_GPIO_Init` in the stock firmware's CubeMX `main.c` (github.com/Ixox/preenfm3), or the schematic. Until then no ADR records them as fact. A card-detect line, if there is one, is filed as a follow-up issue, not built here.
- **SPI mode.** MODE 0, the SD standard. If the card doesn't acquire, try MODE 3 (SD's other SPI mode), then MODE 1 (the stock firmware's CPHA = 2EDGE). The probe tries all three in that order.
- **Volume serial and the boot-sector gate.** `embedded-sdmmc` 0.10 doesn't expose the serial, and its `Bpb::create_from_bytes` divides by sectors-per-cluster and subtracts the metadata from the total unchecked; a zero root cluster underflows later. `chimera_fat::volume` parses the MBR and the boot sector itself, validates every field the library divides by or trusts, and classifies FAT16/FAT32 by cluster count exactly as the library does (< 4085 unsupported, < 65 525 FAT16, else FAT32 with version 0). `FatStore::mount` runs it before `open_raw_volume`. The same parse detects exFAT, in a 0x07 partition or with no MBR.
- **Error classification lives in the medium.** `embedded-sdmmc` maps every `SpiDevice` error to `Error::Transport`, and `FatStore<D: Medium>` can't read a generic `D::Error`. So `Medium::classify(&self, &Self::Error) -> StoreError` classifies device errors, and `Medium::fault(&self) -> Option<StoreError>` reports a fault the medium saw during the operation, which the library may have hidden behind `DiskFull`. The SD adapter keeps a `timed_out` flag and a `BusPhase` (`Acquire` or `Data`): a failure while acquiring is `NoCard`; a timeout after it is `Timeout`.
- **Timeouts.** One deadline per whole operation is wrong both ways: a missing card would take seconds of CMD0 retries, and a FAT scan or plan 2's 60 KB save can legitimately pass 2 s. So:
  - `SD_ACQUIRE_MS = 1500`: one whole acquire; the shell tries a second acquire (fresh `wake`) when the first fails with a card present, after the #186 presence check (`AcquireOpts { acquire_retries: SD_ACQUIRE_RETRIES = 3, use_crc: true }`); a missing card is `NoCard` within it;
  - `SD_IDLE_MS = 600`: no 512 B block moved for this long is `Timeout` (above SDHC's 500 ms write busy);
  - `SD_OP_CAP_MS = 10_000`: a backstop per operation.

  The adapter counts on the DWT cycle counter, which `sd::init` enables itself (it isn't running without `perf-probe`). If the counter won't start, the deadline counts transactions instead (each moves ≥ 8 bits at ≤ SCK, so the count bounds time from below). The probe measures the worst gap and the Task 2 STOP confirms the numbers.
- **Idle swaps.** `reinit` runs only after an error, so after a quiet swap the next operation talks to a card that is still in SD mode. `FatStore::mount` re-inits and retries once when its first block-0 read fails.
- **Chain inconsistency is a file error.** A cut on the truncate path can leave an old length over a one-cluster chain; `read` then fails with `EndOfFile`, `BadCluster` or `UnterminatedFatChain`. `FatStore` maps those to `StoreError::Corrupt`, `check_file` to a torn side, and `Card` doesn't count it as a card fault.
- **`Ready` is a capability.** It is neither `Copy` nor `Clone`, its field is private, and `Card::run` lends it as `&Ready` for one closure; the borrow is its lifetime bound. `CardError` has no NO CARD variant, so `Failed(NoCard)` can't be built. The optional `Busy` token (drawing BUSY as a precondition of `run`) is not taken: it would force every core test to draw, and the two call sites are checked by the Task 13 STOP.
- **The A/B pick rule.** `pick` loads the newest side that decodes; a side that needs newer firmware is refused rather than shadowed; both sides broken gives the newest error, not "missing". `write_target` writes the side `pick` doesn't keep. `save_ab` classifies sides by framing only (KIND, header, CRC, the must-understand bit), because it has no decoder.
- **The streaming serialiser is push, not pull.** The spec says "an iterator of ≤ 512 B chunks". Instead, the encoder writes records into a `ByteSink` that the store buffers in one 512 B block and flushes per block. The streaming, no-copy and running-CRC properties are the same, and there's no resumable state machine. Records are ≤ `MAX_RECORD_LEN` = 512 B and are built on the stack.
- **The fuzz test.** It is a seeded xorshift property test, the repo's pattern (`tests/property_test.rs`, no `proptest`), plus a `cargo fuzz` target in `chimera-core/fuzz/` (excluded from the workspace; `just fuzz` needs `cargo install cargo-fuzz`).
- **Compile-fail tests** are rustdoc `compile_fail` doc tests, pinned by error code where rustc gives one, with no `trybuild` dependency.
- **`Sound.name` becomes `SoundName` (`Name<16>`).** `Sound::init`'s `(init)` isn't a valid name, so it becomes `INIT`. Tests that match `(init)` change, and any screen golden that shows it is re-recorded in Task 8 (ADR 0011: the spec's `Name` type makes the change).
- **`Name` rejects leading and trailing spaces** (`NameError::EdgeSpace`), so no name shows blank. Rejecting keeps the stored bytes what the user typed; the owner may prefer trimming, which is a one-line change in `Name::new`.
- **Every `ParamKind::Enum` param gets a frozen code**, not only the enums the spec lists. That adds `EnvSpeed`, `HoldPos`, `FuncMode`, `LfoForm`, `ResonatorMode`, the LFO SHAPE and SYNC, the chorus MODE, the comp RATIO, `Gamma`, `Accent`, `Bright`, `Black` and the channel. A test fails if any Enum spec lacks one.
- **`Sound::bits_eq`** is listed under plan 2's derived marks, but the round-trip test needs it, so it lands here. It compares only observable state (routes and dests up to `num_dests`, registry entries up to `len`), because plan 2's marks rely on it.

## File structure

| File | Responsibility |
|---|---|
| `chimera-hal/src/store.rs` | `Store` trait and its vocabulary: `VolumeId`, `Unsupported`, `Dir`, `FileName`, `StoreError`, `ReadSink`, `ByteSink`, `CHUNK`. |
| `chimera-hal/src/testkit.rs` (feature `testkit`) | `MemStore`, and `store_suite`, the conformance suite every `Store` passes. |
| `chimera-fat/` (new crate) | `volume.rs`, a pure MBR and boot-sector parser and validator; `store.rs`, `FatStore` over `embedded-sdmmc`, plus `Medium`, `SdBus` and `BusPhase`. `tests/common/image.rs` is a FAT16/FAT32/exFAT image builder, with `RamDisk` and `CutDisk`. |
| `chimera-stm32/src/sd.rs` | SPI2 pins and clock; `SdSpi` (a 1.0 `SpiDevice` with idle and acquire deadlines); `CycleDelay`; `SdStore`; `take_store`. |
| `chimera-stm32/src/sd_probe.rs` (feature `sd-probe`) | The bench-style bring-up probe. |
| `chimera-desktop/src/store.rs` | `DirStore`. |
| `chimera-core/src/name.rs` | `Name<N>`, `SoundName`, `ProjectName`. |
| `chimera-core/src/storage/mod.rs` | Module root and re-exports. |
| `chimera-core/src/storage/crc.rs` | `Crc32`, CRC-32/ISO-HDLC. |
| `chimera-core/src/storage/frame.rs` | `Header`, `FileKind`, `Generation`, `Side`, `ProjectId`, the push `Framer`, `FileError`. |
| `chimera-core/src/storage/record.rs` | `RecordTag`, `ReadTag`, `RecordWriter`, `RecordBuf`, `write_file`. |
| `chimera-core/src/storage/codes.rs` | `BlockRef`/`ModSource`/`EngineType` disk codes, `RETIRED`, `MIGRATIONS`, `ValidAddr`, `DiskValue`. |
| `chimera-core/src/storage/block_codec.rs` | Block record encode and decode: canonical order and clamping. |
| `chimera-core/src/storage/sound.rs` | Sound records, `Sound::neutral`, `SoundDecoder`. |
| `chimera-core/src/storage/card.rs` | `Card`, `CardError`, `Ready`, `CardEvent`, `CardFault`. |
| `chimera-core/src/storage/file.rs` | `Decode`, two-pass `load_file`, streaming save, `AbFile`, `SideState`, `Pick`, `save_ab`/`load_ab`/`delete_ab`. |
| `chimera-core/src/storage/system.rs` | `SystemSettings`, `SystemDecoder`, `SystemSync`. |
| `chimera-core/src/ui/busy.rs` | `draw_busy`. |
| `chimera-core/tests/fixtures/disk_codes_v1.txt`, `tests/fixtures/v1/*` | The frozen code table and the v1 files. |
| `docs/adr/0045-card-format.md` | The format and card-access ADR. |

## Task order

1. Volume parser and the `chimera-fat` crate (committed at 099f251; reopened for a fix round).
2. SD on the chip: SPI2 and the probe. **Hardware STOP.**
3. The `Store` trait, `MemStore` and the conformance suite.
4. `FatStore` passes the suite on a RAM disk.
5. `DirStore` passes the suite.
6. Framing: CRC, `Name`, header, records, the framer, and ADR 0045.
7. Frozen code tables.
8. The block and Sound codecs; the round trip.
9. v1 fixtures, compatibility, corruption and fuzz.
10. The `Card` state machine.
11. Two-pass load, streaming save and A/B; the power cut.
12. SYSTEM and the BUSY overlay.
13. Shell wiring: boot SYSTEM, save on leaving System, the RAM budget. **Hardware STOP.**

Tasks 3–5 don't depend on 6–9, and 6–9 don't depend on 2, except that ADR 0045's pin section waits for the Task 2 STOP (Task 6, Step 5). Task 2 blocks Task 13.

---

### Task 1: Volume parser and the `chimera-fat` crate

**Files:**
- Create: `chimera-fat/Cargo.toml`, `chimera-fat/src/lib.rs`, `chimera-fat/src/volume.rs`
- Create: `chimera-fat/tests/common/image.rs`, `chimera-fat/tests/volume_test.rs`
- Create: `chimera-hal/src/store.rs`; Modify: `chimera-hal/src/lib.rs` (`pub mod store;`)
- Modify: `Cargo.toml` (members, default-members), `Justfile` (`test`, `check`, `clippy` include `-p chimera-fat`)

**Interfaces:**
- Produces, in `chimera-hal/src/store.rs` (the file is created here with only these types; Task 3 adds the trait):
  - `pub struct VolumeId { pub serial: u32, pub label: [u8; 11] }` (Copy, Eq, Debug);
  - `pub enum Unsupported { Exfat, NoPartitionTable, NotFat(u8), BadBootSector }`. `NotFat` carries the MBR type byte, for the log only.
- Produces, in `chimera_fat::volume`:
  - `pub enum FsKind { Fat16, Fat32 }`;
  - `pub enum PartitionType { Fat, ExfatOrNtfs, Other(NonZeroU8) }` (`Fat` is 0x04, 0x06, 0x0E, 0x0B, 0x0C; `ExfatOrNtfs` is 0x07, where the boot sector decides; an empty entry, 0, is no partition);
  - `pub struct Partition { pub lba: u32, pub kind: PartitionType }`;
  - `pub fn first_partition(mbr: &[u8; 512]) -> Result<Partition, Unsupported>`;
  - `pub fn boot_sector(bs: &[u8; 512], kind: PartitionType) -> Result<(FsKind, VolumeId), Unsupported>`.
  - Every BPB offset and limit is a named `const` (`BPB_BYTES_PER_SECTOR = 11`, `BPB_SECTORS_PER_CLUSTER = 13`, `BPB_RESERVED = 14`, `BPB_NUM_FATS = 16`, `BPB_ROOT_ENTRIES = 17`, `BPB_TOTAL16 = 19`, `BPB_FAT_SIZE16 = 22`, `BPB_TOTAL32 = 32`, `BPB_FAT_SIZE32 = 36`, `BPB_FS_VER = 42`, `BPB_ROOT_CLUSTER = 44`, `BPB_FS_INFO = 48`, `BS_VOL_ID16 = 0x27`, `BS_VOL_ID32 = 0x43`, `OEM_NAME = 3..11`, `SIGNATURE = 510`, `MBR_ENTRY = 446`, `FAT12_CLUSTERS = 4085`, `FAT16_CLUSTERS = 65_525`).
- Produces for tests (`tests/common/image.rs`):
  - `RamDisk` (`embedded_sdmmc::BlockDevice`, `RefCell<Vec<[u8; 512]>>`);
  - `fn fat16(blocks: u32, serial: u32) -> RamDisk`, `fn fat32(serial: u32) -> RamDisk` (≥ 65 525 clusters), `fn exfat() -> RamDisk`, `fn exfat_superfloppy() -> RamDisk`, `fn superfloppy() -> RamDisk`;
  - `CutDisk { inner: RamDisk, writes_left: Cell<Option<u32>> }`, which fails every write call after `writes_left` reaches 0. It is the one item with `#[allow(dead_code)]` (volume_test doesn't use it; Task 4 does). Per-block counting and torn writes are added in Task 11, which first needs them.

The boot-sector rules (`boot_sector`), in order:
1. `kind` gate: `Fat` continues; `ExfatOrNtfs` is `Exfat` when the OEM name is `EXFAT   `, else `NotFat(0x07)`; `Other(k)` is `NotFat(k)`.
2. Signature 0xAA55, else `NotFat(byte)`.
3. Validation, each failure `BadBootSector`: bytes per sector == 512; sectors per cluster ∈ {1, 2, 4, …, 128}; reserved sectors ≥ 1; FAT count ∈ {1, 2}; FAT size (FATSz16, else FATSz32) ≥ 1; total sectors (TotSec16, else TotSec32) > reserved + FATs × FAT size + root-dir sectors.
4. Cluster count = data sectors / sectors per cluster, as the library computes it. < 4085 → `NotFat(byte)` (FAT12); < 65 525 → FAT16; else FAT32.
5. FAT16 needs root entries ≥ 1. FAT32 needs FS version 0, a root cluster in `2..cluster_count + 2`, and an FSInfo sector in `1..reserved`. Else `BadBootSector`.
6. The serial and label come from the offsets of the **classified** type (0x27/0x2B for FAT16, 0x43/0x47 for FAT32), never from the type byte.

`first_partition`: block 0 whose OEM name is `EXFAT   ` → `Exfat` (an exFAT card with no MBR), checked first. Then a missing signature, a FAT boot sector in block 0 (jump 0xEB/0xE9 and `FAT` at 0x36 or 0x52), an entry with a bad status, type 0 or LBA 0 → `NoPartitionTable`.

- [x] **Step 1: Write the failing tests** in `chimera-fat/tests/volume_test.rs`:
  - `fat16_serial_and_label`: `fat16(16_384, 0xDEAD_BEEF)` → `boot_sector` gives `(FsKind::Fat16, VolumeId { serial: 0xDEAD_BEEF, label: *b"CHIMERA    " })`.
  - `fat32_serial_and_label`: the same at 0x43/0x47, `FsKind::Fat32`.
  - `exfat_is_unsupported`: `first_partition` gives `ExfatOrNtfs`, then `boot_sector` gives `Err(Unsupported::Exfat)`.
  - `type_07_without_exfat_oem_is_not_fat` → `NotFat(0x07)`.
  - `superfloppy_is_no_partition_table` (Review Focus 8), `superfloppy_with_plausible_entry_is_no_partition_table`, `bad_signature_is_no_partition_table`, `empty_first_entry_is_no_partition_table`, `bad_status_is_no_partition_table`.
  - `other_partition_type`: type 0x83 → `PartitionType::Other(0x83)`, then `NotFat(0x83)`.
  - `every_fat_partition_type_is_accepted`: 0x04, 0x06, 0x0E, 0x0B, 0x0C → `PartitionType::Fat`.
  - `images_open_in_embedded_sdmmc`: `VolumeManager::new(fat16(..), FixedTime)` opens volume 0 and `make_dir_in_dir(root, "CHIMERA")` succeeds, for both FAT16 and FAT32. This checks the image builder.
  - The fix-round tests below.
- [x] **Step 2: Run** `cargo test -p chimera-fat` → FAIL (crate missing).
- [x] **Step 3: Implement.**
  - `chimera-fat/Cargo.toml`: `no_std` lib; deps `chimera-hal`, `embedded-sdmmc = { version = "0.10", default-features = false }`, `embedded-hal = "1.0"` (used from Task 4).
  - `volume.rs` as the rules above.
  - The image builder writes an MBR (one partition at LBA 2048), a boot sector with 1 sector per cluster and 2 FATs, zeroed FATs with the media entries, and an empty root directory.
- [x] **Step 4: Run** `cargo test -p chimera-fat` → PASS. Then `just check` → PASS.
- [x] **Step 5: Commit** `git commit -m "chimera-fat: volume serial and FS type from the MBR and boot sector"` (099f251)

#### Task 1 fix round

What changes versus 099f251, and nothing else:

- **(a) H5: the boot sector is validated and classified as `embedded-sdmmc` 0.10 reads it** (rules 3–6 above). 099f251 checked only the signature and picked FAT32 by `BPB_FATSz16 == 0`. New tests:
  - `corrupt_boot_sector_is_rejected_not_panic`: from a valid FAT16 image, one mutation per case → `Err(BadBootSector)`: bytes per sector 1024; sectors per cluster 0; sectors per cluster 3; reserved 0; FAT count 0; FAT count 3; FAT size 0; total below the metadata. Plus from a valid FAT32 image: FS version 1; root cluster 0; FSInfo 0. And a FAT12-sized volume (4 084 clusters) → `NotFat(0x06)`.
  - `fat32_type_byte_with_fat16_layout_is_fat16`: a FAT16 image with MBR type 0x0C → `(Fat16, serial from 0x27)`.
  - `classification_matches_embedded_sdmmc`: for images at 4 085, 65 524 and 65 525 clusters, `boot_sector`'s `FsKind` equals the FAT type `VolumeManager` opens (FAT16 root dir vs FAT32 root cluster, seen through `open_root_dir` + `iterate_dir` working).
- **(b) An exFAT superfloppy reports `Exfat`.** `first_partition` checks the OEM name of block 0 first. Test `exfat_superfloppy_is_exfat` on a new `exfat_superfloppy()` image.
- **(c) `PartitionType` replaces the raw `u8`** in `Partition::kind` and `boot_sector`'s parameter; `Other` holds a `NonZeroU8`. Tests that compared `p.kind` to bytes compare variants.
- **(d) Named constants** for every BPB and MBR offset and the two cluster thresholds; no bare offset literals remain in `volume.rs`.
- **(e) `#![allow(dead_code)]` in `tests/common/image.rs` narrows** to `#[allow(dead_code)]` on `CutDisk` and its impl.
- **`Unsupported::BadBootSector`** is added to `chimera-hal/src/store.rs` (a damaged FAT volume isn't "not FAT"). Task 3 gives it a message.
- **`embedded-hal` stays** in `chimera-fat/Cargo.toml`: Task 4 uses it.

- [ ] **Fix Step 1:** write the fix-round tests → `cargo test -p chimera-fat` FAILS on them.
- [ ] **Fix Step 2:** implement (a)–(e) → `cargo test -p chimera-fat` PASS; `just check` PASS.
- [ ] **Fix Step 3: Commit** `git commit -m "chimera-fat: check the boot sector as embedded-sdmmc reads it"`

### Task 2: SD on the chip — SPI2 and the probe (hardware STOP)

**Files:**
- Create: `chimera-stm32/src/sd.rs`, `chimera-stm32/src/sd_probe.rs`
- Modify: `chimera-stm32/Cargo.toml` (deps `chimera-fat`, `embedded-sdmmc` (no default features), `embedded-hal = "1.0"`; feature `sd-probe = []`)
- Modify: `chimera-stm32/src/clocks.rs`:
  - `.pll2_p_ck(100.MHz())`; drop the now-unused `.pll1_q_ck(200.MHz())`;
  - `ccdr.peripheral.SPI1 = ccdr.peripheral.SPI1.kernel_clk_mux(Spi123ClkSel::Pll2P)` (it consumes the rec), before any `spi()` call;
  - `assert!(ccdr.clocks.pll2_p_ck() == Some(100.MHz()))`, so a strategy change can't move it silently;
  - move `enable_cycle_counter` and `counting` here from `probe.rs` (always compiled); `probe::init` calls it.
- Modify: `chimera-stm32/src/main.rs`: `#[cfg(feature = "sd-probe")] mod sd; #[cfg(feature = "sd-probe")] mod sd_probe;` (Task 13 removes the first `cfg`); under `sd-probe`, split what the probe needs and call `sd_probe::run(&mut display, clk, sd)` after `display.init`, before `bench::run`. No build gains an unused binding.
- Modify: `Justfile`: build, clippy and stack-check `--features sd-probe`; new recipe `flash-sd-probe`.

**Interfaces:**
- Consumes: `chimera_fat::volume::{first_partition, boot_sector}` (Task 1).
- Produces, in `sd.rs`:
  - `pub const SD_INIT_HZ: u32 = 400_000;`
  - `pub const SD_FAST_HZ: u32` (12 500 000 until the STOP decides);
  - `pub const SD_MODE: spi::Mode` (`MODE_0` until the STOP decides);
  - `pub const SD_ACQUIRE_RETRIES: u32 = 3;`, `pub const SD_ACQUIRE_MS: u32 = 1_500;`, `pub const SD_IDLE_MS: u32 = 600;`, `pub const SD_OP_CAP_MS: u32 = 10_000;`
  - `pub struct SdSpi`: owns `Spi<SPI2, Enabled>`, CS `PE12`, the SPI2 `rec` (returned by `free()`) and a `chimera_fat::deadline::Deadline` (`arm` on DWT cycles, `arm_transfers` budgeted from SCK when the DWT won't count); implements `embedded_hal::spi::SpiDevice<u8>`, with `ErrorType::Error = SdSpiError { Spi, Timeout }`;
  - `impl SdSpi { pub fn new(..) -> Self; pub fn set_hz(&mut self, hz: Hertz); pub fn set_mode(&mut self, m: spi::Mode); pub fn wake(&mut self); pub fn arm(&mut self, idle_ms: u32); pub fn timed_out(&self) -> bool; }`
    - `set_hz`/`set_mode` do `free()` and rebuild with `spi_unchecked`;
    - `wake` sends 10 × 0xFF with CS high (≥ 74 clocks);
    - `arm` starts the operation: clears `timed_out`, and sets the idle deadline and the `SD_OP_CAP_MS` cap. The idle deadline restarts on every transaction that moves ≥ 512 B;
    - `#[cfg(feature = "sd-probe")] pub fn max_gap_us(&self) -> u32`, the longest idle gap seen;
  - `pub struct CycleDelay { cpu_hz: u32 }`, which implements `embedded_hal::delay::DelayNs` through `clocks::delay_us` (no second cycle conversion);
  - `pub type SdDevice = embedded_sdmmc::SdCard<SdSpi, CycleDelay>`;
  - `pub fn init(spi2, rec, pa9, pb14, pb15, pe12, dcb, dwt, clocks, cpu_hz) -> SdDevice`: enables the cycle counter (`clocks::enable_cycle_counter`), MISO pull-up, CS high, `SD_MODE` at `SD_INIT_HZ`, `wake`, then `SdCard::new_with_options(.., AcquireOpts { use_crc: true, acquire_retries: SD_ACQUIRE_RETRIES })`.

- [ ] **Step 1: Wire the pins and clock.**
  - SCK PA9, MISO PB14 (internal pull-up: SD needs DO pulled up, and a floating MISO makes the no-card path random) and MOSI PB15 go to `into_alternate::<5>()` at `Speed::High`. CS PE12 is push-pull, driven high before the SPI starts.
  - On every transfer, `SdSpi` checks the deadline and returns `SdSpiError::Timeout` past it, setting `timed_out`.
  - `SpiDevice::transaction`: CS low; for each `Operation`, `Read` fills 0xFF and `Transfer::transfer`, `Write` uses `Write::write`, `Transfer`/`TransferInPlace` copy and transfer, and `DelayNs` uses `CycleDelay`; then CS high on every path.
- [ ] **Step 2: Write `sd_probe::run(display, clk, sd: SdDevice) -> !`.** It prints each line with `FmtBuf` as `bench.rs` does:
  1. the kernel clock and the actual init SCK, from `Spi2::kernel_clk_unwrap(&ccdr.clocks)` / divider; it must read ≤ 400 kHz; and whether the cycle counter runs;
  2. acquire: `sd.num_bytes()` and `get_card_type()`, or the error, and the milliseconds it took. If it fails, rebuild in MODE 3, then MODE 1, and print the result of each;
  3. block 0 and the partition boot sector through `first_partition`/`boot_sector`: `FsKind`, serial in hex, label;
  4. `VolumeManager` opens volume 0 and lists the first 8 root entries, with the mount time in ms;
  5. write `/CHIMPROB.TXT` (16 KB pattern, generated 512 B at a time; no 16 KB buffer), read it back, compare, then delete it. Do this at 12.5 MHz, then again at 25 MHz, printing OK/FAIL, KB/s and `max_gap_us` for each;
  6. `size_of` of `Sound`, `SoundPool`, `Performance`, `UiState` and `TripleBuffer<AudioShared>`, then `AXI_SRAM − AXI_RESIDENT` and `VOICE_RAM_BUDGET − size_of::<Instrument>()` (spec: "Plan 1 re-measures them on the chip").

  Then it loops forever with the LED on. No audio starts in this build.
- [ ] **Step 3: Build.** `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features sd-probe` → OK. Then `just check` → PASS: the default and `--no-default-features` builds don't compile `sd`, so nothing is dead; the stack check includes `sd-probe`.
- [ ] **Step 4: Check the display on the default build.** `just flash` on the owner's unit, if already at the bench, or defer it to the STOP: the display still inits and draws at 50 MHz from PLL2_P.
- [ ] **Step 5: Commit** `git commit -m "SD on SPI2, polled, and a bring-up probe"`
- [ ] **Step 6: STOP. Ask the owner to run `just flash-sd-probe` with a FAT32 card, then an exFAT card and no card, and wait.** The owner reports:
  - every probe line;
  - whether CS = PE12 matches `MX_SPI2_Init`/`MX_GPIO_Init` in the stock `main.c` or the schematic, and whether the TFT module's SD slot has a card-detect line (if so, file a follow-up issue with the pin);
  - which SPI mode acquired and which fast clock passed readback;
  - the no-card acquire time (must be ≤ `SD_ACQUIRE_MS` plus the wake) and the worst `max_gap_us`.

  Record the answers under `## Measured`. Set `SD_FAST_HZ` to the fastest clock that passed (never above 25 MHz), `SD_MODE` to the mode that acquired, and `SD_IDLE_MS` to at least twice the worst gap if 600 ms is too tight. `SD_IDLE_MS` never goes below 600. If CS isn't PE12, fix it before Task 13. If Task 6 has already written ADR 0045, fill its pending pin section now. Commit `git commit -m "SD clock, mode and pins from the probe"`.

### Task 3: The `Store` trait, `MemStore` and the conformance suite

**Files:**
- Modify: `chimera-hal/src/store.rs` (add to Task 1's types), `chimera-hal/src/lib.rs` (`#[cfg(feature = "testkit")] pub mod testkit;`)
- Modify: `chimera-hal/Cargo.toml` (feature `testkit = []`; `testkit.rs` uses `extern crate std`)
- Create: `chimera-hal/src/testkit.rs`, `chimera-hal/tests/mem_store_test.rs`

**Interfaces:**
- Produces, in `chimera_hal::store`:

```rust
pub const CHUNK: usize = 512;
pub enum Dir { Chimera, Projects, Sounds }         // /CHIMERA, /CHIMERA/PROJECTS, /CHIMERA/SOUNDS
pub struct FileName { /* private: dir, stem [u8; 8], ext [u8; 3], lens */ }
impl FileName {
    pub fn new(dir: Dir, stem: &[u8], ext: &[u8]) -> Option<FileName>; // A–Z 0–9, 1..=8 / 0..=3
    pub fn dir(&self) -> Dir; pub fn stem(&self) -> &[u8]; pub fn ext(&self) -> &[u8];
}
pub enum StoreError { NoCard, Unsupported(Unsupported), NotFound, Full, Timeout, VolumeChanged(VolumeId),
                      Corrupt /* the file's chain or length is inconsistent: a file error, not a card fault */, Io }
impl StoreError { pub fn message(self) -> &'static str; } // "NO CARD", "CARD IS EXFAT: FORMAT FAT32",
    // "CARD HAS NO PARTITION TABLE", "CARD IS NOT FAT16/FAT32", "CARD FORMAT IS DAMAGED", "FILE NOT FOUND",
    // "CARD FULL", "CARD TIMEOUT", "CARD CHANGED", "FILE IS DAMAGED", "CARD ERROR"
pub trait ByteSink { fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError>; }
pub trait ReadSink {
    fn begin(&mut self, len: u32) -> ControlFlow<()>;
    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()>;   // ≤ CHUNK bytes
}
pub trait Store {
    /// (Re)init the medium if needed, open the volume, read its id, close it.
    fn mount(&mut self) -> Result<VolumeId, StoreError>;
    /// Each op below: open volume, check id == vol (else VolumeChanged, nothing touched), act, close every handle.
    fn list(&mut self, vol: VolumeId, dir: Dir, f: &mut dyn FnMut(FileName, u32)) -> Result<(), StoreError>;
    fn read(&mut self, vol: VolumeId, file: FileName, sink: &mut dyn ReadSink) -> Result<(), StoreError>;
    fn write(&mut self, vol: VolumeId, file: FileName,
             body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>) -> Result<u32, StoreError>;
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError>;
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError>; // exists → Ok
}
```

- Produces, in `chimera_hal::testkit`:
  - `pub struct MemStore` (a `BTreeMap` of files, a `VolumeId`, a `present: bool` flag), with `pub fn new(serial: u32) -> Self`, `pub fn swap(&mut self, serial: u32)` (new empty volume) and `pub fn eject(&mut self)`;
  - `pub fn store_suite<S: Store>(make: &mut dyn FnMut() -> S, swap: &mut dyn FnMut(&mut S), after_op: &mut dyn FnMut(&S))`. `after_op` runs after every suite step (Task 4 checks handles there; `MemStore` passes a no-op).

- [ ] **Step 1: Write the failing test.** `mem_store_test.rs::mem_store_passes_suite` calls `store_suite(&mut || MemStore::new(1), &mut |s| s.swap(2), &mut |_| {})`. The suite asserts:
  - `mount` twice gives the same `VolumeId`;
  - write then read gives identical bytes for sizes 0, 1, 511, 512, 513 and 5 000, and `begin` gets the exact length;
  - writing a shorter file over a longer one truncates;
  - `list(Dir::Chimera)` gives each written name with its size; `list` of a missing dir gives `NotFound`;
  - `read` and `delete` of a missing file give `NotFound`;
  - `make_dir` twice gives `Ok`;
  - a `ReadSink` that breaks after the first chunk gives `Ok`, and the next operation works;
  - a `body` that returns `Err(Io)` after 700 bytes propagates `Io`, and the next operation works;
  - after `swap`, an op with the old `VolumeId` gives `VolumeChanged(new)` and doesn't write, and `mount` gives the new id.
  - `messages`: `StoreError::Unsupported(Unsupported::Exfat).message() == "CARD IS EXFAT: FORMAT FAT32"` (the spec's exact text), and every variant's message (every `Unsupported` included) is non-empty ASCII uppercase.
- [ ] **Step 2: Write the compile-fail doc test** on `FileName`: ```` ```compile_fail,E0451 ```` building `FileName { .. }` with fields outside the crate.
- [ ] **Step 3: Run** `cargo test -p chimera-hal --features testkit` → FAIL.
- [ ] **Step 4: Implement** `store.rs` and `testkit.rs`. `MemStore::write` buffers in a `Vec` and commits on `Ok`, then keeps what was written on `Err` (the same as FAT). An ejected `MemStore` gives `NoCard` from every method.
- [ ] **Step 5: Run** `cargo test -p chimera-hal --features testkit` → PASS. In `Justfile`, the multi-package `test`, `check` and `clippy` lines gain `--features chimera-hal/testkit` (a bare `--features` is rejected with several `-p`). Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "Store trait, MemStore and the conformance suite"`

### Task 4: `FatStore` passes the suite on a RAM disk

**Files:**
- Create: `chimera-fat/src/store.rs`, `chimera-fat/tests/fat_store_test.rs`, `chimera-fat/tests/sd_medium_test.rs`
- Modify: `chimera-fat/src/lib.rs`, `chimera-fat/Cargo.toml` (dev-dep `chimera-hal` with `testkit`), `chimera-fat/tests/common/image.rs` (`impl Medium for RamDisk {}`; `impl Medium for CutDisk` with `fault()` → `Some(Io)` once cut; its `#[allow(dead_code)]` stays, since volume_test doesn't use it)

**Interfaces:**
- Consumes: `Store` and everything in `chimera_hal::store` (Task 3); `volume::*` (Task 1).
- Produces, in `chimera_fat`:

```rust
pub trait Medium: embedded_sdmmc::BlockDevice {
    fn start_op(&self) {}                                     // arm the medium's deadline for one Store op
    fn reinit(&self) {}                                       // after an error: wake, slow clock, re-acquire next access
    fn mounted(&self) {}                                      // volume opened: fast clock
    fn classify(&self, _e: &Self::Error) -> StoreError { StoreError::Io }
    fn fault(&self) -> Option<StoreError> { None }            // a fault seen since start_op (the library may hide it)
}
pub enum BusPhase { Acquire, Data }                           // Acquire: init clock, SD_ACQUIRE_MS; Data: fast clock, SD_IDLE_MS
pub trait SdBus {
    fn wake(&mut self);                                       // ≥ 74 clocks, CS high, at the init clock
    fn set_phase(&mut self, p: BusPhase);
    fn phase(&self) -> BusPhase;
    fn start_op(&mut self);
    fn timed_out(&self) -> bool;
}
impl<S: SpiDevice<u8> + SdBus, D: DelayNs> Medium for SdCard<S, D> {
    // start_op: spi(start_op). reinit: spi(set_phase(Acquire)), spi(wake), mark_card_uninit.
    // mounted: spi(set_phase(Data)).
    // classify: CardNotFound → NoCard; Transport or a Timeout* error with timed_out() → NoCard in Acquire,
    //           Timeout in Data; the rest → Io.
    // fault: timed_out() → Some(the same classification), else None.
}
pub struct FatStore<D: Medium, T: TimeSource> { /* VolumeManager */ }
impl<D: Medium, T: TimeSource> FatStore<D, T> { pub fn new(dev: D, time: T) -> Self; pub fn has_open_handles(&self) -> bool; }
pub struct FixedTime;                                         // 2026-01-01 00:00, impl TimeSource
```

- [ ] **Step 1: Write the failing tests** in `fat_store_test.rs` (FAT16 images unless named):
  - `fat16_passes_suite` and `fat32_passes_suite`: `store_suite(&mut || FatStore::new(fat16(16_384, 1), FixedTime), &mut |s| *s = FatStore::new(fat16(16_384, 2), FixedTime), &mut |s| assert!(!s.has_open_handles()))`.
  - `exfat_mount_is_unsupported`, `exfat_superfloppy_mount_is_unsupported`: `Err(StoreError::Unsupported(Unsupported::Exfat))`.
  - `superfloppy_mount_is_unsupported`: `NoPartitionTable`.
  - `corrupt_bpb_mount_is_unsupported`: sectors per cluster 0 → `Unsupported(BadBootSector)`, and `open_raw_volume` is never reached (no panic).
  - `mutated_images_never_panic`: 2 000 xorshift seeds; a FAT16 image holding one 3-cluster file; 1–8 random byte mutations in the MBR, boot sector, FATs or root directory; then `mount`, `list`, `read` and `write`. Each returns (any `Result`); none panics. If a seed panics inside `embedded-sdmmc`, add a guard in `FatStore` before that call if the bytes allow one; otherwise file an upstream issue and a GitHub issue, and exclude that mutation class with a comment naming both.
  - `error_calls_reinit`: a `Medium` wrapper that counts `reinit`. A `CutDisk` write failure → `Err(Io)` and `reinit` count 1; the next `mount` → `Ok`.
  - `mount_retries_once_after_reinit` (Review Focus 4): a wrapper whose first block read fails → `mount` is `Ok` with `reinit` count 1; one whose first two reads fail → `Err`, `reinit` count 2 (the retry's own error), and no third read.
  - `full_maps_both_library_errors` (Review Focus 5): a card with no free cluster; writing a new 1 B file (`NotEnoughSpace` in the library) → `Full`; a card with one free cluster; writing 2 clusters (`DiskFull`) → `Full`.
  - `alloc_device_error_is_not_full`: a `CutDisk` cut during cluster allocation, which the library reports as `DiskFull` → `Io`, from `fault()`.
  - `chain_errors_read_as_corrupt`: a hand-edited image whose entry says 3 clusters over a 1-cluster chain → `read` gives `Err(Corrupt)`, and `reinit` isn't called.
- [ ] **Step 2: Write the failing tests** in `sd_medium_test.rs`. `FakeBus` implements `SpiDevice<u8>` + `SdBus`; `NoDelay` implements `DelayNs`. Both log calls.
  - `no_card_is_no_card` (Review Focus 3): MISO reads 0xFF; `timed_out` goes true after N transactions. `FatStore::new(SdCard::new_with_options(FakeBus, NoDelay, AcquireOpts { acquire_retries: 3, .. }), FixedTime).mount()` → `Err(NoCard)`, within a bounded number of transactions.
  - `classify_table`: `CardNotFound` → `NoCard`; `Transport` with `timed_out` in `Acquire` → `NoCard`, in `Data` → `Timeout`; `Transport` without `timed_out` → `Io`; `CrcError` → `Io`.
  - `reinit_wakes_at_init_clock`: the log after `reinit` is `set_phase(Acquire)` then `wake`; after `mounted`, `set_phase(Data)`.
- [ ] **Step 3: Run** `cargo test -p chimera-fat` → FAIL.
- [ ] **Step 4: Implement `FatStore`.**
  - `mount`: `start_op`; read block 0 and the boot sector through `vm.device(|d| d.read(..))`. If that first read fails: `reinit`, `start_op`, read once more (the idle swap). Then Task 1's parser (a rejection returns before the library sees the volume); `open_raw_volume(VolumeIdx(0))`; `close_volume`; `mounted()`.
  - Each op: `start_op`, open the volume, compare the `VolumeId` from the boot sector, then open the dir chain (`CHIMERA`, then `PROJECTS`/`SOUNDS`). Every opened handle is closed in reverse on every path, through one private `with_dir(vol, dir, |vm, raw_dir| ..)`.
  - `write` uses `Mode::ReadWriteCreateOrTruncate`, buffers `put` bytes in one `[u8; CHUNK]` and writes per full block, then `flush_file` and `close_file`.
  - Error mapping:

    | From `embedded_sdmmc::Error` | `StoreError` | `reinit` |
    |---|---|---|
    | `DeviceError(e)` | `dev.classify(&e)` | yes |
    | `NotEnoughSpace`, `DiskFull` | `dev.fault()`, else `Full` | only on a fault |
    | `NotFound` | `NotFound` | no |
    | `EndOfFile` before the entry's length, `BadCluster`, `UnterminatedFatChain` | `Corrupt` | no |
    | the rest | `Io` | yes |

    The library turns any `alloc_cluster` failure into `DiskFull`; `fault()` recovers a pulled card from it.
- [ ] **Step 5: Run** `cargo test -p chimera-fat` → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "FatStore: embedded-sdmmc behind Store, mounted per operation"`

### Task 5: `DirStore` passes the suite

**Files:**
- Create: `chimera-desktop/src/store.rs` (with `#[cfg(test)] mod tests`)
- Modify: `chimera-desktop/src/main.rs` (`#[cfg_attr(not(test), allow(dead_code))] mod store;`; Task 13 wires it and removes the attribute), `chimera-desktop/Cargo.toml` (dev-dep `chimera-hal` with `testkit`), `.gitignore` (`chimera-card/`)

**Interfaces:**
- Produces: `pub struct DirStore { root: PathBuf }` with `pub fn new(root: PathBuf) -> Self`, `impl Store`.
  - The root is the card. A missing root gives `NoCard`.
  - `root/VOLUME` holds `serial label` (8 hex digits, a space, 11 bytes). It is created with a serial from the system time on the first `mount` of an existing root.
  - `Dir::Chimera` is `root/CHIMERA`, and so on.

- [ ] **Step 1: Write the failing tests.**
  - `dir_store_passes_suite`: a unique dir under `std::env::temp_dir()`. `swap` rewrites `VOLUME` with a new serial and empties `CHIMERA/`.
  - `missing_root_is_no_card`.
- [ ] **Step 2: Run** `cargo test -p chimera-desktop` → FAIL.
- [ ] **Step 3: Implement** with `std::fs`. `write` ends with `sync_all`.
- [ ] **Step 4: Run** `cargo test -p chimera-desktop` → PASS. Then `just check` → PASS (the `cfg_attr` keeps the bin build free of dead-code warnings).
- [ ] **Step 5: Commit** `git commit -m "Desktop card: a directory behind Store"`

### Task 6: Framing — CRC, `Name`, header, records, the framer, and ADR 0045

**Files:**
- Create: `chimera-core/src/name.rs`, `chimera-core/src/storage/{mod,crc,frame,record}.rs`, `chimera-core/tests/storage_frame_test.rs`, `chimera-core/tests/name_test.rs`
- Create: `docs/adr/0045-card-format.md`; Modify: `docs/adr/README.md`
- Modify: `chimera-core/src/lib.rs` (`pub mod name; pub mod storage;`)

**Interfaces:**
- Consumes: `ByteSink`, `StoreError` (Task 3).
- Produces:

```rust
// name.rs
pub struct Name<const N: usize> { /* private bytes [u8; N], len u8 */ }
pub enum NameError { Empty, TooLong, BadChar(u8), EdgeSpace }
impl<const N: usize> Name<N> {
    pub fn new(s: &str) -> Result<Self, NameError>;          // A–Z a–z 0–9 space '-'; 1..=N; no leading or trailing space
    pub fn from_padded(b: &[u8; N]) -> Result<Self, NameError>; // NUL-padded disk form, same rules
    pub fn as_str(&self) -> &str; pub fn padded(&self) -> [u8; N];
}
pub type SoundName = Name<16>; pub type ProjectName = Name<16>;

// storage/crc.rs — CRC-32/ISO-HDLC (poly 0xEDB88320 reflected, init and xorout 0xFFFF_FFFF), const table
pub struct Crc32(u32);
impl Crc32 { pub const fn new() -> Self; pub fn update(&mut self, b: &[u8]); pub fn finish(&self) -> u32; }

// storage/frame.rs
pub const MAGIC: [u8; 4] = *b"CHIM";
pub const FORMAT_VERSION: u16 = 1;
pub const HEADER_LEN: usize = 28;   // magic 4, version u16, kind u8, flags u8 (0), generation u32, name [u8; 16]
pub const TRAILER_LEN: usize = 4;   // CRC32 LE over header + records
pub enum FileKind { Sound = 1, System = 3 }  // 2 Project, 4 Tags, 5 Index reserved (ADR 0045)
pub struct Generation(u32);         // every u32 is a valid generation, so the constructor is public
impl Generation { pub const FIRST: Self; pub const fn new(n: u32) -> Self; pub fn next(self) -> Self /* wrapping */;
                  pub fn is_newer_than(self, o: Self) -> bool /* (self − o) as i32 > 0 */; pub fn get(self) -> u32; }
pub enum Side { A, B }  impl Side { pub fn ext(self) -> &'static [u8]; pub fn other(self) -> Side; }
pub struct ProjectId(u32);
impl ProjectId { pub fn new(n: u32) -> Option<Self> /* 1..=9_999_999 */; pub fn get(self) -> u32; pub fn stem(self) -> [u8; 8] /* b"P0000001" */; }
pub struct Header { pub kind: FileKind, pub generation: Generation, pub name: Option<Name<16>> }
pub enum FileError { Truncated, BadMagic, BadCrc, NeedsNewerFirmware, WrongKind, Bounds, BadName, Corrupt }
impl FileError { pub fn message(self) -> &'static str; pub fn is_torn(self) -> bool /* Truncated | BadMagic | BadCrc */; }
pub enum Event<'a> { Header(Header), Record(ReadTag, &'a [u8]) }
pub struct Framer { /* state, Crc32, pos, file_len, rec_buf [u8; MAX_RECORD_LEN] */ }
impl Framer {
    pub fn new(file_len: u32) -> Result<Self, FileError>;
    pub fn push(&mut self, chunk: &[u8], on: &mut dyn FnMut(Event<'_>) -> Result<(), FileError>) -> Result<(), FileError>;
    pub fn finish(&self) -> Result<(), FileError>;          // pos == file_len and CRC matches
}

// storage/record.rs
pub const MAX_RECORD_LEN: usize = 512;
pub const CRITICAL: u16 = 0x8000;
pub enum RecordTag { Block, Engine, Registry, ModDests, Routes, LastProject }
impl RecordTag { pub const fn code(self) -> u16; pub fn from_code(c: u16) -> Option<Self>; }
pub enum ReadTag { Known(RecordTag), Unknown(u16) }
impl ReadTag { pub fn critical(self) -> bool; }
pub struct RecordBuf { /* [u8; MAX_RECORD_LEN], len */ }
impl RecordBuf { pub fn new() -> Self; pub fn u8(&mut self, v: u8); pub fn u32(&mut self, v: u32); pub fn f32(&mut self, v: f32); pub fn bytes(&mut self, b: &[u8]); pub fn as_slice(&self) -> &[u8]; }
pub struct RecordWriter<'s> { /* sink, Crc32 */ }
impl RecordWriter<'_> { pub fn put(&mut self, tag: RecordTag, payload: &[u8]) -> Result<(), StoreError>; }
pub fn write_file(sink: &mut dyn ByteSink, h: &Header,
                  body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>) -> Result<(), StoreError>;
```

The record codes are frozen in v1:

| Tag | Code | Critical | Payload |
|---|---|---|---|
| `Block` | 0x0001 | no | block code `u8`, then (`ParamId` `u8`, value 4 B LE)* |
| `Engine` | 0x8002 | **yes** | engine code `u8`; the first record of a Sound file |
| `Registry` | 0x0003 | no | (block code, param id, label [8])* |
| `ModDests` | 0x0004 | no | `num_sources` `u8`, then (block code, param id)* |
| `Routes` | 0x0005 | no | (source code, block code, param id, amount `i8`)* |
| `LastProject` | 0x0006 | no | `ProjectId` `u32` LE |

All integers are little-endian.

- [ ] **Step 1: Write the failing tests.**
  - `name_test.rs`:
    - `accepts_the_charset`: `Name::<16>::new("DUB-042 a")` is Ok, and `as_str` round-trips;
    - `rejects`: `""` → `Empty`; 17 chars → `TooLong`; `"(init)"` → `BadChar(b'(')`; `"\u{e9}"` → `BadChar`; `" A"`, `"A "` and `"   "` → `EdgeSpace`;
    - `padded_round_trip`;
    - `from_padded_rejects_a_gap`: NUL then a letter → `BadChar(0)`; `"A "` then NULs → `EdgeSpace`.
  - `storage_frame_test.rs`:
    - `crc_known_answer`: `Crc32` over `b"123456789"` is `0xCBF4_3926`;
    - `generation_wraps`: `Generation::new(u32::MAX).next()` is `Generation::new(0)`, and it `is_newer_than(Generation::new(u32::MAX))`;
    - `project_id_bounds_and_stem`: `new(0)` and `new(10_000_000)` are `None`; `new(42).stem()` is `*b"P0000042"`;
    - `header_round_trip`: `write_file` of an empty body gives 32 bytes. Push them through a `Framer` one byte at a time, then all at once: the same `Event::Header`, and `finish` is Ok;
    - `records_straddle_chunks`: 3 records of 1, 300 and 512 B, pushed in chunks of 1, 7 and 512. The same `(ReadTag, payload)` sequence arrives;
    - `unknown_non_critical_skipped_even_if_long`: tag 0x0077 with a 4 000 B payload arrives as `Unknown(0x0077)` with an empty payload slice and doesn't count as `Bounds`;
    - `known_record_over_max_is_bounds`;
    - `length_past_end_is_bounds`;
    - `bad_magic`, `bad_crc` (one flipped byte), `truncated` (drop the last byte), `newer_version` (version 2 → `NeedsNewerFirmware`), `bad_name` (`(` in the name).
- [ ] **Step 2: Write the compile-fail doc tests.**
  - On `Name`: ```` ```compile_fail,E0451 ```` building `Name::<16> { .. }` directly.
  - On `RecordWriter::put`: ```` ```compile_fail,E0308 ```` passing `ReadTag::Unknown(7)`.
  - On `ProjectId`: ```` ```compile_fail,E0603 ```` `ProjectId(5)`.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test storage_frame_test --test name_test` → FAIL.
- [ ] **Step 4: Implement.**
  - The framer's states are `Header` → `RecordHead` → `RecordBody{known, left}` / `Skip{left}` → `Trailer`.
  - The CRC covers bytes `0..file_len − 4`. `Header` must arrive before any record. `flags ≠ 0` → `Corrupt`.
  - The name is `None` when all 16 bytes are 0.
- [ ] **Step 5: Write ADR 0045** "Store cards in 8.3 A/B files of versioned TLV records" (Proposed). Record:
  - the header, the record table above and the CRC trailer;
  - A/B generations, the pick rule (fall back on every error except `NeedsNewerFirmware`; both broken gives the newest error), `write_target` (the side the reader doesn't keep) and the delete order;
  - **the atomic-block assumption:** a 512 B block write is all-or-nothing and disturbs no other block. SD cards don't promise it; `.A` and `.B` share a directory sector and, when small, FAT sectors, so a torn write there can lose both. Task 11's torn test characterises it;
  - the must-understand bit;
  - frozen codes and the retired list, neutral defaults, migration by a new `ParamId`, and the fixture corpus;
  - the reserved `FileKind`s;
  - the card-access decisions: SPI2 polled, the PLL2_P kernel clock (the stock firmware's precedent), the acquire/idle/cap deadlines, mount per operation, the serial and the boot-sector gate from our own BPB parse, and the `Ready` capability;
  - a **Pending** section: CS PE12, no card-detect and the SPI mode, marked unconfirmed with the source. If the Task 2 STOP has already answered, write the answers instead; otherwise the STOP fills this section. The ADR stays Proposed until it holds no pending item.

  Add it to `docs/adr/README.md`.
- [ ] **Step 6: Run** `just check` → PASS.
- [ ] **Step 7: Commit** `git commit -m "Card file framing: header, TLV records, CRC trailer; ADR 0045"`

### Task 7: Frozen code tables

**Files:**
- Create: `chimera-core/src/storage/codes.rs`, `chimera-core/tests/disk_codes_test.rs`, `chimera-core/tests/fixtures/disk_codes_v1.txt`
- Modify: `chimera-core/src/block.rs` (the `DiskCode` trait and two `Block` methods)
- Modify: the `Block` impls with Enum specs:
  - `params.rs` (Filter KIND/MODE; Env TYPE/SPEED/HOLD/MODE/FORM);
  - `dsp/lfo.rs` (SHAPE, SYNC, TYPE, FORM);
  - `dsp/algo/params.rs` (WAVE, ALG A/B);
  - `dsp/modal/params.rs` (MODE);
  - `dsp/chorus.rs` (MODE);
  - `dsp/comp.rs` (RATIO);
  - `part.rs` (CH, MODE, OUT);
  - `ui/theme_settings.rs` (BRIGHT, GAMMA, ACCENT, BLACK).
- Modify: the enum types, each getting `impl DiskCode`: `FilterKind`, `FilterMode`, `EnvType`, `EnvSpeed`, `HoldPos`, `FuncMode`, `EnvForm`, `LfoType`, `LfoForm`, `ResonatorMode`, `PartMode`, `DacPair`, `EngineType`, `Gamma`, `Accent`.

**Interfaces:**
- Produces, in `block.rs`:

```rust
pub trait DiskCode: Sized + Copy { fn disk_code(self) -> u8; fn from_disk_code(c: u8) -> Option<Self>; }  // exhaustive matches
// on trait Block, default methods:
fn enum_code(&self, id: ParamId) -> Option<u8> { None }
fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool { false }   // false: unknown code, nothing written
```

- Produces, in `storage/codes.rs`:

```rust
impl BlockRef { pub const fn disk_code(self) -> Option<u8>; pub fn from_disk_code(c: u8) -> Option<BlockRef>; } // Channels → None (not stored; plan 2 deletes it)
impl ModSource { pub const fn disk_code(self) -> u8; pub fn from_disk_code(c: u8) -> Option<ModSource>; }
pub const RETIRED: &[(u8, u8)];          // (block code, param id); v1: Filter 3 (FM), 4 (ENV), 5 (KEY) per ADR 0009
pub struct Migration { pub block: u8, pub old: ParamId, pub new: ParamId, pub map: fn(f32) -> f32 }
pub const MIGRATIONS: &[Migration] = &[];
pub struct ValidAddr { /* private: block: BlockRef, spec: &'static ParamSpec */ }
impl ValidAddr {
    pub fn of_block(b: BlockRef) -> impl Iterator<Item = ValidAddr>;   // iterates b.specs(), canonical order
    pub fn find(b: BlockRef, id: ParamId) -> Option<ValidAddr>;        // iterates b.specs()
    pub fn addr(self) -> ParamAddr; pub fn spec(self) -> &'static ParamSpec; pub fn coded(self) -> bool; // kind == Enum
}
pub enum DiskValue { Real(f32), Code(u8) }
pub fn read_value(b: &dyn Block, a: ValidAddr) -> DiskValue;
```

The block codes are frozen:

| Block | Code |
|---|---|
| Modal | 1 |
| Algo | 2 |
| AlgoOp A–F | 3–8 |
| Drive | 9 |
| Filter | 10 |
| Folder | 11 |
| Env 1–3 | 12–14 |
| Lfo 1–3 | 15–17 |
| Out | 18 |
| Pitch | 19 |
| Chorus | 20 |
| Delay | 21 |
| Reverb | 22 |
| Tape | 23 |
| Comp | 24 |
| Part | 25 |
| Theme | 26 |

`ModSource` codes are Env1 0, Lfo1 1, Env2 2, Env3 3, Lfo2 4, Lfo3 5, Vel 6, Note 7. `EngineType` codes are Algo 0, Modal 1.

For Enum params that aren't Rust enums, the code is the value's own meaning:

| Param | Code |
|---|---|
| CH | channel 0–15 |
| BRIGHT | percent 10–100 |
| BLACK | `Black::get() as u8` (two's complement) |
| WAVE | the `WaveId` index |
| ALG | the `AlgoId` index |
| LFO SHAPE, SYNC; chorus MODE; comp RATIO | the stored `u8` |

`FilterMode`'s code is the mode's own, **not** its index in `kind.modes()`.

- [ ] **Step 1: Write the failing tests** in `disk_codes_test.rs`:
  - `table_matches_golden`: build text lines from the code, one per block and param:
    - `B <block code> <param id> <label>`;
    - for each Enum param and each UI value `v` in `0..=max`, `E <block code> <param id> <code> <shown text>`, from `set(v)`, then `enum_code`, then the `ValFmt` name or integer.

    Every line of `fixtures/disk_codes_v1.txt` must be present. New lines may be added only by appending to the file; a missing or changed line fails, naming the line.
  - `every_enum_param_has_codes`: for every stored `BlockRef` and every `ParamKind::Enum` spec, on the block from `ParamSnapshot::default()`/`FxParams::default()`/`PartParams::default()`/`ThemeSettings::DEFAULT`: `set(v)`, then `enum_code(id)` is `Some(c)`; `set_enum_code(id, c)` on a fresh block gives `get(id) == v`.
  - `unknown_code_writes_nothing`: `set_enum_code(FilterParams::MODE, 250)` is `false`, and `get` is unchanged.
  - `block_codes_unique_and_round_trip`: over `BlockRef::ALL` except `Channels`, `from_disk_code(disk_code(b)) == Some(b)`, and the codes are distinct.
  - `mod_source_codes_round_trip`.
  - `retired_never_live`: no live spec is a `RETIRED` pair.
  - `valid_addr_only_from_specs`: `ValidAddr::find(Filter, ParamId(4))` is `None`, and `of_block(Filter)` yields KIND before MODE.
- [ ] **Step 2: Write the compile-fail doc test** on `ValidAddr`: ```` ```compile_fail,E0451 ```` building `ValidAddr { .. }`.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test disk_codes_test` → FAIL.
- [ ] **Step 4: Implement** the tables and `Block` overrides. Generate `disk_codes_v1.txt` once from the implementation, read it line by line against the source enums, and commit it. From now on it is append-only.
- [ ] **Step 5: Run** `cargo test -p chimera-core --test disk_codes_test` → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "Frozen disk codes for blocks, sources and every stored enum"`

### Task 8: The block and Sound codecs; the round trip

**Files:**
- Create: `chimera-core/src/storage/block_codec.rs`, `chimera-core/src/storage/sound.rs`, `chimera-core/tests/sound_codec_test.rs`
- Modify: `chimera-core/src/preset.rs`: `Sound.name: SoundName`, `Sound::init` named `INIT`, `#[derive(Debug)]`, `Sound::bits_eq`
- Modify: `chimera-core/src/mod_path.rs` (`#[derive(Debug)]` on the registry and its entry; `pub fn bits_eq(&self, o: &Self) -> bool` over entries `..len`), `chimera-core/src/modulation.rs` (`pub fn with_sources(n: usize) -> Self`; `pub fn bits_eq(&self, o: &Self) -> bool` over `num_sources`, `num_dests`, dests `..num_dests` and routes `[..num_sources][..num_dests]`). No derived `PartialEq`: it would compare the never-read slots.
- Modify: `chimera-core/src/factory.rs:43-47` (`named` builds a `SoundName`), `chimera-core/src/ui/browser.rs:160`
- Modify: `chimera-core/tests/preset_test.rs:66,320-334` and `tests/ui_routing_test.rs:209` (`(init)` → `INIT`)

**Interfaces:**
- Consumes: `RecordWriter`, `RecordBuf`, `RecordTag`, `ReadTag`, `FileError` (Task 6); `ValidAddr`, `DiskValue`, codes, `MIGRATIONS` (Task 7).
- Produces:

```rust
// block_codec.rs
pub fn encode_block(b: BlockRef, blk: &dyn Block, out: &mut RecordBuf);  // block code, then every spec in canonical order
pub fn decode_block(payload: &[u8], migrations: &[Migration],
                    target: Option<&mut dyn Blocks>) -> Result<(), FileError>;
// Errors: Bounds (payload not 1 + 5n), Corrupt (duplicate id). Unknown block code or param id: skipped.
// With target: collect values by spec index, then write in spec order: Real → reject non-finite, else set() (clamp+quantise);
// Code → set_enum_code, false → keep base. Retired ids go through `migrations`, else skipped.

// sound.rs
impl Sound {
    pub fn neutral(engine: EngineType) -> Sound;   // ParamSnapshot::for_engine, ModState::with_sources(MAX_MOD_SOURCES), empty registry, name INIT
    pub fn bits_eq(&self, o: &Sound) -> bool;      // name, engine, every voice-block param by to_bits, ModState::bits_eq, registry bits_eq
}
pub fn encode_sound(s: &Sound, w: &mut RecordWriter<'_>) -> Result<(), StoreError>; // Engine, Block × voice blocks, Registry, ModDests, Routes
pub struct SoundDecoder<'a> { /* target: &'a mut Sound, seen_engine, ... */ }
impl<'a> SoundDecoder<'a> { pub fn new(target: &'a mut Sound) -> Self; }
// Decode impl is Task 11's trait; here expose the per-event core used by it:
impl SoundDecoder<'_> { pub fn event(&mut self, e: Event<'_>, apply: bool) -> Result<(), FileError>; pub fn end(&mut self, apply: bool) -> Result<(), FileError>; }
```

Sound decoding rules:
- `Engine` must be the first record; otherwise `Corrupt`. An unknown engine code gives `NeedsNewerFirmware` (it is critical). On apply, `*target = Sound::neutral(engine)`, then the header name.
- `Registry` entries: `ValidAddr::find` plus `ParamAddr::modulatable`, else the entry is skipped. More than `MAX_REGISTRY_DESTS` → `Bounds`.
- `ModDests`: `num_sources` is clamped to `MAX_MOD_SOURCES`; dests are pushed in order. More than `MAX_MOD_DESTS` → `Bounds`.
- `Routes` are kept in the decoder (≤ 512 B) and applied in `end`: `find(addr)`, else `push`, else dropped; then `set_route(source, d, amount)`. More than `MAX_MOD_SOURCES × MAX_MOD_DESTS` → `Bounds`. `Routes` before `ModDests` is allowed.
- The voice blocks are the `BlockRef`s whose `ParamSnapshot::block` is `Some`. A `Block` record for a non-voice block in a Sound file is skipped.

- [ ] **Step 1: Write the failing tests** in `sound_codec_test.rs`. The helper `encode(&Sound) -> Vec<u8>` uses `write_file` into a `Vec` sink, and `decode(&[u8]) -> Result<Sound, FileError>` runs a `Framer` twice (apply false, then true) into `Sound::neutral(Algo)`.
  - `factory_round_trip`: for `i in 0..FACTORY_LEN`, `decode(encode(s))?.bits_eq(&s)`, and `format!("{:?}")` of the params and the name are equal (this catches a param field no spec covers).
  - `init_round_trip` for each `EngineType::ALL`.
  - `zero_amount_route_survives`: a route set at amount 0 keeps its present bit.
  - `full_matrix_round_trip`: 16 dests × 8 sources, with random amounts from a fixed xorshift seed.
  - `bits_eq_ignores_dead_slots`: two `ModState`s (and two registries) that differ only past `num_dests` (`len`) are `bits_eq`; a difference inside the range isn't.
  - `mode_before_kind_still_applies`: a hand-built Filter record listing MODE then KIND decodes to the same mode.
  - `nan_takes_base`: CUTOFF bits `0x7FC0_0000` → CUTOFF equals `Sound::neutral`'s.
  - `out_of_range_clamps`: CUTOFF 1e9 → 20 000.0; a Stepped value 3.4 → 3.0.
  - `bad_enum_code_non_critical_keeps_base`: Filter MODE code 250 → the base mode.
  - `missing_block_takes_neutral`: drop the Filter record → the filter equals `Sound::neutral(e).params.filter` (Debug).
  - `engine_not_first_is_corrupt`.
  - `unknown_engine_needs_newer`: engine code 9 → `NeedsNewerFirmware`.
  - `migration_maps_old_id`: `decode_block` with a test `Migration { block: 10, old: ParamId(5), new: ParamId(1), map: |v| v * 0.5 }` and a record carrying id 5 = 0.8 → RESO 0.4.
  - `init_is_named_init`: `Sound::init(e).name.as_str() == "INIT"`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test sound_codec_test` → FAIL.
- [ ] **Step 3: Implement** as specified. The `Sound.name` change touches `factory.rs`, `browser.rs` and the two tests listed.
- [ ] **Step 4: Run** `just test`. Any `screen_golden_test` case that shows the init name fails. Re-record only those cases, list them in the commit body, and confirm no other golden moved (ADR 0011). Then `just check` → PASS.
- [ ] **Step 5: Commit** `git commit -m "Sound codec: frozen base, canonical block order, clamped values; INIT"`

### Task 9: v1 fixtures, compatibility, corruption and fuzz

**Files:**
- Create: `chimera-core/tests/codec_compat_test.rs`, `chimera-core/tests/codec_fuzz_test.rs`
- Create: `chimera-core/tests/fixtures/v1/` (`factory_0.snd` … `factory_7.snd`, `init_algo.snd`, `init_modal.snd`; `system.sys` is added in Task 12)
- Create: `chimera-core/fuzz/Cargo.toml`, `chimera-core/fuzz/fuzz_targets/decode_sound.rs`
- Modify: `Cargo.toml` (`exclude = ["chimera-core/fuzz"]`), `Justfile` (`fuzz` recipe: `cd chimera-core/fuzz && cargo fuzz run decode_sound -- -max_total_time=300`)
- Modify: `chimera-core/tests/common/mod.rs`: extract `pub fn render_sound(params: &ParamSnapshot, mods: &ModState) -> Vec<f32>` from `render_case`; `render_case` calls it

**Interfaces:**
- Consumes: `encode_sound`, `SoundDecoder`, `Framer`, `write_file` (Tasks 6–8).

- [ ] **Step 1: Write the fixture writer.** `codec_compat_test.rs::write_v1_fixtures` is `#[ignore]` and runs only with `FIXTURE_WRITE=1`. It writes each fixture at `Generation::FIRST`. Run it once and commit the files. The fixtures are frozen from here on; a later change never rewrites them.
- [ ] **Step 2: Write the failing tests** in `codec_compat_test.rs`:
  - `v1_fixtures_render_identically`: for each fixture, decode it, `render_sound`, and check `fnv1a` equals a constant table `FIXTURE_RENDERS: &[(&str, u64)]`, recorded when the fixtures are written.
  - `v1_fixtures_equal_factory`: `decode(factory_i.snd).bits_eq(&factory_sound(i))`. A comment says this holds only while the factory Sounds are unchanged; the render test is the lasting one.
  - `unknown_non_critical_skipped`: insert tag 0x0070 (40 B) after `Engine`, and fix the CRC. It decodes `bits_eq` to the original.
  - `unknown_critical_greys`: tag 0x8070 → `NeedsNewerFirmware`, and the target is unchanged.
  - `truncated_bad_crc_bad_magic_leave_target`: for each corruption, the error is `Truncated`/`BadCrc`/`BadMagic`, and the target still `bits_eq`s its value from before the call, because pass 1 fails before pass 2 runs.
- [ ] **Step 3: Write the failing test** in `codec_fuzz_test.rs`:
  - `mutated_files_never_panic_or_escape_specs`: for 20 000 xorshift seeds, take a random fixture and apply 1–8 mutations (byte flip, byte insert, byte delete, record-length poke, count poke), then **fix the CRC trailer**, so the parser gets past the framing.
  - Decode through both passes into `Sound::neutral(Algo)`. It must not panic.
  - On `Ok`, for every voice-block spec: the value is finite and in `min..=max`; an Enum value has `enum_code` `Some`; `mod_state.num_dests() <= MAX_MOD_DESTS`; `dest_registry.len() <= MAX_REGISTRY_DESTS`.
  - Also `random_bytes_never_panic`: 20 000 random buffers of 0–2 048 B.
- [ ] **Step 4: Run** `cargo test -p chimera-core --test codec_compat_test --test codec_fuzz_test`. It fails if a bound is missing; fix the codec, not the test.
- [ ] **Step 5: Write the `cargo fuzz` target.** It runs the same assertions on `data`. Check it builds with `cargo +nightly fuzz build` (needs `cargo install cargo-fuzz`); if that isn't installed, `cargo check` in `chimera-core/fuzz` → OK.
- [ ] **Step 6: Run** `just check` → PASS.
- [ ] **Step 7: Commit** `git commit -m "v1 fixtures, compatibility, corruption and fuzz tests"`

### Task 10: The `Card` state machine

**Files:**
- Create: `chimera-core/src/storage/card.rs`, `chimera-core/tests/card_test.rs`
- Modify: `chimera-core/Cargo.toml` (dev-dep `chimera-hal` with `features = ["testkit"]`)

**Interfaces:**
- Consumes: `Store`, `StoreError`, `Unsupported`, `VolumeId` (Task 3); `MemStore` (tests).
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardError { Unsupported(Unsupported), Full, Timeout, VolumeChanged(VolumeId), Io }   // no NoCard: Failed(NoCard) can't exist
impl CardError { pub fn from_store(e: StoreError) -> Option<CardError>; }  // None for NoCard, NotFound, Corrupt
impl From<CardError> for StoreError { .. }                                 // message() reuse
#[derive(Debug)] pub struct Ready { vol: VolumeId }                          // private field; not Copy, not Clone
impl Ready { pub fn volume(&self) -> VolumeId; }
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Card { Absent, Ready(VolumeId), Failed { err: CardError, last: Option<VolumeId> } }
#[derive(Clone, Copy, Debug, PartialEq)] pub enum CardEvent { Mounted, Same, Swapped { old: VolumeId } }
pub trait CardFault { fn store_error(&self) -> Option<StoreError>; }   // impl for StoreError, LoadError, SaveError
pub fn after_mount(card: Card, r: Result<VolumeId, StoreError>) -> (Card, Option<CardEvent>);  // pure
pub fn after_error(card: Card, e: StoreError) -> Card;  // pure: NoCard → Absent; CardError → Failed { err, last }; else unchanged
impl Card {
    pub const fn new() -> Card;  // Absent
    pub fn run<S: Store, R, E: CardFault + From<StoreError>>(&mut self, store: &mut S,
        op: impl FnOnce(&mut S, &Ready) -> Result<R, E>) -> Result<(R, CardEvent), E>;
}
```

`run` mounts (per operation), applies `after_mount`, builds a `Ready` on its own stack, lends `&Ready` to `op`, and applies `after_error` on a store error. `R` can't name the borrow, and `Ready` can't be copied or built outside `card.rs`, so no `Ready` outlives the mount that made it. There is no `Card::ready()`. A `Swapped` or `Mounted` event is returned so plans 2 and 3 can drop the index and project list and re-validate a `Pending` replace.

- [ ] **Step 1: Write the failing tests** in `card_test.rs`:
  - `transition_table`: `after_mount` over every (`Card` state, `Ok(same)`/`Ok(other)`/`Err(NoCard)`/`Err(Io)`) pair:
    - `Absent` + Ok(v) → `Ready(v)`, `Mounted`;
    - `Ready(v)` + Ok(v) → `Same`; + Ok(w) → `Ready(w)`, `Swapped { old: v }`;
    - `Failed { last: Some(v), .. }` + Ok(v) → `Ready(v)`, `Same`; + Ok(w) → `Swapped { old: v }`;
    - `Failed { last: None, .. }` + Ok(v) → `Mounted`;
    - any + `Err(NoCard)` → `Absent`, `None`;
    - `Ready(v)` + `Err(Io)` → `Failed { err: Io, last: Some(v) }`; `Failed { last, .. }` + `Err(Io)` keeps `last`; `Absent` + `Err(Io)` → `last: None`.
  - `file_errors_leave_the_card`: `after_error(Ready(v), NotFound)` and `after_error(Ready(v), Corrupt)` → `Ready(v)`.
  - `op_error_fails_card`: an `op` returning `Err(Io)` leaves `Card::Failed { err: Io, last: Some(v) }`.
  - `eject_then_insert`: on a `MemStore`, `eject()` → `run` gives `Err(NoCard)` and `Absent`; after re-insert → `Mounted`.
  - `swap_between_ops`: `MemStore::swap(2)` between two `run`s → the second event is `Swapped { old }`.
- [ ] **Step 2: Write the compile-fail doc tests** on `Ready`:
  - ```` ```compile_fail,E0451 ```` `Ready { vol }` from outside the module;
  - ```` ```compile_fail,E0507 ```` `card.run(&mut s, |_, r| { let owned = *r; Ok::<_, StoreError>(()) })` (not Copy);
  - ```` ```compile_fail ```` `card.run(&mut s, |_, r| Ok::<_, StoreError>(r))`: the borrow can't escape `run` (a lifetime error has no stable code; the comment says so).
- [ ] **Step 3: Run** `cargo test -p chimera-core --test card_test` → FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `cargo test -p chimera-core --test card_test` and `cargo test -p chimera-core --doc` → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "Card: mounted per operation, swaps seen by volume serial"`

### Task 11: Two-pass load, streaming save and A/B; the power cut

**Files:**
- Create: `chimera-core/src/storage/file.rs`, `chimera-core/tests/storage_ab_test.rs`
- Create: `chimera-fat/tests/power_cut_test.rs`; Modify: `chimera-fat/Cargo.toml` (dev-dep `chimera-core`)
- Modify: `chimera-fat/tests/common/image.rs`:
  - `CutDisk` counts and cuts **per block**, not per call: `writes: Cell<u32>` (blocks written) and `cut: Cell<Cut>` with `pub enum Cut { Never, After(u32), TornAfter(u32, Tear) }`, `pub enum Tear { HalfOld, Garbage }`. `TornAfter(k, t)` writes block `k` torn (first 256 B new and the rest old, or xorshift garbage) and fails it;
  - `pub fn fat_check(disk: &RamDisk) -> FatReport`: walks both FATs and the `/CHIMERA` entries; reports cross-linked chains, chains shorter than their entry's length, and each entry's raw 32 B and chain;
  - the builders `power_cut_test` doesn't use (`fat32`, `exfat`, `exfat_superfloppy`, `superfloppy`) get item-level `#[allow(dead_code)]`, as `CutDisk` has, because each test binary compiles `common` separately.
- Modify: `chimera-core/src/storage/sound.rs` (`impl Decode for SoundDecoder`)

**Interfaces:**
- Consumes: `Framer`, `write_file`, `Header`, `Generation` (Task 6); `SoundDecoder` (Task 8); `Card`, `Ready`, `CardFault` (Task 10); `Store` (Task 3).
- Produces:

```rust
pub trait Decode {
    const KIND: FileKind;
    fn event(&mut self, e: Event<'_>, apply: bool) -> Result<(), FileError>;
    fn end(&mut self, apply: bool) -> Result<(), FileError>;
}
pub enum LoadError { Store(StoreError), File(FileError), Missing }   // Missing: neither side exists
pub enum SaveError { Store(StoreError) }
pub struct AbFile { /* dir: Dir, stem [u8; 8], len */ }
impl AbFile { pub fn new(dir: Dir, stem: &[u8]) -> Option<AbFile>; pub const SYSTEM: AbFile; pub fn side(&self, s: Side) -> FileName; }
pub enum SideState { Missing, Torn(FileError), Present { gen: Generation, err: Option<FileError> } }
pub enum Pick { Load(Side), Refuse(Side, FileError), Missing }
pub fn pick(a: SideState, b: SideState) -> Pick;                        // pure
pub fn write_target(a: SideState, b: SideState) -> (Side, Generation);  // pure: the side pick doesn't keep; newest present .next() or FIRST
pub fn delete_order(a: SideState, b: SideState) -> [Side; 2];           // pure: older first
pub fn check_file<S: Store, D: Decode>(s: &mut S, r: &Ready, f: FileName, d: &mut D) -> Result<SideState, StoreError>;  // pass 1
pub fn check_frame<S: Store>(s: &mut S, r: &Ready, f: FileName, kind: FileKind) -> Result<SideState, StoreError>;       // framing only
pub fn load_file<S: Store, D: Decode>(s: &mut S, r: &Ready, f: FileName, d: &mut D) -> Result<Header, LoadError>;       // pass 1 then 2
pub fn save_ab<S: Store>(s: &mut S, r: &Ready, f: AbFile, kind: FileKind, name: Option<Name<16>>,
    body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>) -> Result<Generation, SaveError>;
pub fn load_ab<S: Store, D: Decode>(s: &mut S, r: &Ready, f: AbFile, d: &mut D) -> Result<Header, LoadError>;
pub fn delete_ab<S: Store>(s: &mut S, r: &Ready, f: AbFile) -> Result<(), StoreError>;
```

The rules:
- `check_file` and `check_frame` give:
  - `Missing` on `NotFound`;
  - `Torn(e)` on a torn `FileError` (`is_torn`), and `Torn(Truncated)` on `StoreError::Corrupt` (an old length over a short chain after a cut);
  - `Present { gen, err: Some(e) }` on any other `FileError`, such as `NeedsNewerFirmware` or `Bounds`, when the header was read;
  - `Present { gen, err: None }` when the pass passes.

  Other store errors propagate (they are card faults).
- `check_frame` has no decoder: it checks `KIND`, the header, the CRC and the must-understand bit (an unknown critical record → `NeedsNewerFirmware`). `save_ab` uses it on both sides, then `write_target`. (Plan 2 note: that reads both sides in full before every save; a header-only pass is the cheap form if 60 KB projects make it slow.)
- `pick`, over the `Present` sides newest first (tie → A):
  - the first with `err: None` → `Load(side)`;
  - one with `NeedsNewerFirmware` reached first → `Refuse(side, NeedsNewerFirmware)` (an older file never shadows it);
  - any other error → skip to the next side (the spec's "invalid file is ignored");
  - nothing loaded: `Refuse` with the newest `Present` side's error, else A's `Torn` error, else B's; `Missing` only when both are `Missing`.
- `write_target` never writes the side `pick` keeps (`Load` or `Refuse(_, NeedsNewerFirmware)`); otherwise it writes the older, missing or broken side.
- `load_ab` runs `check_file` on both sides, `pick`s, and runs pass 2 on the picked side. `Refuse` → `Err(File(e))`; `Missing` → `Err(Missing)`.
- `load_file`'s pass 2 re-checks the CRC. If it differs from pass 1 (the card changed), it returns `File(BadCrc)`.
- `save_ab` streams `header → body → trailer` through `Store::write`.

- [ ] **Step 1: Write the failing tests** in `storage_ab_test.rs` (on `MemStore`, with Sounds):
  - `pick_and_write_target_table`: every pair of {`Missing`, `Torn`, `Present` gen 1, `Present` gen 2, gen 2 `NeedsNewerFirmware`, gen 2 `Bounds`} gives the expected `Pick` and (side, gen), including the wrap from `Generation::new(u32::MAX)` to 0.
  - `tied_generations_prefer_a_then_write_b` (Review Focus 7).
  - `save_thrice_alternates`: save → A gen 1; save → B gen 2; save → A gen 3; `load_ab` gives the last content.
  - `torn_newest_falls_back`: truncate the newest file by 10 B; `load_ab` gives the older content.
  - `invalid_newest_falls_back`: the newest has a `Registry` record over `MAX_REGISTRY_DESTS` (`Bounds`, CRC fixed) → `load_ab` gives the older content; the next `save_ab` writes over the invalid side.
  - `newer_firmware_newest_does_not_fall_back`: the newest file has a critical unknown record → `Err(File(NeedsNewerFirmware))`; the next `save_ab` writes the other side.
  - `both_torn_is_the_error_not_missing`: both sides truncated → `Err(File(Truncated))`; no files → `Err(Missing)`.
  - `delete_older_first`: a `MemStore` wrapper logs deletes. The order is the older side, then the newer; after the first delete, `load_ab` still loads.
  - `save_streams_in_chunks`: a wrapper sink records the size of each `put`. None exceeds `MAX_RECORD_LEN + 4`.
  - `load_leaves_target_on_error`: a corrupt newest with a missing older → `Err`, and the target is unchanged.
- [ ] **Step 2: Write the failing tests** in `chimera-fat/tests/power_cut_test.rs`, on `FatStore<CutDisk>` over **FAT16** images (cheap to clone; FAT32's 33 MB images stay in the suite):
  - `cut_at_every_block_write_keeps_a_generation` (Review Focus 2): for each of saves 2 (creates B), 3 (truncates A) and 4 (truncates B): count the blocks `n` the save writes, then for every `k in 0..n`: restore the image from before that save, `Cut::After(k)`, save (it errors), then on a fresh `FatStore` over the image:
    - `fat_check` finds no cross-linked chain, and the kept side's entry bytes and chain equal their values before the save;
    - `load_ab` gives the previous generation's Sound or, for high `k`, the new one; never an error;
    - the next full save succeeds, and `load_ab` then gives it.
  - `repeated_cuts_keep_a_generation`: 200 xorshift seeds; each runs 6 rounds of: save with a random cut, remount, check the three invariants above. A final uncut save loads as newest.
  - `torn_block_breaks_only_shared_sectors` (Review Focus 6): save 3 with `TornAfter(k, t)` for every `k` and both `Tear`s. `load_ab` never panics and never returns content other than generation 2's or 3's. Every case that loads neither is on a block holding both directory entries or a FAT sector both chains use (the atomic-block assumption of ADR 0045); the test prints the tally.
  - `cut_then_reinsert_loads_previous_generation`: through `Card::run`. The cut save leaves `Card::Failed { err: Io, last: Some(v) }`; the next `run` gives `CardEvent::Same`, and `load_ab` gives generation 1.
  - `full_card_keeps_previous_generation` (Review Focus 5): A and B exist; a padding file fills the card to one free cluster. A save whose body adds 4 KB of non-critical padding records (so it needs more than the target's freed clusters plus the spare) returns `SaveError::Store(Full)`, and `load_ab` gives the previous generation.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test storage_ab_test && cargo test -p chimera-fat --test power_cut_test` → FAIL.
- [ ] **Step 4: Implement** `file.rs` and `impl Decode for SoundDecoder` (`KIND = FileKind::Sound`).
- [ ] **Step 5: Run** both tests → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "A/B generations: streamed saves, two-pass loads, safe at every cut"`

### Task 12: SYSTEM and the BUSY overlay

**Files:**
- Create: `chimera-core/src/storage/system.rs`, `chimera-core/src/ui/busy.rs`, `chimera-core/tests/system_file_test.rs`
- Create: `chimera-core/tests/fixtures/v1/system.sys` (written by `write_v1_fixtures`, extended here)
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod busy;`; `pub fn in_system(&self) -> bool` = `nav.chain_id == ChainId::System`; `pub fn set_theme(&mut self, t: ThemeSettings)`)
- Modify: `chimera-core/tests/screen_golden_test.rs` (case `busy_saving`)

**Interfaces:**
- Consumes: `save_ab`, `load_ab`, `Decode`, `AbFile::SYSTEM`, `LoadError` (Task 11); `Card`, `Ready` (Task 10); `encode_block`/`decode_block` (Task 8); `ProjectId` (Task 6).
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemSettings { pub theme: ThemeSettings, pub last_project: Option<ProjectId> }
impl SystemSettings { pub const DEFAULT: Self; /* ThemeSettings::DEFAULT, None */ }
pub fn encode_system(s: &SystemSettings, w: &mut RecordWriter<'_>) -> Result<(), StoreError>; // Block(Theme), LastProject if Some
pub struct SystemDecoder<'a> { target: &'a mut SystemSettings }   // impl Decode, KIND = System; unknown records per the rules
pub fn body_crc(s: &SystemSettings) -> u32;                       // CRC of encode_system's bytes, generation-independent
pub enum BootNote { NoCard, NoFile, Error(LoadError) }
pub struct SystemSync { /* saved: Option<u32>, was_in: bool */ }
impl SystemSync {
    pub fn boot<S: Store>(card: &mut Card, store: &mut S) -> (SystemSync, SystemSettings, Option<BootNote>);
    pub fn wants_write(&mut self, in_system: bool, s: &SystemSettings) -> bool;  // true only on the in→out edge and body_crc ≠ saved
    pub fn write<S: Store>(&mut self, card: &mut Card, store: &mut S, s: &SystemSettings) -> Result<(), SaveError>; // make_dir(Chimera), save_ab; saved = crc only on Ok
}
// ui/busy.rs
pub enum BusyLabel { Busy, Saving }
pub fn draw_busy<D: DrawTarget<Color = Rgb565>>(d: &mut D, label: BusyLabel) -> (u16, u16); // the band to flush
```

`boot` decodes into a local `SystemSettings` (20 B) and returns it only on `Ok`; a pass 2 that fails half-way (card pulled after pass 1) leaves `DEFAULT`. `Err(Missing)` → `BootNote::NoFile`; `Err(Store(NoCard))` → `NoCard`; any other error → `Error(e)`. `SystemSync` takes no project state; its signatures are the guarantee, so no test pretends to check it.

- [ ] **Step 1: Write the failing tests** in `system_file_test.rs` (on `MemStore`):
  - `round_trip`: non-default theme values (BRIGHT 40, GAMMA SOFT, ACCENT index 3, BLACK +1) and `last_project` `ProjectId::new(7)`; `write`, then `boot` → equal.
  - `boot_no_card_defaults`: an ejected `MemStore` → `SystemSettings::DEFAULT`, `Some(BootNote::NoCard)`, `Card::Absent`.
  - `boot_no_file_defaults`: `Some(BootNote::NoFile)`.
  - `boot_corrupt_defaults`: both sides torn → `DEFAULT`, `Some(BootNote::Error(LoadError::File(_)))`.
  - `boot_pass_two_failure_keeps_defaults`: a store wrapper that fails the second read of the picked side → `DEFAULT`, `Some(BootNote::Error(_))`.
  - `unknown_record_kept_theme`: a SYSTEM file with an extra non-critical tag 0x0071 still gives the theme.
  - `writes_only_on_exit_edge_and_change`:
    - frames in System → `false`;
    - exit with no change → `false`;
    - enter, change BRIGHT, exit → `true` once;
    - the next frame → `false`.
  - `no_card_exit_tries_once` (Review Focus 3): with the store ejected, the exit edge gives `wants_write` `true` and `write` `Err(NoCard)`; 10 more frames out of System → `false`; re-enter and exit → `true` again.
  - `system_fixture_loads`: `fixtures/v1/system.sys` decodes to its recorded values.
- [ ] **Step 2: Write the screen golden** `busy_saving`: a centred box with `SAVING`, on the default theme; record it once.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test system_file_test --test screen_golden_test` → FAIL.
- [ ] **Step 4: Implement.** Extend `write_v1_fixtures` to write `system.sys`, run it once with `FIXTURE_WRITE=1`, and commit the file.
- [ ] **Step 5: Run** the tests → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "SYSTEM file: theme and last project, written on leaving System"`

### Task 13: Shell wiring — boot SYSTEM, save on leaving System, the RAM budget (hardware STOP)

**Files:**
- Modify: `chimera-core/src/hw.rs:61-66`:
  - `pub const STORE_RESERVE: usize = 4 * 1024;`, covering the `VolumeManager` (one 512 B block cache and the handle tables) and `FatStore`'s 512 B buffer;
  - fix the `UI_RESERVE` comment. The stack is in DTCM (ADR 0025), so the reserve covers renderer and navigation state, not "`main`'s stack temporaries and the interrupt stacks".
- Modify: `chimera-core/src/instrument.rs:32-40` (`+ STORE_RESERVE` in `AXI_RESIDENT`)
- Modify: `chimera-core/tests/memory_budget_test.rs`: add the `STORE_RESERVE` row to `axi_residents_fit`'s list (its `assert_eq!(total, AXI_RESIDENT)` needs it), and assert that the AXI left over is ≥ 64 KB (it was ~91 KB before this plan; the SYSTEM path adds no other static)
- Modify: `chimera-stm32/src/sd.rs`:
  - Presence check before acquire (#186): pure `chimera_fat::sd::r1_within_ncr(&[u8]) -> Option<u8>` with host tests; the shell sends CMD0 (`40 00 00 00 00 95`) with CS low, reads ≤ 8 bytes, three tries; all 0xFF → `NoCard` in ~2 ms. With a card present, a failed acquire gets one fresh `wake` + acquire (the probe's first cold acquire failed, the second passed).
  - `impl chimera_fat::SdBus for SdSpi`: `Acquire` → `set_hz(SD_INIT_HZ.Hz())` and `SD_ACQUIRE_MS`; `Data` → `set_hz(SD_FAST_HZ.Hz())` and `SD_IDLE_MS`; `start_op` → `arm(phase's ms)`; `wake` and `timed_out` forward;
  - `pub type SdStore = FatStore<SdDevice, FixedTime>`;
  - `const _: () = assert!(size_of::<SdStore>() <= STORE_RESERVE);`;
  - `pub fn take_store(..) -> Option<&'static mut SdStore>`, a take-once `static mut MaybeUninit` in AXI, as `shared.rs` does (`// SAFETY:` on the one `unsafe`).
- Modify: `chimera-stm32/src/main.rs`:
  - `mod sd;` loses its `cfg(feature = "sd-probe")`;
  - `clocks::enable_cycle_counter` and `counting` lose their `cfg(any(feature = "perf-probe", feature = "sd-probe"))` gate (`sd::init` now calls them in every build);
  - after `display.init` with the default theme: `draw_busy(Busy)` and flush, then `sd::take_store`, then `SystemSync::boot(&mut card, store)`, then apply `settings.theme` through the same path the loop uses (backlight duty, gamma, palette) and `ui.set_theme`. The boot read is thus behind a visible overlay and inside the timeouts; it still runs before `watchdog::start`, which is safe because every card path is bounded (`SD_OP_CAP_MS`) and panic-free (Task 1's gate);
  - in the loop after `handle_input`: `if sync.wants_write(ui.in_system(), &settings) { draw_busy(Saving) → flush_region → sync.write }`;
  - `settings.theme = ui.theme()` each frame.
- Modify: `chimera-desktop/src/main.rs`: `mod store;` loses its `cfg_attr`; the same wiring, with `DirStore::new(env CHIMERA_CARD or "chimera-card")`. The default dir is created if missing; a `CHIMERA_CARD` that doesn't exist stays "no card".

**Interfaces:**
- Consumes: `SystemSync`, `SystemSettings`, `draw_busy`, `Card` (Tasks 10–12); `FatStore`, `SdBus`, `BusPhase` (Task 4); `SdDevice`, `SdSpi`, the `SD_*` constants (Task 2); `DirStore` (Task 5).
- Produces, for plan 2:
  - the shell owns one `Card` and one `&mut impl Store` beside `UiState`;
  - `UiState::in_system` is replaced by `Location` in plan 2, and `SystemSync::write` is called on project save and load for `last_project`.

- [ ] **Step 1: Write the failing test** in `memory_budget_test.rs`: `axi_counts_the_store` asserts `AXI_RESIDENT - (the old sum) == STORE_RESERVE` and `AXI_SRAM - AXI_RESIDENT >= 64 * 1024`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test memory_budget_test` → FAIL.
- [ ] **Step 3: Implement** `hw.rs`, `instrument.rs`, `sd.rs` and both shells.
- [ ] **Step 4: Run** `just check` → PASS (firmware at every feature set, stack check, clippy; `sd` is now used in every build). Then run `just desktop`:
  - change THEME, leave System (MENU to a Part), quit and relaunch: the theme is back;
  - `CHIMERA_CARD=/nonexistent just desktop`: the defaults apply and leaving System doesn't hang.
- [ ] **Step 5: Commit** `git commit -m "Boot reads SYSTEM; leaving System saves it"`
- [ ] **Step 6: STOP. Ask the owner to run these checks on the unit with `just flash`, and wait.**
  1. Boot with the Task 2 card: BUSY shows, then the default theme. Change BRIGHT and ACCENT, and leave System: SAVING flashes. Power off and on: the theme comes back.
  2. Boot with no card: BUSY shows for no longer than `SD_ACQUIRE_MS` plus the wake, then the defaults apply. Leave System: one quick failure, and audio is unaffected.
  3. Insert the card while running, then change the theme and leave System: it saves (`Absent → Ready`).
  4. Swap to a different FAT32 card while running, then leave System: it saves to the new card on the first try. The next boot with the first card shows its own theme.
  5. Pull the card during SAVING (repeat until it lands mid-save): the next boot loads the earlier theme or the new one, never the defaults, unless the card has no SYSTEM file.
  6. An exFAT card: the defaults apply at boot, with no hang.

  Record the results under `## Measured`. File any failure as a GitHub issue before fixing it. Move ADR 0045 to Accepted once it holds no pending item.

---

## Measured

### Task 2 probe (2026-09-29, rev V, 480 MHz, FAT32 SDHC 15 193 MB)

| Reading | Value |
|---|---|
| Kernel / init SCK | 100 MHz / 390 625 Hz, OK; DWT runs |
| No card | every mode ran the full 1 000 ms budget, NO MODE ACQUIRED (#186) |
| Cold acquire | first M0 FAIL at 1 360 ms (library gave up, 5 s budget); M3 then OK in 140 ms; M1 (not an SD mode) TO |
| Partition | LBA 8192, type 0C, Fat32, serial 37376530, NO NAME; mount 1 ms |
| 12.5 MHz | OK, write 326 KB/s, read 888 KB/s, worst gap 2 721 µs |
| 25 MHz | OK, write 363 KB/s, read 1 333 KB/s, worst gap 2 336 µs |
| Sizes | SOUND 872, POOL 27 904, PERF 5 520, UI 34 900, AUDIO3 12 916, AXI free 91 828, VOICE free 2 728 |
| CS | PE12 (the card answered on it) |
| Card-detect | unknown; exFAT run not done (host tests cover it) |

Set: `SD_MODE = MODE_3` (SD-legal, measured), `SD_FAST_HZ = 25 MHz`, `SD_IDLE_MS` stays 600, `SD_ACQUIRE_MS = 1500` per attempt with one fresh retry. Whether the first cold acquire always fails, or MODE_0 is at fault, is unsettled; MODE_3 plus the retry holds in both cases.

*(Task 13's STOP adds the on-unit checks.)*

---

## Review response

Answers the adversarial review of this plan at 98ef236.

| Finding | Task(s) | Resolution |
|---|---|---|
| H1 | 2, 4, 13 | `Medium::classify`/`fault` plus `SdBus::timed_out`/`phase`; 3 acquire retries under `SD_ACQUIRE_MS`; failure while acquiring is `NoCard`; MISO pull-up; host tests `no_card_is_no_card`, `classify_table`. |
| H2 | 2, 13 | `enable_cycle_counter` moves to `clocks.rs`; `sd::init` enables the DWT itself, and the deadline falls back to counting transactions. |
| H3 | 10, 11 | `Card::Failed { err, last }`; `Failed{last: v}` + Ok(v) → `Same`; table test updated. |
| H4 | 4, 11 | Chain errors on read → `StoreError::Corrupt` → torn side, not a card fault; cuts cover saves 2, 3 and 4, then a FAT check and a next save. |
| H5 | 1 (fix round), 4 | The boot sector is validated and classified by cluster count as the library does, before `open_raw_volume`; corrupt-BPB, FAT32-byte/FAT16-layout and mutated-image tests. |
| H6 | 2, 4 | `SdBus::wake()` (≥ 74 clocks, CS high) in `sd::init` and in `reinit` before `mark_card_uninit`. |
| M1 | 4 | `mount` re-inits and retries once when its first read fails; `mount_retries_once_after_reinit`; STOP check 4 wants a first-try save. |
| M2 | 4, 11 | `NotEnoughSpace` and `DiskFull` → `Full` unless `fault()` reports a device fault; the full-card save is 4 KB bigger than the freed clusters. |
| M3 | 2, 5, 13 | `mod sd` behind `sd-probe` until Task 13; desktop `mod store` under `cfg_attr(not(test), allow(dead_code))` until Task 13. |
| M4 | 11, 12 | `pick` falls back on every error except `NeedsNewerFirmware`; both broken gives the error, `Missing` only for no files; spec § A/B items 2 and 4 amended. |
| M5 | 10, 11, 12 | `Ready` not Copy/Clone, lent as `&Ready` inside `run`; `Card::ready()` gone; `CardError` has no `NoCard`; three compile-fail doc tests. The optional `Busy` token is not taken (Decisions). |
| M6 | 6, 11, spec | Atomic-512 B assumption stated in the spec and ADR 0045; per-block `CutDisk` with torn writes; repeated-cut property; FAT check after every cut. |
| M7 | 11 | `save_ab` classifies sides with `check_frame` (KIND, header, CRC, must-understand); plan 2 note on the cost. |
| M8 | 2, 6 | ADR 0045 records CS/card-detect/mode as Pending until the Task 2 STOP; MODE 0, then 3, then 1; the upstream `MX_SPI2_Init`/`MX_GPIO_Init` named. |
| M9 | 2, 13 | Acquire deadline, per-block idle deadline and a 10 s cap replace the 2 s whole-op deadline; the probe measures the worst gap. |
| L1 | 6, 11 | `Generation::new(u32)` is public; tests use it. |
| L2 | 3 | `--features chimera-hal/testkit` on multi-package lines; `after_op` hook is in Task 3's `store_suite`. |
| L3 | 2 | The probe's 16 KB pattern is generated 512 B at a time. |
| L4 | 11, 12 | `theme_never_touches_project_state` dropped; `save_streams_in_chunks` keeps only its assertion. |
| L5 | 13 | Boot draws BUSY after `display.init`, then reads SYSTEM, then applies the theme. |
| L6 | 12 | `boot` decodes into a local and assigns on `Ok`; `boot_pass_two_failure_keeps_defaults`. |
| L7 | 8 | `ModState::bits_eq` and the registry's compare the observable range; no derived `PartialEq`; `bits_eq_ignores_dead_slots`. |
| L8 | 6 | `Name` rejects leading and trailing spaces (`EdgeSpace`); the owner may switch to trimming. |
| L9 | 2 | Reassign the consumed `SPI1` rec, drop `pll1_q_ck(200 MHz)`, assert PLL2_P is 100 MHz; stock-firmware precedent cited. |
| L10 | 2 | `CycleDelay` uses `clocks::delay_us`. |
| L11 | — | Declined (YAGNI): a GPT card shows "CARD IS NOT FAT16/FAT32", which already says what to do. |
| L12 | 11 | Power-cut loops run on FAT16 images; FAT32 stays in the suite. |
