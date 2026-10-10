# Sound Library Implementation Plan

**Goal:** Save a Part's Sound to `/CHIMERA/SOUNDS` with a build stamp, then browse that library from any project and load a Sound back into the Part through the pool. The USB console gets verbs to manage the files.

**Architecture:**
- **Format** (`chimera-engine/src/storage/library.rs`): pure. Ids, the `Build` record, the lenient peek, and an in-place decoder.
- **Card and project** (`chimera-engine/src/project/library.rs`): save, list and read run inside `Card::run`. Loads use a third sealed replace target, `LibraryPart`, and commit through `Project::adopt`, which stores the source, the slot and the Part together.
- **UI** (`chimera-core`): queues card jobs as SAVE TO PROJ and LOAD PROJECT do. The Sound browser gains a PROJECT/LIBRARY tab on encoder B. The console gains `ls sounds`, `put`, `rm` and `get`.

**Tech Stack:** Rust `no_std`, `chimera-hal` `Store`, `MemStore` and `DirStore` for tests, the `chimera-fat` card-image tests (dosfstools), `cargo fuzz`.

**Spec:** `docs/superpowers/specs/2026-10-09-sound-library-design.md` (owner-approved 2026-10-10, review fixes applied)

## Global Constraints

- No heap allocation and no blocking in the audio callback; the library adds nothing to the audio side.
- Card I/O stays off the audio path. It runs only in `UiState::card_work` and in the console's answer, on the UI loop, behind BUSY.
- Writes only under `/CHIMERA/SOUNDS`, to `S#######.A|B`. The library makes `SOUNDS` only on the first save, never at mount.
- Stock PreenFM3 files are never opened or written (#405): `chimera-fat/tests/stock_files_test.rs` stays green.
- No `unsafe` without `// SAFETY:`. No whole `Sound` on a job's stack beyond what `just stack-check` passes.
- `0x001C` is `RecordTag::Build`, not critical, written second, after `Engine`. It never refuses a file.
- User ids `S0000001`–`S8999999`; factory ids `S9000001`–`S9999999`, never allocated by a save.
- UI copy is exact: `SAVED TO LIBRARY`, `NO SOUND IDS LEFT`, `FILE CHANGED`, `FILE IS GONE`, `LOADED TO SLOT 12`, `LOADED · SOME PARAMS SKIPPED`, `SOURCE MISSING`, `NO SOUNDS YET`, `‹ NEWER`, `OLDER ›`, `MIX + EDIT SAVES TO LIBRARY`, `SAVED ON …`, `SAVED ON THIS BUILD`, `NO STAMP`, `SAVE TO LIBRARY`, `SAVE TO LIB`.
- Touch `chimera-engine/src/lib.rs` (`touch chimera-engine/src/lib.rs`) before running `chimera-core` tests after an engine change, so the re-export rebuilds.
- Commit messages are terse with no type prefix.
- `just check` is green at the end of the last task.
- Report flash and AXI deltas in one line in the last task (flash rule). Don't shave bytes.

## Review Focus

1. **A Sound saved on a build that knows more params** (newer to older, or a param retired between builds) must load and say `SOME PARAMS SKIPPED`, never refuse. Task 2 pins it with an unknown id, block and record, and pins that migrated or retired-and-translated params count 0.
2. **A Part edited by MIDI CC while a TAPE source streams in** (several UI passes) must refuse at `adopt` with no orphan source and no slot. Task 5 has the moved-witness test; Task 6 drives it through the job.
3. **SAVE PART FIRST that eats the room** the library Sound needs must keep the save and refuse the load with `POOL FULL`. Task 6.
4. **A card edited over USB between listing and EDIT** (`rm`, then a save reusing the id; or a `put --over`) must refuse `FILE CHANGED` / `FILE IS GONE`, never load another Sound. Task 5 at the card level, Task 8 through the console.
5. **The console poking outside `SOUNDS/`** (`..`, `//`, `/CHIMERA/PROJECTS/…`, `S9…` with `rm`), or while a load is pending, must refuse without touching the card. Task 8.

---

### Task 1: The `Build` record and the stamped encoder

**Files:**
- Create: `chimera-engine/src/storage/library.rs`
- Modify: `chimera-engine/src/storage/mod.rs`, `chimera-engine/src/storage/record.rs:70-124` (tag tables), `chimera-engine/src/storage/sound.rs` (`encode_sound`, `once_bit`, `SoundCheck::step`), `chimera-engine/src/project/codec.rs` (exhaustive `RecordTag` matches, if any)
- Modify: `chimera-core/src/ui/about_page.rs` (`BuildStamp::THIS`)
- Test: `chimera-core/tests/library_format_test.rs`; `chimera-core/tests/storage_frame_test.rs` (tag golden); `chimera-core/tests/codec_compat_test.rs` (`library_1.snd`)

**Interfaces:**
- Produces, in `chimera_engine::storage` (re-exported as `chimera_core::storage`):
  - `RecordTag::Build` = `0x001C`.
  - `pub const BUILD_LEN: usize = 21;`
  - `pub struct BuildStamp { version: [u8; 12], hash: [u8; 8], dirty: bool }`, `Clone, Copy, Debug, PartialEq, Eq`. Methods:
    - `pub const fn new(version: &str, hash: &str, dirty: bool) -> BuildStamp`: panics in const context past 12 or 8 bytes.
    - `pub fn to_bytes(&self) -> [u8; BUILD_LEN]`.
    - `pub fn parse(p: &[u8]) -> Option<BuildStamp>`: `None` under 21 B; bytes past 21 ignored; non-printable bytes become `?`; flag bits other than 0 ignored.
    - `pub fn version(&self) -> &str`, `pub fn hash(&self) -> &str`, `pub fn dirty(&self) -> bool`.
    - `pub fn is_this_build(&self, this: &BuildStamp) -> bool`.
    - `impl core::fmt::Display`: `0.1.0 02e440e2`, with `+` when dirty.
  - `pub fn encode_library_sound(s: &Sound, stamp: &BuildStamp, w: &mut RecordWriter<'_>) -> Result<(), StoreError>`: `Engine`, `Build`, then `encode_sound`'s remaining records.
  - `pub(crate) fn encode_sound_body(s: &Sound, w: &mut RecordWriter<'_>) -> Result<(), StoreError>` in `sound.rs`: everything after `Engine`. `encode_sound` becomes `Engine` + `encode_sound_body`.
- Produces in `chimera_core::ui::about_page`: `pub const THIS: BuildStamp = BuildStamp::new(VERSION, HASH, DIRTY);` and `const _: () = assert!(VERSION.len() <= 12);`.

- [ ] **Step 1: Write the failing tests** in `library_format_test.rs`:

```rust
#[test] fn build_tag_is_0x001c_and_not_critical() {
    assert_eq!(RecordTag::Build.code(), 0x001C);
    assert_eq!(RecordTag::Build.code() & 0x8000, 0);
}
#[test] fn stamp_round_trips_and_prints() {
    let s = BuildStamp::new("0.1.0", "02e440e2", true);
    assert_eq!(BuildStamp::parse(&s.to_bytes()), Some(s));
    assert_eq!(format!("{s}"), "0.1.0 02e440e2+");
}
#[test] fn stamp_is_lenient() {
    let mut long = BuildStamp::new("0.1.0", "02e440e2", false).to_bytes().to_vec();
    long.extend_from_slice(&[0xAA; 9]);                                 // 30 B
    assert_eq!(BuildStamp::parse(&long).unwrap().hash(), "02e440e2");
    assert_eq!(BuildStamp::parse(&long[..3]), None);                    // 3 B
    let mut odd = long.clone(); odd[0] = 0x07;
    assert_eq!(BuildStamp::parse(&odd).unwrap().version(), "?.1.0");
}
#[test] fn this_build_needs_a_clean_real_hash() {
    let a = BuildStamp::new("0.1.0", "02e440e2", false);
    assert!(a.is_this_build(&a));
    assert!(!BuildStamp::new("0.1.0", "02e440e2", true).is_this_build(&a));
    assert!(!a.is_this_build(&BuildStamp::new("0.1.0", "02e440e2", true)));
    let dev = BuildStamp::new("0.1.0", "dev", false);
    assert!(!dev.is_this_build(&dev));
    let unk = BuildStamp::new("0.1.0", "unknown", false);
    assert!(!unk.is_this_build(&unk));
}
#[test] fn a_stamped_sound_decodes_bit_equal() {   // every factory Sound + INITs
    // encode_library_sound into a file via write_file; SoundDecoder; bits_eq the source
}
#[test] fn build_after_engine_anywhere_or_twice_loads() {
    // a stamped file with a second Build inserted after Routes, and one with a
    // 30 B and a 3 B Build payload: each decodes bits_eq.
}
#[test] fn build_before_engine_is_corrupt() {
    // header, then Build, then Engine: load_file → Err(FileError::Corrupt)
}
#[test] fn build_in_a_project_slot_loads() {
    // project fixture with a Build record after a Slot's Engine: ProjectDecoder loads it.
}
```

  Add `RecordTag::Build => 0x001C` to the tag golden in `storage_frame_test.rs`. Add a `library_1.snd` fixture to `codec_compat_test::write_v1_fixtures`: factory Sound 0, `encode_library_sound`, stamp `BuildStamp::new("0.1.0", "dev", false)`, generation 1. It goes into `v1_fixtures_render_identically`'s list. `unknown_non_critical_skipped` stays unchanged.

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_format_test --test storage_frame_test`
  Expected: compile errors: `BuildStamp`, `RecordTag::Build` and `encode_library_sound` are undefined.

- [ ] **Step 3: Implement**
  - `SoundCheck::step` returns `Ok(())` for `RecordTag::Build` right after the "Engine first, and once" check (so `Build` before `Engine` stays `Corrupt`) and before `once`. `once_bit(Build)` is `0` and is never consulted. `seen` stays `u8`.
  - Every exhaustive `RecordTag` match gains `Build`: the project codec treats it as unknown in a Sound context and `Corrupt` at top level.
  - Write the fixture with `CHIMERA_WRITE_FIXTURES=1` as `write_v1_fixtures` documents.

- [ ] **Step 4: Run the tests and see them pass**

  Run: the Step 2 command, plus `--test codec_compat_test --test sound_codec_test --test storage_ab_test`
  Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src/storage chimera-engine/src/project/codec.rs chimera-core/src/ui/about_page.rs chimera-core/tests/library_format_test.rs chimera-core/tests/storage_frame_test.rs chimera-core/tests/codec_compat_test.rs chimera-core/tests/fixtures/v1/library_1.snd
git commit -m "Build record: a library Sound carries the build it was saved on"
```

### Task 2: The skip count

**Files:**
- Modify: `chimera-engine/src/storage/block_codec.rs:67-144` (`decode_block`), `chimera-engine/src/storage/sound.rs` (`SoundCheck`), the other `decode_block` callers (`orbit/records.rs:215`, `project/codec.rs:484,661`, `storage/system.rs:126`, `block_codec.rs:229`)
- Test: `chimera-core/tests/library_format_test.rs`

**Interfaces:**
- Consumes: Task 1's `encode_library_sound`.
- Produces:
  - `pub fn decode_block(...) -> Result<u16, FileError>`, same parameters; the `u16` is the entries lost. Other callers drop it.
  - `impl SoundCheck { pub fn skips(&self) -> u16 }`. It counts in pass 2 only (when `staged` is `Some`) and resets at `Event::Header`.

  The count, as the spec defines it:
  - in `decode_block`, counted only with a target: an unknown block code not in `RETIRED_BLOCKS`; an unknown id with no migration and not in `RETIRED`; an enum code `set_enum_code` refuses that isn't in `RETIRED_CODES`;
  - in `SoundCheck`: an unknown non-critical record after `Engine`; a route whose `ModSource::from_disk_code` or `addr_of` is `None` (`finish`); a registry or ModDests entry whose `addr_of` is `None`.

  A block this engine doesn't hold isn't counted, and neither is `Build`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn skips_count_what_was_lost() {
    // a stamped INIT ALGO with: a Block entry (param id 250) in OUT's block,
    // a Block record with code 200, and an unknown record 0x0070 after Engine
    assert_eq!(decode_skips(&bytes), 3);
}
#[test] fn every_fixture_and_factory_sound_skips_nothing() {
    for (name, s) in all_v1_and_factory() {             // v1 .snd files + factory_sound(0..FACTORY_LEN)
        assert_eq!(decode_skips(&stamped(&s)), 0, "{name}");
    }
}
#[test] fn a_translated_param_is_not_a_loss() {
    // init_modal.snd (v1 EXCITE → BURST translation): skips == 0
}
#[test] fn pass_one_alone_counts_nothing() {
    // SoundCheck fed pass 1 only over the 3-skip file: skips() == 0
}
```

  `decode_skips` is a test helper: `check_file`, then pass 2 through a `SoundCheck` stepping a staged `Sound`, returning `skips()`.

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_format_test`
  Expected: FAIL: `skips` is undefined.

