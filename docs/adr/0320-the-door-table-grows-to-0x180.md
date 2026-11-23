# 0320. The door table grows to 0x180; the fixed tables after it move 0x80 on

- **Status:** Accepted (2026-11-22); supersedes in part ADRs 0175, 0194, 0208, 0217, 0276 and 0283 where they quote a fixed table's slot (the board's, UI's, bench's, ORBIT's pages', MIDI UI's or ed's address, or `DOOR_ADDR + 0x100` to `+ 0x400`): the slots are this ADR's table now
- **Deciders:** owner (grow the table now, 2026-11-22), firmware (bug batch E)

## Context
Bank 1 is frozen (ADR 0198): new work goes to bank 2 behind doors, and bank 1 keeps only door stubs. Batch E (ADR 0311) added three pieces of real-time code: SYMP's clear pacing (`SymPool::pre_clear`, `KsString::clear_some`, the block's clear room) and the ringing-hit DRUM fold (`DrumEngine::runs`, `Instrument::ringing_kits`). That came to about 1.25 KB of bank 1.

Moving them behind a door failed to link. The door's table (`Bank2Door`) sits at 0x08100100, right after bank 2's 256-byte header. ORBIT's build already filled its 0x100 of room (256 B; 240 B without ORBIT). The board's table follows at a fixed 0x100, so `bank2.rs`'s size assert refused even one more entry. ADR 0283 had already gone round a full table once, by heading the S4 groups with new tables.

## Decision
- **The table's room is 0x180** (`bank2::DOOR_ROOM`), up from 0x100. ORBIT's table is 276 B with batch E's five entries, which leaves room for about 27 more. A second assert keeps eight entries of headroom (`size_of::<Bank2Door>() + 32 <= DOOR_ROOM`), so the next growth asks for the next layout change before the link fails.
- **The fixed tables after it each move 0x80 on.** `bank2.rs`'s constants and `chimera-stm32/build.rs`'s `bank2.x` placement and asserts agree:

  | Table | Old | New |
  |---|---|---|
  | door (`Bank2Door`) | 0x08100100, room 0x100 | 0x08100100, room 0x180 |
  | board (`BOARD_DOOR_ADDR`) | 0x08100200 | 0x08100280 |
  | UI (`UI_DOOR_ADDR`) | 0x08100280 | 0x08100300 |
  | bench (`BENCH_DOOR_ADDR`) | 0x08100300 | 0x08100380 |
  | ORBIT's pages (`ORBIT_UI_DOOR_ADDR`) | 0x08100380 | 0x08100400 |
  | MIDI UI (`MIDI_UI_DOOR_ADDR`) | 0x08100400 | 0x08100480 |
  | ed (`ED_DOOR_ADDR`) | 0x08100480 | 0x08100500 |

  The door itself stays at 0x08100100, so the header's `door_off` (0x100, format 2) is unchanged. The S4 tables (ADR 0283 to 0305) don't move.
- **Batch E's code goes behind `AdmitDoor`**, admission's door. It gains five entries:
  - `clear_avail` (what the block may still clear);
  - `admit_sym` (a Sympathetic note's admission and its clear done ahead);
  - `strike_ready` (a pending strike on a sounding voice, ADR 0311);
  - `ringing_kits` (each Part's kit bill with its ringing hits' runs);
  - `follows` (each hit's live-follow bill).

  Their bodies live in `instrument::sym_admit`. `bank2.x` links that module into an S4 group beside `ed`'s, so S0-S3, already under ADR 0251's 8 KB warning, keeps its room. These are `#[inline(always)]` into them, so none of them has a bank-1 copy:
  - `Voice::sym_pre_clear`, `SymPool::pre_clear` and `KsString::clear_some`;
  - `Voice::sym_strike_adds`;
  - `Voice::drum_runs` and `DrumEngine::runs`;
  - `Voice::drum_follow_extra` and `DrumEngine::follow_state`.

  `check-door-placed` checks both sides: the five entries are in bank 2, and no out-of-line copy of the inlined pieces is in bank 1.
- **With the door closed** (no bank 2, or NEEDS UPDATE):
  - a Sympathetic note is admitted and struck unpaced, as before #306 (MODAL is silent then anyway);
  - nothing is cleared ahead;
  - the kits bill as published, without the ringing-hit fold, and with no follow bill.

  So a closed door can bill a republished ringing hit under its cost, as before #977. The load net (ADR 0273) is the backstop there.
- **A DRUM door of its own** was not needed. The fold reaches `DrumEngine::runs` from admission's entry, inlined.

## Why an old half never runs with a new one
The seal ties the two halves. `bank2-seal` writes the first 8 bytes of the SHA-256 of bank 2's body into both bank 2's header and bank 1's `BANK2_SEAL`. Moving a table changes the body, so the seals differ.
- **An old `.bk2` with a new bank 1** judges `Stale`, and the UI reads NEEDS UPDATE.
- **A new `.bk2` with an old bank 1** judges `Stale` too, and its door never opens. `Door::open` is the only way in, and it takes only `Good`.

`chimera-utils`' `a_bank2_sealed_for_another_door_layout_is_stale_both_ways` holds both directions on two bodies that differ only where the board's table sits. `check_pair` refuses the crossed pairs. Nothing else in the install path names a table's address:
- the boot install and `bank2 install` judge by the seal;
- `chimera-usb.py` carries no address;
- the slot reserves and end markers (ADR 0272) are per S4/S6 slot, untouched;
- `check-slots` still runs on every sealed `.bk2`.

## Consequences
- **Flash:** S0 loses 0x80 (128 B) to the door's room. ORBIT's S0-S3 was already under ADR 0251's 8 KB per-region warning (6,432 B free, 2,208 B with batch E's code in it). The 8 KB floor on the total holds, at about 412 KB free. D2's floor (ADR 0306) is RAM and unchanged.
- **Bank 1:** batch E's growth drops to door stubs: ORBIT's bank 1 is 488 B under the merge base 521712eb (ADR 0311's flash line). Bank 2 grows 4,224 B, its lines raised under this ADR. ORBIT's S0-S3 is left with 2,208 B free.
- **Install:** every unit must install this build's `.bk2` with its bank 1. That is one package, as always (one-step install); a mixed pair is refused, never run.
- **Tests:** `bank2_door_test::the_door_tables_room_and_slots_are_pinned` pins the room and every slot. `bank2.x`'s asserts pin them on the link.

## Sources
- ADR 0137 (bank 2, the seal), 0198 (bank 1 frozen), 0251 (8 KB floor), 0272 (slots), 0283 (S4 tables), 0306 (D2 floor), 0311 (batch E).
- `chimera-engine/src/bank2.rs`, `chimera-engine/src/instrument/admission.rs`, `chimera-stm32/build.rs`, `Justfile` (`check-door-placed`), `chimera-utils/tests/bank2_test.rs`, `chimera-engine/tests/bank2_door_test.rs`.
