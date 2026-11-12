# Flat engine maps (0.4) — Implementation Plan

**Goal:** A Part picks one flat controller map of up to 90 rows (control → cell, MIN, MAX, INPUT, ECHO), which plays on any engine, with the rows the engine lacks dimmed and dead. The factory set ships ALGO, MODAL, VA and TAPE maps in place of SUGGEST. The rows live in ITCM, and D2 gets back the 2.2 KB the layers held.

**Architecture:** A typed flat map (`MapLen`, `MapRowIx`, `ControllerMap` of `Binding`s) in a new bank-1 module, `chimera_engine::midi_map::flat`, replaces `midi_map::ctl`'s sections. Six per-Part slots and one staging slot sit in a NOLOAD `.itcm` section, reached through a pointer in `MidiSlot`, as `recall` is. The MIDI UI's 32 binding slots become the Sound's 16 rows plus 16 seats keyed by `MapRowIx`. A heard map row takes the least recently heard resting seat. The echo covers the seats and walks every ECHO row on a burst. The card format is `MapRows` (`0x8024`, critical) records of `Bindings` payloads, streamed both ways. Old `MapSection` files are flattened on load. `chimera-utils engine-maps` generates the four factory maps from the param tables.

**Tech Stack:** Rust no_std (`chimera-engine`, `chimera-core`, `chimera-board`, `chimera-stm32` linker scripts), host tests on `chimera-hal`'s `MemStore` (`testkit`), `chimera-utils` for the generator, `just` recipes, `tools/factory_zip.py`.

**Spec:** `docs/superpowers/specs/2026-11-12-flat-engine-maps-design.md` (owner-approved 2026-11-12; its Decisions are settled). ADR: `docs/adr/0266-flat-engine-maps-in-itcm.md` (supersedes in part 0226, supersedes 0264, amends 0217 and 0255).

## Global Constraints

- Type-driven development is mandatory. No bare primitives for units, ranges, indices, codes or modes; constructors clamp or return Option.
- No unsafe without a `// SAFETY:` comment. No heap.
- Bank 2 is full, so growth goes to bank 1 under ADR 0251, with budget lines naming it.
- Builds go through `nice -n 15` with CARGO_BUILD_JOBS=10.
- Commits are terse, with no type prefixes.
- No USB descriptor change. Hardware checks happen after merge.

From the design and the ADR:
- At most 90 rows a map (`MAP_ROWS`), six per-Part slots plus one staging slot, all static in ITCM and never growing. No dedup.
- `BINDS` (32), `SLOTS` and their asserts (`SLOTS <= 64`, `BINDS <= 32`) stay. Runtime state is never per map row: 16 seats a Part.
- Echo covers the seated rows plus the bursts. No per-row `last` for the 90.
- Lookup is a linear scan, with no CC index.
- No map buffer on the stack. Reads stream into the staging slot, and writes stream through `Store::write`. `just stack-check` (ADR 0025) must pass unchanged.
- The Sound's own 16 rows win over the map's on the same control. A map stays with the Part across Sound loads. NONE holds nothing.
- Factory maps avoid ADR 0263's built-ins (`midi_map::builtin::BUILT_IN`) and the reserved CCs (`LearnCc` refuses them).
- Rest images are built in place by stores, not copied from statics (ADR 0251).
- Targeted tests in each task. Run `just check` once, before the PR.

## Review Focus

The tests must pin these five input classes:

1. **A 0.3 sectioned map with more than 90 rows total.** A `MapSection` file loads flattened: the first 90 rows in file order (shared, then engines by disk code), the rest counted as skipped, nothing past 90 in the slot, no panic. Its next save writes `MapRows` only (Tasks 3, 4).
2. **A map playing on the wrong engine.** The ALGO map on a VA Part: ALGO-block rows take no seat, move no param, send no echo and draw dimmed. The shared rows (ENV, LFO, filter, FX) play (Tasks 5, 6, 7).
3. **Two Parts on the same map, one edited.** An edit to Part 1's row reaches the card, then Part 2's slot on the next card work. Part 2's seats on that row are reset, and Part 1's other rows are unchanged (Task 5).
4. **PICKUP seat eviction.** A PICKUP row picked up and resting is evicted by 16 other rows heard. Heard again at a far value, it moves nothing until it crosses the param. A seat with a moving ramp is never evicted (Task 5).
5. **Power loss mid-save (A/B).** The side being written is torn (truncated, or its CRC broken). The load takes the last good side with its rows intact, and the Part's slot is never left half-written (Task 4).

---

### Task 1: The ADR and the types

**Files:**
- Already in the tree: `docs/adr/0266-flat-engine-maps-in-itcm.md`, its row in `docs/adr/README.md`. Edit only its Consequences' numbers, in Task 9.
- Create: `chimera-engine/src/midi_map/flat.rs` (types only in this task)
- Modify: `chimera-engine/src/midi_map/mod.rs` (`pub mod flat;`, `pub const MAP_ROWS: usize = 90;` beside `MAP_LEN` at :349)
- Test: `chimera-core/tests/flat_map_types_test.rs`

