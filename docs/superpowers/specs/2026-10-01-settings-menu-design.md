# SETTINGS: one button, a list, values you turn

Status: draft for owner review (2026-10-01). Mockups: https://claude.ai/artifact/GH5LoMokwwJoCF5vhwVqYm (owner: "looks right").

## Intent

MENU opens a SETTINGS menu, the way [SETTINGS] does on the Digitakt II and Digitone (Digitakt II manual OS 1.17, § 14). You press one button and get a list of options. You dive into one and get its sub-options. You dive again and reach values you turn directly. Anyone who has used an Elektron box should find it at once.

This replaces the climbing ladder of ADR 0044. The Projects and Project rungs leave the ladder and become SETTINGS › PROJECT. MENU hold becomes a quick save.

## What this changes, and what it keeps

- **Supersedes** these parts of `2026-09-28-projects-storage-design.md` § Navigation:
  - rungs 0 and 1 (PROJECTS, PROJECT);
  - the `up` table;
  - the MENU, SEQ and MIX±MINUS rows of § The keys;
  - the legend lines for rungs 0 and 1.
- **Keeps from that spec:**
  - rung 2, the Part rung (the mixer's PART and SENDS pages), with its B*n* toggle;
  - rung 3, the Sound rung (the pool and library chooser);
  - § Naming and tags;
  - `HOLD_MS` = 500 ms;
  - press timestamps and edge latching;
  - ORBIT's own B layer.
- **ADR 0044** (Proposed) is amended in place to match. **ADR 0066** (new) records the SETTINGS menu. **ADR 0057** is marked superseded by 0044, as 0044 intended.
- **The UX spec** (`docs/chimera-ui-ux-spec.md`) is the owner's file and is never staged. The amendment is delivered as text in the plan for the owner to apply.

## Where you can be

```rust
enum Loc {
    Pages(PartId, PageAt),        // a Part's sound pages (today's Part chain)
    Part(PartId, MixPage),        // its mixer: PART and SENDS (rung 2)
    Sound(PartId),                // pool and library chooser (rung 3)
    Fx(PageAt),                   // CHORUS, DELAY, EFX, TAPE, MASTER
    Settings(SettingsAt),         // a path in the SETTINGS tree
    Orbit(OrbitAt),               // unbound until the ORBIT plan
}
pub struct SettingsAt { path: [u8; 4], depth: u8, row: u8 }   // a row index per level
```

- **`Recall`** gains `settings_from: Location`: where MENU was pressed, never `Settings`. Backing out of the top of SETTINGS returns there.
- **What goes:** `ChainId::System`, the System chain, and `UiMode::SoundBrowser`, which becomes `Sound`.
- **What stays:** the chain defs stay the page source for `Pages` and `Fx`. A SETTINGS leaf that is a page (THEME, AUDIO, …) is drawn by the page renderer from its block def, as today.

## The keys

| Key | Meaning |
|---|---|
| **MENU tap** | Outside SETTINGS, open SETTINGS at its top list (`settings_from` = here). Inside it, back one level; at the top, close to `settings_from`. Acts on release, before `HOLD_MS`. |
| **MENU hold** | Quick save, from anywhere, ORBIT included. The project is saved over its own file. A project with no file (NEW) opens SAVE PROJECT AS instead. Fires at `HOLD_MS`; the release after it does nothing. |
| **EDIT** | On pages, sub-page down, as today. In a SETTINGS list, open the highlighted row: a list, a leaf page or a screen; on an action row, run it (owner, 2026-10-02): EDIT opens lists, leaves and screens, and runs actions; SEQ runs actions and screen rows. |
| **PLUS / MINUS** | On pages, sideways, as today. On the mixer, PART → SENDS → that Part's FX, clamped; other Parts by MIX+B*n* (owner, 2026-10-02; ADR 0044). In a SETTINGS list, move the bar one row. |
| **Encoder 1** | In a SETTINGS list, move the bar (it wraps). On pages and leaves, its cell, as today. |
| **SEQ tap** | On pages, sub-page up, as today. In a SETTINGS list, run the highlighted action (LOAD, SAVE TO, RENAME…), behind a prompt where it can lose work. On the mixer (no sub-pages) and on the Sound rung, it opens SETTINGS › PART for that Part. In ORBIT, it saves the scene (ADR 0044). |
| **SEQ hold** | ORBIT, as ADR 0044 says. It stays unbound until the ORBIT plan. |
| **B1–B6** | From anywhere, SETTINGS included, Part *n*'s sound pages. From those pages, its mixer, and back (ADR 0044). |
| **MIX + B1–B6** | Part *n*'s mixer. MIX+B6 stays Demo until Demo becomes a debug-only SYSTEM row. |
| **MIX + MENU** | Reserved for the chain editor. |

From the storage spec's key table, SEQ as save goes everywhere, and MIX+MINUS as delete-a-project goes with rung 0: DELETE and SAVE now live in SETTINGS. MIX+MINUS keeps its other meanings, clearing a pool slot on the Sound rung and removing a library sound (plan 3), both behind a confirm.

## The tree

Status in this plan: **built**, **mirror** (an existing page moved here), or **later**. A later row shows dimmed with `LATER` and gets a GitHub issue and its own spec.

```
SETTINGS
├─ PROJECT          built
│  ├─ LOAD PROJECT        the card's projects by slot, + CREATE NEW at the bottom
│  ├─ SAVE PROJECT AS     a new slot → NAMING → save
│  └─ MANAGE PROJECTS     list | commands: LOAD FROM · SAVE TO · RENAME · CLEAR · DELETE · PROTECT(later)
├─ PART             built (the active Part)
│  ├─ RENAME              the Sound's name → NAMING
│  ├─ CLEAR               the Part to INIT of its engine (guarded replace)
│  ├─ SAVE TO PROJ        save over its slot / to a new slot (part_actions)
│  └─ RELOAD FROM PROJ    revert to its slot (part_actions)
├─ ORBIT            later (with ORBIT): RENAME · CLEAR · SAVE TO PROJ · RELOAD FROM PROJ for scenes
├─ MIDI CONFIG
│  ├─ SYNC                later: CLK IN/OUT, TRANS IN/OUT, PRG IN
│  ├─ PORT CONFIG         later: INPUT FROM (DIN/USB/BOTH), OUTPUT TO, RECEIVE NOTES, RECEIVE CC
│  └─ CHANNELS            built: Part 1–6 channels (mirrors each mixer PART page's CHANNEL)
├─ SYSEX DUMP       later
├─ AUDIO ROUTING
│  ├─ OUTPUTS             mirror: each Part's OUT pair; later: STEREO/MONO and LEVEL per DAC pair
│  ├─ SENDS               later: PRE/POST FADER (#259)
│  └─ TUNING              mirror (today's System › TUNING)
├─ PERSONALIZE
│  └─ THEME               mirror (today's THEME: BRIGHT, GAMMA, ACCENT, BLACK)
└─ SYSTEM
   ├─ OS UPGRADE          mirror (today's UPDATES)
   ├─ STORAGE             later: card use, projects, Sounds
   ├─ FORMAT CARD         later
   ├─ USB CONFIG          later
   ├─ DIAGNOSTICS
   │  ├─ AUDIO LOAD       LOAD, PEAK, OVER, DROPS, DESYNC, STACK (was ABOUT's hidden AUD page)
   │  ├─ TEST TONE        later (#295)
   │  ├─ INPUT TEST       later (#296)
   │  └─ DEMO             debug builds only (replaces MIX+B6 there): a row per storyboard page
   └─ ABOUT               VERSION, BUILD, REV, CLOCK, RESET, CARD: read-only, one page
```

A leaf is exactly one page (owner, 2026-10-02): nothing hides behind EDIT or PLUS, and a leaf with sub-pages fails the build (`OnePage`, ADR 0066). A screen with more to show becomes a list of leaves.

The tree is one static table: rows with a label, a kind (list, leaf page, action) and a status. A later row is in the table, so adding a feature means flipping its status, not changing navigation.

## Screens

SETTINGS has its own visual language, on purpose (owner, 2026-10-01: "it's obvious you are in the settings world of the synth"). It uses lists with a highlight bar, a growing breadcrumb and a project footer, where the Part and mixer flows use cell pages and the chain map. A SETTINGS leaf page uses the cell grid for its values but keeps the breadcrumb header and the footer, never the chain map, so you can always tell which world you're in.

- **List:** the breadcrumb in the header, e.g. `SETTINGS › PROJECT › LOAD`, with the last part bold. Rows are 26–30 px. The bar is teal on a soft fill with a left tick. A `›` marks a row that opens more. A right-hand note shows a value or status (`● LOADED`, `FILE DAMAGED`, `SLOT 03`). A scrollbar appears when the list overflows.
- **Footer**, on every SETTINGS screen:
  - the project's name;
  - its status: `* MODIFIED` in `theme::WARN`, `SAVED`, or `NEW`;
  - a one-line legend for that screen's keys.

  The status comes from a cached `project_status` (below).
- **Leaf page:** an ordinary page with up to six cells, one per encoder, eased like every value. The header is the breadcrumb.
- **Prompt:** a teal-outlined panel with the question, the reason in the route colour, and two or three pill options. Encoder 1 picks, SEQ confirms, MENU is always CANCEL. It is used for every `NeedsConfirm`:
  - SAVE THEN LOAD / LOAD ANYWAY / CANCEL;
  - SAVE PART FIRST / REPLACE / CANCEL;
  - P4 ALSO USES SLOT 03: UPDATE P4 / LEAVE;
  - NAME EXISTS: KEEP BOTH / OVERWRITE THAT ONE;
  - DELETE …? / CLEAR …?;
  - FORMAT, when it lands.
- **MANAGE PROJECTS:** two columns, the project list on the left and its commands on the right. EDIT moves into the commands, SEQ runs one, MENU returns to the list. A command that can't apply is dimmed: you can't DELETE the loaded project, and PROTECT is later.
- **NAMING:** the storage spec's screen, unchanged:
  - A moves the cursor;
  - B cycles A–Z;
  - C cycles 0–9, space and `-`;
  - D toggles case;
  - E deletes;
  - F opens tags. Tags are plan 3, so F is inert until then.

  SEQ saves and MENU cancels. It opens with a proposed name.
- **Toasts:** `SAVED: NAME`, the error notes, and NO CARD / NEW PROJECT, as projects-core shows them, drawn over the current screen.

## The PROJECT branch, wired to projects-core

- **LOAD PROJECT:** `list_projects` fills the rows:
  - the loaded project carries `●`;
  - an unreadable entry is greyed with its note;
  - with no card the list is just NO CARD.

  SEQ on an entry runs `ReplaceGuard::check` on `ProjectSource::File`. Pristine or Saved loads at once. Modified shows the prompt. The load goes through `UiState::load_project` with the shell's publish closure, so the SYSTEM write follows the publish. `+ CREATE NEW` is `ProjectSource::New`, through the same guard.
- **SAVE PROJECT AS:** `new_project_id` on the mounted card, then NAMING, then `save_project`. A name that already exists on the card asks KEEP BOTH / OVERWRITE THAT ONE. OVERWRITE needs a typed confirmation (#257); this plan adds `Confirmed<OverwriteTarget>`.
- **MANAGE PROJECTS:**
  - **LOAD FROM:** as LOAD.
  - **SAVE TO:** the active project over the chosen slot, behind the OVERWRITE confirmation.
  - **RENAME:** NAMING. A rename of the loaded project renames it in RAM and marks it Modified. A rename of another project rewrites that file's header name in place, behind a confirm.
  - **CLEAR:** the slot's file becomes a fresh NEW project, behind a confirm. Clearing the loaded project's slot is a guarded replace to NEW plus a save.
  - **DELETE:** behind a typed `Confirmed<DeleteTarget>` (#257). The loaded project can't be deleted. Deleting the project SYSTEM names clears SYSTEM's last project (projects-core).
  - **PROTECT:** later; it needs a write-protect bit in the project file header.
- **Card events:** every card operation returns its `CardEvent`, and the list re-lists after any operation (#257).
- **After a card swap,** SAVE (MENU hold) refuses with CARD CHANGED and offers SAVE PROJECT AS on the card now in the slot (#257).

## The PART branch

It acts on the active Part, which is the last B*n*. The header shows `PART n`, the Sound's name, and its mark: `* EDITED · FROM SLOT 03`, `◦ SLOT nn MOVED`, or `CLEAN`.

- **SAVE TO PROJ** and **RELOAD FROM PROJ** are the `part_actions` that apply:
  - Edited: OverSlot or NewSlot, and Revert.
  - Stale: Revert (UPDATE) and NewSlot.

  A row that doesn't apply is dimmed. A save-over that leaves other Parts Stale asks P4 ALSO USES SLOT 03: UPDATE P4 / LEAVE.
- **CLEAR** is a guarded replace to `PartFrom::Init(engine)`.
- **RENAME** edits the Sound's name, which marks the Part edited.
- This retires the legacy browser's bypasses: its Load answering REPLACE itself (#258), and its Save calling `save_part_to` directly (#257). The browser becomes the Sound rung; its own SEQ save goes, and saving is SETTINGS › PART.

## State and storage

- **Global settings** live in SYSTEM (ADR 0045): THEME and the last project today; later SYNC, PORT, ROUTING and LED options.
- **Per-Part settings** (channel, output pair) stay in the project's Part mix, as now. MIDI › CHANNELS and AUDIO ROUTING › OUTPUTS are mirrors of the same fields; editing in either place edits the one value.
- **`project_status` is cached** on a revision counter that `Project`'s `&mut` accessors bump (#257). The project CRC costs about 1.7 ms on the chip, so the footer and the `*` must not recompute it per frame.
- **Controls:** press timestamps and edge latching in both shells' control ticks. `chimera-stm32/src/controls.rs` keeps only debounced levels today, so a press during a card operation is lost. Taps act on release and holds at `HOLD_MS`. One `HoldGate` serves MENU, SEQ and, later, ORBIT's hold B*n*.

## Tests

- **Location property test:**
  - every SETTINGS path is reachable from the top by EDIT, and MENU backs out to `settings_from`;
  - B*n* leaves from any depth;
  - MENU hold never moves `Location` except into SAVE AS for a NEW project;
  - `settings_from` is never `Settings`.
- **Key tests:**
  - tap versus hold at `HOLD_MS ± 1` frame;
  - no press lost during a blocking card operation, simulated by a stalled tick.
- **Branch tests:** each PROJECT and PART command against `MemStore` (the projects-core suites cover the store); each prompt's three answers; CARD CHANGED after a swap, then SAVE AS.
- **Screens:** the screen atlas (`screen_atlas_test`, PR #261) learns to walk the SETTINGS tree: every built row, every leaf, the prompts, NAMING and the footer, regenerated into `docs/screens` with its README. Pinned goldens cover the top list, LOAD, a prompt and NAMING.
- **Desktop QA:** load, save as, quick save, manage and clear, all on `DirStore`, and a relaunch.
- **The single ship flash:** this plan's chip checks plus projects-core's checklist (its `## Measured`). ADRs 0043, 0044, 0046 and 0066 move to Accepted there.

## Out of scope (filed as issues when the plan starts)

ORBIT's branch and the ORBIT entry; SYSEX DUMP; MIDI SYNC and PORT CONFIG values; AUDIO ROUTING's per-pair STEREO/MONO and LEVEL and the PRE/POST send choice (#259); SYSTEM STORAGE, FORMAT CARD and USB CONFIG; PROTECT; tags and the library (plan 3); the Digitakt's PERSONALIZE options beyond THEME.
