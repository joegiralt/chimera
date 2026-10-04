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
- **Arena constants:** `ARENA_BYTES = 160 * 1024`, `GRANULE = 16`, `MAX_SOUNDS = 128`, `MAX_SOURCES = 32`, `MAX_SOUND_COST = 1824`.
- **AXI margin:** a linker `ASSERT` keeps the gap between `__sheap` and the end of AXI at ≥ 32 KiB on shipping builds and ≥ 8 KiB on `bench` builds. It must hold on every feature set `just check` builds.
- **Delay line (ADR 0090):** `i16`, full scale ±2.0, write `(x * 16384.0) as i16` (truncates toward zero, saturates), `MAX_DELAY_SAMPLES` unchanged at 24 064.
- **Sparse Sounds:** `encode_sound` writes only params whose disk value differs from `Sound::neutral(engine)`. `neutral` is a format contract: a change to it needs a migration in the same commit.
- **Record tags** (frozen once written): `PoolSource` `0x8015`, `SourceRef` `0x0016`, `SourceHead` `0x8017`, `SourceData` `0x0018`; `FileKind::Source = 6`; `Dir::Tapes` = `/CHIMERA/TAPES`; `FORMAT_VERSION` stays 1.
- **`SourceRef`:** `{ crc: u32, frames: u32, rate: u16 }`, 10 B on disk. A source file's 8.3 stem is its CRC in upper-case hex.
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
    - the device-error probe (`probe::Log::fail_write`, and `land_then_fail` once console-put merges).
  - Card writes that can leave lost clusters wait on #359 (device-error chain leak) and #360 (the power-cut lost-cluster sweep). See Task 9.
- **Verification:** `just test`, `just clippy`, `just check` (every firmware feature set). On-unit checks are marked **(unit)**, and use a scratch card, never the owner's sealed card.

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
#[test] fn delay_line_is_i16() { assert_eq!(size_of::<[i16; MAX_DELAY_SAMPLES]>(), 48_128); } // and FxBus shrinks by 48_128 from 159_784
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
  - `pub const MAX_SOUND_COST: usize = 1824`
  - `encode_sound` keeps its signature and skips a block whose count is 0

- [ ] **Step 1: Write the failing tests** in `sound_codec_test.rs`:

```rust
#[test] fn sparse_round_trip()      // factory 0..8, INIT ×2, 1000 seeded random Sounds: pass(decode, encode(s)).bits_eq(s)
#[test] fn sparse_byte_identity()   // encode(decode(encode(s))) == encode(s); and encode(decode(fixture v1 .snd)) == encode(original)
#[test] fn sparse_is_canonical()    // for 1000 pairs (s, s with one param moved one step): (encode(a) == encode(b)) == a.bits_eq(&b)
#[test] fn sparse_sizes()           // factory_0 encodes to ≤ 273 B of records + 16; INIT to 58 − 16 = 42 B of records
#[test] fn sparse_bound()           // every param off neutral, full registry, 128 routes: 16 + records ≤ MAX_SOUND_COST
#[test] fn neutral_is_a_contract()  // Sound::neutral matches fixtures/neutral_v1.txt; message: "changing neutral needs a migration (ADR 0087)"
```

The random generator draws each param uniformly from its spec range, quantised as its spec does, with 0–128 routes and 0–16 registry entries, from `XorShift`.

- [ ] **Step 2: Run them to see them fail.** Run `cargo test -p chimera-core --test sound_codec_test`. Expected: FAIL in `sparse_sizes` and `sparse_bound`.

- [ ] **Step 3: Implement it.** Build `Sound::neutral(s.engine())` once in `encode_sound`, and pass each block's neutral counterpart as `base`. Compare `read_value` results (`DiskValue`) for equality, by bits for reals.

- [ ] **Step 4: Run the tests and the card harnesses.** Run `cargo test -p chimera-core` and `cargo test -p chimera-fat --features chimera-hal/testkit --test power_cut_test --test fat_store_test`. Expected: PASS, with no change to the cut assertions.