**Interfaces:**
- Consumes: `Binding` (`midi_map/mod.rs:340`, 12 B, asserted at :347), `MidiMap::learn`'s one-row-a-target rule (:471), `Input::same_control` (:298), `MAP_LEN` (16).
- Produces (in `midi_map::flat`, all `Copy`, private fields):
  - `MapLen(u8)`, 0..=`MAP_ROWS`: `const fn new(n: usize) -> Option<Self>`, `const fn get(self) -> usize`, `MapLen::ZERO`, `MapLen::FULL`.
  - `MapRowIx(u8)`, 0..`MAP_ROWS`: `const fn new(i: usize) -> Option<Self>`, `const fn get(self) -> usize`, `fn below(len: MapLen) -> impl Iterator<Item = MapRowIx>`.
  - `ControllerMap { rows: [Binding; MAP_ROWS], len: MapLen }`:
    - `fn len(&self) -> MapLen`, `fn rows(&self) -> &[Binding]`, `fn row(&self, MapRowIx) -> Option<&Binding>`;
    - `fn push(&mut self, Binding) -> Result<MapRowIx, MapFull>` (a target already present is replaced in place);
    - `fn set(&mut self, MapRowIx, Binding)`, `fn remove(&mut self, MapRowIx)`;
    - `fn absorb(&mut self, own: &MidiMap) -> Result<(), MapFull>` (each own row clears the rows on its control or target, then is appended; all or nothing);
    - `fn find(&self, b: &Binding) -> Option<MapRowIx>` (by target and control);
    - `unsafe fn empty_at(p: *mut ControllerMap)` (built in place).
  - `const _: () = assert!(MAP_ROWS <= u8::MAX as usize)`, and `size_of::<ControllerMap>() <= 1_084` asserted.

- [ ] **Step 1: Write the failing tests** in `flat_map_types_test.rs`:
  - `map_len_bounds`: `MapLen::new(90).is_some()`, `MapLen::new(91).is_none()`, `MapLen::FULL.get() == 90`.
  - `row_ix_bounds`: `MapRowIx::new(89).is_some()`, `MapRowIx::new(90).is_none()`, `MapRowIx::below(MapLen::new(3).unwrap()).count() == 3`.
  - `push_fills_to_ninety_then_refuses`: 90 distinct targets push `Ok`, the 91st is `Err(MapFull)`, and `len() == MapLen::FULL`.
  - `push_replaces_a_target_in_place`: pushing CC 20 → CUTOFF then CC 21 → CUTOFF leaves one row, on CC 21, at index 0.
  - `absorb_clears_control_and_target`: map rows {CC 20 → CUTOFF, CC 21 → RESO}, own {CC 20 → MORPH} → rows {CC 21 → RESO, CC 20 → MORPH}.
  - `absorb_past_ninety_leaves_the_map`: a full map absorbing a new control and target → `Err(MapFull)`, and the map is equal to before.
  - `empty_at_is_empty`: `empty_at` on a `MaybeUninit` gives `len() == MapLen::ZERO`.
- [ ] **Step 2:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 cargo test -p chimera-core --test flat_map_types_test` → FAIL (no `flat`).
- [ ] **Step 3:** Implement the types in `flat.rs`.
- [ ] **Step 4:** Run again → PASS. Commit `Flat maps: row count and index types`.

### Task 2: ITCM region, placement check, unit check

**Files:**
- Modify: `chimera-stm32/memory.x` (add `ITCM (rwx) : ORIGIN = 0x00000400, LENGTH = 63K`)
- Modify: `chimera-stm32/build.rs` (write `itcm.x` beside `ram_d2.x`/`sram4.x` at :10–47; `cargo:rustc-link-arg=-Titcm.x` beside :436–438)
- Modify: `chimera-core/src/ui/cc.rs` (`MidiSlot.rows: *mut MapSlots`. In `take()` (:107–150), `#[unsafe(link_section = ".itcm")] static mut ROWS: MaybeUninit<MapSlots>`, under `midi-map`, beside `RECALL`. The host's `Box::leak` path is at :152–156.)
- Modify: `Justfile` (new `check-itcm elf` recipe, called in `check` after `check-door-placed`)
- Test: `chimera-core/tests/flat_map_types_test.rs` (the size assert); the link-time asserts; `just check-itcm`

