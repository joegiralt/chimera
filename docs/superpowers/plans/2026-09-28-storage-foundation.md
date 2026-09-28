# Storage Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Chimera reads and writes its SD card. The card works on the chip over SPI2, sits behind a `chimera-hal` `Store` trait with a desktop directory twin, and holds versioned, CRC-checked, A/B-generation files. The first of those files is SYSTEM, which brings THEME back at power-on.

**Architecture:**
- **Functional core** (`chimera-core/src/storage/`), pure and host-tested:
  - the file format: header, TLV records, the must-understand bit and the CRC trailer;
  - the frozen disk-code tables;
  - the block and Sound codecs;
  - the A/B generation rules;
  - the `Card` state machine;
  - SYSTEM.

  All of it sees only bytes in and bytes out through the `Store` trait.
- **Portable shell** (`chimera-fat`, a new `no_std` crate): `embedded-sdmmc` 0.10 behind `Store`, plus a pure MBR and boot-sector parser that yields the volume serial. It is host-tested on a RAM disk.
- **Hardware shell** (`chimera-stm32/src/sd.rs`): SPI2 polled, with embedded-hal 1.0 adapters over `stm32h7xx-hal` 0.16's embedded-hal 0.2 SPI.
- **Desktop shell** (`chimera-desktop/src/store.rs`): a directory.

**Tech Stack:** Rust 2024 (`no_std` core), `embedded-sdmmc` 0.10 (`default-features = false`), `embedded-hal` 1.0 (adapter only), `stm32h7xx-hal` 0.16, `just`. No new dependency in `chimera-core`.

**Spec:** `docs/superpowers/specs/2026-09-28-projects-storage-design.md` at 12fa052 (binding): § Plans item 1, § Types, § Storage, § Boot step 1, § Tests. The review it answers: `projects-spec-review.md` (C1, C2, H2, H6, H7, M1, M3, M6, M9).

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
- SD runs in SPI mode on SPI2: SCK PA9, MISO PB14, MOSI PB15.
- Init at ≤ 400 kHz, then switch to the fast clock.
- Transfers are **polled**, with no DMA. The stack is in DTCM, which DMA1 and DMA2 can't reach, and D2 is full. This supersedes the design doc's DMA2 note.
- Card I/O runs in the UI loop, never on the audio path. A BUSY/SAVING overlay is drawn before any card operation. Every operation has a timeout.
- States are `Absent`, `Ready(VolumeId)` and `Failed(CardError)`.
  - The card is mounted per operation. Every handle is closed on success and on error. Any error sets `Failed`.
  - `Failed` or `Absent` becomes `Ready` only after a fresh init and mount.
  - Each mount compares the volume serial and label with the cached `VolumeId`.
- FAT16 and FAT32 only. exFAT shows "CARD IS EXFAT: FORMAT FAT32".

**Files**
- Layout: `/CHIMERA/SYSTEM.A` and `.B`, with global settings; `/CHIMERA/PROJECTS/P0000001.A` and `.B`, a project. The names are 8.3.
- A/B:
  - Every file is a pair. The header carries a `u32` generation.
  - A save truncates and rewrites the older file of the pair (or the missing one) with generation + 1, then flushes.
  - The CRC32 is a trailer. The reader takes the valid file with the highest generation.
  - A delete removes the older file first.

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
- The parser never panics.

**RAM**
- There is no staging copy and no serialised buffer. Saves stream from live state, and loads use two passes. Nothing new goes in D2.
- Every new static is added to `AXI_RESIDENT`. Fix the stale stack comment on `UI_RESERVE` (ADR 0025).

**SYSTEM**
- It holds `ThemeSettings` (BRIGHT, GAMMA, ACCENT, BLACK) and the last project id, and nothing else.
- It is written on leaving System, only if the bytes changed. It uses A/B.
- Boot step 1: mount the card and read SYSTEM, then apply THEME. With no card, no file or an error, the built-in defaults apply.

**Repo rules (CLAUDE.md)**
- Every `unsafe` has a `// SAFETY:` comment. No heap and no blocking on the audio path. No libc.
- Commits are terse, with no type prefix and no Claude attribution.
- `just check` passes before every commit.

## Review Focus

These are the failure modes the spec implies but no spec'd test exercises, most likely first. Each has a test in the task named.

1. **A card with no partition table** ("superfloppy": FAT written from block 0, as some cameras and `mkfs` without a partition table leave it). It must give the clear error `Unsupported::NoPartitionTable`, not `Io` or a panic. Task 1: `superfloppy_is_no_partition_table`.
2. **A card pulled mid-save, then put back.** The store errors, and `Card` goes to `Failed`. The next operation re-inits and mounts. The event is `Same` (not `Swapped`), and the previous generation loads. Task 11: `cut_then_reinsert_loads_previous_generation`.
3. **A card that fills mid-save.** `save_ab` returns `Full`, and the older generation still loads. Task 11: `full_card_keeps_previous_generation`.
4. **Leaving System with no card in.** Exactly one write attempt fails fast with `NoCard`. It isn't retried every frame, and the next exit from System tries again. Task 12: `no_card_exit_tries_once`.
5. **Both files of a pair at the same generation** (a user copied `.A` over `.B` on a computer). The newer is chosen deterministically (A), and the next save goes to B with generation + 1. Task 11: `tied_generations_prefer_a_then_write_b`.

## Decisions this plan makes where the spec is silent

