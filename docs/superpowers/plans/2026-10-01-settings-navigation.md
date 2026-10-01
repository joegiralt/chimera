# SETTINGS and Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** MENU opens a Digitakt-style SETTINGS menu (a list, sub-lists, values you turn) wired to projects-core, one `Location` replaces `ChainNav`'s `ChainId` and `UiMode::SoundBrowser`, MENU hold quick-saves, and no key press is lost during a card operation. The branch ends at one combined ship flash.

**Architecture:**
- **Functional core** (`chimera-core`, host-tested):
  - `ui::hold`, a pure tap-and-hold gate over latched edges;
  - `ui::nav`, `Location` and `Recall` with one pure `step` per key;
  - `ui::settings::tree`, the SETTINGS tree as one static table;
  - `ui::settings::{view, prompt, naming, listing}`, the list, breadcrumb, footer, prompt and naming screens;
  - `project`, typed overwrite and delete confirmations and a cached `project_status`.
- **Card work** is an intent: input queues a `Job`, and `UiState::card_work` runs it once a frame from the shell, like `sync_system` today.
- **Shells** (`chimera-stm32`, `chimera-desktop`) latch press and release edges with timestamps through `chimera_hal::Latch`, draw BUSY while a job runs, and settle a load's `Swap`.

**Tech Stack:** Rust 2024, `no_std` core, no new dependency; `chimera-hal`'s `testkit` (`MemStore`), `DirStore`, `just`.

**Spec:** `docs/superpowers/specs/2026-10-01-settings-menu-design.md` (owner-approved, binding). What it keeps from `docs/superpowers/specs/2026-09-28-projects-storage-design.md` § Navigation (lines 176–346):
- rung 2, the Part rung: the mixer's PART and SENDS pages, with the B*n* toggle;
- rung 3, the Sound rung: today's pool chooser (the library and tags are plan 3);
- § Naming and tags;
- `HOLD_MS` = 500 ms;
- edge latching.

Also binding:
- ADR 0044 (Proposed; amended here) and ADR 0057 (Proposed; superseded here).
- `docs/superpowers/plans/2026-09-30-projects-core.md`: its `## Deferred to the navigation plan` and its chip checklist (Task 9 and `## Measured`).
- Issues https://github.com/joegiralt/chimera/issues/257 and https://github.com/joegiralt/chimera/issues/258.

---

## Global Constraints

**From the spec**
- `Loc` holds `Pages(PartId, PageAt)`, `Part(PartId, MixPage)`, `Sound(PartId)`, `Fx(PageAt)` and `Settings(SettingsAt)`. `SettingsAt { path: [u8; 4], depth: u8, row: u8 }` keeps a row index per level.
- `Recall` gains `settings_from`: where MENU was pressed, never `Settings`. Backing out of the top of SETTINGS returns there.
- `ChainId::System`, the System chain and `UiMode::SoundBrowser` go. The chain defs stay the page source for `Pages`, `Fx` and SETTINGS leaves.
- **Keys:**
  - **MENU tap:** acts on release, before `HOLD_MS`. Outside SETTINGS it opens the top list. Inside, it backs out one level, and from the top it closes to `settings_from`.
  - **MENU hold:** quick save from anywhere. It saves over the project's own file; NEW opens SAVE PROJECT AS. It fires at `HOLD_MS`, and the release after it does nothing.
  - **EDIT:** sub-page down on pages. In a list, it opens the highlighted list or leaf.
  - **PLUS / MINUS:** sideways on pages. On the mixer, PART → SENDS → the next Part. In a list, the bar moves one row.
  - **Encoder A:** in a list, it moves the bar, and the bar wraps. On pages and leaves, it edits its cell.
  - **SEQ tap:**
    - on pages, sub-page up;
    - in a list, it runs the highlighted action, behind a prompt where the action can lose work;
    - on the mixer and the Sound rung, it opens SETTINGS › PART for that Part.
  - **SEQ hold:** unbound until the ORBIT plan.
  - **B*n*:** from anywhere, SETTINGS included, Part *n*'s sound pages. From those pages, its mixer, and back.
  - **MIX + B*n*:** Part *n*'s mixer. **MIX + MENU** is reserved.
- **The tree is one static table:** rows with a label, a kind (list, leaf page, action) and a status (built, mirror, later). A later row shows dimmed with `LATER` and has a GitHub issue.
- **Lists:**
  - the breadcrumb in the header, e.g. `SETTINGS › PROJECT › LOAD`, with the last part bold;
  - rows of 26–30 px;
  - the bar teal on a soft fill, with a left tick;
  - `›` on a row that opens more;
  - a right-hand note (`● LOADED`, `FILE DAMAGED`, `SLOT 03`);
  - a scrollbar when the list overflows.
- **The footer is on every SETTINGS screen:** the project's name, its status (`* MODIFIED` in the route colour, `SAVED` or `NEW`), and a one-line legend.
- **A leaf page** uses the cell grid, with up to six cells eased like every value. It keeps the breadcrumb and the footer, and never shows the chain map.
- **Prompt:** a teal-outlined panel with the question, the reason in the route colour, and two or three pill options. Encoder A picks, SEQ confirms, and MENU is always CANCEL.
- **NAMING:**
  - A moves the cursor;
  - B cycles A–Z;
  - C cycles 0–9, space and `-`;
  - D toggles case;
  - E deletes;
  - F is inert (tags are plan 3).
  - SEQ saves and MENU cancels. It opens with a proposed name.
- **Global settings** live in SYSTEM (ADR 0045). Per-Part channel and output stay in the project's Part mix. MIDI › CHANNELS and AUDIO ROUTING › OUTPUTS mirror those same fields.
- **`project_status` is cached** on a revision counter that `Project`'s `&mut` accessors bump. Nothing hashes the project per frame: the CRC costs about 1.7 ms on the chip.
- **Controls:**
  - press timestamps and edge latching in both shells' control ticks;
  - taps act on release and holds at `HOLD_MS`;
  - one `HoldGate` type serves MENU and SEQ.

**Repo rules (CLAUDE.md, owner)**
- No `unsafe` without `// SAFETY:`. No heap, no blocking and no allocation on the audio path. No libc.
- Invariants live in types. Pure core, thin shells.
- Every parameter change eases, except after a load (`project_replaced` snaps).
- ADRs record decisions; an accepted ADR is never edited. Follow-ups are GitHub issues (joegiralt/chimera), cited by URL.
- **Commits:**
  - a terse plain sentence, with no type prefix and no Claude or AI attribution;
  - run `git status --short` first;
  - stage files by name, or by the directories a task lists;
  - **never stage `docs/chimera-ui-ux-spec.md` or `chimera.bin`.**
- `just check` passes before every commit, and no task ends red.
- `UiState` besides its `Project` stays within the bound `ui_state_fits_the_ui_reserve` sets. Task 11 raises that bound from 2 KB to 4 KB for the listing; nothing else may grow it past that.
- No stack frame of 8 KB or more (`just stack-check`).
- **Hardware:** desktop QA first, then ONE combined ship flash (Task 15), which also runs projects-core's checklist. Claude does QA (anything measurable); the owner does UAT.

## Review Focus

These five failure modes are the ones the spec implies but no spec test exercises. Each is pinned by a test in the task named.

1. **MENU held across a blocking card operation.** A quick save stalls the UI loop for hundreds of ms, and the release arrives in the same frame as the late hold. Exactly one `Hold` must fire, never a `Tap` as well, and no second save. Task 4: `hold_stalled_through_release_fires_one_hold`. Task 11: `quick_save_stalled_release_saves_once`.
2. **An edit path that doesn't bump the revision, or a frame that does.** The footer would say `SAVED` over unsaved work, or the 1.7 ms CRC would run every frame. Task 2: `every_mutation_bumps_the_revision`, `frames_do_not_bump_the_revision`.
3. **The card is swapped while LOAD PROJECT or MANAGE is on screen.** The listed entries carry the old volume. A load or delete from them is refused with `CARD CHANGED`, and the list re-reads the card in the slot. Task 11: `swap_while_listed_relists_and_refuses`.
4. **A prompt is open when B*n*, MENU hold or a card swap arrives.** The pending replace must be dropped: nothing is replaced, nothing is saved, and the next confirm can't reuse it. Task 9: `bn_drops_an_open_prompt`. Task 11: `menu_hold_inside_a_prompt_does_nothing`.
5. **A delete or overwrite confirmed against a file that changed since it was listed** (saved again, or cleared and re-created). The action is refused with `CHANGED`, and nothing is written. Task 10: `confirmed_delete_after_resave_is_refused`, `confirmed_overwrite_after_resave_is_refused`.

## Pre-flight: where the spec is silent, and what this plan decides

1. **The atlas dependency.** The screen atlas (`chimera-core/tests/screen_atlas_test.rs`, `docs/screens/`) is on `origin/projects-core` at `7a26559`, PR #261, and not yet on `nav-core`. Task 1 merges it first: `origin/main` if #261 has merged, else `origin/projects-core`. Later tasks keep it green, and Task 14 extends it.
2. **How FX pages are reached.** The spec keeps `Fx(PageAt)` but drops the Project rung's `FX ›`. The mixer walk now runs `P1 PART → P1 SENDS → … → P6 SENDS → CHORUS → DELAY → EFX → (TAPE) → MASTER`. PLUS on MASTER stays, and MINUS walks back. This is **owner question 1**.
3. **`Location::Orbit` is not added here.** There is no `OrbitAt` on this branch, so the ORBIT plan adds the variant and `Recall`'s `orbit` and `from`. SEQ hold does nothing until then.
4. **DEMO.** SYSTEM › DEMO exists under `cfg(debug_assertions)` only. With that row in place, MIX+B6 is Part 6's mixer in every build. A release firmware has no demo.
5. **Leaves are small chains.** A leaf row points at a `&'static ChainDef2`:
   - most leaves are one page;
   - ABOUT keeps its AUDIO sub-page (EDIT down, SEQ up);
   - DEMO has five nodes, walked with PLUS and MINUS.

   `SettingsAt` therefore gains `page: PageAt`, which is (0, 0) on lists.
6. **EDIT opens and SEQ runs, strictly.** EDIT on an action row and SEQ on a list or leaf row do nothing, and the legend names the one key that applies. Later rows can be highlighted (that is how you read `LATER`), but EDIT and SEQ ignore them. PLUS and MINUS wrap, like encoder A.
7. **Dynamic lists:**
   - LOAD PROJECT's rows are the listed projects, by id (two digits at least, `03`), then `+ CREATE NEW`; with no card, a single dimmed `NO CARD`.
   - SAVE PROJECT AS is an action (SEQ), since it writes.
   - SAVE TO PROJ is a list of two actions, `OVER SLOT nn` and `TO NEW SLOT nn`, each dimmed when `part_actions` doesn't offer it. RELOAD FROM PROJ is one action (`Revert`), with the note `SLOT nn`.
