# 0044. One ladder and one button map for Parts, the mixer and ORBIT

- **Status:** Proposed
- **Deciders:** project owner (2026-09-30); firmware

## Context
Three designs claimed the same eleven buttons at once:

- **The storage epic's one ladder** (`2026-09-28-projects-storage-design.md`,
  § Navigation, § The keys): Projects › Project › Part › Sound › pages, EDIT
  down, MENU tap up, MENU hold (600 ms) System, SEQ save. It retired the
  MIX+B*n* Mixer chain and folded its PART and SENDS pages into a row list
  on rung 2.
- **ORBIT** (branch `orbit`, `2026-09-30-orbit-design.md` § 7): SEQ hold
  (500 ms) enters ORBIT, SEQ tap stays sub-page up on release, MENU leaves
  ORBIT for System, EDIT on ORB jumps to the ring's Part.
- **Today's mixer work** (branch `modal2-resonators`, Task 15, ADR 0057 in
  progress): pressing B*n* on Part *n*'s pages toggles to its mixer and back,
  because a Part's sends are what you reach for while shaping its sound.

Each was sound alone; together they gave SEQ two meanings, two hold
thresholds, and a mixer both retired and extended. The owner approved one map
on 2026-09-30.

## Decision

### The keys

| Key | Meaning |
|---|---|
| **B*n*** | From anywhere on the ladder: Part *n*'s sound pages (`Pages(n, 0,0)`, as today). From Part *n*'s sound pages: Part *n*'s rung, the mixer (PART and SENDS), opening on the last-used mixer page, SENDS until one is used. From Part *n*'s rung: back to the sound page it left. |
| **MIX + B*n*** | Straight to Part *n*'s rung. MIX+B6 stays Demo until Demo moves to a debug-only System row. |
| **EDIT** | Down or open (`down`). On pages, sub-page down, as today. |
| **MENU tap** | Up one rung (`up`): pages → Part → Project → Projects. Acts on release, before `HOLD_MS`. |
| **MENU hold** | System, from anywhere, ORBIT included. |
| **SEQ tap** | Save what this rung is about: the Sound, the Part or the Project. On pages, in System and in ORBIT it keeps today's sub-page up. Acts on release, before `HOLD_MS`. |
| **SEQ hold** | ORBIT, from anywhere. Inside ORBIT, back to where you came from. |
| **PLUS / MINUS** | Sideways: the next or previous page or node. On the Part rung, PART then SENDS, then on to the next Part's PART, and back the same way, so one key walks every Part's mixer. On the Sound rung, the next or previous Part. |
| **MIX+MINUS** | Remove, behind a confirm (storage spec § The keys). |
| **MIX+PLUS** | Prime a route on pages (ADR 0017); rename and retag on a library entry. |
| **MIX+MENU** | Reserved for the chain editor. |

Inside ORBIT, B1–B5 pick a ring, MIX+B1–B5 mute one and B6 is PLAY/STOP, as
the ORBIT spec § 7 says: ORBIT is off the ladder and keeps its own B layer.
You leave it by MENU tap, SEQ hold, EDIT on ORB or MENU hold.

### `Location`

```rust
pub struct Location(Loc);              // opaque; `Loc` is private to ui::nav
enum Loc {
    Projects,                          // rung 0
    Project,                           // rung 1
    Part(PartId, MixPage),             // rung 2: the mixer. New: MixPage
    Sound(PartId),                     // rung 3
    Pages(PartId, PageAt),             // a sound's pages
    Fx(PageAt),                        // CHORUS, DELAY, EFX, TAPE, MASTER
    System(PageAt),                    // THEME, UPDATES, ABOUT (+ AUDIO)
    Orbit(OrbitAt),                    // new: ORB, RING, CHRD, SCN
}
pub enum MixPage { Part, Sends }
pub struct Recall {                    // held by UiState beside the Location
    pages: [PageAt; 6],                // where each Part's sound pages were left
    mix: MixPage,                      // the last-used mixer page, all Parts
    orbit: OrbitAt,                    // the ORBIT page last left
    from: Location,                    // where SEQ hold came from; never Orbit
}
```

`Loc` stays `Copy` and non-recursive: ORBIT's way back lives in `Recall`, not
in the variant, so no box and no heap.

