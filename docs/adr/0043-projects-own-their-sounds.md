# 0043. Projects own their sounds; marks are derived; one typed replace guard

- **Status:** Proposed (accepted at the navigation plan's ship flash)
- **Deciders:** owner (projects and storage spec, 2026-09-28: § Model,
  § Types decide what's possible, § Copy rules; projects-core plan,
  2026-09-30), firmware

## Context
Until now the synth had one `Performance` and a 32-slot `SoundPool`, with
nothing saying which Sound a Part came from or whether it was edited. The
projects spec makes a project the whole musical state on the card: its
name, its pool and its six Parts and FX. That raises three questions this
ADR answers: who owns a Sound once it is copied, how the `*` and `◦` marks
stay true, and how a replace that would drop unsaved work is stopped. The
owner's rule is that every such rule is carried by a type or a pure
function, never by a stored flag that could drift.

ADR 0045 decides the card format; this ADR adds the project file to it.
ADR 0046 (Task 5) decides the load protocol.

## Decision

### Copies, and who may set what
- **Library → Pool → Part is a copy at each step.** A project owns its
  Sounds; a later library or slot edit never changes a Part.
- **`Project { meta, pool, perf }`, all private.** Reads go through
  `meta()`, `pool()`, `perf()` and `part(PartId)`. Edits go through `&mut`
  freely (`edit_part` gives the Sound and the mix, `edit_fx`, `set_name`,
  `pool_store`, `pool_clear`), because no mark is stored.
- **`Pool`**: 32 `Option<Sound>` slots indexed by `SlotId`, with a
  wrapping `u16` generation per slot; its fields, `store` and `clear` are
  `pub(in crate::project)`. `store` and `clear` move the generation.
  `Project::pool_clear` refuses a slot a Part uses (`InUse`).
- **`Part { sound, origin, mix }`**: `sound` and `mix` are public,
  `origin` is `pub(in crate::project)`. `Origin = Slot { slot, generation,
  crc } | Init(EngineType)` is set only by a load, a save over a slot and a
  revert.
- **Generations are never lowered.** A project load clears and stores every
  slot through `Pool::clear` and `Pool::store`, so every generation moves;
  no `Origin`, `PartAction` or `Confirmed` from before a load can match
  after it by accident. Tests compare an Origin by slot and CRC, never by
  generation. 65 536 stores to one slot between two looks may alias; that
  is accepted.

### Derived marks
- **`sound_crc`** is the CRC of the Sound's 16-byte padded name, then its
  `encode_sound` records: a rename is an edit.
- **`part_status(&Part, &Pool)`** is pure:
  - `Init(e)`: `Clean` if the Sound `bits_eq`s `Sound::init(e)`, else
    `Edited` (an INIT origin is never `Stale`);
  - `Slot`: `Clean` if the Sound `bits_eq`s the slot; else `Edited` if the
    generation is unchanged or `sound_crc` differs from `Origin.crc`; else
    `Stale(slot)`.
- **`Origin.crc` is the source's CRC**, not the Part's: the slot's at a
  load or a revert, or the Sound just stored at a save over a slot or to a
  new slot. A project load recomputes it from the loaded slot. If it were
  the Part's own, an edited Part saved and reloaded would derive `Stale`
  once another Part saved over the slot, and UPDATE, which asks nothing,
  would drop its edits (Review Focus 2,
  `edited_part_never_stale_after_reload`).
- **`project_status(&Project, TemplateCrc)`**: `Pristine` when
  `project_crc` equals the template's, `Saved` when it equals
  `meta.saved_crc`, else `Modified`. `project_crc` hashes the padded
  project name, then the project's records; generations and the header
  don't count. THEME is outside `Project`, so it can't mark it.
- **`TemplateCrc`** is NEW's CRC as built (factory Sounds in slots 1–8,
  INIT Algo and INIT Modal in 9–10, Parts on INIT Algo, `NEW PROJECT`). Only
  `project/template.rs` makes one; it is computed at run time, never pinned.

