# 0251. Below 8 KB of bank 2 free, a feature may go in bank 1

- **Status:** Proposed (supersedes in part 0198: its rule that every new feature lives in bank 2)
- **Deciders:** owner (2026-11-08: "do both", these cuts and this policy), firmware

## Context
Bank 2 is 0x08100000–0x0817FFFF, 524,288 B; bootloader 1.09 owns 0x08180000 and up. At origin/main `3633c52c` the ORBIT build (`--features orbit`, `CHIMERA_STAMP_CLEAN=1`) ends bank 2 at `__bank2_end` 0x0817F6A0: 2,400 B free. Pending 0.3 work needs about 7 KB for #887 alone. Bank 1 meanwhile has room: the ORBIT build's bank 1 is 836,840 B of 917,504 (the bootloader's SD flash), 80,664 B free.

Three cuts free bank 2 without changing a sample (goldens bit-identical):

| Cut | Bank 2 freed (B) |
|---|---|
| Rest images built in place: `ui::midi`'s `REST`, `echo::SILENT`, `layer::REST`, `midi_map::ctl::EMPTY`, `tuning::LINEAR`, `ui::tuning::TWELVE` (statics gone, and the copies of them inlined at each use) | 12,038 |
| `bank2::TABLE` 4,096 B to 16 B | 4,080 |
| One MIDI parser: DIN's and USB's pipe run on one sink type (`PipeSinks`), the ring's size a value | 2,850 |

After them `__bank2_end` is 0x0817ACA0: 21,344 B free. Bank 1 is 836,152 B (81,352 B free); RAM sections are unchanged.

`TABLE` was Phase 1's test payload (plan 2026-10-13: "a test `.bk2` that holds only a data table"; Gate 2's "probe + 4 KiB table"), so the first installs had data behind the door to write and read back. It never showed the image's extent: it sits at 0x08100448, near the start. The seal (SHA-256 of the body) and the CRC prove every byte; the door check's first and last table bytes prove only that data reads through the door, at any length.

TAPE's idle images (`stretch::IDLE` 408 B, `grain::IDLE_NOTE` 408 B, `cloud::IDLE_CLOUD` 328 B) stay: they are copied on TAPE's note-on, in enums and struct updates, where building in place is awkward and close to the audio.

## Decision
- **Bank 2 first, while there is room.** A new feature still goes to bank 2 behind a door (0198) while the ORBIT build's bank 2 keeps at least 8 KB (8,192 B) free, measured as 0x08180000 − `__bank2_end`.
- **Bank 1 when bank 2 is full.** A feature that would leave less than 8 KB of bank 2 free may go in bank 1 instead. It still passes the bank-1 guard (`just check-bank1`, `tools/bank1-budget.txt`): each raised line names this ADR and the feature, `; ADR 0251 +N <feature> (from X)`. Bank 1's limit stays 917,504 B.
- **A rest image is built in place, not copied from a static.** Bank 2 code builds a state at rest with a few stores into its place (`MaybeUninit`, `addr_of_mut!`), in one out-of-line bank-2 builder. A builder called from many places in `chimera-core` (opt-level `z`) is called through its door table's field at the fixed address, never by name: the machine outliner shares a repeated call from a bank-1 outline, and `door-check` fails it (0175). `MidiSlot::build` is the first.
- **The door table is 16 B** (`TABLE_LEN`), checked at its first and last bytes as before.

## Alternatives considered
- **Keep bank 1 frozen and cut bank 2 again later:** the cuts above are the cheap ones; the next room costs sound-identical rewrites of the big kernels.
- **Move features back to bank 1:** churn with no gain while bank 2 has room.
- **A trait object for the two MIDI rings:** its vtable is anonymous rodata, which lands in bank 1 and points into bank 2.

## Consequences
- 0.3's pending features fit in bank 2 for now (21,344 B free, 13,152 B above the floor).
- Once bank 2 is near the floor, bank 1's 81 KB is the room, through the same guard.
- Bank 1 runs bank-2 entries only through doors; a bank-2 builder reached through its own table costs one indirect call when first used.

## Sources
- ADR 0137, 0175, 0198; `docs/superpowers/plans/2026-10-13-bank2.md` (Phase 1, Gate 2).
- Measurements: orbit release builds of origin/main `3633c52c` and branch `bank2-room`, `CHIMERA_STAMP_CLEAN=1`, `rust-nm`, `rust-objcopy -R .bank2`.
