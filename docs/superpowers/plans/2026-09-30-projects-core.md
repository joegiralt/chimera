# Projects Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The synth's whole musical state becomes one `Project` that saves to the card, loads back bit-identical (at boot too), and swaps while notes are held with a voice fade and no FX step, driven by pure APIs that any navigation can call.

**Architecture:**
- **Functional core** (`chimera-core/src/project/`), pure and host-tested:
  - the model: `PartId`, `SlotId`, `PartSet`, `Pool`, `Origin`, `Project`;
  - the project file codec, `project_crc` and `TemplateCrc`;
  - the derived marks (`part_status`, `project_status`, `part_actions`);
  - `ReplaceGuard`, typed per replace target;
  - the load gate, a pure state machine the audio thread steps once a block;
  - `ProjectNote`, the one line the screen shows for each outcome.
- **Storage shell** (`project/store.rs`): save, load, list, delete and boot over plan 1's `Store`, `Card` and A/B files. It is generic over `Store`, so `MemStore` and `DirStore` test it on the host.
- **Hardware and desktop shells:** they step the gate in the audio callback, tag each published snapshot with the epoch, and boot the last project. Neither has key handling; the navigation plan drives the entry points.

**Tech Stack:** Rust 2024, `no_std` core, no new dependency. `chimera-hal`'s `testkit` (`MemStore`), `DirStore`, `just`.

**Spec:** `docs/superpowers/specs/2026-09-28-projects-storage-design.md` (approved, binding except navigation). Sections: § Model, § Types decide what's possible, § Derived marks, § Copy rules, § Unsaved-state edge cases, § Storage (Format, Saving, SYSTEM, Errors), § Loading while playing, § Boot, § Tests. Plan 1 (shipped, PR #202): `docs/superpowers/plans/2026-09-28-storage-foundation.md`, whose `Store`, `Card`, `save_ab`/`load_ab`, codec and `SystemSync` this plan consumes. ADRs 0045 (card format) and 0048 (own FAT layer).

> **Scope.** This is the non-navigation half of the spec's plan 2. `Location`, the ladder's rungs, the key table and MENU tap/hold are out; see § Deferred to the navigation plan. Plan 3 (library, tags, naming) is out too.

## Global Constraints

Copied from the spec. Every task's requirements include this section.

**Model**
- `Project { meta: ProjectMeta, pool: Pool, perf: Performance }`, where `ProjectMeta { id: Option<ProjectId>, name: ProjectName, saved_crc: Option<u32> }`. `Performance.name` moves into `ProjectMeta`. `AudioShared::update_from` keeps reading only `project.perf()`.
- `UiState.performance` and `UiState.pool` merge into one private `project: Project`.
- `SoundPool` becomes `Pool`: 32 slots indexed by `SlotId`, with `gens: [u16; 32]`. `store` and `clear` bump the slot's generation. `clear` refuses a slot a Part uses.
- `Part { sound, origin: Origin, mix }`. `Origin = Slot { slot, gen, crc } | Init(EngineType)`, set only by load, save-over-slot and revert. Nothing else changes in `Part`.
- `active_part: usize`, `Performance::edit(usize)` and every other Part index become `PartId`.
- "Every rule here is carried by a type or by a pure function, never by a stored flag that could drift."
- `Project`'s fields are private. Edits go through `&mut` freely, because the marks are derived. Only the replaces can lose work, and each one needs `Confirmed`.

**Derived marks**
- Part `*`: the sound differs bit for bit from its pool slot (`Sound::bits_eq`). `part_status`: `Init(e)` is `Clean` if the sound `bits_eq(Sound::init(e))`, else `Edited`; `Slot` and bit-equal is `Clean`; generation unchanged is `Edited`; generation moved and `crc(sound) == crc` is `Stale(slot)`; generation moved and edited is `Edited`.
- Project: `Pristine` when the canonical CRC equals `template_crc`; `Saved` when it equals `meta.saved_crc`; else `Modified`.
- Saving a project doesn't clear Part marks. Pool changes, FX, mix and name edits mark the project. THEME never does. A load sets every `Origin::Slot` to the slot's current generation and CRC.
- `+ NEW` or a project load while `Modified` asks **SAVE THEN LOAD / LOAD ANYWAY / CANCEL**; a slot loaded into an `Edited` Part asks **SAVE PART FIRST / REPLACE / CANCEL**. `Pristine` and `Saved` never ask. A cancelled or failed save aborts the load.

**Format** (plan 1's rules, unchanged)
- Header, TLV records (`u16` tag, `u16` length), CRC trailer. Tag bit 15 is must-understand. Enums by frozen code. Continuous values as `f32` in spec units, clamped and quantised through their `ParamSpec`. Decoding starts from a frozen neutral base. Counts are bounded. The parser never panics.
- Project records: meta, the pool (sound records per filled slot), the six Parts (origin slot or INIT engine, then the mix), FX. Reserved: tempo, the AFX map per Part, the CC map, the set list, and (plan 3) the custom tag names.
- `FileKind` 2 is reserved for Project (ADR 0045). `/CHIMERA/PROJECTS/P0000001.A` and `.B`; the name is in the header. Ids are the highest existing id + 1.

**RAM and saving**
- No staging copy and no serialised buffer. Saves stream from live state. Nothing new goes in D2. Every new AXI static is counted in `AXI_RESIDENT`. No stack frame of 8 KB or more (`just stack-check`).
- The UI loop is blocked while it saves or loads; the audio keeps playing from its last snapshot.
- A save sets `meta.saved_crc` and writes the project's id into SYSTEM as the last project. SYSTEM is also written on a project load.

**Loading while playing**
1. Validate (on error, RAM is untouched). 2. Parse into the UI's project, don't publish. 3. Bump `LOAD_EPOCH`. 4. On a new epoch the audio kills every voice (the 128-sample fade, through the old mix and FX), clears `waiting` and stores `LOAD_ACK`. 5. The UI waits for `LOAD_ACK` (≤ 10 ms), then publishes. The gap is at most one block.
- From the epoch to the publish the audio doesn't drain the note queues; note-ons then play on the new project. A note-off for a killed voice is ignored. A held key isn't retriggered.
- If the card fails during step 2, the UI falls back to a NEW project with "LOAD FAILED: <file>", then publishes through steps 3–5.
- After any load the renderer's animators snap (CLAUDE.md's only exception to "never snap").
- FX params switch with the snapshot; delay and reverb buffers aren't cleared. This plan lists every FX param that would step under a live tail and smooths it or records the accepted step in the ADR.

**Boot**
- SYSTEM first (plan 1). Then load the last project id without the fade (nothing sounds yet). No id, no card, or any load error starts a NEW project and shows the reason. NEW is `Performance::new` plus a pool of the factory Sounds and one INIT Sound per engine; its CRC is `template_crc`, so it is `Pristine`.

**Errors reach the screen, never the audio**
- A card error, a full card, a bad CRC or an unsupported format shows a message that names the file. A file that fails to parse or needs newer firmware is listed with its error.

**Repo rules (CLAUDE.md, owner)**
- Every `unsafe` has a `// SAFETY:` comment. No heap, no blocking and no allocation on the audio path. No libc.
- Invariants live in types. Pure core, thin shells.
- Decisions that constrain later work get an ADR; never edit an accepted one.
- Commits: a terse plain sentence, no type prefix, no Claude or AI attribution. Stage files by name. **Never stage `docs/chimera-ui-ux-spec.md` or `chimera.bin`.**
- `just check` passes before every commit. No task ends red.

## Review Focus

The failure modes the spec implies but none of its tests exercise, most likely first. Each has a test in the task named.

1. **The card changes between pass 1 and pass 2 of a project load.** Pass 2 has already written part of the UI's project. The project must end up either loaded or NEW, never half of each, and the audio must still get its epoch and publish. Task 6: `pass_two_failure_falls_back_to_new`.
2. **An edited Part, saved, reloaded, then its slot overwritten by another Part.** If a load set `Origin.crc` from the Part's own sound, the Part would derive `Stale`, and UPDATE (which needs no prompt) would silently drop its edits. `Origin.crc` is the slot's CRC. Task 3: `edited_part_never_stale_after_reload`.
3. **An `Origin` that names an empty slot** (a hand-edited file, or a slot record a newer firmware dropped). The Part keeps its own sound and reads as `Origin::Init(engine)`; no Part points at a missing slot. Task 2: `origin_to_empty_slot_reads_as_init`.
4. **The audio doesn't ack within 10 ms** (a stalled callback, or an epoch bumped during a fade). The UI publishes anyway, the gate reopens on seeing that epoch, and notes are drained again; the synth is never left muted. Task 5: `timeout_publish_reopens`, `second_epoch_restarts_the_fade`.
5. **A save to a card that is full, or has `/CHIMERA` but no `PROJECTS`.** Both directories are made first. `Full` shows "CARD FULL: <name>", the older generation stays loadable, and the project stays `Modified`. Task 6: `first_save_makes_the_dirs`, `full_card_save_stays_modified`.