- [ ] **Step 5: Commit.** Run `git add` on the files above, then `git commit -m "Sounds encode sparse against neutral"`.

### Task 3: The AXI margin in the linker

**Files:**
- Modify: `chimera-stm32/build.rs` (write `axi_margin.x`, add `-Taxi_margin.x`)
- Modify: `chimera-core/src/hw.rs` (delete `UI_RESERVE`)
- Modify: `chimera-core/src/instrument.rs:37-48` (`AXI_RESIDENT` and its two asserts)
- Test: `chimera-core/tests/memory_budget_test.rs` (`axi_residents_fit` → `axi_report`, `ui_state_fits_the_ui_reserve` keeps its 4 KiB cap)

**Interfaces:**
- Produces:
  - the linker symbol `AXI_MARGIN`: 32 KiB, or 8 KiB when `CARGO_FEATURE_BENCH` is set
  - `ASSERT(ORIGIN(RAM) + LENGTH(RAM) - __sheap >= AXI_MARGIN, "AXI: under the margin (ADR 0087)")`

- [ ] **Step 1: Write the failing check.** In `axi_margin.x`, set the margin temporarily to `400K`.

- [ ] **Step 2: Run it.** Run `cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`. Expected: a link error, `AXI: under the margin`.

- [ ] **Step 3: Implement it.** Use the real margins. Delete `UI_RESERVE`, the `AXI_RESIDENT` const assert and the `≥ 64 KiB` assert. `axi_report` prints the parts and asserts nothing about spare.

- [ ] **Step 4: Verify.** Run `just check` and `just test`. Expected: every feature set links. Record `rust-size` free AXI for `orbit` (140 024 today) and `bench,master-tape` (104 188) in the commit message.

- [ ] **Step 5: Commit** with `git commit -m "AXI margin enforced by the linker, not a paper sum"`.

### Task 4: The arena

**Files:**
- Create: `chimera-core/src/project/arena.rs`
- Modify: `chimera-core/src/project/mod.rs` (the module)
- Test: `chimera-core/tests/arena_test.rs`

**Interfaces:**
- Produces:
  - `pub struct Cost(u32)` with `Cost::of(len: usize) -> Cost` (16 B granule, rounded up), `bytes() -> u32`, and `kib_label() -> FmtBuf` (`0.3K`, `41K`)
  - `pub struct ArenaWriter`, built by `ArenaWriter::new(bytes: &'static mut [u8]) -> ArenaWriter` (the static) or `ArenaWriter::boxed() -> ArenaWriter` (tests and desktop), with:
    - `sound_alloc(len) -> Option<u32>`
    - `sound_free(offset, len)`, which compacts at once and returns the moves as `(old, new)` pairs
    - `source_alloc(len) -> Option<u32>`
    - `source_free_pending(offset, len)`
    - `source_compact(&mut self, ready: impl Fn(u32 /*offset*/) -> bool) -> Moves`
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
- Modify: `chimera-core/src/project/pool.rs` (rewrite), `ids.rs` (`SlotId` < 128), `marks.rs` (encoded compare), `mod.rs` (`replace_part`, `pool_store`, `pool_clear`), `codec.rs` (`Slot` index 0..128, slots from the arena), `template.rs` (`fill_new_pool`)
- Modify: `chimera-core/src/preset.rs` (`POOL_SIZE` → `MAX_SOUNDS = 128`), `chimera-core/src/ui/browser.rs` (`TOTAL_ENTRIES`)
- Modify: `chimera-stm32/src/shared.rs` (the `POOL_ARENA` static, its `ArenaWriter` taken once), `chimera-desktop/src/main.rs` and `chimera-desktop/src/qa.rs` (a boxed arena), `chimera-stm32/src/bench.rs` (its `Project`)
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
#[test] fn load_device_error_leaves_new_arena() // fail_write/fail_reads mid pass 2: load errs; project == NEW; pool.used() == NEW's; gap == ARENA_BYTES − used
#[test] fn project_save_cut_at_every_write()    // power_cut pattern: cut at each write of a 128-slot project save: kept side loads, no cross_linked, no short
#[test] fn project_save_device_error_frees()    // fail_write on each data block: fat_check shows no new lost chain once #359's fix is on main (until then: no cross_linked, no short)
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -p chimera-core --test project_model_test` and `cargo test -p chimera-fat --features chimera-hal/testkit --test pool_card_test`. Expected: FAIL to compile, since `Pool::fits` is not defined.

- [ ] **Step 3: Implement it.**
  - `Pool` holds `[Option<SoundEntry>; 128]`, `[u16; 128]` generations and the `ArenaWriter`.
  - `pool_store` encodes into the gap through a `RecordWriter` over `bytes_mut`, verifies with a decode, then takes the entry.
  - A load builds into the arena only in pass 2. On any pass-2 error, the existing NEW fallback (`reset_new`) also resets the arena.
  - `memory_budget_test`'s `axi_counts_the_project` drops the slots term.

- [ ] **Step 4: Run** `just test` and `just check`. Expected: PASS, and every build links under the margin.

- [ ] **Step 5: (unit), on a scratch card:**
  - Run `just flash`. `status` boots, and NEW shows its 10 Sounds.
  - Save a project, power-cycle, and load it. Its Parts sound as before.
  - Take the scratch card to the host and run `fsck.fat -n`: clean, with no lost clusters.

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
#[test] fn every_pool_hint_fits()     // hint::width ≤ 216 for each copy line in Global Constraints at its longest (16-char names, 999K)
#[test] fn pool_leaf_lists_breakdown() // SETTINGS › PROJECT › POOL: bar, "SOUNDS n · xK", "FREE yK"
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -p chimera-core --test meter_test`. Expected: FAIL (no module).

