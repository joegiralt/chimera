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
  label, a crumb of at most 8 characters, a `Kind` (List, Leaf, Screen,
  Act, or Later with its nonzero GitHub issue). A later row is in the table,
  so shipping it replaces its kind and leaves navigation alone. Paths are row indices, at most 4
  deep. SYSTEM › DIAGNOSTICS' DEMO row exists in debug builds only;
  MIX+B6 becomes Part 6's mixer.
- **A leaf is one page** (owner, 2026-10-02): a `Leaf` holds a
  `OnePage`, whose constructor fails the build on a sub-page, so no key
  inside SETTINGS steps a page and nothing hides behind EDIT or PLUS. A
  screen with more to show is a List of leaves.
- **SYSTEM is OS UPGRADE, STORAGE, FORMAT CARD, USB CONFIG (later),
  DIAGNOSTICS and ABOUT.** ABOUT is one read-only page: firmware
  version and build, chip revision and clock, last reset cause, and the
  card as last seen (its label or serial, or NO CARD), with no card I/O
  of its own. DIAGNOSTICS is AUDIO LOAD (the stats ABOUT's hidden AUD
  page held), TEST TONE (#295) and INPUT TEST (#296), later, and in
  debug builds DEMO, a List with a leaf per storyboard page.
- **Keys inside SETTINGS:** MENU tap backs one level, and at the top
  closes to where MENU was pressed. EDIT opens lists, leaves and screens, and runs actions; SEQ runs
  actions and screen rows (owner, 2026-10-02). PLUS, MINUS and
  encoder 1 move the bar. B1-B6 leave
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
  was asked. Each is made from a `Said`, which only a prompt's confirming
  pill holds: REPLACE, RELOAD and LOAD ANYWAY, and DELETE, CLEAR, SAVE
  OVER and OVERWRITE THAT ONE alike.
- **A confirmed file carries a witness**, the generation and name of its
  newest header as listed; a write or delete refuses a file that moved
  since. Accepted limit: a pair deleted and made again at the same id,
  with the same name and the same generation, slips through (closing it
  takes a format change).
- **`project_status` is cached on `rev`**, which `Project`'s `&mut`
  accessors bump, and hashed only while SETTINGS is open: no frame
  outside it hashes, and SETTINGS' first frame catches up.
- **After every card operation the listing is marked stale**, and
  re-listed while LOAD or MANAGE is on screen. After a swap, SAVE says
  CARD CHANGED and offers SAVE PROJECT AS; the prompt carries any load
  the save was inside, so SAVE AS then loads.
- **The screen beneath a prompt is frozen**, and a prompt over a page
  blanks the band between the header and the map.
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
  - **owner, 2026-10-02: PLUS stays in Part n's mixer and reaches its FX
    after SENDS; other Parts by MIX+B*n*.** The FX have no key or
    SETTINGS row of their own.
  - **RENAME of a project that isn't loaded is dimmed `LOAD TO RENAME`**
    until `Store` gains a streaming copy (#273). That needs a new ADR
    superseding the relevant part of ADR 0045, which is Accepted and
    never amended.
  - **Each engine has a home node.** A new or INIT Modal Part lands on
    RES, not EXC (the navigation rewrite).
  - **Each chain declares its home page** (node 0 unless it says
    otherwise), and every never-visited landing goes there; no code
    outside the chain assumes the first node (owner, 2026-10-02).
  - **CLEAR of the loaded project** loads NEW, then saves it over the
    file; the file's card and witness are checked first, so a moved file
    leaves RAM alone. With edits, its prompt's SAVE THEN LOAD saves them
    to a new file under the proposed name, then clears. The project ends
    `Pristine`.
  - **SAVE PART FIRST on RELOAD saves to a new slot**: over its own slot
    the reload would undo nothing and leave the slot's other Parts Stale.
- `ChainId::System` and `UiMode::SoundBrowser` go; the browser becomes
  the Sound rung, and saving is SETTINGS > PART.
- The UX spec is the owner's file and is amended by hand.
- `project::guard` takes a UI type, `ui::settings::replace::said::Said`,
  on purpose: only a prompt's confirming pill may confirm a replace (#258),
  a delete or an overwrite, and Rust can't scope a constructor to a
  sibling module, so the token lives in an import-free leaf of the UI.
- ADRs 0043, 0044, 0046 and 0066 move to Accepted at the single ship
  flash.

## Sources
- `docs/superpowers/specs/2026-10-01-settings-menu-design.md`;
  `docs/superpowers/plans/2026-10-01-settings-navigation.md`.
- Issues #257, #258, #259, #273.
- ADRs 0044, 0045, 0057.
- Elektron Digitakt II manual, OS 1.17, section 14 (Settings).
- `chimera-core/src/ui/settings/`; `chimera-core/tests/settings_tree_test.rs`.