- **embedded-hal versions.** `stm32h7xx-hal` 0.16 implements only embedded-hal **0.2.7**; `Cargo.lock` has no 1.0. `embedded-sdmmc` 0.10 needs 1.0 `SpiDevice<u8>` and `DelayNs`. `chimera-stm32/src/sd.rs` wraps the HAL's blocking `Transfer`/`Write` (0.2) in a 1.0 `SpiDevice` that drives CS itself, and it implements `DelayNs` with `cortex_m::asm::delay`. A scratch `cargo check --target thumbv7em-none-eabihf` of `embedded-sdmmc = { version = "0.10", default-features = false }` builds. Both embedded-hal majors coexist in the lock.
- **SPI kernel clock.** SPI1/2/3 share one kernel clock (`SPI123SEL`), today `pll1_q_ck` at 200 MHz. The largest SPI divider is 256, so 200 MHz can't go below 781 kHz, which breaks the ≤ 400 kHz init. Move `SPI123SEL` to **PLL2_P at 100 MHz**:
  - the display keeps 50 MHz (100 / 2);
  - SD init runs at 390.6 kHz (100 / 256);
  - the fast clock is 12.5 MHz (100 / 8) or 25 MHz (100 / 4), chosen at the Task 2 STOP.
- **CS and card-detect.** Nothing in the repo's docs or code gives them. The only source is `.claude/projects/-home-hermes-dev-chimera/memory/reference_preenfm3_firmware.md` (from Ixox/preenfm3), which gives **CS = PE12**, AF5 on SPI2, and no card-detect line.
  - The plan uses PE12 and assumes **no card-detect**: `Absent` is inferred from an init failure (`StoreError::NoCard`).
  - The owner confirms both against the schematic at the Task 2 STOP. A card-detect line, if there is one, is filed as a follow-up issue, not built here.