**Interfaces:**
- Consumes: `ControllerMap` (Task 1), `MAX_PARTS`.
- Produces:
  - `MapSlots { parts: [ControllerMap; MAX_PARTS], staging: ControllerMap }` in `chimera_core::ui::midi::layer`, with `unsafe fn rest_at(p: *mut MapSlots)` (bank 2's, called from `MidiSlot::build`). Only the type and the static are named in bank 1's `take()`.
  - Linker symbols `__sitcm` and `__eitcm`. The script `.itcm (NOLOAD) : ALIGN(4) { __sitcm = .; *(.itcm .itcm.*); . = ALIGN(4); __eitcm = .; } > ITCM`, `INSERT AFTER .uninit;`, with:
    - `ASSERT(__sitcm >= 0x400, "no slot at or near address 0")`;
    - `ASSERT(__eitcm <= ORIGIN(ITCM) + LENGTH(ITCM), "ITCM overflows")`.
  - `just check-itcm elf`: `rust-nm -C` finds the `ROWS` static. Its address must be in [0x400, 0x10000) and its size at least `7 × size_of::<ControllerMap>()`. It fails if the symbol is missing in a `midi-map` build, and is skipped for the bench sets, which hold no map.

- [ ] **Step 1: Bootloader 1.09 sanity (host only):** search `preenfm3-bootloader-backup.bin` (the stock 1.09 copy; read it, never flash it) for the ITCMCR literal (`90 EF 00 E0`) and for any vector or initial SP word in 0x0000_0000–0x0000_FFFF, with `rust-objdump -D -b binary -m arm --start-address=0` and a byte search. Record the result (expected: none) in the PR body and in ADR 0266's Consequences (Task 9). Stop and ask the owner if either is found.
- [ ] **Step 2: Write the failing check.** Add `check-itcm` to the Justfile and run it on a default release ELF built before the region exists → FAIL (no symbol).
- [ ] **Step 3:** Add the region, the script, the link arg and the `ROWS` static (built in place by `MapSlots::rest_at` on the first `MidiSlot::build`, never zero-initialised by the runtime).
- [ ] **Step 4:** Build and check: `nice -n 15 env CARGO_BUILD_JOBS=10 cargo build -p chimera-stm32 --target thumbv7em-none-eabihf --release` then `just check-itcm target/thumbv7em-none-eabihf/release/chimera-stm32` → PASS. Repeat for `--features orbit` and `--no-default-features` (the latter skips).
- [ ] **Step 5:** Commit `ITCM region for the map slots`.
- [ ] **Step 6 (hardware, after merge):** flash with `just flash`. On the unit's console:
  - `ls maps`;
  - pick ALGO on Part 1 (MIDI MAP row 0);
  - send CC on rows 1, 45 and 90, and record that each param moves;
  - power-cycle through bootloader 1.09 (no BOOT0 change), then pick and play again.
  
  Pass: every move lands after the cold start, so ITCM holds the rows through the bootloader's jump.

### Task 3: Flat codec and old-format flattening

**Files:**
- Modify: `chimera-engine/src/storage/record.rs` (`RecordTag::MapRows` ↔ `0x8024`, critical, in `code`/`from_code` at :112–172; the doc of `MapSection` says read only)
- Modify: `chimera-engine/src/midi_map/codec.rs` (row-level entry points; the `MAP_LEN` cap at :192 stays for a Sound's `Bindings`)
- Modify: `chimera-engine/src/midi_map/flat.rs`
- Modify: `chimera-engine/src/storage/sound.rs` (:239–241: `MapRows` is never a Sound's record)
- Test: `chimera-core/tests/flat_map_codec_test.rs`

**Interfaces:**
- Consumes: codec's `input_bytes`/`input_of`/`KINDS` (`codec.rs:58–150`), `addr_code`/`addr_of`, `MAX_RECORD_LEN` (640).
- Produces:
  - In `codec`: `pub(crate) enum RowWidth { Plain, SysEx }` (8 B / 12 B, from the version byte); `pub(crate) fn row_of(w: RowWidth, bytes: &[u8]) -> Option<Binding>`; `pub(crate) fn row_bytes(b: &Binding, w: RowWidth, out: &mut [u8; 12]) -> usize`.
  - In `flat`: `RecordKind { Rows, Section }` (from `RecordTag`). `pub struct RowFeed` is a push decoder fed record by record: `fn record(&mut self, k: RecordKind)` starts a payload, `fn bytes(&mut self, b: &[u8], into: &mut ControllerMap)` takes its bytes in any chunks, and `fn skipped(&self) -> Skipped` reports the count. `Skipped(u16)` is a newtype. A section's leading code byte and each payload's version byte are consumed by `bytes`. Rows go in through `ControllerMap::push`, so a duplicate target replaces the earlier row in place. Rows past `MAP_ROWS` and replaced duplicates count as skipped.
  - `const ROWS_PER_RECORD: [usize; 2]` = `(MAX_RECORD_LEN - 1) / 8` and `/ 12` (79, 53). `fn records(m: &ControllerMap) -> impl Iterator<Item = (RowWidth, Range<MapRowIx>)>`: the record split a save writes, a record `SysEx`-wide only if it holds a SysEx row.
  - `const FILE_MAX: usize` (header + the records of 90 SysEx rows + the trailer), with `const _: () = assert!(FILE_MAX <= 4096)` (the console `put` limit).

- [ ] **Step 1: Write the failing tests** in `flat_map_codec_test.rs` (bytes built by hand, no card):
  - `rows_round_trip_plain_and_sysex`: one CC row, one NRPN row and one SysEx row → `row_bytes` → `row_of` gives equal `Binding`s.
  - `ninety_rows_split_into_records_under_640`: 90 plain rows → two records (79 + 11); 90 rows with one SysEx row in the first 53 → that record is 12-B wide with 53 rows; every payload ≤ 640.
  - `feed_reads_rows_across_records`: feeding the two records' bytes in chunks of 1, 7 and 640 gives the same 90 rows, `skipped == 0`.
  - `sectioned_map_over_ninety_is_flattened` (Review Focus 1): five `Section` payloads of 20 rows each (codes 0, 1, 2, 3, 4) → `len() == MapLen::FULL`, rows 0–19 are section 0's, row 89 is section 4's 10th, `skipped == 10`.
  - `a_duplicate_target_keeps_the_last`: two rows to CUTOFF on CC 20 and CC 21 → one row, at index 0, on CC 21, `skipped == 1`.
  - `an_unknown_version_skips_its_record`: a `Rows` payload with version 9 → no rows from it, skipped 1, later records still read.
  - `map_rows_is_critical`: `RecordTag::MapRows.code() & CRITICAL != 0`, and `from_code(0x8024) == Some(MapRows)`.
- [ ] **Step 2:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 cargo test -p chimera-core --test flat_map_codec_test` → FAIL.
- [ ] **Step 3:** Implement the codec entry points, `RowFeed` and the record split.
- [ ] **Step 4:** Run → PASS. Also run `cargo test -p chimera-core --test bindings_codec_test --features bank2` (a Sound's codec is unchanged). Commit `Flat map records; sectioned maps flattened`.

### Task 4: Card read and write without stack buffers

**Files:**
- Modify: `chimera-engine/src/midi_map/flat.rs` (`load`, `save`, `delete`, `list`, `stem` re-export)
- Delete: `chimera-engine/src/midi_map/ctl.rs` (its `load_stem` :256–290, `write_side` :358–390 and `FILE_MAX` buffers go). Remove its glob line in `chimera-stm32/build.rs:239`. Leave `flat` out of every bank-2 glob.
- Modify: `chimera-engine/src/storage/small.rs` only if a header-peek helper is needed. Prefer a `ByteSink` in `flat` that breaks after `HEADER_LEN`.
- Test: rewrite `chimera-core/tests/controller_map_test.rs` (its card tests :187–400) for the flat map

**Interfaces:**
- Consumes: `Store::read(vol, f, &mut dyn ByteSink)`, `Store::write(vol, f, &mut dyn FnMut(&mut dyn ByteSink) -> …)`, `peek_header`, `Crc32`, `Header`, `Side`, `Generation`, `list_stems`, `RowFeed` (Task 3).
- Produces (in `flat`, signatures as `ctl`'s were, so `layer.rs` changes little):
  - `fn load(s: &mut dyn Store, r: &Ready, n: &SoundName, staging: &mut ControllerMap, into: &mut ControllerMap) -> Result<Skipped, LoadError>`. It peeks both sides' headers, streams the newest side naming `n` into `staging` through a `MapSink` (CRC as it goes), and falls back to the other side on a bad CRC. Only on success does it copy `staging` into `into`; `into` is untouched on any error.
  - `fn save(s, r, n, m: &ControllerMap) -> Result<(), SaveError>`: the side a load does not take, a generation on. The records are streamed through `Store::write` with a running CRC, and the largest local is the 12-B row buffer.
  - `fn delete(s, r, n) -> Result<(), LoadError>`, `fn list(s, r, out: &mut [Option<SoundName>; MAX_MAPS]) -> Result<(usize, bool), StoreError>`: as in `ctl`, with header peeks only.
  - `struct MapSink<'a>` (`ByteSink`): header, then the TLV walk feeding `RowFeed`, with the last 4 bytes held back as the trailer. An unknown critical tag refuses the file.

- [ ] **Step 1: Write the failing tests** in `controller_map_test.rs`:
  - `a_full_map_round_trips_through_the_card`: 90 rows (with SysEx rows) → `save` → `load` gives equal rows, `Skipped(0)`.
  - `a_torn_side_loads_the_last_good_map` (Review Focus 5): save map A, then map B (edited row 45). `plant` side B truncated mid-record → `load` gives map A's rows. A second case: side B whole but with one flipped CRC byte → map A. A third: `into` holds sentinel rows before a load that fails on both sides, and still holds them after.
  - `a_save_after_a_torn_write_goes_to_the_torn_side`: after the truncated B, a save writes B again, a generation past A's.
  - `a_sectioned_file_on_the_card_loads_and_saves_flat`: a 0.3 file (bytes from 0.3's `write_side` layout, planted) loads flattened. After `save`, the side written holds no `0x0022` tag and one or two `0x8024` tags.
  - `a_name_sharing_a_stem_never_overwrites_it`, `the_listing_names_the_maps`, `a_full_listing_says_there_are_more`: kept, ported to `flat`.
  - `no_map_buffer_on_the_stack`: `size_of_val` of `MapSink` ≤ 64 B (the frame bound itself is `just stack-check`'s, in Task 9).
- [ ] **Step 2:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 cargo test -p chimera-core --test controller_map_test` → FAIL.
- [ ] **Step 3:** Implement `MapSink`, `load`, `save`, `delete` and `list`. Delete `ctl.rs` and repoint `use` sites (`layer.rs`, `chimera-utils/src/suggest_map.rs` until Task 8, `console/put.rs` if it names `ctl`).
- [ ] **Step 4:** Run → PASS, plus `cargo test -p chimera-core --test console_put_map_test --test header_map_test`. Commit `Flat maps stream to and from the card`.

### Task 5: Per-Part slots and the seat pool replace `Layers`

**Files:**
- Modify: `chimera-core/src/ui/midi/layer.rs` (`Layers` :58–85 loses `layers` and `scratch`, and reads into `MapSlots` through `MidiSlot.rows`. `ctl_work_in` :218–293 and `ctl_job` :296–370 load and edit the Part's slot. `job_on` :438–515 works on `staging`. `shadowed` stays.)
- Modify: `chimera-core/src/ui/midi/mod.rs` (`PartMidi` :91–118 gains `seats: [SeatRow; SEATS]` and `ages: [SeatAge; SEATS]`; `bind_at` :381–391 reads a seat's row; `cc_control` :602–648 and `cc_sysex` :690–717 scan and seat; `hand_in` :1177–1215; `cc_turns` :1035, `cc_step` :1130 unchanged in shape)
- Create: `chimera-core/src/ui/midi/seat.rs` (pure seat choice)
- Modify: `chimera-core/src/ui/cc.rs` (`MidiSlot` doc: no layers in D2)
- Test: `chimera-core/tests/midi_seat_test.rs`; extend `chimera-core/tests/controller_map_test.rs` and `keep_map_test.rs`

**Interfaces:**
- Consumes: `MapSlots`, `ControllerMap`, `MapRowIx` (Tasks 1–2), `Mappable::live` (`midi_map/mod.rs:108`), `PartMidi::rests` logic (:188), `BindRuntime` (`resolve.rs:15`).
- Produces:
  - `pub const SEATS: usize = MAP_LEN;` with `const _: () = assert!(MAP_LEN + SEATS == BINDS)`.
  - `SeatIx(u8)`, 0..`SEATS`: `fn new(usize) -> Option<Self>`, `fn slot(self) -> usize` (= `MAP_LEN + i`, a binding slot).
  - `SeatRow`, a seat's row or none: a 1-B newtype with `fn get(self) -> Option<MapRowIx>`, never a bare `u8` sentinel in callers.
  - `SeatAge(u8)`, wrapping, compared through `fn older_than(self, other) -> bool`.
  - In `seat`: `fn choose(rows: &[SeatRow; SEATS], resting: impl Fn(SeatIx) -> bool, ages: &[SeatAge; SEATS], want: MapRowIx) -> SeatChoice`, a pure function, and `enum SeatChoice { Seated(SeatIx), Take(SeatIx), Full }`.
  - `PartMidi::seat_row(&mut self, want: MapRowIx) -> Option<SeatIx>`: `Take` resets the seat's ramp, bin, pending, turn and `BindRuntime` (a fresh `TakeState`) and its echo slot. `Full` is `None`, and the message is dropped.
  - `bind_at(own: &MidiMap, map: &ControllerMap, seats: &[SeatRow; SEATS], j: usize) -> Option<Binding>`: the Sound's row j < 16, else the seat's row unless shadowed.
  - `fn map_rows_on<'a>(map: &'a ControllerMap, input: &'a Input) -> impl Iterator<Item = MapRowIx> + 'a` (the linear scan).

- [ ] **Step 1: Write the failing pure tests** in `midi_seat_test.rs`:
  - `a_seated_row_keeps_its_seat`: `choose` with `want` already in seat 3 → `Seated(3)`.
  - `the_oldest_resting_seat_is_taken`: all resting, ages 5, 2, 9… → `Take` of age 2's seat.
  - `a_moving_seat_is_never_taken`: the oldest is not resting → the next oldest resting is taken; none resting → `Full`.
- [ ] **Step 2: Write the failing UI tests** (through `UiState` and `MemStore`, as `midi_bind_test.rs` drives a Part):
  - `ninety_rows_each_move_their_cell`: a 90-row map on Part 1 (VA). CC on rows 0, 44 and 89 each moves its cell (ramped, landed after 120 ms).
  - `pickup_row_evicted_waits_again` (Review Focus 4): a row on CC 20 → CUTOFF set PICKUP. It is picked up and rests. Then 16 other rows are heard and rest, so the CC 20 row has lost its seat. The cell is moved by the hand. CC 20 at a far value leaves CUTOFF unchanged until a value crosses it.
  - `a_moving_seat_survives_seventeen_controls`: 16 rows heard in one pass (ramps moving), then a 17th → its message dropped, and the 16 ramps land where they were going.
  - `wrong_engine_rows_do_nothing` (Review Focus 2): the ALGO map on a VA Part. CC on an ALGO-block row → no seat taken (`seats` all empty) and the ALGO block is unchanged. CC on its ENV 1 ATTACK row moves VA's ENV 1 ATTACK.
  - `a_sound_row_shadows_the_map_row`: own row CC 20 → RESO, map row CC 20 → CUTOFF → only RESO moves.
  - `two_parts_one_map_one_edited` (Review Focus 3): Parts 1 and 2 on map `JOE`. Part 1 edits row 7's MAX via `map_in`, and card work runs. Card row 7's MAX is the new value, and Part 2's slot row 7 equals it after the next `ctl_work_in`. Part 2's seat on row 7, if any, is reset. Rows 0–6 and 8+ are unchanged on both.
  - `none_holds_no_rows`: a Part on NONE has `len() == MapLen::ZERO`, and its seats stay empty under any CC.
  - `a_map_stays_through_a_sound_load` (in `keep_map_test.rs`): a Sound load keeps the Part's slot and drops its seats.
  - `d2_freed`: `size_of::<Layers>()` is at most its old size minus `size_of::<[MidiMap; 6]>()` and the old `ControllerMap`'s size (≈2.1 KB), so the move is visible.
- [ ] **Step 3:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 cargo test -p chimera-core --test midi_seat_test --test controller_map_test --test keep_map_test` → FAIL.
- [ ] **Step 4:** Implement `seat.rs` and the slots, then replace the `layers` reads (`Layers::layer`, `edit`, `bind_at`'s callers, `page_layer`). `rest_at` builds `MapSlots` in place.
- [ ] **Step 5:** Run → PASS, plus `--test midi_bind_test --test midi_binding_test --test midi_control_test --test midi_learn_test --test midi_sysex_test --test midi_ui_door_test --features bank2 --test midi_map_door_test`. Commit `Map rows in per-Part slots, runtime in 16 seats`.

### Task 6: Echo on seats plus bursts

**Files:**
- Modify: `chimera-core/src/ui/midi/echo.rs` (`EchoPlan` :39–56 gains `walk: [BurstWalk; MAX_PARTS]`; `burst` :121; `send` :217; `value` :296; `Layers<'a>` alias :61 now gives the slots and seats)
- Test: `chimera-core/tests/midi_echo_test.rs`

**Interfaces:**
- Consumes: `bind_at` (Task 5), `ControllerMap::rows`, `MapRowIx::below`.
- Produces:
  - `BurstWalk`: the next `MapRowIx` a Part's burst sends, or done (a 1-B newtype); `EchoPlan::burst(p)` sets it to row 0.
  - `send` order unchanged (identity, active Part, other Parts). Each Part's burst sends the ECHO rows its walk reaches, one message each, while DIN has room, skipping rows its engine lacks or a Sound row shadows. The walk moves past each row sent and resumes next pass.
  - Seated rows echo as Sound rows do (`last`, `echoed`, `QUIET_MS`, `owed`), indexed by binding slot. A seat taken clears its slot's `last` and `echoed`.

- [ ] **Step 1: Write the failing tests:**
  - `a_burst_sends_every_echo_row_once`: 90 rows, 30 with ECHO, and a Sound load → over passes, exactly those 30 messages go out (DIN and USB), each once, then nothing until the next burst.
  - `a_burst_skips_rows_the_engine_lacks`: the ALGO map on VA with ECHO on every row → no message for an ALGO-block row.
  - `a_seated_row_echoes_on_change`: an ECHO row heard (seated), then the cell turned by hand → its value goes out after `QUIET_MS`.
  - `an_unseated_row_is_silent_between_bursts`: an ECHO row never heard, its cell turned by hand → nothing out until a burst.
  - `a_full_ring_resumes_the_walk`: DIN's ring full mid-burst → the remaining rows go next pass, none twice.
- [ ] **Step 2:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 cargo test -p chimera-core --test midi_echo_test` → FAIL.
- [ ] **Step 3:** Implement `BurstWalk` and the seat-indexed resets.
- [ ] **Step 4:** Run → PASS. Commit `Echo: seats, and bursts walk the map`.

### Task 7: MIDI MAP for up to 107 rows, with dimming

**Files:**
- Modify: `chimera-core/src/ui/nav.rs` (`SettingsAt.row` :126, `row()` :135, `settings_at(path, row)` :756: `PageRow`)
- Modify: `chimera-core/src/ui/midi/page.rs` (`Row` :337–342 `Layer` → `Map(MapRowIx, Binding)`; `row_of` :345–357; `map_rows_in` :367; `map_row_in` :372–430; `map_in` :435–498; `focus_note` :501–520)
- Modify: `chimera-core/src/ui/settings/midi_map.rs` (`MapRow::Layer` → `MapRow::Map` :37–46; `map_deleted`'s `as u8` :251)
- Modify: `chimera-core/src/ui/settings/view.rs` only if the MIDI MAP list window assumes ≤ 33 rows (check `MapBand`'s scroll at `midi_map.rs:145`)
- Test: `chimera-core/tests/midi_map_page_test.rs`

**Interfaces:**
- Consumes: the slots and seats (Task 5), `Mappable::live`, `layer::shadowed`.
- Produces:
  - `PageRow(u8)`: `const MAX: usize = 1 + MAP_ROWS + MAP_LEN` (107), `fn new(usize) -> Option<Self>` (clamped by the caller to the page's count), `fn get(self) -> usize`, `const _: () = assert!(PageRow::MAX <= u8::MAX as usize)`. `Location::settings_at` takes `PageRow` for MIDI MAP. Other screens keep their row type, or take `PageRow` if it is shared; decide by the smallest diff and note it.
  - `map_row_in` dims a map row when `!e.target.live(engine) || shadowed(own, e)`, and its focus-band tail stays `IN <map name>`. `focus_note` reads seated rows only.

- [ ] **Step 1: Write the failing tests:**
  - `the_page_shows_all_rows`: a 90-row map and 16 own rows → `map_rows() == 107`. The bar moves to row 106 and stays there on a further step.
  - `the_last_row_is_reachable_after_a_delete`: on row 106, delete the own row → the bar sits on row 105.
  - `rows_the_engine_lacks_are_dimmed` (Review Focus 2): the ALGO map on a VA Part → each ALGO-block row's `look == RowLook::Dimmed`, and its ENV/LFO/filter rows `Normal`.
  - `a_shadowed_row_is_dimmed`: own row on CC 20 → the map's CC 20 row is dimmed.
  - `editing_row_89_edits_the_card`: B–F on row 90 (map row 89) → one `CtlJob::Edit` for row 89, and the slot row changes at once.
  - `page_row_bounds`: `PageRow::new(107).is_none()`, `PageRow::new(106).is_some()`.
- [ ] **Step 2:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 cargo test -p chimera-core --test midi_map_page_test` → FAIL.
- [ ] **Step 3:** Implement. Update the screen goldens only where the row word changes (`cargo test -p chimera-core --features master-tape --test screen_golden_test`, and review each changed image).
- [ ] **Step 4:** Run → PASS. Commit `MIDI MAP: 107 rows, map rows dimmed off-engine`.

### Task 8: Factory engine maps, the pack and the guide

**Files:**
- Rename: `chimera-utils/src/suggest_map.rs` → `chimera-utils/src/engine_maps.rs`. The `main.rs` subcommand :68, :270–274 becomes `engine-maps DIR | engine-maps --tables`.
- Modify: `Justfile` (`suggest-map` :166–171 → `engine-maps`; `release` :331)
- Modify: `tools/factory_zip.py` (:30, :45: require the four engine maps by stem, refuse `SUGG05F6.A`), `tools/test_factory_zip.py`
- Modify: `docs/guide/MIDI.md` ("SUGGEST, the factory map" :232–262 → "The factory engine maps", with generated tables; the "Controller maps" text :168–231 says one flat list, 90 rows, any engine, dimmed rows)
- Test: `chimera-utils/src/engine_maps.rs` `#[cfg(test)]`, `tools/test_factory_zip.py`

**Interfaces:**
- Consumes: the param constants (`EnvParams::{ATTACK, DECAY, SUSTAIN, RELEASE, RISE, FALL, SHAPE, TIME}` `params.rs:447–462`, `LfoParams` `dsp/lfo.rs:111`, `FilterParams` `params.rs:57`, `VoiceFxParams`, `PitchParams`, and each engine's own: `AlgoParams`/`AlgoOpParams`, `ModalParams`, `VaParams`, the TAPE params), `Mappable::new`, `ParamSpec` (to walk the tables), `LearnCc`, `BUILT_IN`, `flat::save`.
- Produces:
  - `pub const FACTORY_CCS: [LearnCc; 90]`: MIDI's undefined CCs first (3, 9, 14, 15, 20–31, 85–90, 102–119), then the other `LearnCc`s, less `BUILT_IN` and {1, 2, 4, 65–69, 84}.
  - `pub const COMMON: &[(BlockRef, ParamId)]`: the cells every engine reads, in play order: ENV 1–2 A and B, LFO 1–3, the filter, the FX slot, the voice. Each sits on the same `FACTORY_CCS` position in every map.
  - `fn engine_cells(e: EngineType) -> impl Iterator<Item = Mappable>`: a hand-ordered head (SUGGEST's ten: MORPH…, STRUCT…, WAVE1…, POSITION…), then the rest of the engine's mappable cells in table order, skipping any already placed.
  - `pub fn map(e: EngineType) -> ControllerMap` (`COMMON`, then `engine_cells`, until 90 rows or the cells run out), `pub fn name(e) -> SoundName` (`ALGO`, `MODAL`, `VA`, `TAPE`), `pub fn write(dir) -> Result<(), String>` (four files through `flat::save`), `pub fn tables() -> String`.
  - No Part LEVEL/PAN (CC 7/10's), master FX or ORBIT rows. Every row is absolute `Jump`, ECHO off.

- [ ] **Step 1: Write the failing tests** (ported from SUGGEST's at `suggest_map.rs:225–305`):
  - `factory_ccs_are_free`: 90 distinct, each a `LearnCc`, none in `BUILT_IN` or {1, 2, 4, 65–69, 84}, and the first 40 exactly MIDI's undefined set.
  - `each_map_loads_and_every_row_plays`: each file read back through `flat::load` has `Skipped(0)`. Every row is `Mappable`, live on its engine, absolute `Jump`, and on a `FACTORY_CCS` CC, with no CC twice and no target twice. Its length is `min(90, COMMON.len() + engine cells)`.
  - `common_cells_share_a_cc`: for each `COMMON` cell, its CC is the same in all four maps.
  - `both_env_types_are_mapped`: each map has ENV 1 and ENV 2 ATTACK, DECAY, SUSTAIN, RELEASE, RISE, FALL, SHAPE and TIME.
  - `guide_tables_are_current`: `tables()` equals the generated section of `docs/guide/MIDI.md`.
  - `test_factory_zip.py`: a pack without `ALGO`/`MODAL`/`VA`/`TAPE` maps is refused, and a pack with `SUGG05F6.A` is refused.
- [ ] **Step 2:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 cargo test -p chimera-utils engine_maps` and `python3 -m unittest discover -s tools -p test_factory_zip.py` → FAIL.
- [ ] **Step 3:** Implement. Write the guide text in plain words. It describes the instrument as it is: SUGGEST is not named as "removed", since that belongs in the release notes. Run `just engine-maps --tables` and paste the tables.
- [ ] **Step 4:** Run → PASS. Commit `Factory engine maps: ALGO, MODAL, VA, TAPE`.
- [ ] **Step 5 (outside this repo, after merge):** regenerate the private Cirklon `.cki` files in (private notes) from `just engine-maps`. Not committed here.

### Task 9: `just check`, flash deltas, the PR

**Files:**
- Modify: `tools/bank1-budget.txt` (each set's line `; ADR 0266, ADR 0251 +N flat engine maps (from X)`)
- Modify: `docs/adr/0266-flat-engine-maps-in-itcm.md` (Consequences: measured bank 1/bank 2/D2/ITCM numbers and the bootloader check's result. It is still Proposed, so it may be edited.)

- [ ] **Step 1:** Run `nice -n 15 env CARGO_BUILD_JOBS=10 just check` (with `CHIMERA_STAMP_CLEAN=1` as the recipe sets). Fix anything it finds; `check-bank1` will name the sets that grew.
- [ ] **Step 2:** Measure on the `orbit` and `default` release builds against `0.4`'s merge base:
  - bank 1 (`rust-objcopy -R .bank2` size);
  - bank 2 free (0x08180000 − `__bank2_end`);
  - D2 free (`.ram_d2` end vs `RAM_D2`);
  - ITCM used (`__eitcm − __sitcm`);
  - `size_of::<MidiUi>()`.
  
  Raise the budget lines naming ADR 0266 and ADR 0251. If bank 2 overflows, move `ui::midi::seat` to `chimera-core/src/ui/map_seat.rs` (bank 1) and re-measure. Report each delta in one line. Escalate only if bank 1's free space falls under ~20 KB (the owner's 2026-11-12 line).
- [ ] **Step 3:** `just stack-check` passes with no new frame over its bound (ADR 0025).
- [ ] **Step 4:** Fill ADR 0266's measured numbers. Commit `Flat engine maps: budgets and measured numbers`.
- [ ] **Step 5:** Push the branch and open the PR against `0.4`. The body covers: the five Review Focus classes and their tests; the flash, RAM and ITCM deltas in one line each; the bootloader 1.09 check; and the hardware step (Task 2 Step 6, plus the unit end-to-end check: MIDI notes, MIDI MAP on every factory map, CCs on first, middle and last rows, audio recorded) marked after merge. Merge after a passing review with green checks.

---

## Self-review

- **Spec coverage:**
  - Flat ≤ 90 rows: Tasks 1 and 3.
  - Factory maps per engine, replacing SUGGEST: Task 8.
  - Map stays across Sound loads, NONE holds nothing: Task 5 (`a_map_stays_through_a_sound_load`, `none_holds_no_rows`).
  - Any map on any engine, rows dim and dead: Tasks 5 and 7.
  - Sound rows win: Tasks 5 and 7.
  - Six ITCM slots: Task 2. Dedup not taken: ADR.
  - 16 seats with pickup re-wait: Task 5.
  - Echo on seats plus bursts: Task 6.
  - Linear scan: Task 5 (`map_rows_on`).
  - Flat records, no stack buffer: Tasks 3, 4 and 9 (stack-check).
  - Old maps flattened: Tasks 3 and 4.
  - SUGGEST not shipped: Task 8 (`factory_zip`).
  - Generator from param tables, `.cki` regenerated: Task 8.
  - ITCM `memory.x`/`build.rs`, unit check, bootloader 1.09 check: Task 2.
  - Bank 1 growth under ADR 0251: Task 9.
  - Page row u8 for 107: Task 7.
- **Type consistency:** `MAP_ROWS`, `MapLen` and `MapRowIx` (Task 1) are used by `RowFeed`, `records` and `FILE_MAX` (Task 3), `load`/`save` (Task 4), `SeatRow`/`bind_at`/`map_rows_on` (Task 5), `BurstWalk` (Task 6), `Row::Map`/`PageRow` (Task 7) and `map(e)` (Task 8). `SEATS == MAP_LEN` keeps `BINDS == 32`. `MapRow` (the page's enum) and `MapRowIx` (a map's row) are distinct names on purpose.
- **Proportion:** signatures, test names and assertions only. Function bodies are left to the implementer.
