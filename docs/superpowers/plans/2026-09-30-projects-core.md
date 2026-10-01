# Projects Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The synth's whole musical state becomes one `Project` that saves to the card, loads back bit-identical (at boot too), and swaps while notes are held with a voice fade and no FX step, driven by pure APIs that any navigation can call.

**Architecture:**
- **Functional core** (`chimera-core/src/project/`), pure and host-tested:
  - the model: `PartId`, `SlotId`, `PartSet`, `Pool`, `Origin`, `Project`;
  - the project file codec, `project_crc` and `TemplateCrc`;
  - the derived marks (`part_status`, `project_status`, `part_actions`);
  - `ReplaceGuard`, typed per replace target, whose confirmations carry a witness of the state they confirmed;
  - the load gate, a pure state machine the audio thread steps once a block;
  - `ProjectNote`, the one line the screen shows for each outcome.
- **Storage shell** (`project/store.rs`): save, load, list, delete and boot over plan 1's `Store`, `Card` and A/B files. It is generic over `Store`, so `MemStore` and `DirStore` both run it on the host.
- **Hardware and desktop shells:** they step the gate in the audio callback, tag each published snapshot with the epoch, and boot the last project. Neither has key handling; the navigation plan drives the entry points.

**Tech Stack:** Rust 2024, `no_std` core, no new dependency. `chimera-hal`'s `testkit` (`MemStore`), `DirStore`, `just`.

