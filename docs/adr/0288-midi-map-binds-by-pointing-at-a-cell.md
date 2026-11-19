# 0288. Add and retarget MIDI MAP bindings by pointing at a cell, no controller needed

- **Status:** Proposed
- **Deciders:** owner (design approved 2026-11-18, from chimera-releases#18), firmware

## Context
A tester drives Chimera from Terra's touch pads and knows which CCs Terra sends. MIDI MAP (SETTINGS › PART › MIDI MAP, ADR 0139, 0217, 0226) could only gain a row by MIDI learn (MIX + SEQ on a cell, then a message from the controller), and a row's target could only change by deleting the row and learning it again. So no map could be built or fixed with the controller unplugged. The owner approved this design on 2026-11-18:
- MIDI MAP ends with a `+ NEW BINDING` row. EDIT on it creates a binding: the CC is set with the INPUT cell's existing modes and number, the destination by pointing at it.
- Pointing walks the box's own pages to any cell, the same way learn picks its target, and EDIT binds it. A hint shows while pointing (ADR 0086).
- EDIT on an existing binding re-enters pointing and changes only its destination; its CC, MIN, MAX, INPUT and ECHO stay.
- MIDI learn stays exactly as it is.

Bank 1 is frozen (ADR 0198, 0251): the logic and words go to bank 2.

## Decision
- **One binding model, one target rule.** A new row is `MidiMap::learn`'s, the same call learn makes: absolute `JUMP`, the full range, echo off. The destination is learn's: the focused cell's param if `Mappable` (`UiState::learn_target`). The row's control is then edited on MIDI MAP with the keys it always had: B the number, E the INPUT, C and D the range, F the echo.
- **The new row's control** is the first CC no row of the Part's map or of its controller layer is on (`Input::same_control`), so a new row never shadows a layer row (ADR 0226). The row is appended; the map's one row per target holds.
- **`+ NEW BINDING`** is MIDI MAP's last row, after the pick, the layer's rows and the Sound's own (`MapRow::New`). Its lines read `POINT AT A CELL` and `B AND E SET ITS CC`, its values line `EDIT POINT AT A CELL`; the legend reads `EDIT RUN · MENU BACK`. A full map (16 rows) dims it: `MAP FULL`, `DELETE A ROW FIRST`, and EDIT does nothing. The empty page keeps its two learn lines, under the new row.
- **Pointing is a learn state.** `Learn::Pointing(Point { part, aim, at })`, so learn and pointing can't both run:
  - `Aim::New` (`+ NEW BINDING`) or `Aim::Retarget(Mappable)` (EDIT on a Sound row, the row found again by its target, as DELETE finds it).
  - `Pointer::Walking` or `Pointer::At`. On arrival at any page it is `Walking`, and EDIT walks the pages as it always does (down a sub-page under STACK, up under ROW). A turn points at its cell, as learn's armed turn does: it moves the focus, never a value, and makes it `At`.
  - EDIT binds only while the hint says so: pointing `At` a cell that is mappable and that no other row holds (`Pointed::Binds`). Otherwise EDIT walks. This keeps every page reachable with EDIT under both PAGES settings, and no cell is bound by a stray EDIT.
  - MENU cancels. Either way MIDI MAP opens again, the bar on the new or retargeted row, or on the row pointing began from.
  - Pointing starts on the page SETTINGS was opened from when it is the Part's (MENU from SETTINGS' top, resolved as `Recall::close_settings` does), else on the Part's engine home. It goes on through the Part's pages, mixer, FX, Sound rung and ORBIT (ring amounts are mappable, ADR 0225). Another Part's place, a quick save, `ed part` or SETTINGS ends it. MIX + SEQ does nothing while pointing (pointing judges the key first); learn is not armed. A program change waits while pointing, as it waits while learn runs.