- [ ] **Step 3: Implement it.** Draw the four segments in the theme tokens from Global Constraints. The refusal band persists until the next input (ADR 0017's convention). Record the snapshots with `just screens`.

- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.

- [ ] **Step 5: Commit** with `git commit -m "One pool meter, row costs, POOL FULL refusals"`.

### Task 7: NEW from a sparse factory blob

**Files:**
- Create: `chimera-core/src/factory.bin` (generated), `chimera-desktop/src/bin/factory_blob.rs` (the generator)
- Modify: `chimera-core/src/factory.rs` (the blob reader replaces the code-built Sounds; the authored Sounds stay under `#[cfg(test)]` for the check), `chimera-core/src/project/template.rs` (`fill_new_pool` copies the blob)
- Test: `chimera-core/tests/factory_test.rs`

**Interfaces:**
- Produces:
  - `pub static FACTORY_BLOB: &[u8]`
  - `pub fn factory_entries() -> impl Iterator<Item = (SoundName, &'static [u8])>`

  The blob format is a `u16` count, then a `u16` length followed by the name (16 B) and records for each entry.

- [ ] **Step 1: Write the failing tests.** `blob_matches_authored`: every entry decodes `bits_eq` to its authored Sound. `new_copies_without_decode`: NEW's slot bytes `==` the blob's bytes. `new_is_pristine`: `project_status == Pristine`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test factory_test`. Expected: FAIL.
- [ ] **Step 3: Implement it.** The generator writes `factory.bin` from the authored Sounds (today's 8 plus 2 INITs; #317 swaps in the curated 32). `fill_new_pool` copies the bytes into the arena. Record the flash delta from `rust-size` on `bench,orbit` in the commit message.
- [ ] **Step 4: Run** `just test` and `just check`. Expected: PASS.
- [ ] **Step 5: Commit** with `git commit -m "NEW copies a sparse factory blob"`.

### Task 8: Sources in RAM: table, gate, retirement, purge

**Files:**
- Create: `chimera-core/src/project/sources.rs` (`SourceId`, `SourceEntry`, `source_users`), `chimera-core/src/instrument/source_table.rs` (`SourceTable`, `SourceRow`, gate and `sounding`)
- Modify: `chimera-core/src/project/pool.rs` (the source slots), `chimera-core/src/ui/settings/tree.rs` (`Act::PurgeUnusedSources` under PROJECT), `chimera-core/src/ui/settings/prompt.rs`
- Test: `chimera-core/tests/source_pool_test.rs`

**Interfaces:**
- Consumes: `ArenaWriter::source_*` (Task 4); `Pool` (Task 5); `MeterView` (Task 6).
- Produces:
  - `pub struct SourceId(u8)` (< 32)
  - `pub struct SourceRef { pub crc: u32, pub frames: u32, pub rate: u16 }`
  - `Pool::source_store(SourceRef, held: &[u8]) -> Result<SourceId, PoolFull>`
  - `Pool::source_absent(SourceRef) -> SourceId` (cost 0)
  - `pub struct Users { pub parts: PartSet, pub sounds: [u64; 2] }`, with one bit per `SlotId` and `is_empty()`
  - `pub fn source_users(p: &Project, s: SourceId) -> Users`
  - `Project::purge_unused_sources(&mut self, sounding: u32) -> Purged { freed: Cost, retired: u8 }`
  - `SourceTable::publish(&self, id, base, held, rate)`, `close_gate(&self) -> u32` (the epoch), `gate_acked(&self, epoch) -> bool`, `open_gate(&self)`, `sounding(&self) -> u32`, and on the audio side `begin_block(&self) -> GateView`

- [ ] **Step 1: Write the failing tests:**

```rust
#[test] fn choir_orphan()  // slot 5 CHOIR → source CHOIR; Part 2 loads it, then another Sound; clear hint names "CHOIR 41K UNUSED"; after clear, source_users empty, purge frees exactly CHOIR's cost; before the clear purge frees 0; with Part 2 still on CHOIR, clear frees the Sound only and the hint has no UNUSED
#[test] fn purge_recounts()  // 500 seeded sequences of clears / Part loads / purges: freed == model's unreferenced sum at that moment
#[test] fn pending_hole_resolves_when_note_ends() // Review Focus 3: purge with sounding bit set → pending > 0 on the meter; clear the bit, run the UI pass: pending 0, gap grew by the cost, no second purge
#[test] fn gate_recheck_catches_late_note_on() // note-on on a source in `above` in the block before close: post-ack re-check sees its bit, reopens, moves nothing; after the note ends compaction runs; the voice's samples equal the unmoved source throughout
#[test] fn gate_holds_note_on() // closed gate: a note-on on a moving source starts on the first block after open, first sample unchanged; others start at once
#[test] fn retire_never_cuts()  // a retired source's row stays live until its sounding bit clears
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test source_pool_test`. Expected: FAIL.

- [ ] **Step 3: Implement it.**
  - The sequence is ADR 0087's: wait for `sounding & above == 0`; close the gate; wait for the ack; re-check; then move or reopen; `copy_within`; rewrite the rows; open with `Release`.
  - A voice latches its `SourceId` at note-on. Until TAPE exists, a stub reader in the test plays the voice side.
  - `source_users` is computed, never stored.

- [ ] **Step 4: Run** `just test` and `just clippy`. Expected: PASS.

- [ ] **Step 5: Commit** with `git commit -m "Pool sources: gated compaction, retirement, purge unused"`.

### Task 9: Source files on the card, `PoolSource` and `SourceRef` records

**Precondition:** the console-put fix for #359 (a device error frees its chain) and the #360 sweep (lost clusters freed at mount) are on `main`. Until then, this task's writer stays behind `#[cfg(test)]`, and nothing ships that writes `/CHIMERA/TAPES`.

**Files:**
- Create: `chimera-core/src/storage/source_file.rs` (`FileKind::Source` read and write, `SourceHead`/`SourceData`), `chimera-core/src/storage/twelve.rs` (pack, unpack, `quantise_tpdf`)
- Modify: `chimera-hal/src/store.rs` (`Dir::Tapes`), `chimera-fat` (create `/CHIMERA/TAPES` as it does `SOUNDS`), `chimera-core/src/storage/frame.rs` (`FileKind::Source = 6`), `chimera-core/src/storage/record.rs` (the four tags), `chimera-core/src/project/codec.rs` (`PoolSource` records, pass-1 costs, over-budget order), `chimera-core/src/storage/sound.rs` (`SourceRef` inside a Sound)
- Test: `chimera-core/tests/twelve_test.rs`, `chimera-core/tests/source_codec_test.rs`, `chimera-fat/tests/source_card_test.rs`

**Interfaces:**
- Consumes: `SourceRef`, `Pool::source_store` and `source_absent` (Task 8).
- Produces:
  - `pub fn pack12(samples: &[i16], out: &mut [u8]) -> usize`
  - `pub fn unpack12(bytes: &[u8], pair: usize) -> (i16, i16)`
  - `pub struct Tpdf(u32)`, an xorshift32 seeded by the caller
  - `pub fn quantise_tpdf(x: f32, rng: &mut Tpdf) -> i16` (12-bit range, −2048..=2047)
  - `pub fn source_file(r: SourceRef) -> AbFile` (stem = CRC hex)
  - `pub fn read_source_into<S: Store>(s, r: &Ready, sref: SourceRef, out: &mut [u8]) -> Result<usize, LoadError>`, which checks the header's frames and rate against `sref`
  - `pub fn write_source<S: Store>(…, sref: SourceRef, name: SoundName, data: &[u8]) -> Result<(), SaveError>`, which refuses if the file already exists (write-once)

- [ ] **Step 1: Write the failing tests:**

```rust
// twelve_test.rs
#[test] fn pack_unpack_all_values()  // release: all 4096×4096 pairs; debug: 100 000 seeded
#[test] fn odd_tail_pads()           // 3 frames → 6 B, last sample 0
#[test] fn dither_keeps_peak_flat_floor() // −60 dBFS 1 kHz sine: peak bin within 0.5 dB; floor flat within 3 dB
// source_codec_test.rs
#[test] fn pool_source_round_trips()     // save → load byte-equal; file contains no SourceData record
#[test] fn over_budget_loads_in_order()  // Part-used sources, then slots by index, then other sources; note "LOADED · a SOUNDS, b SRC DIDN'T FIT"
#[test] fn missing_source_ref_survives_save() // Review Focus 5: file absent → absent entry, cost 0, Part silent; save → PoolSource bytes identical
// chimera-fat/tests/source_card_test.rs
#[test] fn truncated_source_reads_missing()   // Review Focus 4: a source write cut at every block: the ref reads missing, never shorter audio; no cross_linked, no short
#[test] fn card_swap_between_passes_reads_missing() // VolumeChanged between pass 1 and 2: load refuses as today; no source bytes enter the arena
#[test] fn source_write_device_error_no_leak() // fail_write and land_then_fail on each block: fat_check lost clusters unchanged (#359)
#[test] fn source_write_power_cut_swept()      // cut at each block, remount: #360's sweep leaves zero lost clusters
#[test] fn source_is_write_once()              // a second write_source to the same ref: Err, file unchanged
#[test] fn quick_save_writes_no_audio()        // a project with 3 sources: MENU-hold save's bytes written independent of source sizes
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test twelve_test --test source_codec_test` and `cargo test -p chimera-fat --features chimera-hal/testkit --test source_card_test`. Expected: FAIL.

- [ ] **Step 3: Implement it.**
  - Pass 1 sums costs from `PoolSource` frames and the model's `held`, with no card read.
  - Pass 2 reads sources inside the existing epoch fade (ADR 0046).
  - A header that mismatches the ref, or a short read, makes an absent entry.

- [ ] **Step 4: Run** `just test`, `just clippy` and `just check`. Expected: PASS.

- [ ] **Step 5: Commit** with `git commit -m "Card sources: write-once TAPES files, PoolSource refs, 12-bit packing"`.

---

**Order and gates:**
- Tasks 1 → 9 in order. Task 3 must land before Task 5 links. Task 9 waits on #359 and #360.
- **(unit)** marks Tasks 1 and 5. Task 9's card behaviour is host-proven through the harnesses. Its first on-unit run goes with TAPE's first voice (#279).