- **`down(self, row) -> Option<Location>`** is EDIT, as the storage spec has
  it, except:
  - `Part(p, _)`: → `Sound(p)`. The mixer pages have no sub-pages, so EDIT
    is free there. Part *n*'s sound is one EDIT down, then EDIT on its
    current slot for its pages, or B*n* straight to them.
  - `Orbit(ORB)`: → `Pages(p, recall.pages[p])`, where `p` is the Part the
    selected ring plays. Other ORBIT pages: sub-page down, as the ORBIT spec
    says.
- **`up(self, &Recall) -> Location`** is a MENU tap:

  | From | To |
  |---|---|
  | `Projects` | `Projects` (no-op) |
  | `Project` | `Projects` |
  | `Part(p, _)` | `Project` |
  | `Sound(p)` | `Part(p, recall.mix)` |
  | `Pages(p, _)` | `Part(p, recall.mix)` |
  | `Fx(_)` | `Project` |
  | `System(_)` | `Projects` |
  | `Orbit(_)` | `recall.from` |

- **`part_key(self, n, &Recall) -> Location`** is B*n*, the toggle:
  `Pages(n, _)` → `Part(n, recall.mix)`; `Part(n, _)` →
  `Pages(n, recall.pages[n])`; anything else → `Pages(n, 0,0)`. It is sugar,
  defined as a composition of `down` and `up`, so the storage spec's
  property test (every jump lands where its path ends) covers it.
- **`orbit_key(self, &Recall) -> Location`** is SEQ hold: from outside ORBIT,
  → `Orbit(recall.orbit)` and `from = self`; from `Orbit(_)`, → `recall.from`.
- Leaving a place writes `Recall`: a Part's `PageAt` on leaving its pages,
  `mix` on leaving the mixer, `orbit` on leaving ORBIT.

### Tap and hold

- One constant, **`ui::hold::HOLD_MS` = 500 ms**, for MENU and SEQ. It
  replaces the storage spec's `MENU_HOLD_MS` = 600 ms.
- A hold fires once, when the key has been down `HOLD_MS`, before release;
  its release does nothing. A tap is a press released before `HOLD_MS`, and
  acts on the release. So a tap and a hold on one key never both fire.
- This is the storage spec's edge latching (press timestamps latched in the
  controls tick, no press lost across a blocking card operation) read by the
  ORBIT spec's pure `HoldGate`. One mechanism, not two.
- Why 500 ms: a deliberate tap takes 100–200 ms, so 500 ms leaves room, and
  it is the usual long-press time. The tap's own latency doesn't depend on
  the threshold, since a tap acts on release; only a hold waits. The 600 ms
  had no measurement behind it; the ORBIT spec's 500 ms has its tests
  written (`hold_gate_*`).

## Alternatives considered
- **The storage spec's retired mixer:** PART and SENDS as a row list on
  rung 2, MIX+B*n* gone. It costs a new row-list screen for params the page
  renderer already draws, and loses the one-press hop from a sound to its
  sends that ADR 0057 was written for. The mixer pages become the rung
  instead.
- **SEQ = ORBIT only** (the hold, no tap meaning): frees SEQ of the save
  on rungs and sub-page up on pages, but then save needs another key, and
  every key is taken. With tap-on-release, both fit on one key.
- **MENU = leave** (MENU tap always to System, or out of any mode, as
  today and as the ORBIT spec had it): one rule, but it throws away the
  ladder's "up" and the owner's "tap → project, hold → system". Up is
  already the way out of a place; from ORBIT, up is where you came from.
- **Keep two thresholds** (600 ms MENU, 500 ms SEQ): two numbers for one
  gesture, and a hand would learn neither.
- **B*n* from ORBIT to Part *n*** (the "from anywhere" reading applied to
  ORBIT too): costs ORBIT its ring keys, and no other row of buttons is
  free for them. EDIT on ORB already reaches the ring's Part. The owner
  should confirm this reading.

## Consequences
- **Absorbs ADR 0057** (the Part-button toggle, in progress on branch
  `modal2-resonators`): B*n* on Part *n*'s pages → its mixer and back, and
  MIX+B*n* straight to it. 0057 should not be written separately; Task 15
  builds this ADR's B*n* and MIX+B*n* rows and `Recall.mix`, and cites 0044.
