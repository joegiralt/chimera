# Sound Library Implementation Plan

**Goal:** Save a Part's Sound to `/CHIMERA/SOUNDS` with a build stamp, then browse that library from any project and load a Sound back into the Part through the pool. The USB console gets verbs to manage the files.

**Architecture:**
- **Format** (`chimera-engine/src/storage/library.rs`): pure. Ids, the `Build` record, the lenient peek, and an in-place decoder.
- **Card and project** (`chimera-engine/src/project/library.rs`): save, list and read run inside `Card::run`. Loads use a third sealed replace target, `LibraryPart`, and commit through `Project::adopt`, which stores the source, the slot and the Part together, after the witness and budget checks.
- **UI** (`chimera-core`): queues card jobs as SAVE TO PROJ and LOAD PROJECT do. A TAPE source is streamed into the arena's staging without publishing. The Sound browser gains a PROJECT/LIBRARY tab on encoder B. The console gains `ls sounds`, `put`, `rm` and `get`.

**Tech Stack:** Rust `no_std`, `chimera-hal` `Store`, `MemStore` and `DirStore` for tests, `chimera_core::project::test_support` (`FullOnWrite`), the `chimera-fat` card-image tests (`power_cut_test`, `stock_files_test`, dosfstools), `cargo fuzz`.

**Spec:** `docs/superpowers/specs/2026-10-09-sound-library-design.md` (owner-approved 2026-10-10; both review rounds applied)

## Global Constraints

