# 0175. Put TAPE's card interrupt and TAPE's UI behind the bank-2 door

- **Status:** Proposed (supersedes in part 0174: "No TAPE code moves in this change")
- **Deciders:** owner ("move the whole tape over to bank 2", 2026-10-19), firmware

## Context
ADR 0174 left TAPE's remaining bank-1 code where it was: about 17.7 KB of feature-gated note-path and card-interrupt glue, and about 8 KB of TAPE UI that is generic over the display or the card store. Neither had a seam one door entry could name. The door table (`Bank2Door`) lives in chimera-engine, so it can't name chimera-board's card interrupt or chimera-core's UI. The ORBIT build had 24,488 B of bank 1 free on tape-streaming. The owner's unit build with main merged has 10,120 B, and main is still adding VA modes and FX. The owner ordered the move and authorised the restructuring it needs.

Measured on the ORBIT ELF (`rust-nm -S`; bank 2 with and without `tape`, as in 0174), TAPE's share of bank 1 was:
- **The card interrupt** (`chimera_board::card_dma`: `IrqState`'s steps, the bursts, the stops and drains, the serve's pick, the EDF): about 12 KB. Only TAPE's streams use it, plus the bench's timed reads.
- **The UI**: the strip's raster and overview (`tape_strip`, 3.1 KB), the SOURCE cell's turns and card work (`source_cell`, 5.1 KB, its store work generic over `S: Store`), and the glyph's inputs (`TapeGauge::from_set`, 2.7 KB).
- **The note path**: `handle_streamed::<Streams>`, `start_waiting::<Streams>`, the block and `render_knobs`. These are every engine's note and block code, monomorphised over the stream ports. TAPE's own part of them is only about 1.1 KB more than the `NoPorts` build: the `Streaming` bookkeeping and the port it builds, inline on every note. The rest of 0174's 17.7 KB is inlining that moves around (`render_knobs` out of line, the block smaller by about as much).

## Decision
- **Each crate the engine can't name keeps its own table, in its own fixed slot after the door.** `bank2.x` puts `.bank2.door.board` at `__bank2_door + 0x100` (0x08100200, `BOARD_DOOR_ADDR`) and `.bank2.door.ui` at `+ 0x180` (0x08100280, `UI_DOOR_ADDR`), and asserts both addresses. Bank 1 reaches a slot only through `DoorCell::with_at`, which answers only while the cell holds the judged `DOOR_ADDR`. On a host, `open_host`'s door is a plain static with no slots, so `with_at` returns `None` there. Each table starts with a magic number, and `open_door` checks it with the probe. The seal covers the slots, as it covers the door.
- **The card interrupt's work is bank 2's** (`card_dma::CardDoor { step, watch }`, in `BoardDoor`). Bank 1 keeps `init`, the `DMA2_STR0` shim (the clock pair, the counters, `events`, `take_abort`, one door call) and `closed`. With bank 2 closed, `closed` runs instead of the step. It requeues a retry, and if a burst is in flight it grounds it (`ABORT`), drives CS high, gives the bus back and requeues the claim, with no CMD12. For that path bank 1 keeps `Held::op`/`run`/`deselect`, `Flying::ground`, `give` and `CardSide::requeue`; `CardSide::fill_done` stays too, since the bench's TAPE rig calls it. A gate build's `bank2 erase` is the only thing that closes a live door. It now waits, inside its TAPE-off budget, for the bus to be free of streams before the door closes.
- **TAPE's UI is bank 2's** through `chimera_core::bank2::UiDoor`:
  - the strip: `tape_strip::raster` (`strip_in`, `key_in`, `Overview::of_in`) draws onto `Paint`, a display behind a `dyn` (rect and line), so one function serves every display;
  - the SOURCE cell's turns, picks, loads, `kind`, re-heads and card jobs: `source_cell::card`, over `&mut dyn Store`. `read_head`, `SourceLoad::step`/`step_staged`, `TapeList::list` and `StreamLoad::step` take `&mut dyn Store` now;
  - `StreamLoad::start`/`found`/`rehead` and `load_kind`;
  - the glyph's inputs: `tape_glyph::draw::inputs`, through the engine's `GlyphDoor`.

  Bank 1 keeps the wrappers, the overview's cache, the cell's state and memory, and the library's source staging.