- **ADR 0017** is unchanged: MIX+PLUS primes a route on pages. It stays inert
  on rungs, and renames and retags on a library entry (storage spec).
- **The storage spec** § Navigation and § The keys are amended in place
  (marked "amended 2026-09-30, owner: ADR 0044"). The Mixer chain is not
  retired: its PART and SENDS pages are rung 2, its FX nodes are `Fx`. MIDI
  Setup's channels still move to PART. Where its § ADRs and § Decisions
  awaiting owner review differ, this ADR rules.
- **"Same button = snap home"** (UX spec invariant 7) goes: B*n* on Part *n*'s
  pages now toggles to the mixer. MINUS back to node 0 replaces it. Plan 2
  amends `docs/chimera-ui-ux-spec.md`: invariants 1, 2, 3 and 7, and the
  control table's B1–B6, MIX+B1–B6, MENU and Seq/Edit rows.
- **Tests** (plan 2): `part_key` toggles and restores the page left; the
  mixer opens on SENDS, then on the last-used page; `up` from `Orbit` and
  SEQ hold from `Orbit` both return to `from`; `from` is never `Orbit`; a
  499 ms press is a tap on release, 500 ms a hold, never both; the
  `down`-path property test covers `part_key` and `orbit_key`.
- **The ORBIT spec** (branch `orbit`, `docs/superpowers/specs/2026-09-30-orbit-design.md`)
  must be amended there. The lines, as of `orbit` `ee4380f`:
  - **362** (hold SEQ): "Inside ORBIT it goes to ORB" → "Inside ORBIT it
    returns to where you came from (ADR 0044)".
  - **363** (tap SEQ): "in the browser, save" → "on a ladder rung, save
    (ADR 0044)".
  - **367** (ORB, EDIT): "Jump to the pages of the Part the selected ring
    plays (`ChainId::Part`)" → "… (`Pages(p, recall.pages[p])`); MENU tap
    from there goes to Part *p*'s rung, not back to ORBIT".
  - **370** (ORBIT, MENU): "Leave, to System, as today" → "MENU tap: back to
    where you came from. MENU hold: System."
  - **373** (threshold): "500 ms" stands, as the shared `ui::hold::HOLD_MS`
    for MENU and SEQ; cite ADR 0044.
  - **374** (tap on release): add MENU's tap, and "the browser's save" →
    "a rung's save".
  - **375** (`HoldGate`): it reads the storage spec's latched edges and
    timestamps; one gate for MENU and SEQ.
  - **376** (EDIT: jump or sub-page): the jump lands on the Part's sound
    pages as a ladder `Location`.
  - **377**: "EDIT + B<n> (the browser)" → "EDIT + B<n> (the Sound rung)";
    "MIX + B6 keeps its job (Demo)" stands.
  - **378** (leaving): "Holding SEQ returns to the ORBIT page you left"
    stands; add "MENU tap or SEQ hold inside ORBIT returns to where you
    came from".
  - **478** (`hold_gate_*`): the test pins `HOLD_MS`, not a literal 500.
  - **480** (`orbit_buttons`): "MENU opens System" → "hold MENU opens
    System; tap MENU returns to where you came from".
  - **513** (plan step 3's ADR): "the hold threshold" → "per ADR 0044".
  - **570** (summary): "MENU leaves as today" → "MENU tap goes back where
    you came from; hold MENU opens System".
  - **614**: "inside ORBIT it goes to ORB" → "inside ORBIT it returns to
    where you came from".
- Costs: `Recall` is about a dozen bytes of UI state; the ladder gains one
  variant and two sugar functions. Nothing touches the audio thread.

## Sources
- `docs/superpowers/specs/2026-09-28-projects-storage-design.md` § Navigation, § The keys
- `git show orbit:docs/superpowers/specs/2026-09-30-orbit-design.md` § 7
- Branch `modal2-resonators`: Task 15; ADR 0057 (in progress)
- ADR 0017; `docs/chimera-ui-ux-spec.md` § Controls, § Invariants
- `chimera-core/src/ui/chain.rs:75-127` (today's B*n*, MENU, SEQ, EDIT);
  `chimera-core/src/ui/block_registry.rs` (`PART`, `SENDS`, `MIXER_CHANNEL_CHAIN`)
