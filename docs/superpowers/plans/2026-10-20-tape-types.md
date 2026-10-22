# TAPE type-driven refactor (#697) Implementation Plan

**Goal:** Close every finding of the TAPE type-driven review (H1–H4, M1–M7, L1–L10) and the three #702-review items, with no change in behaviour.

**Architecture:** Three stacked PRs into `tape-streaming`: (A) stream, board and door types; (B) params, codec and UI types; (C) the deck. Each finding becomes a type that makes the forbidden state unrepresentable; pure functions stay pure, shells stay thin.

**Tech Stack:** Rust, no_std, `chimera-engine`, `chimera-core`, `chimera-board`, `chimera-stm32`, `chimera-hal`, `chimera-fat`.

**Spec:** (private notes) (the findings, with fix sketches) and issue #697 plus its comment (private notes S1, S2 and the `Judged` note). The report predates #702 and #706: check every finding against the code as it is now, and skip or trim what is already closed.

## Global Constraints

- Behaviour must not change. Every existing test passes unchanged, except a test asserting a now-unrepresentable state: turn that into a `compile_fail` doctest.
- Goldens identical (no re-record). Disk codes and saved Sounds byte-identical. Bills unchanged.
- Bank-1 flash and RAM must not grow (baseline: `just firmware-orbit`, then `rust-size -A` on the ELF; free = 917504 − size of `target/chimera-orbit.bin`). Report the delta in one line.
- No `unsafe` without `// SAFETY:`. No heap, no blocking in audio. No libc.
- Commits: terse, plain, no type prefix. Comments only where genuinely needed.
- Never stage `chimera.bin` or `docs/chimera-ui-ux-spec.md`.
- Builds: `nice -n 15`; at most two heavy jobs machine-wide (check `uptime`); one heavy build at a time per agent.
- Don't touch the unit (no flash, no USB).
- Newtype rule: when a primitive carries a unit or a domain, give it a type; when two args of one type sit side by side and mean different things, give them different types; enums over bools and sentinels.

## Review Focus