8. **MANAGE PROJECTS' command column** is UI state (`UiState.manage: Option<u8>`), not part of `Location`. Leaving MANAGE clears it.
9. **RENAME of a project that isn't loaded is dimmed**, with the note `LOAD TO RENAME`. A header rename needs a streaming copy from one side to the other, and `Store` has no read-while-write. This is **owner question 2**. RENAME of the loaded project works as the spec says: in RAM, and it marks the project.
10. **CLEAR of a project that isn't loaded** writes NEW's bytes into that slot's write side, through `encode_new_project`, which streams NEW without a second `Project` in RAM. It is confirmed as an overwrite.
11. **Card events:** the list re-reads the card after every card operation, and whenever LOAD or MANAGE is entered. So no store call needs to return its `CardEvent`. The swap is caught by the volume check (`CARD CHANGED`), and the re-list shows the new card.
12. **The listing holds 48 entries** (`MAX_LISTED`). A card with more ends the list with a dimmed `MORE ON CARD` row. Task 1 files an issue for paging.
13. **Prompt copy** (question / reason / options; MENU is always CANCEL):
    - `LOAD <NAME>?` or `START A NEW PROJECT?` / `<CURRENT> HAS UNSAVED CHANGES` / `SAVE THEN LOAD`, `LOAD ANYWAY`, `CANCEL`;
    - `REPLACE P2 SOUND?` or `CLEAR P2 TO INIT?` / `P2 IS EDITED` / `SAVE PART FIRST`, `REPLACE`, `CANCEL`;
    - `P4 ALSO USES SLOT 03` (`P4 P5 ALSO USE SLOT 03`) / `IT KEEPS THE OLD SOUND` / `UPDATE P4` (`UPDATE ALL`), `LEAVE`;
    - `NAME EXISTS` / `SLOT 05 IS NAMED <NAME>` / `KEEP BOTH`, `OVERWRITE THAT ONE`;
    - `DELETE <NAME>?` / `THIS CANNOT BE UNDONE` / `DELETE`, `CANCEL`;
    - `CLEAR <NAME>?` / `IT BECOMES A NEW PROJECT` / `CLEAR`, `CANCEL`;
    - `SAVE OVER <NAME>?` / `ITS CONTENTS ARE REPLACED` / `SAVE OVER`, `CANCEL`;
    - `CARD CHANGED` / `SAVE AS A NEW PROJECT ON THIS CARD?` / `SAVE AS`, `CANCEL`;
    - `CLEAR SLOT 03?` / `NO PART USES IT` / `CLEAR`, `CANCEL`.

    "The route colour" is `theme::WARN`.
14. **The quick-save edges:**
    - It always writes, even when the project is `Saved`.
    - Inside a prompt or NAMING, MENU hold does nothing.
    - On NEW it moves `Location` to `SETTINGS › PROJECT`, with the bar on SAVE PROJECT AS, and opens NAMING. Cancelling NAMING leaves you on that list.
    - `CARD CHANGED` from a quick save opens the CARD CHANGED prompt, whose SAVE AS runs SAVE PROJECT AS on the card now in the slot.
15. **The prompt is replaced:**
    - SAVE PART FIRST runs the first save action `part_actions` offers (`OverSlot`, else `NewSlot`). With none (pool full), the toast is `POOL FULL` and the prompt stays.
    - SAVE THEN LOAD runs a quick save, or NAMING for a NEW project, inside the pending load. A cancelled or failed save aborts the load.
    - NAME EXISTS compares names ignoring case, and OVERWRITE takes the lowest id with that name.
16. **The proposed name** is `WORDS[id % 8]` + `-` + `id % 1000` as three digits, with `WORDS = ["DUB", "ACID", "DRIFT", "PULSE", "GLASS", "EMBER", "TIDE", "STATIC"]`. So id 42 gives `GLASS-042`.
17. **NAMING edits:**
    - B and C write the character at the cursor. At the end, they append, up to 16 characters.
    - E turned right deletes at the cursor; turned left, it deletes before it.
    - SEQ trims edge spaces. An empty name is refused with the toast `NAME IS EMPTY`.
18. **Glyphs.** The u8g2 `_tr` faces are ASCII only, so `›`, `●` and `◦` are drawn as primitives in `draw::text_tracked`, as `·` is today:
    - `›`: a 3×5 chevron, advance 5;
    - `●`: a filled circle, r = 2, advance 6;
    - `◦`: a hollow circle, r = 2, advance 6.

    A breadcrumb too wide for 216 px drops its leading parts behind `..`.
19. **The PART list header** is a 20 px strip under the breadcrumb:
    - left: `P2 · <SOUND NAME>`;
    - right: the mark, which is `* EDITED · FROM SLOT 03`, `* EDITED · FROM INIT`, `◦ SLOT MOVED` or `CLEAN`.
20. **Mirrors** bind through a new UI-only block, `BlockRef::PartMix(PartId)`: a given Part's `PartParams`. It has no disk code and isn't modulatable. `UiBlocks` resolves it against `project.edit_part(id).mix`. MIDI › CHANNELS is six cells `P1`–`P6` of `CHANNEL`; AUDIO › OUTPUTS is six of `OUTPUT`.
21. **Taps.** A key gives at most one tap per UI frame. Two full taps of one key inside one stalled frame count as one: the latch keeps the last press and release. A press-and-release latched in one frame whose length is `HOLD_MS` or more is a `Hold`, fired late, once.
22. **The Sound rung** is today's browser (the pool, then the INIT rows) as `Loc::Sound`:
    - EDIT loads through the guard and its prompt;
    - SEQ opens SETTINGS › PART;
    - MIX+MINUS clears an unused slot behind a prompt; a used slot gives the toast `SLOT IN USE: P1 P3`;
    - PLUS and MINUS step the Part;
    - MENU opens SETTINGS.

    Its hint line reads `EDIT LOAD · SEQ PART · MIX- CLEAR`.
23. **`HoldGate`'s input differs from the ORBIT plan's** (`git show orbit:docs/superpowers/plans/2026-09-30-orbit.md` Task 6). It reads `Edges` and `now_ms`, not `ButtonState` and elapsed ms. `HOLD_MS` and `Press` keep the ORBIT plan's names, so ORBIT rebases onto this plan and adds its B*n* gates to `HoldGates`.

## Owner questions

1. **FX reach** (Pre-flight 2): are the shared FX pages after Part 6's SENDS in the PLUS walk right, or should they have their own key or SETTINGS row?
2. **RENAME of another project** (Pre-flight 9): is dimming it until `Store` gains a streaming copy (an issue, and an amendment to ADR 0045) acceptable for this ship?

## Issue map

Task 1 fills this table. Task 5's tree reads the numbers from it into `Status::Later(n)`.

| Row or item | Issue |
|---|---|
| SETTINGS › ORBIT (scenes: RENAME, CLEAR, SAVE TO PROJ, RELOAD FROM PROJ) and `Location::Orbit` | |
| SYSEX DUMP | |
| MIDI CONFIG › SYNC values (CLK IN/OUT, TRANS IN/OUT, PRG IN) | |
| MIDI CONFIG › PORT CONFIG values (INPUT FROM, OUTPUT TO, RECEIVE NOTES, RECEIVE CC) | |
| AUDIO ROUTING › OUTPUTS per pair: STEREO/MONO and LEVEL | |
| AUDIO ROUTING › SENDS PRE/POST FADER | https://github.com/joegiralt/chimera/issues/259 (comment) |
| SYSTEM › STORAGE | |
| SYSTEM › FORMAT CARD | |
| SYSTEM › USB CONFIG | |
| PROJECT › MANAGE › PROTECT (a write-protect bit in the project header) | |
| Tags and the library (plan 3), and NAMING's F | |
| PERSONALIZE options beyond THEME | |
| RENAME of a project that isn't loaded (needs a `Store` streaming copy) | |
| Project lists past 48 entries | |

## UX spec amendment (the owner applies it to `docs/chimera-ui-ux-spec.md`)

Never staged by this plan. Each block replaces the text named.

- **§ Guiding Principles 1:**
  > **Everything you play is a chain.** Part sound design, the mixer and the FX are chains of pages. Settings are not: MENU opens a SETTINGS list (§ SETTINGS).
- **§ Guiding Principles 4:**
  > **B1–B6 select context.** B*n* = Part *n*'s sound pages; again = its mixer, and back. MIX + B*n* = Part *n*'s mixer. MENU = SETTINGS.
- **§ Guiding Principles 5:** add
  > SETTINGS is the one exception, a list with its own breadcrumb and footer, so you always know you are in the settings world.
- **§ Hardware Surface, the control table:**

  | Control | Function |
  |---|---|
  | **B1–B6** | Part *n*'s sound pages; from them, its mixer, and back |
  | **MIX + B1–B6** | Part *n*'s mixer (PART, SENDS) |
  | **MENU** | Tap: open SETTINGS, back one level, or close. Hold (500 ms): save the project |
  | **Minus / Plus** | Pages: left and right. Mixer: PART → SENDS → the next Part, then the shared FX. SETTINGS: move the bar |
  | **Seq / Edit** | Pages: sub-page up (on release) / down. SETTINGS: run an action / open a row. Mixer and Sound: SEQ opens SETTINGS › PART |
- **§ System Chain (MENU):** replace the section with
  > ### SETTINGS (MENU)
  > See `docs/superpowers/specs/2026-10-01-settings-menu-design.md` § The tree and § Screens: PROJECT, PART, ORBIT (later), MIDI CONFIG, SYSEX DUMP (later), AUDIO ROUTING, PERSONALIZE, SYSTEM. Lists, a breadcrumb and a project footer; leaves keep the breadcrumb and footer and never show the chain map. ADR 0066.
- **§ Navigation State Machine › State:**
  > `Location` (ADR 0044, as amended by 0066): `Pages(PartId, PageAt)`, `Part(PartId, MixPage)`, `Sound(PartId)`, `Fx(PageAt)`, `Settings(SettingsAt)`.
- **§ Transitions:** replace the B1–B6, MIX + B1–B6, MENU, Seq and Edit rows with the control table above.
- **§ Invariants:**
  > 1. **B1–B6 always reach Part chains.** From anywhere, SETTINGS included.
  > 2. **MIX + B1–B6 always reach a Part's mixer.** From anywhere.
  > 3. **MENU tap always opens or walks SETTINGS; MENU hold always saves.**
  > 5. **One page active at a time.** The only overlays are SETTINGS prompts and toasts.
  > 6. **The dungeon map shows the active chain.** SETTINGS screens show a footer instead.
  > 7. *(removed: B*n* on Part *n* toggles to its mixer, ADR 0044.)*

## File structure

