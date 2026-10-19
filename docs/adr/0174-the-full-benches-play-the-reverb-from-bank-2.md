# 0174. Build the ALGO, STRESS and ORBIT benches with bank 2

- **Status:** Proposed (supersedes in part 0142: its bank-1 vs bank-2 reverb A/B bench)
- **Deciders:** firmware (#654), owner to rule on the next bank-1 move

## Context
After main was merged into tape-streaming (#655), bank 1 of `firmware-orbit` had 24,488 B free, and four bench builds no longer linked: `bench-algo` (FLASH over by 3,224 B), `bench-algo,master-tape` (12,752 B), `bench-stress` (5,548 B) and `bench-orbit` (7,636 B). Those builds have no `bank2`, so they carry the reverb's TYPE engines in bank 1. The shipping builds play the reverb from bank 2 (0142).

The plan for #654 was to move TAPE's remaining bank-1 code behind the door. Measured at 16fd30af, that can't reach the 45 KB target:
- TAPE's deck, its spans and the stream cursor, streamed admission, the loads' passes and the glyph raster are already in bank 2 (0137, 0142, 0173).
- An ORBIT build with `bank2` but without `tape` has 875,260 B in bank 1, against 893,016 B with it. So all of TAPE's feature-gated code in bank 1 is 17,756 B. Part of the symbol diff is only inlining: with `tape`, `Voice::render_knobs` (20,060 B) sits out of line, and the block shrinks by about as much. The rest is spread over the note path (`handle_streamed`, `start_waiting`, `pick_where`), the card interrupt (`IrqState::fly`), the card store and the UI's update. All of it is audio-thread or card-interrupt glue, with no seam to put behind one door.
- TAPE's code that isn't feature-gated (the strip, `SourceCell`, the source records, the specs, about 8 KB) is generic over the display or the card store. A door entry can't name those types from chimera-engine, so moving it means re-homing it there, as the glyph raster was.
- The bench builds have no `tape`, so moving TAPE frees nothing in them.

## Decision
- **`bench-algo`, `bench-algo,master-tape`, `bench-stress` and `bench-orbit` build with `bank2`** in `bench_sets` (`bench-algo,bank2`, `bench-algo,master-tape,bank2`, `bench-stress,bank2`, `bench-orbit,bank2`), as `bench-stress,orbit,bank2` already does. Their reverb rows time bank 2's code, which is what ships, and each run needs its build's `.bk2` installed (`flash-bench` says so).
- **The bank-1 reverb A/B is retired.** Gate 3 (2026-10-15) measured the REV rows from bank 1 against bank 2 (QUAD 77→54, GATE 160→148, RVRS 171→151, ROOM 250→243, HALL 261→256, BLOOM 274→273): bank 2 is never slower. `tools/test_justfile.py` now checks that every ALGO, STRESS and ORBIT set carries `bank2`. A build without `bank2` still plays the reverb from bank 1 (the desktop, the host tests, bench-routing and bench-memory).
- **No TAPE code moves in this change.** TAPE's bank-1 share is the glue above. Bank 1's room comes back through the non-TAPE seams instead.

## Alternatives considered
- **Move TAPE's remaining bank-1 code through the door.** At most about 8 KB is cold enough: the strip, `SourceCell` and the records. Each piece needs re-homing into chimera-engine behind a type-erased entry, which is several door entries to gain about a fifth of the gap. The rest is note-path and card-interrupt code shared with every engine. Closed, it would silence all notes, not only TAPE's.
- **Gate TAPE's UI out of builds without `tape`.** It frees about 8 KB in the benches, but not the 12,752 B that `bench-algo,master-tape` lacks. It also adds cfg seams to UI code that the desktop and tests always build with TAPE.
- **Drop the overflowing sets.** That loses the ALGO kernel's loop counts, the STRESS rows and ORBIT's block cost.

## Consequences
- **Bench margins** (bank 1 free, 917,504 B limit): `bench-algo,bank2` 8,068 B, `bench-algo,master-tape,bank2` 31,316 B, `bench-stress,bank2` 6,336 B, `bench-orbit,bank2` 37,312 B.
- **The ORBIT build is unchanged:** bank 1 has 24,488 B free (893,016 B) and the `.bk2` body is 159,520 B. That is under the ~40 KB alarm, and TAPE can't lift it. The next bank-1 room has to come from a non-TAPE move: the VA MOD DSP door (0163, in flight), or the FX send bus behind the door (about 25 KB, at the cost of closed bank 2 silencing chorus and delay; 0142's alternative). That is the owner's call, and #654 tracks it.

## Sources
- #654, #655; ADR 0137, 0141, 0142 (Gate 3: (private notes)), 0163, 0173.
- Measurements: `rust-nm -S` on the ORBIT ELF at 16fd30af, with `bank2` with and without `tape`.