## Decisions this plan makes where the spec is silent or ambiguous

- **A project load applies in place.** A `Project` is about 34 KB and there is no staging copy, so pass 2 writes straight into the UI's project instead of staging (ADR 0045's "two passes, staged" clause). The result type says which case happened: `Untouched` (pass 1 failed, RAM as it was), `Loaded`, or `Clobbered` (pass 2 failed, and the loader resets to NEW). ADR 0046 records this as superseding 0045 in part, for projects only. Sounds and SYSTEM stay staged.
- **`Origin.crc` is the source's CRC,** the slot's (or, for save-over and new-slot, the sound just stored) at the moment the Origin is set. A project load recomputes it from the loaded slot. Review Focus 2 explains why it can't be the Part's own.
- **Every Part's own Sound is stored,** not only the ones that differ from their slot. That is six sounds more per file, and no cross-record reference to get wrong.
- **`Confirmed<R>` is generic over the replace target.** The spec's single `Replace` enum becomes two target types, `PartSource` and `ProjectSource`, so a project confirmation can't be passed to a Part replace (a compile-fail test). `PartFromLibrary` waits for plan 3's `LibId`. The legacy sound browser's INIT entries become `PartFrom::Init(engine)`.
- **An empty slot is a runtime refusal,** not a type. A `Pending` re-validates its source on confirm, and the slot may have been cleared in between, so `replace_part` returns `Err(SlotEmpty)`.
- **`PartAction` is opaque:** only `part_actions` builds one, and `apply_part_action` re-checks that it still applies. A `Stale` Part offers UPDATE (`Revert`) and a new slot, never `OverSlot`, which would undo the other Part's save. `ToLibrary` is plan 3's.
- **The epoch is written by the project load functions only** (`LoadLink::bump` is `pub(in crate::project)`), not by a `Project` method, which keeps `Project` free of statics. The UI tags every snapshot with the latest epoch. The gate reopens when a snapshot carries the epoch it is waiting for.
- **The project list reads headers only** (one block per side), as the spec says. A parse error that needs the whole file shows when the load fails. The list is a callback, so the navigation plan decides what to keep in RAM.
- **NEW's name is `NEW PROJECT`,** and NEW's pool is the 8 factory Sounds in slots 1–8, then INIT Algo and INIT Modal (`EngineType::ALL` order) in slots 9–10.
- **Copy.** Part and slot numbers are 1-based on screen, slots in two digits. The saved toast is `SAVED`, `SAVED: P2 DIFFERS FROM SLOT 03`, or `SAVED: 3 PARTS DIFFER FROM SLOTS`. Every error line ends with `: ` and the project's name, or the file stem (`P0000007`) when no name is known.
- **A failed SYSTEM write after a good project save doesn't change the toast.** The project is safe on the card; the next save or load retries SYSTEM.
- **The generation is a wrapping `u16`.** 65 536 stores to one slot between a Part's load and its next look could alias a generation. That is accepted.
- **Controller state.** MIDI CC and pitch bend are dropped at the note queue today (`note_queue.rs:66`), so there is nothing to keep across a swap.

## Deferred to the navigation plan