| File | Responsibility |
|---|---|
| `chimera-hal/src/lib.rs` | `Edges`, `Latch`, `ButtonState::from_edges`, and `Controls::{edges, now_ms}` with defaults. |
| `chimera-stm32/src/controls.rs`, `main.rs` | The ISR feeds a `Latch` per button; snapshot takes `Edges`; `handle_input` runs every frame; BUSY, `card_work` and the settle. |
| `chimera-desktop/src/controls.rs`, `main.rs` | The `Latch` fed from minifb's levels and its pressed and released keys; `card_work`. |
| `chimera-core/src/ui/hold.rs` | `HOLD_MS`, `Press`, `HoldGate`, `Presses`, `HoldGates`. |
| `chimera-core/src/ui/nav.rs` | `PageAt`, `MixPage`, `SettingsAt`, `Location`, `Outside`, `Recall`, `NavKey`, `NavCtx`, `Step`. Replaces `ui/chain.rs`'s `ChainId`, `ChainNav`, `next_on_part_button`; `chain_def_for` moves here. |
| `chimera-core/src/ui/settings/tree.rs` | `Row`, `Kind`, `Screen`, `Act`, `Status`, `ROOT`, `row_at`, `rows`; the leaf chains. |
| `chimera-core/src/ui/settings/view.rs` | Breadcrumb, list, PART strip and footer drawing; `legend`. |
| `chimera-core/src/ui/settings/prompt.rs` | `PromptView`, `Choice`, `Answer`. |
| `chimera-core/src/ui/settings/naming.rs` | `Naming`, `proposed_name`. |
| `chimera-core/src/ui/settings/listing.rs` | `Listed`, `Listing`, `MAX_LISTED`. |
| `chimera-core/src/ui/settings/job.rs` | `Job`, `CardCx`, `UiState::{card_pending, card_work}`. |
| `chimera-core/src/ui/settings/mod.rs` | `Ask`, the SETTINGS input handler, and the PROJECT and PART branches. |
| `chimera-core/src/ui/{mod,renderer,region,page,components,draw,browser,block_registry}.rs` | `UiState` on `Location`; the regions `Crumbs`, `List` and `Footer`; the glyphs; the System chain removed; the mirrors. |
| `chimera-core/src/addr.rs`, `project/parts.rs` | `BlockRef::PartMix(PartId)`; `PartRead`. |
| `chimera-core/src/project/{mod,marks,guard,store,codec,note}.rs` | `rev`; `StatusCache`; `OverwriteTarget`, `DeleteTarget`, `FreshFile`, `SaveTo`; `ProjectEntry.generation`; `ProjectNote::Changed`; `encode_new_project`, `clear_project`. |
| `chimera-core/tests/*` | The new tests named in each task; migrations of `part_button_test`, `header_map_test`, `browser_test`, `ui_test`, `all_pages_walk_test`, `screen_atlas_test` and `screen/mod.rs`. |
| `docs/adr/0044-…`, `0057-…`, `0066-settings-menu.md`, `docs/adr/README.md` | The ADRs. |
| `docs/screens/` | Regenerated by `just screens`. |

## Task order

