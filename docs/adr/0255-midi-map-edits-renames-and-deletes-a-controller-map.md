# 0255. Edit, rename and delete a controller map on MIDI MAP, and reach it over USB

- **Status:** Proposed (amends 0226)
- **Deciders:** firmware (#880, PR #879's follow-ups; the owner's "stop pausing, get all 0.3 tickets done")

## Context
ADR 0226 keeps controller maps as card files a Part names. It left out three things the unit needs to own its maps:
- A map's rows could only be deleted on the Part. 0226 chose "learn over the row and save" over editing, to keep one save path.
- A whole map could be neither renamed nor deleted on the unit.
- The console's `ls`, `put`, `get` and `rm` didn't reach `/CHIMERA/MAPS`, so a map couldn't be copied to or from a computer.

Bank 1 is frozen (ADR 0198). ORBIT's bank 2 has about 10 KB free, D2 about 4 KB.

## Decision
- **Layer rows edit as Sound rows do.** On a controller map's row, B steps the control, C and D the range, E the input and F the echo, as on a Sound's row.
  - The row changes on the page at once. The edit is written to the map on the card by the next card work, read, changed and saved as a row's delete already is.
  - Edits of one row before the card work runs make one job, from the row as the card holds it to the last edit. "One row" is the whole binding, not its target: a hand-made file can hold two rows on one target.
- **One map job waits at a time, none lost.** While one waits for the card, a turn on another row does nothing, and a save, rename or delete confirmed on screen is refused with `CARD BUSY`. Nothing waiting is overwritten.
  - The Sound is not edited: no `*`.
  - The row's hint is a Sound row's (`B CONTROL  E INPUT`, `RELEARN TO CHANGE` for a fixed control).
- **Rename: MIX + EDIT on row 0.** NAMING opens on the map's name, its crumb `RENAME MAP`.
  - The map is saved under the new name, then the old pair deleted. Every Part on the old name takes the new one.
  - A name another map holds is refused (`NAME TAKEN`), nothing moved. The same name does nothing.
  - With no map picked, MIX + EDIT saves as EDIT does.
  - If the old pair can't be deleted after the new one is whole, it stays, a copy. Retrying would find the new file and read it as taken.
- **Delete: MIX + MINUS on row 0** asks `DELETE MAP?` over the map's name, then deletes that map's pair (the name asked about, even if the Part picks another before the card work runs). Every Part on it plays `NONE`. With no map picked it asks nothing.
- **Hints.** With a map picked, row 0's tail line reads `MIX+EDIT RENAME` and its hint line `EDIT SAVE  MIX+MINUS DELETE` (`B PICK` stays in the bottom legend): both on one line don't fit the band. With none picked the tail reads `CARD HOLDS MORE: 16 SHOWN` when that applies, and the hint `B PICK  EDIT SAVE ROWS TO MAP`, as before.
- **Over USB** (the console, `tools/chimera-usb.py`):
  - `ls maps` lists `/CHIMERA/MAPS`.
  - `get` and `rm` take `/CHIMERA/MAPS/<stem>`, the pair. `get` sends the side a load takes, judged by its framing (kind, header, CRC). `rm` deletes both sides in ADR 0045's order (the side a load does not take first, so a cut between leaves the live map), as `ctl::delete` does on the unit.
  - `put` takes `/CHIMERA/MAPS/<stem>.A` or `.B`, a whole file of at most 4 KiB, as a tuning goes. The side letter is ignored: the file goes to the side a load does not take (`write_target` over both sides' framing). It is read back as a controller map's frame, and its header's name must make the stem (`midi_map::stem`, bank 1's for this); else it is refused and deleted. Only then is the live side deleted, so the map put is the one that loads.
  - A failed transfer, a full card or a refused file leaves the live side as it was. The unkept side's older copy may go.
  - The host tool's `put <file> /CHIMERA/MAPS/` names the file by the map's own name (the unit's stem). `get <remote> <local>` writes the file, its CRC checked, never over a file there.
  - A `put` or `rm` moves `midi_map::maps_put()`, a count. Bank 2's card work sees it move and reads every Part's layer and the listing again.
- **Placement.**
  - **Bank 2:** the row edit and its job, rename, delete, the tail's words (`ui::midi::{page, layer}`), and `ControllerMap::replace` and `ctl::delete` (`midi_map::ctl`).
  - **Bank 1:** the console's map paths, `put`'s side choice and judge, and `ls maps`. The console is bank 1's because it has no door, not by ADR 0251's rule. Also: the `maps_put` count; `MapDelete`'s whole-map form; `NamingFor::Controller`'s rename flag and MIX's read on row 0's EDIT; the `PICK_MAP` hint. `MidiUiDoor`'s `ctl_ask` takes the job whole, so its magic is `0xB2C0_0255`. A `.bk2` from before won't open the door.

## Alternatives considered
- **Edit a layer row by learning over it and saving** (0226's choice). It takes three steps and moves the row into the Part's own rows on the way. Editing in place costs one more job kind; the save path is the same.
- **A write on every detent.** One write a turn of a knob is many small writes. The job coalesces a row's detents until the card work runs.
- **Rename as "save as, then delete".** Two gestures, and a save as takes the Part's own rows into the map. A rename never touches the rows.
- **A `put` saved the unit's way** (the next generation, the side a load doesn't take, the kind's own check, as a Sound). It needs a check for maps and another monomorph of `save` in bank 1. A map file is under 1 KB and its rows are checked when it loads: written whole, its framing checked, its other side dropped.
- **Both gestures on one line.** `MIX+EDIT RENAME  MIX+MINUS DELETE` is a character or two wider than the band. The values line holds KEEP MAP.

## Consequences
- A map can be made on the unit, copied to a computer, shared and copied back.
- The door's magic moved: an old `.bk2` reads `NEEDS UPDATE` on MIDI MAP until updated (one-step install already pairs them).
- **Measured** against main 6d170da0:
  - **Bank 1:** `default` +2560 B, `orbit` +2888 B, `bank2-gate` +2440 B, `usb-audio` +2336 B; the bench sets +0 to +8 B (no console `put`, no MIDI MAP). By symbol (`orbit`):
    - The console: `put` +276 and the map `whole_session` +360 (the side choice, the judge's header read and stem check), `midi_map::stem` 346 (moved from bank 2 for that check), `write_target` 240, `map_pair` 196, a `check_frame` instance 196, `PairArg::parse` +76, `rm` +164, `get` +52.
    - MIDI MAP's stubs: `MapDelete`'s panel +72, `map_deleted` +12, `NamingFor::crumb` +56, `ctl_ask` 60.
    - The rest inlining churn (`Usb::service`, `UiState::input`) and the outliner's. `#[inline(never)]` on `console::answer` (+80 B) or `name_controller` (+0 B) did not shrink it.
    - The console is bank 1's because it has no door, not by ADR 0251: ORBIT's bank 2 keeps 8.4 KB, just above 0251's 8 KB line.
  - **Bank 2 (`orbit`):** +2208 B, 8480 B free.
  - **RAM:** D2 +8 B (the layers' `puts`); AXI none.
  - **Stack:** nested depth unchanged (59020 B `default`, 59580 B `orbit`).
- Not done here: the permanent built-in layer (its CCs aren't reserved yet) and ready-made maps shipped with the firmware.

## Sources
- #880; PR #879; #807.
- ADR 0045 (card format), 0198, 0226, 0240 (a whole small file over `put`), 0251, 0253.
- Elektron and Octatrack file management: rename and delete from the list the file is picked in.
