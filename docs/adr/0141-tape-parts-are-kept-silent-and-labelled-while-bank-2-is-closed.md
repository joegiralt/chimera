# 0141. Keep TAPE Parts as TAPE: silent and labelled while bank 2 is closed

- **Status:** Proposed
- **Deciders:** owner (bank 2 plan), Claude (implementation)

## Context
ADR 0132 point 4 turned a project's TAPE Part into a clean INIT VA on load,
and made the load lossy, so a save dropped the TAPE Sound. That was right
while no firmware had TAPE's DSP. With bank 2 (ADR 0137) TAPE's DSP ships
in bank 2 and plays through the door. When bank 2 is closed (empty, stale,
torn, never sealed), TAPE can't play. Converting the Part would then throw
away a tester's Sound because of a missing file.

## Decision
1. **Load.** A TAPE Part loads as TAPE: its Sound and Origin as saved. The
   load is not lossy because of it, so a save writes the Part back byte for
   byte (`ProjectCheck::finish`, `codec.rs`).
2. **Silent.** While bank 2 isn't open, the TAPE proxy refuses every note
   and plays nothing (ADR 0137, Task 11). The other Parts play.
3. **Labelled.** Bank 2 is closed when its status is set and isn't `Open`
   (`chimera-core/src/bank2.rs`). When closed:
   - every page of a TAPE Part (`Head::Sound`/`Head::Mix` of that Part)
     warns `UPDATE` in the header, before any OUT warning
     (`bank2::closed_part`). `NEEDS UPDATE` (94 px) doesn't fit beside
     `PART n · SOUND` and a page name, so the header gets the last word;
   - DECK's MODEL reads `NEEDS UPDATE` in the focus band and `UPDATE` in its
     cell (`bank2::closed_text`, the same rule as the reverb's TYPE, ADR
     0142).
4. **No bank 2, no label.** A build without the `bank2` feature plays TAPE
   (or not, without `tape`) and the reverb from bank 1. The board sets no
   status there (`ui.bank2()` is `None`), so nothing reads NEEDS UPDATE.
   ABOUT shows no bank-2 line, `status` prints none, and the `bank2`
   command answers that it isn't in the build. `Unsealed` now means a
   `bank2` build that was never sealed, which can't open bank 2.
5. **Desktop.** `CHIMERA_BANK2=closed` sets `Empty`, for QA of the labels.
   Unset, the desktop has no bank 2 (`None`), which reads as open. The
   desktop plays TAPE either way.
6. ADR 0132 points 1–3 stand: the browser's TAPE row and pool TAPE Sounds
   read COMING SOON, LIBRARY lists no TAPE Sound, NEW holds no INIT TAPE.

This supersedes ADR 0132 point 4.

## Alternatives considered
- **Keep the INIT VA conversion.** A missing `.bk2` would cost the tester
  their Sound on the next save.
- **`NEEDS UPDATE` whole in the header.** It needs 94 px; beside the
  context (97 px) and the shortest page name it overruns the line.
- **Replace the page name with `NEEDS UPDATE`.** Hides where the player is.
- **The Part strip's mark (SETTINGS › PART) reads `NEEDS UPDATE`.** That
  mark is the Part's save state; overwriting it hides an edit.
- **A `bank2` feature in chimera-core for "closed".** Host tests would need
  the feature to see the labels. A status of `None` already means "no bank
  2 here".
- **Desktop default `Open`.** It needs a made-up version and seal. `None`
  reads the same.

## Consequences
- An old project with a TAPE Part round-trips whole on every build.
- The bench builds without `bank2` and without `tape` play TAPE Parts
  silent with no label. They are bench builds only.
- Turning MODEL while closed still changes the model; the readout keeps
  saying NEEDS UPDATE until bank 2 opens.
- `header_text` takes the closed warning; `page_header` and
  `UiState::page_header` take or read the bank-2 status.

## Sources
ADR 0132 (TAPE coming soon), ADR 0137 (bank 2 behind one door), ADR 0142
(reverb in bank 2, the closed-label rule), bank-2 plan Task 12
(`docs/superpowers/plans/2026-10-13-bank2.md`),
`chimera-core/tests/tape_part_load_test.rs`.
