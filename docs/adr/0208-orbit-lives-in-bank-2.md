# 0208. Move ORBIT's runner and pages to bank 2

- **Status:** Proposed
- **Deciders:** owner ("move ORBIT", the first step of #741's plan under ADR 0198), firmware

## Context
ADR 0198 froze bank 1. #741 lists what can move out of it, and ORBIT came first: about 29 KB of the ORBIT build's bank 1 (private notes), feature-gated, and run once a block. Bank 1's `Bank2Door` lives in `chimera-engine`. That crate can name the runner, but it can't name ORBIT's pages, which are `chimera-core` UI drawn on a generic display. ORBIT runs on the audio thread (0071) and follows one MIDI clock (0072). Moving it must keep it playing step for step the same.

## Decision
- **The runner is bank 2's**, behind `Bank2Door::orbit` (`chimera_engine::orbit::door::OrbitDoor`: `block`, `block_streamed`, `realtime`, `live`, `fill_view`). A door entry is a plain `fn` and `block_ported` is generic over the voices' ports, so the table holds one entry for each port type a shell runs: `NoPorts` and `Streams`. The sealed trait `OrbitPorts` picks the entry, and the shells' blocks are bounded by it. `OrbitRunner`'s public methods are bank 1's proxies, so every caller (the board's shell, the bench, the desktop, the tests) goes through the door. `bank2.x` places the door module, `orbit::engine`, `mutate`, `nudge`, `walk_next`, the runner's `advance` and `load`, and `MidiClock::tick` in bank 2. Bank 1 keeps the runner's state and its constructor, whose `OrbitState` is now built at compile time (`OrbitState::start` is const). It also keeps the default view, the command channel and the scenes with their specs, codec, records, pools, register note, Euclid fill and theory, because the UI's edits and the project's load and save use them.
- **The pages are bank 2's**, behind their own slot. A table the engine can't name sits at a fixed slot after the door (0175's `DoorSlot`). `ORBIT_UI_DOOR_ADDR` is `DOOR_ADDR + 0x280`, after the board's (0x100), TAPE's UI (0x180) and the bench's (0x200). `bank2.x` links it there in every build with `orbit`, and asserts that it sits at `0x08100380`. It is a table of its own, not fields in TAPE's `UiDoor`, so each feature's pages come and go with its own feature. It holds `ui::orbit::door::OrbitUiDoor`: the seven viz bands, ORBIT's cell text, its focus band and its viz key, each `ui::orbit::draw`'s function wrapped in `ui::orbit::raster`. They draw on `Ink`, one concrete display type over `&mut dyn Paint`, so each band is one function the table can name. `Ink` and the `ui::draw`, embedded-graphics and u8g2 functions built for it stay in bank 1. In bank 2, `core`'s size-built code gets machine-outlined, and a fragment outlined into bank 1 that branched into bank 2 failed the door check. With those helpers in bank 1, no fragment does, and the `dyn Iterator` vtables they build are bank 1's too. The table carries a magic word. The boot's door check reads it beside the board's and TAPE's (`ui::orbit::door::door_ok`), so a wrong table at the slot leaves bank 2 corrupt, and no call reads it again.
- **Closed, ORBIT is stopped and says so.** No block sends a note or moves the clock, and the harmony is let go. Keys and real-time messages pass ORBIT by, and the view says it isn't running, so the beat LED stays dark. ORBIT's pages warn `UPDATE` in the header (`bank2::closed_orbit`). Each ORBIT param reads `NEEDS UPDATE` in the focus band and `UPDATE` in its cell (`closed_text`), and ORBIT's own cells read the same (`closed_focus`). The viz bands stay blank. Edits still change the project, and a load and a save keep every scene, drift and seed (`orbit_with_the_door_closed_keeps_the_project`). A scene queued while the door is closed waits, as it would for a stalled runner (0082).
- **Proof.** `chimera-core/tests/orbit_door_test.rs` hashes four songs: NEW's INIT scene, the frozen `orbit.prj` fixture with a scene switch and STOP, a dense scene with a live capture and a BPM change, and a DAW's clock with Start, Stop and Continue. For each it hashes ORBIT's events block by block, its view, and the whole engine's summed audio. The expected values were taken from main before the move. Bank 1's own path and the host's open door both match them. `--features bank2` runs the closed tests. The screen goldens check that the pages draw the same pixels through `Ink`.
- **Cost.** A door call is the cell's load and barrier, a branch, the table's load and an indirect call: about 10 cycles. A block makes two of them (`block` and `fill_view`), plus one for each key and each real-time message. ORBIT schedules in samples, not CPU time, so the calls add no jitter. Bank 2 runs from the same flash at the same wait states as bank 1, and is never slower (0142, 0174). `OrbitRunner::COST` stands.

## Alternatives considered
- **Moving the page code into `chimera-engine`, as the LFO raster did (0177):** it is built from `core`'s theme, fonts, hints and view, so it would move half the UI with it.
- **Leaving the pages in bank 1:** about 6 KB stays there, and 0198 wants each feature in bank 2 whole.
- **`Ink`'s helpers in bank 2 too (about 3 KB more):** the machine outliner shares tail fragments of `core`'s size-built code in bank 1, and some of those branched into bank 2. That fails the door check. Turning the outliner off grows all of bank 1.
- **ORBIT's entries as fields of TAPE's `UiDoor`:** one table, but its fields would follow two features' gates, and TAPE's closed check would answer for ORBIT's pages.
- **A door only in the board's shell:** the desktop, the bench and the tests would call the runner directly, so no host test would prove the door's path.

## Consequences
- **Flash**, measured on tape-streaming (54aa17dd) before and after, release builds, bank 1 the `.bin` and bank 2 the `.bk2`:

  | Set | Bank 1 before | Bank 1 after | Bank 1 Δ | Bank 2 before | Bank 2 after | Bank 2 Δ |
  |---|---|---|---|---|---|---|
  | `orbit` | 909,164 | 891,356 | −17,808 | 313,600 | 344,704 | +31,104 |
  | `bench-orbit` | 887,572 | 867,284 | −20,288 | 324,064 | 352,512 | +28,448 |

  Bank 2 grows by more than bank 1 shrinks: `block` and `block_streamed` each carry their own copy of the notes' sending, and the code inlined into the entries is also kept in bank 1 for its other callers. `tools/bank1-budget.txt`'s `orbit` and `bench-orbit` lines come down to the new sizes, so the freed room can't silently refill.
- **Sets without ORBIT** shed its pages: they linked them, unreachable, as SEQ hold never opens ORBIT there. A firmware build without `orbit` now answers every page call closed, so none links. Each set's line comes down 13,872 to 14,384 B: `default` 888,328 → 874,392, `bank2-gate` 889,924 → 876,052, `fault-inject` 862,336 → 848,304, and the bench sets and `filter-probes` alike (each line's comment). Their bank 2 doesn't change.
- The bench's ORBIT rows (`bench-orbit`) now time the runner from bank 2. They need the `.bk2` installed, as the other bank-2 benches do.
- `check-door-placed` checks ORBIT's door module, its raster, `draw::cell_text` and `draw::viz_key` on a build that links them.
- The next crate table takes the slot after ORBIT's (`DOOR_ADDR + 0x300`) the same way.

## Sources
- ADR 0071, 0072, 0082 (ORBIT), 0137 and 0141 (the door, closed labels), 0142 and 0174 (bank 2's cost), 0175 (the crates' slots), 0198 (the freeze); #741.
- `chimera-engine/src/orbit/door.rs`, `chimera-core/src/ui/orbit/door.rs`, `chimera-core/src/ui/orbit/raster.rs`, `chimera-stm32/build.rs`, `chimera-core/tests/orbit_door_test.rs`.