- No heap allocation and no blocking in the audio callback; the library adds nothing to the audio side.
- Card I/O stays off the audio path. It runs only in `UiState::card_work` and in the console's answer, on the UI loop, behind BUSY.
- Writes only under `/CHIMERA/SOUNDS`, to `S#######.A|B`. The library makes `SOUNDS` only on the first save, never at mount.
- Stock PreenFM3 files are never opened or written (#405): `chimera-fat/tests/stock_files_test.rs` stays green.
- No `unsafe` without `// SAFETY:`. No whole `Sound` on a job's stack; `just stack-check` passes at the end of Tasks 4, 5 and 9.
- `0x001C` is `RecordTag::Build`, not critical, written second, after `Engine`.
- **`Build` is skipped in every context of every file kind.** It never refuses a file.
- User ids `S0000001`–`S8999999`; factory ids `S9000001`–`S9999999`, never allocated by a save.
- **No card job, project load or SOURCE card-row load starts while a library load is pending,** and no library load starts while a SOURCE load is. MENU still cancels.
- A TAPE source enters the pool only inside `Project::adopt`.
- UI copy is exact:
  - saves and lists: `SAVED TO LIBRARY`, `NO SOUND IDS LEFT`, `NO SOUNDS YET`, `‹ NEWER`, `OLDER ›`;
  - loads: `FILE CHANGED`, `FILE IS GONE`, `LOADED TO SLOT 12`, `LOADED · SOME PARAMS SKIPPED`, `SOURCE MISSING`, `CANCELLED`, `PART CHANGED`, `POOL FULL`, `NEEDS 1.2K, 0.4K FREE`, `WON'T FIT`, `NEEDS NEWER FIRMWARE`;
  - hints and stamps: `EDIT LOADS · +0.3K`, `MIX + EDIT SAVES TO LIBRARY`, `SAVED ON …`, `SAVED ON THIS BUILD`, `NO STAMP`;
  - NAMING and SETTINGS: `SOUND NAME`, `SAVE TO LIBRARY`, `SAVE TO LIB`;
  - console: `ERR put: exists`, `ERR rm: factory`, `ERR rm: not found`, `ERR busy`.
- Touch `chimera-engine/src/lib.rs` (`touch chimera-engine/src/lib.rs`) before running `chimera-core` tests after an engine change, so the re-export rebuilds.
- Commit messages are terse with no type prefix.
- `just check` is green at the end of the last task.
- Report flash and AXI deltas in one line in the last task (flash rule). Don't shave bytes.

## Review Focus

1. **A Sound saved on a build that knows more params** must load and say `SOME PARAMS SKIPPED`, never refuse. Task 2 pins an unknown id, block and record, and pins that a migrated or translated param counts 0.
2. **A Part edited by MIDI CC while a TAPE source streams in** must refuse at `adopt` (`PART CHANGED`) with no source and no slot added. Task 5 tests it at the project level, Task 6 through the job.
3. **A second EDIT, LOAD PROJECT or SOURCE row during a streaming load** must queue nothing, and the first file's Sound must be the one adopted. Task 6.
4. **A card edited over USB between listing and EDIT** must refuse `FILE CHANGED` / `FILE IS GONE`, never load another Sound. Task 5 at the card level, Task 8 through the console.
5. **The console poking outside `SOUNDS/`** (`..`, `//`, `/CHIMERA/PROJECTS/…`, `S9…` with `rm`), or while a load is pending, must refuse without touching the card. Task 8.

---

### Task 1: The `Build` record and the stamped encoder

**Files:**
- Create: `chimera-engine/src/storage/library.rs`
- Modify: `chimera-engine/src/storage/mod.rs`; `chimera-engine/src/storage/record.rs:70-124` (tag tables); `chimera-engine/src/storage/sound.rs` (`encode_sound`, `once_bit`, `SoundCheck::step`); `chimera-engine/src/project/codec.rs:461-471` (project contexts); `chimera-core/build.rs:27` (hash cut to 8); `chimera-core/src/ui/about_page.rs` (`BuildStamp::THIS`)
- Test: `chimera-core/tests/library_format_test.rs`; `chimera-core/tests/storage_frame_test.rs` (tag golden); `chimera-core/tests/codec_compat_test.rs` (`library_1.snd`)

**Interfaces:**
- Produces, in `chimera_engine::storage` (re-exported as `chimera_core::storage`):
  - `RecordTag::Build`, where `RecordTag::Build.code() == 0x001C`.
  - `pub const BUILD_LEN: usize = 21;`
  - `pub struct BuildStamp { version: [u8; 12], hash: [u8; 8], dirty: bool }`, `Clone, Copy, Debug, PartialEq, Eq`. Methods:
    - `pub const fn new(version: &str, hash: &str, dirty: bool) -> BuildStamp`: panics in const context past 12 or 8 bytes.
    - `pub fn to_bytes(&self) -> [u8; BUILD_LEN]`.
    - `pub fn parse(p: &[u8]) -> Option<BuildStamp>`: `None` under 21 B; bytes past 21 ignored; non-printable bytes become `?`; flag bits other than 0 ignored.
    - `pub fn version(&self) -> &str`, `pub fn hash(&self) -> &str`, `pub fn dirty(&self) -> bool`.
    - `pub fn is_this_build(&self, this: &BuildStamp) -> bool`: equal hashes, neither dirty, hash neither `dev` nor `unknown`.
    - `impl core::fmt::Display`: `0.1.0 02e440e2`, with `+` when dirty.
  - `pub fn encode_library_sound(s: &Sound, stamp: &BuildStamp, w: &mut RecordWriter<'_>) -> Result<(), StoreError>`: `Engine`, `Build`, then `encode_sound_body`.
  - `pub(crate) fn encode_sound_body(s: &Sound, w: &mut RecordWriter<'_>) -> Result<(), StoreError>` in `sound.rs`. `encode_sound` is `Engine` + `encode_sound_body`, byte-identical to today's.
- Produces in `chimera_core::ui::about_page`: `pub const THIS: BuildStamp = BuildStamp::new(VERSION, HASH, DIRTY);` and `const _: () = assert!(VERSION.len() <= 12);`.
- `build.rs` emits at most the first 8 characters of `git rev-parse --short=8`, which may print more when 8 are ambiguous.

- [ ] **Step 1: Write the failing tests** in `library_format_test.rs`. `stamped(&Sound) -> Vec<u8>` is a helper: `write_file` with a `Sound` header and `encode_library_sound`. `insert_record(bytes, at, tag, payload) -> Vec<u8>` re-frames the file and re-CRCs it, like `codec_compat_test::with_record`.

```rust
const ST: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);

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
    let mut long = ST.to_bytes().to_vec();
    long.extend_from_slice(&[0xAA; 9]);
    assert_eq!(BuildStamp::parse(&long).unwrap().hash(), "02e440e2");
    assert_eq!(BuildStamp::parse(&long[..3]), None);
    let mut odd = long.clone(); odd[0] = 0x07;
    assert_eq!(BuildStamp::parse(&odd).unwrap().version(), "?.1.0");
}
#[test] fn this_build_needs_a_clean_real_hash() {
    assert!(ST.is_this_build(&ST));
    assert!(!BuildStamp::new("0.1.0", "02e440e2", true).is_this_build(&ST));
    assert!(!ST.is_this_build(&BuildStamp::new("0.1.0", "02e440e2", true)));
    let dev = BuildStamp::new("0.1.0", "dev", false); assert!(!dev.is_this_build(&dev));
    let unk = BuildStamp::new("0.1.0", "unknown", false); assert!(!unk.is_this_build(&unk));
}
#[test] fn plain_encode_is_unchanged() {
    for i in 0..FACTORY_LEN { assert_eq!(encoded(&factory_sound(i)), fixture_bytes_of(i)); } // factory_i.snd bodies
}
#[test] fn a_stamped_sound_decodes_bit_equal() {
    for s in sounds() { assert!(decode(&stamped(&s)).unwrap().bits_eq(&s)); }       // factory + INIT per engine
}
#[test] fn odd_builds_load() {
    let s = factory_sound(0); let f = stamped(&s);
    let after_routes = record_end(&f, RecordTag::Routes);
    assert!(decode(&insert_record(&f, after_routes, 0x001C, &ST.to_bytes())).unwrap().bits_eq(&s)); // a repeat
    assert!(decode(&replace_record(&f, 1, 0x001C, &[0xAA; 30])).unwrap().bits_eq(&s));               // 30 B
    assert!(decode(&replace_record(&f, 1, 0x001C, &[1, 2, 3])).unwrap().bits_eq(&s));                // 3 B
}
#[test] fn build_before_engine_is_corrupt() {
    let f = insert_record(&stamped(&factory_sound(0)), HEADER_LEN, 0x001C, &ST.to_bytes());
    assert_eq!(decode(&f).unwrap_err(), LoadError::File(FileError::Corrupt));
}
#[test] fn build_in_a_project_slot_loads() {
    let p = fixture("project.prj");
    let f = insert_record(&p, record_end(&p, RecordTag::Engine), 0x001C, &ST.to_bytes());
    assert!(load_project_bytes(&f).is_ok());
}
#[test] fn build_at_project_top_level_loads() {
    let p = fixture("project.prj");
    for at in [HEADER_LEN, record_end(&p, RecordTag::Fx)] {                       // context None, then Fx
        assert!(load_project_bytes(&insert_record(&p, at, 0x001C, &ST.to_bytes())).is_ok(), "{at}");
    }
}
```

  Add `RecordTag::Build => 0x001C` to the tag golden in `storage_frame_test.rs`. Add a `library_1.snd` fixture to `codec_compat_test::write_v1_fixtures`: factory Sound 0, `encode_library_sound`, stamp `BuildStamp::new("0.1.0", "dev", false)`, generation 1. It goes into `v1_fixtures_render_identically`'s list. `unknown_non_critical_skipped`, the proof for older builds, stays unchanged.

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_format_test --test storage_frame_test`
  Expected: compile errors: `BuildStamp`, `RecordTag::Build` and `encode_library_sound` are undefined.

- [ ] **Step 3: Implement**
  - `SoundCheck::step` returns `Ok(())` for `RecordTag::Build` right after the "Engine first, and once" check (so a `Build` before `Engine` stays `Corrupt`, as any record there does) and before `once`. `once_bit(Build)` is `0` and is never consulted. `seen` stays `u8`.
  - In `ProjectCheck`/`ProjectDecoder`, `ReadTag::Known(RecordTag::Build)` is `Ok(())` in the `None`, `Fx` and `Orbit` contexts. `Slot` and `Part` go through `SoundCheck`.
  - Write the fixture with `CHIMERA_WRITE_FIXTURES=1` as `write_v1_fixtures` documents.

- [ ] **Step 4: Run the tests and see them pass**

  Run: the Step 2 command, plus `--test codec_compat_test --test sound_codec_test --test storage_ab_test --test project_store_test`
  Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src/storage chimera-engine/src/project/codec.rs chimera-core/build.rs chimera-core/src/ui/about_page.rs chimera-core/tests/library_format_test.rs chimera-core/tests/storage_frame_test.rs chimera-core/tests/codec_compat_test.rs chimera-core/tests/fixtures/v1/library_1.snd
git commit -m "Build record: a library Sound carries the build it was saved on"
```

### Task 2: The skip count

**Files:**
- Modify: `chimera-engine/src/storage/block_codec.rs:67-144` (`decode_block`); `chimera-engine/src/storage/sound.rs` (`SoundCheck`); the other `decode_block` callers (`orbit/records.rs:215`, `project/codec.rs:484,661`, `storage/system.rs:126`, `block_codec.rs:229`)
- Test: `chimera-core/tests/library_format_test.rs`

**Interfaces:**
- Consumes: Task 1's `encode_library_sound`.
- Produces:
  - `pub fn decode_block(...) -> Result<u16, FileError>`, same parameters; the `u16` is the entries lost. Other callers drop it.
  - `impl SoundCheck { pub fn skips(&self) -> u16 }`. It counts only when `staged` is `Some` (pass 2) and resets at `Event::Header`.

  The count:
  - in `decode_block`, counted only with a target: an unknown block code not in `RETIRED_BLOCKS`; an unknown id with no migration and not in `RETIRED`; an enum code `set_enum_code` refuses that isn't in `RETIRED_CODES`;
  - in `SoundCheck`: an unknown non-critical record after `Engine`; a route whose `ModSource::from_disk_code` or `addr_of` is `None` (`finish`); a registry or ModDests entry whose `addr_of` is `None`.

  Not counted: a block this engine doesn't hold, and `Build`.

- [ ] **Step 1: Write the failing tests.** `decode_skips(bytes) -> u16` is `check_file` + pass 2 through a `SoundCheck` stepping a staged `Sound`. `with_block_entry(f, block_code, id, value)` adds an entry to that block's record, making the record if absent.

```rust
#[test] fn skips_count_what_was_lost() {
    let f = stamped(&Sound::init(EngineType::Algo));
    let f = with_block_entry(&f, BlockRef::Out.disk_code().unwrap(), 250, 1.0);
    let f = with_block_entry(&f, 200, 1, 1.0);
    let f = insert_record(&f, record_end(&f, RecordTag::Routes), 0x0070, &[0; 4]);
    assert_eq!(decode_skips(&f), 3);
}
#[test] fn every_fixture_and_factory_sound_skips_nothing() {
    for (name, s) in all_v1_and_factory() { assert_eq!(decode_skips(&stamped(&s)), 0, "{name}"); }
}
#[test] fn translated_v1_modal_skips_nothing() { assert_eq!(decode_skips(&fixture("init_modal.snd")), 0); }
#[test] fn pass_one_alone_counts_nothing() {
    let mut c = SoundCheck::new();
    feed_pass_one(&mut c, &three_skip_file());
    assert_eq!(c.skips(), 0);
}
```

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_format_test`
  Expected: FAIL: `skips` is undefined.

- [ ] **Step 3: Implement** as in Interfaces.

- [ ] **Step 4: Run** `touch chimera-engine/src/lib.rs && cargo test -p chimera-core -p chimera-engine --features chimera-hal/testkit`
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
- Create: `chimera-core/fuzz/fuzz_targets/peek_library.rs`; modify `chimera-core/fuzz/Cargo.toml` (bin entry) and the `Justfile` `fuzz` recipe (run `peek_library` too)
- Test: `chimera-core/tests/library_store_test.rs`; `chimera-fat/tests/power_cut_test.rs` (with `common/ab.rs`)

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
  - `pub fn next_user_id(top: u32, window_base: u32, taken: &[u8; 512]) -> NextId`, with `pub enum NextId { Id(LibId), Scan(u32), NoIds }` (`Debug, PartialEq`). It returns `top + 1` when `top < USER_MAX`. Otherwise it returns the lowest clear bit of the window (bit *i* = id `window_base + i`), or `Scan(window_base + ID_WINDOW)`, or `NoIds` once the window passes `USER_MAX`.
  - `pub const LIB_PEEK: usize = 64;`
  - `pub struct Peek { pub generation: Generation, pub name: Option<SoundName>, pub engine: Option<EngineType>, pub stamp: Option<BuildStamp> }` (`Debug, PartialEq`).
  - `pub fn peek_library(b: &[u8], file_len: u32) -> Result<Peek, FileError>`. Under 32 B by `file_len` is `Truncated`. The header follows `peek_header`'s rules, and a kind other than `Sound` is `WrongKind`. An unknown engine code is `NeedsNewerFirmware`. `stamp` is `BuildStamp::parse` of the second record's payload when its tag is `0x001C`. It never panics.
- Produces in `project::library`:
  - `pub enum LibNote { Store(StoreError), File(FileError), NoIds, Changed, Gone }` (`Debug, PartialEq`), with `pub fn message(self) -> &'static str`: the inner message, `NO SOUND IDS LEFT`, `FILE CHANGED`, `FILE IS GONE`.
  - `pub fn save_library_sound<S: Store>(card: &mut Card, store: &mut S, s: &Sound, name: SoundName, stamp: &BuildStamp) -> CardOut<Result<LibId, LibNote>>`. Inside one `Card::run`:
    1. `make_dir` `Chimera`, then `Sounds`;
    2. one directory pass for `top`, plus window passes (512 B stack bitmap) only when `top == USER_MAX`, looping until `Id` or `NoIds`;
    3. `save_ab(.., &mut SoundCheck::new(), Some(name), body)`, the body calling `encode_library_sound`;
    4. `check_file` on `.A`, deleting `.A` on any error after the write began.

- [ ] **Step 1: Write the failing tests** (`library_store_test.rs`, `MemStore` unless noted):

```rust
#[test] fn ids_parse_only_library_names() {
    let n = |d, s: &[u8], e: &[u8]| lib_id(FileName::new(d, s, e).unwrap());
    assert_eq!(n(Dir::Sounds, b"S0000012", b"A").map(LibId::get), Some(12));
    assert_eq!(n(Dir::Sounds, b"S9000001", b"B").map(LibId::is_factory), Some(true));
    for (s, e) in [(&b"S000012"[..], &b"A"[..]), (b"BANK1", b"BNK"), (b"S0000012", b"C"), (b"S9000000", b"A")] {
        assert_eq!(n(Dir::Sounds, s, e), None);
    }
    assert_eq!(n(Dir::Projects, b"S0000012", b"A"), None);
}
#[test] fn a_save_takes_top_plus_one_ignoring_factory_and_stock() {
    let mut st = card_with(&["S0000003.A", "S9000001.A", "BANK1.BNK"]);
    assert_eq!(save(&mut st).unwrap().get(), 4);
}
#[test] fn a_save_past_the_top_takes_the_lowest_free() {
    let mut st = card_with(&["S8999999.A", "S0000001.A"]);
    assert_eq!(save(&mut st).unwrap().get(), 2);
}
#[test] fn next_user_id_scans_windows_and_runs_out() {
    let full = [0xFF; 512];
    assert_eq!(next_user_id(LibId::USER_MAX, 1, &full), NextId::Scan(1 + ID_WINDOW));
    let mut t = [0xFF; 512]; t[1] = 0b1111_1011;
    assert_eq!(next_user_id(LibId::USER_MAX, 1, &t), NextId::Id(LibId::new(11).unwrap()));
    assert_eq!(next_user_id(LibId::USER_MAX, LibId::USER_MAX - 7, &full), NextId::NoIds);
}
#[test] fn a_save_makes_sounds_only_then_and_loads_back() {
    let mut st = MemStore::new(1); let v = st.mount().unwrap(); st.make_dir(v, Dir::Chimera).unwrap();
    assert_eq!(listing(&mut st, Dir::Sounds), Err(StoreError::NotFound));
    let id = save_named(&mut st, &factory_sound(0), "BASS-1").unwrap();
    assert_eq!(listing(&mut st, Dir::Sounds).unwrap(), vec![("S0000001.A".into(), len_of(&st, "S0000001.A"))]);
    let p = peek_library(&first(&st, "S0000001.A", LIB_PEEK), len_of(&st, "S0000001.A")).unwrap();
    assert_eq!((p.generation, p.name.unwrap().as_str(), p.stamp), (Generation::FIRST, "BASS-1", Some(ST)));
    assert!(load_sound(&mut st, id).bits_eq(&factory_sound(0)));
}
#[test] fn a_full_card_leaves_the_directory_as_it_was() {
    let mut st = FullOnWrite(card_with(&["S0000001.A"]), false);
    let before = listing(&mut st, Dir::Sounds).unwrap();
    assert_eq!(save(&mut st).unwrap_err(), LibNote::Store(StoreError::Full));
    assert_eq!(listing(&mut st, Dir::Sounds).unwrap(), before);
}
#[test] fn peek_reads_name_engine_and_stamp() {
    let f = stamped_named(&factory_sound(0), "BASS-1");
    let p = peek_library(&f[..LIB_PEEK], f.len() as u32).unwrap();
    assert_eq!((p.name.unwrap().as_str(), p.engine, p.stamp), ("BASS-1", Some(EngineType::Algo), Some(ST)));
    let u = unstamped(&Sound::neutral(EngineType::Algo));          // 50 B
    assert_eq!(peek_library(&u, u.len() as u32).unwrap().stamp, None);
    assert_eq!(peek_library(&[], 0), Err(FileError::Truncated));
}
```

  In `chimera-fat/tests/power_cut_test.rs`, add `library_save_cut_at_every_block_never_loads_wrong`. Using `common/ab.rs`'s cut and tear harness, it cuts and tears a `save_library_sound` at every block, and asserts each outcome is one of: no `S0000001.A`; a file whose peek or `load_ab` errors; or a load `bits_eq` the Sound.

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_store_test && cargo test -p chimera-fat --features chimera-hal/testkit --test power_cut_test library_save`
  Expected: compile errors: `lib_id`, `save_library_sound` and the rest are undefined.

- [ ] **Step 3: Implement** as in Interfaces.
  - `peek_library` follows `project::store::peek_side`'s header rules.
  - The fuzz target calls `peek_library(data, data.len() as u32)`.

- [ ] **Step 4: Run** the Step 2 commands, plus `cargo +nightly fuzz build peek_library` in `chimera-core/fuzz`.
  Expected: PASS; the target builds.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src chimera-core/tests/library_store_test.rs chimera-fat/tests chimera-core/fuzz Justfile
git commit -m "Library ids, the header peek, and saving a Sound to the card"
```

### Task 4: The listing

**Files:**
- Modify: `chimera-engine/src/project/library.rs`
- Test: `chimera-core/tests/library_store_test.rs`

**Interfaces:**
- Consumes: Task 3's `LibId`, `lib_id`, `peek_library`, `LibNote`.
- Produces:
  - `pub struct LibSeen { pub generation: Option<Generation>, pub name: Option<SoundName> }` (`Clone, Copy, Debug, PartialEq`).
  - `pub struct LibFile { pub id: LibId, pub vol: VolumeId, pub seen: LibSeen }` (`Clone, Copy, Debug, PartialEq`).
  - `pub struct LibEntry { pub file: LibFile, pub engine: Option<EngineType>, pub stamp: Option<BuildStamp>, pub len: u32, pub err: Option<FileError> }` (`Clone, Copy, Debug`).
  - `pub enum PageAt { Newest, Below(LibId), Above(LibId) }` (`Clone, Copy, Debug, PartialEq`).
  - `pub struct LibPage { entries: [Option<LibEntry>; MAX_LISTED], len: u8, pub newer: bool, pub older: bool, pub vol: Option<VolumeId> }`, with:
    - `pub const EMPTY: LibPage`;
    - `pub fn entries(&self) -> &[Option<LibEntry>]`, user ids only, highest first;
    - `pub fn position(&self, id: LibId) -> Option<usize>`.
  - `pub fn list_library<S: Store>(card: &mut Card, store: &mut S, at: PageAt, out: &mut LibPage) -> CardOut<Result<(), LibNote>>`. It fills `out` in place; the page never goes on the stack by value.
    - The directory pass keeps the 48 ids nearest `at` (highest below, or lowest above, displayed highest first), the sides it saw and their sizes.
    - It peeks only those sides. A size under 32 B is `Truncated` with no read.
    - The newer readable side gives the name, engine and stamp. A `NeedsNewerFirmware` side is the entry's `err` (`project::store::entry`'s rule).
    - A missing `SOUNDS` is an empty page.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn a_hundred_sounds_page_48_48_4_newest_first() {
    let mut st = card_with_sounds(1..=100);
    let p1 = list(&mut st, PageAt::Newest);
    assert_eq!((ids(&p1)[..3].to_vec(), ids(&p1).len(), p1.newer, p1.older), (vec![100, 99, 98], 48, false, true));
    let p2 = list(&mut st, PageAt::Below(LibId::new(53).unwrap()));
    assert_eq!((ids(&p2)[0], ids(&p2).len(), p2.newer, p2.older), (52, 48, true, true));
    let p3 = list(&mut st, PageAt::Below(LibId::new(5).unwrap()));
    assert_eq!((ids(&p3), p3.older), (vec![4, 3, 2, 1], false));
    assert_eq!(ids(&list(&mut st, PageAt::Above(LibId::new(52).unwrap())))[47], 53);
}
#[test] fn factory_and_foreign_names_are_never_listed_or_opened() {
    let mut st = Counting::new(card_with(&["S0000001.A", "S9000001.A", "BANK1.BNK", "S000001.A"]));
    let p = list(&mut st, PageAt::Newest);
    assert_eq!(ids(&p), vec![1]);
    assert_eq!(st.reads(), vec!["S0000001.A"]);
}
#[test] fn only_seen_sides_are_read() {
    let mut st = Counting::new(card_with_sounds(1..=10));                 // .A only
    list(&mut st, PageAt::Newest);
    assert_eq!(st.reads().len(), 10);
    assert!(st.reads().iter().all(|n| n.ends_with(".A")));
}
#[test] fn bad_sides_list_with_their_errors() {
    let mut st = card_with_sounds(1..=2);
    truncate(&mut st, "S0000001.A", 0);
    set_version(&mut st, "S0000002.A", 2);
    let p = list(&mut st, PageAt::Newest);
    assert_eq!(entry(&p, 1).err, Some(FileError::Truncated)); assert_eq!(entry(&p, 1).file.seen.name, None);
    assert_eq!(entry(&p, 2).err, Some(FileError::NeedsNewerFirmware));
}
#[test] fn no_sounds_dir_is_an_empty_page() { assert!(ids(&list(&mut chimera_only(), PageAt::Newest)).is_empty()); }
#[test] fn no_card_is_its_store_error() {
    let mut st = MemStore::new(1); st.eject();
    assert_eq!(list_err(&mut st), LibNote::Store(StoreError::NoCard));
}
```

  `Counting` is a test `Store` wrapper that records the name of each `read`.

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_store_test`
  Expected: FAIL: `list_library` is undefined.