- [ ] **Step 3: Implement** as in Interfaces; the existing control flow decides nothing new.

- [ ] **Step 4: Run** `cargo test -p chimera-core -p chimera-engine --features chimera-hal/testkit`
  Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src chimera-core/tests/library_format_test.rs
git commit -m "Count what a Sound load had to drop"
```

### Task 3: Library ids, the peek, and the save

**Files:**
- Modify: `chimera-engine/src/storage/library.rs`
- Create: `chimera-engine/src/project/library.rs`; modify `chimera-engine/src/project/mod.rs` (module, re-exports)
- Create: `chimera-core/fuzz/fuzz_targets/peek_library.rs`; modify `chimera-core/fuzz/Cargo.toml` (bin entry) and the `Justfile` `fuzz` recipe (seed `library_1.snd`, run `peek_library`)
- Test: `chimera-core/tests/library_store_test.rs`

**Interfaces:**
- Consumes: Task 1's `encode_library_sound` and `BuildStamp`.
- Produces in `storage::library`:
  - `pub struct LibId(u32)`, `Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord`. Constants and methods:
    - `pub const USER_MAX: u32 = 8_999_999;` and `pub const FACTORY_MIN: u32 = 9_000_001;`
    - `pub fn new(n: u32) -> Option<LibId>`: 1..=9_999_999 and not 9_000_000.
    - `pub fn get(self) -> u32`, `pub fn is_factory(self) -> bool`, `pub fn stem(self) -> [u8; 8]`.
  - `pub fn library_file(id: LibId) -> AbFile` (`Dir::Sounds`).
  - `pub fn lib_id(f: FileName) -> Option<LibId>`: only `Dir::Sounds`, `S` + 7 digits, ext `A` or `B`.
  - `pub const ID_WINDOW: u32 = 4096;`
  - `pub fn next_user_id(top: u32, window_base: u32, taken: &[u8; 512]) -> NextId`, with `pub enum NextId { Id(LibId), Scan(u32), NoIds }`. It returns `top + 1` when `top < USER_MAX`. Otherwise it returns the lowest clear bit of the window at `window_base` (bit *i* = id `window_base + i`), or `Scan(next_base)`, or `NoIds` past `USER_MAX`.
  - `pub const LIB_PEEK: usize = 64;`
  - `pub struct Peek { pub generation: Generation, pub name: Option<SoundName>, pub engine: Option<EngineType>, pub stamp: Option<BuildStamp> }`.
  - `pub fn peek_library(b: &[u8], file_len: u32) -> Result<Peek, FileError>`. Under 32 B by `file_len` is `Truncated`. The header follows `peek_header`'s rules, and a kind other than `Sound` is `WrongKind`. An unknown engine code is `NeedsNewerFirmware`. `stamp` is `BuildStamp::parse` of the second record's payload when its tag is `0x001C`. It never panics on any bytes.
- Produces in `project::library`:
  - `pub enum LibNote { Store(StoreError), File(FileError), NoIds, Changed, Gone }`, with `pub fn message(self) -> &'static str`: the `StoreError`/`FileError` message, `NO SOUND IDS LEFT`, `FILE CHANGED`, `FILE IS GONE`.
  - `pub fn save_library_sound<S: Store>(card: &mut Card, store: &mut S, s: &Sound, name: SoundName, stamp: &BuildStamp) -> CardOut<Result<LibId, LibNote>>`. Inside one `Card::run`:
    1. `make_dir` `Chimera`, then `Sounds`;
    2. directory passes for the id;
    3. `save_ab(.., &mut SoundCheck::new(), Some(name), body)`, the body calling `encode_library_sound`;
    4. `check_file` on `.A`, deleting `.A` on any error after the write began.

