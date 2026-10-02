# 0066. MENU opens a SETTINGS menu

- **Status:** Proposed
- **Deciders:** project owner (2026-10-01, "looks right" on the mockups; the rulings below); firmware

## Context
ADR 0044 gave MENU a climbing ladder: Projects, then Project, then the Part
rung. Projects and Project are lists of things to do, not pages to play.
The owner's reference is the Digitakt II (manual OS 1.17, section 14):
[SETTINGS] opens a list, each row dives into a sub-list, and the last level
holds values you turn. The owner wants it obvious that you are in the
settings world of the synth. The spec is
`docs/superpowers/specs/2026-10-01-settings-menu-design.md`.

## Decision
- **The tree is one static table** (`ui::settings::tree`): a `Row` has a
  label, a crumb of at most 8 characters, a `Kind` (List, Leaf chain,
  Screen, Act, or Later with its nonzero GitHub issue). A later row is in the table,
  so shipping it replaces its kind and leaves navigation alone. Paths are row indices, at most 4
  deep. SYSTEM's DEMO row exists in debug builds only; MIX+B6 becomes
  Part 6's mixer.
- **Keys inside SETTINGS:** MENU tap backs one level, and at the top
  closes to where MENU was pressed. EDIT opens the row. PLUS, MINUS and
  encoder 1 move the bar. SEQ runs the highlighted action. B1-B6 leave
  for Part *n*'s sound pages from any depth.
- **Its own visual language:** lists with a highlight bar, a growing
  breadcrumb, a project footer (name and `* MODIFIED`, `SAVED` or `NEW`,
  and a key legend). A leaf page uses the cell grid but keeps the
  breadcrumb and footer, never the chain map.
- **MENU hold is a quick save**, at `HOLD_MS`, from anywhere: over the
  project's own file, or SAVE PROJECT AS for a project with no file.
- **A prompt for every `NeedsConfirm`.** Encoder 1 picks, SEQ confirms,
  MENU cancels.
- **Typed confirmations (#257):** `Confirmed<OverwriteTarget>`,
  `Confirmed<DeleteTarget>` and `SaveTo` carry the proof that the user
  was asked.
- **`project_status` is cached on `rev`**, which `Project`'s `&mut`
  accessors bump; the footer does not recompute the CRC per frame.
- **The card is re-listed after every card operation** (`CardEvent`);
  after a swap, SAVE says CARD CHANGED and offers SAVE PROJECT AS.
- **Mirrors go through `BlockRef::PartMix`:** MIDI CONFIG > CHANNELS and
  AUDIO ROUTING > OUTPUTS edit the same fields as each mixer PART page.
  Their defs take ids 68 and 69.
- **The Projects and Project rungs leave the ladder; the System chain
  goes** (its pages become leaves, in the navigation rewrite). The Part rung and the Sound
  rung stay.

## Alternatives considered
- **ADR 0044's Projects and Project rungs, as first written:** a ladder of
  lists the player climbs one rung at a time, in the same visual language
  as the pages. It hides where you are.
- **A System chain of pages:** settings drawn as chain pages with the map,
  indistinguishable from sound design.
- **MENU hold opening SETTINGS:** puts the common act, saving, behind a
  menu walk, and a tap does the opening already.

## Consequences
- Amends 0044 (Proposed) in place. Supersedes 0057 (through 0044, which
  absorbs it). Supersedes rungs 0 and 1, the `up` table, and the MENU,
  SEQ and MIX+MINUS key rows of the storage spec (2026-09-28).
- Owner rulings of 2026-10-01:
  - **The shared FX pages come after P6 SENDS in the mixer walk.** They
    have no key or SETTINGS row of their own.
  - **RENAME of a project that isn't loaded is dimmed `LOAD TO RENAME`**
    until `Store` gains a streaming copy (#273). That needs a new ADR
    superseding the relevant part of ADR 0045, which is Accepted and
    never amended.
  - **Each engine has a home node.** A new or INIT Modal Part lands on
    RES, not EXC (the navigation rewrite).
  - **Each chain declares its home page** (node 0 unless it says
    otherwise), and every never-visited landing goes there; no code
    outside the chain assumes the first node (owner, 2026-10-02).
- `ChainId::System` and `UiMode::SoundBrowser` go; the browser becomes
  the Sound rung, and saving is SETTINGS > PART.
- The UX spec is the owner's file and is amended by hand.
- `project::guard` takes a UI type, `ui::settings::replace::said::Said`,
  on purpose: only a prompt's confirming pill may confirm a replace (#258),
  and Rust can't scope a constructor to a sibling module, so the token
  lives in an import-free leaf of the UI.
- ADRs 0043, 0044, 0046 and 0066 move to Accepted at the single ship
  flash.

## Sources
- `docs/superpowers/specs/2026-10-01-settings-menu-design.md`;
  `docs/superpowers/plans/2026-10-01-settings-navigation.md`.
- Issues #257, #258, #259, #273.
- ADRs 0044, 0045, 0057.
- Elektron Digitakt II manual, OS 1.17, section 14 (Settings).
- `chimera-core/src/ui/settings/`; `chimera-core/tests/settings_tree_test.rs`.