### Part actions
`part_actions(&Project, PartId)` offers only what applies, in menu order;
`PartAction` is opaque, and `apply_part_action` refuses one (`ActionGone`)
whose Part's Origin, status or `sound_crc` moved since it was offered.

| Part | Offers |
|---|---|
| Clean (INIT or slot) | NEW SLOT |
| Edited, from INIT | NEW SLOT |
| Edited, from a slot | SAVE OVER SLOT, NEW SLOT, REVERT |
| Stale | UPDATE (revert), NEW SLOT |

- NEW SLOT is offered only when a slot is free; REVERT only when its slot
  is filled.
- A Stale Part never offers SAVE OVER: it would undo the other Part's save.
- A save over a slot returns the other users of the slot that now derive
  `Stale`, for the save screen to name.
- Saving a project is explicit, never automatic, and doesn't clear Part
  marks: those compare a Part with its slot, not with the file.

### One typed guard on every replace that can lose work
- **`Confirmed<R>` is generic over the replace target**, `PartSource`
  (a Part from a slot or from INIT) or `ProjectSource` (`File { id, vol }`
  or `New`), so a project's confirmation can't be passed to a Part replace
  (compile-fail E0308). `Target` is sealed to those two.
- **`ReplaceGuard::check(&Project, TemplateCrc, R)`** returns `Confirmed`
  when nothing is at risk, else `NeedsConfirm` with its `Prompt`:
  - a slot into an `Edited` Part: **SAVE PART FIRST / REPLACE / CANCEL**;
  - `+ NEW` or a project load over a `Modified` project: **SAVE THEN LOAD /
    LOAD ANYWAY / CANCEL**;
  - `Clean`, `Stale`, `Pristine` and `Saved` never ask.
- **The answers.** `Pending::anyway` is REPLACE / LOAD ANYWAY. CANCEL drops
  the `Pending`. `Pending::save_then` re-checks after the caller's save, so
  a cancelled or failed save leaves the target at risk and the caller
  aborts. A `Pending` holds the source's id, never parsed data.
- **A confirmation carries a witness** of what it confirmed, taken when it
  is made: the Part's `sound_crc`, or the project's `project_crc`. A file
  load also carries the card's `VolumeId`. `Confirmed`'s fields are
  private to `project/guard.rs` (compile-fail E0451).
- **The replace refuses if the witness moved** (`ReplaceError::Changed`):
  a Part edited between `check` and `replace_part` would otherwise lose
  the edit without the prompt it now needs (Review Focus 4). `load_project`
  (Task 6) makes the same check, and refuses a swapped volume inside
  `Card::run`.
- **An empty slot is a runtime refusal** (`ReplaceError::SlotEmpty`), not a
  type: a `Pending` may be confirmed after its slot was cleared.
- **`Project::replace_part(Confirmed<PartSource>)` is the only public Part
  load.** `load_part` is private to `project` (compile-fail E0624); a
  Part action's REVERT uses it after its own re-check.