- [ ] **Step 1: Write the failing tests** (`MemStore`; `fat` image tests where the FAT matters):

```rust
#[test] fn ids_parse_only_library_names() {
    assert_eq!(lib_id(FileName::new(Dir::Sounds, b"S0000012", b"A").unwrap()).map(LibId::get), Some(12));
    assert_eq!(lib_id(FileName::new(Dir::Sounds, b"S9000001", b"B").unwrap()).map(|i| i.is_factory()), Some(true));
    for (stem, ext) in [(&b"S000012"[..], &b"A"[..]), (b"BANK1", b"BNK"), (b"S0000012", b"C"), (b"S9000000", b"A")] {
        assert_eq!(lib_id(FileName::new(Dir::Sounds, stem, ext).unwrap()), None);
    }
    assert_eq!(lib_id(FileName::new(Dir::Projects, b"S0000012", b"A").unwrap()), None);
}
#[test] fn a_save_takes_top_plus_one_ignoring_factory_and_stock() {
    // card with S0000003.A, S9000001.A, BANK1.BNK in SOUNDS → saved id 4
}
#[test] fn a_save_past_the_top_takes_the_lowest_free() {
    // S8999999.A and S0000001.A present → id 2
}
#[test] fn next_user_id_scans_windows_and_runs_out() {
    let full = [0xFF; 512];
    assert_eq!(next_user_id(LibId::USER_MAX, 1, &full), NextId::Scan(1 + ID_WINDOW));
    let mut t = [0xFF; 512]; t[1] = 0b1111_1011;           // id base+10 free
    assert_eq!(next_user_id(LibId::USER_MAX, 1, &t), NextId::Id(LibId::new(11).unwrap()));
    assert_eq!(next_user_id(LibId::USER_MAX, LibId::USER_MAX - 7, &full), NextId::NoIds);
}
#[test] fn a_save_makes_sounds_only_then_and_loads_back() {
    // fresh card: no SOUNDS before; after save SOUNDS/S0000001.A exists,
    // generation 1, header name as given, peek stamp == the stamp passed,
    // load_ab with SoundDecoder bits_eq the Sound; no .B
}
#[test] fn a_full_card_leaves_the_directory_as_it_was() {
    // MemStore with a byte cap: Err(LibNote::Store(StoreError::Full)); listing of SOUNDS unchanged
}
#[test] fn a_cut_at_every_block_never_loads_wrong() {
    // the storage tests' torn-write harness over a save: each outcome is
    // no file, or a file whose peek or load_ab errors, or a bit-equal load
}
#[test] fn peek_reads_name_engine_and_stamp() {
    let p = peek_library(&first_64(&stamped_file), len).unwrap();
    assert_eq!((p.name.unwrap().as_str(), p.engine, p.stamp), ("BASS-1", Some(EngineType::Algo), Some(stamp)));
    assert_eq!(peek_library(&unstamped[..], unstamped.len() as u32).unwrap().stamp, None);  // 50 B file
    assert_eq!(peek_library(&[], 0), Err(FileError::Truncated));
}
```

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_store_test`
  Expected: compile errors: `lib_id`, `save_library_sound` and the rest are undefined.

- [ ] **Step 3: Implement** as in Interfaces.
  - The id search is pass 1 for `top`. Only when `top == USER_MAX` does it run window passes with a 512 B stack bitmap. A FAT directory holds at most 65 536 entries, so the lowest free id is at most 32 769 and needs no more than 9 windows.
  - `peek_library` follows `project::store::peek_side` for the header rules.
  - The fuzz target calls `peek_library(data, data.len() as u32)`.

- [ ] **Step 4: Run** the Step 2 command, plus `cargo +nightly fuzz build peek_library` in `chimera-core/fuzz`.
  Expected: PASS; the fuzz target builds.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src chimera-core/tests/library_store_test.rs chimera-core/fuzz Justfile
git commit -m "Library ids, the header peek, and saving a Sound to the card"
```