1. Merge the screen atlas; file the follow-up issues.
2. `project_status` cached on a revision counter (#257).
3. Latched press edges and timestamps in the HAL and both shells.
4. `ui::hold`: tap and hold; the test harness taps and holds.
5. The SETTINGS tree as one static table; ADR 0066.
6. `Location`, `Recall` and the key map, pure; ADR 0044 amended, 0057 superseded.
7. SETTINGS screens: breadcrumb, list, footer, leaf regions, glyphs and the `PartMix` mirrors.
8. `UiState` on `Location`: the System chain and the browser mode go.
9. Prompt and NAMING.
10. Typed overwrite and delete confirmations, and NEW streamed (#257).
11. The PROJECT branch: LOAD, SAVE AS, quick save, CARD CHANGED, `card_work` in both shells.
12. MANAGE PROJECTS.
13. The PART branch and the Sound rung; the browser's bypasses go (#257, #258).
14. The atlas walks SETTINGS; goldens; `docs/screens`.
15. Desktop QA, then the single ship flash (STOP for the owner).

Each task depends on the one before it, except: 2, 3 and 5 depend only on 1; 4 on 3; 10 on 2.

---

### Task 1: Merge the screen atlas; file the follow-up issues

**Files:**
- Modify: this plan (§ Issue map)

**Interfaces:**
- Produces: `screen_atlas_test` and `docs/screens` on `nav-core`; the issue numbers in § Issue map.

- [ ] **Step 1: Merge.** Run `gh pr view 261 -R joegiralt/chimera --json state -q .state`. If `MERGED`, run `git fetch origin && git merge --no-edit origin/main`, else `git merge --no-edit origin/projects-core`. Conflicts: none are expected (`7a26559` sits on `14228af`, which `nav-core` has). If any arise, stop and report.
- [ ] **Step 2: Run** `just check` → PASS, and `cargo test -p chimera-core --test screen_atlas_test` → PASS.
- [ ] **Step 3: File the issues.** For each row of § Issue map without a link, first search with `gh issue list -R joegiralt/chimera --search "<keywords>" --state all`. Reuse a match by commenting on it; otherwise run `gh issue create -R joegiralt/chimera`.
  - Each body cites the spec (`docs/superpowers/specs/2026-10-01-settings-menu-design.md` § The tree or § Out of scope) and this plan.
  - The ORBIT issue links #216.
  - USB CONFIG links #203 and #204.
  - Comment on #259: "SETTINGS › AUDIO ROUTING › SENDS shows PRE/POST FADER as LATER until this is decided."
- [ ] **Step 4: Record** each URL in § Issue map.
- [ ] **Step 5: Commit**

```bash
git status --short
git add docs/superpowers/plans/2026-10-01-settings-navigation.md
git commit -m "Issues filed for the SETTINGS rows that come later"
```

(The merge commit is its own commit from Step 1.)

---

### Task 2: `project_status` cached on a revision counter (#257)

**Files:**
- Modify: `chimera-core/src/project/{mod,marks,store,codec,parts}.rs`, `chimera-core/src/ui/mod.rs`
- Test: `chimera-core/tests/project_status_cache_test.rs`

**Interfaces:**
- Produces:

```rust
// project/mod.rs
impl Project { pub fn rev(&self) -> u32; }          // wrapping; bumped by every &mut path
// project/marks.rs
pub struct StatusCache { /* rev: Option<u32>, status: ProjectStatus */ }
impl StatusCache {
    pub const fn new() -> Self;
    pub fn get(&mut self, p: &Project, t: TemplateCrc) -> ProjectStatus; // hashes only when p.rev() moved
}
// project/parts.rs
pub struct PartRead<'a> { pub sound: &'a Sound, pub mix: &'a PartParams, pub fx: &'a FxParams }
impl Blocks for PartRead<'_> { /* block_mut → None */ }
impl Project { pub fn read_part(&self, p: PartId) -> PartRead<'_>; }
// ui/mod.rs
impl UiState { pub fn project_status(&mut self) -> ProjectStatus; } // through its StatusCache
```

- [ ] **Step 1: Write the failing tests** in `project_status_cache_test.rs`:

```rust
#[test] fn every_mutation_bumps_the_revision() {
    // For each: edit_part, edit_fx, set_name, pool_store, pool_clear, apply_part_action,
    // replace_part, and save_project / load_project / boot_project on MemStore:
    // let r = p.rev(); <op>; assert_ne!(p.rev(), r, "<op name>");
}
#[test] fn the_cache_hashes_once_per_revision() {
    // A counting wrapper isn't possible on project_crc, so: the status after
    // edit → get → Modified; mark_saved_for_test → get → Saved; 1000 gets
    // with no mutation return the same value (time < 2 × one get on the host,
    // measured with Instant, release only: #[cfg_attr(debug_assertions, ignore)]).
}
#[test] fn frames_do_not_bump_the_revision() {
    // UiState::new(); let r = ui.project().rev();
    // 100 × (ui.update(); ui.render_with_scope(..)); assert_eq!(ui.project().rev(), r);
}
#[test] fn ui_status_follows_edits_and_saves() {
    // NEW → Pristine; turn a cell → Modified; save to a fresh id on MemStore → Saved.
}
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test project_status_cache_test` → FAIL: `rev` not found.
- [ ] **Step 3: Implement.**
  - `Project` gains a private `rev: u32`. Every `&mut self` method bumps it with `wrapping_add(1)`, and so does every direct field write inside `project::` (the store's save sets `saved_crc`; the decoder and `reset_new`).
  - `UiState`'s read-only paths stop calling `edit_part`: `display_values`, `update` and `page_values`' callers read through `read_part` and `PartRead`. Only real edits go through `edit_part`.
  - `UiState` holds a `StatusCache`.
- [ ] **Step 4: Run** the test → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests
git commit -m "Project status is cached on a revision the edits bump; frames read without bumping it"
```

---

### Task 3: Latched press edges and timestamps in the HAL and both shells

**Files:**
- Modify: `chimera-hal/src/lib.rs`, `chimera-stm32/src/controls.rs`, `chimera-stm32/src/main.rs`, `chimera-desktop/src/controls.rs`, `chimera-desktop/src/main.rs`

**Interfaces:**
- Produces:

```rust
// chimera-hal
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Edges { pub down: bool, pub pressed_at: Option<u32>, pub released_at: Option<u32> } // ms
pub struct Latch { /* down, pressed_at, released_at */ }
impl Latch {
    pub const fn new() -> Self;
    pub fn level(&mut self, down: bool, now_ms: u32); // records a press or release on a change only
    pub fn take(&mut self) -> Edges;                  // the frame's edges; clears them, keeps `down`
}
impl ButtonState { pub const fn from_edges(e: Edges) -> Self; } // a press this frame → Pressed (even if
                                                               // already released); else a release →
                                                               // Released; else Held / Up by `down`
pub trait Controls {
    fn encoder_delta(&self, id: EncoderId) -> i8;
    fn button_state(&self, id: ButtonId) -> ButtonState;
    fn edges(&self, id: ButtonId) -> Edges { /* from button_state at now_ms */ }
    fn now_ms(&self) -> u32 { 0 }
}
// chimera-desktop
impl DesktopControls { pub fn update_events(&mut self, down: &[Key], pressed: &[Key], released: &[Key], now_ms: u32); }
```

- [ ] **Step 1: Write the failing tests.**
  - `chimera-hal` (`#[cfg(test)]` in `lib.rs`):
    - `latch_keeps_a_tap_between_takes`: `level(true, 10)`, `level(false, 40)`, then `take()` gives `Edges { down: false, pressed_at: Some(10), released_at: Some(40) }`, and `from_edges` gives `Pressed`. The next `take()` gives `Edges { down: false, .. None }`, which is `Up`.
    - `latch_held_across_takes`: a press, then two takes. The second is `Held`, with no timestamps.
    - `latch_release_then_press`: down at 0, then `take`, then `level(false, 5)` and `level(true, 9)`. That gives `down: true`, `released_at: Some(5)`, `pressed_at: Some(9)`.
    - `default_edges_follow_button_state`.
  - `chimera-desktop/src/controls.rs`: `a_tap_inside_one_frame_is_a_press`. `update_events(&[], &[Key::Key3], &[Key::Key3], 100)` gives `button_state(B3) == Pressed` and `edges(B3).released_at == Some(100)`.
- [ ] **Step 2: Run** `cargo test -p chimera-hal && cargo test -p chimera-desktop` → FAIL.
- [ ] **Step 3: Implement.**
  - **stm32:** the ISR calls `LATCHES[i].level(BTN_STATE[i], tick * 1000 / CONTROLS_HZ)` on each debounced level. `LATCHES` is a `cortex_m::interrupt::Mutex<RefCell<[Latch; NUM_BUTTONS]>>`.
  - `snapshot` takes all twelve in one `interrupt::free` and stores the `Edges`. `button_state` is `from_edges`, and `edges` and `now_ms` are served from the snapshot.
  - `has_activity` goes, and `main.rs` calls `ui.handle_input` every frame, since a held key must age.
  - **Desktop:** `update` becomes `update_events`, fed from `display.get_keys()`, `get_keys_pressed(KeyRepeat::No)`, `get_keys_released()` and `Instant` ms since start.
- [ ] **Step 4: Run** → PASS; `just check` → PASS (the stm32 builds compile the ISR).
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-hal/src chimera-stm32/src chimera-desktop/src
git commit -m "Controls latch press and release edges with timestamps; a tap inside a stalled frame is kept"
```

---

### Task 4: `ui::hold`: tap and hold; the test harness taps and holds

**Files:**
- Create: `chimera-core/src/ui/hold.rs`, `chimera-core/tests/hold_test.rs`
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod hold;`), `chimera-core/tests/screen/mod.rs`

**Interfaces:**
- Consumes: `Edges`, `Controls::{edges, now_ms}` (Task 3).
- Produces:

```rust
pub const HOLD_MS: u32 = 500;
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Press { Tap, Hold }
pub struct HoldGate { /* down_at: Option<u32>, fired: bool, muted: bool */ }
impl HoldGate {
    pub const fn new() -> Self;
    pub fn step(&mut self, e: Edges, now_ms: u32) -> Option<Press>;
    pub fn mute_this_press(&mut self);   // the press under way yields nothing (MIX + MENU)
}
pub struct Presses { pub menu: Option<Press>, pub seq: Option<Press> }
pub struct HoldGates { /* menu, seq */ }
impl HoldGates { pub const fn new() -> Self; pub fn step(&mut self, c: &impl Controls) -> Presses;
                 pub fn mute(&mut self, b: ButtonId); }
// tests/screen/mod.rs
impl Input { pub fn at(self, ms: u32) -> Self; pub fn release(b: ButtonId) -> Self; }
pub fn tap(ui: &mut UiState, b: ButtonId);    // press, then release, 100 ms apart
pub fn hold(ui: &mut UiState, b: ButtonId);   // press, a frame at HOLD_MS, release
```

- [ ] **Step 1: Write the failing tests** in `hold_test.rs`, with `FRAME_MS = 33`:
  - `tap_is_the_release`: a press at 0, frames to `HOLD_MS - 1`, then a release. That gives `Tap` on the release frame only.
  - `hold_fires_once_at_hold_ms`: a frame at `HOLD_MS` gives `Hold`. Later frames and the release give `None`.
  - `hold_ms_minus_one_frame_is_a_tap`: a release at `HOLD_MS - FRAME_MS` gives `Tap`. A frame at `HOLD_MS + FRAME_MS` while down gives `Hold`.
  - `hold_stalled_through_release_fires_one_hold`: the press at 0, then one frame at 900 with `released_at: Some(850)`. That gives one `Hold` and nothing after.
  - `tap_inside_a_stalled_frame`: `Edges { down: false, pressed_at: Some(1000), released_at: Some(1080) }` at 1400 gives `Tap`.
  - `muted_press_yields_nothing`.
  - `hold_ms_is_500`: `assert_eq!(HOLD_MS, 500)`.

  The gate's order of events is not fixed by the signature. Use this:

```rust
// Events sorted by time; at equal times, a release first when e.down, else the press first.
// Press(t): down_at = Some(t), fired = false, muted = false.
// Release(t): if let Some(d) = down_at, and !fired && !muted:
//     out = Some(if t.wrapping_sub(d) >= HOLD_MS { Hold } else { Tap });
//   then down_at = None.
// Then, if e.down && !fired && !muted && down_at.is_some_and(|d| now.wrapping_sub(d) >= HOLD_MS):
//     fired = true; out = Some(Hold).
```

- [ ] **Step 2: Run** `cargo test -p chimera-core --test hold_test` → FAIL: unresolved `ui::hold`.
- [ ] **Step 3: Implement** `hold.rs` and the harness helpers. `Input` gains `at_ms: u32` and an `edges` override. `Input::press(b)` sets `pressed_at: Some(at_ms)` and `down: true`; `Input::release(b)` sets `released_at`.
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src/ui chimera-core/tests
git commit -m "A tap acts on release and a hold at 500 ms, from latched edges"
```

---

### Task 5: The SETTINGS tree as one static table; ADR 0066

**Files:**
- Create: `chimera-core/src/ui/settings/{mod,tree}.rs`, `chimera-core/tests/settings_tree_test.rs`, `docs/adr/0066-settings-menu.md`
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod settings;`), `chimera-core/src/ui/block_registry.rs` (the leaf chains), `docs/adr/README.md`

**Interfaces:**
- Consumes: § Issue map (Task 1).
- Produces:

```rust
pub struct Row { pub label: &'static str, pub crumb: &'static str, pub kind: Kind, pub status: Status }
pub enum Kind { List(&'static [Row]), Leaf(&'static ChainDef2), Screen(Screen), Act(Act) }
pub enum Screen { LoadProject, ManageProjects, SaveToProj }  // rows built at run time
pub enum Act { SaveProjectAs, PartRename, PartClear, PartReload }
pub enum Status { Built, Mirror, Later(u16) }                // the GitHub issue number
pub static ROOT: Row;                                        // "SETTINGS", List(&TOP)
pub fn row_at(path: &[u8]) -> Option<&'static Row>;          // the row a path names
pub fn rows(path: &[u8]) -> &'static [Row];                  // a List's rows; empty otherwise
// block_registry.rs: one ChainDef2 per leaf
pub static CHANNELS_LEAF, OUTPUTS_LEAF, TUNING_LEAF, THEME_LEAF, UPDATES_LEAF, ABOUT_LEAF: ChainDef2;
```

`SaveToProj` is a `Screen`, since its two rows (`OVER SLOT nn`, `TO NEW SLOT nn`) carry run-time slot numbers. `CHANNELS_LEAF` and `OUTPUTS_LEAF` are declared here with `EMPTY` slots; Task 7 binds them.

The table (labels exact; crumbs in brackets where they differ; Debug = `cfg(debug_assertions)`):

```
PROJECT › LOAD PROJECT [LOAD] Screen(LoadProject) · SAVE PROJECT AS [SAVE AS] Act · MANAGE PROJECTS [MANAGE] Screen(Manage)
PART › RENAME Act · CLEAR Act · SAVE TO PROJ [SAVE TO] Screen(SaveToProj) · RELOAD FROM PROJ [RELOAD] Act
ORBIT Later
MIDI CONFIG [MIDI] › SYNC Later · PORT CONFIG [PORT] Later · CHANNELS Leaf Mirror
SYSEX DUMP [SYSEX] Later
AUDIO ROUTING [AUDIO] › OUTPUTS Leaf Mirror · SENDS Later(259) · TUNING Leaf Mirror
PERSONALIZE › THEME Leaf Mirror
SYSTEM › OS UPGRADE [OS] Leaf(UPDATES) Mirror · STORAGE Later · FORMAT CARD [FORMAT] Later · USB CONFIG [USB] Later
         · ABOUT Leaf(ABOUT + AUDIO sub) Mirror · DEMO Leaf(DEMO_CHAIN) Built, Debug only
```

- [ ] **Step 1: Write the failing tests** in `settings_tree_test.rs`:
  - `every_later_row_names_its_issue`: walk the tree. Every `Later(n)` has `n > 0`, and `n` is one of § Issue map's numbers (a `const ISSUES: &[u16]` in the test, copied from the map).
  - `top_list_is_the_spec_order`: the labels are `PROJECT, PART, ORBIT, MIDI CONFIG, SYSEX DUMP, AUDIO ROUTING, PERSONALIZE, SYSTEM`.
  - `paths_resolve`: `row_at(&[0, 0])` is LOAD PROJECT; `row_at(&[6, 0])` is THEME; `row_at(&[9])` is `None`.
  - `depth_fits_settings_at`: no path is longer than 4.
  - `demo_only_in_debug`: `#[cfg(debug_assertions)]`, SYSTEM's last row is DEMO.
  - `every_crumb_fits`: each crumb is at most 8 characters.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test settings_tree_test` → FAIL.
- [ ] **Step 3: Implement** the table as nested `static [Row]` arrays. The DEMO row uses `#[cfg(debug_assertions)]` on its array element, as `MIXER_CHANNEL_BLOCKS` does for TAPE.
- [ ] **Step 4: Write ADR 0066**, "MENU opens a SETTINGS menu", from the template, Status Proposed.
  - **Context:** the ladder of ADR 0044, the owner's Digitakt reference (Digitakt II manual OS 1.17 § 14), and the 2026-10-01 spec.
  - **Decision:**
    - the tree as one table, with statuses;
    - the keys inside SETTINGS;
    - its own visual language (lists, breadcrumb, footer; leaves keep both, never the map);
    - MENU hold as quick save;
    - prompts for every `NeedsConfirm`;
    - typed `Confirmed<OverwriteTarget>` and `Confirmed<DeleteTarget>` and `SaveTo`;
    - `project_status` cached on `rev`;
    - re-listing after every card operation;
    - CARD CHANGED → SAVE AS;
    - DEMO as a debug-only row, with MIX+B6 becoming Part 6's mixer;
    - mirrors through `BlockRef::PartMix`.
  - **Alternatives:**
    - the Projects/Project rungs (0044 as first written);
    - a System chain of pages;
    - MENU hold opening SETTINGS.
  - **Consequences:** amends 0044, supersedes 0057 (through 0044), and supersedes the storage spec's rungs 0 and 1. Pre-flight 2 and 9 are open owner questions.
  - **Sources:** the spec, this plan, #257, #258.
  - Add its row to `docs/adr/README.md`.
- [ ] **Step 5: Run** → PASS; `just check` → PASS.
- [ ] **Step 6: Commit**

```bash
git status --short
git add chimera-core/src/ui chimera-core/tests docs/adr/0066-settings-menu.md docs/adr/README.md
git commit -m "The SETTINGS tree is one table of rows with a kind and a status; ADR 0066"
```

---

### Task 6: `Location`, `Recall` and the key map, pure; ADR 0044 amended, 0057 superseded

**Files:**
- Create: `chimera-core/src/ui/nav.rs`, `chimera-core/tests/nav_test.rs`
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod nav;`), `docs/adr/0044-one-ladder-one-button-map.md`, `docs/adr/0057-part-button-toggles-sound-and-mixer.md`, `docs/adr/README.md`

**Interfaces:**
- Consumes: `tree::{ROOT, row_at, rows, Kind, Status}` (Task 5); `block_registry::{MIXER_CHANNEL_CHAIN, MIXER_HOME, MIXER_PART}`.
- Produces:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct PageAt { pub node: u8, pub sub: u8 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum MixPage { Part, Sends }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettingsAt { path: [u8; 4], depth: u8, row: u8, page: PageAt }  // page: (0,0) on lists
impl SettingsAt { pub fn path(&self) -> &[u8]; pub fn row(&self) -> u8; pub fn page(&self) -> PageAt;
                  pub fn at_leaf(&self) -> Option<&'static ChainDef2>; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Location(Loc);   // Loc private
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct Outside(Loc);    // never Settings
impl Outside { pub fn new(l: Location) -> Option<Outside>; pub fn get(self) -> Location; }
pub struct Recall { /* pages: [SoundPage; 6], mix: MixPage, settings_from: Outside */ }
pub struct NavCtx { pub engines: [EngineType; 6], pub dyn_rows: u8 } // dyn_rows: a Screen's row count
pub enum NavKey { Part(PartId), MixPart(PartId), EditPart(PartId), Plus, Minus, Edit, SeqTap, MenuTap, Bar(i8) }
pub enum Step { Go(Location), Act(Act), Screen(Screen), Run, Stay }
// Run: SEQ on a Screen row (LOAD's entry, MANAGE's command, SAVE TO's row): UiState decides.
impl Location {
    pub const HOME: Location;                          // Pages(P1, (0,0))
    pub fn pages(p: PartId, at: PageAt) -> Location;
    pub fn mixer(p: PartId, m: MixPage) -> Location;
    pub fn sound(p: PartId) -> Location;
    pub fn settings_at(path: &[u8], row: u8) -> Location;
    pub fn step(self, k: NavKey, cx: &NavCtx, r: &mut Recall) -> Step;
    pub fn page(self, cx: &NavCtx) -> Option<(&'static ChainDef2, PageAt)>; // None on lists and Sound
    pub fn part(self) -> Option<PartId>;
    pub fn settings(self) -> Option<SettingsAt>;
}
impl Recall { pub const fn new() -> Self; pub fn settings_from(&self) -> Location; }
pub fn chain_def_for(e: EngineType) -> &'static ChainDef2;  // moved from chain.rs
```

**Rules:**
- **The mixer walk** (Pre-flight 2): `Part(p, Part)` → `Part(p, Sends)` → `Part(p+1, Part)` … → `Part(P6, Sends)` → `Fx(CHORUS)` … → `Fx(MASTER)`. MINUS is the reverse. Each Part's mixer has two pages, so `MixPage` addresses them, and `Fx`'s `PageAt.node` indexes `MIXER_CHANNEL_CHAIN` from CHORUS (node 2).
- **ADR 0057's memory, kept:**
  - B*n* into the mixer from outside it opens `recall.mix`, except that a remembered PART opens SENDS;
  - mixer to mixer keeps PART;
  - `Pages(n)` restores `recall.pages[n]` only when its engine still matches, else `(0,0)`.
- **MENU tap:**
  - outside SETTINGS: `Settings` at the top list, and `settings_from = Outside(self)`;
  - on a leaf: back to its list, with the row on the leaf;
  - on a list at depth > 0: up one level, with the row on the list just left;
  - at the top: `settings_from`.
- **EDIT** on a list opens `List` and `Leaf` rows (a `Screen` row gives `Step::Screen`). On a `Later` row it gives `Stay`.
- **SEQ tap** on an `Act` row gives `Step::Act`; on a `Screen`'s row, `Step::Run`; on a leaf, sub-page up; on pages, sub-page up.
  - On `Part(p, _)` and `Sound(p)`, SEQ tap goes to `settings_at(&[1], 0)` (PART), with `settings_from` set.
- **`Bar(d)`, PLUS and MINUS** in a list move `row` modulo the row count: the list's static length, or `cx.dyn_rows` on a `Screen`.
- **EDIT on `Part(p, _)`** is `Sound(p)`. **EDIT+B*n*** is `Sound(n)`. **MIX+B*n*** is `Part(n, recall.mix rule)`.
- **PLUS and MINUS on `Sound(p)`** step the Part, and wrap.

- [ ] **Step 1: Write the failing tests** in `nav_test.rs`:
  - `every_settings_path_is_reached_by_edit_and_menu_backs_out`. For each `List` and `Leaf` row in the tree:
    - from `Location::HOME`, `MenuTap`, then `Bar`/`Edit` steps along its path, reaches `settings_at(path)`;
    - then `MenuTap` × (depth + 1) returns to `HOME`.
  - `bn_leaves_from_any_depth`: from every reachable `Settings` location, `Part(P3)` gives `Pages(P3, …)`.
  - `settings_from_is_never_settings`: a proptest-style loop over 10 000 random `NavKey` sequences of length 40, with a seeded xorshift and no new dependency. After each step, `r.settings_from()` is never `Settings`.
  - `part_key_toggles_and_restores` (ADR 0044, migrated from `part_button_test`'s pure cases): `Pages(n)` → mixer → back to the page left; the mixer opens on SENDS, then the last used.
  - `mixer_walk_reaches_the_fx`: PLUS from `Part(P1, Part)` 12 times gives `Part(P6, Sends)`; once more gives `Fx(node 2)`; MINUS from there gives `Part(P6, Sends)`.
  - `seq_on_mixer_and_sound_opens_part_settings`.
  - `later_rows_are_inert`: EDIT and SEQ on ORBIT give `Stay`.
  - `bar_wraps_any_delta_on_any_list`: `Bar(10)`, `Bar(-10)`, and `dyn_rows` of 0 and 1, never panic, and the row stays below the length (a list of 0 rows keeps row 0).
- [ ] **Step 2: Run** `cargo test -p chimera-core --test nav_test` → FAIL.
- [ ] **Step 3: Implement** `nav.rs`. Nothing calls it yet; `UiState` still runs `ChainNav` until Task 8.
- [ ] **Step 4: Amend ADR 0044 in place** (Proposed), each change marked "(amended 2026-10-01, owner: ADR 0066)":
  - `Loc` loses `Projects`, `Project`, `System` and `Orbit`, and gains `Settings(SettingsAt)`. ORBIT returns with the ORBIT plan.
  - `Recall` gains `settings_from`.
  - The `up` table goes; MENU tap opens SETTINGS, backs out, or closes.
  - MENU hold is quick save, ORBIT included (orbit spec lines 370 and 570: "MENU hold: save the project").
  - SEQ tap on the mixer and the Sound rung opens SETTINGS › PART.
  - The mixer walk reaches the FX after Part 6 (pending owner question 1).
  - MIX+B6 is Part 6's mixer once SYSTEM › DEMO exists.
- [ ] **Step 5: ADR 0057:** set its Status to `Superseded by [0044](0044-one-ladder-one-button-map.md)`, and update the README rows for 0057 and 0044.
- [ ] **Step 6: Run** → PASS; `just check` → PASS.
- [ ] **Step 7: Commit**

```bash
git status --short
git add chimera-core/src/ui chimera-core/tests docs/adr/0044-one-ladder-one-button-map.md docs/adr/0057-part-button-toggles-sound-and-mixer.md docs/adr/README.md
git commit -m "One Location for pages, mixer, Sound and SETTINGS, with a pure step per key; ADR 0044 amended for SETTINGS"
```

---

### Task 7: SETTINGS screens: breadcrumb, list, footer, leaf regions, glyphs and the `PartMix` mirrors

**Files:**
- Create: `chimera-core/src/ui/settings/view.rs`, `chimera-core/tests/settings_screen_test.rs`
- Modify:
  - `chimera-core/src/ui/{draw,region,renderer,theme}.rs`;
  - `chimera-core/src/addr.rs`, `project/parts.rs`, `storage/codes.rs` (`BlockRef::PartMix`);
  - `chimera-core/src/ui/block_registry.rs` (bind `CHANNELS_LEAF`, `OUTPUTS_LEAF`);
  - `chimera-core/tests/addr_test.rs`.

**Interfaces:**
- Consumes: `tree` (Task 5), `SettingsAt` (Task 6), `ProjectStatus`.
- Produces:

```rust
// view.rs
pub const LIST_TOP: i32 = 34; pub const ROW_H: i32 = 28; pub const VISIBLE_ROWS: usize = 8;
pub const FOOTER_TOP: i32 = theme::MAP_TOP;           // 266: the footer takes the map's band
pub struct Crumbs { /* up to 5 parts, &'static str or a run-time PART n */ }
pub struct ListRow<'a> { pub label: &'a str, pub opens: bool, pub note: Option<&'a str>,
                         pub look: RowLook }          // RowLook: Normal, Dimmed, Later
pub struct Footer<'a> { pub name: &'a str, pub status: ProjectStatus, pub legend: &'static str }
pub fn draw_crumbs<D: DrawTarget<Color = Rgb565>>(d: &mut D, c: &Crumbs);
pub fn draw_list<D: DrawTarget<Color = Rgb565>>(d: &mut D, rows: &[ListRow<'_>], bar: usize, first: usize);
pub fn draw_footer<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &Footer<'_>);
pub fn legend(on: LegendFor) -> &'static str;
pub fn first_visible(bar: usize, len: usize, prev_first: usize) -> usize;
// region.rs
pub enum RegionKind { Header, Focus, Viz, Cells, Nav, Grid, Crumbs, List, Footer }
pub fn settings_regions(leaf: Option<PageLayout>) -> &'static [(RegionKind, u16, u16)];
// addr.rs
BlockRef::PartMix(PartId)   // UI-only: no disk code, voice_reads() false, specs PART_SPECS
```

Legend copy (exact):

| `LegendFor` | Text |
|---|---|
| list row that opens | `EDIT OPEN · MENU BACK` |
| action row | `SEQ RUN · MENU BACK` |
| later row | `LATER · MENU BACK` |
| at the top list | the same, with `MENU CLOSE` for `MENU BACK` |
| leaf | `A-F EDIT · MENU BACK` |
| prompt | `A PICK · SEQ OK · MENU CANCEL` |
| NAMING | `SEQ SAVE · MENU CANCEL` |
| MANAGE list | `EDIT COMMANDS · MENU BACK` |
| MANAGE commands | `SEQ RUN · MENU LIST` |

Status copy: `Pristine` → `NEW`, `Saved` → `SAVED`, `Modified` → `* MODIFIED` in `theme::WARN`.

- [ ] **Step 1: Write the failing tests** in `settings_screen_test.rs`:
  - `settings_leaf_never_draws_the_map`: `settings_regions(Some(CellGrid))` holds `Crumbs` and `Footer`, and neither `Header` nor `Nav`. The leaf's `Cells` band ends at 266.
  - `footer_band_is_the_footer`: draw a footer alone into an `Fb`, and render a SETTINGS leaf (via `settings_regions`). Rows 266–320 are pixel-equal.
  - `every_legend_fits`: `draw::text_width` of each legend is at most 216.
  - `breadcrumb_drops_leading_parts_behind_dots`: `SETTINGS › AUDIO ROUTING › OUTPUTS` with a forced 120 px width starts with `..`.
  - `glyphs_have_width`: `text_width("›")` is 5, and `text_width("●")` and `text_width("◦")` are 6.
  - `first_visible_keeps_the_bar_on_screen`: for every `bar` in 0..40 and `len` in 0..40, the result keeps the bar in `[first, first + VISIBLE_ROWS)`.
  - `part_mix_edits_the_named_part` (`addr_test`): `ParamAddr::new(BlockRef::PartMix(P3), PartParams::CHANNEL)` through `UiBlocks` sets Part 3's channel, and Part 1's is unchanged.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test settings_screen_test --test addr_test` → FAIL.
- [ ] **Step 3: Implement** the drawing in Direction A (ADR 0016) with the theme's tokens:
  - the bar: `ACCENT_SOFT` fill, a 2 px `ACCENT` tick at x = 4;
  - the scrollbar at x = 236, as `browser.rs` draws one;
  - the glyphs in `draw::text_tracked`, beside `MIDDOT`;
  - `BlockRef::PartMix` added to the `match`es the compiler names, and kept out of `BlockRef::ALL` (it isn't a Sound or Part block, so codec loops never see it). `codes.rs` gives it no disk code.
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests
git commit -m "SETTINGS screens draw a breadcrumb, a list and a project footer; leaves keep both and never the map"
```

---

### Task 8: `UiState` on `Location`: the System chain and the browser mode go

**Files:**
- Modify:
  - `chimera-core/src/ui/{mod,chain,renderer,page,components,region,browser,block_registry}.rs`;
  - `chimera-core/src/ui/settings/mod.rs`;
  - `chimera-desktop/src/main.rs`, `chimera-stm32/src/main.rs`;
  - the tests the compiler names: `part_button_test`, `header_map_test`, `browser_test`, `ui_test`, `all_pages_walk_test`, `binding_test`, `preset_test`, `toast_test`, `project_boot_test`, `screen_atlas_test`, `screen/mod.rs`.
- Test: `chimera-core/tests/ui_nav_test.rs`

**Interfaces:**
- Consumes: Tasks 4, 6 and 7.
- Produces:

```rust
impl UiState {
    pub fn location(&self) -> Location;           // replaces `pub nav`
    pub fn handle_input(&mut self, c: &impl Controls);
    pub fn in_settings(&self) -> bool;            // replaces in_system; sync_system uses it
}
// UiState fields: loc: Location, recall: Recall, gates: HoldGates, list_first: u8
// (ChainNav, ChainId, UiMode and ALL_CHAINS' SYSTEM_CHAIN go; chain.rs keeps nothing but re-exports, or goes.)
```

**Rules:**
- `handle_input` order:
  1. dismiss the toast and prime status on any input;
  2. step the `HoldGates`. With MIX down at a MENU press, `mute(Menu)`.
  3. build the `NavKey`s (B*n*, MIX+B*n*, EDIT+B*n*, EDIT, PLUS, MINUS, SEQ `Tap`, MENU `Tap`, and encoder A as `Bar` on a list);
  4. `Location::step` each, then apply the `Step`;
  5. apply encoder deltas when the location has a page.
- MENU `Hold` and SEQ `Hold` do nothing until Task 11.
- `Step::Act`, `Step::Screen` and `Step::Run` show the toast `NOT YET` until Tasks 11–13 wire them, so the desktop is usable between tasks.
- `Loc::Sound` draws `browser::draw` (now `Location`-keyed). Its SEQ save goes now; Task 13 adds the prompt.
- Going to a Part's `Pages` or mixer sets `active_part`, the engine, the matrix and the page, as `nav_changed` does today. One private `go(&mut self, to: Location)` does it.
- **Leaves render through the page renderer** with `settings_regions(Some(layout))`, and lists with `settings_regions(None)`.
  - The SETTINGS footer reads `self.project_status()` (Task 2) and the `legend`.
  - A leaf's header is the breadcrumb, never `header_text`.
- The screen atlas's `Ctx::System` becomes `Ctx::Settings`: it walks each leaf by its path. `Ctx::Demo` walks SYSTEM › DEMO under debug. Its file names stay `system_*` and `demo_*`, so the pictures diff cleanly.

- [ ] **Step 1: Write the failing tests** in `ui_nav_test.rs`, through `screen::{tap, hold, feed}`:
  - `menu_tap_opens_settings_and_backs_out`: from FILTER on Part 2, a MENU tap gives the top list, and a MENU tap gives FILTER on Part 2 again.
  - `menu_press_without_release_does_nothing`.
  - `seq_tap_is_sub_page_up_on_release`.
  - `bn_from_a_settings_leaf_lands_on_part_pages_and_syncs_system`: on THEME, turn BRIGHT, then B2. `in_settings()` is false, and `sync_system` on `MemStore` saved the theme.
  - `theme_leaf_edits_ease`: after one detent, the renderer's anim value is not yet at the target (`update()` once).
  - `channels_mirror_edits_the_mixer_value`: on MIDI › CHANNELS, C +1 (Part 3) gives Part 3's mixer PART page showing CHANNEL 4.
  - `mix_b6_is_part_6_mixer`.
  - `edit_on_mixer_opens_sound_and_seq_opens_part_settings`.
  - `no_press_lost_in_a_stalled_frame`: one `Input` carrying B3's press and release gives `Pages(P3)`; one carrying MENU's gives the top list.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test ui_nav_test` → FAIL.
- [ ] **Step 3: Implement.** Migrate the named tests: SEQ and MENU presses become `tap`, `ChainId` asserts become `Location` asserts, and `UiMode::SoundBrowser` becomes `Location::sound`. Delete `SYSTEM_CHAIN` and `ChainId`.
- [ ] **Step 4: Run** `just check` → PASS. `cargo test -p chimera-core --test screen_atlas_test` → PASS; regenerate with `just screens` and eyeball the diff. Only the System and Demo pages change, and only to breadcrumb and footer. Run `just desktop`: MENU, the leaves and B*n* work by hand.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-desktop/src chimera-stm32/src docs/screens
git commit -m "The UI runs on one Location: MENU opens SETTINGS, the System chain and the browser mode go"
```

---

### Task 9: Prompt and NAMING

**Files:**
- Create: `chimera-core/src/ui/settings/{prompt,naming}.rs`, `chimera-core/tests/prompt_naming_test.rs`
- Modify: `chimera-core/src/ui/settings/mod.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/src/ui/settings/view.rs`

**Interfaces:**
- Consumes: `view` (Task 7), `ProjectName`, `SoundName`.
- Produces:

```rust
// prompt.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Answer { Pick(u8), Cancel }
pub struct PromptView<'a> { pub question: &'a str, pub reason: &'a str, pub options: &'a [&'a str] } // 2 or 3
pub struct Choice { pick: u8, n: u8 }
impl Choice { pub fn new(n: u8) -> Self;   // pick 0
              pub fn input(&mut self, c: &impl Controls, p: &Presses) -> Option<Answer>; } // A picks (clamped,
              // no wrap), SEQ tap → Pick, MENU tap → Cancel
pub fn draw_prompt<D: DrawTarget<Color = Rgb565>>(d: &mut D, v: &PromptView<'_>, pick: u8);
// naming.rs
pub struct Naming { /* buf: [u8; 16], len: u8, cursor: u8 */ }
pub enum NamingOut { Save(Name<16>), Cancel, Empty }
impl Naming {
    pub fn new(start: &str) -> Self;
    pub fn input(&mut self, c: &impl Controls, p: &Presses) -> Option<NamingOut>;
    pub fn text(&self) -> &str; pub fn cursor(&self) -> u8;
}
pub fn proposed_name(id: ProjectId) -> ProjectName;  // Pre-flight 16
pub fn draw_naming<D: DrawTarget<Color = Rgb565>>(d: &mut D, n: &Naming, title: &str);
// settings/mod.rs
pub(crate) enum Ask { /* one variant per prompt in Pre-flight 13; Tasks 11–13 add theirs */ }
pub(crate) enum NamingFor { RenameLoaded, RenamePart(PartId) }  // Task 11 adds SaveAs(FreshFile),
                                                               // SaveAsThenLoad(FreshFile, Pending<ProjectSource>)
// UiState: prompt: Option<(Ask, Choice)>, naming: Option<(NamingFor, Naming)>
```

**Rules:**
- While a prompt or NAMING is open it takes every input, except B*n* and MIX+B*n*. Those drop it (Answer `Cancel`), then act.
- MENU hold is ignored there.
- A prompt draws over the screen beneath, as a `List`-band overlay (y 60–240), keyed in the region set. On close, the region set is invalidated, so the screen beneath redraws.

- [ ] **Step 1: Write the failing tests** in `prompt_naming_test.rs`:
  - `prompt_picks_with_a_and_clamps`: with three options, A +5 picks 2, and A −9 picks 0.
  - `seq_confirms_and_menu_cancels_on_release`.
  - `bn_drops_an_open_prompt`: open a test `Ask` that holds a `Pending<PartSource>`. B3 gives `Pages(P3)`, `ui.prompt_open()` is false, and the Part is unchanged.
  - `naming_edits_per_encoder`: start `DUB-042`. A −10 puts the cursor at 0; B +1 gives `EUB-042`; D gives `eUB-042`; E +1 gives `UB-042`; C at the end appends `0`.
  - `naming_trims_and_refuses_empty`: `"  AB "` saves `AB`; all spaces gives `Empty`.
  - `naming_caps_at_16`.
  - `proposed_names`: 42 gives `GLASS-042`, and 1 gives `ACID-001`.
  - `every_prompt_fits`: each Pre-flight 13 string, with a 16-character name, fits the panel's 200 px.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test prompt_naming_test` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests
git commit -m "A prompt panel and the naming screen; B buttons cancel either and leave"
```

---

### Task 10: Typed overwrite and delete confirmations, and NEW streamed (#257)

**Files:**
- Modify: `chimera-core/src/project/{guard,store,codec,note,mod}.rs`, `chimera-core/src/project/test_support.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/tests/{project_store_test,project_boot_test,project_model_test,replace_guard_test,project_marks_test}.rs`, `chimera-desktop/src/store.rs`
- Test: `chimera-core/tests/project_confirm_test.rs`

**Interfaces:**
- Consumes: `ProjectEntry`, `save_project`, `delete_project`, `Confirmed`.
- Produces:

```rust
// guard.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct OverwriteTarget { file: ProjectFile }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct DeleteTarget { file: ProjectFile }
impl Confirmed<OverwriteTarget> { pub(crate) fn answered(e: &ProjectEntry) -> Self; } // witness = e.generation
impl Confirmed<DeleteTarget>    { pub(crate) fn answered(e: &ProjectEntry) -> Self; }
// store.rs
pub struct ProjectEntry { pub id, pub vol, pub name, pub err, pub generation: Option<Generation> }
#[derive(Debug)] pub struct FreshFile(ProjectFile);          // only new_project_id makes one
pub fn new_project_id<S: Store>(card: &mut Card, store: &mut S) -> Result<FreshFile, ProjectNote>;
pub enum SaveTo { Own, Fresh(FreshFile), Over(Confirmed<OverwriteTarget>) }
pub fn save_project<S: Store>(card: &mut Card, store: &mut S, p: &mut Project, to: SaveTo) -> ProjectNote;
pub fn delete_project<S: Store>(card: &mut Card, store: &mut S, loaded: &Project, c: Confirmed<DeleteTarget>) -> Result<(), ProjectNote>;
pub fn clear_project<S: Store>(card: &mut Card, store: &mut S, loaded: &Project, c: Confirmed<OverwriteTarget>) -> Result<(), ProjectNote>;
// codec.rs
pub fn encode_new_project(w: &mut RecordWriter<'_>) -> Result<(), StoreError>; // NEW's records, no Project in RAM
// note.rs
ProjectNote::Changed(Subject)   // "CHANGED SINCE ASKED: <name>"
ProjectNote::NoFile             // SaveTo::Own on a project with no file: "NOT SAVED YET"
// mod.rs
pub(crate) fn save_part_to(..)  // was pub (#257)
// test_support.rs
pub fn confirm_overwrite(e: &ProjectEntry) -> Confirmed<OverwriteTarget>; pub fn confirm_delete(e: &ProjectEntry) -> Confirmed<DeleteTarget>;
```

**Rules:**
- `Over` and `Delete` re-peek the newest header on the card first. If the generation differs from the witness, they return `Changed` and write nothing. A missing pair is `Changed` too.
- `Over` and `clear_project` of the loaded project's own file are allowed; they are a SAVE.
- `delete_project` of the loaded file stays `IsLoaded`.
- `clear_project` on another file writes `encode_new_project`'s bytes, named `NEW PROJECT`, to the write side.
- `UiState::{save_project, delete_project}` take `SaveTo` and `Confirmed<DeleteTarget>`. The compile-fail doc test now passes a `ProjectFile` where `SaveTo` is wanted (E0308).

- [ ] **Step 1: Write the failing tests** in `project_confirm_test.rs`, on `MemStore`:
  - `encode_new_matches_project_new`: the `encode_new_project` bytes equal `encode_project(&Project::new())`'s, and `project_crc` matches `TemplateCrc`.
  - `confirmed_delete_after_resave_is_refused`: list, confirm, re-save that id. The delete gives `Changed`, and the file is still there.
  - `confirmed_overwrite_after_resave_is_refused`.
  - `overwrite_saves_over_and_becomes_that_file`: the project's `meta().file()` is the target, and its status is `Saved`.
  - `clear_other_makes_it_new`: loading it after is `Pristine` (by its CRC), named `NEW PROJECT`.
  - `fresh_file_only_from_new_project_id`: a `compile_fail,E0451` doc test on `FreshFile(..)`.
  - `save_own_without_a_file_is_no_file`.
  - `save_part_to_is_not_public`: a `compile_fail,E0624` doc test.

  Existing tests that called `save_part_to` move to `apply_part_action`, or to `test_support`'s re-export under `test-support`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test project_confirm_test` → FAIL.
- [ ] **Step 3: Implement.** `project_store_suite` (desktop `DirStore` too) gains the delete and overwrite cases.
- [ ] **Step 4: Run** → PASS; `just check` → PASS (the `stack-check` covers `encode_new_project`).
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-desktop/src
git commit -m "Overwrite and delete need a typed confirmation that a changed file refuses; NEW streams without a second project"
```

---

### Task 11: The PROJECT branch: LOAD, SAVE AS, quick save, CARD CHANGED, `card_work` in both shells

**Files:**
- Create: `chimera-core/src/ui/settings/{listing,job}.rs`, `chimera-core/tests/settings_project_test.rs`
- Modify: `chimera-core/src/ui/settings/mod.rs`, `chimera-core/src/ui/mod.rs`, `chimera-core/tests/memory_budget_test.rs` (the bound to 4 KB, with a comment naming the listing), `chimera-stm32/src/main.rs`, `chimera-desktop/src/main.rs`

**Interfaces:**
- Consumes: Tasks 2, 9 and 10; `list_projects`, `ReplaceGuard`, `UiState::load_project`.
- Produces:

```rust
// listing.rs
pub const MAX_LISTED: usize = 48;
pub struct Listed { pub id: ProjectId, pub name: Option<ProjectName>, pub err: Option<FileError>, pub generation: Option<Generation> }
pub struct Listing { /* vol: Option<VolumeId>, items: [Option<Listed>; MAX_LISTED], more: bool, no_card: bool, stale: bool */ }
impl Listing { pub fn entry(&self, i: usize) -> Option<ProjectEntry>; pub fn len(&self) -> usize; pub fn mark_stale(&mut self);
               pub fn named(&self, n: &ProjectName) -> Option<ProjectEntry>; } // ignoring case, lowest id
// job.rs
pub(crate) enum Job { List, QuickSave, Fresh(Option<Pending<ProjectSource>>) /* Some: SAVE THEN LOAD */, Save(SaveTo), Load(Confirmed<ProjectSource>),
                      Delete(Confirmed<DeleteTarget>), Clear(Confirmed<OverwriteTarget>) }
pub struct CardCx<'a, S: Store> { pub card: &'a mut Card, pub store: &'a mut S,
                                  pub sync: &'a mut SystemSync, pub settings: &'a mut SystemSettings }
impl UiState {
    pub fn card_pending(&self) -> bool;
    pub fn card_work<S: Store, R>(&mut self, cx: CardCx<'_, S>, link: &LoadLink,
                                  publish: impl FnOnce(Swap, &Project) -> R) -> Option<R>;
}
```

**Rules:**
- Every job but `List` marks the listing stale. `card_work` runs one job, then a `List` whenever the listing is stale and LOAD or MANAGE is on screen. So it re-lists after every operation, and on entering either screen.
- **LOAD PROJECT:** `Step::Run` on an entry runs `ReplaceGuard::check(ProjectSource::File { id, vol })`.
  - `Ok` queues `Load`.
  - `NeedsConfirm` opens `Ask::LoadProject`: SAVE THEN LOAD, LOAD ANYWAY or CANCEL (Pre-flight 15).
  - `+ CREATE NEW` does the same with `ProjectSource::New`.
  - An errored entry is dimmed, and SEQ on it shows its note.
- **SAVE PROJECT AS:**
  1. `Job::Fresh` → `new_project_id` → NAMING with `proposed_name`;
  2. SEQ in NAMING checks `listing.named`;
  3. a match opens `Ask::NameExists`: KEEP BOTH → `Save(Fresh)`; OVERWRITE → `Save(Over(Confirmed::answered(entry)))`;
  4. else `Save(Fresh)`. The saved project's name is the NAMING result (`set_name` first).
- **Quick save (MENU `Hold`)** is `Job::QuickSave`, which runs `SaveTo::Own`:
  - no file (NEW) moves to `settings_at(&[0], 1)` and runs SAVE PROJECT AS;
  - a `Card { err: VolumeChanged(_) }` note opens `Ask::CardChanged`, whose SAVE AS runs SAVE PROJECT AS.
- **Shells:**
  - before `card_work`, if `ui.card_pending()`, draw `busy::draw_busy` and flush its band;
  - **stm32 publish closure:** `swap.settle(&LOAD_LINK, || controls::ticks().wrapping_sub(t0) < 5)` (10 ms at 500 Hz), then `shared_w.publish(..)`;
  - **desktop:** `swap.settle` with an `Instant` deadline, then `audio.update`;
  - `card_work` runs right after `handle_input`, before `sync_system`.

- [ ] **Step 1: Write the failing tests** in `settings_project_test.rs`. Each drives `UiState` with `screen::{tap, hold, feed}` and calls `card_work` on `MemStore`, with a publish closure that records the swap.
  - `load_lists_marks_the_loaded_and_greys_the_damaged`: save two projects and damage one side pair. The rows show `● LOADED` on the loaded one and `FILE DAMAGED` on the damaged one.
  - `no_card_lists_only_no_card`.
  - `load_over_modified_asks_and_each_answer`:
    - LOAD ANYWAY loads, and the status is `Saved`;
    - CANCEL leaves the project bit-identical;
    - SAVE THEN LOAD saves the old project to its file, then loads.
  - `save_then_load_on_new_names_first_and_cancel_aborts`.
  - `save_as_names_and_saves`: SEQ on SAVE PROJECT AS, then SEQ in NAMING. The listing shows `ACID-001`, and the footer reads `SAVED`.
  - `name_exists_keep_both_and_overwrite`: two files after KEEP BOTH. After OVERWRITE, still one, and it holds the new content.
  - `quick_save_saves_over_own_file`: the toast is `SAVED`, and `Location` is unchanged.
  - `quick_save_on_new_opens_save_as` (the spec's property: MENU hold moves `Location` only here).
  - `quick_save_stalled_release_saves_once`: the MENU press at 0, then one frame at 900 with the release latched. That is one save (the file's generation moved by one) and no SETTINGS open.
  - `menu_hold_inside_a_prompt_does_nothing`.
  - `card_changed_then_save_as`: save on card A, then swap the `MemStore`'s volume. A quick save shows CARD CHANGED, and SAVE AS saves on the new card.
  - `swap_while_listed_relists_and_refuses`: on LOAD, swap the card, SEQ an old entry. The note is `CARD CHANGED`, the project is untouched, and the next frame's listing shows the new card's projects.
  - `relist_after_every_operation`: after a save, the new project appears without leaving the screen.
- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test settings_project_test` → FAIL.
- [ ] **Step 3: Implement**, including both shells.
- [ ] **Step 4: Run** → PASS; `just check` → PASS (`ui_state_fits_the_ui_reserve` passes at ≤ 4 KB). Then `CHIMERA_CARD=$(mktemp -d) just desktop`: save as, quick save, load, relaunch, by hand.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests chimera-stm32/src chimera-desktop/src
git commit -m "SETTINGS loads, saves as and quick-saves projects; a card swap offers SAVE AS"
```

---

### Task 12: MANAGE PROJECTS

**Files:**
- Modify: `chimera-core/src/ui/settings/{mod,view}.rs`, `chimera-core/src/ui/mod.rs`
- Test: `chimera-core/tests/settings_manage_test.rs`

**Interfaces:**
- Consumes: Tasks 10 and 11.
- Produces: `view::draw_manage(d, rows: &[ListRow], bar, first, cmds: &[ListRow], cmd: Option<u8>)`, with the list at x 0–140 and the commands at x 144–240. Also the `Ask::{Delete, Clear, SaveOver}` variants.

**Rules:**
- **Commands:** `LOAD FROM · SAVE TO · RENAME · CLEAR · DELETE · PROTECT`.
  - DELETE is dimmed on the loaded project;
  - RENAME on another project is dimmed with `LOAD TO RENAME` (Pre-flight 9);
  - PROTECT is `LATER`.
- **On the left list**, EDIT moves into the commands (`manage = Some(0)`), and encoder A moves the focused column. In the commands, SEQ runs one, and MENU returns to the list.
- **What each command does:**
  - LOAD FROM is LOAD's path.
  - SAVE TO opens `Ask::SaveOver` → `Save(Over)`.
  - RENAME of the loaded project opens NAMING, then `set_name` (it marks the project).
  - CLEAR opens `Ask::Clear`. On the loaded project's own slot, that is the guarded `ProjectSource::New` (through its prompt when Modified), then `Save(Over(own entry))`. On another slot, it is `Job::Clear`.
  - DELETE opens `Ask::Delete`, then `Job::Delete`.

- [ ] **Step 1: Write the failing tests** in `settings_manage_test.rs`, on `MemStore`:
  - `delete_is_dimmed_on_the_loaded_project`.
  - `delete_another_after_confirm`: it is gone after DELETE; CANCEL keeps it.
  - `delete_the_project_system_names_clears_last_project`.
  - `save_to_overwrites_after_confirm`.
  - `clear_other_and_clear_own`: the other loads as NEW. Clearing your own leaves RAM NEW, saved over its own file, and `Saved`.
  - `rename_loaded_marks_modified`.
  - `menu_from_commands_returns_to_the_list`.
- [ ] **Step 2: Run** `cargo test -p chimera-core --features chimera-hal/testkit --test settings_manage_test` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests
git commit -m "MANAGE PROJECTS loads from, saves to, renames, clears and deletes, each behind its prompt"
```

---

### Task 13: The PART branch and the Sound rung; the browser's bypasses go (#257, #258)

**Files:**
- Modify: `chimera-core/src/ui/settings/{mod,view}.rs`, `chimera-core/src/ui/{mod,browser}.rs`, `chimera-core/tests/browser_test.rs`
- Test: `chimera-core/tests/settings_part_test.rs`

**Interfaces:**
- Consumes: `part_actions`, `apply_part_action`, `PartFrom::Init`, `ReplaceGuard` (projects-core); Tasks 9 and 11.
- Produces: `view::draw_part_strip(d, part: PartId, name: &str, mark: PartMark)`, the mark per Pre-flight 19. Also the `Ask::{ReplacePart, UpdateStale, ClearSlot}` variants.

**Rules:**
- **The PART rows act on `active_part`.**
  - SAVE TO PROJ's rows come from `part_actions` (`OverSlot` → `OVER SLOT nn`, `NewSlot` → `TO NEW SLOT nn`). A missing one is dimmed. RELOAD FROM PROJ is `Revert`, dimmed when not offered.
  - A save-over whose `PartSet` isn't empty opens `Ask::UpdateStale`. UPDATE applies each listed Part's `Revert` from a fresh `part_actions`; LEAVE does nothing.
- **CLEAR** checks `PartSource { part, from: PartFrom::Init(engine) }` → `Ask::ReplacePart` (Pre-flight 15) → `replace_part`.
- **RENAME** opens NAMING with the Sound's name, then sets `edit_part(p).sound.name`.
- **On the Sound rung** (Pre-flight 22), EDIT goes through the same `Ask::ReplacePart`. No call to `.anyway(` remains outside the prompt's REPLACE answer and tests (#258).

- [ ] **Step 1: Write the failing tests** in `settings_part_test.rs`:
  - `part_strip_marks`: Clean, Edited from a slot, Edited from INIT, and Stale give their strings.
  - `save_over_then_update_stale`: P1 and P4 on slot 3. Edit P1, then OVER SLOT 03 gives the prompt `P4 ALSO USES SLOT 03`. UPDATE leaves P4 `Clean` with the new sound; LEAVE leaves it `Stale`.
  - `stale_part_offers_no_over_slot`: OVER SLOT is dimmed, and SEQ on it does nothing.
  - `clear_edited_part_asks_and_each_answer`: REPLACE gives INIT; CANCEL leaves it unchanged; SAVE PART FIRST saves to a new slot, then clears.
  - `rename_marks_the_part_edited`.
  - `sound_rung_load_asks_when_edited` (#258).
  - `sound_rung_seq_opens_part_settings`.
  - `sound_rung_mix_minus_clears_an_unused_slot_after_confirm`, and a used slot gives the toast `SLOT IN USE: P1`.
  - `no_anyway_outside_the_prompt`: a test that greps `chimera-core/src` with `include_str!` over the UI files and asserts that `.anyway(` appears only in `settings/mod.rs`'s REPLACE arm.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test settings_part_test --test browser_test` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/src chimera-core/tests
git commit -m "SETTINGS › PART saves, reloads, clears and renames from the Part's actions; the browser asks before it replaces"
```

The PR that carries this branch says "Closes #257" and "Closes #258".

---

### Task 14: The atlas walks SETTINGS; goldens; `docs/screens`

**Files:**
- Modify: `chimera-core/tests/screen_atlas_test.rs`, `chimera-core/tests/screen/mod.rs`, `chimera-core/tests/screen_golden_test.rs`, `docs/screens/` (generated), `docs/screens/README.md` (generated)

**Interfaces:**
- Consumes: everything above.
- Produces: `settings_*.png` for every built list, leaf, prompt, NAMING and MANAGE, and the footer in each status. Pinned goldens: `settings_top`, `settings_load`, `settings_prompt_load`, `settings_naming`.

- [ ] **Step 1: Write the failing tests.**
  - `screen_atlas_test` gains `Ctx::Settings`. It walks `tree::ROOT` depth first by real key presses (MENU tap, then `Bar`/EDIT), and captures:
    - each list and leaf;
    - LOAD with three projects on a `MemStore` and with NO CARD;
    - MANAGE in both columns;
    - each `Ask` variant;
    - NAMING;
    - the footer as `NEW`, `SAVED` and `* MODIFIED`.

    The name is `settings_<crumbs joined by _>`, lowercased.
  - `atlas_settings_never_shows_the_map`: for each SETTINGS capture, rows 266–320 equal `draw_footer` alone, for that frame's footer.
  - The four new cases go in `screen_golden_test`'s table.
- [ ] **Step 2: Run** `cargo test -p chimera-core --test screen_atlas_test --test screen_golden_test` → FAIL (the cases are missing).
- [ ] **Step 3: Implement**, then `just screens`, and read every new PNG with the Read tool. Fix anything that clips, overlaps or misaligns before pinning.
- [ ] **Step 4: Run** → PASS; `just check` → PASS.
- [ ] **Step 5: Commit**

```bash
git status --short
git add chimera-core/tests docs/screens
git commit -m "The screen atlas walks every SETTINGS screen; four are pinned"
```

---

### Task 15: Desktop QA, then the single ship flash (STOP for the owner)

**Files:**
- Modify: this plan (`## Measured`); `docs/adr/{0043,0044,0046,0066}-*.md` and `docs/adr/README.md` (Status → Accepted, after the flash only); `docs/superpowers/plans/2026-09-30-projects-core.md` is not edited (its checklist is run and recorded here)

**Interfaces:**
- Consumes: the whole branch; projects-core's chip checklist (its Task 9 and `## Measured`).

- [ ] **Step 1: Desktop QA (Claude).** Run and record under `## Measured`:
  1. `just check` → PASS; `cargo test -p chimera-core --test param_sweep_test` → PASS (the sweep on the `Location`-driven `UiState`).
  2. On `CHIMERA_CARD=$(mktemp -d) just desktop`, by key script where possible (the tests above) and by hand for what only the window shows:
     - SAVE AS;
     - a quick save;
     - LOAD with an edit pending (each answer);
     - MANAGE: SAVE TO, CLEAR, DELETE;
     - PART: SAVE TO, UPDATE, CLEAR, RENAME;
     - quit and relaunch: the same project, `SAVED`.
  3. Swap `CHIMERA_CARD` between runs: boot shows the right toast. A quick save after `DirStore`'s volume changes shows CARD CHANGED (the `card_changed_then_save_as` test, run on `DirStore` in `chimera-desktop/src/store.rs`, to be added beside `project_store_suite_on_dir_store`).
  4. Measure on the host (release) the frame time while turning a MIDI › CHANNELS cell, with the footer recomputing once per edited frame. Record it beside `PROJ CRC`.

  Any failure becomes a GitHub issue and is fixed before the flash.
- [ ] **Step 2: Commit** `## Measured` (desktop).

```bash
git status --short
git add docs/superpowers/plans/2026-10-01-settings-navigation.md docs/screens chimera-desktop/src
git commit -m "Desktop QA for SETTINGS recorded"
```

- [ ] **Step 3: STOP: the single ship flash.** The owner flashes with `just flash-bench` (`bench::run` falls through to the synth), then `just flash`. Claude prepares this checklist and records the results the owner reads out.

  **This plan:**
  - MENU tap opens SETTINGS on release. MENU held 500 ms quick-saves (`SAVED`), and its release does nothing more. SEQ tap moves a sub-page up on release.
  - Hold MENU to save and, during BUSY, tap B3: you land on Part 3 after the save (no press lost).
  - The `PROJ CRC` row, and the frame time while turning a CHANNELS cell (the footer recomputes once per edit). No audio overrun (`AudioStats`).
  - Every built SETTINGS screen on the panel. UAT (owner): legible, and "obviously the settings world".
  - Swap the card, then hold MENU: CARD CHANGED → SAVE AS saves on the new card.
  - Release firmware: there is no SYSTEM › DEMO, and MIX+B6 is Part 6's mixer.

  **projects-core's checklist** (its Task 9):
  - the `PROJ CRC` and BUS rows, the gate's cost, and no audio overrun during a load (`AudioStats`);
  - boot with no project (`NEW PROJECT`), and boot with the last project;
  - save, power off, power on: the same project;
  - a load with held notes: no click, and the tails ring on;
  - pulling the card mid-save: the previous generation loads;
  - the project list's cost for tens of projects (LOAD PROJECT's first draw).
- [ ] **Step 4: Record** every figure and verdict under `## Measured`. On success, set ADRs 0043, 0044, 0046 and 0066 to `Accepted (<date>)` in their files and in `docs/adr/README.md`. On failure, file issues and stop. No ADR moves.
- [ ] **Step 5: Commit**

```bash
git status --short
git add docs/superpowers/plans/2026-10-01-settings-navigation.md docs/adr
git commit -m "Ship flash measured; ADRs 0043, 0044, 0046 and 0066 accepted"
```

## Measured

(Task 15 fills this in.)
