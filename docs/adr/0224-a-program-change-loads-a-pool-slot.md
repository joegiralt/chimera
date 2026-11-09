# 0224. A program change loads a pool slot into every Part on its channel

- **Status:** Accepted (2026-11-09)
- **Deciders:** owner (MIDI I/O spec decisions 1, 2C, 6 and 7, 2026-10-15/16, #515), firmware

## Context
Program changes were parsed and dropped (`PipeStats::programs_dropped`). A sequencer changes Sounds by program change on each pattern, usually just ahead of the pattern's first notes. The design is `docs/superpowers/specs/2026-10-15-midi-io-design.md` § 2 (branch `midi-io`), with the owner's answers in #515: bank 0 is the pool and bank 1 the INITs (1); an edited Part loads and keeps its edits as a recall copy, Elektron style (2C); every Part sharing the channel loads (6); the channel's notes wait up to 40 ms for the load (7). Bank 1 is frozen (ADR 0198).

## Decision
- **What a program addresses.** Bank 0, program *n*: pool slot *n* + 1 (128 slots, one bank). Bank 1: the INIT of the engine whose disk code is *n* (ALGO 0, MODAL 1, TAPE 2, VA 3), not browser order. Banks 2 to 127, an empty slot and a code with no engine do nothing. LIBRARY and FACTORY are not addressed: their order on the card shifts as files are added.
- **Bank select.** CC 0 and CC 32 (the parser's latch, `Ctl::Program { bank }`) set the bank: the MSB, or the LSB when the MSB is 0, so sequencers that send MSB 0, LSB 1 reach the INITs. It stands per port and channel until changed (MIDI 1.0), from 0 at boot (`MidiSlot::banks`).
- **Who loads.** Every Part on the channel, each on its own.
- **No-ops.** A Part already holding the Sound, Clean or edited, stays; a Stale one reloads it.
- **An edited Part never refuses and never loses its edits.** `Project::program_load` swaps the Part's Sound into its recall copy (one Sound a Part, 8.8 KB in a D2 static beside `UiState`, reached through `MidiSlot::recall`; AXI has no room past its margin) before loading. A later change back to the Sound the copy was edited from swaps the edits back, still edited (`*`): its Origin is the slot as it is now, so a slot saved over since (its CRC moved) drops the copy. One copy a Part: a new edited Part's leaving replaces it. Any other load of the Part (RELOAD, the browser, a library load) moves its `Loads` and drops the copy; a project load drops every copy. `program_load` is the one load without `ReplaceGuard`: it loses nothing.
- **When.** The UI applies each channel's latest change at the end of its MIDI pass, unless card work runs (a load, a library load, a source row), a prompt or learn is open, or the Part's Sound rung is browsed; then it waits (`MidiSlot::wait`, latest wins), kept across a project load. An applied change on the active Part moves its pages to its engine (`project_replaced`), unless the browser is open; another Part's never moves the page.
- **The note hold.** With PROGRAM IN on, a pipe that queues a program change sets its channel's bit in `PROGRAM_PENDING`. The audio's drain then keeps that channel's events in order in a 16-event hold (`NoteHold`), other channels unheld. The UI clears the bit the pass after it applied, refused or dropped the change, so its load is published first; the drain lets the channel go when the bit clears, or after 40 ms (`HOLD_BLOCKS`, clearing every pending bit); a full hold lets its oldest event go ahead of each new one. Once a block, while a bit is set or anything is held, the door's `hold` sorts the queues: what may play goes to a 16-event `Ready` ring per source (`ProgramHold`, a static of the shell's, only in builds with the MIDI map, which drain with `NoteDrain::drain_held`; past 16 a block, the rest wait in the queue behind it, unheld), which the drain empties ahead of the source's own queue, so the drain stays one loop and the Parts' note handling is inlined once.
- **PROGRAM IN**, SETTINGS › MIDI CONFIG › SYNC's B cell (`PROGRAM`, OFF/ON, off by default; `MidiSettings` switch bit 4, `ParamId(4)`, ident `PRGIN`; a SYSTEM file from before it reads OFF). Off, program changes and bank selects are ignored and nothing is held. The UI mirrors it into `PROGRAM_IN` each pass for the pipes. Without ORBIT, SYNC is a LATER row and PROGRAM IN stays off.
- **Bank placement.** The UI side is `ui::midi::program` (bank 2 with `ui::midi`); `Project::program_load` and `NoteHold` are globbed into bank 2; the drain reaches the hold through `MidiDoor::mpe`'s `Call::Hold` (the door table is full; MPE's one entry carries it) only while a bit is set or the hold is not empty. Closed, the drain drains as before. Bank 1 gains the door stub in `NoteDrain::drain`, the setting's closed-door data and the `MidiSlot` fields.

## Alternatives considered
- **Load only the channel's lowest Part** (the spec's first draft): a layer or split on one channel would keep stale Sounds; the owner chose every Part (6).
- **Refuse an edited Part** (2A): after the first controller tweak, program change stops working live. **Always load, edits lost** (2B): a careless pattern change loses work. **B behind a third PROGRAM IN value** (2D): two behaviours to learn.
- **The hold in the pipe**: DIN's pipe runs only when bytes arrive, so nothing would let held notes go without a new bank-1 timer.
- **Clear the pending bit as the change is applied**: the audio could drain the held notes between the apply and the publish and play them on the old Sound.

## Consequences
- A sequencer's program change and the downbeat on the same tick play the new Sound, up to 40 ms late on that channel only.
- Bank 1 grows +224 to +760 B a set (default +712, ORBIT +760, on main 95f5241f, stamped clean), measured and raised in `tools/bank1-budget.txt`. Per symbol (default): PROGRAM IN's closed-door data, its spec (+72), `write` arm (+52) and names (+148, anonymous rodata); the drain's door stub and `Ready` loop (+176 in the audio block); the MIDI slot's waits written in place by `take` (+72, `take_ui`); `Project::replace_part` out of line now `load_part` has a second caller (+62); outliner churn (+92). Builds without the MIDI map build no program-change code or state (`ui::midi::program`, the slot's waits and recall pointer, `ProgramHold`), drain with no hold, and keep only the setting's data.
- 8.8 KB more D2 for the recall copies (+9,120 B of `.ram_d2` with alignment), only in builds with the MIDI map; D2 keeps 26,344 B free (268,568 of 294,912 B, was 35,464) (written on the first program change after a project load).
- Program change out, LIBRARY and FACTORY by program, and a per-Part receive switch are not done.

## Sources
- #515 and its comments; `docs/superpowers/specs/2026-10-15-midi-io-design.md` § 2 (branch `midi-io`).
- Elektron Digitakt manual, MIDI CONFIG › SYNC › PROG CH RECEIVE; the Octatrack's unsaved-part reload.
- MIDI 1.0 Detailed Specification: bank select (CC 0, CC 32) applies to the following program changes.
