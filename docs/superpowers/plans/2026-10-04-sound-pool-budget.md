# Sound Pool by Memory Budget Implementation Plan

**Goal:** Replace the project's fixed 32-slot pool with a 160 KiB arena of sparse-encoded Sounds and card-backed 12-bit sources, shown on one meter. Free the 48 KB that pays for it by storing the master delay line as `i16`.

**Architecture:**
- **Two-ended arena, one meter.** One AXI arena holds Sounds packed down from the top and sources packed up from 0, with free space as the gap between them. Sounds compact at once. Sources compact under an audio gate, only when no moved source is sounding.
- **Parts decode, sources are shared.** A Part still decodes its Sound into `Performance`. A TAPE Part shares an immutable source through a `SourceTable` of atomics.
- **The card holds the audio.** Source audio lives only on the card (`/CHIMERA/TAPES`, write-once); projects carry `SourceRef`s, never audio.
- **The linker guards AXI.** It enforces a real AXI margin in place of the paper `AXI_RESIDENT` rule.

**Tech Stack:** Rust `no_std` (`chimera-core`), cortex-m-rt linker scripts (`chimera-stm32/build.rs`), the project's TLV card format (ADR 0045) on its own FAT layer (`chimera-fat`, ADR 0048), embedded-graphics UI.

**Spec:** `docs/superpowers/specs/2026-10-04-sound-pool-budget-design.md`; ADR 0087 (pool) and ADR 0090 (delay line). Read both before any task.

## Global Constraints

- **Code rules (CLAUDE.md):** no `unsafe` without a `// SAFETY:` comment; no heap in the audio callback; no libc; the audio thread never blocks or allocates; parameter changes lerp and never snap.
- **Commits:** a terse subject with no type prefix. Stage by path, never `git add -A`.
- **Arena constants:** `ARENA_BYTES = 160 * 1024`, `GRANULE = 16`, `MAX_SOUNDS = 128`, `MAX_SOURCES = 32`, `MAX_SOUND_COST = 1872`.
- **AXI margin:** a linker `ASSERT` keeps the gap between `__sheap` and the end of AXI at ≥ 32 KiB on shipping builds and ≥ 8 KiB on `bench` builds. It must hold on every feature set `just check` builds.
- **Delay line (ADR 0090):** `i16`, full scale ±2.0, write `(x * 16384.0) as i16` (truncates toward zero, saturates), `MAX_DELAY_SAMPLES` unchanged at 24 064.
- **Sparse Sounds:** `encode_sound` writes only params whose disk value differs from `Sound::neutral(engine)`. `neutral` is a format contract: a change to it needs a migration in the same commit.
- **Record tags** (frozen once written): `PoolSource` `0x8015`, `SourceRef` `0x0016`, `SourceHead` `0x8017`, `SourceData` `0x0018`; `FileKind::Source = 6`; `Dir::Tapes` = `/CHIMERA/TAPES`; `FORMAT_VERSION` stays 1.
- **`SourceRef`:** `{ crc: u32, frames: u32, rate: u16 }`, 10 B on disk. Task 8 defines the type and its record codec; Task 9 consumes them. A source file is `/CHIMERA/TAPES/<CRC>.SRC`, with the CRC in upper-case hex, sealed read-only once verified.
- **12-bit packing:** `b0 = s0[7:0]`, `b1 = s0[11:8] | s1[3:0] << 4`, `b2 = s1[11:4]`; an odd tail pads with 0; `SourceData` carries up to 504 B (168 pairs).
- **Meter:** a bar 216 × 4 px. Sounds `INK`, sources `MID`, pending `FAINT`, free `BAR_REST`, preview `ACCENT` or `ALERT`. It reads `⌈used × 100 / ARENA_BYTES⌉ %`, a percentage only.
- **Copy:**
  - `POOL FULL · NEEDS {n}K, {m}K FREE`
  - `MIX + MINUS CLEARS · FREES {n}K`
  - `MIX + MINUS CLEARS · {NAME} {n}K UNUSED`
  - `MIX + EDIT SAVES · +{n}K`
  - `WON'T FIT`
  - `PURGE {n} SOURCES? FREES {m}K`
  - `LOADED · {a} SOUNDS, {b} SRC DIDN'T FIT`
  - `SOURCE MISSING`
  - `SOURCE NOT ON CARD`
  - `CARD READ FAILED`

  Every hint must fit 216 px (ADR 0086).