- **Closed, TAPE's UI says so and does nothing.** The SOURCE cell reads `NEEDS UPDATE` (`SourceState::Failed`), and it doesn't turn or load. A card job queued before the door closed is dropped, but a library load's staging is kept. The strip and its overview are empty, and the glyph isn't drawn. The header and MODEL keep 0141's labels. Without `bank2` (the desktop, the host tests), the UI calls its own table, `UiDoor::OWN`, and nothing changes. Without `tape` (the bench sets that leave it out) the UI's table is as closed, unlabelled (0141), and its code links out: `bench-routing` goes from 568 to 8,968 B free, where `dyn Store` alone would have overflowed it by 240 B.
- **The note path stays in bank 1.** Other engines' notes never run TAPE code there. Streamed bills, the stealing pick and the re-bill already go through `AdmitDoor` (bank 2), and only for a TAPE Part. What is left is about 1.1 KB of inline bookkeeping that every note does. Moving it would send every engine's note through the door, against 0141.
- **Also staying in bank 1** (TAPE-named, not TAPE's DSP or UI):
  - the source pool, records and source files (`Pool::source_*`, `Records`, `source_file`, about 7 KB), and the TAPE params' specs and codec. With bank 2 closed a project's TAPE Part must still load and save byte for byte (0141);
  - the console's source `put`, a file transfer;
  - the card store's `runs`, a `Store` method its vtable names.
- **Globs name whole functions** where the machine outliner shares a closure's call with bank 1 (`OUTLINED_FUNCTION_*` tail-calls). `door-check` fails the build on any such call, as it does on every other bank-1 reference into bank 2.

## Alternatives considered
- **Re-home the strip and the SOURCE cell into chimera-engine**, as the glyph raster was (0173). The engine has no display, font or `Project` UI types, so all of them would have to move, or be copied as canvases. The crate's own table at a fixed slot reaches the same code with fewer moves.
- **Name board and UI functions in the engine's door through `extern` statics.** That doesn't link in the engine's host tests, and Rust has no stable weak linkage.
- **Move the note path's TAPE branches.** About 1.1 KB, and every note of every engine would pass through the door (0141).
- **Disable the machine outliner** (`-enable-machine-outliner=never`). Bank 1 grows everywhere; whole-function globs are enough.
- **Keep the card interrupt in bank 1 for latency.** Bank 2 is the same flash, with the same wait states, and the MPU's `Open` region is cached as bank 1 is. Gate 3 measured bank 2 never slower (0142). The shim adds one door call to each entry: the cell's load, a DMB, a compare, the slot's entry and a `blx`, about 10 cycles (about 20 ns at 480 MHz). The interrupt's own counters (`irq_us_max`, `irq_own_us_max`, `STREAM_STATS.irq_cycles`) wrap the door call, so they now time bank 2's code.

## Consequences
- **Flash, ORBIT build** (`just firmware-orbit`, from tape-streaming at 4d493993):
  - bank 1: 893,016 → 871,768 B, so 24,488 → 45,736 B free (+21,248 B);
  - the `.bank2` section: 159,776 → 182,848 B (+23,072 B).
  - By step (bank 1 free): the card interrupt about +10.3 KB, the UI and the glyph's inputs +10.1 KB, the streamed load's start and `load_kind` +0.9 KB.
- **Timing.** No audio-thread code moved. The card interrupt runs from bank 2. On the next unit run, `bench-tape,bank2`'s IRQ share (the streams' `irq_cycles`, `irq_own_us_max`) is its row.
- **With bank 2 closed**, nothing streams. The card interrupt's entries then only clear their flags, and the SOURCE cell reads NEEDS UPDATE. A tester's TAPE Part keeps its Sound and source (0141). ALGO, MODAL and VA play on.
- **The stack check charges `DMA2_STR0`'s door call its known target** (`tools/stack_depth.py`'s `DOOR_CALLS`: `IrqState::step`), not every address-taken function's 14 KB, which no card-interrupt entry can reach.
- **The bench's timed reads need bank 2 open** in a `bank2` bench build, as its `.bk2` already must be installed (0174).
- **A new door entry outside the engine** goes in its crate's table, never in a new slot, unless `bank2.x` and `chimera_engine::bank2` add the slot together.

## Sources
- ADR 0137, 0141, 0142 (Gate 3: (private notes)), 0172, 0173, 0174; `docs/superpowers/specs/2026-10-11-bank2-design.md`.
- `chimera-stm32/build.rs` (`bank2.x`), `chimera-engine/src/bank2.rs` (`with_at`, the slots), `chimera-board/src/bank2.rs` (`BoardDoor`), `chimera-board/src/card_dma.rs` (`CardDoor`, `closed`), `chimera-core/src/bank2.rs` (`UiDoor`), `chimera-engine/tests/glyph_door_test.rs`.
- Measurements: `rust-nm -S` and `rust-size -A` on the ORBIT ELF before and after, and with `bank2` without `tape`.