### Task 4: The listing

**Files:**
- Modify: `chimera-engine/src/project/library.rs`
- Test: `chimera-core/tests/library_store_test.rs`

**Interfaces:**
- Consumes: Task 3's `LibId`, `lib_id`, `peek_library`, `LibNote`.
- Produces:
  - `pub struct LibSeen { pub generation: Option<Generation>, pub name: Option<SoundName> }` (`Copy, PartialEq`).
  - `pub struct LibFile { pub id: LibId, pub vol: VolumeId, pub seen: LibSeen }` (`Copy, PartialEq`).
  - `pub struct LibEntry { pub file: LibFile, pub engine: Option<EngineType>, pub stamp: Option<BuildStamp>, pub len: u32, pub err: Option<FileError> }`.
  - `pub enum PageAt { Newest, Below(LibId), Above(LibId) }`.
  - `pub struct LibPage { entries: [Option<LibEntry>; MAX_LISTED], len: u8, pub newer: bool, pub older: bool, pub vol: VolumeId }`, with `pub fn entries(&self) -> &[Option<LibEntry>]`. Entries are user ids only, highest first.
  - `pub fn list_library<S: Store>(card: &mut Card, store: &mut S, at: PageAt) -> CardOut<Result<LibPage, LibNote>>`. The directory pass keeps the 48 ids nearest `at` (highest below, lowest above, displayed highest first), the sides it saw and their sizes. It peeks only the sides it saw (`read` with a `LIB_PEEK` sink; a size under 32 B is `Truncated` with no read). The newer readable side gives the name, engine and stamp, and a `NeedsNewerFirmware` side is the entry's `err` (`project::store::entry`'s rule). A missing `SOUNDS` is an empty page.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_hundred_sounds_page_48_48_4_newest_first() {
    let p1 = list(PageAt::Newest);
    assert_eq!(ids(&p1)[..3], [100, 99, 98]); assert_eq!(ids(&p1).len(), 48);
    assert!(!p1.newer && p1.older);
    let p2 = list(PageAt::Below(LibId::new(53).unwrap()));
    assert_eq!((ids(&p2)[0], ids(&p2).len(), p2.newer, p2.older), (52, 48, true, true));
    let p3 = list(PageAt::Below(LibId::new(5).unwrap()));
    assert_eq!((ids(&p3), p3.older), (vec![4, 3, 2, 1], false));
    assert_eq!(ids(&list(PageAt::Above(LibId::new(52).unwrap())))[47], 53);
}
#[test] fn factory_and_foreign_names_are_never_listed_or_opened() {
    // counting store: S9000001.A, BANK1.BNK, S000001.A present → never read; not listed
}
#[test] fn only_seen_sides_are_read() {
    // 10 Sounds with .A only: exactly 10 reads (counting store), none for .B
}
#[test] fn bad_sides_list_with_their_errors() {
    // zero-length .A → err Some(Truncated), name None; header version 2 → Some(NeedsNewerFirmware), name kept when the other side reads
}
#[test] fn no_sounds_dir_is_an_empty_page() { assert!(list(PageAt::Newest).entries().iter().all(Option::is_none)); }
#[test] fn no_card_is_its_store_error() { assert_eq!(list_err(ejected), LibNote::Store(StoreError::NoCard)); }
```

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_store_test`
  Expected: FAIL: `list_library` is undefined.

- [ ] **Step 3: Implement** as in Interfaces, with a bounded sorted array like `project::store::Ids`.

- [ ] **Step 4: Run** the Step 2 command.
  Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src/project/library.rs chimera-core/tests/library_store_test.rs