- [ ] **Step 3: Implement** as in Interfaces, with a bounded sorted array like `project::store::Ids`.

- [ ] **Step 4: Run** the Step 2 command, then `just stack-check`.
  Expected: PASS; stack-check green.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src/project/library.rs chimera-core/tests/library_store_test.rs
git commit -m "List the library newest first, 48 a page"
```

### Task 5: The load: `LibraryPart`, `LibraryAnswer`, the scratch read and `Project::adopt`

**Files:**
- Modify: `chimera-engine/src/project/library.rs`; `chimera-engine/src/project/guard.rs:72-78` (sealed list, `impl Witnessed`, `impl Target`); `chimera-engine/src/project/pool.rs` (`scratch_ref`); `chimera-engine/src/project/mod.rs`; `chimera-engine/src/storage/library.rs` (`LibraryDecoder`); `chimera-engine/src/confirm.rs:17-33,119-145` (`LibraryAnswer`)
- Test: `chimera-core/tests/library_load_test.rs`; one compile-fail doctest on `adopt`

**Interfaces:**
- Consumes: Task 2's `SoundCheck::skips`; Task 4's `LibFile`.
- Produces:
  - `pub struct LibraryDecoder<'a> { check: SoundCheck, target: &'a mut Sound }`:
    - `pub fn new(target: &'a mut Sound) -> Self`;
    - `impl DecodeInPlace`, whose `finish` runs `SoundCheck::finish(Some(target))`;
    - `pub fn skips(&self) -> u16`.
  - `pub struct LibraryPart { pub part: PartId, pub file: LibFile }` (`Clone, Copy, Debug, PartialEq`). It is sealed, with `impl Witnessed<Witness = u32>` and `impl Target`; `at_risk` and `witness` delegate to `PartSource { part, from: PartFrom::Init(EngineType::Algo) }` (the witness reads only the Part).
  - In `confirm.rs`:
    - `answers!(LibraryAnswer { SavePartFirst, Replace, Cancel } Three);`
    - `LibraryAnswer => Replace, LibraryPart;` added to `commits!`;
    - `impl Guarded for LibraryAnswer { const SAVE_FIRST: Self = LibraryAnswer::SavePartFirst; }`.
  - `pub(in crate::project) fn scratch_ref(&self) -> &Sound` on `Pool`.
  - `pub struct LibRead { pub skips: u16, pub source: Option<SourceNeed> }` (`Clone, Copy, Debug, PartialEq`).
  - `pub struct SourceNeed { pub r: SourceRef, pub held: usize, pub on_card: bool }` (`Clone, Copy, Debug, PartialEq`).
    - `held` is `held_for_file_len(size)` from the `/CHIMERA/TAPES/<CRC>.SRC` directory entry, read in the same `Card::run`.
    - Not on the card: `held` is 0 and `on_card` is false.
    - Already in the pool (`find_source(r)` holds it, not absent): `source` is `None`.
  - `pub fn read_library_sound<S: Store>(card: &mut Card, store: &mut S, p: &mut Project, file: LibFile) -> CardOut<Result<LibRead, LibNote>>`. It re-peeks the sides the directory shows:
    - neither side is `Gone`;
    - a newest generation or name ≠ `file.seen` is `Changed`;
    - a volume ≠ `file.vol` is `Store(VolumeChanged(..))`.

    Then `load_ab_in_place(.., &mut LibraryDecoder::new(p.pool.scratch()))`. A clobbered scratch is harmless, because only `adopt` reads it.
  - `pub enum Staged { Read(SourceNeed), Missing(SourceRef) }` (`Clone, Copy, Debug`).
  - `pub struct Adopted { pub slot: SlotId, pub deduped: bool }` and `pub enum AdoptError { Changed, Full(Option<PoolFull>) }` (`Debug, PartialEq`); `None` means every slot is taken. `impl From<AdoptError> for ReplaceError` maps `Changed` to `Changed`, which the UI shows as `PART CHANGED`.
  - `impl Project { pub fn library_needs(&self, src: Option<SourceNeed>) -> Result<Option<SlotId>, Option<PoolFull>> }`:
    - `Ok(Some(slot))` is a dedupe hit: a slot whose `crc()` equals `sound_crc(scratch_ref())`;
    - `Ok(None)` means it fits;
    - the needs are the scratch's slot cost (`pool.needs(first_free, scratch_ref())`) and a free Sound slot, plus, for a `src` with `on_card`, `source_cost(src.held)` and a free source slot, all against `pool.free()`;
    - a source not on the card costs 0 and needs no slot (it is stored absent).
  - `impl Project { pub fn adopt(&mut self, c: Confirmed<LibraryPart>, staged: Option<Staged>) -> Result<Adopted, AdoptError> }`. In order:
    1. `c.holds(self)`, else `Changed`;
    2. `library_needs`, else `Full`;
    3. `pool.source_commit(r, held)` for `Read`, or `source_absent(r)` for `Missing`;
    4. `pool.store_scratch(first_free)` unless deduped;
    5. `self.load_part(PartSource { part, from: PartFrom::Slot(slot) })`, which bumps the project and `part.loads` and sets the Origin.

    A refusal changes nothing. `replace_part` is untouched. The doctest is `compile_fail,E0308`: a `Confirmed<PartSource>` passed to `adopt`.

- [ ] **Step 1: Write the failing tests** (`library_load_test.rs`). `confirmed(&p, part, file)` is `ReplaceGuard::check(..).unwrap()` on a Clean Part. `tape_library(card)` saves a TAPE factory Sound whose `.SRC` is on the card.

```rust
#[test] fn adopt_stores_the_slot_and_the_part_reads_clean() {
    let (mut p, mut st, file) = saved(factory_sound(0));
    read(&mut st, &mut p, file).unwrap();
    let loads = p.part(P1).loads;
    let a = p.adopt(confirmed(&p, P1, file), None).unwrap();
    assert!(!a.deduped);
    assert_eq!(part_status(p.part(P1), p.pool()), PartStatus::Clean);
    assert!(p.pool().get(a.slot).unwrap().matches(&factory_sound(0)));
    assert_ne!(p.part(P1).loads, loads);
}
#[test] fn loading_the_same_sound_twice_reuses_its_slot() {
    let (first, second) = adopt_twice(factory_sound(0));
    assert!(second.deduped); assert_eq!(second.slot, first.slot);
}
#[test] fn over_budget_changes_nothing() {
    let (mut p, mut st, file) = saved_with_full_arena(factory_sound(0));
    read(&mut st, &mut p, file).unwrap();
    let (rev, used, n) = (p.rev(), p.pool().used(), p.pool().sound_count());
    let e = p.adopt(confirmed(&p, P1, file), None).unwrap_err();
    assert!(matches!(e, AdoptError::Full(Some(f)) if f.needs.bytes() > f.free.bytes()));
    assert_eq!((p.rev(), p.pool().used(), p.pool().sound_count()), (rev, used, n));
}
#[test] fn a_full_source_table_refuses_up_front() {
    let (mut p, mut st, file) = tape_library_with_32_sources_held();
    let r = read(&mut st, &mut p, file).unwrap();
    assert!(matches!(p.library_needs(r.source), Err(_)));
    assert_eq!(p.pool().source_count(), 32);
}
#[test] fn a_moved_witness_leaves_no_source_and_no_slot() {
    let (mut p, mut st, file) = tape_library();
    let r = read(&mut st, &mut p, file).unwrap();
    let c = confirmed(&p, P1, file);
    stage_source(&mut p, &mut st, r.source.unwrap());            // source_staging + fill, unpublished
    let (srcs, snds) = (p.pool().source_count(), p.pool().sound_count());
    p.edit_part(P1).sound.params.out.volume = 0.1;
    assert_eq!(p.adopt(c, Some(Staged::Read(r.source.unwrap()))).unwrap_err(), AdoptError::Changed);
    assert_eq!((p.pool().source_count(), p.pool().sound_count()), (srcs, snds));
}
#[test] fn a_missing_source_costs_nothing_and_loads_absent() {
    let (mut p, mut st, file) = tape_library_without_src();
    let r = read(&mut st, &mut p, file).unwrap();
    let need = r.source.unwrap();
    assert!(!need.on_card && need.held == 0);
    set_free_to(&mut p, p.pool().needs(p.pool().first_free().unwrap(), scratch_of(&mut p)));   // exactly the Sound's cost
    let a = p.adopt(confirmed(&p, P1, file), Some(Staged::Missing(need.r))).unwrap();
    let id = p.pool().find_source(need.r).unwrap();
    assert!(p.pool().absence(id).is_some()); assert!(!a.deduped);
}
#[test] fn a_file_changed_or_gone_since_listed_refuses() {
    let (mut p, mut st, file) = saved(factory_sound(0));
    rm_pair(&mut st, file.id);
    assert_eq!(read(&mut st, &mut p, file).unwrap_err(), LibNote::Gone);
    save_named(&mut st, &factory_sound(1), "OTHER");             // reuses id 1
    assert_eq!(read(&mut st, &mut p, file).unwrap_err(), LibNote::Changed);
}
#[test] fn a_corrupt_file_refuses_and_leaves_the_part() {
    let (mut p, mut st, file) = saved(factory_sound(0));
    flip_byte(&mut st, file.id, 40);
    let before = p.part(P1).sound.clone();
    assert_eq!(read(&mut st, &mut p, file).unwrap_err(), LibNote::File(FileError::BadCrc));
    assert!(p.part(P1).sound.bits_eq(&before));
}
#[test] fn a_translated_file_bigger_than_its_estimate_refuses_cleanly() {
    // a Modal Sound file built with one retired DECAY entry and EXCITE ≠ BURST's neutral
    let (mut p, mut st, file, len) = saved_bytes(translated_modal_file());
    read(&mut st, &mut p, file).unwrap();
    let estimate = Cost::of(len as usize - HEADER_LEN - TRAILER_LEN + 16);
    let exact = p.pool().needs(p.pool().first_free().unwrap(), scratch_of(&mut p));
    assert!(exact.bytes() > estimate.bytes());
    set_free_to(&mut p, estimate);
    assert!(matches!(p.adopt(confirmed(&p, P1, file), None), Err(AdoptError::Full(Some(_)))));
}
```

  `scratch_of` and `set_free_to` go in `project::test_support`. `set_free_to` fills the arena with filler Sounds until `free()` equals the given cost.

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_load_test && cargo test -p chimera-engine --doc`
  Expected: FAIL: `adopt`, `LibraryPart` and `LibraryAnswer` are undefined.

- [ ] **Step 3: Implement** as in Interfaces.

- [ ] **Step 4: Run** the Step 2 commands, plus `--test replace_guard_test --test project_model_test`, then `just stack-check`.
  Expected: PASS; stack-check green.

- [ ] **Step 5: Commit**

```bash
git add chimera-engine/src chimera-core/tests/library_load_test.rs
git commit -m "Load a library Sound through the pool in one step"
```

### Task 6: The UI: tab, jobs, staged source, prompt routing, NAMING and SAVE TO LIBRARY

**Files:**
- Create: `chimera-core/src/ui/library.rs` (`LibState`, `Tab`, `PendingLoad`)
- Modify:
  - `chimera-core/src/ui/source_cell.rs:204-243` (`step_staged`);
  - `chimera-core/src/ui/settings/job.rs` (`Job`, `card_work`);
  - `chimera-core/src/ui/settings/mod.rs` (`Ask`, `AskKind`, `NamingFor`);
  - `chimera-core/src/ui/settings/replace.rs` (`LibraryAsk`);
  - `chimera-core/src/ui/settings/prompt.rs:46,200-215` (`LibraryAnswer` labels; the prompt words `LOAD INTO P{n}?`);
  - `chimera-core/src/ui/settings/tree.rs:61,106-112` (`Act::SaveToLibrary`, the PART row);
  - `chimera-core/src/ui/browser.rs` (`Browse` holds the LIBRARY cursor and scroll);
  - `chimera-core/src/ui/mod.rs` (`UiState` fields, `queue`, the key loop, B's turn, EDIT and MIX + EDIT by tab, `save_first`, toasts)
- Test: `chimera-core/tests/library_ui_test.rs`; `chimera-core/tests/tape_source_cell_test.rs`; `chimera-core/tests/settings_screen_test.rs` (sixth PART row)

**Interfaces:**
- Consumes: Tasks 3–5.
- Produces:
  - `SourceLoad::step_staged<S: Store>(&mut self, s: &mut S, r: &Ready, p: &mut Project) -> StagedStep`, with `pub enum StagedStep { More, Staged(SourceNeed), Failed(&'static str) }`. It is today's read up to the last chunk, without `source_commit`. `step` becomes `step_staged` followed by the commit, unchanged for SOURCE.
  - `Job::LibraryList(PageAt)`, `Job::LibrarySave(SoundName)`, `Job::LibraryLoad(Confirmed<LibraryPart>)`.
  - `pub(crate) struct LibState { page: LibPage, listed: bool, load: Option<PendingLoad>, tab: Tab }`, with `pub enum Tab { Project, Library }`. Methods:
    - `pub fn busy(&self) -> bool`: a load is pending;
    - `pub fn forget(&mut self)`: `listed = false`;
    - `pub fn page(&self) -> Option<&LibPage>`.
  - `UiState::lib_tab(&self) -> Tab`, `UiState::forget_library(&mut self)`, and `UiState::card_pending` including `self.lib.busy()` (BUSY).
  - **Gating:**
    - `UiState::queue` refuses (drops the job) while `self.lib.busy() || self.src.busy()`;
    - the key loop's `if self.job.is_some() || self.modal.is_some() { break }` also breaks on `self.lib.busy()`, except that a MENU tap cancels the pending load (`CANCELLED`);
    - `SourceCell` refuses to start a card row while `lib.busy()`.
  - **Listing:** LIBRARY on screen with `!listed` queues `Job::LibraryList(PageAt::Newest)` on the next pass, as `wants_list` does for LOAD PROJECT. That covers the first B turn, a console `forget`, and `CARD CHANGED`.
  - **Keys** (the Sound rung): B's turn switches `Tab`. A moves the cursor of the current tab. EDIT and MIX + EDIT dispatch by tab:
    - PROJECT: unchanged;
    - LIBRARY: EDIT on a Sound row runs `ReplaceGuard::check(.., LibraryPart { part, file })`, queuing `Job::LibraryLoad` or opening `Ask::LoadLibrary`; EDIT on a paging row queues `LibraryList(Below(last))` / `LibraryList(Above(first))`; MIX + EDIT opens NAMING;
    - MIX + MINUS acts in PROJECT only.
  - `pub type LibraryAsk = Asked<LibraryPart, LibraryAnswer>;` and `Ask::LoadLibrary(LibraryAsk)`.
    - REPLACE confirms through `Asked::input` → `Pending::confirm(p, Said<LibraryPart>)` (#258), never `anyway`.
    - `save_first` becomes generic: `fn save_first<R: Target, A: Guarded<Confirms = R>>(&mut self, ask: Asked<R, A>, save: fn(&Offer) -> Option<PartAction>, again: impl Fn(Asked<R, A>) -> Ask, then: fn(&mut UiState, Confirmed<R>))`. Parts pass `UiState::replace`; the library passes a fn that queues `Job::LibraryLoad`.
  - `NamingFor::SaveToLibrary(PartId)`: `title()` is `SOUND NAME`, `crumb()` is `SAVE TO LIB`, pre-filled with the Part's Sound name.
  - The PART list's sixth row: `crumb("SAVE TO LIBRARY", "SAVE TO LIB", Kind::Act(Act::SaveToLibrary))`.

  The job flow:
  - **`LibrarySave`**: `save_library_sound(.., &about_page::THIS)`. On Ok: toast `SAVED TO LIBRARY`, `forget`, then queue `LibraryList(Below(id + 1))` with the cursor on `id`. That puts it on page 1 for a top + 1 id, and on its own page after the lowest-free fallback. On Err: toast `LibNote::message`.
  - **`LibraryLoad`**:
    1. `read_library_sound`, then `library_needs(r.source)`. `Err` calls `refuse(f)` (the existing `POOL FULL` / `NEEDS …` band).
    2. A `source` with `on_card` starts a `SourceLoad` and steps `step_staged` once a pass; MENU gives `CANCELLED`, and `Failed(m)` toasts `m`.
    3. `adopt(c, staged)`, where `staged` is `Staged::Read(need)`, or `Staged::Missing(r)` when `!on_card`, or `None`.
    4. On Ok, go to the Part's pages. One toast, by precedence: `SOURCE MISSING`, then `LOADED · SOME PARAMS SKIPPED` (skips > 0), then `LOADED TO SLOT n` (1-based). `Changed` toasts `prompt::refused(&ReplaceError::Changed)` (`PART CHANGED`), and `Full` refuses as in step 1.

- [ ] **Step 1: Write the failing tests** (`UiState` on `MemStore`, driven as `project_store_test` drives `card_work`; `work(&mut ui)` runs passes until `!card_pending()`):

```rust
#[test] fn b_turns_between_project_and_library_and_lists_once() {
    turn(&mut ui, EncoderId::B, 1); assert_eq!(ui.lib_tab(), Tab::Library);
    let reads = st.reads(); work(&mut ui); assert!(st.reads() > reads);
    turn(&mut ui, EncoderId::B, -1); turn(&mut ui, EncoderId::B, 1);
    let reads = st.reads(); work(&mut ui); assert_eq!(st.reads(), reads);          // cached
}
#[test] fn mix_edit_in_library_names_then_saves() {
    to_library(&mut ui); press(&mut ui, &[ButtonId::Mix, ButtonId::Edit]);
    assert_eq!(ui.naming_title(), Some("SOUND NAME"));
    let status = ui.project_status();
    press(&mut ui, &[ButtonId::Seq]); work(&mut ui);
    assert_eq!(toast(&ui), "SAVED TO LIBRARY");
    assert_eq!(card_ids(&st), vec![1]);
    assert_eq!(ui.project_status(), status);
}
#[test] fn save_to_library_row_in_settings_part() {
    open_settings_part(&mut ui); select_row(&mut ui, 5); press(&mut ui, &[ButtonId::Edit]);
    assert_eq!(ui.naming_crumb(), Some("SAVE TO LIB"));
}
#[test] fn edit_loads_into_the_part_and_the_project_reads_modified() {
    library_with(&mut st, &[factory_sound(0)]); to_library(&mut ui); work(&mut ui);
    press(&mut ui, &[ButtonId::Edit]); work(&mut ui);
    assert_eq!(toast(&ui), "LOADED TO SLOT 1");
    assert_eq!(ui.loc(), Location::part_home(P1, EngineType::Algo));
    assert_eq!(ui.project_status(), ProjectStatus::Modified);
}
#[test] fn an_edited_part_asks_and_replace_loads() {
    edit_part(&mut ui, P1); press(&mut ui, &[ButtonId::Edit]);
    assert_eq!(ui.ask_kind(), Some(AskKind::LoadLibrary));
    answer(&mut ui, LibraryAnswer::Replace); work(&mut ui);
    assert_eq!(toast(&ui), "LOADED TO SLOT 1");
}
#[test] fn save_part_first_that_takes_the_room_refuses_and_keeps_the_save() {
    arena_room_for_one(&mut ui); edit_part(&mut ui, P1); press(&mut ui, &[ButtonId::Edit]);
    answer(&mut ui, LibraryAnswer::SavePartFirst); work(&mut ui);
    assert!(ui.project().pool().get(SlotId::new(0).unwrap()).unwrap().matches(&edited_sound()));
    assert_eq!(ui.refusal_line(), Some("POOL FULL"));
    assert!(ui.project().part(P1).sound.bits_eq(&edited_sound()));
}
#[test] fn a_midi_edit_during_the_source_read_refuses() {
    tape_library(&mut st); to_library(&mut ui); work(&mut ui);
    press(&mut ui, &[ButtonId::Edit]); one_pass(&mut ui);                         // More
    let srcs = ui.project().pool().source_count();
    midi_cc_on_p1(&mut ui); work(&mut ui);
    assert_eq!(toast(&ui), "PART CHANGED");
    assert_eq!(ui.project().pool().source_count(), srcs);
}
#[test] fn the_last_chunk_does_not_publish_before_adopt() {
    // tape_source_cell_test: step_staged to Staged(_); source_count() unchanged
}
#[test] fn a_pending_load_blocks_every_other_card_start() {
    tape_library_two(&mut st); to_library(&mut ui); work(&mut ui);
    press(&mut ui, &[ButtonId::Edit]); one_pass(&mut ui);
    move_cursor(&mut ui, 1); press(&mut ui, &[ButtonId::Edit]);                   // second row
    ui.queue_for_test(Job::List(Screen::LoadProject));                            // LOAD PROJECT
    start_source_card_row(&mut ui);                                                // SOURCE card row
    assert!(ui.job_for_test().is_none()); assert!(!ui.src_busy_for_test());
    work(&mut ui);
    assert!(slot_of_p1(&ui).matches(&tape_sound(0)));                              // the first file's
}
#[test] fn menu_during_the_source_read_cancels() {
    tape_library(&mut st); start_load(&mut ui); one_pass(&mut ui);
    let (srcs, snds) = counts(&ui);
    press(&mut ui, &[ButtonId::Menu]); work(&mut ui);
    assert_eq!(toast(&ui), "CANCELLED"); assert_eq!(counts(&ui), (srcs, snds));
}
#[test] fn skipped_params_say_so() {
    put_raw(&mut st, 1, &three_skip_file()); load_row(&mut ui, 0);
    assert_eq!(toast(&ui), "LOADED · SOME PARAMS SKIPPED");
}
#[test] fn a_forgotten_page_relists_on_the_next_pass() {
    to_library(&mut ui); work(&mut ui); ui.forget_library();
    let reads = st.reads(); work(&mut ui); assert!(st.reads() > reads);
}
#[test] fn a_lowest_free_save_lands_on_its_page() {
    library_ids(&mut st, &[1, 8_999_999]); save_from_library(&mut ui, "X"); work(&mut ui);
    assert_eq!(cursor_id(&ui), Some(2));
}
```

  The `*_for_test` accessors are `#[cfg(any(test, feature = "test-support"))]`.

- [ ] **Step 2: Run them and see them fail**

  Run: `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --features chimera-hal/testkit --test library_ui_test --test tape_source_cell_test`
  Expected: compile errors: `Job::LibrarySave`, `lib_tab` and `step_staged` are undefined.

- [ ] **Step 3: Implement** as in Interfaces. The library browser draws only placeholder rows here; Task 7 draws them properly.

- [ ] **Step 4: Run** `cargo test -p chimera-core --features chimera-hal/testkit` and `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release`. The firmware build proves no test-only API is used.
  Expected: PASS; it builds.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui chimera-core/tests
git commit -m "Save to and load from the library: tab, jobs, prompts and NAMING"
```

### Task 7: Drawing the LIBRARY tab

**Files:**
- Modify: `chimera-core/src/ui/browser.rs` (title, rows, hint lines)
- Test: `chimera-core/tests/browser_test.rs`, `chimera-core/tests/dpad_nav_test.rs`, `chimera-core/tests/screen_golden_test.rs` (+ goldens)

**Interfaces:**
- Consumes: Task 6's `LibState`, `Tab`, `LibPage`.
- Produces:
  - `browser::draw` takes `view: BrowserView<'_>`, with `pub enum BrowserView<'a> { Project(&'a Pool), Library(&'a LibState, &'a BuildStamp) }`.
  - Title: `PROJECT  LIBRARY  B ◂▸` with `· P{n}` right-aligned.
  - Exact copy:
    - `pub const EMPTY_LIBRARY: &str = "NO SOUNDS YET";`
    - `pub const NEWER: &str = "‹ NEWER";` and `pub const OLDER: &str = "OLDER ›";`
    - `pub const SAVE_TO_LIBRARY: Gesture<'static> = Gesture("MIX + EDIT", "SAVES TO LIBRARY");`
    - `pub fn stamp_words(stamp: Option<BuildStamp>, this: &BuildStamp) -> FmtBuf`: `SAVED ON 0.1.0 02E440E2` (upper-cased, `+` when dirty), `SAVED ON THIS BUILD`, or `NO STAMP`.
  - LIBRARY row cost: `Cost::of(len − HEADER_LEN − TRAILER_LEN − 25 + 16)`, saturating; `Cost::of` rounds to the 16 B granule.
  - Greyed rows show the `FileError` message. Line 3 draws with HINTS off.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn stamp_words_forms() {
    let this = BuildStamp::new("0.1.0", "02e440e2", false);
    assert_eq!(stamp_words(Some(BuildStamp::new("0.1.0", "aabbccdd", true)), &this).as_str(), "SAVED ON 0.1.0 AABBCCDD+");
    assert_eq!(stamp_words(Some(this), &this).as_str(), "SAVED ON THIS BUILD");
    assert_eq!(stamp_words(None, &this).as_str(), "NO STAMP");
}
#[test] fn the_new_lines_fit_216_px() {
    assert!(hint::width(&[SAVE_TO_LIBRARY]) <= 216);
    let wide = stamp_words(Some(BuildStamp::new("10.10.10", "wwwwwwww", true)), &this());
    assert!(draw::text_width(&theme::FONT_LABEL, wide.as_str(), 0) <= 216);
}
#[test] fn line_three_draws_with_hints_off() { assert!(row_has_text(&drawn_library(hints_off()), STAMP_Y)); }
#[test] fn mix_minus_in_library_clears_nothing() {
    // dpad_nav_test: slot 0 filled, LIBRARY cursor 0, MIX+MINUS → ui.ask_kind() == None, pool unchanged
}
```

  `dpad_nav_test` gains B's turn, MIX + EDIT in LIBRARY (opens NAMING), and MIX + MINUS in LIBRARY (no-op). The screen goldens are both tabs with `P1`, `NO SOUNDS YET`, `NO CARD`, a greyed `FILE IS CUT SHORT` row, both paging rows, `WON'T FIT`, line 3's three forms, and NAMING `SAVE TO LIB`. Generate them with `just screens` and look at each PNG.

- [ ] **Step 2: Run them and see them fail**

  Run: `cargo test -p chimera-core --features chimera-hal/testkit --test browser_test --test dpad_nav_test --test screen_golden_test`
  Expected: FAIL.

- [ ] **Step 3: Implement** as in Interfaces.

- [ ] **Step 4: Run** the Step 2 command.
  Expected: PASS. Every golden was looked at.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/browser.rs chimera-core/tests
git commit -m "Sound browser: draw the LIBRARY tab, with each Sound's build"
```

### Task 8: Console verbs for library files

**Files:**
- Create: `chimera-core/src/console/sound_file.rs` (`rm`, `get`)
- Modify:
  - `chimera-core/src/console/put.rs` (`LsArg::Sounds`, `PutTo::Sound`, the exists rule, `--over`);
  - `chimera-core/src/console/mod.rs:126-140` (`rm` and `get` rows under `card-put`);
  - `chimera-core/src/console/answer.rs:190-205` (dispatch, `ERR busy`; the `Unit` trait gains `fn forget_library(&mut self) {}`);
  - `chimera-board/src/usb.rs:323-460` (`ChipUnit` records `forget: bool`; `PortState::service` calls `ui.forget_library()` after the answer when set);
  - `tools/chimera-usb.py` (a Sound `put` takes `ls sounds`' top user id + 1; `ERR busy` is retried with backoff, 5 tries, 200 ms doubling), `tools/test_chimera_usb.py`
- Test: `chimera-core/tests/console_parse_test.rs`, `chimera-core/tests/console_answer_test.rs`, `chimera-fat/tests/console_put_test.rs`, `tools/test_chimera_usb.py`

**Interfaces:**
- Consumes: Task 3's `lib_id`, `library_file`, `LibId`; `delete_ab`, `load_ab`/`pick`.
- Produces:
  - `LsArg::Sounds` (`ls sounds`).
  - `PutTo::Sound { id: LibId, over: bool }`, parsed from `/CHIMERA/SOUNDS/S#######.A|B`, any case, with an optional trailing `--over`. User and factory ids are both accepted.
  - `pub struct SoundArg(pub LibId)`: `/CHIMERA/SOUNDS/S#######`, any case, no extension.
  - `Request::Rm(SoundArg)` and `Request::Get(SoundArg)`.
  - `pub fn rm<F: Files>(arg: SoundArg, f: &mut F, out: &mut impl Out) -> Result<(), Stalled>`: `ERR rm: factory` for `S9…`; `ERR rm: not found` with neither side; else `delete_ab`, then `OK`.
  - `pub fn get<F: Files, P: Out>(arg: SoundArg, f: &mut F, out: &mut P) -> Result<(), Stalled>`. It answers `GET <len>`, base64 lines of 48 bytes, `CRC <crc32 hex>`, then `OK`. Both sides torn answers `ERR get: <FileError message>`.
  - A Sound `put` onto an id with either side present, without `--over`, answers `ERR put: exists` and writes nothing.
  - Every card verb answers `ERR busy` while `unit.ui().card_pending()`.
  - A successful Sound `put` or `rm` calls `unit.forget_library()`.

- [ ] **Step 1: Write the failing tests**

```rust
// console_parse_test.rs; parse_line(l) = feed(&mut Console::new(), &[l, b"\n"].concat()).pop().unwrap()
#[test] fn sound_paths_parse_and_others_refuse() {
    assert!(matches!(parse_line(b"put /chimera/sounds/s0000012.a 120 deadbeef"), Ok(Request::Put(PutArg { to: PutTo::Sound { over: false, .. }, .. }))));
    assert!(matches!(parse_line(b"put /CHIMERA/SOUNDS/S0000012.A 120 deadbeef --over"), Ok(Request::Put(PutArg { to: PutTo::Sound { over: true, .. }, .. }))));
    for bad in [&b"rm /CHIMERA/SOUNDS/../PROJECTS/P0000001"[..], b"rm //CHIMERA/SOUNDS/S0000001", b"rm /CHIMERA/PROJECTS/P0000001", b"rm SOUNDS/S0000001", b"rm /CHIMERA/SOUNDS/S000001", b"get /CHIMERA/SOUNDS/S0000001.A"] {
        assert!(matches!(parse_line(bad), Err(Refusal::Arguments(_))), "{bad:?}");
    }
    assert!(matches!(parse_line(b"ls sounds"), Ok(Request::Ls(LsArg::Sounds))));
}
// console_put_test.rs (MemStore and the fat image, as the file's other tests)
#[test] fn put_onto_an_existing_id_refuses_unless_over() {
    let before = bytes(&st, "S0000001.A");
    assert_eq!(answer_put(&mut unit, "S0000001.A", &other_sound_file()), "ERR put: exists\n");
    assert_eq!(bytes(&st, "S0000001.A"), before);
}
#[test] fn put_over_replaces_the_unit_way() {
    assert!(answer_put_over(&mut unit, "S0000001.A", &other_sound_file()).ends_with("OK\n"));
    assert_eq!(header(&st, "S0000001.B").generation, Generation::new(2));
    assert!(load_sound(&mut st, 1).bits_eq(&other_sound()));
}
#[test] fn rm_deletes_pairs_and_torn_ones_and_refuses_factory_and_missing() {
    assert_eq!(answer(&mut unit, "rm /CHIMERA/SOUNDS/S0000001"), "OK\n"); assert!(!exists(&st, "S0000001.A"));
    truncate(&mut st, "S0000002.A", 0);
    assert_eq!(answer(&mut unit, "rm /CHIMERA/SOUNDS/S0000002"), "OK\n");
    assert_eq!(answer(&mut unit, "rm /CHIMERA/SOUNDS/S0000077"), "ERR rm: not found\n");
    assert_eq!(answer(&mut unit, "rm /CHIMERA/SOUNDS/S9000001"), "ERR rm: factory\n"); assert!(exists(&st, "S9000001.A"));
}
#[test] fn get_streams_the_picked_side() {
    let (data, crc) = parse_get(&answer(&mut unit, "get /CHIMERA/SOUNDS/S0000001"));
    assert_eq!(data, bytes(&st, "S0000001.A")); assert_eq!(crc, Crc32::of(&data));
}
#[test] fn get_of_a_torn_pair_says_why() {
    tear(&mut st, "S0000003.A"); tear(&mut st, "S0000003.B");
    assert!(answer(&mut unit, "get /CHIMERA/SOUNDS/S0000003").starts_with("ERR get: FILE "));
}
#[test] fn ls_sounds_lists_the_library() {
    let len = len_of(&st, "S0000001.A");
    assert_eq!(answer(&mut unit, "ls sounds"), format!("S0000001.A {len}\nOK\n"));
}
// console_answer_test.rs
#[test] fn card_verbs_answer_busy_while_a_library_load_is_pending() {
    unit.ui_mut_for_test().start_pending_library_load_for_test();
    for line in ["ls sounds", "put /CHIMERA/SOUNDS/S0000002.A 10 0", "rm /CHIMERA/SOUNDS/S0000001", "get /CHIMERA/SOUNDS/S0000001", "ls"] {
        assert_eq!(answer(&mut unit, line), "ERR busy\n", "{line}");
    }
}
#[test] fn put_and_rm_ask_to_forget_the_page() {
    answer(&mut unit, "rm /CHIMERA/SOUNDS/S0000001"); assert_eq!(unit.forgotten, 1);
}
```

  In `tools/test_chimera_usb.py`, `test_busy_is_retried`: a fake port answers `ERR busy` twice, then `OK`; `put` succeeds after 3 tries.

- [ ] **Step 2: Run them and see them fail**

  Run: `cargo test -p chimera-core --features chimera-hal/testkit --test console_parse_test --test console_answer_test && cargo test -p chimera-fat --features chimera-hal/testkit --test console_put_test && python3 -m unittest discover -s tools -p 'test_*.py'`
  Expected: FAIL.

- [ ] **Step 3: Implement** as in Interfaces. A Sound `put` reuses the project path's A/B save, with `SoundCheck` as the kind's check. `help` lists `rm` and `get`.

- [ ] **Step 4: Run** the Step 2 commands, plus `cargo test -p chimera-core --features fault-inject --test console_fault_test --test console_answer_test` and `cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release`.
  Expected: PASS; it builds.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/console chimera-board/src/usb.rs chimera-core/tests chimera-fat/tests tools
git commit -m "Console: ls, put, rm and get for library Sounds"
```

### Task 9: Stock files, desktop QA, ADR 0119 and the full check

**Files:**
- Modify: `chimera-fat/tests/stock_files_test.rs`
- Create: `docs/adr/0119-a-sound-library-on-the-card.md` (template `docs/adr/0000-template.md`, Status Proposed); modify `docs/adr/README.md`
- Test: `chimera-fat/tests/stock_files_test.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: ADR 0119, holding the spec's § ADRs bullets. It amends 0081 and 0043 (both Proposed) and supersedes 0086 in part (its browser hint rows). README rows: 0119 is added, and the 0081, 0043 and 0086 rows each gain "amended by / superseded in part by [0119]". No other ADR file is edited.

- [ ] **Step 1: Write the failing test**

```rust
#[test] #[ignore] fn library_paths_never_touch_stock_files() {
    let img = stock_card_image();
    cut_and_tear_every_block(&img, |s| save_library_sound(..));    // as the Sound-save case
    console_put(&img, "/CHIMERA/SOUNDS/S0000002.A", &sound_file()); console_rm(&img, "/CHIMERA/SOUNDS/S0000002");
    list_every_page(&img); read_and_adopt(&img, 1);
    assert_stock_unchanged(&img);                                   // entries, FAT links, data bytes
    assert!(written_entries(&img).iter().all(|p| p.starts_with("/CHIMERA/SOUNDS/") || p == "/CHIMERA/SOUNDS"));
    assert!(fsck_fat_passes(&img));
}
```

- [ ] **Step 2: Run it and see it fail**

  Run: `cargo test -p chimera-fat --features chimera-hal/testkit --test stock_files_test -- --ignored library_paths_never_touch_stock_files`
  Expected: FAIL (helpers missing).

- [ ] **Step 3: Implement** the test's helpers. Then write ADR 0119 and the README rows.

- [ ] **Step 4: Desktop QA** (the spec's § Testing):
  1. Run `just desktop` with `CHIMERA_CARD` set to a scratch dir, and save a Sound from LIBRARY.
  2. Change one param's neutral default, with its migration, and retire one param, in a scratch commit not kept.
  3. Rebuild and load the Sound back.

  Expected: it loads, toasts `LOADED · SOME PARAMS SKIPPED`, and renders. Revert the scratch commit.

- [ ] **Step 5: Run the full check**

  Run: `touch chimera-engine/src/lib.rs && just check && just stack-check`
  Expected: both green. Record the flash and AXI deltas of `chimera-stm32` release against `origin/main` in one line in the ADR's Consequences. Expect a few KB of flash and about 2.5 KB of AXI (the static page). Escalate only if free flash drops under about 40 KB.

- [ ] **Step 6: Commit**

```bash
git add chimera-fat/tests/stock_files_test.rs docs/adr/0119-a-sound-library-on-the-card.md docs/adr/README.md
git commit -m "ADR 0119 and the stock-files check for the library"
```

---

**On the chip, at ship (not a task):**
- measure one page's listing with 500 files in `SOUNDS/`; over 1.5 s, file `SNDINDEX` as a GitHub issue;
- the owner's UAT: save on one build, flash the next, load it back by ear.

## Review resolutions

Every finding of both rounds (private notes) was accepted.

| Round | # | Where it landed |
|---|---|---|
| 1 | M1–M5, m6–m16 | Spec § Review resolutions; this plan's Tasks 1–9 |
| 1 | M2 partial | Task 6 `step_staged` (no early commit) and gating; Task 5 `adopt` commits the source |
| 1 | M3 partial | Task 5 `LibraryAnswer` in `confirm.rs`; Task 6 REPLACE via `Pending::confirm`, generic `save_first` |
| 1 | M4 partial | Task 1: `Build` `Ok` in every project context; build.rs cuts the hash to 8 |
| 1 | m16 partial | Spec: `MAX_SOUND_COST` + 41 B |
| 2 | A | Task 6 `SourceLoad::step_staged`; test that the last chunk doesn't publish |
| 2 | B | Task 6: `queue`, the key loop and `SourceCell` refuse while busy; clobber test |
| 2 | C | Tasks 5 and 6 (`confirm.rs`, `prompt.rs`, `Said`); firmware build in Task 6 Step 4 |
| 2 | D | Task 1 `build_at_project_top_level_loads` |
| 2 | E | Task 5 `SourceNeed` from the TAPES entry; missing source costs 0; `scratch_ref` |
| 2 | m1 | Task 5 step 5 calls `load_part` |
| 2 | m2 | Task 3: `FullOnWrite`; the cut test in `chimera-fat/tests/power_cut_test.rs` |
| 2 | m3 | Task 3: window loop uncapped, bound sentence gone |
| 2 | m4 | Tab, B and dispatch moved into Task 6; Task 7 draws only |
| 2 | m5 | Task 6: LIBRARY with no page queues `LibraryList(Newest)`; relist test |
| 2 | m6 | Task 8: `ChipUnit` flag, `PortState::service` forgets |
| 2 | m7 | Files: `Act` in `tree.rs`, `Browse` in `browser.rs`, `confirm.rs`, `prompt.rs` |
| 2 | m8 | Task 4: `list_library(.., out: &mut LibPage)`; stack-check in Tasks 4 and 5 |
| 2 | m9 | Task 1: build.rs takes the first 8 characters |
| 2 | m10 | Task 6: re-list `Below(id + 1)`, cursor on the id; spec § Order |
| 2 | m11 | Spec: + 41 B |
| 2 | m12 | Task 9 Step 4 desktop QA; Global Constraints copy and two constraints |
| 2 | m13 | Tests rewritten as assertions; translated-file test asserts `exact > estimate` first; `ls` uses the real length; Modified asserted |
| 2 | m14 | Task 5 `From<AdoptError> for ReplaceError`; `PART CHANGED` |
| 2 | m15 | Task 8: host tool retries `ERR busy`, with a test |