- **SPI mode.** Use MODE 0, the SD standard. The stock firmware's CPHA = 2EDGE is noted; the probe reports if MODE 0 fails.
- **Volume serial.** `embedded-sdmmc` 0.10 doesn't expose the serial (`Bpb` has only `volume_label`). `chimera-fat::volume` parses the MBR and the partition boot sector itself (BS_VolID at 0x27 for FAT16, 0x43 for FAT32). The same parse detects exFAT (partition type 0x07 with OEM `EXFAT   `) and cards with no MBR.
- **Timeouts.** Every `Store` operation has a deadline of `OP_TIMEOUT_MS = 2000`, enforced in the SPI adapter from the DWT cycle counter; past it, `StoreError::Timeout`. `embedded-sdmmc`'s own bounded busy-waits stay underneath.
- **The streaming serialiser is push, not pull.** The spec says "an iterator of ≤ 512 B chunks". Instead, the encoder writes records into a `ByteSink` that the store buffers in one 512 B block and flushes per block. The streaming, no-copy and running-CRC properties are the same, and there's no resumable state machine. Records are ≤ `MAX_RECORD_LEN` = 512 B and are built on the stack.
- **The fuzz test.** It is a seeded xorshift property test, the repo's pattern (`tests/property_test.rs`, no `proptest`), plus a `cargo fuzz` target in `chimera-core/fuzz/` (excluded from the workspace; `just fuzz` needs `cargo install cargo-fuzz`).
- **Compile-fail tests** are rustdoc `compile_fail,E…` doc tests, pinned by error code, with no `trybuild` dependency.
- **`Sound.name` becomes `SoundName` (`Name<16>`).** `Sound::init`'s `(init)` isn't a valid name, so it becomes `INIT`. Tests that match `(init)` change, and any screen golden that shows it is re-recorded in Task 8 (ADR 0011: the spec's `Name` type makes the change).
- **Every `ParamKind::Enum` param gets a frozen code**, not only the enums the spec lists. That adds `EnvSpeed`, `HoldPos`, `FuncMode`, `LfoForm`, `ResonatorMode`, the LFO SHAPE and SYNC, the chorus MODE, the comp RATIO, `Gamma`, `Accent`, `Bright`, `Black` and the channel. A test fails if any Enum spec lacks one.
- **`Sound::bits_eq`** is listed under plan 2's derived marks, but the round-trip test needs it, so it lands here.

## File structure

| File | Responsibility |
|---|---|
| `chimera-hal/src/store.rs` | `Store` trait and its vocabulary: `VolumeId`, `Dir`, `FileName`, `StoreError`, `Unsupported`, `ReadSink`, `ByteSink`, `CHUNK`. |
| `chimera-hal/src/testkit.rs` (feature `testkit`) | `MemStore`, and `store_suite`, the conformance suite every `Store` passes. |
| `chimera-fat/` (new crate) | `volume.rs`, a pure MBR and boot-sector parser; `store.rs`, `FatStore` over `embedded-sdmmc`, plus the `Medium`/`SdBus` hooks. `tests/common/image.rs` is a FAT16/FAT32/exFAT image builder, with `RamDisk` and `CutDisk`. |
| `chimera-stm32/src/sd.rs` | SPI2 pins and clock; `SdSpi` (a 1.0 `SpiDevice` with a deadline); `CycleDelay`; `SdStore`; `take_store`. |
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
| `chimera-core/src/storage/card.rs` | `Card`, `Ready`, `CardEvent`, `CardFault`. |
| `chimera-core/src/storage/file.rs` | `Decode`, two-pass `load_file`, streaming `save_file`, `AbFile`, `SideState`, `save_ab`/`load_ab`/`delete_ab`. |
| `chimera-core/src/storage/system.rs` | `SystemSettings`, `SystemDecoder`, `SystemSync`. |
| `chimera-core/src/ui/busy.rs` | `draw_busy`. |
| `chimera-core/tests/fixtures/disk_codes_v1.txt`, `tests/fixtures/v1/*` | The frozen code table and the v1 files. |
| `docs/adr/0045-card-format.md` | The format and card-access ADR. |

## Task order

1. Volume parser and the `chimera-fat` crate.
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

Tasks 3–5 don't depend on 6–9, and 6–9 don't depend on 2. Task 2 blocks only Task 13.

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
  - `pub enum Unsupported { Exfat, NoPartitionTable, NotFat(u8) }`.
- Produces, in `chimera_fat::volume`:
  - `pub enum FsKind { Fat16, Fat32 }`;
  - `pub fn first_partition(mbr: &[u8; 512]) -> Result<Partition, Unsupported>`, where `Partition { pub lba: u32, pub kind: u8 }`;
  - `pub fn boot_sector(bs: &[u8; 512], part_kind: u8) -> Result<(FsKind, VolumeId), Unsupported>`.
- Produces for tests (`tests/common/image.rs`):
  - `RamDisk` (`embedded_sdmmc::BlockDevice`, `RefCell<Vec<[u8; 512]>>`);
  - `fn fat16(blocks: u32, serial: u32) -> RamDisk`, `fn fat32(serial: u32) -> RamDisk` (≥ 65 525 clusters), `fn exfat() -> RamDisk`, `fn superfloppy() -> RamDisk`;
  - `CutDisk { inner: RamDisk, writes_left: Cell<Option<u32>> }`, which fails every write after `writes_left` reaches 0.

- [ ] **Step 1: Write the failing tests** in `chimera-fat/tests/volume_test.rs`:
  - `fat16_serial_and_label`: `fat16(16_384, 0xDEAD_BEEF)` → `boot_sector` gives `(FsKind::Fat16, VolumeId { serial: 0xDEAD_BEEF, label: *b"CHIMERA    " })`.
  - `fat32_serial_and_label`: the same at 0x43/0x47, `FsKind::Fat32`.
  - `exfat_is_unsupported`: `first_partition` gives kind 0x07, then `boot_sector` gives `Err(Unsupported::Exfat)`.
  - `superfloppy_is_no_partition_table` (Review Focus 1): block 0 is a FAT boot sector (0x55AA, jump 0xEB, `FAT` at 0x36 or 0x52, partition table entries not valid) → `Err(Unsupported::NoPartitionTable)`.
  - `bad_signature_is_no_partition_table`: a zero block → `Err(Unsupported::NoPartitionTable)`.
  - `other_partition_type`: kind 0x83 → `Err(Unsupported::NotFat(0x83))`.
  - `images_open_in_embedded_sdmmc`: `VolumeManager::new(fat16(..), FixedTime)` opens volume 0 and `make_dir_in_dir(root, "CHIMERA")` succeeds, for both FAT16 and FAT32. This checks the image builder.
- [ ] **Step 2: Run** `cargo test -p chimera-fat` → FAIL (crate missing).
- [ ] **Step 3: Implement.**
  - `chimera-fat/Cargo.toml`: `no_std` lib; deps `chimera-hal`, `embedded-sdmmc = { version = "0.10", default-features = false }`, `embedded-hal = "1.0"`.
  - `volume.rs`:
    - FAT16 partition types are 0x04, 0x06, 0x0E; FAT32 are 0x0B, 0x0C.
    - Type 0x07 is exFAT when bytes 3..11 are `EXFAT   `, else `NotFat(0x07)`.
    - FAT32 when `BPB_FATSz16 == 0`. The serial is at 0x27 (FAT16) or 0x43 (FAT32), the label at 0x2B or 0x47.
  - The image builder writes an MBR (one partition at LBA 2048), a boot sector with 1 sector per cluster and 2 FATs, zeroed FATs with the media entries, and an empty root directory.
- [ ] **Step 4: Run** `cargo test -p chimera-fat` → PASS. Then `just check` → PASS.
- [ ] **Step 5: Commit** `git commit -m "chimera-fat: volume serial and FS type from the MBR and boot sector"`

### Task 2: SD on the chip — SPI2 and the probe (hardware STOP)

**Files:**
- Create: `chimera-stm32/src/sd.rs`, `chimera-stm32/src/sd_probe.rs`
- Modify: `chimera-stm32/Cargo.toml` (deps `chimera-fat`, `embedded-sdmmc` (no default features), `embedded-hal = "1.0"`; feature `sd-probe = []`)
- Modify: `chimera-stm32/src/clocks.rs:18-48`: `.pll2_p_ck(100.MHz())`, and select `Spi123ClkSel::Pll2P` on `ccdr.peripheral.SPI1`
- Modify: `chimera-stm32/src/main.rs`: split GPIOB unconditionally (PB7 stays under `midi-din`); `#[cfg(feature = "sd-probe")] sd_probe::run(&mut display, clk, sd)` after `display.init`, before `bench::run`
- Modify: `Justfile`: build and clippy `--features sd-probe` in `check`/`clippy`/`stack-check`; new recipe `flash-sd-probe`

**Interfaces:**
- Consumes: `chimera_fat::volume::{first_partition, boot_sector}` (Task 1).
- Produces, in `sd.rs`:
  - `pub const SD_INIT_HZ: u32 = 400_000;`
  - `pub const SD_FAST_HZ: u32` (12 500 000 until the STOP decides);
  - `pub const OP_TIMEOUT_MS: u32 = 2000;`
  - `pub struct SdSpi`: owns `Spi<SPI2, Enabled>`, CS `PE12` and the SPI2 `rec`; implements `embedded_hal::spi::SpiDevice<u8>`, with `ErrorType::Error = SdSpiError { Spi, Timeout }`;
  - `impl SdSpi { pub fn new(..) -> Self; pub fn set_hz(&mut self, hz: u32); pub fn start_op(&mut self); }`. `set_hz` does `free()` and re-`spi()`;
  - `pub struct CycleDelay { cpu_hz: u32 }`, which implements `embedded_hal::delay::DelayNs`;
  - `pub type SdDevice = embedded_sdmmc::SdCard<SdSpi, CycleDelay>`;
  - `pub fn init(spi2, rec, pa9, pb14, pb15, pe12, clocks, cpu_hz) -> SdDevice`.

- [ ] **Step 1: Wire the pins and clock.**
  - SCK PA9, MISO PB14 and MOSI PB15 go to `into_alternate::<5>()` at `Speed::High`. CS PE12 is push-pull, driven high before the SPI starts.
  - `spi::Config::new(spi::MODE_0)` at `SD_INIT_HZ`.
  - On every transfer, `SdSpi` checks `DWT::cycle_count()` against the start set by `start_op` and returns `SdSpiError::Timeout` past `OP_TIMEOUT_MS`.
  - `SpiDevice::transaction`: CS low; for each `Operation`, `Read` fills 0xFF and `Transfer::transfer`, `Write` uses `Write::write`, `Transfer`/`TransferInPlace` copy and transfer, and `DelayNs` uses `CycleDelay`; then CS high on every path.
- [ ] **Step 2: Write `sd_probe::run(display, clk, sd: SdDevice) -> !`.** It prints each line with `FmtBuf` as `bench.rs` does:
  1. the kernel clock and the actual init SCK, from `Spi2::kernel_clk_unwrap(&ccdr.clocks)` / divider; it must read ≤ 400 kHz;
  2. `sd.num_bytes()` and `get_card_type()`, or the error;
  3. block 0 and the partition boot sector through `first_partition`/`boot_sector`: `FsKind`, serial in hex, label;
  4. `VolumeManager` opens volume 0 and lists the first 8 root entries;
  5. write `/CHIMPROB.TXT` (16 KB pattern), read it back, compare, then delete it. Do this at 12.5 MHz, then again at 25 MHz, printing OK/FAIL and KB/s for each;
  6. `size_of` of `Sound`, `SoundPool`, `Performance`, `UiState` and `TripleBuffer<AudioShared>`, then `AXI_SRAM − AXI_RESIDENT` and `VOICE_RAM_BUDGET − size_of::<Instrument>()` (spec: "Plan 1 re-measures them on the chip").

  Then it loops forever with the LED on. No audio starts in this build.
- [ ] **Step 3: Build.** `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf --features sd-probe` → OK. Then `just check` → PASS; the stack check includes `sd-probe`.
- [ ] **Step 4: Check the display on the default build.** `just flash` on the owner's unit, if already at the bench, or defer it to the STOP: the display still inits and draws at 50 MHz from PLL2_P.
- [ ] **Step 5: Commit** `git commit -m "SD on SPI2, polled, and a bring-up probe"`
- [ ] **Step 6: STOP. Ask the owner to run `just flash-sd-probe` with a FAT32 card, then an exFAT card and no card, and wait.** The owner reports:
  - every probe line;
  - whether CS = PE12 matches the schematic, and whether the TFT module's SD slot has a card-detect line (if so, file a follow-up issue with the pin);
  - which fast clock passed readback.

  Record the answers under `## Measured` at the end of this plan. Set `SD_FAST_HZ` to the fastest clock that passed (never above 25 MHz), and commit `git commit -m "SD fast clock from the probe"`. If init fails in MODE 0, retry with MODE 1 (the stock CPHA) before anything else.

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
pub enum StoreError { NoCard, Unsupported(Unsupported), NotFound, Full, Timeout, VolumeChanged(VolumeId), Io }
impl StoreError { pub fn message(self) -> &'static str; } // "NO CARD", "CARD IS EXFAT: FORMAT FAT32",
    // "CARD HAS NO PARTITION TABLE", "CARD IS NOT FAT16/FAT32", "FILE NOT FOUND", "CARD FULL",
    // "CARD TIMEOUT", "CARD CHANGED", "CARD ERROR"
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
  - `pub fn store_suite<S: Store>(make: &mut dyn FnMut() -> S, swap: &mut dyn FnMut(&mut S))`.

- [ ] **Step 1: Write the failing test.** `mem_store_test.rs::mem_store_passes_suite` calls `store_suite(&mut || MemStore::new(1), &mut |s| s.swap(2))`. The suite asserts:
  - `mount` twice gives the same `VolumeId`;
  - write then read gives identical bytes for sizes 0, 1, 511, 512, 513 and 5 000, and `begin` gets the exact length;
  - writing a shorter file over a longer one truncates;
  - `list(Dir::Chimera)` gives each written name with its size; `list` of a missing dir gives `NotFound`;
  - `read` and `delete` of a missing file give `NotFound`;
  - `make_dir` twice gives `Ok`;
  - a `ReadSink` that breaks after the first chunk gives `Ok`, and the next operation works;
  - a `body` that returns `Err(Io)` after 700 bytes propagates `Io`, and the next operation works;
  - after `swap`, an op with the old `VolumeId` gives `VolumeChanged(new)` and doesn't write, and `mount` gives the new id.
  - `messages`: `StoreError::Unsupported(Unsupported::Exfat).message() == "CARD IS EXFAT: FORMAT FAT32"` (the spec's exact text), and every variant's message is non-empty ASCII uppercase.
- [ ] **Step 2: Write the compile-fail doc test** on `FileName`: ```` ```compile_fail,E0451 ```` building `FileName { .. }` with fields outside the crate.
- [ ] **Step 3: Run** `cargo test -p chimera-hal --features testkit` → FAIL.
- [ ] **Step 4: Implement** `store.rs` and `testkit.rs`. `MemStore::write` buffers in a `Vec` and commits on `Ok`, then keeps what was written on `Err` (the same as FAT). An ejected `MemStore` gives `NoCard` from every method.
- [ ] **Step 5: Run** `cargo test -p chimera-hal --features testkit` → PASS. Add `--features testkit` to the `chimera-hal` lines of `Justfile` `test`/`check`/`clippy`. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "Store trait, MemStore and the conformance suite"`

### Task 4: `FatStore` passes the suite on a RAM disk

**Files:**
- Create: `chimera-fat/src/store.rs`, `chimera-fat/tests/fat_store_test.rs`
- Modify: `chimera-fat/src/lib.rs`, `chimera-fat/Cargo.toml` (dev-dep `chimera-hal` with `testkit`)

**Interfaces:**
- Consumes: `Store` and everything in `chimera_hal::store` (Task 3); `volume::*` (Task 1).
- Produces, in `chimera_fat`:
  - `pub trait Medium: embedded_sdmmc::BlockDevice { fn reinit(&self) {} fn mounted(&self) {} fn start_op(&self) {} }`;
  - `pub trait SdBus { fn set_slow(&mut self); fn set_fast(&mut self); fn start_op(&mut self); }`;
  - `impl<S: SpiDevice<u8> + SdBus, D: DelayNs> Medium for SdCard<S, D>`: `reinit` is `spi(set_slow)` then `mark_card_uninit`; `mounted` is `spi(set_fast)`;
  - `pub struct FatStore<D: Medium, T: TimeSource>` with `pub fn new(dev: D, time: T) -> Self` and `pub fn has_open_handles(&self) -> bool`;
  - `pub struct FixedTime` (2026-01-01 00:00), `impl TimeSource`.

- [ ] **Step 1: Write the failing tests** in `fat_store_test.rs`:
  - `fat16_passes_suite` and `fat32_passes_suite`: `store_suite(&mut || FatStore::new(fat16(16_384, 1), FixedTime), &mut |s| *s = FatStore::new(fat16(16_384, 2), FixedTime))`. After every suite step, the test asserts `!store.has_open_handles()`; add a hook to `store_suite` for that, `after_op: &mut dyn FnMut(&S)`, with a no-op for `MemStore`.
  - `exfat_mount_is_unsupported`: `mount` → `Err(StoreError::Unsupported(Unsupported::Exfat))`.
  - `superfloppy_mount_is_unsupported`: `NoPartitionTable`.
  - `error_calls_reinit`: a `Medium` wrapper that counts `reinit`. A `CutDisk` write failure → `Err(Io)` and `reinit` count 1; the next `mount` → `Ok`.
- [ ] **Step 2: Run** `cargo test -p chimera-fat` → FAIL.
- [ ] **Step 3: Implement `FatStore`.**
  - `mount`: `start_op`; read block 0 and the boot sector through `vm.device(|d| d.read(..))` and Task 1's parser; `open_raw_volume(VolumeIdx(0))`; `close_volume`; `mounted()`.
  - Each op: `start_op`, open the volume, compare the `VolumeId` from the boot sector, then open the dir chain (`CHIMERA`, then `PROJECTS`/`SOUNDS`). Every opened handle is closed in reverse on every path, through one private `with_dir(vol, dir, |vm, raw_dir| ..)`.
  - `write` uses `Mode::ReadWriteCreateOrTruncate`, buffers `put` bytes in one `[u8; CHUNK]` and writes per full block, then `flush_file` and `close_file`.
  - Error mapping:

    | From | `StoreError` |
    |---|---|
    | `Error::DeviceError(_)` whose card isn't found | `NoCard` |
    | `DiskFull` | `Full` |
    | `NotFound` | `NotFound` |
    | `SdSpiError::Timeout` | `Timeout` |
    | the rest | `Io` |

    Every `Err` calls `dev.reinit()`.
- [ ] **Step 4: Run** `cargo test -p chimera-fat` → PASS. Then `just check` → PASS.
- [ ] **Step 5: Commit** `git commit -m "FatStore: embedded-sdmmc behind Store, mounted per operation"`

### Task 5: `DirStore` passes the suite

**Files:**
- Create: `chimera-desktop/src/store.rs` (with `#[cfg(test)] mod tests`)
- Modify: `chimera-desktop/src/main.rs` (`mod store;` only), `chimera-desktop/Cargo.toml` (dev-dep `chimera-hal` with `testkit`), `.gitignore` (`chimera-card/`)

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
- [ ] **Step 4: Run** `cargo test -p chimera-desktop` → PASS. Then `just check` → PASS.
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
pub enum NameError { Empty, TooLong, BadChar(u8) }
impl<const N: usize> Name<N> {
    pub fn new(s: &str) -> Result<Self, NameError>;          // A–Z a–z 0–9 space '-'; 1..=N
    pub fn from_padded(b: &[u8; N]) -> Result<Self, NameError>; // NUL-padded disk form
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
pub struct Generation(u32);
impl Generation { pub const FIRST: Self; pub fn next(self) -> Self /* wrapping */; pub fn is_newer_than(self, o: Self) -> bool /* (self − o) as i32 > 0 */; pub fn get(self) -> u32; }
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
    - `rejects`: `""` → `Empty`; 17 chars → `TooLong`; `"(init)"` → `BadChar(b'(')`; `"\u{e9}"` → `BadChar`;
    - `padded_round_trip`;
    - `from_padded_rejects_a_gap`: NUL then a letter → `BadChar(0)`.
  - `storage_frame_test.rs`:
    - `crc_known_answer`: `Crc32` over `b"123456789"` is `0xCBF4_3926`;
    - `generation_wraps`: `Generation(u32::MAX).next()` is 0, and `is_newer_than(Generation(u32::MAX))`;
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
  - A/B generations and the delete order;
  - the must-understand bit;
  - frozen codes and the retired list, neutral defaults, migration by a new `ParamId`, and the fixture corpus;
  - the reserved `FileKind`s;
  - the card-access decisions: SPI2 polled, the PLL2_P kernel clock, CS PE12, no card-detect, `OP_TIMEOUT_MS`, mount per operation, and the serial from our own BPB parse.

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
- Modify: `chimera-core/src/mod_path.rs` (`#[derive(Debug, PartialEq)]` on the registry and its entry), `chimera-core/src/modulation.rs` (`#[derive(PartialEq)]` on `ModState`; `pub fn with_sources(n: usize) -> Self`)
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
    pub fn bits_eq(&self, o: &Sound) -> bool;      // name, engine, every voice-block param by to_bits, ModState ==, registry ==
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
  - `factory_round_trip`: for `i in 0..FACTORY_LEN`, `decode(encode(s))?.bits_eq(&s)`, and `format!("{:?}")` of the params, `mod_state`, registry and name are equal (this catches a field no spec covers).
  - `init_round_trip` for each `EngineType::ALL`.
  - `zero_amount_route_survives`: a route set at amount 0 keeps its present bit.
  - `full_matrix_round_trip`: 16 dests × 8 sources, with random amounts from a fixed xorshift seed.
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
- Consumes: `Store`, `StoreError`, `VolumeId` (Task 3); `MemStore` (tests).
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq)] pub struct Ready { vol: VolumeId }   // private field
impl Ready { pub fn volume(self) -> VolumeId; }
#[derive(Clone, Copy, Debug, PartialEq)] pub enum Card { Absent, Ready(Ready), Failed(StoreError) }
#[derive(Clone, Copy, Debug, PartialEq)] pub enum CardEvent { Mounted, Same, Swapped { old: VolumeId } }
pub trait CardFault { fn store_error(&self) -> Option<StoreError>; }   // impl for StoreError, LoadError, SaveError
pub fn after_mount(card: Card, r: Result<VolumeId, StoreError>) -> (Card, Option<CardEvent>);  // pure
pub fn after_error(card: Card, e: StoreError) -> Card;                                         // NoCard → Absent, else Failed(e)
impl Card {
    pub const fn new() -> Card;  // Absent
    pub fn run<S: Store, R, E: CardFault + From<StoreError>>(&mut self, store: &mut S,
        op: impl FnOnce(&mut S, Ready) -> Result<R, E>) -> Result<(R, CardEvent), E>;
    pub fn ready(&self) -> Option<Ready>;
}
```

`run` mounts (per operation), applies `after_mount`, runs `op` with the fresh `Ready` and applies `after_error` on a store error. A `Swapped` or `Mounted` event is returned so plans 2 and 3 can drop the index and project list and re-validate a `Pending` replace.

- [ ] **Step 1: Write the failing tests** in `card_test.rs`:
  - `transition_table`: `after_mount` over every (`Card` state, `Ok(same)`/`Ok(other)`/`Err(NoCard)`/`Err(Io)`) pair:
    - `Absent` + Ok → `Ready`, `Mounted`;
    - `Ready(v)` + Ok(v) → `Same`;
    - `Ready(v)` + Ok(w) → `Ready(w)`, `Swapped { old: v }`;
    - `Failed` + Ok(v) → `Ready(v)`, `Mounted`;
    - any + `Err(NoCard)` → `Absent`, `None`;
    - any + `Err(Io)` → `Failed(Io)`, `None`.
  - `op_error_fails_card`: an `op` returning `Err(Io)` leaves `Card::Failed(Io)`.
  - `eject_then_insert`: on a `MemStore`, `eject()` → `run` gives `Err(NoCard)` and `Absent`; after re-insert → `Mounted`.
  - `swap_between_ops`: `MemStore::swap(2)` between two `run`s → the second event is `Swapped { old }`.
- [ ] **Step 2: Write the compile-fail doc test** on `Ready`: ```` ```compile_fail,E0451 ```` `Ready { vol }`.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test card_test` → FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `cargo test -p chimera-core --test card_test` → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "Card: mounted per operation, swaps seen by volume serial"`

### Task 11: Two-pass load, streaming save and A/B; the power cut

**Files:**
- Create: `chimera-core/src/storage/file.rs`, `chimera-core/tests/storage_ab_test.rs`
- Create: `chimera-fat/tests/power_cut_test.rs` (dev-dep `chimera-core`)
- Modify: `chimera-core/src/storage/sound.rs` (`impl Decode for SoundDecoder`)

**Interfaces:**
- Consumes: `Framer`, `write_file`, `Header` (Task 6); `SoundDecoder` (Task 8); `Card`, `Ready`, `CardFault` (Task 10); `Store` (Task 3).
- Produces:

```rust
pub trait Decode {
    const KIND: FileKind;
    fn event(&mut self, e: Event<'_>, apply: bool) -> Result<(), FileError>;
    fn end(&mut self, apply: bool) -> Result<(), FileError>;
}
pub enum LoadError { Store(StoreError), File(FileError), Missing }
pub enum SaveError { Store(StoreError) }
pub struct AbFile { /* dir: Dir, stem [u8; 8], len */ }
impl AbFile { pub fn new(dir: Dir, stem: &[u8]) -> Option<AbFile>; pub const SYSTEM: AbFile; pub fn side(&self, s: Side) -> FileName; }
pub enum SideState { Missing, Torn, Present { gen: Generation, err: Option<FileError> } }
pub fn newest(a: SideState, b: SideState) -> Option<(Side, Generation, Option<FileError>)>; // tie → A
pub fn write_target(a: SideState, b: SideState) -> (Side, Generation);  // not-newest side; gen = newest.next() or FIRST
pub fn delete_order(a: SideState, b: SideState) -> [Side; 2];           // older first
pub fn check_file<S: Store, D: Decode>(s: &mut S, r: Ready, f: FileName, d: &mut D) -> Result<SideState, StoreError>; // pass 1 only
pub fn load_file<S: Store, D: Decode>(s: &mut S, r: Ready, f: FileName, d: &mut D) -> Result<Header, LoadError>;  // pass 1 then pass 2
pub fn save_ab<S: Store>(s: &mut S, r: Ready, f: AbFile, kind: FileKind, name: Option<Name<16>>,
    body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>) -> Result<Generation, SaveError>;
pub fn load_ab<S: Store, D: Decode>(s: &mut S, r: Ready, f: AbFile, d: &mut D) -> Result<Header, LoadError>;
pub fn delete_ab<S: Store>(s: &mut S, r: Ready, f: AbFile) -> Result<(), StoreError>;
```

The rules:
- `check_file` gives:
  - `Missing` on `NotFound`;
  - `Torn` on a torn `FileError` (`is_torn`);
  - `Present { gen, err: Some(e) }` on any other `FileError`, such as `NeedsNewerFirmware`, when the header was read;
  - `Present { gen, err: None }` when pass 1 passes.
- `load_ab` loads the `newest` side. If that side is `Present` with an error, it returns that error and doesn't fall back, so newer data is never shadowed. With no present side, it returns `Missing`.
- `save_ab` never writes over the newest present side, even one that needs newer firmware. It streams `header → body → trailer` through `Store::write`.
- `load_file`'s pass 2 re-checks the CRC. If it differs from pass 1 (the card changed), it returns `File(BadCrc)`.

- [ ] **Step 1: Write the failing tests** in `storage_ab_test.rs` (on `MemStore`, with Sounds):
  - `newest_and_write_target_table`: every pair of {`Missing`, `Torn`, `Present` gen 1, `Present` gen 2, `Present` gen 2 needing newer firmware} gives the expected (side, gen), including the wrap from `u32::MAX` to 0.
  - `tied_generations_prefer_a_then_write_b` (Review Focus 5).
  - `save_twice_alternates`: save → A gen 1; save → B gen 2; save → A gen 3; `load_ab` gives the last content.
  - `torn_newest_falls_back`: truncate the newest file by 10 B; `load_ab` gives the older content.
  - `newer_firmware_newest_does_not_fall_back`: the newest file has a critical unknown record → `Err(File(NeedsNewerFirmware))`; the next `save_ab` writes the other side.
  - `delete_older_first`: a `MemStore` wrapper logs deletes. The order is the older side, then the newer; after the first delete, `load_ab` still loads.
  - `save_streams_in_chunks`: a wrapper sink records the size of each `put`. None exceeds `MAX_RECORD_LEN + 4`, and no whole-file buffer exists (the API has no `Vec`).
  - `load_leaves_target_on_error`: a corrupt newest with a missing older → `Err`, and the target is unchanged.
- [ ] **Step 2: Write the failing tests** in `chimera-fat/tests/power_cut_test.rs` (on `FatStore<CutDisk>`):
  - `cut_at_every_block_write_keeps_previous_generation`: save a Sound (gen 1), then count the block writes `n` of a second save. For `k in 0..n`, restore the gen-1 image, allow `k` writes, save (it errors), then remount a fresh `FatStore` on the image. `load_ab` gives gen 1's Sound or, when `k` is high enough, gen 2's; never an error.
  - `cut_then_reinsert_loads_previous_generation` (Review Focus 2): through `Card::run`. The cut save leaves `Card::Failed`; the next `run` gives `CardEvent::Same`, and `load_ab` gives gen 1.
  - `full_card_keeps_previous_generation` (Review Focus 3): a FAT16 image filled with a padding file until one cluster is left. A larger save returns `SaveError::Store(Full)`, and `load_ab` gives the previous generation.
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
- Consumes: `save_ab`, `load_ab`, `Decode`, `AbFile::SYSTEM` (Task 11); `Card` (Task 10); `encode_block`/`decode_block` (Task 8); `ProjectId` (Task 6).
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

- [ ] **Step 1: Write the failing tests** in `system_file_test.rs` (on `MemStore`):
  - `round_trip`: non-default theme values (BRIGHT 40, GAMMA SOFT, ACCENT index 3, BLACK +1) and `last_project` `ProjectId::new(7)`; `write`, then `boot` → equal.
  - `boot_no_card_defaults`: an ejected `MemStore` → `SystemSettings::DEFAULT`, `Some(BootNote::NoCard)`, `Card::Absent`.
  - `boot_no_file_defaults`: `Some(BootNote::NoFile)`.
  - `boot_corrupt_defaults`: both sides torn → `DEFAULT`, `Some(BootNote::Error(_))`.
  - `unknown_record_kept_theme`: a SYSTEM file with an extra non-critical tag 0x0071 still gives the theme.
  - `writes_only_on_exit_edge_and_change`:
    - frames in System → `false`;
    - exit with no change → `false`;
    - enter, change BRIGHT, exit → `true` once;
    - the next frame → `false`.
  - `no_card_exit_tries_once` (Review Focus 4): with the store ejected, the exit edge gives `wants_write` `true` and `write` `Err(NoCard)`; 10 more frames out of System → `false`; re-enter and exit → `true` again.
  - `theme_never_touches_project_state`: `SystemSync` takes no `Performance` or `UiState`. This is a type check: a comment names it, and there's no test body beyond calling the API.
  - `system_fixture_loads`: `fixtures/v1/system.sys` decodes to its recorded values.
- [ ] **Step 2: Write the screen golden** `busy_saving`: a centred box with `SAVING`, on the default theme; record it once.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test system_file_test --test screen_golden_test` → FAIL.
- [ ] **Step 4: Implement.** Extend `write_v1_fixtures` to write `system.sys`, run it once with `FIXTURE_WRITE=1`, and commit the file.
- [ ] **Step 5: Run** the tests → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit** `git commit -m "SYSTEM file: theme and last project, written on leaving System"`

### Task 13: Shell wiring — boot SYSTEM, save on leaving System, the RAM budget (hardware STOP)

**Files:**
- Modify: `chimera-core/src/hw.rs:61-66`:
  - `pub const STORE_RESERVE: usize = 4 * 1024;`, covering the `VolumeManager` and `FatStore`'s 512 B buffer;
  - fix the `UI_RESERVE` comment. The stack is in DTCM (ADR 0025), so the reserve covers renderer and navigation state, not "`main`'s stack temporaries and the interrupt stacks".
- Modify: `chimera-core/src/instrument.rs:32-40` (`+ STORE_RESERVE` in `AXI_RESIDENT`)
- Modify: `chimera-core/tests/memory_budget_test.rs`: assert `AXI_RESIDENT` includes `STORE_RESERVE`, and that the AXI left over is ≥ 64 KB (it was ~91 KB before this plan; the SYSTEM path adds no other static)
- Modify: `chimera-stm32/src/sd.rs`:
  - `pub type SdStore = FatStore<SdDevice, FixedTime>`;
  - `const _: () = assert!(size_of::<SdStore>() <= STORE_RESERVE);`;
  - `pub fn take_store(..) -> Option<&'static mut SdStore>`, a take-once `static mut MaybeUninit` in AXI, as `shared.rs` does.
- Modify: `chimera-stm32/src/main.rs`:
  - after GPIO split and before the backlight: `sd::take_store`, then `SystemSync::boot(&mut card, store)`, then `theme = settings.theme`, then `ui.set_theme`;
  - in the loop after `handle_input`: `if sync.wants_write(ui.in_system(), &settings) { draw_busy → flush_region → sync.write }`;
  - `settings.theme = ui.theme()` each frame.
- Modify: `chimera-desktop/src/main.rs`: the same, with `DirStore::new(env CHIMERA_CARD or "chimera-card")`. The default dir is created if missing; a `CHIMERA_CARD` that doesn't exist stays "no card".

**Interfaces:**
- Consumes: `SystemSync`, `SystemSettings`, `draw_busy`, `Card` (Tasks 10–12); `FatStore` (Task 4); `SdDevice`, `SD_FAST_HZ` (Task 2); `DirStore` (Task 5).
- Produces, for plan 2:
  - the shell owns one `Card` and one `&mut impl Store` beside `UiState`;
  - `UiState::in_system` is replaced by `Location` in plan 2, and `SystemSync::write` is called on project save and load for `last_project`.

- [ ] **Step 1: Write the failing test** in `memory_budget_test.rs`: `axi_counts_the_store` asserts `AXI_RESIDENT - (the old sum) == STORE_RESERVE` and `AXI_SRAM - AXI_RESIDENT >= 64 * 1024`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test memory_budget_test` → FAIL.
- [ ] **Step 3: Implement** `hw.rs`, `instrument.rs`, `sd.rs` and both shells.
- [ ] **Step 4: Run** `just check` → PASS (firmware at every feature set, stack check, clippy). Then run `just desktop`:
  - change THEME, leave System (MENU to a Part), quit and relaunch: the theme is back;
  - `CHIMERA_CARD=/nonexistent just desktop`: the defaults apply and leaving System doesn't hang.
- [ ] **Step 5: Commit** `git commit -m "Boot reads SYSTEM; leaving System saves it"`
- [ ] **Step 6: STOP. Ask the owner to run these checks on the unit with `just flash`, and wait.**
  1. Boot with the Task 2 card: the default theme. Change BRIGHT and ACCENT, and leave System: SAVING flashes. Power off and on: the theme comes back.
  2. Boot with no card: the defaults apply, and boot doesn't wait longer than `OP_TIMEOUT_MS`. Leave System: one quick failure, and audio is unaffected.
  3. Insert the card while running, then change the theme and leave System: it saves (`Absent → Ready`).
  4. Swap to a different FAT32 card while running, then leave System: it saves to the new card. The next boot with the first card shows its own theme.
  5. Pull the card during SAVING (repeat until it lands mid-save): the next boot loads the earlier theme or the new one, never the defaults, unless the card has no SYSTEM file.
  6. An exFAT card: the defaults apply at boot, with no hang.

  Record the results under `## Measured`. File any failure as a GitHub issue before fixing it.

---

## Measured

*(Filled in at the Task 2 and Task 13 STOPs: the probe's lines, CS and card-detect, `SD_FAST_HZ`, the chip `size_of` figures and the hardware checks.)*