git commit -m "List the library newest first, 48 a page"
```

### Task 5: The load: `LibraryPart`, the scratch read and `Project::adopt`

**Files:**
- Modify: `chimera-engine/src/project/library.rs`, `chimera-engine/src/project/guard.rs:72-78` (sealed list; `impl Witnessed`, `impl Target`), `chimera-engine/src/project/mod.rs`, `chimera-engine/src/storage/library.rs` (`LibraryDecoder`)
- Test: `chimera-core/tests/library_load_test.rs`; one compile-fail doctest on `LibraryPart`

**Interfaces:**
- Consumes: Task 2's `SoundCheck::skips`; Task 4's `LibFile`.
- Produces:
  - `pub struct LibraryDecoder<'a> { check: SoundCheck, target: &'a mut Sound }` with `pub fn new(target: &'a mut Sound) -> Self`, `impl DecodeInPlace` (`finish` runs `SoundCheck::finish(Some(target))`), and `pub fn skips(&self) -> u16`.
  - `pub struct LibraryPart { pub part: PartId, pub file: LibFile }`. It is sealed with `impl Target`; `at_risk` and `witness` delegate to `PartSource { part, from: PartFrom::Init(EngineType::Algo) }` (both read only the Part). Doctest `compile_fail,E0308`: a `Confirmed<PartSource>` can't be passed to `adopt`.
  - `pub struct LibRead { pub skips: u16, pub source: Option<SourceRef>, pub held: bool }`. `held` means the pool already holds that source.
  - `pub fn read_library_sound<S: Store>(card: &mut Card, store: &mut S, p: &mut Project, file: LibFile) -> CardOut<Result<LibRead, LibNote>>`. It re-peeks both sides: no side is `Gone`; a newest generation or name ≠ `file.seen` is `Changed`; a volume ≠ `file.vol` is `Store(VolumeChanged(..))`. Then `load_ab_in_place(.., &mut LibraryDecoder::new(p.pool.scratch()))`. Clobbered scratch is harmless, because scratch is only staging.
  - `pub enum Staged { Read { r: SourceRef, len: usize }, Missing(SourceRef) }`.
  - `pub struct Adopted { pub slot: SlotId, pub deduped: bool }` and `pub enum AdoptError { Changed, Full(Option<PoolFull>) }`; `None` means every slot is taken.
  - `impl Project { pub fn library_needs(&self, staged: Option<SourceRef>) -> Result<Option<SlotId>, Option<PoolFull>> }`. `Ok(Some(slot))` is a dedupe hit (a slot whose `crc()` equals the scratch's `sound_crc`). `Ok(None)` means it fits. The cost counts the scratch Sound and the source (`source_cost(held_for_file_len(..))`, read from the Sound's `SourceRef`), when not held, against `pool.free()`, a free Sound slot, and a free source slot.
  - `impl Project { pub fn adopt(&mut self, c: Confirmed<LibraryPart>, staged: Option<Staged>) -> Result<Adopted, AdoptError> }`. In order:
    1. `c.holds(self)`, else `Changed`;
    2. `library_needs`, else `Full`;
    3. `source_commit(r, len)` for `Read`, or `source_absent(r)` for `Missing`;
    4. `store_scratch(first_free)` unless deduped;
    5. load the Part from the slot with a fresh `Origin::Slot`;
    6. `bump`.

    A refusal changes nothing. `replace_part` and `load_part` are untouched.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn adopt_stores_the_slot_and_the_part_reads_clean() {
    let a = adopt_from(&mut p, &file, None).unwrap();
    assert!(!a.deduped);
    assert_eq!(part_status(p.part(P1), p.pool()), PartStatus::Clean);
    assert!(p.pool().get(a.slot).unwrap().matches(&saved));
}
#[test] fn loading_the_same_sound_twice_reuses_its_slot() { assert!(second.deduped); assert_eq!(second.slot, first.slot); }
#[test] fn over_budget_changes_nothing() {
    // fill the arena; rev, used, sound_count before == after; Err(AdoptError::Full(Some(f))) with f.needs > f.free
}
#[test] fn a_full_source_table_refuses_up_front() { /* 32 sources held, a TAPE Sound with a new SourceRef: Full, source_count unchanged */ }
#[test] fn a_moved_witness_leaves_no_source_and_no_slot() {
    // read a TAPE library Sound, stage its source (source_staging + fill),
    // edit the Part's Sound (part.sound.params.out.volume = 0.1), adopt → Err(Changed);
    // source_count() and sound_count() unchanged
}
#[test] fn a_missing_source_loads_absent() { /* Staged::Missing(r): Ok; pool().absence(id).is_some() */ }
#[test] fn a_file_changed_or_gone_since_listed_refuses() {
    // list; rm the pair → read_library_sound: Err(LibNote::Gone)
    // list; save_library_sound again after rm reuses the id with another name → Err(LibNote::Changed)
}
#[test] fn a_corrupt_file_refuses_and_leaves_the_part() { /* flipped byte: Err(File(BadCrc)); part bits unchanged */ }
#[test] fn a_translated_file_bigger_than_its_estimate_refuses_cleanly() {
    // v1 Modal fixture (EXCITE adds BURST) with the pool's free set to its estimate: Full, nothing changed
}
```

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_load_test && cargo test -p chimera-engine --doc`
  Expected: FAIL: `adopt` and `LibraryPart` are undefined.

- [ ] **Step 3: Implement** as in Interfaces.

- [ ] **Step 4: Run** the Step 2 commands, plus `--test replace_guard_test --test project_model_test`.
  Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src chimera-core/tests/library_load_test.rs
