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
  ORBIT for System, EDIT on ORB jumps to the ring's Part (as first drafted;
  the owner's mode rule below replaces the last).
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
| **B*n*** | From anywhere on the ladder, ORBIT excepted (below): Part *n*'s sound pages (`Pages(n, 0,0)`, as today). From Part *n*'s sound pages: Part *n*'s rung, the mixer (PART and SENDS), opening on the last-used mixer page, SENDS until one is used. From Part *n*'s rung: back to the sound page it left. |
| **MIX + B*n*** | Straight to Part *n*'s rung. MIX+B6 stays Demo until Demo moves to a debug-only System row. |
| **EDIT** | Down or open (`down`). On pages, sub-page down, as today. In ORBIT, only ORBIT's own pages: on RING it cycles the sub-pages, RHYTHM → VOICE → NOTES → back to RHYTHM (amended 2026-09-30, owner); SCN's commit; it never leaves ORBIT. |
| **Hold B*n* in ORBIT** | Out to the pages of the Part ring *n* plays: `Pages(p, recall.pages[p])`. Fires at `HOLD_MS`; the release does nothing. The only way from ORBIT to sound design. |
| **MENU tap** | Up one rung (`up`): pages → Part → Project → Projects. Acts on release, before `HOLD_MS`. |
| **MENU hold** | System, from anywhere, ORBIT included. |
| **SEQ tap** | Save what this rung is about: the Sound, the Part or the Project. On pages and in System it keeps today's sub-page up. In ORBIT it always saves the scene, on every ORBIT page, since EDIT cycles the RING sub-pages and no sub-page-up is needed there (amended 2026-09-30, owner). Acts on release, before `HOLD_MS`. |
| **SEQ hold** | ORBIT, from anywhere, including from the pages a hold B*n* opened. |
| **MENU tap in ORBIT** | Leave ORBIT, back to where you were before; ORBIT keeps playing. |
| **PLUS / MINUS** | Sideways: the next or previous page or node. On a Part's rung (the mixer), PLUS steps PART → SENDS → the next Part's mixer, and MINUS goes the other way, so one key walks every Part's mixer (amended 2026-09-30, owner). On the Sound rung, the next or previous Part. |
| **MIX+MINUS** | Remove, behind a confirm (storage spec § The keys). |
| **MIX+PLUS** | Prime a route on pages (ADR 0017); rename and retag on a library entry. |
| **MIX+MENU** | Reserved for the chain editor. |

**ORBIT is a mode** (owner, 2026-09-30). Inside it every control serves the
sequencer:

- **B1–B5** are rings (sequences) 1–5, and never jump to a Part.
- **MIX + B1–B5** mutes or unmutes a ring.
- **B6** is PLAY/STOP.
- **SEQ tap** saves the scene, on every ORBIT page (amended 2026-09-30, owner).
- **Encoders, PLUS/MINUS and EDIT** act only on ORBIT's pages. EDIT cycles the RING sub-pages, RHYTHM → VOICE → NOTES → RHYTHM (amended 2026-09-30, owner).

Crossing to sound design is deliberately expensive: hold B*n* (the shared
`HOLD_MS`) jumps out to the pages of the Part ring *n* plays. The old "EDIT on
ORB jumps to the ring's Part" shortcut is removed. MENU tap leaves ORBIT, back
to where you were before, and ORBIT keeps playing; hold SEQ from anywhere
returns to ORBIT. MENU hold is System, as everywhere.

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
  - `Orbit(_)`: cycles the sub-pages (RING: RHYTHM → VOICE → NOTES → RHYTHM; amended 2026-09-30, owner); never leaves ORBIT.
    There is no jump from ORB (removed 2026-09-30, owner).
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
  → `Orbit(recall.orbit)` and `from = self`; from `Orbit(_)`, it is a no-op.
- **`orbit_out(self, ring, &Recall) -> Location`** is hold B*n* in ORBIT: →
  `Pages(p, recall.pages[p])`, `p` the Part ring *n* plays; `from` is not
  touched. MENU tap there goes up the ladder to Part *p*'s rung, not back to
  ORBIT; hold SEQ returns.
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
  ORBIT too), or EDIT on ORB to the ring's Part: costs ORBIT its ring keys
  or its EDIT, and no other row of buttons is free. The owner ruled ORBIT a
  mode instead, with the hold B*n* crossing.

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
  mixer opens on SENDS, then on the last-used page; `up` from `Orbit`
  returns to `from`; SEQ hold from `Orbit` does nothing; hold B*n* in ORBIT
  lands on the ring's Part pages and B*n* taps never leave ORBIT; `from` is
  never `Orbit`; in ORBIT a SEQ tap saves the scene on every page, and three EDITs on RING return to RHYTHM; PLUS on a Part's mixer walks PART → SENDS → the next Part's mixer, MINUS the reverse (amended 2026-09-30, owner); a
  499 ms press is a tap on release, 500 ms a hold, never both; the
  `down`-path property test covers `part_key`, `orbit_key` and `orbit_out`.