- A saved Sound with an off-PLAY MODE (H2): it loads to the same machine, saves to the same bytes as the factory file did. Test: round-trip every factory TAPE Sound (Task B1).
- A retired MODEL code (1, 2, 7) in a v1 file: still translated, never rejected by the loader (Task B2's test over `RetiredModel::ALL`).
- A big TRANSWAVE/ZONESET or a too-short source on a streamed machine: UI shows the true refusal, not NEEDS RAM (Task B3 test).
- A streamed voice whose note ends in a port-less block: no stale "streams" anywhere; the ring is dropped (Task C1 test).
- A bill at every rate and ratio the old formula covered: same `u32` byte rate (Task A2 table test old vs new).

---

## PR A: stream, board and door types (`tape-types` → `tape-streaming`)

### Task A1: Descriptor and stream units (H4, M1, L2, L3, L6)

**Files:** `chimera-engine/src/dsp/tape_engine/desc.rs`, `stream/{cursor,layout,units,ring,registry,stage,stage/place}.rs`, `dsp/tape_engine/{span,read,heads,mod}.rs`, `instrument/admission.rs`.

**Interfaces (produces):**
- `Descriptor` private fields; `Descriptor::new(..) -> Option<Self>`; accessors; `rate() -> SourceRate`; `held() -> Held`; one `is_streamed()` used by `row_of`, `refuses`, `SourceView::of`, admission. Layout per the report (`Layout::{Plain(Held), Transwave{count}, ZoneSet{count}}`, `Held::{Whole, Streamed{heads, reach}}`) unless the bytes force otherwise. `to_bytes`/`from_bytes` byte-identical.
- `SourceRate` replaces `u16` rate in `bill`, `head_frames`, `window::*`, deck `stream_rate`. The `4_000..=48_000` range lives once.
- `SourceFrame(u32)` (positions; `SourceFrame − SourceFrame = Frames`), `OutSamples(u32)` (fade lengths; `Inc * OutSamples -> Frames`), `SpanEnd::{At(SourceFrame), FileEnd}` replacing `u32::MAX`, `chunk_of -> ChunkIdx` everywhere (no silent return on a failed `ChunkIdx::new`).
- L2: one pure `fn holds(first, end, file, i)` used by cursor and `span::Span::holds`.
- L3: `Heads::{One(HeadSpec), Looped{start, looped}}`, `HeadIdx::{Start, Loop}`; disk `heads: u8` stays on the codec side.
- L6: `AtomicTag<SourceId>` helper (one place for `index+1`/`checked_sub(1)`), `Guard::bits()`.

- [ ] Write tests: `Descriptor::new` refuses rate 0 / heads on a ZoneSet / heads without reach; `to_bytes(from_bytes(b)) == b` over every descriptor the factory tapes and tests produce; `SpanEnd::FileEnd` plans as `u32::MAX` did (existing cursor tests cover it).
- [ ] Implement; convert the type in place, call sites follow.
- [ ] `nice -n 15 cargo test -p chimera-engine -p chimera-core` green, goldens untouched.
- [ ] Commit.

### Task A2: Admission arithmetic (M2 except the deck's cents, L8, L4 stream bools)

**Files:** `stream/admit.rs`, `stream/cursor.rs` (const assert), `instrument/admission.rs`, `instrument.rs`, `stream/serve.rs`, `chimera-board` card_dma, `chimera-hal/src/units.rs`.

**Interfaces (produces):**
- `const fn bill(rate: SourceRate, r: ReadRatio) -> Bill` (or the f32 core behind a `const fn bytes_per_frame()` built from `RAW_CHUNK_BYTES`/`CHUNK_BYTES`), with the cursor's const assert calling the same function. `Bill` private field, `BytesPerSec` for budgets and `STREAM_BUDGET_BPS`.
- `Cents(f32)` in `stream`/`units` (shared with Task C1); `ratio_max(note: MidiNote, root: Cents, reach: Cents, bend: Cents)`.
- `enum Age { Held{since: Blocks}, Released{since: Blocks} }` keeping the derived `Ord` order.
- `units::Ratio` moves to the board crate (card bench only). `inc()` returns `Inc`.
- L8: `PartId`/`VoiceIdx` in admission, `VoiceSet` bitset for `gone`, named structs for `BillAt`/`PickAt`.
- L4 (stream side): `Attempt::{First, Retry}`, `Duck::{Out, In}`, and enums for `tend(asking)`, `IrqState::step(asked)`.

- [ ] Write a test: the new `bill` equals the old formula's `u32` across rates 4000..=48000 (step 1000) × ratios {0.25, 0.5, 1, 2, 4, 16}.
- [ ] Implement.
- [ ] Engine + core tests green; `bench_sets` builds compile (`cargo build -p chimera-stm32 --release --no-default-features --features bench-stress,bank2`).
- [ ] Commit.

### Task A3: Ring claims, board clocks, #702 S1 (L5, L10, S1)

**Files:** `stream/ring.rs`, `chimera-board/src/card_dma.rs`, `chimera-stm32/src/midi_din.rs`.

- L5: `CardSide` tracks live claims per slot; `claim` returns `None` while one lives. Loom tests (`just loom`) still pass.
- L10: `Hz(u32)`, `Cycles(u32)`, `Bytes` in `init`, `clocks()` (a struct, not a tuple), `Line`.
- S1: `midi_din.rs:55,94` read the cycle counter and `AUDIO_CYCLES` as a pair through the same retry loop `card_dma::clocks()` uses — one shared helper.

- [ ] Test: a second `claim` on a slot with a live claim returns `None` (ring unit test).
- [ ] Implement; `just loom` green; firmware builds.
- [ ] Commit.

### Task A4: Doors (M5, #702 `Checked`)

**Files:** `chimera-engine/src/bank2.rs`, `chimera-core/src/bank2.rs`, `chimera-board/src/bank2.rs`, the bank2.x slot asserts.

- `DoorSlot<T> { at: u32, _t: PhantomData<fn() -> T> }`, consts beside the `bank2.x` asserts, safe `with_slot<T, R>(&self, s: DoorSlot<T>, f: impl FnOnce(&T) -> R)`; `with_at` goes.
- Door fn-pointer fields private; methods call them, so no `fn` value leaves a `with` closure. A `compile_fail` doctest: `ui_door(|d| d.strip)` doesn't compile.
- `Judged::check(self, ..) -> Option<Checked>`; `DoorCell::open(Checked)`.
- Must stay byte-identical in layout (`#[repr(C)]` tables unchanged); `utils door-check` and `check-door-placed` pass.

- [ ] Implement, the doctests first.
- [ ] Host door tests (`stream_door_test`, `glyph_door_test`, proxy tests under `--features bank2,test-support`) green; firmware + door-check green.
- [ ] Commit.

### Task A5: FAT runs and the TAPE cfg gate (#702 S2, M7)

**Files:** `chimera-fat/src/store.rs:292-296`, `chimera-engine/src/lib.rs`, `stream/mod.rs`, `chimera-board/src/lib.rs`, `chimera-stm32/src/audio/engine.rs`, `project/codec.rs:607`, `serve.rs` `Rot`.

- S2: a run is checked against the card's block count (`BlockCount`), returning `Corrupt`; a test with a run past the last block.
- M7: measure first. Build `--features orbit` and `--no-default-features` as is, then with `#[cfg(feature = "tape")]` on `stream::{cursor, ring, serve, slot, registry, stage, layout}` (keeping a tiny always-on `stream::off` with `NoPorts`, `StreamCount`, `STREAMS`), `card_dma` behind `any(tape, bench)`, `Rot`'s runtime guards and the codec's `cfg!` guard gone. If the gate compiles cleanly, keep it; if it can't (CARD_BENCH needs the ring), keep the runtime gate and write an ADR saying why with both measurements.

- [ ] Implement S2 with its test; implement or ADR M7 with the measured numbers.
- [ ] `cargo test -p chimera-core -p chimera-engine --no-default-features` and every firmware feature set in `just check` build.
- [ ] Commit.

### PR A gate

- [ ] `nice -n 15 just check` with `--no-fail-fast` semantics (run each line, collect failures; only #646 may fail), `just stack-check`, door-check. Flash/RAM delta vs baseline.
- [ ] One sonnet review of the PR diff (quality, efficiency, performance, principled-ness); at most two fix rounds.
- [ ] Push, `gh pr create --base tape-streaming`.

---

## PR B: params, codec and UI types (`tape-types-ui` → `tape-types`)

### Task B1: One `TapeMachine` in memory (H2, M3)

**Files:** `dsp/tape_engine/params.rs`, `machine.rs`, `stream/stage/place.rs`, `project/streamed.rs`, `chimera-core` callers, `docs/adr/0189-*.md` + `docs/adr/README.md`.

- Params hold `machine: TapeMachine`; the (MODEL code, MODE code) pair exists only in `enum_code` / `set_enum_code` / `translate_v1`. `hold`, `at_root`, `needs_ram`, `heads_in`, `model_cost`, `streamed_cost`, `reads_in`, `ordered` match on `TapeMachine`.
- M3: `Choice` trait (`from_value(f32) -> Option<Self>`, `value(self) -> f32`) for TYPE, WOW, MODE, MODEL, exhaustive; `Cell::default` typed (`enum Default { Unit(f32), Choice(u8) }`). `MachineIndex(u8)` names MODEL's value domain; `TapeModel` the disk code. A `compile_fail` doctest: passing a `TapeModel` code where a `MachineIndex` is wanted.
- An off-PLAY MODE written by a factory file: the saved bytes must still match. If dropping the remembered MODE would change a factory Sound's bytes, keep the code the file had (codec-side), never in params.
- ADR 0189 supersedes 0178 in part (the two in-memory fields); 0178 is not edited.

- [ ] Test first: round-trip every factory TAPE Sound (load → save → bytes equal) and every machine × codec pair.
- [ ] Implement; all tests and goldens unchanged.
- [ ] Commit.

### Task B2: Retired codes (M4)

- `#[repr(u8)] enum RetiredModel { Reverse = 1, PingPong = 2, Oneshot = 7 }` with `ALL`, `from_code`, `translate`; `storage/codes.rs` `RETIRED_CODES` built by a const fn from `RetiredModel::ALL`, `BlockRef::TapeEngine.code()`, `P::MODEL.0`.
- [ ] Test: every `RetiredModel::ALL` code loads through the loader and translates. Implement, commit.

### Task B3: Refusals (H3, M6, L9)

**Files:** `project/streamed.rs`, `project/streamed/load.rs`, `params.rs`, `sources.rs`, `chimera-core/src/ui/source_cell.rs`, `source_cell/card.rs`, `codec.rs:392`.

- `enum Seeks { Tron, Scrub, Stretch, Wave }`; `NeedsRam { Ping, Reverse, Machine(Seeks) }`; `enum Refusal { NeedsRam(NeedsRam), Shape(Shape), TooShort, Closed }`; `LoadKind::Refused(Refusal)`; `heads_for -> Result<Heads, Refusal>`; `Absence::NeedsRam` takes `Refusal`. The UI says the real reason; the impossible `Machine(Play)` arm is gone. New words follow ADR 0186 and full-word labels; keep each existing message identical where its case is unchanged.
- M6: `Stem(u32)` through `source_name`, `FileKey`, `StreamFile::stem`, `TapeList`, the cell; `enum SourceNote { .. }` with `words()` replacing `&'static str` failures; `[Option<HeadsKey>; N]` for `stuck`; `UiFrames(u16)`; `file(stem)` without a fake `SourceRef`.
- L9: `SourceSet` bitset for `LoadDoor::sources` and `ProjectCheck::to_stream`; `enum Phase { Find, Heads { file, at: Bytes } }`.
- [ ] Test: a TRANSWAVE source too big to stream under PLAY LOOP reports `Refusal::Shape`, the cell's words say so. Implement, commit.

### Task B4: Glyph inputs (L7, L4 gauge, L4 `SourceJob::Head{pick}`)

- `Machine::Ping` without `xfade`; `Canvas::at` takes a checked/unsigned point; `Palette` sized by `Ink::ALL.len()`; `GlyphInputs { machine: TapeMachine, .., pose: Pose::{Still, Live} }` replacing `from_set([f32; 10], still)`. Glyph goldens identical.
- [ ] Implement, commit.

### PR B gate — as PR A, base `tape-types`.

---

## PR C: the deck (`tape-types-deck` → `tape-types-ui`)

### Task C1: Note and machine state (H1, L1, M2's deck cents)

**Files:** `dsp/tape_engine/mod.rs` (split into `deck/` modules if it helps), `oneshot.rs`, `tron.rs`, `wave.rs`, `stretch.rs`, `window.rs`, `instrument.rs`.

- `enum Note { Idle, Gated { src, waited }, Playing(Playing) }`; `Playing { src, machine: MachineState, feed: Feed, .. }`; `enum MachineState { One(Once), Loop(..), Ping(PingPass), Tron(Once), Scrub(Scrub), Stretch(Stretch), Wave(Wave) }`; `enum Feed { Ram, Streamed { cursor, rate: SourceRate, jumped, ducked } }`. `streams()` is a `matches!` on the feed; `end_stream` consumes the feed; `Instrument::streaming[v].paid` derives from it or goes, so one owner records "this voice streams".
- `inc()` returns `Inc`; cents as `Cents`; `note` as `MidiNote`.
- L1: `Bounds::new(ls, le, lx) -> Option`, private fields; `Sizing { size, place, xfade }`; `NonZeroU32` file frames where `frames - 1` is taken.
- `TapeDeck` size must not grow (it should shrink: report `size_of::<TapeDeck>()` before/after).
- [ ] Test: port-less block ends a streamed note → `streams()` false and the ring dropped; every existing deck test and golden unchanged.
- [ ] Implement in steps that each stay green (note enum, then machine enum, then feed), committing each.

### PR C gate — as PR A, base `tape-types-ui`.