- **What it says.** In the focus band's value place, as learn's words are (`LearnNote::Point(Pointed)`): `NEW BINDING` or `NEW TARGET`; `<CELL> NOT MAPPABLE` (learn's words); `ALREADY BOUND` on a param another row holds. Its own target binds again, unchanged. On the hint line, whatever the cell's own: `A-F POINT  MENU CANCELS`, and `EDIT BINDS  MENU CANCELS` once EDIT would bind. A BigViz page has no focus band: the hint takes its status line, but for a refusal, which says why. A Sound row's keys hint adds `EDIT TARGET` (`B CONTROL  E INPUT  EDIT TARGET`); a fixed control's B reads `RELEARN` to make room.
- **Cancel is MENU**, as it cancels a card or library load and backs out of SETTINGS. MIX alone is a modifier everywhere else on the box, and MIX + SEQ is learn's.
- **Retarget is the Sound's rows'.** A controller layer's row (ADR 0226) is edited in the map file by section, and a new target may belong to another section. EDIT on it does nothing; learning a Sound row over its control and saving the map, ADR 0226's one path, changes it.
- **Placement (ADR 0198, 0272).** `ui::pointing` (what pointing says, the keys and moves it judges, the bind, the new row's words and one named word table) links into bank 2's S4 group, high, behind three `MidiUiDoor` entries: `point_say`, `point` (a `PointAct`: `Nav(NavKey)`, `Orbit(&[Option<OrbitKey>])` or `Moved(Location)`, `true` when it takes the frame) and `point_hint` (`MAGIC` 0xB2C0_0288). `just check-door-placed` holds it to the S4 group. MIDI MAP's row and key changes are `ui::midi::page`'s, bank 2's; its EDIT reaches `map_in` as a fourth key bit. A bind or cancel takes the frame, so MIDI MAP's keys never read the bind's EDIT again. Bank 1 gains the types (`Point`, `Aim`, `Pointer`, `Pointed` with its fieldless `AimKind`, `PointAct`), one out-of-line stub (`UiState::point`; closed, it ends pointing), the calls from `go`, the ladder's key loop and ORBIT's, the renderer's one hint line (`point_line`, three sites), MIDI MAP's row kind and legend, and `EDIT TARGET`. Closed, pointing never starts and MIDI MAP reads `NEEDS UPDATE`, as before.

## Alternatives considered
- **EDIT always binds the focused cell while pointing.** Under STACK, EDIT is the only key down a sub-page, so a deeper page (FILTER › MORE, REV › MORE) could not be reached, and EDIT on arrival would bind whatever cell the page last focused. A turn to point first costs one detent and makes the bind deliberate, as learn's arming on a touched cell is.
- **SEQ wraps upward while pointing** (to free EDIT). It changes the ladder's rules in one mode only, and REV › MORE 2's skip would need a second path.
- **MIX cancels** (the brief's example). MIX alone does nothing anywhere; MENU is the box's back and cancel.
- **Ask for the CC first, then point.** A number picked before the target is a second editor for what B and E already do on the row; after the bind the row is on screen with its keys.
- **Replace the other row on `ALREADY BOUND`, as learn does.** Learn writes a control the player just moved; pointing would silently drop a row the player set up. Saying so and leaving both is safer: that row can be retargeted instead.
- **Retarget a layer row in its map file.** Its section can change with the target (an engine's own block against a shared one), which needs a move between sections and a second save path for one rare edit.

## Consequences
- A map can be built or fixed with the controller unplugged, and a row moved to another param without retyping its control, range or echo.
- MIDI MAP has one more row; the screen goldens of its pages with the bar near the end move with it.
- Flash (measured, release builds; base origin/0.4 76de24c6): bank 1 ORBIT +992 B (894172's line → 895164), default +768, bank2-gate +792, orbit,usb-config-writer +992, bench-tape +104 (each line raised by its delta); the other bench and probe sets −72 to −104 (no `midi-map`: pointing's calls compile out). The types and calls are the share that can't leave bank 1: `go`'s and the key loops' calls, `point`'s stub, the renderer's `point_line` (generic over the display, so not a door entry), MIDI MAP's legend and the `EDIT TARGET` hint. Bank 2: ORBIT +2304 B (S0-S3 −320 for MIDI MAP's row and EDIT, the rest in the S4 group), default +2272, bank2-gate +2304; ORBIT's S0-S3 keeps 8448 B free.
- RAM: `Learn` grows by a `Point` (a Part, an `Aim` with a `Mappable`, a `Pointer`) in `UiState` (D2); `LearnNote` carries a `Pointed`.

## Sources
- chimera-releases#18; owner's design, 2026-11-18.
- ADR 0017, 0081, 0086, 0139, 0145, 0198, 0217, 0225, 0226, 0251, 0255, 0272.
- Precedent: Ableton Live's MIDI map mode (click a control to bind it, no message needed); Elektron Digitakt/Digitone CC tables set by hand on the unit; Deluge learn (hold LEARN, touch the target).