- **The ORBIT spec** (branch `orbit`, `docs/superpowers/specs/2026-09-30-orbit-design.md`)
  must be amended there, each change marked "(amended 2026-09-30, owner:
  ORBIT is a mode; ADR 0044)". The lines, as of `orbit` `ee4380f`:
  - **362** (hold SEQ): "Inside ORBIT it goes to ORB" → "hold SEQ from
    anywhere returns to ORBIT; inside it does nothing".
  - **363** (tap SEQ): "in the browser, save" → "on a ladder rung, save; in
    ORBIT, save the scene".
  - **364, 366** (B1–B5, B6): stand; add "B1–B5 never jump to a Part"; add a
    row, hold B*n* → the pages of the Part ring *n* plays.
  - **367** (ORB, EDIT): the jump row is removed; EDIT acts on ORBIT's pages
    only.
  - **370** (ORBIT, MENU): "Leave, to System, as today" → "MENU tap: leave,
    back to where you came from; ORBIT keeps playing. MENU hold: System."
  - **373** (threshold): "500 ms" stands, as the shared `ui::hold::HOLD_MS`
    for MENU, SEQ and hold B*n*; cite ADR 0044.
  - **374** (tap on release): add MENU's tap, and "the browser's save" →
    "a rung's save".
  - **375** (`HoldGate`): it reads the storage spec's latched edges and
    timestamps; one gate for MENU, SEQ and B*n*.
  - **376** (EDIT: jump or sub-page): no jump; EDIT only moves through
    ORBIT's sub-pages, and SCN's commit.
  - **377**: "EDIT + B<n> (the browser)" → hold B*n* is the crossing; "MIX +
    B6 keeps its job (Demo)" stands outside ORBIT only (in ORBIT MIX + B6 is
    unassigned).
  - **378** (leaving): "Holding SEQ returns to the ORBIT page you left"
    stands; add "MENU tap leaves to where you came from; hold B*n* leaves to
    the ring's Part pages".
  - **478** (`hold_gate_*`): the test pins `HOLD_MS`, not a literal 500.
  - **479** (`seq_hold_enters_orbit`): "a tap in the browser saves once" →
    "a tap on a rung saves once; in ORBIT it saves the scene".
  - **480** (`orbit_buttons`): "EDIT on ORB jumps to the target Part's
    chain" → "hold B3 jumps to ring 3's Part pages; a B3 tap never does";
    "MENU opens System" → "tap MENU returns to where you came from; hold
    MENU opens System".
  - **513** (plan step 3's ADR): "the hold threshold" → "per ADR 0044".
  - **563** (buttons): "from a ring you can jump straight to its Part's
    pages" → "by holding its button".
  - **570** (summary): "EDIT jumps to the selected ring's Part" removed;
    "MENU leaves as today" → "MENU tap goes back where you came from; hold
    MENU opens System; hold B*n* jumps to the ring's Part pages; SEQ tap
    saves the scene".
  - **614**: "inside ORBIT it goes to ORB" → "inside ORBIT it does nothing".
- Costs: `Recall` is about a dozen bytes of UI state; the ladder gains one
  variant and two sugar functions. Nothing touches the audio thread.

## Sources
- `docs/superpowers/specs/2026-09-28-projects-storage-design.md` § Navigation, § The keys
- `git show orbit:docs/superpowers/specs/2026-09-30-orbit-design.md` § 7
- Branch `modal2-resonators`: Task 15; ADR 0057 (in progress)
- ADR 0017; `docs/chimera-ui-ux-spec.md` § Controls, § Invariants
- `chimera-core/src/ui/chain.rs:75-127` (today's B*n*, MENU, SEQ, EDIT);
  `chimera-core/src/ui/block_registry.rs` (`PART`, `SENDS`, `MIXER_CHANNEL_CHAIN`)