The owner is redesigning navigation in ADR 0044 (B<n> toggles Part sound ⇄ Part mixer; MIX+Bn stays as a shortcut). Everything below touches keys or screens, so it waits for that plan, which calls this plan's APIs:
- `Location`, the rungs, breadcrumb, legend, the key table, MENU tap and hold, press timestamps and edge latching.
- Deleting `BlockRef::Channels`, `impl Block for [Part; MAX_PARTS]`, System › MIDI Setup, `ChainId::Mixer` and `UiMode::SoundBrowser`. The spec couples these to rung 2; ADR 0044 now decides them. This plan keeps them and routes the legacy browser through `ReplaceGuard` (with `Pending::anyway`, today's behaviour, until a prompt exists).
- The prompt screens for `NeedsConfirm` (SAVE PART FIRST / REPLACE / CANCEL, SAVE THEN LOAD / LOAD ANYWAY / CANCEL), the Part menu built from `part_actions`, "P4 ALSO USES SLOT 03: UPDATE P4 / LEAVE" (the model returns that `PartSet`), and NAME EXISTS / KEEP BOTH / OVERWRITE THAT ONE (the model gives `list_projects` and `save_project` to any id).
- What triggers SAVE, SAVE AS, `+ NEW`, a load and a delete. SAVE AS naming is plan 3's naming screen.
- Showing `*` and `◦`, the per-input-frame recompute of `project_status` (Task 9 measures its cost), the Project rung's stats strip, the NO CARD rung, the greyed project list and the "NOT SAVED" footer.
- The UX spec amendment. This plan never stages `docs/chimera-ui-ux-spec.md`.
- The chip checks that need a save or load trigger: save, power-cycle, same project; load with held notes; pull the card mid-save. Task 9 hands them over.

## Rebase risk: `modal2-resonators`

That branch changes `ModalParams` (new disk codes 12–20, retirements), adds a v1 `Translation` hook to `decode_block` (a new argument), re-records screen goldens and edits UI pages. This plan builds on main and doesn't depend on it. Where they meet:
- **`decode_block` call sites.** Task 2 adds calls in `project/codec.rs` (the Part mix and FX blocks), and Task 2 makes `SoundCheck::step` `pub(crate)`. Whoever merges second passes `TRANSLATIONS` at the new call sites and keeps the `sound.rs:232` edit.
- **The project fixture.** It holds INIT Modal. `project_fixture_loads` compares each slot with the decode of the matching v1 `.snd` fixture, not with `Sound::init`, so Modal 2's translation applies to both sides and the test survives the merge.
- **`ui/mod.rs` and the test rig.** Task 1 rewrites every `performance`/`pool`/`active_part` access. Modal 2's page edits touch the same file. Expect textual conflicts only; resolve them by keeping both.
- **Screen goldens.** This plan changes no pixels except the toast (Task 8 re-records `toast_saved`).

## File structure

| File | Responsibility |
|---|---|
| `chimera-core/src/project/mod.rs` | `Project`, `ProjectMeta`, `PartFrom`, `PartSource`, `InUse`, `SlotEmpty`; the edit, pool and part operations. |
| `chimera-core/src/project/ids.rs` | `PartId`, `SlotId`, `PartSet`. |
| `chimera-core/src/project/pool.rs` | `Pool`. |
| `chimera-core/src/project/perf.rs` | `Origin`, `Part`, `Performance`, `PartEdit`, `part_block(_mut)` (moved from `preset.rs`, which re-exports them). |
| `chimera-core/src/project/codec.rs` | Project records: `encode_project`, `ProjectCheck`, `ProjectDecoder`, `project_crc`, `TemplateCrc`. |
| `chimera-core/src/project/marks.rs` | `PartStatus`, `ProjectStatus`, `PartAction`, `PartActions`, the pure mark functions. |
| `chimera-core/src/project/guard.rs` | `ReplaceGuard`, `Target`, `Prompt`, `Confirmed`, `NeedsConfirm`, `Pending`. |
| `chimera-core/src/project/swap.rs` | `LoadLink`, `LOAD_LINK`, `Swap`, `Settled`, `LoadGate`, `GateStep`. |
| `chimera-core/src/project/note.rs` | `Line`, `Subject`, `Differ`, `ProjectNote`. |
| `chimera-core/src/project/store.rs` | `project_file`, `new_project_id`, `save_project`, `load_project`, `list_projects`, `delete_project`, `boot_project`. |
| `chimera-core/src/storage/file.rs` | + `DecodeInPlace`, `InPlace`, `load_ab_in_place`. |
| `chimera-core/src/storage/{frame,record,sound,crc}.rs` | `FileKind::Project`; four record tags; `sound_crc`; `CrcSink` shared. |
| `chimera-core/src/instrument.rs`, `voice_alloc.rs` | `AudioShared.epoch`; `kill_all`, `quiet`. |
| `chimera-core/src/dsp/{fx_bus,delay}.rs` | Smoothed FX returns and delay time. |
| `chimera-core/src/ui/{mod,busy}.rs` | `UiState` holds one `Project`; entry points; `Toast` carries a `Line`. |
| `chimera-stm32/src/{main,shared,bench}.rs`, `audio/engine.rs` | Gate, epoch tag, boot load, bench row. |
| `chimera-desktop/src/{main,audio,store}.rs` | Gate, epoch tag, boot load, relaunch test. |
| `chimera-core/tests/project_*_test.rs`, `load_protocol_test.rs`, `fx_swap_test.rs` | The tests. |
| `chimera-core/tests/fixtures/v1/project.prj` | The v1 project fixture. |
| `chimera-core/fuzz/fuzz_targets/decode_project.rs` | The `cargo fuzz` target. |
| `docs/adr/0043-projects-own-their-sounds.md`, `docs/adr/0046-project-load-protocol.md` | The ADRs, added to `docs/adr/README.md`. |

## Task order

1. The project model; `UiState` holds one `Project`.
2. The project file codec, `project_crc` and `TemplateCrc`.
3. Derived marks and the Part actions.
4. `ReplaceGuard` and ADR 0043.
5. The load gate on the audio side, and ADR 0046.
6. Save, load, list and delete on the card; the notes.
7. FX at the swap.
8. Boot, the `UiState` entry points and both shells.
9. Budgets, the bench and the ship STOP.

Each depends on the one before it, except Task 7, which needs only Task 5.

---

### Task 1: The project model; `UiState` holds one `Project`

**Files:**
- Create: `chimera-core/src/project/{mod,ids,pool,perf}.rs`, `chimera-core/tests/project_model_test.rs`
- Modify: `chimera-core/src/lib.rs` (`pub mod project;`); `preset.rs` (keeps `Sound`; `Part`, `Performance`, `PartEdit`, `part_block(_mut)` move to `project/perf.rs` and are re-exported; `SoundPool` goes); `factory.rs` (drop `load_factory`); `storage/crc.rs` (`pub(crate) struct CrcSink`, moved from `system.rs`); `storage/sound.rs` (+ `sound_crc`); `instrument.rs` (`AXI_RESIDENT` counts `size_of::<Project>()`, not `Performance` + `SoundPool`); `ui/mod.rs`, `ui/renderer.rs`, `ui/browser.rs`; `chimera-stm32/src/{main,shared,bench,sd_probe}.rs`; `chimera-desktop/src/main.rs`; every test that the compiler names (`preset_test`, `mixer_page_test`, `browser_test`, `ui_routing_test`, `memory_budget_test`, `instrument_test`, `common/rig.rs`, and the rest of the grep for `.performance`, `.pool`, `active_part`, `loaded_from`, `SoundPool`)

**Interfaces:**
- Consumes: `Sound`, `Sound::bits_eq`, `encode_sound`, `ProjectId`, `ProjectName` (plan 1).
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
    pub(in crate::project) fn clear(&mut self, s: SlotId);                // gen wrapping_add(1)
}
// perf.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin { Slot { slot: SlotId, generation: u16, crc: u32 }, Init(EngineType) }
pub struct Part { pub sound: Sound, origin: Origin, pub mix: PartParams }
impl Part { pub fn origin(&self) -> Origin; }
pub struct Performance { pub parts: [Part; MAX_PARTS], pub fx: FxParams }   // no `name`
impl Performance { pub fn new() -> Self; pub fn part(&self, p: PartId) -> &Part; }
// storage/sound.rs
pub fn sound_crc(s: &Sound) -> u32;   // CRC32 of encode_sound's bytes
// mod.rs
pub const NEW_NAME: &str = "NEW PROJECT";
pub struct ProjectMeta { id: Option<ProjectId>, name: ProjectName, saved_crc: Option<u32> }
impl ProjectMeta { pub fn id(&self) -> Option<ProjectId>; pub fn name(&self) -> ProjectName; pub fn saved_crc(&self) -> Option<u32>; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum PartFrom { Slot(SlotId), Init(EngineType) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct PartSource { pub part: PartId, pub from: PartFrom }
#[derive(Debug, PartialEq)] pub struct InUse(pub PartSet);
#[derive(Debug, PartialEq)] pub struct SlotEmpty;
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
    pub fn save_part_to(&mut self, p: PartId, s: SlotId) -> PartSet;  // store a copy, set p's Origin fresh; returns the other users of s
    pub fn load_part(&mut self, src: PartSource) -> Result<(), SlotEmpty>;  // unguarded until Task 4 makes it private
    pub(crate) fn reset_new(&mut self);
}
```

Private fields make the rules hold: only `project` sets an `Origin` or touches `gens`, and nothing outside it can build a `Part`. `init_in_place` and `reset_new` write the pool slot by slot, so the 28 KB pool never passes through the stack. NEW is: name `NEW_NAME`, id and `saved_crc` `None`, the 8 `factory_sound`s in slots 0–7, `Sound::init(e)` for each `EngineType::ALL` in slots 8 and 9, `Performance::new()` with every Origin `Init(EngineType::Algo)`, and `FxParams::default()`.

- [ ] **Step 1: Write the failing tests** in `project_model_test.rs`:

```rust
#[test]
fn ids_are_bounded() {
    assert_eq!(PartId::new(5).map(PartId::index), Some(5));
    assert_eq!(PartId::new(6), None);
    assert_eq!(SlotId::new(31).map(SlotId::index), Some(31));
    assert_eq!(SlotId::new(32), None);
    assert_eq!(PartId::ALL.len(), 6);
    assert_eq!(SlotId::ALL.len(), 32);
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
    assert_eq!(p.pool().generation(s), 0);
    p.pool_store(s, Sound::init(EngineType::Modal));
    assert_eq!(p.pool().generation(s), 1);
    assert_eq!(p.pool_clear(s), Ok(()));
    assert_eq!(p.pool().generation(s), 2);
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
    assert_eq!(p.part(part).origin(), Origin::Slot { slot: s, generation: 1, crc });
    assert!(p.part(part).sound.bits_eq(p.pool().get(s).unwrap()));
    assert_eq!(p.load_part(PartSource { part, from: PartFrom::Slot(SlotId::ALL[30]) }), Err(SlotEmpty));
}

#[test]
fn save_part_to_names_the_other_users() {
    let mut p = Project::boxed();
    let s = SlotId::ALL[0];
    for part in [PartId::ALL[0], PartId::ALL[4]] {
        p.load_part(PartSource { part, from: PartFrom::Slot(s) }).unwrap();
    }
    p.edit_part(PartId::ALL[0]).part.mix.level = 0.1; // mix: not the sound
    assert_eq!(p.save_part_to(PartId::ALL[0], s), PartSet::EMPTY.with(PartId::ALL[4]));
}
```

  Also the compile-fail doc tests on `Part` and `Pool` (in `perf.rs` and `pool.rs`): ```` ```compile_fail,E0451 ```` building `Part { sound, origin, mix }` outside the module; ```` ```compile_fail,E0624 ```` `pool.store(s, sound)` outside `project`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_model_test` → FAIL (no `project` module).
- [ ] **Step 3: Implement** the model as above. Then migrate the callers:
  - `UiState` replaces `performance`, `pool` and `active_part: usize` with private `project: Project` and `active_part: PartId`, and adds `pub fn project(&self) -> &Project` and `pub fn project_mut(&mut self) -> &mut Project`. `field_list!` and `init_in_place` follow; `project` is built in place.
  - Every `usize` Part index in `ui/` becomes `PartId`: `blocks`, `load_matrix`, `sync_mod_state`, `UiMode::SoundBrowser { part }`, `renderer`'s `active_part`. The chain ids keep their `usize` (navigation's).
  - The legacy browser's Load calls `project.load_part(PartSource { part, from })`, with `PartFrom::Init(engine)` for its INIT rows. Its Save calls `project.save_part_to(part, slot)`.
  - `AudioShared::update_from(ui.project().perf())` in both shells and in `shared::take_audio`; `bench::run(.., ui.project().perf())`.
  - `Performance.name` goes; `preset_test`'s name assert moves to `meta().name()`.
- [ ] **Step 4: Run** `cargo test -p chimera-core --test project_model_test` → PASS, then `just check` → PASS. No screen golden changes.
- [ ] **Step 5: Commit**

```bash
git add chimera-core/src chimera-core/tests chimera-stm32/src chimera-desktop/src
git commit -m "One Project holds the pool and the Parts; Parts and slots are typed ids"
```

### Task 2: The project file codec, `project_crc` and `TemplateCrc`

**Files:**
- Create: `chimera-core/src/project/codec.rs`, `chimera-core/tests/project_codec_test.rs`, `chimera-core/tests/fixtures/v1/project.prj`, `chimera-core/fuzz/fuzz_targets/decode_project.rs`
- Modify: `storage/frame.rs` (`FileKind::Project = 2`); `storage/record.rs` (four tags); `storage/sound.rs` (`SoundCheck::{new, step, finish}` `pub(crate)`; the four tags are `Corrupt` in a Sound file); `storage/system.rs` (the same in SYSTEM); `storage/file.rs` (`DecodeInPlace`); `storage/mod.rs` (exports); `project/mod.rs` (`init_in_place` and `reset_new` return `TemplateCrc`, and `boxed` returns the pair); `ui/mod.rs` (`template: TemplateCrc`, `pub fn template(&self) -> TemplateCrc`); `tests/project_model_test.rs` (`boxed` now returns the pair); `tests/storage_frame_test.rs` (the tag table); `tests/codec_compat_test.rs` (`write_v1_fixtures` writes `project.prj`); `tests/codec_fuzz_test.rs`; `chimera-core/fuzz/Cargo.toml`