git commit -m "Load a library Sound through the pool in one step"
```

### Task 6: The UI's jobs, prompt routing, NAMING and SAVE TO LIBRARY

**Files:**
- Modify: `chimera-core/src/ui/settings/job.rs` (`Job`, `card_work`, `card_pending`), `chimera-core/src/ui/settings/mod.rs` (`Ask`, `AskKind`, `NamingFor`), `chimera-core/src/ui/settings/replace.rs` (`LibraryAsk`), `chimera-core/src/ui/settings/tree.rs:106-112` (PART row), `chimera-core/src/ui/settings/part.rs` (`Act::SaveToLibrary`), `chimera-core/src/ui/mod.rs` (`UiState` fields, MIX + EDIT dispatch, toasts)
- Create: `chimera-core/src/ui/library.rs`: `LibState`, holding the cached page and the pending load.
- Test: `chimera-core/tests/library_ui_test.rs`

**Interfaces:**
- Consumes: Tasks 3–5.
- Produces:
  - `Job::LibraryList(PageAt)`, `Job::LibrarySave(SoundName)`, `Job::LibraryLoad(Confirmed<LibraryPart>)`.
  - `pub(crate) struct LibState { page: Option<LibPage>, load: Option<PendingLoad>, tab: Tab }`, with `pub enum Tab { Project, Library }`. Methods:
    - `pub fn busy(&self) -> bool`: a load is pending;
    - `pub fn forget(&mut self)`: drops the page;
    - `pub fn page(&self) -> Option<&LibPage>`.
  - `UiState::card_pending` includes `self.lib.busy()`. `card_work` runs `self.lib.work(..)` each pass, beside `self.src.work`.
  - `pub type LibraryAsk = Asked<LibraryPart, LibraryAnswer>;` and `Ask::LoadLibrary(LibraryAsk)`, with answers SAVE PART FIRST / REPLACE / CANCEL, as `ReplaceAnswer`'s.
  - `NamingFor::SaveToLibrary(PartId)`: `title()` is `SOUND NAME`, `crumb()` is `SAVE TO LIB`, pre-filled with the Part's Sound name.
  - The PART list gains a sixth row, `crumb("SAVE TO LIBRARY", "SAVE TO LIB", Kind::Act(Act::SaveToLibrary))`.
  - `pub fn forget_library(&mut self)` on `UiState`, for the console (Task 8).

  The job flow follows spec § Save and § Load.
  - **`LibrarySave`**: `save_library_sound(.., &about_page::THIS)`. Ok: toast `SAVED TO LIBRARY`, `forget`, queue `LibraryList(Newest)` with the cursor on row 0. Err: toast `LibNote::message`.
  - **`LibraryLoad`**: `read_library_sound`, then `library_needs`.
    - `Full` refuses into the browser band: `refuse(Some(f))`, the existing `POOL FULL` / `NEEDS …`.
    - An unheld source with a `.SRC` on the card starts SOURCE's `SourceLoad` into staging, one 4 KiB step a pass. MENU gives `CANCELLED`. A failed source step toasts its message, and nothing is adopted.
    - With no `.SRC` on the card, it adopts with `Staged::Missing`.
    - Then `adopt`; on Ok, go to the Part's pages.
    - One toast, by precedence: `SOURCE MISSING`, then `LOADED · SOME PARAMS SKIPPED` (skips > 0), then `LOADED TO SLOT n`.
  - **Prompt routing**: EDIT on a library row runs `ReplaceGuard::check(.., LibraryPart { part, file })`. Ok queues the load; otherwise `Ask::LoadLibrary`. REPLACE uses `anyway`. SAVE PART FIRST runs `save_first`'s save, then `save_then`. Each queues `Job::LibraryLoad`, never `replace`.

- [ ] **Step 1: Write the failing tests** (`UiState` on `MemStore`, as `project_store_test` drives `card_work`):

```rust
#[test] fn mix_edit_in_library_names_then_saves() {
    ui.set_library_tab(); press(Mix+Edit);
    assert_eq!(ui.naming_title(), Some("SOUND NAME"));
    press(Seq); work(&mut ui);
    assert_eq!(toast(&ui), "SAVED TO LIBRARY");
    assert_eq!(card_ids(), [1]);
    assert_eq!(ui.project_status(), before_status);              // project not marked
}
#[test] fn save_to_library_row_in_settings_part() { /* MENU, PART, EDIT on row 5 → NAMING crumb "SAVE TO LIB" */ }
#[test] fn edit_loads_into_the_part_and_the_project_reads_modified() {
    assert_eq!(toast(&ui), "LOADED TO SLOT 1"); assert_eq!(ui.loc(), Location::part_home(P1, engine));
}
#[test] fn an_edited_part_asks_and_replace_loads() { /* prompt kind LoadLibrary; REPLACE → loaded */ }
#[test] fn save_part_first_that_takes_the_room_refuses_and_keeps_the_save() {
    // arena sized so the Part's save fits and the library Sound then doesn't:
    // the Part's save is in the pool; refusal line "POOL FULL"; Part unchanged
}
#[test] fn a_midi_edit_during_the_source_read_refuses() {
    // TAPE library Sound, its .SRC on card; run one card_work pass (More),
    // edit the Part, finish: toast refused(Changed); source_count unchanged
}
#[test] fn menu_during_the_source_read_cancels() { /* toast "CANCELLED"; pool unchanged */ }
#[test] fn skipped_params_say_so() { /* file with an unknown id: toast "LOADED · SOME PARAMS SKIPPED" */ }
#[test] fn card_work_is_refused_while_a_load_is_pending() { assert!(ui.card_pending()); /* LOAD PROJECT queues nothing */ }
#[test] fn the_page_is_cached_until_a_save() { /* second tab entry: no store read (counting store) */ }
```

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_ui_test`
  Expected: compile errors: `Job::LibrarySave` and the rest are undefined.

- [ ] **Step 3: Implement** as in Interfaces.

- [ ] **Step 4: Run** `cargo test -p chimera-core --features chimera-hal/testkit`
  Expected: PASS. `settings_screen_test` is updated for the sixth PART row.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui chimera-core/tests