- The legacy sound browser has no prompt screen yet and answers REPLACE
  itself (https://github.com/joegiralt/chimera/issues/258); the
  navigation plan's prompt retires it.

### The project file
- **`FileKind` 2** (reserved by ADR 0045), at
  `/CHIMERA/PROJECTS/P0000001.A` and `.B`; the project's name is in the
  header (a header without a name is `Corrupt`); ids are the highest
  existing id + 1.
- **Records** (bit 15 is must-understand, ADR 0045):

  | Tag | Record | Payload |
  |---|---|---|
  | `0x8007` | `Slot` | slot `u8` (0..32), then the Sound's name, 16 B padded; the Sound's records follow |
  | `0x8008` | `Part` | Part `u8` (0..6), then the Sound's name, 16 B padded; the Sound's records, `Block(PART)` (the mix) and `Origin` follow |
  | `0x8009` | `Fx` | empty; the five FX `Block`s follow |
  | `0x000A` | `Origin` | `[0, slot]` or `[1, engine code]`, 2 B |

- **Order:** `Fx` and its blocks, then a `Slot` per filled slot in slot
  order, then the six `Part`s in order. `Slot`, `Part` and `Fx` each open a
  context; the records after one belong to it. A context repeated, a known
  record outside a context or not held by it, or a record read twice in a
  Part is `Corrupt`; an index out of range or a payload of the wrong shape
  is `Bounds`.
- **Every Part's own Sound is stored**, not only one that differs from its
  slot: no cross-record reference to get wrong, for about 16 % more bytes.
- **Neutral base:** each slot and Part Sound starts from `Sound::neutral`,
  each mix from `PartParams::for_part`, the FX from `FxParams::default`;
  every slot is cleared first.
- **All six Parts are required**; a file missing one is `Corrupt`.
- **An Origin to an empty slot reads as INIT** of the Part's engine, as
  does an unknown origin kind or engine code, or a missing Origin.
- **A new Origin shape takes a new tag**; `0x000A` stays 2 B.
- **The low bits `0x0B`–`0x0F` are kept free** for the reserved records:
  tempo, the AFX map per Part, the CC map, the set list, and plan 3's
  custom tag names.
- **The project list reads headers only**, one block per side. A parse
  error that needs the whole file shows when the load fails. A torn newer
  side's header name is listed although a load falls back to the older
  side.

### Not decided here
- System › MIDI Setup and `BlockRef::Channels` are deleted (CHANNEL is
  edited on the mixer's PART page) before ADR 0044 is accepted. The UX
  spec (`docs/chimera-ui-ux-spec.md`, § MIDI Block, Channel Overview) still
  lists it until the navigation plan amends it.

## Alternatives considered
- **A stored dirty flag per Part and per project.** Every edit path would
  have to set it, and an edit undone by hand would stay `*`. Derived marks
  can't disagree with the bits.
- **The spec's single `Replace` enum and one `Confirmed`.** A project
  confirmation could then be handed to a Part replace; generic `Confirmed<R>`
  makes that a compile error.
- **A confirmation without a witness.** A Part confirmed Clean and edited
  before the replace would lose the edit unasked.
- **An empty slot ruled out by type** (a load taking `&Sound`). A `Pending`
  confirmed later can't hold a borrow of the pool, and the slot may be
  cleared in between, so it is a runtime refusal.
- **`Origin.crc` from the Part's own Sound.** See Review Focus 2 above.
- **Storing only a Part's diff from its slot.** Smaller files, but a
  cross-record reference a bad file could break.

## Consequences
- Every replace site needs a `TemplateCrc` and a prompt answer; the
  navigation plan builds the prompt screens on `Prompt` and `Pending`.
- `project_status` hashes the whole project per call (~20–45 KB); caching
  it on a revision counter is
  https://github.com/joegiralt/chimera/issues/257.
- `part_status` on an INIT origin builds `Sound::init(e)` on the stack per
  call (~900 B), within the 8 KB frame rule.
- A `PartAction` or `Confirmed` from before a project load can't apply
  after it: the generation, Origin or witness moved.

## Sources
- Spec: `docs/superpowers/specs/2026-09-28-projects-storage-design.md`
  (§ Model, § Types decide what's possible, § Derived marks, § Copy rules,
  § Unsaved-state edge cases, § Format).
- Plan: `docs/superpowers/plans/2026-09-30-projects-core.md` (§ Decisions,
  Review Focus 2 and 4).
- Code: `chimera-core/src/project/` (`mod.rs`, `pool.rs`, `parts.rs`,
  `marks.rs`, `guard.rs`, `codec.rs`, `template.rs`),
  `chimera-core/src/storage/record.rs` (`RecordTag`, `records_crc`).
- Tests: `project_model_test`, `project_codec_test`, `project_marks_test`,
  `replace_guard_test`, `codec_compat_test::project_fixture_loads`.
- ADRs 0045 (card format), 0044 (navigation ladder, proposed), 0046
  (project load protocol, Task 5).