**Interfaces:**
- Consumes: `encode_sound`, `SoundCheck`, `encode_block`/`decode_block`, `MIGRATIONS`, `RecordWriter`, `Check`, `Event`, `FileError`, `Framer` (plan 1); `Project`, `Pool`, `Origin`, `sound_crc` (Task 1).
- Produces:

```rust
// record.rs (frozen codes)
RecordTag::Slot   => 0x8007,  // payload: slot u8 (0..32); sound records follow
RecordTag::Part   => 0x8008,  // payload: part u8 (0..6); sound records, Block(Part), Origin follow
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
impl TemplateCrc { #[doc(hidden)] pub fn get(self) -> u32; }  // for tests
pub struct ProjectCheck { /* SoundCheck, context, seen slots/parts, fx seen */ }   // impl Check, KIND = Project
pub struct ProjectDecoder<'a> { /* ProjectCheck, target: &'a mut Project */ }     // impl Check + DecodeInPlace
impl<'a> ProjectDecoder<'a> { pub fn new(target: &'a mut Project) -> Self; }
```

Record order as written: `Fx` and the five FX `Block`s; for each filled slot in order, `Slot(n)` then `encode_sound`; for each Part in order, `Part(n)`, `encode_sound`, `Block(Part)` (its mix), then `Origin`. The reserved records (tempo, AFX map, CC map, set list, custom tag names) get codes when they're defined; `0x000B`–`0x000F` stay free for them.

The rules:
- A context record closes the previous context: its `SoundCheck::finish` runs, which applies the routes.
- `Slot` and `Part` indexes must be in range and unique. `Fx` appears at most once. All six `Part`s are required. Any breach is `Corrupt` (or `Bounds` for an out-of-range index). Records before the first context are `Corrupt`.
- In a `Part` context, `Block` with the Part block's code goes to the mix, `Origin` to the origin, and every other record to the `SoundCheck`, so the Sound rules (Engine first, once-only records) hold per Sound. In `Fx`, only FX `Block`s are allowed.
- A header with no name is `Corrupt`. `Project`, `Slot`, `Part` and `Fx` in a Sound or SYSTEM file are `Corrupt`.
- Pass 2's base, set at the header: every slot empty with generation 0, `FxParams::default()`, and the header's name. At `Part(n)`: `Sound::neutral(Algo)` until its Engine record, and mix `PartParams::for_part(n)`.
- At `finish`, each Part's Origin resolves:
  - `Slot(s)` with `s` filled becomes `Slot { slot: s, generation: pool.generation(s), crc: sound_crc(slot) }`;
  - `Slot(s)` with `s` empty, a missing `Origin`, or an unknown engine code becomes `Init(part's engine)`.
- `project_crc` and `sound_crc` hash through `CrcSink`; neither allocates or buffers.

- [ ] **Step 1: Write the failing tests** in `project_codec_test.rs`. The helpers `encode(&Project) -> Vec<u8>` and `decode(&[u8], &mut Project) -> Result<(), FileError>` run `write_file` and a `Framer` pass 1 through `ProjectCheck`, then pass 2 through `ProjectDecoder`, as `sound_codec_test.rs`'s helpers do. `full()` is a NEW project with all 32 slots filled (factory, INIT and edited copies), Parts loaded from slots 0, 3, 9 and INIT, two Parts edited after loading, and non-default FX, mix and name.

```rust
fn same(a: &Project, b: &Project) {
    assert_eq!(a.meta().name(), b.meta().name());
    for s in SlotId::ALL {
        match (a.pool().get(s), b.pool().get(s)) {
            (Some(x), Some(y)) => assert!(x.bits_eq(y), "slot {}", s.index()),
            (x, y) => assert!(x.is_none() && y.is_none()),
        }
    }
    for p in PartId::ALL {
        let (x, y) = (a.part(p), b.part(p));
        assert!(x.sound.bits_eq(&y.sound) && x.mix == y.mix && x.origin() == y.origin(), "part {}", p.index());
    }
    assert_eq!(a.perf().fx, b.perf().fx);
    assert_eq!(project_crc(a), project_crc(b));
}

#[test] fn new_round_trip() { let (p, _) = Project::boxed(); let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap(); same(&p, &q); }
#[test] fn full_pool_round_trip() { let p = full(); let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap(); same(&p, &q); assert_eq!(encode(&q), encode(&p)); }

#[test]
fn origin_to_empty_slot_reads_as_init() { // Review Focus 3
    // A file whose Part 0 has Origin [0, 31] and no Slot(31) record.
    let (mut q, _) = Project::boxed();
    decode(&with_origin(PartId::ALL[0], &[0, 31]), &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[0]).origin(), Origin::Init(q.part(PartId::ALL[0]).sound.engine()));
}

#[test]
fn structure_errors() {
    assert_eq!(decode_err(&missing_part(PartId::ALL[5])), FileError::Corrupt);
    assert_eq!(decode_err(&duplicate_slot(SlotId::ALL[2])), FileError::Corrupt);
    assert_eq!(decode_err(&slot_index(32)), FileError::Bounds);
    assert_eq!(decode_err(&part_index(6)), FileError::Bounds);
    assert_eq!(decode_err(&block_before_context()), FileError::Corrupt);
    assert_eq!(decode_err(&nameless_header()), FileError::Corrupt);
    assert_eq!(sound_file_decode_err(&[(RecordTag::Slot, vec![0])]), FileError::Corrupt);
}

#[test]
fn compatibility() {
    // Unknown non-critical 0x0070 inside a Part: skipped, the project equals the original.
    // Unknown critical 0x8070 anywhere: NeedsNewerFirmware.
    // A Part with no Block(Part): its mix is PartParams::for_part(n).
    // No Fx context: FxParams::default().
}

#[test]
fn crc_sees_name_pool_fx_and_mix_but_not_theme() {
    let (mut p, t) = Project::boxed();
    let base = project_crc(&p);
    assert_eq!(t.get(), base);
    p.set_name(ProjectName::new("ACID PARTY").unwrap());
    assert_ne!(project_crc(&p), base);
    // likewise, from a fresh NEW each time: pool_store, pool_clear, edit_fx().delay.mix,
    // edit_part(..).part.mix.pan: each changes the CRC; undoing the edit restores it.
}

#[test]
fn project_fixture_loads() {
    // fixtures/v1/project.prj: slot n bits_eq the decode of fixtures/v1/<its .snd>, every
    // Part's origin, mix and FX equal the recorded values, and its bytes' length and FNV-1a
    // join FIXTURE_BYTES.
}
```

  In `storage_frame_test.rs`, append `(RecordTag::Slot, 0x8007)`, `(Part, 0x8008)`, `(Fx, 0x8009)`, `(Origin, 0x000A)` to the tag table and assert `FileKind::Project as u8 == 2`. In `codec_fuzz_test.rs`, add `project_bytes_never_panic`: the existing xorshift seeds feed mutated `encode(full())` bytes to `decode`; every `Ok` project has every param within its `ParamSpec`, every Origin that names a filled slot, and no panic.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_codec_test --test storage_frame_test --test codec_fuzz_test` → FAIL.
- [ ] **Step 3: Implement** `codec.rs`, the tags and `DecodeInPlace`. `init_in_place` and `reset_new` end with `TemplateCrc(project_crc(self))`. Add the `cargo fuzz` target, which mirrors `decode_sound.rs`.
- [ ] **Step 4: Write the fixture.** Extend `write_v1_fixtures` to write `project.prj` from a fixed project (NEW; slot 11 an edited copy of slot 0; Part 1 from slot 11, then edited; Part 2 from slot 9; delay MIX 0.3; name `FIXTURE`). Run it once with `FIXTURE_WRITE=1`, and paste the printed row into `FIXTURE_BYTES`.
- [ ] **Step 5: Run** the tests → PASS. Then `just check` → PASS.
- [ ] **Step 6: Commit**

```bash
git add chimera-core/src chimera-core/tests chimera-core/fuzz
git commit -m "Projects as card records: slots, Parts and FX, round-tripped bit for bit"
```

### Task 3: Derived marks and the Part actions

**Files:**
- Create: `chimera-core/src/project/marks.rs`, `chimera-core/tests/project_marks_test.rs`
- Modify: `project/mod.rs` (`apply_part_action`)

**Interfaces:**
- Consumes: `Project`, `Part`, `Pool`, `Origin`, `sound_crc` (Task 1); `project_crc`, `TemplateCrc`, `encode`/`decode` helpers (Task 2).
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
impl Project { pub fn apply_part_action(&mut self, a: PartAction) -> Result<PartSet, ActionGone>; } // Ok: the other Parts now Stale
```