git commit -m "Save to and load from the library: jobs, prompts and NAMING"
```

### Task 7: The browser's LIBRARY tab

**Files:**
- Modify: `chimera-core/src/ui/browser.rs` (title, rows, hint lines), `chimera-core/src/ui/mod.rs:2075-2135` (B's turn, MIX + EDIT and MIX + MINUS by tab), `chimera-core/src/ui/nav.rs` (`Browse` holds the LIBRARY cursor too)
- Test: `chimera-core/tests/browser_test.rs`, `chimera-core/tests/dpad_nav_test.rs`, `chimera-core/tests/screen_golden_test.rs` (+ goldens)

**Interfaces:**
- Consumes: Task 6's `LibState`, `Tab`, `LibPage`.
- Produces:
  - `browser::draw` takes a `BrowserView` enum: `Project(&Pool)` or `Library(&LibState, &BuildStamp)`.
  - Title: `PROJECT  LIBRARY  B ◂▸` with `· P{n}` right-aligned.
  - Exact copy:
    - `pub const EMPTY_LIBRARY: &str = "NO SOUNDS YET";`
    - `pub const NEWER: &str = "‹ NEWER";` and `pub const OLDER: &str = "OLDER ›";`
    - `pub const SAVE_TO_LIBRARY: Gesture<'static> = Gesture("MIX + EDIT", "SAVES TO LIBRARY");`
    - `pub fn stamp_words(stamp: Option<BuildStamp>, this: &BuildStamp) -> FmtBuf`: `SAVED ON 0.1.0 02E440E2` (upper-cased, `+` when dirty), `SAVED ON THIS BUILD`, or `NO STAMP`.
  - LIBRARY row cost: `Cost::of(len − HEADER_LEN − TRAILER_LEN − 25 + 16)` (saturating; `Cost::of` rounds to the 16 B granule).
  - Line 3 draws with HINTS off.
  - MIX + MINUS in LIBRARY does nothing.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn b_turns_between_project_and_library() { turn(EncoderId::B, 1); assert_eq!(ui.lib_tab(), Tab::Library); turn(EncoderId::B, -1); assert_eq!(ui.lib_tab(), Tab::Project); }
#[test] fn mix_minus_in_library_clears_nothing() { /* slot 0 filled, LIBRARY cursor 0, MIX+MINUS: no prompt, pool unchanged */ }
#[test] fn stamp_words_forms() {
    let this = BuildStamp::new("0.1.0", "02e440e2", false);
    assert_eq!(stamp_words(Some(BuildStamp::new("0.1.0", "aabbccdd", true)), &this).as_str(), "SAVED ON 0.1.0 AABBCCDD+");
    assert_eq!(stamp_words(Some(this), &this).as_str(), "SAVED ON THIS BUILD");
    assert_eq!(stamp_words(None, &this).as_str(), "NO STAMP");
}
#[test] fn paging_rows_run_by_edit() { /* EDIT on "OLDER ›" queues Job::LibraryList(Below(last id)) */ }
#[test] fn the_new_hint_lines_fit_216_px() { assert!(hint::width(&[SAVE_TO_LIBRARY]) <= 216); /* and the widest stamp line */ }
```

  `dpad_nav_test` gains B's turn, MIX + EDIT in LIBRARY (opens NAMING), and MIX + MINUS in LIBRARY (no-op). The screen goldens are both tabs with `P1`, `NO SOUNDS YET`, `NO CARD`, a greyed `FILE IS CUT SHORT` row, both paging rows, `WON'T FIT`, line 3's three forms, and NAMING `SAVE TO LIB`. Generate them with `just screens` and inspect each PNG.

- [ ] **Step 2: Run them and see them fail**

  Run: `cargo test -p chimera-core --features chimera-hal/testkit --test browser_test --test dpad_nav_test --test screen_golden_test`
  Expected: FAIL.

- [ ] **Step 3: Implement** as in Interfaces.

- [ ] **Step 4: Run** the Step 2 command.
  Expected: PASS. Every golden was looked at.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui chimera-core/tests