**Spec:** `docs/superpowers/specs/2026-09-28-projects-storage-design.md` (approved, binding except navigation). Sections: § Model, § Types decide what's possible, § Derived marks, § Copy rules, § Unsaved-state edge cases, § Storage (Format, Saving, SYSTEM, Errors), § Loading while playing, § Boot, § Tests. Navigation: ADR 0044 (Proposed, branch `nav-ladder-adr`, `docs/adr/0044-one-ladder-one-button-map.md`) and that branch's amendment to the spec. Plan 1 (shipped, PR #202): `docs/superpowers/plans/2026-09-28-storage-foundation.md`, whose `Store`, `Card`, `save_ab`/`load_ab`, codec and `SystemSync` this plan consumes. ADRs 0045 (card format) and 0048 (own FAT layer).

> **Scope.** This is the non-navigation half of the spec's plan 2. `Location`, the ladder's rungs, the key table and MENU tap/hold are out; see § Deferred to the navigation plan. Plan 3 is out too: the library, tags (the spec's `Sound.tags: TagSet` and the custom tag names record) and naming.
>
> **Execution order.** `modal2-resonators` merges to main first. This plan runs on `projects-core` only after that branch is rebased onto main with Modal 2 in it (§ Rebase onto Modal 2).

## Global Constraints

Copied from the spec. Every task's requirements include this section.

**Model**
- `Project { meta: ProjectMeta, pool: Pool, perf: Performance }`, where `ProjectMeta { id: Option<ProjectId>, name: ProjectName, saved_crc: Option<u32> }`. `Performance.name` moves into `ProjectMeta`. `AudioShared::update_from` keeps reading only `project.perf()`.
- `UiState.performance` and `UiState.pool` merge into one private `project: Project`.
- `SoundPool` becomes `Pool`: 32 slots indexed by `SlotId`, with a `u16` generation per slot. `store` and `clear` bump the slot's generation. `clear` refuses a slot a Part uses.
- `Part { sound, origin: Origin, mix }`. `Origin = Slot { slot, generation, crc } | Init(EngineType)`, set only by load, save-over-slot and revert. Nothing else changes in `Part`.
- `active_part: usize`, `Performance::edit(usize)` and every other Part index become `PartId`.
- `BlockRef::Channels`, `impl Block for [Part; MAX_PARTS]` and System › MIDI Setup are deleted. CHANNEL is edited on the mixer's PART page.
- "Every rule here is carried by a type or by a pure function, never by a stored flag that could drift."
- `Project`'s fields are private. Edits go through `&mut` freely, because the marks are derived. Only the replaces can lose work, and each one needs `Confirmed`.

**Derived marks**
- Part `*`: the sound differs bit for bit from its pool slot (`Sound::bits_eq`). `part_status`:
  - `Init(e)` is `Clean` if the sound `bits_eq(Sound::init(e))`, else `Edited`;
  - `Slot`, and the sound is bit-equal to the slot: `Clean`;
  - the generation is unchanged: `Edited`;
  - the generation moved and `crc(sound) == crc`: `Stale(slot)`;
  - the generation moved and the Part was edited: `Edited`.
- Project status:
  - `Pristine` when the canonical CRC equals `template_crc`;
  - `Saved` when it equals `meta.saved_crc`;
  - else `Modified`.
- Saving a project doesn't clear Part marks. Pool changes, FX, mix and name edits mark the project. THEME never does. A load sets every `Origin::Slot` to the slot's current generation and CRC.
- Two prompts:
  - `+ NEW` or a project load while `Modified` asks **SAVE THEN LOAD / LOAD ANYWAY / CANCEL**;
  - a slot loaded into an `Edited` Part asks **SAVE PART FIRST / REPLACE / CANCEL**.
- `Pristine` and `Saved` never ask. A cancelled or failed save aborts the load.
- On a card swap, "any `Pending` replace re-validates".

**Format** (plan 1's rules, unchanged)
- Header, TLV records (`u16` tag, `u16` length), CRC trailer.
- Tag bit 15 is must-understand.
- Enums are stored by frozen code.
- Continuous values are stored as `f32` in spec units, clamped and quantised through their `ParamSpec`.
- Decoding starts from a frozen neutral base.
- Counts are bounded.
- The parser never panics.
- Project records: meta, the pool (sound records per filled slot), the six Parts (origin slot or INIT engine, then the mix), FX. Reserved: tempo, the AFX map per Part, the CC map, the set list, and (plan 3) the custom tag names.
- `FileKind` 2 is reserved for Project (ADR 0045). The files are `/CHIMERA/PROJECTS/P0000001.A` and `.B`; the name is in the header. Ids are the highest existing id + 1.

**RAM and saving**
- No staging copy and no serialised buffer. Saves stream from live state.
- Nothing new goes in D2. Every new AXI static is counted in `AXI_RESIDENT`.
- No stack frame of 8 KB or more (`just stack-check`).
- The UI loop is blocked while it saves or loads; the audio keeps playing from its last snapshot.
- A save sets `meta.saved_crc` and writes the project's id into SYSTEM as the last project. SYSTEM is also written on a project load.

**Loading while playing**
1. **Validate.** On error, RAM is untouched.
2. **Parse** into the UI's project, and don't publish.
3. **Bump** `LOAD_EPOCH`.
4. **Fade.** On a new epoch the audio kills every voice (the 128-sample fade, through the old mix and FX), clears `waiting` and stores `LOAD_ACK`.
5. **Publish.** The UI waits for `LOAD_ACK` (≤ 10 ms), then publishes. The gap is at most one block.

- From the epoch to the publish, the audio doesn't drain the note queues; note-ons then play on the new project.
- A note-off for a killed voice is ignored. A held key isn't retriggered.
- If the card fails during step 2, the UI falls back to a NEW project with "LOAD FAILED: <file>", then publishes through steps 3–5.
- After any load the renderer's animators snap (CLAUDE.md's only exception to "never snap").
- FX params switch with the snapshot; the delay and reverb buffers aren't cleared. This plan lists every FX param that would step under a live tail, and either smooths it or records the accepted step in the ADR.

**Boot**
1. SYSTEM first (plan 1).
2. Load the last project id, without the fade (nothing sounds yet).
3. No id, no card, or any load error starts a NEW project and shows the reason.
4. NEW is `Performance::new` plus a pool of the factory Sounds and one INIT Sound per engine. Its CRC is `template_crc`, so it is `Pristine`.

**Errors reach the screen, never the audio**
- A card error, a full card, a bad CRC or an unsupported format shows a message that names the file. A file that fails to parse or needs newer firmware is listed with its error.

**Repo rules (CLAUDE.md, owner)**
- Every `unsafe` has a `// SAFETY:` comment. No heap, no blocking and no allocation on the audio path. No libc.
- Invariants live in types. Pure core, thin shells.
- Decisions that constrain later work get an ADR; never edit an accepted one. Follow-ups go in GitHub issues (joegiralt/chimera), referenced by URL.
- **Commits:**
  - a terse plain sentence, with no type prefix and no Claude or AI attribution;
  - files are staged by name or by the source directories each task lists (`chimera-core/src`, `chimera-core/tests`, `chimera-stm32/src`, `chimera-desktop/src`), none of which holds `chimera.bin` or the UX spec;
  - run `git status` first. **Never stage `docs/chimera-ui-ux-spec.md` or `chimera.bin`.**
- `just check` passes before every commit. No task ends red.
- **Hardware:** desktop QA first, then one combined flash when the branch ships. This plan has no chip STOP; its chip checks go to the navigation plan's ship flash (Task 9).

## Review Focus

These are the failure modes the spec implies but none of its tests exercise, most likely first. Each has a test in the task named.

1. **The card changes between pass 1 and pass 2 of a project load.** Pass 2 has already written part of the UI's project. The project must end up either loaded or NEW, never half of each, and the audio must still get its epoch and publish. Task 6: `pass_two_failure_falls_back_to_new`, and `load_failure_still_publishes` in `load_protocol_test.rs`.
2. **An edited Part is saved and reloaded, then its slot is overwritten by another Part.** If a load set `Origin.crc` from the Part's own sound, the Part would derive `Stale`. UPDATE needs no prompt, so it would silently drop the edits. `Origin.crc` is the slot's CRC. Task 3: `edited_part_never_stale_after_reload`.
3. **The card is swapped between a load prompt and its confirmation.** The pending load must not run on the new card's `P000000n` and replace RAM in place. It is refused with `CARD CHANGED`, and the project is untouched. Task 6: `swapped_card_refuses_a_pending_load`.
4. **A confirmation is used after its target changed.** For example, a Part edited between `check` (Clean) and `replace_part`. The replace is refused, not run without the prompt it would now need. Task 4: `confirmed_then_edited_is_refused`.
5. **The audio doesn't ack within 10 ms**, because the callback stalled or an epoch was bumped during a fade. The UI publishes anyway, the gate reopens on seeing that epoch, and the notes are drained again. The synth is never left muted. Task 5: `timeout_publish_reopens`, `second_epoch_restarts_the_fade`.

## Decisions this plan makes where the spec is silent or ambiguous

- **A project load applies in place.** A `Project` is about 34 KB and there is no staging copy, so pass 2 writes straight into the UI's project instead of staging (ADR 0045's "two passes, staged" clause). `load_ab_in_place` says whether pass 2 had started when an error came: pass 1 failed and RAM is as it was, or pass 2 failed and the loader resets to NEW. ADR 0046 records this as superseding 0045 in part, for projects only. Sounds and SYSTEM stay staged.
- **Sound names travel in the context records.** A Sound file keeps its name in its header, but a project holds 38 Sounds. So `Slot` and `Part` each carry the Sound's 16-byte padded name after the index. `sound_crc` hashes the padded name, then `encode_sound`'s bytes, so a rename is an edit.
- **`Origin.crc` is the source's CRC:** the slot's, or the sound just stored for a save-over or a new slot, at the moment the Origin is set. A project load recomputes it from the loaded slot. Review Focus 2 says why it can't be the Part's own.
- **Generations are never lowered.** A load clears and stores slots through `Pool::clear` and `Pool::store`, which bump the generation, so every generation moves on a load. No `Origin` or `PartAction` from before a load can match a Part after it by accident. Tests compare an `Origin` by slot and CRC (or engine), never by generation.
- **Every Part's own Sound is stored,** not only the ones that differ from their slot. That is six Sounds more per file (about 16 % more bytes and CRC work), and no cross-record reference to get wrong.
- **`Confirmed<R>` is generic over the replace target, and it carries a witness.**
  - The spec's single `Replace` enum becomes two target types, `PartSource` and `ProjectSource`, so a project confirmation can't be passed to a Part replace (a compile-fail test).
  - A confirmation holds a private witness of what it confirmed: the Part's `sound_crc`, or the project's `project_crc`. It also holds the card's `VolumeId` for a file load (`ProjectSource::File { id, vol }`).
  - The replace refuses if any of them moved. That is how a `Pending` re-validates.
  - `PartFromLibrary` waits for plan 3's `LibId`. The legacy sound browser's INIT rows become `PartFrom::Init(engine)`.
- **An empty slot is a runtime refusal,** not a type. A `Pending` re-validates on confirm, and the slot may have been cleared in between, so `replace_part` returns `Err(ReplaceError::SlotEmpty)`.
- **`PartAction` is opaque.**
  - Only `part_actions` builds one, and `apply_part_action` re-checks that it still applies.
  - A `Stale` Part offers UPDATE (`Revert`) and a new slot. It never offers `OverSlot`, which would undo the other Part's save.
  - The set a save-over returns is "the other users of the slot that now derive `Stale`".
  - `ToLibrary` is plan 3's.
- **The epoch is written by the project load functions only.** `LoadLink::bump` is `pub(in crate::project)`; no `Project` method writes it, which keeps `Project` free of statics. The UI tags every snapshot with the latest epoch. The gate reopens when a snapshot carries the epoch it is waiting for.
- **The project list reads headers only** (one block per side), as the spec says.
  - A parse error that needs the whole file shows when the load fails.
  - A torn newer side's header name is listed even though a load would fall back to the older side.
  - Each id costs two directory lookups: O(n²) directory sectors for n projects. That is acceptable for tens of projects; it is measured at the navigation plan's ship flash.
  - The list is a callback, so the navigation plan decides what to keep in RAM.
- **NEW:**
  - its name is `NEW PROJECT`;
  - its pool is the 8 factory Sounds in slots 1–8, then INIT Algo and INIT Modal (`EngineType::ALL` order) in slots 9–10;
  - both INIT slots are named `INIT`, and pool rows tell them apart by engine, as the browser already does.
- **SYSTEM's last project** changes only on a successful save or file load. `+ NEW` and a fallback to NEW leave it, so the next boot reloads the last project that was on the card. A failed SYSTEM write after a good project save doesn't change the toast; the next save or load retries it.
- **Copy.**
  - Part and slot numbers are 1-based on screen, slots in two digits.
  - The saved toast is `SAVED`, `SAVED: P2 DIFFERS FROM SLOT 03`, or `SAVED: 3 PARTS DIFFER FROM SLOTS`.
  - Every error line ends with `: ` and the project's name, or the file stem (`P0000007`) when no name is known.
- **The generation is a wrapping `u16`.** 65 536 stores to one slot between a Part's load and its next look could alias a generation. That is accepted.
- **Controller state.** MIDI CC and pitch bend are dropped at the note queue today (`note_queue.rs:66`), so there is nothing to keep across a swap.

## Deferred to the navigation plan

Navigation follows ADR 0044 (Proposed) and the spec as amended on `nav-ladder-adr`:
- B*n* toggles between Part *n*'s sound pages and its rung, the mixer (PART and SENDS).
- MIX+B*n* goes straight to the rung.
- The mixer pages stay.

The navigation plan calls this plan's APIs and owns:
- `Location`, the rungs, breadcrumb, legend, the key table, MENU and SEQ tap and hold, press timestamps and edge latching.
- Folding `ChainId::Mixer(i)` into `Location::Part(PartId, MixPage)` and the Mixer chain's FX nodes into `Fx`. The mixer's PART and SENDS pages themselves stay. Folding `UiMode::SoundBrowser` into `Location`.
- The prompt screens for `NeedsConfirm` (SAVE PART FIRST / REPLACE / CANCEL; SAVE THEN LOAD / LOAD ANYWAY / CANCEL) and the Part menu built from `part_actions`.
- "P4 ALSO USES SLOT 03: UPDATE P4 / LEAVE"; the model returns that `PartSet`.
- NAME EXISTS / KEEP BOTH / OVERWRITE THAT ONE; the model gives `list_projects` and `save_project` to any id.
- Retiring the legacy browser's `Pending::anyway` bypass (Task 4 files the issue).
- What triggers SAVE, SAVE AS, `+ NEW`, a load and a delete. SAVE AS naming is plan 3's naming screen.
- Showing `*` and `◦`, the per-input-frame recompute of `project_status`, the Project rung's stats strip, the NO CARD rung, the greyed project list and the "NOT SAVED" footer.
- The UX spec amendment. This plan never stages `docs/chimera-ui-ux-spec.md`.
- **Every chip check, at its single ship flash** (the list is in Task 9).

(The deletion of `BlockRef::Channels` and System › MIDI Setup is *not* deferred. It has no navigation dependency under ADR 0044, and Task 1 needs it: without Channels, `UiState` no longer needs `&mut Performance`.)

## Rebase onto Modal 2

`modal2-resonators` merges to main first. This plan executes after `projects-core` is rebased onto that main; nothing here is written against pre-Modal-2 code. The overlaps:

| File (changed by Modal 2) | Here | What to do |
|---|---|---|
| `storage/block_codec.rs`, `codes.rs`, `mod.rs` (`decode_block` gains `translations`; `TRANSLATIONS`) | Task 2 calls `decode_block` for Part mix and FX blocks | Pass `TRANSLATIONS` at every new call site. |
| `storage/sound.rs` (its `decode_block` call) | Task 2 makes `SoundCheck::{new, step, finish}` `pub(crate)` and adds `sound_crc` | Keep both edits. |
| `storage/system.rs` (its `decode_block` call) | Task 2 rejects the project tags there | Keep both edits. |
| `tests/codec_compat_test.rs` (+106) | Task 2 extends `write_v1_fixtures` | Append; the project gets its own constants. |
| `tests/common/mod.rs` (+183) | Task 2 adds `tests/common/project.rs` | A new file; `mod project;` is the only shared line. |
| `tests/memory_budget_test.rs` (+30) | Task 1 | Replace the `Performance` and `SoundPool` rows on top of Modal 2's. |
| `chimera-stm32/src/bench.rs` (+178) | Tasks 1, 5, 9 | `ui.project().perf()`; add the `PROJ CRC` row after Modal 2's. |
| `ui/browser.rs`, `ui/renderer.rs`, `ui/chain.rs`, `ui/block_registry.rs`, `ui/mod.rs` | Task 1 | See the next paragraph. |

**Modal 2's Task 15** adds the Part-button toggle (`next_on_part_button`) and `ChainNav::go`, which rewrite the Part-index code that Task 1 retypes. Task 1 **adopts** them. It retypes their Part indexes to `PartId` (the toggle takes and returns `PartId`, and `go` is called with one) rather than keeping a parallel `usize` path or re-implementing the toggle. Any per-Part memory Task 15 adds (for example the last-used mixer page) is indexed by `PartId::index()`.

- `ModalParams`'s new codes 12–20 don't touch the project codec, which only calls `decode_block`.
- The project fixture is written after the rebase, so it holds Modal 2's INIT Modal. Its test compares slots with the v1 `.snd` decodes, which Modal 2's translation applies to as well.
- **Dependency for the QA gate:** the parameter-sweep harness (`chimera-core/tests/param_sweep_test.rs`, from branch `param-sweep` via `modal2-resonators`) must be on main before Task 9's QA gate runs.

## File structure

| File | Responsibility |
|---|---|
| `chimera-core/src/project/mod.rs` | `Project`, `ProjectMeta`, `PartFrom`, `PartSource`, `InUse`, `ReplaceError`; the edit, pool and part operations. |
| `chimera-core/src/project/ids.rs` | `PartId`, `SlotId`, `PartSet`. |
| `chimera-core/src/project/pool.rs` | `Pool`. |
| `chimera-core/src/project/perf.rs` | `Origin`, `Part`, `Performance`, `PartEdit`, `part_block(_mut)` (moved from `preset.rs`, which re-exports them). |
| `chimera-core/src/project/codec.rs` | Project records: `encode_project`, `ProjectCheck`, `ProjectDecoder`, `project_crc`, `TemplateCrc`. |
| `chimera-core/src/project/marks.rs` | `PartStatus`, `ProjectStatus`, `PartAction`, `PartActions`, the pure mark functions. |
| `chimera-core/src/project/guard.rs` | `ReplaceGuard`, `Target`, `Prompt`, `Confirmed`, `NeedsConfirm`, `Pending`. |
| `chimera-core/src/project/swap.rs` | `LoadLink`, `LOAD_LINK`, `Swap`, `Settled`, `LoadGate`, `GateStep`. |
| `chimera-core/src/project/note.rs` | `Line`, `Subject`, `Differ`, `ProjectNote`. |
| `chimera-core/src/project/store.rs` | `project_file`, `new_project_id`, `save_project`, `load_project`, `list_projects`, `delete_project`, `boot_project`. |
| `chimera-core/src/project/test_support.rs` (`test-support`) | `full()`, `same()`, `FullOnWrite`, `FlipOnSecondRead`, `project_store_suite`: shared by core, `chimera-fat` and desktop tests. |
| `chimera-core/src/storage/file.rs` | + `DecodeInPlace`, `InPlaceError`, `load_ab_in_place`. |
| `chimera-core/src/storage/{frame,record,sound,crc}.rs` | `FileKind::Project`; four record tags; `sound_crc`; a shared `CrcSink`. |
| `chimera-core/src/instrument.rs`, `voice_alloc.rs` | `AudioShared.epoch`; `kill_all`, `quiet`. |
| `chimera-core/src/dsp/{fx_bus,delay}.rs` | Smoothed FX returns and delay time. |
| `chimera-core/src/ui/{mod,busy,block_registry,mod_grid,chain}.rs` | `UiState` holds one `Project`; the entry points; `Toast` carries a `Line`; MIDI Setup goes. |
| `chimera-stm32/src/{main,shared,bench}.rs`, `audio/engine.rs` | The gate, the epoch tag, the boot load, the bench row. |
| `chimera-desktop/src/{main,audio,store}.rs` | The gate, the epoch tag, the boot load, the relaunch test and the store suite on `DirStore`. |
| `chimera-core/tests/project_*_test.rs`, `load_protocol_test.rs`, `fx_swap_test.rs`, `tests/common/project.rs` | The tests; `common/project.rs` re-exports `test_support` and adds the byte helpers. |
| `chimera-core/tests/fixtures/v1/project.prj` | The v1 project fixture. |
| `chimera-core/fuzz/fuzz_targets/decode_project.rs`, `Justfile` | The `cargo fuzz` target; `just fuzz` runs both targets. |
| `docs/adr/0043-projects-own-their-sounds.md`, `docs/adr/0046-project-load-protocol.md` | The ADRs, added to `docs/adr/README.md`. |

## Task order

1. The project model; `UiState` holds one `Project`; Channels and MIDI Setup go; the AXI budget.
2. The project file codec, `project_crc` and `TemplateCrc`.
3. Derived marks and the Part actions.
4. `ReplaceGuard` and ADR 0043.
5. The load gate on the audio side, and ADR 0046.
6. Save, load, list and delete on the card; the notes.
7. FX at the swap.
8. Boot, the `UiState` entry points and both shells.
9. The bench row and the desktop QA gate.

Each depends on the one before it, except Task 7, which needs only Task 5.

---

### Task 1: The project model; `UiState` holds one `Project`

**Files:**
- Create:
  - `chimera-core/src/project/{mod,ids,pool,perf}.rs`;
  - `chimera-core/tests/project_model_test.rs`.
- Modify, in `chimera-core/src`:
  - `lib.rs` (`pub mod project;`);
  - `preset.rs`: keeps `Sound`. `Part`, `Performance`, `PartEdit` and `part_block(_mut)` move to `project/perf.rs` and are re-exported. `SoundPool`, `impl Block for [Part; MAX_PARTS]` and `CHANNEL_SPECS` go;
  - `part.rs` (drop `CHANNEL_SPECS`);
  - `addr.rs`, `storage/codes.rs` and `ui/mod_grid.rs`: `BlockRef::Channels` and its arms go. `disk_code` keeps its `Option` return, and no code changes;
  - `ui/block_registry.rs` and `ui/chain.rs`: `SYS_MIDI` and the System chain's MIDI Setup node go;
  - `factory.rs` (drop `load_factory`);
  - `storage/crc.rs`: `pub(crate) struct CrcSink`, moved from `system.rs`;
  - `storage/sound.rs` (+ `sound_crc`);
  - `instrument.rs`: `AXI_RESIDENT` counts `size_of::<Project>()`, not `Performance` + `SoundPool`;
  - `ui/mod.rs`, `ui/renderer.rs`, `ui/browser.rs`.
- Modify the shells: `chimera-stm32/src/{main,shared,bench,sd_probe}.rs`; `chimera-desktop/src/main.rs`.
- Modify every test the compiler names. That is the grep for `.performance`, `.pool`, `active_part`, `loaded_from`, `SoundPool`, `Channels` and `SYS_MIDI`, including:
  - `preset_test`, `mixer_page_test`, `browser_test`, `ui_routing_test`, `instrument_test`, `common/rig.rs`;
  - `midi_channel_test` (its MIDI Setup cases go; CHANNEL is tested on the PART page);
  - `header_map_test`, `all_pages_walk_test`;
  - `memory_budget_test`;
  - `screen_golden_test` (the System goldens re-recorded without MIDI SETUP).

**Interfaces:**
- Consumes:
  - `Sound`, `Sound::bits_eq`, `encode_sound`, `ProjectId`, `ProjectName` (plan 1);
  - after the rebase, Modal 2's `next_on_part_button` and `ChainNav::go` (§ Rebase onto Modal 2).
- Produces:

```rust
// ids.rs — each Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord
pub struct PartId(u8);
impl PartId { pub const ALL: [PartId; MAX_PARTS]; pub const fn new(n: u8) -> Option<PartId>; pub const fn index(self) -> usize; }
pub struct SlotId(u8);
impl SlotId { pub const ALL: [SlotId; POOL_SIZE]; pub const fn new(n: u8) -> Option<SlotId>; pub const fn index(self) -> usize; }
pub struct PartSet(u8);
impl PartSet { pub const EMPTY: PartSet; pub fn with(self, p: PartId) -> PartSet; pub fn contains(self, p: PartId) -> bool;
               pub fn len(self) -> usize; pub fn iter(self) -> impl Iterator<Item = PartId>; }
// pool.rs
pub struct Pool { slots: [Option<Sound>; POOL_SIZE], gens: [u16; POOL_SIZE] }
impl Pool {
    pub fn init_in_place(slot: &mut MaybeUninit<Pool>) -> &mut Pool;   // empty, gens 0
    pub fn get(&self, s: SlotId) -> Option<&Sound>;
    pub fn generation(&self, s: SlotId) -> u16;
    pub fn first_free(&self) -> Option<SlotId>;
    pub fn used(&self) -> usize;
    pub(in crate::project) fn store(&mut self, s: SlotId, sound: Sound);  // gen wrapping_add(1)
    pub(in crate::project) fn clear(&mut self, s: SlotId);                // gen wrapping_add(1), even when empty
}
// perf.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin { Slot { slot: SlotId, generation: u16, crc: u32 }, Init(EngineType) }
pub struct Part { pub sound: Sound, origin: Origin, pub mix: PartParams }
impl Part { pub fn origin(&self) -> Origin; }
pub struct Performance { pub parts: [Part; MAX_PARTS], pub fx: FxParams }   // no `name`
impl Performance { pub fn new() -> Self; pub fn part(&self, p: PartId) -> &Part; }
// storage/sound.rs
pub fn sound_crc(s: &Sound) -> u32;   // CRC32 of s.name.padded(), then encode_sound's bytes
// mod.rs
pub const NEW_NAME: &str = "NEW PROJECT";
pub struct ProjectMeta { id: Option<ProjectId>, name: ProjectName, saved_crc: Option<u32> }
impl ProjectMeta { pub fn id(&self) -> Option<ProjectId>; pub fn name(&self) -> ProjectName; pub fn saved_crc(&self) -> Option<u32>; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PartFrom { Slot(SlotId), Init(EngineType) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct PartSource { pub part: PartId, pub from: PartFrom }
#[derive(Debug, PartialEq)] pub struct InUse(pub PartSet);
#[derive(Debug, PartialEq)] pub enum ReplaceError { SlotEmpty, Changed }   // Changed: Task 4
pub struct Project { meta: ProjectMeta, pool: Pool, perf: Performance }
impl Project {
    pub fn init_in_place(slot: &mut MaybeUninit<Project>) -> &mut Project;  // NEW (Task 2 adds the TemplateCrc)
    #[cfg(any(test, feature = "test-support"))] pub fn boxed() -> Box<Project>;
    pub fn meta(&self) -> &ProjectMeta; pub fn perf(&self) -> &Performance; pub fn pool(&self) -> &Pool;
    pub fn part(&self, p: PartId) -> &Part;
    pub fn edit_part(&mut self, p: PartId) -> PartEdit<'_>;
    pub fn edit_fx(&mut self) -> &mut FxParams;
    pub fn set_name(&mut self, n: ProjectName);
    pub fn users(&self, s: SlotId) -> PartSet;                    // Parts whose Origin is Slot(s)
    pub fn pool_store(&mut self, s: SlotId, sound: Sound);
    pub fn pool_clear(&mut self, s: SlotId) -> Result<(), InUse>;
    /// Stores a copy of p's sound in s and sets p's Origin fresh. Returns the
    /// other users of s that now derive Stale(s).
    pub fn save_part_to(&mut self, p: PartId, s: SlotId) -> PartSet;
    pub fn load_part(&mut self, src: PartSource) -> Result<(), ReplaceError>;  // unguarded until Task 4 makes it private
    pub(crate) fn reset_new(&mut self);
}
```

Private fields make the rules hold: only `project` sets an `Origin` or touches `gens`, and nothing outside it can build a `Part`. `init_in_place` and `reset_new` write the pool slot by slot, so the 28 KB pool never passes through the stack. NEW is:
- name `NEW_NAME`, with id and `saved_crc` `None`;
- the 8 `factory_sound`s in slots 0–7, and `Sound::init(e)` for each `EngineType::ALL` in slots 8 and 9;
- `Performance::new()` with every Origin `Init(EngineType::Algo)`, and `FxParams::default()`.

`save_part_to` computes "now Stale" with `part_status`'s rule inline (the generation moved and the Part's `sound_crc` equals its Origin's `crc`), because `marks.rs` arrives in Task 3, which then calls the shared function.

`UiState`'s blocks view becomes `UiBlocks { edit: PartEdit<'a>, theme: &'a mut ThemeSettings }`. With Channels gone it needs no `&mut Performance`, and `Project` lends none.

- [ ] **Step 1: Write the failing tests** in `project_model_test.rs`:

```rust
#[test]
fn ids_are_bounded() {
    assert_eq!(PartId::new(5).map(PartId::index), Some(5));
    assert_eq!(PartId::new(6), None);
    assert_eq!(SlotId::new(31).map(SlotId::index), Some(31));
    assert_eq!(SlotId::new(32), None);
    assert_eq!((PartId::ALL.len(), SlotId::ALL.len()), (6, 32));
}

#[test]
fn new_project_contents() {
    let p = Project::boxed();
    assert_eq!(p.meta().name().as_str(), "NEW PROJECT");
    assert_eq!((p.meta().id(), p.meta().saved_crc()), (None, None));
    for i in 0..FACTORY_LEN {
        assert!(p.pool().get(SlotId::ALL[i]).unwrap().bits_eq(&factory_sound(i).unwrap()));
    }
    assert!(p.pool().get(SlotId::ALL[8]).unwrap().bits_eq(&Sound::init(EngineType::Algo)));
    assert!(p.pool().get(SlotId::ALL[9]).unwrap().bits_eq(&Sound::init(EngineType::Modal)));
    assert_eq!(p.pool().used(), 10);
    for id in PartId::ALL {
        assert_eq!(p.part(id).origin(), Origin::Init(EngineType::Algo));
    }
}

#[test]
fn store_and_clear_bump_the_generation() {
    let mut p = Project::boxed();
    let s = SlotId::ALL[20];
    let g = p.pool().generation(s);
    p.pool_store(s, Sound::init(EngineType::Modal));
    assert_eq!(p.pool().generation(s), g.wrapping_add(1));
    assert_eq!(p.pool_clear(s), Ok(()));
    assert_eq!(p.pool().generation(s), g.wrapping_add(2));
}

#[test]
fn clear_refuses_a_used_slot() {
    let mut p = Project::boxed();
    let (a, b, s) = (PartId::ALL[1], PartId::ALL[3], SlotId::ALL[0]);
    for part in [a, b] {
        p.load_part(PartSource { part, from: PartFrom::Slot(s) }).unwrap();
    }
    assert_eq!(p.pool_clear(s), Err(InUse(PartSet::EMPTY.with(a).with(b))));
    assert!(p.pool().get(s).is_some());
}

#[test]
fn load_sets_origin_from_the_slot() {
    let mut p = Project::boxed();
    let (part, s) = (PartId::ALL[0], SlotId::ALL[2]);
    p.load_part(PartSource { part, from: PartFrom::Slot(s) }).unwrap();
    let crc = sound_crc(p.pool().get(s).unwrap());
    assert_eq!(p.part(part).origin(), Origin::Slot { slot: s, generation: p.pool().generation(s), crc });
    assert!(p.part(part).sound.bits_eq(p.pool().get(s).unwrap()));
    let empty = PartSource { part, from: PartFrom::Slot(SlotId::ALL[30]) };
    assert_eq!(p.load_part(empty), Err(ReplaceError::SlotEmpty));
}

#[test]
fn save_part_to_returns_only_the_parts_now_stale() {
    let mut p = Project::boxed();
    let s = SlotId::ALL[0];
    let [a, b, c] = [PartId::ALL[0], PartId::ALL[2], PartId::ALL[4]];
    for part in [a, b, c] {
        p.load_part(PartSource { part, from: PartFrom::Slot(s) }).unwrap();
    }
    p.edit_part(c).part.sound.params.filter.cutoff *= 0.5;  // c is Edited
    p.edit_part(a).part.sound.params.filter.cutoff *= 0.25;
    assert_eq!(p.save_part_to(a, s), PartSet::EMPTY.with(b));   // c, edited, isn't Stale
}

#[test]
fn sound_crc_sees_the_name() {
    let mut s = Sound::init(EngineType::Algo);
    let before = sound_crc(&s);
    s.name = SoundName::new("RENAMED").unwrap();
    assert_ne!(sound_crc(&s), before);
}
```

  Add the compile-fail doc tests on `Part` and `Pool` (in `perf.rs` and `pool.rs`):
  - ```` ```compile_fail,E0451 ```` building `Part { sound, origin, mix }` outside the module;
  - ```` ```compile_fail,E0624 ```` `pool.store(s, sound)` outside `project`.

  In `memory_budget_test.rs`:
  - `axi_counts_the_project`: `axi_residents_fit`'s row list has `Project` in place of `Performance` and `SoundPool`; `size_of::<Project>() - (size_of::<Performance>() + POOL_SIZE * size_of::<Option<Sound>>())` is ≤ 128 B (the meta and generations); and `AXI_SRAM - AXI_RESIDENT >= 64 * 1024`;
  - `ui_state_fits_the_ui_reserve` stops naming `Performance` and `SoundPool`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_model_test --test memory_budget_test` → FAIL (no `project` module).
- [ ] **Step 3: Implement** the model as above. Then migrate the callers:
  - `UiState` replaces `performance`, `pool` and `active_part: usize` with a private `project: Project` and `active_part: PartId`. It adds `pub fn project(&self) -> &Project` and `pub fn project_mut(&mut self) -> &mut Project`. `field_list!` and `init_in_place` follow, with `project` built in place.
  - Every `usize` Part index in `ui/` becomes `PartId`: `blocks`, `load_matrix`, `sync_mod_state`, `UiMode::SoundBrowser { part }`, `renderer`'s `active_part`, and Modal 2's Part-button toggle and `ChainNav::go`. `ChainId::Part(i)` and `ChainId::Mixer(i)` keep their `usize`, which is navigation's to fold; convert at that boundary with `PartId::new(i as u8)`.
  - Delete Channels and MIDI Setup (Files above).
  - The legacy browser's Load calls `project.load_part(PartSource { part, from })`, with `PartFrom::Init(engine)` for its INIT rows. Its Save calls `project.save_part_to(part, slot)`.
  - Use `update_from(ui.project().perf())` in both shells and in `shared::take_audio`, and `bench::run(.., ui.project().perf())`.
  - `Performance.name` goes; `preset_test`'s name assert moves to `meta().name()`.
- [ ] **Step 4: Run** the tests → PASS. Run `SCREEN_DUMP=… just screens` for the System goldens that lose MIDI SETUP and check them by eye. Then `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-stm32/src chimera-desktop/src
git commit -m "One Project holds the pool and the Parts; Parts and slots are typed ids"
```

### Task 2: The project file codec, `project_crc` and `TemplateCrc`

**Files:**
- Create:
  - `chimera-core/src/project/codec.rs` and `chimera-core/src/project/test_support.rs`;
  - `chimera-core/tests/common/project.rs` and `chimera-core/tests/project_codec_test.rs`;
  - `chimera-core/tests/fixtures/v1/project.prj`;
  - `chimera-core/fuzz/fuzz_targets/decode_project.rs`.
- Modify, in `chimera-core/src`:
  - `storage/frame.rs` (`FileKind::Project = 2`);
  - `storage/record.rs` (the four tags);
  - `storage/sound.rs`: `SoundCheck::{new, step, finish}` become `pub(crate)`, and `step` takes an `Option<SoundName>` override for the name. `Slot`, `Part`, `Fx` and `Origin` are `Corrupt` in a Sound file;
  - `storage/system.rs` (the same four tags are `Corrupt` in SYSTEM);
  - `storage/file.rs` (`DecodeInPlace`);
  - `storage/mod.rs` (exports);
  - `dsp/fx_bus.rs` and the five FX param structs: derive `PartialEq`;
  - `project/mod.rs`: `init_in_place` and `reset_new` return `TemplateCrc`, and `boxed` returns the pair;
  - `ui/mod.rs`: `template: TemplateCrc` and `pub fn template(&self) -> TemplateCrc`.
- Modify the tests and tooling:
  - `tests/common/mod.rs` (`pub mod project;`);
  - `tests/project_model_test.rs` (`boxed` now returns the pair);
  - `tests/storage_frame_test.rs` (the tag table);
  - `tests/codec_compat_test.rs` (`write_v1_fixtures`, `PROJECT_FIXTURE_BYTES`);
  - `tests/codec_fuzz_test.rs`;
  - `chimera-core/fuzz/Cargo.toml`;
  - `Justfile` (`fuzz` runs both targets).

**Interfaces:**
- Consumes:
  - from plan 1: `encode_sound`, `SoundCheck`, `encode_block`/`decode_block` (with `TRANSLATIONS` after the rebase), `MIGRATIONS`, `RecordWriter`, `Check`, `Event`, `FileError`, `Framer`;
  - from Task 1: `Project`, `Pool`, `Origin`, `sound_crc`.
- Produces:

```rust
// record.rs (frozen codes)
RecordTag::Slot   => 0x8007,  // payload: slot u8 (0..32), then the Sound's name, 16 B padded; its sound records follow
RecordTag::Part   => 0x8008,  // payload: part u8 (0..6), then the Sound's name, 16 B; sound records, Block(Part), Origin follow
RecordTag::Fx     => 0x8009,  // empty payload; FX Block records follow
RecordTag::Origin => 0x000A,  // payload: [0, slot] or [1, engine code]
// file.rs
/// Pass 2 in place: writes the target as it hears events. Used only where
/// staging can't fit (a project); `load_ab_in_place` (Task 6) says whether
/// the target was touched.
pub trait DecodeInPlace: Check {
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError>;
    fn finish(&mut self) -> Result<(), FileError>;
}
// project/codec.rs
pub fn encode_project(p: &Project, w: &mut RecordWriter<'_>) -> Result<(), StoreError>;
pub fn project_crc(p: &Project) -> u32;           // name.padded(), then encode_project's bytes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateCrc(u32);                      // private: only init_in_place and reset_new make one
impl TemplateCrc { #[doc(hidden)] pub fn get(self) -> u32; }
pub struct ProjectCheck { /* SoundCheck, context, seen slots/parts, fx seen */ }   // impl Check, KIND = Project
pub struct ProjectDecoder<'a> { /* ProjectCheck, target: &'a mut Project */ }     // impl Check + DecodeInPlace
impl<'a> ProjectDecoder<'a> { pub fn new(target: &'a mut Project) -> Self; }
// project/test_support.rs (feature test-support), re-exported by tests/common/project.rs
pub fn full() -> (Box<Project>, TemplateCrc);
pub fn same(a: &Project, b: &Project);            // panics with the first difference
```

**Record order** as written:
1. `Fx`, then the five FX `Block`s.
2. For each filled slot in order: `Slot(n, name)`, then `encode_sound`.
3. For each Part in order: `Part(n, name)`, `encode_sound`, `Block(Part)` (its mix), then `Origin`.

**Reserved records.** The reserved records (tempo, AFX map, CC map, set list, custom tag names) get codes when they're defined. Low 15 bits `0x0B`–`0x0F` stay free for them, in either criticality.

**The rules:**
- **Contexts.** A context record closes the previous context: its `SoundCheck::finish` runs, which applies the routes. A context's name is valid `Name<16>` or `BadName`. It overrides the header's name for that Sound.
- **Structure.**
  - `Slot` and `Part` indexes must be in range (`Bounds` otherwise) and unique (`Corrupt`).
  - `Fx` appears at most once, and all six `Part`s are required (`Corrupt`).
  - Records before the first context are `Corrupt`.
  - A header with no name is `Corrupt`.
- **Routing inside a context.**
  - In a `Part` context, `Block` with the Part block's code goes to the mix and `Origin` goes to the origin. Every other record goes to the `SoundCheck`, so the Sound rules (Engine first, once-only records) hold per Sound.
  - In `Fx`, only FX `Block`s are allowed.
- **Pass 2's base, set at the header.**
  - Every slot is cleared through `Pool::clear`, so generations only move: **no generation is ever lowered**.
  - FX becomes `FxParams::default()`, and the name is the header's.
  - A `Slot` stores `Sound::neutral(Algo)` through `Pool::store`, then decodes into it.
  - At `Part(n)`: `Sound::neutral(Algo)` until its Engine record, and mix `PartParams::for_part(n)`.
- **Origins resolve at `finish`:**
  - `Slot(s)` with `s` filled becomes `Slot { slot: s, generation: pool.generation(s), crc: sound_crc(slot) }`;
  - `Slot(s)` with `s` empty, a missing `Origin`, or an unknown engine code becomes `Init(part's engine)`.
- **CRCs.** `project_crc` and `sound_crc` hash through `CrcSink`; neither allocates or buffers.

**The test helpers.**
- `full()`:
  - a NEW project with all 32 slots filled (factory, INIT, and edited copies with new names);
  - Parts loaded from slots 0, 3 and 9 and from INIT;
  - two Parts edited after loading, one of them renamed;
  - non-default FX and mix, and the name `FULL`.
- `same()` compares:
  - the names;
  - each slot by `bits_eq`;
  - each Part's sound (`bits_eq`) and mix;
  - its `Origin` by slot and CRC (or engine), **never by generation**;
  - its `part_status` (from Task 3 on);
  - `perf().fx` with `==`, and `project_crc`.
- `tests/common/project.rs` adds the byte helpers, `encode(&Project) -> Vec<u8>` and `decode(&[u8], &mut Project) -> Result<(), FileError>`. They run `write_file`, then a `Framer` pass 1 through `ProjectCheck`, then pass 2 through `ProjectDecoder`, as `sound_codec_test.rs`'s helpers do.

- [ ] **Step 1: Write the failing tests** in `project_codec_test.rs`:

```rust
#[test] fn new_round_trip() { let (p, _) = Project::boxed(); let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap(); same(&p, &q); }

#[test] fn full_pool_round_trip() { let (p, _) = full(); let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap(); same(&p, &q); assert_eq!(encode(&q), encode(&p)); }

#[test]
fn slot_and_part_names_round_trip() {
    let (mut p, _) = Project::boxed();
    let mut s = factory_sound(0).unwrap();
    s.name = SoundName::new("DUB-042").unwrap();
    p.pool_store(SlotId::ALL[12], s);
    p.load_part(PartSource { part: PartId::ALL[1], from: PartFrom::Slot(SlotId::ALL[12]) }).unwrap();
    p.edit_part(PartId::ALL[1]).part.sound.name = SoundName::new("DUB-043").unwrap();
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    assert_eq!(q.pool().get(SlotId::ALL[12]).unwrap().name.as_str(), "DUB-042");
    assert_eq!(q.part(PartId::ALL[1]).sound.name.as_str(), "DUB-043");
    assert_eq!(q.pool().get(SlotId::ALL[0]).unwrap().name, factory_sound(0).unwrap().name);
}

#[test]
fn a_load_never_lowers_a_generation() {
    let (p, _) = full();
    let (mut q, _) = Project::boxed();
    let before: Vec<u16> = SlotId::ALL.iter().map(|&s| q.pool().generation(s)).collect();
    decode(&encode(&p), &mut q).unwrap();
    for (s, g) in SlotId::ALL.iter().zip(before) {
        assert_ne!(q.pool().generation(*s), g, "slot {}", s.index());
    }
}

#[test]
fn origin_to_empty_slot_reads_as_init() {
    // A file whose Part 0 has Origin [0, 31] and no Slot(31) record.
    let (mut q, _) = Project::boxed();
    decode(&with_origin(PartId::ALL[0], &[0, 31]), &mut q).unwrap();
    let engine = q.part(PartId::ALL[0]).sound.engine();
    assert_eq!(q.part(PartId::ALL[0]).origin(), Origin::Init(engine));
}

#[test]
fn structure_errors() {
    assert_eq!(decode_err(&missing_part(PartId::ALL[5])), FileError::Corrupt);
    assert_eq!(decode_err(&duplicate_slot(SlotId::ALL[2])), FileError::Corrupt);
    assert_eq!(decode_err(&slot_index(32)), FileError::Bounds);
    assert_eq!(decode_err(&part_index(6)), FileError::Bounds);
    assert_eq!(decode_err(&block_before_context()), FileError::Corrupt);
    assert_eq!(decode_err(&nameless_header()), FileError::Corrupt);
    assert_eq!(decode_err(&slot_named(b"\x01bad            ")), FileError::BadName);
    let mut slot = vec![0u8];
    slot.extend_from_slice(&[0; 16]);
    assert_eq!(sound_file_decode_err(&[(RecordTag::Slot, slot)]), FileError::Corrupt);
}

#[test]
fn compatibility() {
    let (p, _) = full();
    let (mut q, _) = Project::boxed();
    decode(&with_record_in_part(&p, PartId::ALL[2], 0x0070, &[1, 2, 3]), &mut q).unwrap();
    same(&p, &q);                                                   // unknown non-critical: skipped
    assert_eq!(decode_err(&with_record_in_part(&p, PartId::ALL[2], 0x8070, &[])),
               FileError::NeedsNewerFirmware);                      // unknown critical
    decode(&without_mix(&p, PartId::ALL[3]), &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[3]).mix, PartParams::for_part(3)); // missing record: its base
    decode(&without_fx(&p), &mut q).unwrap();
    assert_eq!(q.perf().fx, FxParams::default());
}

#[test]
fn crc_sees_name_pool_fx_and_mix() {
    let (mut p, t) = Project::boxed();
    let base = project_crc(&p);
    assert_eq!(t.get(), base);
    p.set_name(ProjectName::new("ACID PARTY").unwrap());
    assert_ne!(project_crc(&p), base);
    p.set_name(ProjectName::new("NEW PROJECT").unwrap());
    assert_eq!(project_crc(&p), base);
    p.edit_fx().delay.mix = 0.3;
    assert_ne!(project_crc(&p), base);
    p.edit_fx().delay.mix = FxParams::default().delay.mix;
    p.edit_part(PartId::ALL[0]).part.mix.pan = 0.5;
    assert_ne!(project_crc(&p), base);
    p.edit_part(PartId::ALL[0]).part.mix.pan = 0.0;
    p.pool_store(SlotId::ALL[20], Sound::init(EngineType::Algo));
    assert_ne!(project_crc(&p), base);
    p.pool_clear(SlotId::ALL[20]).unwrap();
    assert_eq!(project_crc(&p), base);                              // content, not generations
}
```

  `with_origin`, `with_record_in_part`, `without_mix`, `without_fx`, `missing_part`, `duplicate_slot`, `slot_index`, `part_index`, `slot_named`, `block_before_context`, `nameless_header` and `decode_err` are file builders in this test file. They re-emit `encode(&p)`'s records with the one change, and a fresh CRC.

  **The fixture test** `project_fixture_loads`, in `codec_compat_test.rs`:
  - slots 0–9 `bits_eq` the decodes of `factory_0.snd` … `factory_7.snd`, `init_algo.snd` and `init_modal.snd`;
  - slot 11 and Parts 1 and 2 `bits_eq` the same edits (a function in the test, `fixture_edits`) applied to the decoded `factory_0.snd`;
  - FX and each Part's mix equal the recorded values;
  - the file's length and FNV-1a equal `PROJECT_FIXTURE_BYTES`, a constant of its own, so `FIXTURE_BYTES.len() == sources().len()` still holds.

  **Also:**
  - In `storage_frame_test.rs`, append `(RecordTag::Slot, 0x8007)`, `(Part, 0x8008)`, `(Fx, 0x8009)` and `(Origin, 0x000A)` to the tag table, and assert `FileKind::Project as u8 == 2`.
  - In `codec_fuzz_test.rs`, add `project_bytes_never_panic`. The existing xorshift seeds feed mutated `encode(&full().0)` bytes to `decode`. Every `Ok` project has every param within its `ParamSpec`, every Origin that names a filled slot, and no panic.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_codec_test --test storage_frame_test --test codec_fuzz_test --test codec_compat_test` → FAIL.
- [ ] **Step 3: Implement** `codec.rs`, the tags, `DecodeInPlace`, `test_support.rs` and the `PartialEq` derives. `init_in_place` and `reset_new` end with `TemplateCrc(project_crc(self))`.
- [ ] **Step 4: Add the fuzz target.** `decode_project.rs` mirrors `decode_sound.rs`. It builds its `Project` with `Box::new_uninit()` and `Project::init_in_place`, since the fuzz crate has no `test-support`. `just fuzz` loops over `decode_sound` and `decode_project`, seeding `corpus/decode_project` from `project.prj`.
- [ ] **Step 5: Write the fixture.** Extend `write_v1_fixtures` to write `project.prj` from a fixed project:
  - NEW, with slot 11 an edited copy of slot 0 named `EDITED`;
  - Part 1 from slot 11, then edited through `fixture_edits`;
  - Part 2 from slot 9;
  - delay MIX 0.3, and the name `FIXTURE`.

  Run `FIXTURE_WRITE=1 cargo test -p chimera-core --test codec_compat_test -- --ignored --nocapture` once, and paste the printed `project` row into `PROJECT_FIXTURE_BYTES`.
- [ ] **Step 6: Run** the Step 2 tests → PASS, then `just check` → PASS.
- [ ] **Step 7: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-core/fuzz Justfile
git commit -m "Projects as card records: slots, Parts and FX, round-tripped bit for bit"
```

### Task 3: Derived marks and the Part actions

**Files:**
- Create: `chimera-core/src/project/marks.rs`, `chimera-core/tests/project_marks_test.rs`
- Modify: `project/mod.rs` (`apply_part_action`; `save_part_to` calls `part_status`); `project/test_support.rs` (`same` compares `part_status`)

**Interfaces:**
- Consumes: `Project`, `Part`, `Pool`, `Origin`, `sound_crc` (Task 1); `project_crc`, `TemplateCrc`, `full`, `same`, `encode`, `decode` (Task 2).
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PartStatus { Clean, Edited, Stale(SlotId) }
pub fn part_status(part: &Part, pool: &Pool) -> PartStatus;
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ProjectStatus { Pristine, Saved, Modified }
pub fn project_status(p: &Project, t: TemplateCrc) -> ProjectStatus;
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PartActionKind { OverSlot(SlotId), NewSlot(SlotId), Revert(SlotId) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct PartAction { part: PartId, kind: PartActionKind }  // only part_actions builds one
impl PartAction { pub fn kind(self) -> PartActionKind; }
pub struct PartActions { items: [Option<PartAction>; 3] }
impl PartActions { pub fn iter(&self) -> impl Iterator<Item = PartAction> + '_; }
pub fn part_actions(p: &Project, part: PartId) -> PartActions;
#[derive(Debug, PartialEq)] pub struct ActionGone;
impl Project {
    /// Ok: the other users of the slot that now derive Stale (as `save_part_to`).
    pub fn apply_part_action(&mut self, a: PartAction) -> Result<PartSet, ActionGone>;
    #[cfg(any(test, feature = "test-support"))] #[doc(hidden)] pub fn mark_saved_for_test(&mut self);  // saved_crc := project_crc
}
```

`part_status` follows § Global Constraints exactly. `part_actions` offers, in this order:

| Status (origin) | Actions |
|---|---|
| `Clean` | `NewSlot(first_free)` |
| `Edited` (`Slot(s)`) | `OverSlot(s)`, `NewSlot(first_free)`, `Revert(s)` |
| `Edited` (`Init`) | `NewSlot(first_free)` |
| `Stale(s)` | `Revert(s)` (shown as UPDATE), `NewSlot(first_free)` |

- `NewSlot` appears only when a slot is free.
- `apply_part_action` returns `ActionGone` if the action no longer applies:
  - the Part's status or origin changed;
  - the `NewSlot` slot filled;
  - the `Revert` slot emptied.
- `OverSlot` and `NewSlot` go through `save_part_to`. `Revert` copies the slot back, sets the Origin fresh, and returns `PartSet::EMPTY`.

- [ ] **Step 1: Write the failing tests** in `project_marks_test.rs`. They cover the spec's § Tests › Derived marks and Review Focus 2. `src(part, slot)` builds a `PartSource`.

```rust
#[test] fn new_is_pristine() { let (p, t) = Project::boxed(); assert_eq!(project_status(&p, t), ProjectStatus::Pristine); }

#[test]
fn edit_back_to_the_slot_is_clean() {
    let (mut p, _) = Project::boxed();
    let a = PartId::ALL[0];
    p.load_part(src(a, SlotId::ALL[0])).unwrap();
    let was = p.part(a).sound.params.filter.cutoff;
    p.edit_part(a).part.sound.params.filter.cutoff = was * 0.5;
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Edited);
    p.edit_part(a).part.sound.params.filter.cutoff = was;
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
}

#[test]
fn save_over_a_slot_stales_the_other_unedited_users() {
    let (mut p, _) = Project::boxed();
    let s = SlotId::ALL[0];
    let [a, b, c] = [PartId::ALL[0], PartId::ALL[3], PartId::ALL[5]];
    for part in [a, b, c] { p.load_part(src(part, s)).unwrap(); }
    p.edit_part(c).part.sound.params.filter.cutoff *= 0.5;
    p.edit_part(a).part.sound.params.filter.cutoff *= 0.25;
    let over = part_actions(&p, a).iter().find(|x| x.kind() == PartActionKind::OverSlot(s)).unwrap();
    assert_eq!(p.apply_part_action(over), Ok(PartSet::EMPTY.with(b)));
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
    assert_eq!(part_status(p.part(b), p.pool()), PartStatus::Stale(s));
    assert_eq!(part_status(p.part(c), p.pool()), PartStatus::Edited);
}

#[test]
fn revert_is_bit_exact() {
    let (mut p, _) = Project::boxed();
    let (a, s) = (PartId::ALL[0], SlotId::ALL[2]);
    p.load_part(src(a, s)).unwrap();
    p.edit_part(a).part.sound.params.filter.cutoff *= 0.5;
    let rev = part_actions(&p, a).iter().find(|x| x.kind() == PartActionKind::Revert(s)).unwrap();
    assert_eq!(p.apply_part_action(rev), Ok(PartSet::EMPTY));
    assert!(p.part(a).sound.bits_eq(p.pool().get(s).unwrap()));
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
}

#[test]
fn init_origin_never_stale() {
    let (mut p, _) = Project::boxed();
    let a = PartId::ALL[1];
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
    p.pool_store(SlotId::ALL[8], factory_sound(0).unwrap());   // the INIT slot changes
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
    p.edit_part(a).part.sound.params.filter.cutoff *= 0.5;
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Edited);
}

#[test]
fn edited_part_never_stale_after_reload() { // Review Focus 2
    let (mut p, _) = Project::boxed();
    let s = SlotId::ALL[5];
    p.load_part(src(PartId::ALL[0], s)).unwrap();
    p.edit_part(PartId::ALL[0]).part.sound.params.filter.cutoff *= 0.5;
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    assert_eq!(part_status(q.part(PartId::ALL[0]), q.pool()), PartStatus::Edited);
    q.load_part(src(PartId::ALL[2], s)).unwrap();
    q.edit_part(PartId::ALL[2]).part.sound.params.filter.cutoff *= 0.25;
    q.save_part_to(PartId::ALL[2], s);
    assert_eq!(part_status(q.part(PartId::ALL[0]), q.pool()), PartStatus::Edited);
}

#[test]
fn stale_reloads_as_edited() {
    // Parts 3 and 4 from slot 0; Part 4 edits and saves over it: Part 3 is Stale.
    // encode → decode: Part 3 is Edited (it differs from its slot).
}

#[test]
fn project_marks() {
    let (mut p, t) = Project::boxed();
    let st = |p: &Project| project_status(p, t);
    p.set_name(ProjectName::new("ACID PARTY").unwrap());
    assert_eq!(st(&p), ProjectStatus::Modified);
    p.mark_saved_for_test();
    assert_eq!(st(&p), ProjectStatus::Saved);
    p.pool_store(SlotId::ALL[20], Sound::init(EngineType::Modal));
    assert_eq!(st(&p), ProjectStatus::Modified);
    p.pool_clear(SlotId::ALL[20]).unwrap();
    assert_eq!(st(&p), ProjectStatus::Saved);
    let was = p.perf().fx.reverb.mix;
    p.edit_fx().reverb.mix = 0.7;
    assert_eq!(st(&p), ProjectStatus::Modified);
    p.edit_fx().reverb.mix = was;
    assert_eq!(st(&p), ProjectStatus::Saved);
    p.edit_part(PartId::ALL[2]).part.mix.pan = -0.5;
    assert_eq!(st(&p), ProjectStatus::Modified);
    // THEME can't mark it: ThemeSettings isn't reachable from `Project` (a type guarantee).
}

#[test]
fn actions_offer_only_what_applies() {
    // The table, row by row, as `kinds(part_actions(&p, part))` vectors.
    // Fill the pool: NewSlot drops out of every row.
    // Build a Revert, then clear the slot's other user and reload the Part from INIT:
    // apply_part_action(stale) → Err(ActionGone), and project_crc is unchanged.
}
```

  `stale_reloads_as_edited` and `actions_offer_only_what_applies` are written with the same calls as the tests above. Their assertions are the ones their comments name.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_marks_test` → FAIL.
- [ ] **Step 3: Implement** `marks.rs` and `apply_part_action`, and have `save_part_to` use `part_status`.
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src/project chimera-core/tests/project_marks_test.rs
git commit -m "Part and project marks are derived; Part actions offer only what applies"
```

### Task 4: `ReplaceGuard` and ADR 0043

**Files:**
- Create: `chimera-core/src/project/guard.rs`, `chimera-core/tests/replace_guard_test.rs`, `docs/adr/0043-projects-own-their-sounds.md`
- Modify:
  - `project/mod.rs`: `load_part` becomes private, and `pub fn replace_part(&mut self, c: Confirmed<PartSource>) -> Result<(), ReplaceError>` is added;
  - `ui/mod.rs` (the legacy browser);
  - `tests/project_*`: callers of `load_part` go through `ReplaceGuard::check(..).unwrap()`, and `test_support::full` does too;
  - `docs/adr/README.md`.

**Interfaces:**
- Consumes: `part_status`, `project_status`, `TemplateCrc`, `project_crc`, `sound_crc` (Tasks 1–3); `PartSource` (Task 1); `ProjectId`, `VolumeId` (plan 1).
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Prompt { SavePartFirst, SaveProjectFirst }
/// Implemented for the two targets only.
pub trait Target: Copy {
    fn at_risk(&self, p: &Project, t: TemplateCrc) -> Option<Prompt>;
    fn witness(&self, p: &Project) -> u32;   // PartSource: sound_crc of the Part; ProjectSource: project_crc
}
impl Target for PartSource { .. }       // Edited → SavePartFirst; Clean or Stale → None
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectSource { File { id: ProjectId, vol: VolumeId }, New }   // vol: from list_projects' entry
impl Target for ProjectSource { .. }    // Modified → SaveProjectFirst; Pristine or Saved → None
#[must_use] #[derive(Debug)] pub struct Confirmed<R> { target: R, witness: u32 }
impl<R: Target> Confirmed<R> {
    pub fn target(&self) -> R;
    /// The target's state still what was confirmed.
    pub fn holds(&self, p: &Project) -> bool;
}
#[must_use] #[derive(Debug)] pub struct NeedsConfirm<R> { pending: Pending<R>, prompt: Prompt }
impl<R> NeedsConfirm<R> { pub fn prompt(&self) -> Prompt; pub fn into_pending(self) -> Pending<R>; }
#[derive(Debug)] pub struct Pending<R>(R);                 // the source id, never parsed data
impl<R: Target> Pending<R> {
    pub fn source(&self) -> R;
    pub fn anyway(self, p: &Project) -> Confirmed<R>;                               // REPLACE / LOAD ANYWAY; witness taken now
    pub fn save_then(self, p: &Project, t: TemplateCrc) -> Result<Confirmed<R>, NeedsConfirm<R>>; // re-checks after the caller's save
}
pub struct ReplaceGuard;
impl ReplaceGuard { pub fn check<R: Target>(p: &Project, t: TemplateCrc, r: R) -> Result<Confirmed<R>, NeedsConfirm<R>>; }
```

**Confirming.**
- CANCEL is dropping the `Pending`.
- A cancelled or failed save leaves the target at risk, so `save_then` returns `NeedsConfirm` again and the caller aborts (§ Unsaved-state edge cases).
- `replace_part(c)` refuses with `ReplaceError::Changed` when `!c.holds(self)`, and with `SlotEmpty` when the slot was cleared.
- `load_project` (Task 6) makes the same witness check, and the volume check inside `Card::run`.
- Nothing else re-reads a source: a file load runs its own pass 1.

- [ ] **Step 1: Write the failing tests** in `replace_guard_test.rs`:

```rust
fn vol() -> VolumeId { MemStore::new(1).mount().unwrap() }

#[test]
fn part_replace_prompts_only_when_edited() {
    let (mut p, t) = Project::boxed();
    let r = PartSource { part: PartId::ALL[0], from: PartFrom::Slot(SlotId::ALL[4]) };
    assert!(ReplaceGuard::check(&p, t, r).is_ok());                                  // Clean
    p.edit_part(PartId::ALL[0]).part.sound.params.filter.cutoff *= 0.5;
    assert_eq!(ReplaceGuard::check(&p, t, r).unwrap_err().prompt(), Prompt::SavePartFirst);
    // and a Stale Part (built as in Task 3): check → Ok.
}

#[test]
fn project_replace_prompts_only_when_modified() {
    let (mut p, t) = Project::boxed();
    for src in [ProjectSource::New, ProjectSource::File { id: ProjectId::new(3).unwrap(), vol: vol() }] {
        assert!(ReplaceGuard::check(&p, t, src).is_ok());                            // Pristine
    }
    p.set_name(ProjectName::new("X").unwrap());
    assert_eq!(ReplaceGuard::check(&p, t, ProjectSource::New).unwrap_err().prompt(), Prompt::SaveProjectFirst);
    p.mark_saved_for_test();
    assert!(ReplaceGuard::check(&p, t, ProjectSource::New).is_ok());                 // Saved
}

#[test]
fn each_answer_does_what_it_says() {
    let (mut p, t) = Project::boxed();
    let (a, s) = (PartId::ALL[0], SlotId::ALL[4]);
    let r = PartSource { part: a, from: PartFrom::Slot(s) };
    p.edit_part(a).part.sound.params.filter.cutoff *= 0.5;
    let edited = p.part(a).sound.clone();
    let pending = ReplaceGuard::check(&p, t, r).unwrap_err().into_pending();
    let pending = pending.save_then(&p, t).unwrap_err().into_pending();  // no save made: still at risk
    assert!(p.part(a).sound.bits_eq(&edited));
    let new = part_actions(&p, a).iter().find(|x| matches!(x.kind(), PartActionKind::NewSlot(_))).unwrap();
    p.apply_part_action(new).unwrap();                                    // SAVE PART FIRST
    let c = pending.save_then(&p, t).unwrap();
    p.replace_part(c).unwrap();
    assert!(p.part(a).sound.bits_eq(p.pool().get(s).unwrap()));
    // REPLACE: edit again, check → Err, into_pending().anyway(&p), replace_part → Ok, bits_eq slot.
    // CANCEL: edit again, check → Err, drop it: the Part keeps its edit.
}

#[test]
fn confirmed_then_edited_is_refused() { // Review Focus 4
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[1];
    let c = ReplaceGuard::check(&p, t, PartSource { part: a, from: PartFrom::Slot(SlotId::ALL[3]) }).unwrap();
    p.edit_part(a).part.sound.params.filter.cutoff *= 0.5;
    let edited = p.part(a).sound.clone();
    assert_eq!(p.replace_part(c), Err(ReplaceError::Changed));
    assert!(p.part(a).sound.bits_eq(&edited));
}

#[test]
fn confirmed_slot_emptied_before_replace() {
    let (mut p, t) = Project::boxed();
    let s = SlotId::ALL[20];
    p.pool_store(s, Sound::init(EngineType::Modal));
    let c = ReplaceGuard::check(&p, t, PartSource { part: PartId::ALL[1], from: PartFrom::Slot(s) }).unwrap();
    p.pool_clear(s).unwrap();
    assert_eq!(p.replace_part(c), Err(ReplaceError::SlotEmpty));
}
```

  The compile-fail doc tests on `Confirmed` (in `guard.rs`):
  - ```` ```compile_fail,E0451 ```` `Confirmed { target: src, witness: 0 }` outside the module;
  - ```` ```compile_fail,E0308 ```` `project.replace_part(ReplaceGuard::check(&p, t, ProjectSource::New).unwrap())`;
  - ```` ```compile_fail,E0624 ```` `project.load_part(src)` outside `project`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test replace_guard_test` and `cargo test -p chimera-core --doc` → FAIL.
- [ ] **Step 3: File the issue** "Legacy sound browser replaces an edited Part without a prompt" (`gh issue create -R joegiralt/chimera`). Its body says the navigation plan's prompt retires it.
- [ ] **Step 4: Implement** `guard.rs`. The legacy browser's Load becomes:

```rust
// No prompt screen yet: <issue URL>.
let c = ReplaceGuard::check(&self.project, self.template, src)
    .unwrap_or_else(|n| n.into_pending().anyway(&self.project));
```

- [ ] **Step 5: Write ADR 0043**, "Projects own their sounds; marks are derived; one typed replace guard". Its status is Proposed until the navigation plan's ship flash. It covers:
  - the copy rules, and `Part`, `Pool` and `Origin` with their private fields;
  - the derived marks, and why `Origin.crc` is the source's;
  - `sound_crc` including the name, `TemplateCrc`, and generations never lowered;
  - `Confirmed<R>` per target, with its witness and volume;
  - an empty slot refused at runtime;
  - the Part actions table, and explicit project saves;
  - **the project file:** FileKind 2; tags `0x8007`–`0x000A` with their payloads, the 16-byte names included; the record order; the neutral base; six Parts required; an origin to an empty slot read as INIT; low bits `0x0B`–`0x0F` kept free for the reserved records; the header-only list and its torn-newer-side case.

  Add its row to `docs/adr/README.md`.
- [ ] **Step 6: Run** → PASS; `just check` → PASS.
- [ ] **Step 7: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests docs/adr/0043-projects-own-their-sounds.md docs/adr/README.md
git commit -m "Every replace that can lose work goes through one typed guard"
```

### Task 5: The load gate on the audio side, and ADR 0046

**Files:**
- Create: `chimera-core/src/project/swap.rs`, `chimera-core/tests/load_protocol_test.rs`, `docs/adr/0046-project-load-protocol.md`
- Modify:
  - `instrument.rs`: `AudioShared.epoch`; `update_from(perf, epoch)`; `kill_all` and `quiet`;
  - `voice_alloc.rs` (`Allocator::kill_all`);
  - `chimera-stm32/src/audio/engine.rs` (`Engine.gate`) and `chimera-stm32/src/main.rs` (publish with `LOAD_LINK.epoch()`);
  - `chimera-desktop/src/audio.rs` (the gate in the callback; `update(perf, epoch)`) and `chimera-desktop/src/main.rs`;
  - every test that calls `update_from`;
  - `docs/adr/README.md`.

**Interfaces:**
- Consumes: `Instrument`, `AudioShared`, `Allocator` (main).
- Produces:

```rust
pub const LOAD_ACK_TIMEOUT_MS: u32 = 10;
pub struct LoadLink { epoch: AtomicU32, ack: AtomicU32 }
pub static LOAD_LINK: LoadLink = LoadLink::new();
impl LoadLink {
    pub const fn new() -> Self;
    pub fn epoch(&self) -> u32;                          // Acquire; what the UI tags each snapshot with
    pub(in crate::project) fn bump(&self) -> Swap;       // Release; only the project load functions
    #[cfg(any(test, feature = "test-support"))] #[doc(hidden)] pub fn bump_for_test(&self) -> Swap;
    pub fn ack(&self, e: u32);                           // audio; Release
    pub fn acked(&self, e: u32) -> bool;                 // UI; Acquire
}
#[must_use] pub struct Swap { epoch: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Settled { Acked, TimedOut }
impl Swap { pub fn settle(self, link: &LoadLink, within: impl FnMut() -> bool) -> Settled; }  // spins while !acked && within()
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub struct GateStep { pub kill: bool, pub drain: bool, pub ack: Option<u32> }
pub struct LoadGate { seen: u32, phase: Phase }   // Phase { Open, Fading, Waiting }, private
impl LoadGate {
    pub const fn new() -> Self;                   // seen 0, Open
    pub fn step(&mut self, requested: u32, snapshot: u32, quiet: bool) -> GateStep;   // pure
    pub fn before_block(&mut self, link: &LoadLink, inst: &mut Instrument, shared: &AudioShared) -> bool; // kill_all, ack; returns drain
}
// instrument.rs
pub struct AudioShared { pub parts: [PartAudio; MAX_PARTS], pub fx: FxParams, pub epoch: u32 }
impl AudioShared { pub fn update_from(&mut self, perf: &Performance, epoch: u32); }
impl Instrument { pub fn kill_all(&mut self); pub fn quiet(&self) -> bool; }  // quiet: no voice active
impl Allocator { pub fn kill_all(&mut self); }                                 // every booked slot dying
```

`step`, the whole table:

| Phase | Input | Output, next phase |
|---|---|---|
| any | `requested != seen` | `seen = requested` and `kill`. Then, with the new `seen`: `Open` and `drain` if `snapshot == seen` (the UI already timed out), else `Fading`. |
| `Open` | — | `drain` |
| `Fading` | `snapshot == seen` | `drain`, `Open` |
| `Fading` | `quiet` | `ack: Some(seen)`, `Waiting` |
| `Fading` | else | nothing |
| `Waiting` | `snapshot == seen` | `drain`, `Open` |
| `Waiting` | else | nothing |

**`kill_all`:**
- It marks every booked slot dying, calls `Voice::kill` on each active voice (the 128-sample fade, rendered on the old snapshot), and clears `waiting`.
- It doesn't count `dropped_unheard`: nothing was refused.
- A killed slot is freed by `render`'s step 5, as a shed one is. So a later note-off finds nothing, and a held key isn't retriggered.

Both shells call `before_block` once per callback, before the drain, and drain only when it returns `true`.

- [ ] **Step 1: Write the failing tests** in `load_protocol_test.rs`:

```rust
#[test]
fn gate_table() {
    let mut g = LoadGate::new();
    let s = |kill, drain, ack| GateStep { kill, drain, ack };
    assert_eq!(g.step(0, 0, true), s(false, true, None));
    assert_eq!(g.step(1, 0, false), s(true, false, None));
    assert_eq!(g.step(1, 0, false), s(false, false, None));
    assert_eq!(g.step(1, 0, true), s(false, false, Some(1)));
    assert_eq!(g.step(1, 0, true), s(false, false, None));   // acked once
    assert_eq!(g.step(1, 1, true), s(false, true, None));
    assert_eq!(g.step(1, 1, true), s(false, true, None));
}

#[test]
fn second_epoch_restarts_the_fade() { // Review Focus 5
    let mut g = LoadGate::new();
    let _ = g.step(1, 0, false);
    assert!(g.step(2, 0, false).kill);
    assert_eq!(g.step(2, 0, true).ack, Some(2));
    assert!(g.step(2, 2, true).drain);
}

#[test]
fn timeout_publish_reopens() { // Review Focus 5
    let mut g = LoadGate::new();
    let _ = g.step(1, 0, false);
    assert!(g.step(1, 1, false).drain);  // published before the ack
}

#[test]
fn settle_times_out() {
    let link = LoadLink::new();
    let swap = link.bump_for_test();
    let mut n = 0;
    assert_eq!(swap.settle(&link, || { n += 1; n < 5 }), Settled::TimedOut);
    let swap = link.bump_for_test();
    link.ack(link.epoch());
    assert_eq!(swap.settle(&link, || true), Settled::Acked);
}

#[test]
fn kill_all_fades_and_frees() {
    let mut r = rig();                         // instrument_test's helper: Instrument, FxBus, shared
    r.note_on(0, 60); r.note_on(0, 64);
    r.render(4);
    r.inst.kill_all();
    r.render(2);                               // FADE = 128 = 2 × 64
    assert!(r.inst.quiet());
    assert!(r.inst.allocator().slots().iter().all(|s| s.is_free()));
    r.note_off(0, 60);
    r.render(1);
    assert!(r.inst.quiet());
}
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test load_protocol_test` → FAIL.
- [ ] **Step 3: Implement** `swap.rs`, the `Instrument` and `Allocator` additions, and both shells:
  - the stm32 `Engine` gains `gate: LoadGate` (written in `init`), and `render_half` becomes `if e.gate.before_block(&LOAD_LINK, e.inst, shared) { e.notes.drain(..) }`;
  - the desktop callback does the same;
  - both UI loops publish with `update_from(ui.project().perf(), LOAD_LINK.epoch())`.
- [ ] **Step 4: Write ADR 0046**, "Swap projects by epoch: fade the voices, then publish" (Proposed). It covers:
  - `LOAD_EPOCH`/`LOAD_ACK` beside ADR 0021's triple buffer, and why they are outside it;
  - the gate table;
  - the 10 ms timeout, and what a late ack costs (the fade runs on the new snapshot);
  - the MIDI rules (the spec's table; CC and pitch bend aren't implemented);
  - boot publishing with no fade;
  - **the in-place project load, which supersedes in part ADR 0045's "two passes, staged", for projects only:** why (34 KB, no staging copy), and the fallback to NEW.

  Leave a `## FX at the swap` heading for Task 7. Its README row reads "(supersedes in part [0045](0045-card-format.md))". ADR 0045 is not edited.
- [ ] **Step 5: Run** the tests → PASS; `just check` → PASS (the firmware builds at every feature set).
- [ ] **Step 6: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-stm32/src chimera-desktop/src docs/adr/0046-project-load-protocol.md docs/adr/README.md
git commit -m "A load epoch fades every voice before the new project is published"
```

### Task 6: Save, load, list and delete on the card; the notes

**Files:**
- Create: `chimera-core/src/project/{store,note}.rs`, `chimera-core/tests/project_store_test.rs`
- Modify:
  - `storage/file.rs` (`InPlaceError`, `load_ab_in_place`);
  - `storage/frame.rs`: `pub fn peek_header(b: &[u8; HEADER_LEN]) -> Result<Header, FileError>`, the existing `decode` made public under that name;
  - `project/test_support.rs`: `FullOnWrite`, `FlipOnSecondRead` and `project_store_suite`;
  - `chimera-fat/Cargo.toml` (the dev-dependency on `chimera-core` gains `features = ["test-support"]`) and `chimera-fat/tests/power_cut_test.rs` (a project case);
  - `chimera-core/tests/load_protocol_test.rs` (end to end).

**Interfaces:**
- Consumes:
  - from plan 1: `Card`, `CardEvent`, `CardFault`, `Store`, `VolumeId`, `save_ab`, `delete_ab`, `AbFile`, `LoadError`, `SaveError`;
  - from earlier tasks: `ProjectCheck`, `ProjectDecoder`, `DecodeInPlace`, `project_crc`, `full`, `same` (Task 2); `part_status` (Task 3); `Confirmed<ProjectSource>` (Task 4); `LoadLink`, `Swap` (Task 5).
- Produces:

```rust
// storage/file.rs
#[derive(Debug)] pub struct InPlaceError { pub err: LoadError, pub clobbered: bool }   // impl CardFault, From<StoreError> (clobbered false)
pub fn load_ab_in_place<S: Store, D: DecodeInPlace>(s: &mut S, r: &Ready, f: AbFile, d: &mut D) -> Result<Header, InPlaceError>;
// note.rs
pub const LINE_LEN: usize = 48;
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Line { buf: [u8; LINE_LEN], len: u8 }
impl Line { pub fn new(s: &str) -> Line; pub fn as_str(&self) -> &str; }   // + core::fmt::Write, truncating
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Subject { Name(ProjectName), File(ProjectId) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Differ { None, One(PartId, SlotId), Many(u8) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectNote { Saved(Differ), Card { err: StoreError, subject: Option<Subject> }, File { err: FileError, subject: Subject },
                       Missing(Subject), LoadFailed(Subject), Changed, IsLoaded, NoIds, NewProject }
impl ProjectNote { pub fn line(&self) -> Line; }
// store.rs
pub fn project_file(id: ProjectId) -> AbFile;                          // Dir::Projects, id.stem()
pub fn new_project_id<S: Store>(card: &mut Card, store: &mut S) -> Result<ProjectId, ProjectNote>;
pub fn save_project<S: Store>(card: &mut Card, store: &mut S, p: &mut Project, id: ProjectId) -> ProjectNote;
#[must_use] pub struct LoadOutcome { pub swap: Option<Swap>, pub note: Option<ProjectNote>, pub event: Option<CardEvent> }
pub fn load_project<S: Store>(card: &mut Card, store: &mut S, p: &mut Project, go: Confirmed<ProjectSource>, link: &LoadLink) -> LoadOutcome;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectEntry { pub id: ProjectId, pub vol: VolumeId, pub name: Option<ProjectName>, pub err: Option<FileError> }
pub fn list_projects<S: Store>(card: &mut Card, store: &mut S, f: &mut dyn FnMut(ProjectEntry)) -> Result<CardEvent, ProjectNote>;
pub fn delete_project<S: Store>(card: &mut Card, store: &mut S, loaded: &Project, id: ProjectId) -> Result<(), ProjectNote>;
// test_support.rs
pub struct FullOnWrite<S>(pub S, pub bool);          // when .1: write puts half the body, then Err(StoreError::Full)
pub struct FlipOnSecondRead<S> { pub inner: S, pub file: FileName, reads: u8 }  // flips one byte of `file` from its 2nd read on
pub fn project_store_suite<S: Store>(make: &mut dyn FnMut() -> S);             // the save/load/list/delete cases below
```

**The rules:**
- **`load_ab_in_place`** runs pass 1 on both sides with the decoder's `Check`, then `pick`s, exactly as `load_ab`, so a failure there is `clobbered: false`. Pass 2 then runs `apply` on each event, then `finish`. If it fails, or its CRC differs from pass 1's, the error is `clobbered: true`.
- **`save_project`** does, inside one `Card::run`: `make_dir(Chimera)`, `make_dir(Projects)`, then `save_ab(.., ProjectCheck, Some(name), encode_project)`.
  - On success it sets `meta.id = Some(id)` and `meta.saved_crc = Some(project_crc(p))`, and returns `Saved(differ)`. `differ` counts the Parts whose status is `Edited` and whose Origin is `Slot`: `One(part, slot)` for one, `Many(n)` for more.
  - On any error the meta is untouched. The subject is `Name(meta.name)`.
- **`load_project`:**
  - It first checks `go.holds(p)`; if that fails, it returns `Changed` with no swap.
  - `New`: `reset_new`, then `bump`, with no note.
  - `File { id, vol }`: inside `Card::run`, `r.volume() != vol` returns `Card { VolumeChanged(..), Some(File(id)) }` before any read, with the project untouched. Otherwise:
    - loaded: `meta.id = Some(id)`, `saved_crc = Some(project_crc)`, then `bump`;
    - pass 1 failed: the note, no swap, and the project untouched;
    - clobbered: `reset_new`, `bump`, and `LoadFailed(File(id))`.
  - The only writer of the epoch is here. `boot_project` (Task 8) doesn't bump.
- **`new_project_id`** lists `Projects` for `P#######.A`/`.B` stems and returns the highest id + 1. A missing directory means 1. Past `ProjectId::MAX` it is `NoIds`.
- **`list_projects`** reads each id's two headers: a `Store::read` whose sink breaks after `HEADER_LEN` bytes and calls `peek_header`.
  - It reports the newer readable header's name, with the mount's `vol`.
  - `err` is `NeedsNewerFirmware`, `BadMagic` or `WrongKind` when that's all there is.
- **`delete_project`** returns `IsLoaded` when `id == loaded.meta().id()`.
- **`line()` copy, exactly:**

| Note | Line |
|---|---|
| `Saved(None)` | `SAVED` |
| `Saved(One(P2, slot 3))` | `SAVED: P2 DIFFERS FROM SLOT 03` |
| `Saved(Many(3))` | `SAVED: 3 PARTS DIFFER FROM SLOTS` |
| `Card { Full, Some(Name(ACID PARTY)) }` | `CARD FULL: ACID PARTY` |
| `Card { NoCard, None }` | `NO CARD` |
| `Card { VolumeChanged(_), Some(File(7)) }` | `CARD CHANGED: P0000007` |
| `File { BadCrc, File(7) }` | `FILE CHECKSUM FAILED: P0000007` |
| `File { NeedsNewerFirmware, Name(ACID PARTY) }` | `NEEDS NEWER FIRMWARE: ACID PARTY` |
| `Missing(File(7))` | `PROJECT NOT FOUND: P0000007` |
| `LoadFailed(File(7))` | `LOAD FAILED: P0000007` |
| `Changed` | `CHANGED SINCE ASKED: TRY AGAIN` |
| `IsLoaded` | `CAN NOT DELETE THE LOADED PROJECT` |
| `NoIds` | `NO PROJECT IDS LEFT` |
| `NewProject` | `NEW PROJECT` |

  Store and file messages are `StoreError::message()` and `FileError::message()`.

- [ ] **Step 1: Write the failing tests** in `project_store_test.rs`, on `MemStore`:
  - the cases shared with `DirStore` live in `project_store_suite`, which this file calls once, as `project_store_suite(&mut || MemStore::new(1))`;
  - the rest need a wrapper or an eject.

```rust
// In project_store_suite:
fn save_then_load_is_bit_identical<S: Store>(store: &mut S) {
    let mut card = Card::new();
    let (mut p, _) = full();
    let id = new_project_id(&mut card, store).unwrap();
    assert_eq!(id, ProjectId::new(1).unwrap());
    assert!(matches!(save_project(&mut card, store, &mut p, id), ProjectNote::Saved(_)));
    let (mut q, t) = Project::boxed();
    let link = LoadLink::new();
    let mut vol = None;
    list_projects(&mut card, store, &mut |e| vol = Some(e.vol)).unwrap();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol: vol.unwrap() }).unwrap();
    let out = load_project(&mut card, store, &mut q, go, &link);
    assert!(out.swap.is_some() && out.note.is_none());
    same(&p, &q);
    assert_eq!(project_status(&q, t), ProjectStatus::Saved);
    assert_eq!(q.meta().id(), Some(id));
}
// Also in the suite: first_save_makes_the_dirs (a store with /CHIMERA only → Saved),
// list_and_next_id, delete_rules, missing_file.

#[test] fn suite_on_memstore() { project_store_suite(&mut || MemStore::new(1)); }

#[test]
fn full_card_save_stays_modified() {
    let mut s = FullOnWrite(MemStore::new(1), false);
    let mut card = Card::new();
    let (mut p, t) = full();
    let id = new_project_id(&mut card, &mut s).unwrap();
    let _ = save_project(&mut card, &mut s, &mut p, id);
    let saved = p.meta().saved_crc();
    p.edit_fx().delay.mix = 0.9;
    s.1 = true;
    assert_eq!(save_project(&mut card, &mut s, &mut p, id),
               ProjectNote::Card { err: StoreError::Full, subject: Some(Subject::Name(p.meta().name())) });
    assert_eq!((p.meta().saved_crc(), project_status(&p, t)), (saved, ProjectStatus::Modified));
    // load_project on a fresh project gives the first save's content (same() against it).
}

#[test]
fn swapped_card_refuses_a_pending_load() { // Review Focus 3
    // Save id 1 on MemStore(1); take its entry's vol; build the Pending (a Modified project);
    // store.swap(2) and save a different project as id 1 there; pending.anyway(&q) → load_project
    // → note Card { VolumeChanged(_), Some(File(1)) }, swap None, project_crc(q) unchanged.
}

#[test] fn saved_toast_names_differing_parts() { /* one Edited Part from slot 3 → Saved(One(p, slot 3)); three → Many(3); an edited INIT-origin Part isn't counted */ }
#[test] fn pass_one_failure_leaves_ram() { /* truncated, bad CRC, bad magic on both sides → the File note, swap None, project_crc unchanged */ }

#[test]
fn pass_two_failure_falls_back_to_new() { // Review Focus 1
    // FlipOnSecondRead over the picked side: note LoadFailed(File(id)), swap Some, project_crc(q) == t.get().
}

#[test] fn stale_confirmation_is_refused() { /* check(New) while Saved, then set_name, then load_project → Changed, no swap */ }
#[test] fn no_card_note() { /* ejected → Card { NoCard, .. }; Card::Absent */ }
#[test] fn note_lines() { /* the table above, row by row, as assert_eq!(note.line().as_str(), "…") */ }
```

  In `load_protocol_test.rs`, add the end-to-end tests (spec § Tests › Load protocol):
  - **The rig.** Each test runs an `Instrument`, an `FxBus`, a `TripleBuffer<AudioShared>`, `NoteSources` and a `LoadGate` in one thread. `audio_block()` does `before_block`, then the drain, then `render`, and records the pair-1 samples. `settle`'s `within` closure runs `audio_block()` and counts to a cap, so the single thread plays both sides.
  - `swap_step_within_fade_bound` (L7): three held notes at velocity 127, their project saved and then another loaded. Across the blocks from the bump to two blocks after the publish, the largest sample step is at most the largest step of the 16 blocks before the bump, plus the loudest voice's peak / 128.
  - `fading_voice_keeps_its_routes`: the old Part routes LFO 1 to CUTOFF and the new one doesn't. The samples up to the ack equal, bit for bit, a reference run with no load that calls `kill_all` at the same block.
  - `note_on_between_epoch_and_publish_plays_new`: a note-on pushed after the bump and before the publish sounds on the new project's Part once the snapshot lands (its slot kind is the new engine), and not before.
  - `held_key_not_retriggered`: after the publish no voice is active, and the held key's note-off changes nothing.
  - `load_failure_still_publishes` (Review Focus 1): with pass 2 failing, the NEW project reaches the audio through the same epoch.
- [ ] **Step 2: Write the failing test** `project_cut_keeps_a_generation` in `chimera-fat/tests/power_cut_test.rs`.
  - It follows `cut_at_every_block_write_keeps_a_generation`, for save 3 of `test_support::full()` on FAT16, with a mix edit between saves.
  - For every `k`, `load_ab_in_place` gives generation 2's or 3's project, checked with `same`, and never an error.
  - The next full save succeeds.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test project_store_test --test load_protocol_test && cargo test -p chimera-fat --test power_cut_test` → FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** → PASS; `just check` → PASS.
- [ ] **Step 6: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-fat/Cargo.toml chimera-fat/tests/power_cut_test.rs
git commit -m "Projects save, load, list and delete on the card; a failed load falls back to NEW"
```

### Task 7: FX at the swap

The audit this plan owes (spec § Loading while playing). At the publish every voice has faded, so what's left sounding is the FX tails. A param "steps" when switching it moves a tail's sample value discontinuously. A gain or read position does that; a coefficient or LFO rate changes only the slope.

**Where MIX lives today:**
- the chorus multiplies its output by `mix` (`chorus.rs:270-281`);
- the delay does `delayed * params.mix` (`delay.rs:348`);
- the reverb takes `params.reverb.mix` (`fx_bus.rs:172`).

Each effect runs only while `is_on()` (`mix >= 0.001`, and for the chorus also MODE ≠ Off).

| Block | Param | Today | Ruling |
|---|---|---|---|
| CHORUS | RATE, DEPTH | per block, no smoothing | **Accept.** No feedback, and the line is at most 7 ms, fed by voices that faded before the ack. |
| CHORUS | MIX | output gain; below 0.001 cuts the effect | **Smooth**, as a return (below). |
| CHORUS | MODE → Off | cuts at once | **Smooth.** The bus keeps the last on-mode while the return fades. |
| CHORUS | MODE between on-modes | switches lines | **Accept**, as RATE and DEPTH. |
| DELAY | TIME | the read head jumps | **Smooth:** the base delay glides (a 50 ms one-pole per block, ramped per sample). |
| DELAY | FDBK, SAT, TONE | in-loop gain and coefficients | **Accept.** The loop's output stays continuous; the tail's decay changes. |
| DELAY | MIX | output gain; cuts the tail | **Smooth.** |
| DELAY | REV | already a 20 ms per-block one-pole | Already smooth. |
| REVERB | GRIT, TIME, DAMP | coefficients | **Accept.** |
| REVERB | SIZE | already crossfades between size steps | Already smooth. |
| REVERB | MIX | output gain; cuts the tail | **Smooth.** |
| COMP | THRESH, MAKEUP, MIX | already smoothed per block | Already smooth. |
| COMP | RATIO | enum; the gain reduction is smoothed in the log domain | **Accept.** |
| TAPE | all | `master-tape` builds only (ADR 0055) | **Accept.** Off the chain by default. |

**Files:**
- Create: `chimera-core/tests/fx_swap_test.rs`
- Modify:
  - `dsp/fx_bus.rs`: `returns: [f32; FX_SENDS]`, `chorus_mode: u8`, and `seeded: bool`;
  - `dsp/delay.rs`: `base: Option<f32>` state in `TapeDelay`;
  - `docs/adr/0046-project-load-protocol.md` (§ FX at the swap: the table above).

  No effect's own file changes except the delay's glide.

**Interfaces:**
- Consumes: `FxBus::process`, `TapeDelay::run`, and the REV SEND smoothing pattern (`fx_bus.rs:151-167`).
- Produces no new public API:
  - `FxBus::process` keeps each return's gain, smoothed toward `params.<fx>.mix` by a 20 ms one-pole per block.
  - It passes each effect a **copy** of its params with `mix` set to the smoothed value; for the chorus, a MODE of Off becomes the last on-mode while the gain is above zero. The effect's own `is_on()` then gates as before.
  - A gain snaps to its target within 1e-6, and to 0 below 0.001 (the effects' own on-threshold).
  - `TapeDelay` glides its base delay in samples.
  - **Both are seeded to their target on the first block** after `new` or `init_in_place`, so a still patch renders exactly as before.

- [ ] **Step 1: Write the failing tests** in `fx_swap_test.rs`. The helpers:
  - `tail(fx)`: 200 ms of a loud burst into the send of the effect under test, then silence;
  - `max_step(xs)`: the largest |x[n] − x[n−1]|;
  - `before`: `max_step` over the 20 ms before the change.

```rust
#[test] fn delay_time_change_glides() {
    let (before, after) = step_around(|p| p.delay.time_ms = 100.0, Fx::Delay);  // from 375 ms
    assert!(after <= 2.0 * before, "{after} > 2 × {before}");
}
#[test] fn mix_to_zero_fades_the_tail() {
    for fx in [Fx::Chorus, Fx::Delay, Fx::Reverb] {
        let (before, after) = step_around(|p| set_mix(p, fx, 0.0), fx);
        assert!(after <= 2.0 * before, "{fx:?}");
        assert!(return_is_zero_within(fx, 100 /* ms */));
    }
}
#[test] fn chorus_off_fades_the_tail() {
    let (before, after) = step_around(|p| p.chorus.mode = ChorusMode::Off as u8, Fx::Chorus);
    assert!(after <= 2.0 * before);
}
#[test] fn accepted_params_keep_the_tail_continuous() {
    // FDBK, TONE, SAT (delay); GRIT, TIME, DAMP (reverb): each stepped mid-tail, after ≤ 2 × before.
}
```

  The existing `fx_golden_test`, `delay_test`, `reverb_test` and `chorus_test` stay green unchanged; that is what the first-block seeding guarantees.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test fx_swap_test` → FAIL.
- [ ] **Step 3: Implement.** An effect running on after MIX 0 costs at most its on-cost, which `FxBus::COST` already bills in full, so the bill doesn't change.
- [ ] **Step 4: Write** ADR 0046's § FX at the swap from the table.
- [ ] **Step 5: Run** → PASS; `just check` → PASS (both `master-tape` builds).
- [ ] **Step 6: Commit**

```bash
git status --short
git add chimera-core/src/dsp/fx_bus.rs chimera-core/src/dsp/delay.rs chimera-core/tests/fx_swap_test.rs docs/adr/0046-project-load-protocol.md
git commit -m "FX returns and delay time glide, so a project swap never steps a tail"
```

### Task 8: Boot, the `UiState` entry points and both shells

**Files:**
- Create: `chimera-core/tests/project_boot_test.rs`
- Modify:
  - `project/store.rs` (`boot_project`);
  - `ui/mod.rs` (the entry points);
  - `ui/busy.rs`: `Toast.text: Line`, `ToastStep::Show(Line)`, and `toast_for` builds a `Line`;
  - `tests/toast_test.rs` (no screen golden changes: a `Line` draws as its `&str` did);
  - `chimera-stm32/src/main.rs` and `chimera-desktop/src/{main,store}.rs`.

**Interfaces:**
- Consumes: Tasks 1–6; `SystemSync::write` and `SystemSettings.last_project` (plan 1).
- Produces:

```rust
// store.rs
pub fn boot_project<S: Store>(card: &mut Card, store: &mut S, last: Option<ProjectId>, p: &mut Project) -> Option<ProjectNote>;
// ui/mod.rs
impl UiState {
    pub fn show_note(&mut self, n: ProjectNote);                      // a toast of n.line(); SAVED_MS for Saved, else ERROR_MS
    pub fn project_replaced(&mut self);                               // snap animators, active Part's engine and matrix, page (L6)
    pub fn boot_project<S: Store>(&mut self, card: &mut Card, store: &mut S, last: Option<ProjectId>);
    pub fn save_project<S: Store>(&mut self, card: &mut Card, store: &mut S, sync: &mut SystemSync,
                                  settings: &mut SystemSettings, id: ProjectId);
    pub fn load_project<S: Store>(&mut self, card: &mut Card, store: &mut S, sync: &mut SystemSync,
                                  settings: &mut SystemSettings, go: Confirmed<ProjectSource>, link: &LoadLink) -> Option<Swap>;
}
```

- **`boot_project` gives:**
  - `None` → `NewProject`;
  - no card → `Card { NoCard, None }`;
  - a missing file → `Missing(File(id))`;
  - a pass 1 error → `File { .. }`;
  - a clobber → `LoadFailed`.

  In every case but a load, the project is left NEW, with no bump: nothing sounds yet (spec § Boot 2). On a load it sets `id` and `saved_crc`.
- `UiState::boot_project` shows the note, if any, and calls `project_replaced`.
- **`save_project` and `load_project`** show the note.
  - After a successful save, or a successful **file** load, they set `settings.last_project` and call `sync.write`. Its error is ignored (see Decisions).
  - `+ NEW` and a fallback to NEW leave `last_project` unchanged.
  - `load_project` calls `project_replaced` whenever a `Swap` comes back, and returns it. The shell then calls `swap.settle(&LOAD_LINK, within_10_ms)` and publishes.
  - `save_project` takes a concrete `ProjectId`, so "saving without an id" doesn't compile.

- [ ] **Step 1: Write the failing tests** in `project_boot_test.rs`, on `MemStore`:

```rust
#[test] fn boot_loads_the_last_project() { /* save full() as id 2 with SYSTEM last = 2; a fresh UiState boots SYSTEM then boot_project → same(), status Saved, no toast */ }
#[test] fn boot_without_a_last_id_is_new_and_says_so() { /* toast "NEW PROJECT"; project_status Pristine */ }
#[test] fn boot_reasons() { /* no card / missing / corrupt / clobbered → the notes above; project_crc == template each time */ }
#[test] fn save_writes_the_last_project() { /* save_project(id 3) → SystemSync::boot on the same store gives last_project Some(3) */ }
#[test] fn load_writes_the_last_project() { /* load id 5 → last_project Some(5); then load New → still Some(5) */ }
#[test] fn replaced_ui_snaps() { /* after load_project, the renderer's values equal the new page's values without a lerp step */ }
```

  Add the compile-fail doc test on `UiState::save_project`: ```` ```compile_fail,E0308 ```` passing `ui.project().meta().id()` (an `Option`).

  In `chimera-desktop/src/store.rs`, add two tests beside `theme_survives_a_relaunch`:
  - `project_store_suite_on_dir_store`: `project_store_suite(&mut || DirStore::new(unique_root()))`. This is spec § Tests › Store fake, with both stores passing the same cases.
  - `project_survives_a_relaunch`:
    - a `UiState` on a `DirStore` at `unique_root()` edits a Part, a slot, FX and the name, gets `new_project_id`, and calls `save_project`;
    - a second `UiState` on a new `DirStore` over the same root boots SYSTEM, then `boot_project`;
    - its project equals the first (`test_support::same`) and is `Saved`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_boot_test && cargo test -p chimera-desktop` → FAIL.
- [ ] **Step 3: Implement.** Then wire the shells:
  - **stm32 `synth`:** after the SYSTEM boot and theme, and before `shared::take_audio`, call `ui.boot_project(&mut card, store, settings.last_project)`. BUSY is still up, the audio hasn't started, and the watchdog isn't running, so the ≈130 KB read (pass 1 on both sides, then pass 2) is covered.
  - **Desktop `main`:** the same, after `SystemSync::boot`.
  - Neither shell calls save or load: the navigation plan does, and settles each `Swap` with a 10 ms deadline, on `controls::ticks()` (stm32) or `Instant` (desktop).
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-stm32/src/main.rs chimera-desktop/src
git commit -m "Boot loads the last project; saves and loads record it in SYSTEM"
```

### Task 9: The bench row and the desktop QA gate

**Files:**
- Modify: `chimera-stm32/src/bench.rs` (the `PROJ CRC` row); this plan (`## Measured`)

**Interfaces:**
- Consumes: `project_crc`, `Project` (Tasks 1–2); the bench's existing row pattern.
- Produces: the host estimates and the chip-check list the navigation plan's ship flash runs.

- [ ] **Step 1: Add the bench row** `PROJ CRC`: the cycles of one `project_crc` over `full()`'s shape (all 32 slots filled), printed like the others. It is built by `just check`'s bench build, and flashed at ship, not here.
- [ ] **Step 2: Estimate `PROJ CRC` on the host.** Take the instruction count of `project_crc`'s loop from the release disassembly (`cargo objdump` on the firmware, as `just stack-check` does) for one full project, and convert it at rev V's 480 MHz. Record the figure under `## Measured`. At 1 ms or more, file a GitHub issue: the navigation plan must then cache the status instead of recomputing it each input frame.
- [ ] **Step 3: Run** `just check` → PASS. The stack check covers `encode_project`, `ProjectDecoder` and `load_ab_in_place`.
- [ ] **Step 4: The desktop QA gate.** Run and record under `## Measured`:
  1. `cargo test -p chimera-core --test param_sweep_test`, the parameter-sweep harness. It lands on main from branch `param-sweep` via `modal2-resonators`, and this gate waits for it: it must pass on the project-backed `UiState`.
  2. `just desktop` with `CHIMERA_CARD` at the relaunch test's root: that project plays at boot, with no toast.
  3. `CHIMERA_CARD=/nonexistent just desktop`: the toast says `NO CARD`, and a NEW project plays.
  4. A fresh `CHIMERA_CARD` directory: the toast says `NEW PROJECT`, and a note held on the keyboard sounds as on main.

  Any failure becomes a GitHub issue, fixed before the merge.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-stm32/src/bench.rs docs/superpowers/plans/2026-09-30-projects-core.md
git commit -m "The bench times the project CRC; desktop QA recorded"
```

**Handed to the navigation plan's single ship flash** (the owner's rule: desktop QA first, one combined flash). This plan adds no flash. That plan's STOP runs, on one `just flash-bench` build, since `bench::run` falls through to the synth:
- the `PROJ CRC` and BUS rows, the gate's cost, and no audio overrun during a load (`AudioStats`);
- boot with no project (`NEW PROJECT`), and boot with the last project;
- save, power off, power on: the same project;
- a load with held notes: no click, and the tails ring on;
- pulling the card mid-save: the previous generation loads;
- the project list's cost for tens of projects.

ADRs 0043 and 0046 move to Accepted in that plan's `## Measured` commit.

---

## Measured

Task 9, 2026-10-01, on the host; nothing flashed.

**`PROJ CRC` bench row** (MEMORY screen, last line). `bench::run` now takes
the live project: before the audio rows it fills every free slot that no
Part names (copies as `full()` makes them: INIT Modal at every third index,
a factory Sound otherwise), times `project_crc` 16 times with the DWT and
averages, then `pool_clear`s those slots, so the content and status are
unchanged. It builds with `--features bench` and
`--features bench,master-tape`. Its frame is 1 876 B (`sub sp, #0x754`),
under the stack check's 8 KB.

**`PROJ CRC` estimate** (rev V, 480 MHz), from the release bench ELF's
disassembly and the encoded sizes:

| | `full()` | NEW |
|---|---|---|
| file bytes | 45 499 | 19 441 |
| records | 930 | 402 |
| `Block` records | 733 | 315 |
| param entries (5 B each) | 7 864 | 3 354 |

- **The CRC.** `Crc32::update` is inlined into `RecordWriter::put`, and the
  compiler unrolled its loop by four: 22 instructions per 4 bytes, so 5.5
  per byte. One table load per byte, from a 1 KB table.
- **The encode.** Each entry is about 30 instructions in `encode_block`
  (the `ValidAddr` scan and the `RecordBuf` writes), plus two dynamic calls.
  `enum_code` is about 7 instructions, and `get` (a `tbb` jump, a load and
  a convert) about 10. With the CRC's 27.5, that is about 75 instructions
  per entry.
- **Each record** costs about 250 more. That is `put`'s setup, its 4-byte
  header CRC and two `Discard` calls, `encode_block`'s dispatch, and
  `RecordBuf::new`'s 512 B `memclr`.
- **`full()`:** 7 864 × 75 + 930 × 250 ≈ 0.82 M instructions. At an IPC of
  0.8 to 1.6 (dual issue, but a `blx`, a `tbb` and a table load every few
  instructions), that is **≈ 1.1–2.1 ms, about 1.7 ms at IPC 1**.
- **NEW:** 3 354 × 75 + 402 × 250 ≈ 0.35 M instructions, **≈ 0.5–0.9 ms**.
- **Host check** (release, x86): `project_crc(full())` runs in 88–104 µs,
  against a reviewer's 80 µs.
- **Verdict.** The figure is ≥ 1 ms, so the navigation plan must not hash
  the project per input frame. https://github.com/joegiralt/chimera/issues/257
  already asks it to cache `project_status` on a revision counter; it has a
  comment with these figures. The bench row gives the chip's number at the
  ship flash.

**Desktop QA gate**

1. **The parameter sweep** passes on this branch, on a bare
   `Performance`/`AudioShared` (ruling R20). `param_sweep_test` gives
   14 passed with 1 ignored, and `sweep_thorough` with `--ignored` passes
   in 135 s.
2. **The project tests pass.** `load_protocol_test` 23,
   `project_boot_test` 13, `project_codec_test` 14, `project_marks_test`
   14, `project_model_test` 12, `project_store_test` 14, and
   `system_file_test`. The desktop's `project_survives_a_relaunch`,
   `project_store_suite_on_dir_store` and `theme_survives_a_relaunch` pass
   too. `just check` is green, with 3 037 tests passed and 16 ignored over
   every run.
3. **Window checks, automated headless.** No one read the screen. The
   desktop's boot moved into `main.rs`'s `boot()`, and the test
   `audio::tests::each_card_boots_a_project_that_plays` runs it against a
   `DirStore` and then renders 100 ms of a held middle C. The note goes on
   the selected Part's channel, through the desktop `Engine` (gate, drain,
   render, `stereo_frame`). `card_dir()` only maps `CHIMERA_CARD` to the
   root, so the test passes the roots directly.
   - **The relaunch root** (`full()` saved, so SYSTEM names it): no toast.
     `same()` matches the saved project, the status is `Saved`, and the
     note sounds (peak 0.089).
   - **No card** (a root that doesn't exist): the toast is `NO CARD`. At
     first it was `NEW PROJECT`: SYSTEM's defaults name no project, so
     `boot_project` reported NEW's note. Now, with no last project, it
     mounts the card to say why, so a card that won't mount shows its
     fault as well (`project_boot_test`'s
     `boot_without_a_card_says_no_card`). The note sounds (peak 0.072),
     sample for sample as `Performance::new`, which is what main played.
   - **A fresh empty directory:** the toast is `NEW PROJECT`. The note
     sounds sample for sample as `Performance::new`.
4. **Not run:** the window itself. Item 3 automates the toast text and the
   audio path, not what the screen draws or what the speakers play.
   - UAT (owner): `just desktop` against each of the three roots. Check
     the toast as drawn, and that a held key sounds as on main by ear.
     For no card, expect `NO CARD`.

`just stack-check` passes, which covers `encode_project`, `ProjectDecoder`
and `load_ab_in_place` on every feature set. `just firmware` builds.