`part_status` follows § Global Constraints exactly. `part_actions`, in this order:

| Status (origin) | Actions |
|---|---|
| `Clean` | `NewSlot(first_free)` |
| `Edited` (`Slot(s)`) | `OverSlot(s)`, `NewSlot(first_free)`, `Revert(s)` |
| `Edited` (`Init`) | `NewSlot(first_free)` |
| `Stale(s)` | `Revert(s)` (shown as UPDATE), `NewSlot(first_free)` |

`NewSlot` appears only when a slot is free. `apply_part_action` returns `ActionGone` if the action no longer applies: the Part's status or origin changed, the `NewSlot` slot filled, or the `Revert` slot emptied. `OverSlot` and `NewSlot` go through `save_part_to`; `Revert` copies the slot back and sets the Origin fresh.

- [ ] **Step 1: Write the failing tests** in `project_marks_test.rs` (the spec's § Tests › Derived marks, and Review Focus 2):

```rust
#[test] fn new_is_pristine() { let (p, t) = Project::boxed(); assert_eq!(project_status(&p, t), ProjectStatus::Pristine); }

#[test]
fn edit_back_to_the_slot_is_clean() {
    let (mut p, _) = Project::boxed();
    let (a, s) = (PartId::ALL[0], SlotId::ALL[0]);
    p.load_part(PartSource { part: a, from: PartFrom::Slot(s) }).unwrap();
    let was = p.part(a).sound.params.filter.cutoff;
    p.edit_part(a).part.sound.params.filter.cutoff = was * 0.5;
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Edited);
    p.edit_part(a).part.sound.params.filter.cutoff = was;
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
}

#[test]
fn save_over_a_slot_stales_the_other_unedited_users() {
    // Parts 0 and 3 load slot 0; Part 0 edits and saves over it.
    // → apply_part_action(OverSlot) returns {P4}; P1 Clean, P4 Stale(slot 0).
    // A third user that was Edited stays Edited, never Stale.
}

#[test]
fn revert_is_bit_exact() {
    // Edit Part 0 (from slot 2), then Revert(slot 2): bits_eq the slot, Clean, Origin generation current.
}

#[test]
fn init_origin_never_stale() {
    // Origin::Init(Algo): untouched → Clean; edited → Edited; no pool change makes it Stale.
}

#[test]
fn edited_part_never_stale_after_reload() { // Review Focus 2
    // Part 0 loads slot 5, edits (Edited). encode → decode into a fresh project.
    // After the load: Part 0 Edited. Part 2 loads slot 5, edits, saves over it (the gen moves).
    // Part 0 is Edited, not Stale.
}

#[test]
fn stale_reloads_as_edited() {
    // Part 3 Stale at save time → after encode/decode it is Edited (differs from its slot).
}

#[test]
fn project_marks() {
    // NEW → Pristine. set_name → Modified. `mark_saved_for_test` (what a save does) → Saved.
    // pool_store into an empty slot → Modified; pool_clear of it → Saved again (the CRC hashes
    // content, not generations). edit_fx().reverb.mix changed, then restored → Modified, then
    // Saved. edit_part(..).part.mix.pan → Modified. A UiState theme change → still Saved.
}

#[test]
fn actions_offer_only_what_applies() {
    // The table above, row by row; a full pool drops NewSlot; a stale PartAction applied after
    // the pool changed → Err(ActionGone), and the project is unchanged (project_crc equal).
}
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_marks_test` → FAIL.
- [ ] **Step 3: Implement** `marks.rs` and `apply_part_action`. `pub fn mark_saved_for_test(&mut self)` (sets `saved_crc` to `project_crc`) is `#[doc(hidden)]` and `cfg(any(test, feature = "test-support"))`; Task 6's `save_project` is the real setter.
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/project chimera-core/tests/project_marks_test.rs
git commit -m "Part and project marks are derived; Part actions offer only what applies"
```

### Task 4: `ReplaceGuard` and ADR 0043

**Files:**
- Create: `chimera-core/src/project/guard.rs`, `chimera-core/tests/replace_guard_test.rs`, `docs/adr/0043-projects-own-their-sounds.md`
- Modify: `project/mod.rs` (`load_part` becomes private; `pub fn replace_part(&mut self, c: Confirmed<PartSource>) -> Result<(), SlotEmpty>`); `ui/mod.rs` (the legacy browser); `tests/project_*` (callers of `load_part` use the guard); `docs/adr/README.md`

**Interfaces:**
- Consumes: `part_status`, `project_status`, `TemplateCrc` (Tasks 2–3); `PartSource` (Task 1).
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Prompt { SavePartFirst, SaveProjectFirst }
pub trait Target: Copy { fn at_risk(&self, p: &Project, t: TemplateCrc) -> Option<Prompt>; }
impl Target for PartSource { .. }       // Edited → SavePartFirst; Clean or Stale → None
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum ProjectSource { File(ProjectId), New }
impl Target for ProjectSource { .. }    // Modified → SaveProjectFirst; Pristine or Saved → None
#[must_use] #[derive(Debug)] pub struct Confirmed<R>(R);   // private field
impl<R: Copy> Confirmed<R> { pub fn target(&self) -> R; }
#[must_use] #[derive(Debug)] pub struct NeedsConfirm<R> { pending: Pending<R>, prompt: Prompt }
impl<R> NeedsConfirm<R> { pub fn prompt(&self) -> Prompt; pub fn into_pending(self) -> Pending<R>; }
#[derive(Debug)] pub struct Pending<R>(R);                 // the source id, never parsed data
impl<R: Target> Pending<R> {
    pub fn source(&self) -> R;
    pub fn anyway(self) -> Confirmed<R>;                                            // REPLACE / LOAD ANYWAY
    pub fn save_then(self, p: &Project, t: TemplateCrc) -> Result<Confirmed<R>, NeedsConfirm<R>>; // re-checks after the caller's save
}
pub struct ReplaceGuard;
impl ReplaceGuard { pub fn check<R: Target>(p: &Project, t: TemplateCrc, r: R) -> Result<Confirmed<R>, NeedsConfirm<R>>; }
```

CANCEL is dropping the `Pending`. A save that was cancelled or failed leaves the target at risk, so `save_then` returns `NeedsConfirm` again and the caller aborts (§ Unsaved-state edge cases). Nothing re-reads a source at confirm: `replace_part` looks the slot up then (`Err(SlotEmpty)` if it was cleared), and a project load runs its own pass 1 (Task 6).

- [ ] **Step 1: Write the failing tests** in `replace_guard_test.rs`:

```rust
#[test]
fn part_replace_prompts_only_when_edited() {
    // Clean Part: check → Ok. Stale Part (UPDATE): Ok. Edited Part: Err with Prompt::SavePartFirst.
}

#[test]
fn project_replace_prompts_only_when_modified() {
    // for src in [ProjectSource::New, ProjectSource::File(ProjectId::new(3).unwrap())]:
    // Pristine → Ok; Saved → Ok; Modified → Err(SaveProjectFirst).
}

#[test]
fn each_answer_does_what_it_says() {
    // Edited Part 0, replace from slot 4:
    // anyway() → replace_part → Part 0 bits_eq slot 4, Clean.
    // save_then without saving → Err(NeedsConfirm) again, Part unchanged (cancelled save aborts).
    // apply_part_action(OverSlot), then save_then → Ok → replace applies.
    // Dropping the Pending (CANCEL) → Part 0 still Edited, bits unchanged.
}

#[test]
fn confirmed_slot_emptied_before_replace() {
    // check(Part 1 ← slot 20) → Ok(c); pool_clear(slot 20); replace_part(c) → Err(SlotEmpty); Part 1 unchanged.
}
```

  And the compile-fail doc tests on `Confirmed` (in `guard.rs`):
  - ```` ```compile_fail,E0423 ```` `let c = Confirmed(src);` outside the module;
  - ```` ```compile_fail,E0308 ```` `project.replace_part(ReplaceGuard::check(&p, t, ProjectSource::New).unwrap())`;
  - ```` ```compile_fail,E0624 ```` `project.load_part(src)` outside `project`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test replace_guard_test` and `cargo test -p chimera-core --doc` → FAIL.
- [ ] **Step 3: Implement** `guard.rs`. The legacy browser's Load becomes `ReplaceGuard::check(..).or_else(|n| Ok::<_, ()>(n.into_pending().anyway()))`, with the comment: `// No prompt screen yet: the navigation plan's prompt replaces this `anyway`.`
- [ ] **Step 4: Write ADR 0043**, "Projects own their sounds; marks are derived; one typed replace guard" (status Proposed; the owner accepts it at Task 9). It covers: the copy rules; `Part`/`Pool`/`Origin` and their private fields; the derived marks and why `Origin.crc` is the source's; `TemplateCrc`; `Confirmed<R>` per target; the Part actions table; explicit project saves; and the project file (FileKind 2; tags `0x8007`–`0x000A` with their payloads, the record order, the neutral base, six Parts required, an origin to an empty slot read as INIT; `0x000B`–`0x000F` kept free for the reserved records). Add its row to `docs/adr/README.md`.
- [ ] **Step 5: Run** → PASS; `just check` → PASS.
- [ ] **Step 6: Commit**

```bash
git add chimera-core/src chimera-core/tests docs/adr/0043-projects-own-their-sounds.md docs/adr/README.md
git commit -m "Every replace that can lose work goes through one typed guard"
```

### Task 5: The load gate on the audio side, and ADR 0046

**Files:**
- Create: `chimera-core/src/project/swap.rs`, `chimera-core/tests/load_protocol_test.rs`, `docs/adr/0046-project-load-protocol.md`
- Modify: `instrument.rs` (`AudioShared.epoch`; `update_from(perf, epoch)`; `kill_all`, `quiet`); `voice_alloc.rs` (`Allocator::kill_all`); `chimera-stm32/src/audio/engine.rs` (`Engine.gate`); `chimera-stm32/src/main.rs` (publish with `LOAD_LINK.epoch()`); `chimera-desktop/src/audio.rs` (gate in the callback; `update(perf, epoch)`); `chimera-desktop/src/main.rs`; every test calling `update_from`; `docs/adr/README.md`

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
| any | `requested != seen` | `seen = requested`; `kill`; then as below with the new `seen`: `Open` if `snapshot == seen` (the UI already timed out), else `Fading` |
| `Open` | — | `drain` |
| `Fading` | `snapshot == seen` | `drain`, `Open` |
| `Fading` | `quiet` | `ack: Some(seen)`, `Waiting` |
| `Fading` | else | nothing |
| `Waiting` | `snapshot == seen` | `drain`, `Open` |
| `Waiting` | else | nothing |

`kill_all` marks every booked slot dying, calls `Voice::kill` on each active voice (the 128-sample fade, rendered on the old snapshot) and clears `waiting`. It doesn't count `dropped_unheard`: nothing was refused. A killed slot is freed by `render`'s step 5 as a shed one is, so a later note-off finds nothing, and a held key isn't retriggered. Both shells call `before_block` once per callback, before the drain, and drain only when it returns `true`.

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
fn second_epoch_restarts_the_fade() { // Review Focus 4
    let mut g = LoadGate::new();
    let _ = g.step(1, 0, false);
    assert!(g.step(2, 0, false).kill);
    assert_eq!(g.step(2, 0, true).ack, Some(2));
    assert!(g.step(2, 2, true).drain);
}

#[test]
fn timeout_publish_reopens() { // Review Focus 4
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
    // Two held notes on Part 0 (a rig from instrument_test's helpers). kill_all, render 2 blocks
    // (FADE = 128 = 2 × 64): quiet() is true and every slot is free; a note-off for either note
    // changes nothing; nothing sounds until a new note-on.
}
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test load_protocol_test --features test-support` → FAIL.
- [ ] **Step 3: Implement** `swap.rs`, the `Instrument`/`Allocator` additions and both shells. The stm32 `Engine` gains `gate: LoadGate` (written in `init`), and `render_half` becomes `if e.gate.before_block(&LOAD_LINK, e.inst, shared) { e.notes.drain(..) }`. The desktop callback does the same. Both UI loops publish with `update_from(ui.project().perf(), LOAD_LINK.epoch())`.
- [ ] **Step 4: Write ADR 0046**, "Swap projects by epoch: fade the voices, then publish" (Proposed). It covers: `LOAD_EPOCH`/`LOAD_ACK` beside ADR 0021's triple buffer and why they are outside it; the gate table; the 10 ms timeout and what a late ack costs (the fade runs on the new snapshot); the MIDI rules (the spec's table, with CC and pitch bend not implemented); boot publishing with no fade. It also covers the in-place project load, which supersedes in part ADR 0045's "two passes, staged" for projects only: why (34 KB, no staging copy) and the fallback to NEW. Leave a `## FX at the swap` heading for Task 7. Add the README row.
- [ ] **Step 5: Run** the tests → PASS; `just check` → PASS (the firmware builds at every feature set).
- [ ] **Step 6: Commit**