git commit -m "Sound browser: a LIBRARY tab on B, with each Sound's build"
```

### Task 8: Console verbs for library files

**Files:**
- Modify: `chimera-core/src/console/put.rs` (`LsArg::Sounds`, `PutTo::Sound`, the exists rule, `--over`), `chimera-core/src/console/mod.rs:126-140` (table rows `rm`, `get`, under `card-put`), `chimera-core/src/console/answer.rs:190-205` (dispatch, `ERR busy`, cache drop), `Unit` trait (`fn forget_library(&mut self) {}`), `chimera-board/src/lib.rs` (its `Unit` impl calls `UiState::forget_library`)
- Create: `chimera-core/src/console/sound_file.rs` (`rm`, `get`)
- Modify: `tools/chimera-usb.py` (`put` of a Sound takes `ls sounds`' top user id + 1), `tools/test_chimera_usb.py`
- Test: `chimera-core/tests/console_parse_test.rs`, `chimera-fat/tests/console_put_test.rs`, `chimera-core/tests/console_answer_test.rs`

**Interfaces:**
- Consumes: Task 3's `lib_id`, `library_file`, `LibId`; ADR 0045's `delete_ab`, `load_ab`/`pick`.
- Produces:
  - `LsArg::Sounds` (`ls sounds`).
  - `PutTo::Sound { id: LibId, over: bool }`, parsed from `/CHIMERA/SOUNDS/S#######.A|B`, any case, with an optional trailing `--over`. Both user and factory ids are accepted.
  - `pub struct SoundArg(LibId)`: `/CHIMERA/SOUNDS/S#######`, any case, no extension.
  - `Request::Rm(SoundArg)` and `Request::Get(SoundArg)`.
  - `pub fn rm<F: Files>(arg: SoundArg, f: &mut F, out: &mut impl Out) -> Result<(), Stalled>`: `ERR rm: factory` for `S9…`, `ERR rm: not found` with neither side, else `delete_ab`, then `OK`.
  - `pub fn get<F: Files, P: Out>(arg: SoundArg, f: &mut F, out: &mut P) -> Result<(), Stalled>`. It answers `GET <len>`, then base64 lines of 48 bytes (put's line size), then `CRC <crc32 hex>`, then `OK`. Both sides torn answers `ERR get: <FileError message>`.
  - A Sound `put` onto an id with either side present, without `--over`, answers `ERR put: exists` and writes nothing.
  - Every card verb answers `ERR busy` when `unit.ui().card_pending()`.
  - A successful Sound `put` or `rm` calls `unit.forget_library()`.

- [ ] **Step 1: Write the failing tests**

```rust
// console_parse_test.rs: parse_line(l) = feed(&mut Console::new(), l + b"\n").pop().unwrap()
#[test] fn sound_paths_parse_and_others_refuse() {
    assert!(matches!(parse_line(b"put /chimera/sounds/s0000012.a 120 deadbeef"), Ok(Request::Put(PutArg { to: PutTo::Sound { over: false, .. }, .. }))));
    assert!(matches!(parse_line(b"put /CHIMERA/SOUNDS/S0000012.A 120 deadbeef --over"), Ok(Request::Put(PutArg { to: PutTo::Sound { over: true, .. }, .. }))));
    for bad in [&b"rm /CHIMERA/SOUNDS/../PROJECTS/P0000001"[..], b"rm //CHIMERA/SOUNDS/S0000001", b"rm /CHIMERA/PROJECTS/P0000001", b"rm SOUNDS/S0000001", b"rm /CHIMERA/SOUNDS/S000001", b"get /CHIMERA/SOUNDS/S0000001.A"] {
        assert!(matches!(parse_line(bad), Err(Refusal::Arguments(_))), "{bad:?}");
    }
    assert!(matches!(parse_line(b"ls sounds"), Ok(Request::Ls(LsArg::Sounds))));
}
// console_put_test.rs (fat image + MemStore)
#[test] fn put_onto_an_existing_id_refuses_unless_over() { assert_eq!(answer, "ERR put: exists\n"); /* .A bytes unchanged */ }
#[test] fn put_over_replaces_the_unit_way() { /* .B written at generation 2; load picks it */ }
#[test] fn rm_deletes_pairs_torn_ones_and_refuses_factory_and_missing() {
    // pair → OK, both gone; torn .A only → OK; missing → "ERR rm: not found"; S9000001 → "ERR rm: factory", file kept
}
#[test] fn get_streams_the_picked_side() { /* decoded base64 == .A bytes; CRC line matches Crc32 */ }
#[test] fn get_of_a_torn_pair_says_why() { /* both sides torn → starts "ERR get: FILE " */ }
#[test] fn ls_sounds_lists_the_library() { /* "S0000001.A 58\nOK\n" */ }
// console_answer_test.rs
#[test] fn card_verbs_answer_busy_while_a_library_load_is_pending() {
    for line in ["ls sounds", "put /CHIMERA/SOUNDS/S0000002.A 10 0", "rm /CHIMERA/SOUNDS/S0000001", "get /CHIMERA/SOUNDS/S0000001"] {
        assert_eq!(answer(line), "ERR busy\n");
    }
}
#[test] fn put_and_rm_drop_the_library_page() { /* unit.forgotten == 1 after each */ }
```

- [ ] **Step 2: Run them and see them fail**

  Run: `cargo test -p chimera-core --features chimera-hal/testkit --test console_parse_test --test console_answer_test && cargo test -p chimera-fat --features chimera-hal/testkit --test console_put_test`
  Expected: FAIL.

- [ ] **Step 3: Implement** as in Interfaces. `put` of a Sound reuses the project path's A/B save and the kind's check (`SoundCheck`). `help` lists `rm` and `get`. Extend the host tool's `put` for Sounds.

- [ ] **Step 4: Run** the Step 2 commands, plus `python3 -m unittest discover -s tools -p 'test_*.py'` and `cargo test -p chimera-core --features fault-inject --test console_fault_test --test console_answer_test`.
  Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/console chimera-board/src/lib.rs chimera-core/tests chimera-fat/tests tools
git commit -m "Console: ls, put, rm and get for library Sounds"
```

### Task 9: Stock files, ADR 0119, the budget report and the full check

**Files:**
- Modify: `chimera-fat/tests/stock_files_test.rs` (library save, console `put`/`rm` of a Sound, listing every page, load)
- Create: `docs/adr/0119-a-sound-library-on-the-card.md` (template `docs/adr/0000-template.md`, Status Proposed); modify `docs/adr/README.md`
- Test: `chimera-fat/tests/stock_files_test.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: ADR 0119, holding the spec's § ADRs bullets. It amends 0081 and 0043 (both Proposed) and supersedes 0086 in part (its browser hint rows). README rows: 0119 is added, and the 0081, 0043 and 0086 rows each gain "amended by / superseded in part by [0119]".

- [ ] **Step 1: Write the failing test**

```rust
#[test] #[ignore] fn library_paths_never_touch_stock_files() {
    // stock /pfm3 tree + /CHIMERA; save_library_sound (cut and torn at every block, as the
    // Sound-save case), console put + rm of S0000002, list_library every page, read + adopt;
    // assert every stock entry, FAT link and data byte unchanged, fsck.fat passes,
    // and every written directory entry is under /CHIMERA/SOUNDS
}
```

- [ ] **Step 2: Run it and see it fail**

  Run: `cargo test -p chimera-fat --features chimera-hal/testkit --test stock_files_test -- --ignored library_paths_never_touch_stock_files`
  Expected: FAIL (the test is new and its helpers don't exist yet).

- [ ] **Step 3: Implement** the test's helpers, then write ADR 0119 and the README rows.

- [ ] **Step 4: Run the full check**

  Run: `touch chimera-engine/src/lib.rs && just check && just stack-check`
  Expected: both green. Record the flash and AXI deltas of `chimera-stm32` release against `origin/main` in one line in the ADR's Consequences. Expect a few KB of flash and about 2.5 KB of AXI. Escalate only if free flash drops under about 40 KB.

- [ ] **Step 5: Commit**

```bash
git add chimera-fat/tests/stock_files_test.rs docs/adr/0119-a-sound-library-on-the-card.md docs/adr/README.md
git commit -m "ADR 0119 and the stock-files check for the library"
```

---

**On the chip, at ship (not a task):**
- measure one page's listing with 500 files in `SOUNDS/`; over 1.5 s, file `SNDINDEX` as a GitHub issue;
- the owner's UAT: save on one build, flash the next, load it back by ear.
