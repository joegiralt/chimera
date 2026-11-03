# EDIT+Part Opens on the Loaded Sound Implementation Plan

**Goal:** EDIT+Part opens that Part's Sound browser with the cursor on the row its Sound came from.

**Architecture:** A Part already keeps its `Origin` (`chimera-engine/src/project/parts.rs:17`: `Slot{slot, generation, crc}` | `Init(EngineType)`), saved in the project. A pure function maps a still-valid origin to a PROJECT-tab entry index. `Browse::at(entry)` puts the cursor there and scrolls it into view (`browser::follow`). `NavKey::EditPart` (`chimera-core/src/ui/nav.rs:837`) and EDIT on a Part page (`nav.rs:850`) open `Sound(n, Browse::at(..))` instead of `Browse::default()`.

**Tech Stack:** Rust no_std, chimera-core UI, chimera-engine project.

**Spec:** `docs/superpowers/specs/2026-11-03-browser-opens-on-loaded-sound-design.md`

## Global Constraints

- Bank 1 is frozen (ADR 0198). `ui::browser` and `ui::nav` live in bank 1, and this change may grow bank 1 only by its measured bytes. Measure on clean committed trees, before and after. If a set leaves its 64-B slack, raise its line in `tools/bank1-budget.txt` as `(measured X -> Y; from Z)`, naming ADR 0198. `just check-bank1` and `door-check` must pass.
- No project format change and no new record.
- Old behaviour is the fallback: when the origin maps to nothing, `Browse::default()`.
- Terse comments that match the surrounding code. No type prefixes in commits, never stage `chimera.bin`.

## Review Focus

- The origin slot was overwritten by another Sound, or emptied: no jump to a row that now holds a different Sound (the generation or CRC no longer matches → fallback).
- The origin slot is far down the pool (slot 100 of 128): the cursor row must be visible on open (scroll follows), not off-screen.
- `Init(e)` for an engine absent from this build's `INIT_TYPES` (a cfg'd-out engine): fallback, no panic.
- EDIT pressed on a Part page and EDIT+Part give the same row.
- The browser's tab: opening always lands on PROJECT, even if the last visit was LIBRARY or FACTORY.

---

### Task 1: The browser opens on the Part's origin

**Files:**
- Modify: `chimera-core/src/ui/browser.rs` (add `entry_of`, `Browse::at`)
- Modify: `chimera-core/src/ui/nav.rs:837, :850` (open with the origin)
- Test: `chimera-core/tests/browser_test.rs` (or the existing browser/nav test file; follow the precedent)

**Interfaces:**
- Consumes: `Part::origin() -> Origin`; the pool's per-slot generation and CRC (use whatever `part_status`/marks already use to judge `Origin::Slot` validity, `chimera-engine/src/project/marks.rs`); `browser::INIT_TYPES`, `HEAD`, `follow`.
- Produces: `pub fn entry_of(origin: Origin, pool: &<pool type>) -> Option<usize>` (PROJECT entry index), and `Browse::at(entry: usize) -> Browse` (cursor on `entry`, scroll from `follow`, library fields default).

- [ ] **Step 1: Write failing tests**
  - `opens_on_its_slot`: a Part loaded from slot 5 → `entry_of(..) == Some(HEAD + 5)`; `Browse::at` puts the cursor there.
  - `opens_on_its_init`: `Origin::Init(e)` → `Some(index of e in INIT_TYPES)`.
  - `stale_slot_opens_as_today`: slot 5 rewritten (generation bumped) or emptied → `None`.
  - `far_slot_scrolls_into_view`: slot 100 → the cursor is inside the visible window (`scroll <= cursor < scroll + rows`).
  - `edit_and_edit_part_agree`: `NavKey::EditPart(p)` and EDIT on a Part page land on the same `Location::Sound(p, b)`.
- [ ] **Step 2: Run and see them fail** (`cargo test -p chimera-core --test <file>`, unresolved `entry_of`/`Browse::at`).
- [ ] **Step 3: Implement** `entry_of` and `Browse::at` in `browser.rs`, and use them at `nav.rs:837` and `:850` (nav needs the Part's origin and the pool; pass them in the way nav already reaches project state).
- [ ] **Step 4: Run the tests to green**, then `just check`.
- [ ] **Step 5: Measure bank 1** (clean committed trees, before and after), raise lines only if a set leaves its slack, and commit.

### Task 2 (coordinator, hardware)

- [ ] Flash, after checking `status` reads SAVED. Load a library Sound into Part 2, go to another Part, then press EDIT+Part 2: the cursor sits on that Sound's row. Check a CLEARed Part (its INIT row) and an untouched Part. Take a `shot` of each.