```bash
git add chimera-core/src chimera-core/tests chimera-stm32/src chimera-desktop/src docs/adr/0046-project-load-protocol.md docs/adr/README.md
git commit -m "A load epoch fades every voice before the new project is published"
```

### Task 6: Save, load, list and delete on the card; the notes

**Files:**
- Create: `chimera-core/src/project/{store,note}.rs`, `chimera-core/tests/project_store_test.rs`
- Modify: `storage/file.rs` (`InPlace`, `load_ab_in_place`); `storage/frame.rs` (`pub fn peek_header(b: &[u8; HEADER_LEN]) -> Result<Header, FileError>`, the existing `decode` made public under that name); `chimera-fat/tests/power_cut_test.rs` (a project case); `load_protocol_test.rs` (end to end)

**Interfaces:**
- Consumes: `Card`, `CardEvent`, `CardFault`, `Store`, `save_ab`, `delete_ab`, `AbFile`, `LoadError`, `SaveError` (plan 1); `ProjectCheck`, `ProjectDecoder`, `DecodeInPlace`, `project_crc` (Task 2); `part_status` (Task 3); `Confirmed<ProjectSource>` (Task 4); `LoadLink`, `Swap` (Task 5).
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
                       Missing(Subject), LoadFailed(Subject), IsLoaded, NoIds, NewProject }
impl ProjectNote { pub fn line(&self) -> Line; }
// store.rs
pub fn project_file(id: ProjectId) -> AbFile;                          // Dir::Projects, id.stem()
pub fn new_project_id<S: Store>(card: &mut Card, store: &mut S) -> Result<ProjectId, ProjectNote>;
pub fn save_project<S: Store>(card: &mut Card, store: &mut S, p: &mut Project, id: ProjectId) -> ProjectNote;
pub struct LoadOutcome { pub swap: Option<Swap>, pub note: Option<ProjectNote>, pub event: Option<CardEvent> }
pub fn load_project<S: Store>(card: &mut Card, store: &mut S, p: &mut Project, go: Confirmed<ProjectSource>, link: &LoadLink) -> LoadOutcome;
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct ProjectEntry { pub id: ProjectId, pub name: Option<ProjectName>, pub err: Option<FileError> }
pub fn list_projects<S: Store>(card: &mut Card, store: &mut S, f: &mut dyn FnMut(ProjectEntry)) -> Result<CardEvent, ProjectNote>;
pub fn delete_project<S: Store>(card: &mut Card, store: &mut S, loaded: Option<ProjectId>, id: ProjectId) -> Result<(), ProjectNote>;
```

The rules:
- `load_ab_in_place` runs pass 1 on both sides with the decoder's `Check` and `pick`s, exactly as `load_ab`, so a failure there is `clobbered: false`. Pass 2 then runs `apply` on each event and `finish`. If it fails, or its CRC differs from pass 1's, the error is `clobbered: true`.
- `save_project` does, inside one `Card::run`: `make_dir(Chimera)`, `make_dir(Projects)`, then `save_ab(.., ProjectCheck, Some(name), encode_project)`. On success it sets `meta.id = Some(id)` and `meta.saved_crc = Some(project_crc(p))`, and returns `Saved(differ)`. `differ` counts the Parts whose status is `Edited` and whose Origin is `Slot`: `One(part, slot)` for one, `Many(n)` for more. On any error the meta is untouched.
- `load_project`:
  - `ProjectSource::New`: `reset_new`, `bump`, and no note;
  - `File(id)` loaded: `meta.id = Some(id)`, `saved_crc = Some(project_crc)`, `bump`;
  - pass 1 failed: the note, no swap, and the project untouched;
  - clobbered: `reset_new`, `bump`, and `LoadFailed(File(id))`.
  - The only writer of the epoch is here and in `boot_project` (Task 8), which doesn't bump.
- `new_project_id` lists `Projects` for `P#######.A`/`.B` stems and returns the highest id + 1. A missing directory means 1. Past `ProjectId::MAX` it is `NoIds`.
- `list_projects` reads each id's two headers (`Store::read` with a sink that breaks after `HEADER_LEN` bytes and calls `peek_header`). It reports the newer readable one's name, and `err` is `NeedsNewerFirmware`, `BadMagic` or `WrongKind` when that's all there is. Deleting the loaded project is `IsLoaded`.
- `line()` copy, exactly:

| Note | Line |
|---|---|
| `Saved(None)` | `SAVED` |
| `Saved(One(P2, slot 3))` | `SAVED: P2 DIFFERS FROM SLOT 03` |
| `Saved(Many(3))` | `SAVED: 3 PARTS DIFFER FROM SLOTS` |
| `Card { Full, Some(Name(ACID PARTY)) }` | `CARD FULL: ACID PARTY` |
| `Card { NoCard, None }` | `NO CARD` |
| `File { BadCrc, File(7) }` | `FILE CHECKSUM FAILED: P0000007` |
| `File { NeedsNewerFirmware, Name(ACID PARTY) }` | `NEEDS NEWER FIRMWARE: ACID PARTY` |
| `Missing(File(7))` | `PROJECT NOT FOUND: P0000007` |
| `LoadFailed(File(7))` | `LOAD FAILED: P0000007` |
| `IsLoaded` | `CAN NOT DELETE THE LOADED PROJECT` |
| `NoIds` | `NO PROJECT IDS LEFT` |
| `NewProject` | `NEW PROJECT` |

  Store and file messages are `StoreError::message()` and `FileError::message()`.

- [ ] **Step 1: Write the failing tests** in `project_store_test.rs` (on `MemStore`; `full()` as in Task 2):

```rust
#[test]
fn save_then_load_is_bit_identical() {
    let (mut store, mut card) = (MemStore::new(1), Card::new());
    let mut p = full();
    let id = new_project_id(&mut card, &mut store).unwrap();
    assert_eq!(id, ProjectId::new(1).unwrap());
    assert!(matches!(save_project(&mut card, &mut store, &mut p, id), ProjectNote::Saved(_)));
    let (mut q, t) = Project::boxed();
    let link = LoadLink::new();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File(id)).unwrap();
    let out = load_project(&mut card, &mut store, &mut q, go, &link);
    assert!(out.swap.is_some() && out.note.is_none());
    same(&p, &q);                                  // Task 2's helper
    assert_eq!(project_status(&q, t), ProjectStatus::Saved);
    assert_eq!(q.meta().id(), Some(id));
}

#[test] fn first_save_makes_the_dirs() { /* Review Focus 5: a MemStore with /CHIMERA only; save → Saved */ }

#[test]
fn full_card_save_stays_modified() { // Review Focus 5
    // Save once (gen 1), edit, then a MemStore capacity that fails the next write with Full:
    // note == Card { err: Full, subject: Some(Name(..)) }, status Modified, meta.saved_crc unchanged,
    // and a load gives generation 1's content.
}

#[test]
fn saved_toast_names_differing_parts() {
    // One Edited Part from slot 3 → Saved(One(P?, slot 3)); three → Saved(Many(3)); an edited
    // INIT-origin Part is not counted.
}

#[test]
fn pass_one_failure_leaves_ram() {
    // Corruption: truncated, bad CRC, bad magic on both sides → the File note, swap None, and
    // project_crc unchanged.
}

#[test]
fn pass_two_failure_falls_back_to_new() { // Review Focus 1
    // A MemStore wrapper that flips a byte in the picked side between its first and second read.
    // → note LoadFailed(File(id)), swap Some, project_crc(q) == t.get().
}

#[test] fn missing_file() { /* no files for id 9 → Missing(File(9)), swap None */ }
#[test] fn no_card_note() { /* ejected → Card { NoCard, .. }; Card::Absent */ }

#[test]
fn list_and_next_id() {
    // Save ids 1 and 4, a hand-made P0000006.A with format version 2 → list gives 1 and 4 with
    // their names and err None, and 6 with NeedsNewerFirmware; new_project_id → 7.
}

#[test]
fn delete_rules() {
    // delete_project(loaded = Some(1), 1) → Err(IsLoaded), files kept; delete 4 → both sides gone.
}

#[test] fn note_lines() { /* the table above, row by row */ }
```

  In `load_protocol_test.rs`, the end-to-end tests (spec § Tests › Load protocol). Each runs an `Instrument`, an `FxBus`, a `TripleBuffer<AudioShared>`, `NoteSources` and a `LoadGate` in one thread. `audio_block()` does `before_block`, then the drain, then `render`, and records the pair-1 samples. `settle`'s `within` closure runs `audio_block()` and counts to a cap, so the single thread plays both sides:
  - `swap_step_within_fade_bound` (L7): three held notes at velocity 127, their project saved and then another loaded. Across the blocks from the bump to two blocks after the publish, the largest sample step is at most the largest step of the 16 blocks before the bump, plus the loudest voice's peak / 128.
  - `fading_voice_keeps_its_routes`: the old Part routes LFO 1 to CUTOFF and the new one doesn't. The samples up to the ack equal, bit for bit, a reference run with no load that calls `kill_all` at the same block.
  - `note_on_between_epoch_and_publish_plays_new`: a note-on pushed after the bump and before the publish sounds on the new project's Part (its slot kind is the new engine) once the snapshot lands, and not before.
  - `held_key_not_retriggered`: after the publish no voice is active; the held key's note-off changes nothing.
  - `load_failure_still_publishes` (Review Focus 1): with pass 2 failing, the NEW project reaches the audio through the same epoch.
- [ ] **Step 2: Write the failing test** `project_cut_keeps_a_generation` in `chimera-fat/tests/power_cut_test.rs`. It follows `cut_at_every_block_write_keeps_a_generation`, for save 3 of `full()` (with a mix edit between saves) on FAT16: for every `k`, `load_ab_in_place` gives generation 2's or 3's project, never an error. Then the next full save succeeds.
- [ ] **Step 3: Run** `cargo test -p chimera-core --test project_store_test --test load_protocol_test --features test-support && cargo test -p chimera-fat --test power_cut_test` → FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** → PASS; `just check` → PASS.
- [ ] **Step 6: Commit**

```bash
git add chimera-core/src chimera-core/tests chimera-fat/tests/power_cut_test.rs
git commit -m "Projects save, load, list and delete on the card; a failed load falls back to NEW"
```

### Task 7: FX at the swap

The audit this plan owes (spec § Loading while playing). At the publish every voice has faded, so what's left sounding is the FX tails. A param "steps" when switching it moves a tail's sample value discontinuously. A gain or read position does that; a coefficient or LFO rate changes only the slope.

| Block | Param | Today | Ruling |
|---|---|---|---|
| CHORUS | MODE, RATE, DEPTH | per block, no smoothing | **Accept.** No feedback, and the line is at most 7 ms, fed by voices that faded before the ack. |
| CHORUS | MIX | return gain; 0 cuts the effect | **Smooth**, as a return (below). |
| DELAY | TIME | the read head jumps | **Smooth:** the base delay glides (a 50 ms one-pole, ramped per sample). |
| DELAY | FDBK, SAT, TONE | in-loop gain and coefficients | **Accept.** The loop's output stays continuous; the tail's decay changes. |
| DELAY | MIX | return gain; 0 cuts the tail | **Smooth.** |
| DELAY | REV | already a 20 ms per-block one-pole | Already smooth. |
| REVERB | GRIT, TIME, DAMP | coefficients | **Accept.** |
| REVERB | SIZE | already crossfades between size steps | Already smooth. |
| REVERB | MIX | return gain; 0 cuts the tail | **Smooth.** |
| COMP | THRESH, MAKEUP, MIX | already smoothed per block | Already smooth. |
| COMP | RATIO | enum; the gain reduction is smoothed in the log domain | **Accept.** |
| TAPE | all | `master-tape` builds only (ADR 0055) | **Accept.** Off the chain by default. |

**Files:**
- Create: `chimera-core/tests/fx_swap_test.rs`
- Modify: `dsp/fx_bus.rs` (`returns: [f32; FX_SENDS]`); `dsp/delay.rs` (`base: f32` state in `TapeDelay`); `docs/adr/0046-project-load-protocol.md` (§ FX at the swap: the table above)

**Interfaces:**
- Consumes: `FxBus::process`, `TapeDelay::run`, the REV SEND smoothing pattern (`fx_bus.rs:153-166`).
- Produces: no new public API. `FxBus` keeps each return's smoothed gain, a 20 ms one-pole per block, ramped across the block as REV SEND is. An effect runs while its smoothed return is non-zero, even with MIX at 0. The gain snaps to the target within 1e-6 and to 0 under 1e-4. `TapeDelay` keeps its base delay in samples and glides it (50 ms one-pole per block, ramped per sample). **Both are seeded to their target on the first block after `new`/`init_in_place`,** so a still patch renders exactly as before.

- [ ] **Step 1: Write the failing tests** in `fx_swap_test.rs`:

```rust
// `tail(fx)`: 200 ms of a loud burst into the send of the effect under test, then silence.
// `max_step(xs)`: the largest |x[n] − x[n−1]|.
#[test] fn delay_time_change_glides() { /* 375 → 100 ms mid-tail: max_step across the change ≤ 2× the tail's own max_step before it */ }
#[test] fn mix_to_zero_fades_the_tail() { /* for each of chorus, delay, reverb: MIX 0.5 → 0 mid-tail: max_step bound as above, and the return reaches exact 0 within 100 ms */ }
#[test] fn accepted_params_keep_the_tail_continuous() { /* FDBK, TONE, SAT, GRIT, TIME, DAMP stepped mid-tail: max_step ≤ 2× before */ }
```

  The existing `fx_golden_test`, `delay_test`, `reverb_test` and `chorus_test` stay green unchanged: that is the first-block seeding.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test fx_swap_test` → FAIL.
- [ ] **Step 3: Implement.** Re-run the bench's BUS row reasoning. An effect running on after MIX 0 can only cost up to its on-cost, which `FxBus::COST` already bills in full, so the cost doesn't change.
- [ ] **Step 4: Write** ADR 0046's § FX at the swap from the table.
- [ ] **Step 5: Run** → PASS; `just check` → PASS (both `master-tape` builds).
- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/dsp/fx_bus.rs chimera-core/src/dsp/delay.rs chimera-core/tests/fx_swap_test.rs docs/adr/0046-project-load-protocol.md
git commit -m "FX returns and delay time glide, so a project swap never steps a tail"
```

### Task 8: Boot, the `UiState` entry points and both shells

**Files:**
- Create: `chimera-core/tests/project_boot_test.rs`
- Modify: `project/store.rs` (`boot_project`); `ui/mod.rs` (the entry points); `ui/busy.rs` (`Toast.text: Line`; `ToastStep::Show(Line)`; `toast_for` builds a `Line`); `tests/toast_test.rs`, `tests/screen_golden_test.rs` (`toast_saved` re-recorded only if its pixels move); `chimera-stm32/src/main.rs`; `chimera-desktop/src/{main,store}.rs`

**Interfaces:**
- Consumes: Tasks 1–6; `SystemSync::write`, `SystemSettings.last_project` (plan 1).
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

- `boot_project` gives:
  - `None` → `NewProject`;
  - no card → `Card { NoCard, None }`;
  - a missing file → `Missing(File(id))`;
  - a pass 1 error → `File { .. }`;
  - a clobber → `LoadFailed`.

  In every case but a load, the project is left NEW, with no bump: nothing sounds yet (spec § Boot 2). On a load it sets `id` and `saved_crc`.
- `UiState::boot_project` shows the note, if any, and calls `project_replaced`.
- `save_project` and `load_project` show the note. After a successful save or load, they set `settings.last_project` and call `sync.write`; its error is ignored (see Decisions). `load_project` calls `project_replaced` whenever a `Swap` comes back and returns it. The shell then calls `swap.settle(&LOAD_LINK, within_10_ms)` and publishes. `save_project` takes a concrete `ProjectId`, so "saving without an id" doesn't compile.

- [ ] **Step 1: Write the failing tests** in `project_boot_test.rs` (on `MemStore`):

```rust
#[test] fn boot_loads_the_last_project() { /* save full() as id 2, SYSTEM last = 2; a fresh UiState boots → same(), Saved */ }
#[test] fn boot_without_a_last_id_is_new_and_says_so() { /* note NewProject; project_status Pristine */ }
#[test] fn boot_reasons() { /* no card / missing / corrupt / clobbered → the notes above; project_crc == template each time */ }
#[test] fn save_writes_the_last_project() { /* save_project(id 3) → SystemSync::boot on the same store gives last_project 3 */ }
#[test] fn load_writes_the_last_project() { /* load id 5 → SYSTEM last_project 5 */ }
#[test] fn replaced_ui_snaps() { /* after load_project, the renderer's values equal the new page's values without a lerp step */ }
```

  And the compile-fail doc test on `UiState::save_project`: ```` ```compile_fail,E0308 ```` passing `ui.project().meta().id()` (an `Option`).

  In `chimera-desktop/src/store.rs`, add `project_survives_a_relaunch` beside `theme_survives_a_relaunch`. A `UiState` on a `DirStore` in `unique_root()` edits a Part, a slot, FX and the name, gets `new_project_id`, and calls `save_project`. A second `UiState` on a new `DirStore` over the same root boots SYSTEM, then `boot_project`. Its project equals the first, bit for bit (Task 2's `same`, copied), and is `Saved`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test project_boot_test && cargo test -p chimera-desktop` → FAIL.
- [ ] **Step 3: Implement.** Then wire the shells:
  - **stm32 `synth`:** after the SYSTEM boot and theme, and before `shared::take_audio`, call `ui.boot_project(&mut card, store, settings.last_project)`. BUSY is still up, the audio hasn't started, and the watchdog isn't running, so the ~60 KB read is covered.
  - **Desktop `main`:** the same, after `SystemSync::boot`.
  - Neither shell calls save or load: the navigation plan does, and it settles each `Swap` with a 10 ms deadline on `controls::ticks()` (stm32) or `Instant` (desktop).
- [ ] **Step 4: Run** → PASS; `just check` → PASS. Then `just desktop` with `CHIMERA_CARD` pointing at the relaunch test's root: it boots that project, with no toast, and plays. With `CHIMERA_CARD=/nonexistent`: the toast says `NO CARD` and a NEW project plays.
- [ ] **Step 5: Commit**

```bash
git add chimera-core/src chimera-core/tests chimera-stm32/src/main.rs chimera-desktop/src
git commit -m "Boot loads the last project; saves and loads record it in SYSTEM"
```

### Task 9: Budgets, the bench and the ship STOP

**Files:**
- Modify: `chimera-core/tests/memory_budget_test.rs`; `chimera-stm32/src/bench.rs` (`PROJ CRC` row); this plan (`## Measured`); ADRs 0043 and 0046 (status, once accepted)

**Interfaces:**
- Consumes: `project_crc`, `Project` (Tasks 1–2); the bench's existing row pattern.
- Produces: the measurements the navigation plan relies on.

- [ ] **Step 1: Write the failing test** `axi_counts_the_project` in `memory_budget_test.rs`. The `Project` row replaces `Performance` and `SoundPool` in `axi_residents_fit`'s list; `size_of::<Project>() - (size_of::<Performance>() + 32 * size_of::<Option<Sound>>())` is at most 128 B (the meta and gens); `AXI_SRAM - AXI_RESIDENT >= 64 * 1024`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test memory_budget_test` → FAIL, until the row list is updated → PASS.
- [ ] **Step 3: Add the bench row** `PROJ CRC`: the cycles of one `project_crc` over a project with all 32 slots filled, printed like the others. The spec estimates well under 1 ms for 60 KB.
- [ ] **Step 4: Run** `just check` → PASS (the stack check covers `encode_project`, `ProjectDecoder` and `load_ab_in_place`).
- [ ] **Step 5: Commit**

```bash
git add chimera-core/tests/memory_budget_test.rs chimera-stm32/src/bench.rs
git commit -m "The project counts in the AXI budget; the bench times its CRC"
```

- [ ] **Step 6: STOP. Ask the owner to flash (`just flash`, then `just flash-bench`) and run these checks, and wait.**
  1. Boot with the usual card (SYSTEM, no project): BUSY, the theme, then the `NEW PROJECT` toast. The synth plays as before, and a held note through the boot's first seconds is clean.
  2. Copy the relaunch test's `CHIMERA/PROJECTS/P0000001.A` to the card, and set SYSTEM's last project to 1: from the desktop, boot with `CHIMERA_CARD` on that root, which writes it, then copy `SYSTEM.A`/`.B` too. Power on: that project plays (its FX and Part sounds are audible), with no toast.
  3. The bench: record `PROJ CRC` and the BUS row (unchanged by Task 7 within noise).

  Record the results under `## Measured`, with `.text` before and after. If `PROJ CRC` is 1 ms or more, file a GitHub issue: the navigation plan must then cache the status instead of recomputing it each input frame. File any other failure as an issue before fixing it. Once the owner agrees, move ADRs 0043 and 0046 to Accepted in the same commit as `## Measured`:

```bash
git add docs/superpowers/plans/2026-09-30-projects-core.md docs/adr/0043-projects-own-their-sounds.md docs/adr/0046-project-load-protocol.md docs/adr/README.md
git commit -m "Projects core measured on the chip; ADRs 0043 and 0046 accepted"
```

  **Handed to the navigation plan's STOP**, since they need a trigger this plan doesn't add:
  - save, power off, power on: the same project;
  - a load with held notes: no click, the tails ring on;
  - pulling the card mid-save: the previous generation loads;
  - the gate's cost, with no audio overrun during a load (`AudioStats`).

---

## Measured

(Task 9 fills this in.)