- **Card safety (the owner's sealed card):**
  - No task may leak or cross-link clusters.
  - Every card-writing or project path is tested with:
    - the power-cut harness (`chimera-fat/tests/power_cut_test.rs`: `Cut::After(k)`, `fat_check` → no `cross_linked`, no `short`, the kept side intact);
    - the device-error probes in `chimera-fat/tests/common/probe.rs`: `Log::fail_write` (every write to one block errs; on `main`), and `Log::land_then_fail` (the write lands, then errs, as a card that timed out busy). Both are on `main`.
  - Under either fault, `fat_check` must show no `cross_linked` and no `short`. Its `lost` list must be unchanged, except when the failed write is the file's `put_entry`. That write may have landed, so #359's rule leaves its chain for #360's sweep.
  - **console-put is on `main`** (#361, 0f3ca453), and this branch is rebased onto it. It brought #359's chain free (`drop_chain`, closed), `writable()` and `StoreError::ReadOnly` (which also refuse `remove` of a read-only file), `ByteSink::put`, the probe's two faults, `fat_check`'s `lost` and its walk of the directories under `/CHIMERA`. Tasks 5–9 use them as they are and never re-fix #359. `Store::seal` is not there: Task 9 still adds it.
- **Verification:** `just test`, `just clippy`, `just check` (every firmware feature set). **(unit)** marks an on-unit check.
- **The owner's card is sealed, and there is no other card.** An on-unit check is a read-only boot. No project save, quick-save or source write runs on the unit from this branch until #360 is on `main` too (#359 is).

## Review Focus

1. **A card error during a project load's second pass** (device error or card pulled): the load falls back to NEW, and the arena is exactly NEW's (`free() == gap`, the meter at NEW's percentage), not half-filled. *Task 5: `load_device_error_leaves_new_arena`.*
2. **SAVE OVER a slot whose edited Sound grew past free space:** refused with `POOL FULL`. The Part keeps its edits, and `project_crc` is unchanged. *Task 6: `save_over_growth_refused_keeps_part`.*
3. **A purged source still sounding:** its bytes come back by themselves when the note ends, with no second purge. The pending segment leaves the meter. *Task 8: `pending_hole_resolves_when_note_ends`.*
4. **A source file truncated by a power cut, or a card swapped between a load's two passes:** the source reads as missing, never as wrong-length audio. *Task 9: `truncated_source_reads_missing`, `card_swap_between_passes_reads_missing`.*
5. **A project loaded while a source file is missing, then saved:** the save keeps the `PoolSource` ref, so the reference survives a card that is temporarily without the file. *Task 9: `missing_source_ref_survives_save`.*

---

### Task 1: The delay line as `i16` (ADR 0090) (unit)

**Files:**
- Modify: `chimera-core/src/dsp/delay.rs` (`TapeDelay::buffer` at line 274, its init at 328, the write at ~465, `read` at ~488)
- Test: `chimera-core/src/dsp/delay.rs` (`#[cfg(test)]`), `chimera-core/tests/delay_test.rs`, `chimera-core/tests/memory_budget_test.rs`
- Modify: the delay goldens (`chimera-core/tests/fx_golden_test.rs` tables, re-recorded)

**Interfaces:**
- Produces:
  - `const LINE_SCALE: f32 = 16384.0`
  - `fn to_line(x: f32) -> i16`
  - `fn read(buffer: &[i16; MAX_DELAY_SAMPLES], write_pos: usize, delay: f32) -> f32`, which folds `1/LINE_SCALE` into the interpolation weights
  - `size_of::<TapeDelay>()` shrinks by 48 128 B

- [ ] **Step 1: Write the failing tests**

```rust
// delay.rs #[cfg(test)]
#[test] fn write_truncates_toward_zero_and_saturates() {
    assert_eq!(to_line(0.9 / 16384.0), 0);
    assert_eq!(to_line(-0.9 / 16384.0), 0);
    assert_eq!(to_line(3.0), i16::MAX);
    assert_eq!(to_line(-3.0), i16::MIN);
    assert_eq!(to_line(1.0), 16384);
}
// delay_test.rs
#[test] fn tail_reaches_exact_zero() // impulse 0.5, FDBK 0.9, SAT 0.2, TONE default: every line sample == 0 within 30 s of 48 kHz blocks
#[test] fn noise_floor_tracks_feedback() // a 0.25-peak 1 kHz burst, then silence: the tail's RMS above the repeat at FDBK 0.9 is within 3 dB of −82 dBFS, and at 0.99 within 3 dB of −72 dBFS
#[test] fn hot_send_clips_never_wraps() // a +6 dB send: every output sample is finite and |y| ≤ 2.0 + 1e-3
// memory_budget_test.rs
/// FxBus as measured at cf50ee74, with the f32 line; the orbit firmware's real AXI use there (`rust-size`).
const FXBUS_F32: usize = 159_784;
const ORBIT_AXI_USED: usize = 384_264;
#[test] fn delay_line_frees_48128() { assert_eq!(FXBUS_F32 - size_of::<FxBus>(), 48_128); }
#[test] fn arena_needs_the_delay() {
    let freed = FXBUS_F32 - size_of::<FxBus>();
    let after = |freed: usize| ORBIT_AXI_USED - freed - 28_672 + 2_568 + 160 * 1024;
    assert!(after(freed) <= hw::AXI_SRAM - 32 * 1024, "160 KiB fits the 32 KiB margin with the i16 line");
    assert!(after(0) > hw::AXI_SRAM - 32 * 1024, "and would not with the f32 line (ADR 0087)");
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p chimera-core delay`. Expected: FAIL, because `to_line` is not defined.

- [ ] **Step 3: Implement it.** Change `buffer` to `[i16; MAX_DELAY_SAMPLES]`, write through `to_line`, and read through the new `read`. `full_feedback_with_no_saturation_stays_bounded` (ADR 0038) must pass unchanged.

- [ ] **Step 4: Re-record the delay goldens.** Run `GOLDEN_RECORD=1 cargo test -p chimera-core --test fx_golden_test`, then `cargo test -p chimera-core`. Expected: PASS. Only the delay rows change; check the diff before staging.

- [ ] **Step 5: (unit) The bench and the sound.**
  - Run `just flash-bench`, then `python3 tools/chimera-usb.py bench`. The DELAY row's cycles must be within the f32 build's DELAY row + 7 cycles a sample; record both in the commit message.
  - Run `just flash`, then play a held chord into DLY SEND at FDBK 0.5, 0.9 and 0.99 and SAT 0, 0.2 and 1.0, and record the output. Its repeats must match the f32 build's by ear, with no hiss under a playing repeat.
  - The owner's UAT is by ear (QA before UAT).

- [ ] **Step 6: Commit.** Run `git add chimera-core/src/dsp/delay.rs chimera-core/tests/delay_test.rs chimera-core/tests/memory_budget_test.rs chimera-core/tests/fx_golden_test.rs`, then `git commit -m "Delay line as i16, full scale ±2 (ADR 0090)"`.

### Task 2: Sparse `encode_sound`

**Files:**
- Modify: `chimera-core/src/storage/block_codec.rs` (`encode_block`)
- Modify: `chimera-core/src/storage/sound.rs` (`encode_sound`, add `MAX_SOUND_COST`)
- Modify: the callers `chimera-core/src/project/codec.rs:97`, `chimera-core/src/orbit/records.rs:40`, `chimera-core/src/storage/system.rs:46,49`
- Test: `chimera-core/tests/sound_codec_test.rs`, `chimera-core/tests/disk_codes_test.rs` (neutral note), `chimera-fat/tests/power_cut_test.rs` (re-run)

**Interfaces:**
- Produces:
  - `pub fn encode_block(b: BlockRef, blk: &dyn Block, base: Option<&dyn Block>, out: &mut RecordBuf) -> usize`: the params written; with `base`, only those whose disk value differs. Every non-Sound caller passes `None`.
  - `pub const MAX_SOUND_COST: usize = 1872`
  - `encode_sound` keeps its signature and skips a block whose count is 0

- [ ] **Step 1: Write the failing tests** in `sound_codec_test.rs`:

```rust
#[test] fn sparse_round_trip()      // factory 0..8, INIT ×2, 1000 seeded random Sounds: pass(decode, encode(s)).bits_eq(s)
#[test] fn sparse_byte_identity()   // encode(decode(encode(s))) == encode(s); and encode(decode(fixture v1 .snd)) == encode(original)
#[test] fn sparse_is_canonical()    // for 1000 pairs (s, s with one param moved one step): (encode(a) == encode(b)) == a.bits_eq(&b)
#[test] fn sparse_sizes()           // 16 + records: factory_0 ≤ 273 B; INIT == 128 B
#[test] fn sparse_bound()           // every param off neutral, full registry, 128 routes: 16 + records ≤ MAX_SOUND_COST
#[test] fn neutral_is_a_contract()  // Sound::neutral matches fixtures/neutral_v1.txt; message: "changing neutral needs a migration (ADR 0087)"
```

The random generator draws each param uniformly from its spec range, quantised as its spec does, with 0–128 routes and 0–16 registry entries. It runs on a new `pub struct XorShift(u64)` in `chimera-core/tests/common/mod.rs` (`next_u32`, `below(n)`), because `chimera-fat`'s test copy can't be imported here. Add that file to this task's Files.

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test sound_codec_test`. Expected: FAIL in `sparse_sizes` and `sparse_bound`.

- [ ] **Step 3: Implement it.** Build `Sound::neutral(s.engine())` once in `encode_sound`, and pass each block's neutral counterpart as `base`. Compare `read_value` results (`DiskValue`) for equality, by bits for reals.

- [ ] **Step 4: Run the tests and the card harnesses.** Run `cargo test -p chimera-core` and `cargo test -p chimera-fat --features chimera-hal/testkit --test power_cut_test --test fat_store_test`. Expected: PASS, with no change to the cut assertions.

- [ ] **Step 5: Commit.** Run `git add` on the files above, then `git commit -m "Sounds encode sparse against neutral"`.

### Task 3: The AXI margin in the linker

**Files:**
- Modify: `chimera-stm32/build.rs` (write `axi_margin.x`, add `-Taxi_margin.x`)
- Modify: `chimera-core/src/hw.rs` (delete `UI_RESERVE`)
- Modify: `chimera-core/src/instrument.rs:20,44-57` (the `UI_RESERVE` import, `AXI_RESIDENT` and its two asserts)
- Modify: `chimera-stm32/src/sd_probe.rs:9-10,341`. Its `AXI FREE` line uses `AXI_RESIDENT`; it now prints the real gap, `0x2408_0000 − addr_of!(__sheap)`, from `extern "C" { static __sheap: u8; }`, with a `// SAFETY:` comment (address only, never read).
- Test: `chimera-core/tests/memory_budget_test.rs`:
  - `axi_residents_fit` becomes `axi_report`;
  - `ui_state_fits_the_ui_reserve` (lines 170–180) becomes `ui_state_rest_under_4k`, asserting `size_of::<UiState>() − size_of::<Project>() ≤ 4096` with no `UI_RESERVE`.

**Interfaces:**
- Produces:
  - the linker symbol `AXI_MARGIN`: 32 KiB, or 8 KiB when `CARGO_FEATURE_BENCH` is set
  - `ASSERT(ORIGIN(RAM) + LENGTH(RAM) - __sheap >= AXI_MARGIN, "AXI: under the margin (ADR 0087)")`

- [ ] **Step 1: Write the failing check.** In `axi_margin.x`, set the margin temporarily to `400K`.

- [ ] **Step 2: Run it.** Run `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`. Expected: a link error, `AXI: under the margin`.

- [ ] **Step 3: Implement it.** Use the real margins. Delete `UI_RESERVE`, the `AXI_RESIDENT` const assert and the `≥ 64 KiB` assert. `axi_report` prints the parts and asserts nothing about spare.

- [ ] **Step 4: Verify.** Run `just check` and `just test`. Expected: every feature set links, and `just flash-sd-probe`'s build prints `AXI FREE`. Record `rust-size` free AXI for `orbit` (140 024 today) and `bench,master-tape` (104 188) in the commit message.

- [ ] **Step 5: Commit** with `git commit -m "AXI margin enforced by the linker, not a paper sum"`.

### Task 4: The arena

**Files:**
- Create: `chimera-core/src/project/arena.rs`
- Modify: `chimera-core/src/project/mod.rs` (the module)
- Test: `chimera-core/tests/arena_test.rs`

**Interfaces:**
- Produces:
  - `pub struct Cost(u32)` with `Cost::of(len: usize) -> Cost` (16 B granule, rounded up), `bytes() -> u32`, and `kib_label() -> FmtBuf`. The label is `bytes / 1024` rounded **up**, with one decimal under 10 (`0.3K`, `9.9K`) and a whole number from 10 (`10K`, `41K`).
  - `pub struct Shift { pub from: u32, pub by: u32 }`: every entry on the moving side of `from` moved `by` bytes toward its own end. Sounds above the freed offset move up; sources above the hole move down. The caller rewrites its index from it, so no list of moves exists.
  - `pub struct ArenaWriter`, built only by `ArenaWriter::new(bytes: &'static mut [u8]) -> ArenaWriter`: the AXI static on the chip, and `Box::leak(vec![0; ARENA_BYTES].into_boxed_slice())` in tests and on the desktop, so it needs no `alloc` cfg. It provides:
    - `sound_alloc(len) -> Option<u32>`
    - `sound_free(offset, len) -> Shift`, which compacts at once
    - `source_alloc(len) -> Result<u32, Refused>`
    - `source_free_pending(offset, len)`
    - `source_compact(&mut self, ready: impl Fn(u32 /*offset*/) -> bool) -> Option<Shift>`: `None` while any source above the lowest hole isn't `ready`
    - `used() -> Cost`, `pending() -> Cost`, `gap() -> Cost`, `bytes(offset, len) -> &[u8]`, `bytes_mut(offset, len) -> &mut [u8]`
  - Invariant: `ARENA_BYTES - used() == gap()`, with pending counted as used.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn cost_rounds_to_granule() { assert_eq!(Cost::of(1).bytes(), 16); assert_eq!(Cost::of(16).bytes(), 16); assert_eq!(Cost::of(17).bytes(), 32); }
#[test] fn kib_labels() { assert_eq!(Cost::of(300).kib_label().as_str(), "0.3K"); assert_eq!(Cost::of(10_240).kib_label().as_str(), "10K"); assert_eq!(Cost::of(41_000).kib_label().as_str(), "41K"); }
#[test] fn ends_never_meet() // fill Sounds from the top and sources from 0 until refusal; no overlap; last refusal leaves gap < request
#[test] fn model_10k_ops()   // seeded: alloc, free, pending, compact; after each: disjoint, aligned to 16, sources contiguous from 0 but for pending holes, Sounds contiguous to ARENA_BYTES, used()+gap()==ARENA_BYTES, per-entry CRC unchanged
#[test] fn compact_waits_while_not_ready() // a pending hole below an entry whose ready() is false stays; gap unchanged
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -p chimera-core --test arena_test`. Expected: FAIL (no module).

- [ ] **Step 3: Implement it.** Use two cursors: `src_end` grows up from 0, `snd_start` grows down from `ARENA_BYTES`. Keep pending holes in a fixed list of at most `MAX_SOURCES`. `bytes_mut` is UI-only, and its `// SAFETY:` comment names ADR 0087's gate (Task 8).

- [ ] **Step 4: Run** `cargo test -p chimera-core --test arena_test`. Expected: PASS.

- [ ] **Step 5: Commit** with `git commit -m "Pool arena: two ends, one gap"`.

### Task 5: The pool on the arena, Sounds only (unit)

**Files:**
- Modify:
  - `chimera-core/src/project/pool.rs` (rewrite)
  - `ids.rs` (`SlotId` < 128)
  - `marks.rs` (encoded compare; `apply_part_action` at :229–231 maps `PoolFull`)
  - `mod.rs` (`replace_part`; `pool_store`, `pool_clear`, `save_part_to` now return `Result<_, PoolFull>`)
  - `codec.rs` (`Slot` index 0..128, slots from the arena)
  - `template.rs` (`Project::fill_new` at :79 and `new_slot` at :120; `reset_new` resets the arena)
- Modify the `pool_store` and `save_part_to` callers: `chimera-core/src/project/test_support.rs:31-33,83`, `chimera-core/src/project/template.rs:167` (test), `chimera-core/src/ui/mod.rs` (its `pool().get` and `pool_clear` sites), `chimera-desktop/src/store.rs:352`, `chimera-stm32/src/bench.rs:669`
- Modify: `chimera-core/src/preset.rs` (`POOL_SIZE` → `MAX_SOUNDS = 128`), `chimera-core/src/ui/browser.rs` (`TOTAL_ENTRIES`)
- Modify: `chimera-stm32/src/shared.rs` (the `POOL_ARENA` static, its `ArenaWriter` taken once), `chimera-desktop/src/main.rs` and `chimera-desktop/src/qa.rs` (a leaked boxed arena), `chimera-stm32/src/bench.rs` (its `Project`)
- Test: `chimera-core/tests/project_model_test.rs`, `project_marks_test.rs`, `project_codec_test.rs`, `memory_budget_test.rs`; `chimera-fat/tests/pool_card_test.rs` (new)

**Interfaces:**
- Consumes: `ArenaWriter`, `Cost` (Task 4); sparse `encode_sound` (Task 2).
- Produces:
  - `pub struct EncodedSound<'a>`, with `name() -> SoundName`, `engine() -> EngineType`, `cost() -> Cost`, `crc() -> u32` and `decode_into(&self, s: &mut Sound) -> Result<(), FileError>`
  - `Pool::get(SlotId) -> Option<EncodedSound<'_>>`, `Pool::used() -> Cost`, `Pool::free() -> Cost`, `Pool::fits(Cost) -> bool`, `Pool::sound_count() -> usize`
  - `Project::pool_store(SlotId, &Sound) -> Result<(), PoolFull>`, where `pub struct PoolFull { pub needs: Cost, pub free: Cost }`
  - `part_status` keeps its signature: `Clean` iff `encode(part.sound)` equals the slot's bytes

- [ ] **Step 1: Write the failing tests:**

```rust
// project_model_test.rs
#[test] fn new_pool_reads_its_cost()  // NEW: sound_count() == 10 (8 factory + 2 INIT), used() == sum of Cost::of(16 + records)
#[test] fn store_refuses_past_budget() // store worst-case Sounds until Err(PoolFull { needs, free }) with free < needs; pool unchanged after the refusal
#[test] fn clear_compacts_and_moves_gen() // clear: gap grows by the cost; generation moves; other slots decode bit-equal
#[test] fn in_use_clear_still_refused()   // a slot a Part uses: Err(InUse) with 128 slots, slot 100 included
// project_marks_test.rs
#[test] fn marks_compare_encodings()  // Clean iff bits_eq, for 200 seeded (part, slot) pairs
// project_codec_test.rs
#[test] fn v1_project_fixture_loads_same() // fixtures/v1/project.prj: every Part bits_eq as before; slots re-encoded sparse
#[test] fn slot_index_127_round_trips()    // and 128 is Bounds
// chimera-fat/tests/pool_card_test.rs
#[test] fn load_device_error_leaves_new_arena() // Log::fail_reads and Log::absent mid pass 2: load errs; project == NEW; pool.used() == NEW's; gap == ARENA_BYTES − used
#[test] fn project_save_cut_at_every_write()    // power_cut pattern: cut at each write of a 128-slot project save: kept side loads, no cross_linked, no short
#[test] fn project_save_device_error_no_leak()  // Log::fail_write and Log::land_then_fail at each write: no cross_linked, no short; `lost` unchanged unless the failed write is the put_entry
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -p chimera-core --test project_model_test` and `cargo test -p chimera-fat --features chimera-hal/testkit --test pool_card_test`. Expected: FAIL to compile, since `Pool::fits` is not defined.

- [ ] **Step 3: Implement it.**
  - `Pool` holds `[Option<SoundEntry>; 128]`, `[u16; 128]` generations and the `ArenaWriter`.
  - `pool_store` encodes into the gap through a `RecordWriter` over `bytes_mut`, verifies with a decode, then takes the entry.
  - A load builds into the arena only in pass 2. On any pass-2 error, the existing NEW fallback (`reset_new`) also resets the arena.
  - `memory_budget_test`'s `axi_counts_the_project` drops the slots term.

- [ ] **Step 4: Run** `just test` and `just check`. Expected: PASS, and every build links under the margin.

- [ ] **Step 5: (unit) A read-only boot on the owner's card:**
  - Run `just flash`, then `python3 tools/chimera-usb.py status` and `shot`.
  - It boots, and the last project loads. The browser lists its Sounds, and the meter reads its percentage.
  - **No save, no quick-save (MENU hold), no `card` command.** Project saves on the unit wait for #360 on `main` (#359 is).

- [ ] **Step 6: Commit** with `git commit -m "Pool on the arena: 128 slots, sparse Sounds, encoded marks"`.

### Task 6: The meter, row costs and refusals

**Files:**
- Create: `chimera-core/src/ui/meter.rs`
- Modify: `chimera-core/src/ui/browser.rs` (the info line becomes the bar; row costs), `chimera-core/src/ui/mod.rs` (the refusal band), `chimera-core/src/project/marks.rs` (`part_actions` offers `NewSlot` and `OverSlot` only when they fit), `chimera-core/src/ui/settings/tree.rs` (PROJECT › POOL leaf), `chimera-core/src/ui/settings/leaves.rs`, the hint table (`ui::hint` call sites in `browser.rs`)
- Test: `chimera-core/tests/meter_test.rs` (new), `browser_test.rs`, `hint_test.rs`, `settings_project_test.rs`, `screen_golden_test.rs` (new snapshots)

**Interfaces:**
- Consumes: `Pool::used`, `free`, `fits` and `EncodedSound::cost` (Task 5).
- Produces:
  - `pub struct MeterView { sounds: Cost, sources: Cost, pending: Cost, preview: Option<Cost> }`
  - `pub fn percent(used: Cost) -> u8`
  - `pub fn draw<D>(d: &mut D, v: &MeterView, x: i32, y: i32)` (216 × 4 px)
  - `MeterView::of(pool: &Pool, preview: Option<Cost>)`

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn meter_percent() { assert_eq!(percent(Cost(0)), 0); assert_eq!(percent(Cost(16)), 1); assert_eq!(percent(Cost(ARENA − 16)), 100); assert_eq!(percent(Cost(ARENA/2)), 50); }
#[test] fn new_reads_its_percent() // NEW's browser line draws "{percent}%" right-aligned and no count text
#[test] fn preview_alert_when_it_wont_fit() // preview > free: ALERT pixels from the used edge to x+216
#[test] fn save_over_growth_refused_keeps_part() // Review Focus 2: fill to free < growth; MIX+EDIT: band reads "POOL FULL · NEEDS …"; part sound bits unchanged; project_crc unchanged
#[test] fn clear_hint_names_bytes()   // filled slot: "MIX + MINUS CLEARS · FREES 0.3K"
#[test] fn every_pool_hint_fits()     // hint::width ≤ 216 for this task's lines at their longest (999K): POOL FULL, CLEARS · FREES, EDIT SAVES, WON'T FIT. Task 8 adds UNUSED and PURGE to the same test
#[test] fn pool_leaf_lists_breakdown() // SETTINGS › PROJECT › POOL: bar, "SOUNDS n · xK", "FREE yK"
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -p chimera-core --test meter_test`. Expected: FAIL (no module).

- [ ] **Step 3: Implement it.** Draw the four segments in the theme tokens from Global Constraints. The bar is a readout of pool state, not a parameter, so it redraws at its new width with no lerp, as the CPU bar does. The refusal band persists until the next input (ADR 0017's convention). Record the snapshots with `just screens`.

- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.

- [ ] **Step 5: Commit** with `git commit -m "One pool meter, row costs, POOL FULL refusals"`.

### Task 7: NEW from a sparse factory blob

**Files:**
- Create: `chimera-core/src/factory.bin` (generated by the test below, no separate tool)
- Modify: `chimera-core/src/factory.rs` (the blob reader replaces the code-built Sounds; the authored Sounds move under `#[cfg(any(test, feature = "test-support"))]` for the check), `chimera-core/src/project/template.rs` (`Project::fill_new` copies the blob)
- Test: `chimera-core/tests/factory_test.rs`

**Interfaces:**
- Produces:
  - `pub static FACTORY_BLOB: &[u8]`
  - `pub fn factory_entries() -> impl Iterator<Item = (SoundName, &'static [u8])>`

  The blob format is a `u16` count, then a `u16` length followed by the name (16 B) and records for each entry.

- [ ] **Step 1: Write the failing tests.** `blob_matches_authored`: every entry decodes `bits_eq` to its authored Sound, and with `GOLDEN_RECORD=1` (the golden convention, `tests/common/golden.rs`) it writes `factory.bin` from the authored Sounds. `just test` is the drift check. `new_copies_without_decode`: NEW's slot bytes `==` the blob's bytes. `new_is_pristine`: `project_status == Pristine`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test factory_test`. Expected: FAIL.
- [ ] **Step 3: Implement it.** Run `GOLDEN_RECORD=1 cargo test -p chimera-core --features test-support --test factory_test` to write the blob: today's 8 plus 2 INITs, and #317 swaps in the curated 32. `Project::fill_new` copies its bytes into the arena. Record the flash delta from `rust-size` on `bench,orbit` in the commit message.
- [ ] **Step 4: Run** `just test` and `just check`. Expected: PASS.
- [ ] **Step 5: Commit** with `git commit -m "NEW copies a sparse factory blob"`.

### Task 8: Sources in RAM: table, gate, retirement, purge

**Files:**
- Create: `chimera-core/src/project/sources.rs` (`SourceId`, `SourceRef` and its `SourceRef` record codec, tag `0x0016`, inside a Sound; `SourceEntry`, `source_users`), `chimera-core/src/source_table.rs` (`SourceTable`, `SourceRow`, gate and `sounding`)
- Modify: `chimera-core/src/lib.rs` (`pub mod source_table;`), `chimera-core/src/storage/record.rs` (`RecordTag::SourceRef`), `chimera-core/src/storage/sound.rs` (read and write `SourceRef` in a Sound), `chimera-core/src/project/pool.rs` (the source slots), `chimera-core/src/project/codec.rs` (pass 1's limit, `cost_slot` at ~469), `chimera-core/src/ui/settings/tree.rs` (`Act::PurgeUnusedSources` under PROJECT), `chimera-core/src/ui/settings/prompt.rs`
- Test: `chimera-core/tests/source_pool_test.rs`

**Interfaces:**
- Consumes: `ArenaWriter::source_*` (Task 4); `Pool` (Task 5); `MeterView` (Task 6).
- Produces:
  - `pub struct SourceId(u8)` (< 32)
  - `pub struct SourceRef { pub crc: u32, pub frames: u32, pub rate: u16 }`, with `to_bytes(&self) -> [u8; 10]` and `from_bytes(&[u8]) -> Option<SourceRef>`. This is its only definition.
  - `Pool::source_store(SourceRef, held: &[u8]) -> Result<SourceId, PoolFull>`
  - `Pool::source_absent(SourceRef) -> SourceId` (cost 0)
  - `pub struct Users { pub parts: PartSet, pub sounds: [u64; 2] }`, with one bit per `SlotId` and `is_empty()`
  - `pub fn source_users(p: &Project, s: SourceId) -> Users`
  - `Project::purge_unused_sources(&mut self, sounding: u32) -> Purged { freed: Cost, retired: u8 }`
  - `SourceTable::publish(&self, id, base, held, rate)`, `close_gate(&self) -> u32` (the epoch), `gate_acked(&self, epoch) -> bool`, `open_gate(&self)`, `sounding(&self) -> u32`, and on the audio side `begin_block(&self) -> GateView`
  - `GateView::bytes(&self, id: SourceId) -> Option<&[u8]>`: the row's `held` bytes from `base`. `None` while a closed gate moves `id`, or while the row is empty. It reads only through the `SourceTable`, under the `SourceReader`'s `// SAFETY:` argument (ADR 0087).
  - `SourceTable::store_sounding(&self, mask: u32)`: the audio's once-a-block store (`Release`). It is the writer `sounding(&self) -> u32` reads.
  - `Pool::source_used() -> Cost`: the bytes the sources hold, pending included.
  - Pass 1's limit: `ProjectDecoder` costs slots against `ARENA_BYTES − pool.source_used()`, read from the target pool before pass 1, not against the whole `ARENA_BYTES` (`codec.rs` ~469). Once sources live in the arena the whole arena is wrong: pass 1 passes, then pass 2 hits `PoolFull` and falls back to NEW after the user confirmed.

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn choir_orphan()  // slot 5 CHOIR → source CHOIR; Part 2 loads it, then another Sound; clear hint names "CHOIR 41K UNUSED"; after clear, source_users empty, purge frees exactly CHOIR's cost; before the clear purge frees 0; with Part 2 still on CHOIR, clear frees the Sound only and the hint has no UNUSED
#[test] fn purge_recounts()  // 500 seeded sequences of clears / Part loads / purges: freed == model's unreferenced sum at that moment
#[test] fn pending_hole_resolves_when_note_ends() // Review Focus 3: purge with sounding bit set → pending > 0 on the meter; clear the bit, run the UI pass: pending 0, gap grew by the cost, no second purge
#[test] fn gate_recheck_catches_late_note_on() // note-on on a source in `above` in the block before close: post-ack re-check sees its bit, reopens, moves nothing; after the note ends compaction runs; the voice's samples equal the unmoved source throughout
#[test] fn gate_holds_note_on() // closed gate: a note-on on a moving source starts on the first block after open, first sample unchanged; others start at once
#[test] fn retire_never_cuts()  // a retired source's row stays live until its sounding bit clears
#[test] fn source_ref_round_trips() // to_bytes/from_bytes; a Sound with a SourceRef encodes and decodes bits_eq; a 9-byte payload is Bounds
#[test] fn gate_view_bytes_none_while_moving() // a closed gate on `id`: bytes(id) == None, bytes(other) == Some(held)
#[test] fn store_sounding_is_read() // store_sounding(m), then sounding() == m
#[test] fn pass_one_counts_the_sources() // sources hold S: a project whose slots cost ARENA_BYTES − S + 16 is refused in pass 1 (Bounds, nothing written, project unchanged, no NEW fallback); at ARENA_BYTES − S it loads and pass 2 never sees PoolFull
// hint_test.rs: every_pool_hint_fits gains "MIX + MINUS CLEARS · {16-char NAME} 999K UNUSED" and "PURGE 32 SOURCES? FREES 999K"
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test source_pool_test`. Expected: FAIL.

- [ ] **Step 3: Implement it.**
  - The sequence is ADR 0087's: wait for `sounding & above == 0`; close the gate; wait for the ack; re-check; then move or reopen; `copy_within`; rewrite the rows; open with `Release`.
  - A voice latches its `SourceId` at note-on. Until TAPE exists, a stub reader in the test plays the voice side; the stub in `gate_recheck_catches_late_note_on` reads through `GateView::bytes` and reports through `store_sounding`.
  - The loader reads `pool.source_used()` before pass 1 and hands the decoder its limit; `cost_slot` refuses past it.
  - `source_users` is computed, never stored.

- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.

- [ ] **Step 5: Commit** with `git commit -m "Pool sources: gated compaction, retirement, purge unused"`.

### Task 8b: The lost-cluster sweep (#360), and #362 at its source (unit)

#360 (private issue #360): a power cut mid-save can leave a new file's clusters marked used with no directory entry. #362 (private issue #362) is the device-error variant: `alloc` marks the new cluster end-of-chain before it links the previous one, so when the second sector's flush fails, `drop_chain` stops at the old tail and strands the new cluster. Detection exists only in the tests (`fat_check`, `chimera-fat/tests/common/image.rs:598`). This task makes it a bounded sweep in `chimera-fat`, run once at boot.

**Decisions:**
- **#362 is fixed at its source, not by reordering.** Writing EOC before the link is what keeps a *reachable* chain safe: `grow` extends a live directory, and linking first would let a cut leave a live chain pointing at a free cluster, which the next `alloc` cross-links. So `alloc` keeps its order. Instead `Table` remembers the cluster it last allocated (`fresh`) until that link flushes. `drop_chain(start)` frees the chain from `start`, then frees `fresh` too if the card still marks it EOC. Its precondition is that no entry holds the chain, so `fresh` is the dropped chain's own new cluster: if the link landed, the walk already freed it and it reads FREE. A refused card keeps `(start, fresh)` as the orphan for the next `heal`.
- **The sweep runs at boot only.** A power cut is always followed by a boot, so the first mount after power-on covers #360. Device errors mid-session are #362's fix and the orphan. A card swapped in from a computer gets no sweep: its own disk check is the computer's job.
- **Reachability needs a bitmap, one bit a cluster, which the store can't hold** (ADR 0048: 2 KB). At boot the pool's arena gap is unused, so the firmware lends it: 157 KiB with NEW loaded holds 1.28 M clusters, more than a 32 GB card's at 32 KB clusters (1 M). The host lends a `Vec`. A volume with more clusters than bits is an abort, not a slower path.
- **One operation, under the card deadlines** (`SD_IDLE_MS` 600, `SD_OP_CAP_MS` 10 s, `chimera-stm32/src/sd.rs:34`). Mark reads every directory and each chain; count reads FAT 1 once (ADR 0048: about 3 s for a 32 GB card's 4 MB FAT, about 1.5 s for the owner's 15 GB card). A timeout in either phase frees nothing. Free writes only the FAT sectors that hold lost clusters, FAT 1 then the other copies, as every change does; it is never interrupted by a check, because every cluster it frees was proven unreachable before its first write.
- **Freeing waits for the owner.** The firmware runs `Sweep::DryRun` only, until the owner says go. `Sweep::Free` is behind the `chimera-stm32` cargo feature `sweep-free`, which no `just` recipe sets.

**Abort rules.** The sweep must never free a reachable cluster, so any doubt ends it with `Err(Doubt)` before the first write, and nothing is freed:
1. **FAT copies.** FAT 2 differs from FAT 1 in more than one sector. A cut can leave exactly one, the sector whose FAT 1 write landed and FAT 2's didn't; FAT 1 is the truth, as `Table` reads only FAT 1. In `Free` that one sector is copied from FAT 1 first (`heal`'s path); `DryRun` reports it.
2. **An unreadable directory.** Any device error reading a directory block or a FAT sector.
3. **A broken directory.** A directory chain with a free, reserved, bad or out-of-range link, or a loop; a `.`/`..` pair that names the wrong clusters; a subdirectory entry with start 0.
4. **A broken file.** A file whose chain breaks before its end mark, or is shorter than its length (`fat_check`'s `short`).
5. **A cross-link.** Any cluster reached twice while marking.
6. **Too deep or too many.** The walk covers **every** directory on the volume, not only `/CHIMERA`: the card may hold the stock firmware's folders and a computer's files. Deeper than `MAX_DEPTH = 8`, or more than `MAX_DIRS = 256` directories, is an abort.
7. **No room.** More clusters than the bitmap's bits.
8. **Time.** The deadline passes (the store's `Timeout`), or the volume id changes (`VolumeChanged`).
9. **A lost cluster's own value.** Only a cluster whose FAT 1 value is an end mark or an in-range link counts as lost (`fat_check`'s rule). Bad (`0x…FFF7`) and reserved values stay.

**Files:**
- Create: `chimera-fat/src/sweep.rs` (mark, count, free; the abort rules), `chimera-fat/tests/sweep_test.rs`
- Modify:
  - `chimera-fat/src/fat.rs`: `Table` keeps `fresh: Option<u32>`, which `alloc` sets and a flush that lands clears; `drop_chain` and `heal` free it; `FatCache::orphan` becomes `Option<(u32, Option<u32>)>`;
  - `chimera-fat/src/store.rs`: `FatStore::sweep`, the orphan's new type;
  - `chimera-fat/src/lib.rs` (the module);
  - `chimera-hal/src/store.rs`: `Sweep`, `Swept`, `Doubt`, and `Store::sweep` with a default that sweeps nothing (`MemStore` has no FAT);
  - `chimera-stm32/src/main.rs` (before `ui.boot_project` at ~338: the dry run, the arena gap lent), `chimera-stm32/Cargo.toml` (`sweep-free = []`), `chimera-core/src/console/status.rs` (`lost {n}`, or `lost ? {doubt}`);
  - `chimera-core/src/project/arena.rs`: `ArenaWriter::gap_mut(&mut self) -> &mut [u8]`, UI-only, its `// SAFETY:` comment as `bytes_mut`'s;
  - `docs/adr/`: an ADR for the sweep (boot-only, the bitmap in the arena gap, the abort rules, dry run first), added to `docs/adr/README.md`.
- Test: `chimera-fat/tests/sweep_test.rs` (new); `pool_card_test.rs`, `power_cut_test.rs`, `console_put_test.rs` (each cut and each device error now ends with a sweep); `dosfstools_test.rs`; `chimera-fat/tests/common/image.rs` (a `swept(&disk)` helper: mount, `Sweep::Free`, then `fat_check`).

**Interfaces:**
- Produces:
  - `pub enum Sweep { DryRun, Free }`
  - `pub struct Swept { pub lost: u32, pub freed: u32, pub healed: bool }`: `freed` is 0 in `DryRun`, and `healed` says rule 1's one sector was copied (or, in `DryRun`, would be)
  - `pub enum Doubt { FatsDiffer, Unreadable, DirBroken, FileShort, CrossLinked, TooDeep, NoRoom, Card(StoreError) }`
  - `fn Store::sweep(&mut self, vol: VolumeId, mode: Sweep, marks: &mut [u8]) -> Result<Swept, Doubt>`
  - `pub const MAX_DEPTH: usize = 8`, `pub const MAX_DIRS: usize = 256`
  - `drop_chain(start)` frees the stranded `fresh` cluster as well (#362)

- [ ] **Step 1: Write the failing tests:**

```rust
// sweep_test.rs
#[test] fn sweep_frees_every_cut_leak()   // a 128-slot project save cut at every write (Cut::After(k)), then swept: lost == [], cross_linked == [], short == [], fats_differ == []; every entry's chain and bytes as before the sweep; freed == fat_check's lost before it
#[test] fn dry_run_writes_nothing()       // on each cut image: DryRun's lost == fat_check's lost.len(); the probe log has no write; the image is byte-identical
#[test] fn sweep_is_idempotent()          // a second Free: freed == 0, no write
#[test] fn sweep_keeps_foreign_trees()    // a card with a computer's tree (pfm3/ three deep, files beside CHIMERA) and 5 lost clusters: freed == 5; every foreign file reads identical
#[test] fn sweep_aborts_on_doubt()        // one case per abort rule 1–8: Err(the rule's Doubt), no write, image byte-identical; rule 1 with one stale sector is not a doubt: Ok, healed
#[test] fn sweep_read_error_frees_nothing() // Log::fail_reads, and Log::absent, armed as the sweep starts: Err(Unreadable or Card), no write
#[test] fn sweep_write_error_is_safe()    // Log::fail_write and Log::land_then_fail at each of the sweep's writes: no cross_linked, no short, entries intact, lost ⊆ before; a second sweep then leaves lost == []
#[test] fn sweep_cut_is_safe()            // Cut::After(k) at each of the sweep's writes: the same, after the next boot's sweep
// fat_core_test.rs
#[test] fn drop_chain_frees_the_stranded_cluster() // alloc across a FAT sector, the link's flush fails: drop_chain leaves fat_check's lost == []
// pool_card_test.rs
// sector_crossing_fat2_error_loses_a_cluster: un-ignored, renamed sector_crossing_fat2_error_loses_nothing, and passes on #362's fix alone, with no sweep
// dosfstools_test.rs (ignored, as its others)
#[test] fn swept_images_pass_fsck()      // every cut image, swept: fsck.fat -n reports no lost cluster
```

The existing cut and device-error tests in `pool_card_test`, `power_cut_test` and `console_put_test` end with `swept(&disk)`, and assert `lost == []` where they asserted "unchanged unless the put_entry".

- [ ] **Step 2: Run** `cargo test -p chimera-fat --features chimera-hal/testkit --test sweep_test --test fat_core_test --test pool_card_test`. Expected: FAIL (no `sweep`; the #362 test strands cluster 256).

- [ ] **Step 3: Implement it.**
  - #362 first, on its own: `fresh`, `drop_chain`, the orphan pair. `sector_crossing_fat2_error_loses_nothing` passes.
  - Mark: from the root (FAT32: its chain from the BPB), an explicit stack of at most `MAX_DEPTH` `(dir start, block)` frames walks every directory; each entry's chain sets its bits. A bit already set is `CrossLinked`. LFN, volume-label and deleted entries are skipped; `.` and `..` are checked, not followed.
  - Count: one pass over FAT 1, reading each sector once; a lost cluster is rule 9's and its bit is clear.
  - Free (`Sweep::Free` only): `Table::set(c, FREE)` for each, sector by sector, flushed to every copy; FSInfo's free count is marked unknown, as every change does (ADR 0048).
  - The firmware: `ui.boot_project` comes after `store.sweep(vol, Sweep::DryRun, arena.gap_mut())`, whose result `status` reports. A `Doubt` is logged and the boot goes on.

- [ ] **Step 4: Run** `just test` (it runs `test-fat-tools`), `just clippy` and `just check`. Expected: PASS, and no `just` recipe enables `sweep-free`.

- [ ] **Step 5: (unit) A read-only boot, dry run:**
  - Run `just flash`, then `python3 tools/chimera-usb.py status` and `shot`.
  - `status` reports `lost {n}`: what the sweep *would* free on the owner's card, and how long it took. Record both in the commit message. A `Doubt` is recorded the same way, with its rule.
  - Nothing is written. Real freeing on the unit, and `sweep-free`, wait for the owner's go.

- [ ] **Step 6: Commit** with `git add chimera-fat/src/sweep.rs chimera-fat/src/fat.rs chimera-fat/src/store.rs chimera-fat/src/lib.rs chimera-hal/src/store.rs chimera-core/src/project/arena.rs chimera-stm32/src/main.rs chimera-stm32/Cargo.toml chimera-core/src/console/status.rs chimera-fat/tests docs/adr`, then `git commit -m "A boot sweep frees lost clusters, dry run on the unit; drop_chain frees the stranded cluster"`.

### Task 9: Source files on the card and `PoolSource` records

**Precondition:** met (console-put on `main`). It uses #359's chain free and `writable()` as they are. Task 8b delivers #360's sweep on this branch, but the writer ships only once #360 is on `main`. Until then the writer (`write_source`, `Store::seal`'s caller) stays behind the `chimera-core` cargo feature `source-writer`, host-only. The firmware never enables it until #360 is on `main` (TAPE Task 18 Step 6). `chimera-core`'s own tests enable it through its dev-dependency features, and `chimera-desktop` enables it (TAPE Task 11). Nothing ships that writes `/CHIMERA/TAPES`.

**TAPE Task 2 first.** It owns `storage/twelve.rs` and `Shape` (`dsp/tape_engine/read.rs`), and has landed on `tape-spec`. Until it is on this branch, cherry-pick its two code commits, in order: `git cherry-pick 79c111f6 9aa9ba2c` (`TAPE read: descriptor, 12-bit decode, Hermite`; `TAPE read: halfword taps, no bounds checks; tighter descriptor and response tests`). A conflict in `tests/common/mod.rs` keeps both `mod` lines. The rest of 79c111f6..45a5f929 is docs and stays on `tape-spec`. Task 9 writes no `twelve.rs` of its own.

**Files:**
- Create: `chimera-core/src/storage/source_file.rs` (`FileKind::Source` read and write, `SourceHead`/`SourceData`). Conversion, decimation and dither are host-only (ADR 0093), and are not in `chimera-core`.
- Consumes: `storage/twelve.rs` (TAPE Task 2).
- Modify:
  - `chimera-hal/src/store.rs`: `Dir::Tapes`, and `Store::seal(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError>`, which sets the FAT read-only attribute;
  - `chimera-fat/src/fs.rs`: `seal` writes the entry's attribute byte through `put_entry`, and `add_dir` makes `/CHIMERA/TAPES` as it does `SOUNDS`;
  - `chimera-core/src/storage/frame.rs` (`FileKind::Source = 6`);
  - `chimera-core/src/storage/file.rs` (`SaveError::Damaged`);
  - `chimera-core/src/storage/record.rs` (`PoolSource` `0x8015`, `SourceHead` `0x8017`, `SourceData` `0x0018`);
  - `chimera-core/src/project/codec.rs` (`PoolSource` records, pass-1 costs, over-budget order);
  - `chimera-core/Cargo.toml` (`source-writer = []`, and the dev-dependency's `features` gains `source-writer`).
- Test: `chimera-core/tests/source_codec_test.rs`, `chimera-fat/tests/source_card_test.rs`

**Interfaces:**
- Consumes:
  - `SourceRef` and its codec, `Pool::source_store` and `source_absent` (Task 8); `writable()` and `StoreError::ReadOnly` (console-put).
  - `pub fn pack12(samples: &[i16], out: &mut [u8]) -> usize` and `pub fn unpack12(bytes: &[u8], pair: usize) -> (i16, i16)` (TAPE Task 2).
  - `Shape` (`dsp::tape_engine::read::Shape`, TAPE Task 2).
- Produces:
  - `pub struct SourceHead { pub sref: SourceRef, pub shape: Shape, pub count: u8, pub root: u16, pub period_q16: u32, pub name: SoundName }`, with `to_bytes(&self) -> [u8; 34]` and `from_bytes(&[u8]) -> Option<SourceHead>`. The layout is ADR 0093's: `SourceRef` 10, `shape` 1, `count` 1, `root` 2 (MIDI cents), `period` 4 (Q16.16; 0 is unpitched), name 16. `Shape` is 0 `Plain`, 1 `Transwave`, 2 `ZoneSet`.
  - `pub fn source_crc(shape: Shape, count: u8, packed: &[u8]) -> u32`: over `shape`, `count` and the packed bytes. Never the name, the root or the period. This is the file's stem. TAPE Task 10 folds a zone set's layout in after `count`, for `ZoneSet` only, so a `Plain` or `Transwave` CRC never changes.
  - `pub fn source_file(r: SourceRef) -> FileName` (`Dir::Tapes`, stem = CRC hex, ext `SRC`)
  - `pub fn read_source_into<S: Store>(s, r: &Ready, sref: SourceRef, out: &mut [u8]) -> Result<usize, LoadError>`, which takes and checks the `SourceHead`: its frames and rate against `sref`, as now
  - `pub fn write_source<S: Store>(s: &mut S, r: &Ready, head: &SourceHead, data: &[u8]) -> Result<Wrote, SaveError>`, where `enum Wrote { New, Present, Replaced }`. It writes, reads back, checks the head and CRC, then seals.
    - **Present:** a sealed file that verifies; nothing is written.
    - **Replaced:** an unsealed file, the torn remains of a cut write, which is deleted and written again.
    - A sealed file that fails its check is `Err(SaveError::Damaged)`, with nothing written: `writable()` refuses it anyway.

- [ ] **Step 1: Write the failing tests:**

```rust
// source_codec_test.rs (12-bit packing is TAPE Task 2's twelve_test)
#[test] fn head_round_trips()            // 34 B, every Shape
#[test] fn crc_ignores_labels()          // name, root and period changed → the same CRC; shape, count or one byte → a different one
#[test] fn pool_source_round_trips()     // save → load byte-equal; file contains no SourceData record
#[test] fn over_budget_loads_in_order()  // Part-used sources, then slots by index, then other sources; note "LOADED · a SOUNDS, b SRC DIDN'T FIT"
#[test] fn missing_source_ref_survives_save() // Review Focus 5: file absent → absent entry, cost 0, Part silent; save → PoolSource bytes identical
// chimera-fat/tests/source_card_test.rs
#[test] fn truncated_source_reads_missing()   // Review Focus 4: a source write cut at every block: the ref reads missing, never shorter audio; no cross_linked, no short
#[test] fn card_swap_between_passes_reads_missing() // VolumeChanged between pass 1 and 2: load refuses as today; no source bytes enter the arena
#[test] fn source_write_device_error_no_leak() // Log::fail_write and Log::land_then_fail at each write: no cross_linked, no short; `lost` unchanged unless the failed write is the put_entry (#359's rule)
#[test] fn source_write_power_cut_safe()       // Cut::After(k) at each write: no cross_linked, no short. Lost clusters are #360's sweep (private issue #360), not asserted here
#[test] fn source_is_write_once()              // a sealed source: write_source returns Present and writes no block; a raw Store::write to it is Err(ReadOnly), file unchanged
#[test] fn torn_source_is_replaced()           // cut the first write at each block, then write_source again: Replaced (or New if no entry landed), reads back verified, sealed; no cross_linked, no short
#[test] fn quick_save_writes_no_audio()        // a project with 3 sources: MENU-hold save's bytes written independent of source sizes
#[test] fn same_audio_new_root_is_present()    // write_source, then again with the root moved: Present, one file
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test source_codec_test` and `cargo test -p chimera-fat --features chimera-hal/testkit --test source_card_test`. Expected: FAIL.

- [ ] **Step 3: Implement it.**
  - Pass 1 sums costs from `PoolSource` frames and the model's `held`, with no card read.
  - Pass 2 reads sources inside the existing epoch fade (ADR 0046).
  - A header that mismatches the ref, or a short read, makes an absent entry.

- [ ] **Step 4: Run** `just test`, `just clippy`, `just check` (no firmware feature set enables `source-writer`) and `cargo build -p chimera-core --features source-writer`. Expected: PASS.

- [ ] **Step 5: Commit** with `git commit -m "Card sources: write-once TAPES files, PoolSource refs, 12-bit packing"`.

---

**Order and gates:**
- **Tasks 1–4 are done.** console-put is merged and the branch rebased, so Tasks 5–9 can run. Task 3 must land before Task 5 links.
- Task 8b delivers #360 (and #362) on this branch, before Task 9. Task 9 still ships its writer to the firmware only once #360 is on `main`: `source-writer` stays host-only until then.
- **(unit)** marks Tasks 1, 5 and 8b. Task 5's check is a read-only boot, and 8b's is a read-only boot with the sweep's dry run; real freeing on the unit waits for the owner's go. Task 9's card behaviour is proven on the host through the harnesses. Its first on-unit run goes with TAPE's first voice (#279), after #360 is on `main` (#359 is).
