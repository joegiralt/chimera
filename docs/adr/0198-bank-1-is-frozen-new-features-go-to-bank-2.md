# 0198. Bank 1 is frozen; new features go to bank 2

- **Status:** Proposed (supersedes in part 0163: its ~1.5 KB threshold, ruling R8, under which smaller new code stayed in bank 1; and 0177's line citing it)
- **Deciders:** owner (2026-10-21: "stop putting stuff on bank one. It's ridiculous. Everything should be moving forward on bank two."), firmware

## Context
Bank 1's firmware area holds 917,504 B: the stock bootloader's SD flash erases itself at that size (`just check-bin-size`). Bank 2 (0x08100000–0x0817FFFF, 512 KB) sits behind one sealed door (0137) and already plays TAPE, the reverb, VA's MOD path, the LFO raster, FLT › FX's curve, FUZZ, FB and SEM. Gate 3 measured the reverb from both banks and found bank 2 never slower (0142, 0174). Every install carries bank 2 (0137's one-action update), and tape-streaming makes it required in every firmware build (0194).

Each move so far was a rescue: a feature landed in bank 1, a build stopped fitting, and the feature was moved behind the door afterwards (0142, 0157, 0163, 0174, 0194). 0163's rule sent only new DSP over about 1.5 KB to bank 2, so smaller additions kept filling bank 1. At origin/main `f745c0a1`, the ORBIT build (the one firmware a release ships) has 31,400 B of bank 1 left.

## Decision
- **Bank 1 is frozen.** New code lives in bank 2 behind a door: a `Bank2Door` field of its own (as `SemDoor`, `FuzzDoor`, `ReverbDoor`), its text and rodata placed by a `bank2.x` glob, its symbols listed in `check-door-placed`. This applies to every new feature, DSP or UI, of any size.
- **Bank 1 may gain only:**
  1. **Door stubs and a closed door's data:** the bank-1 side of an entry (a `play`/`process` wrapper inside `DOOR_CELL.with`, its closed-door fallback, a `Bank2Door` field, the `NEEDS UPDATE` labels of 0141), and the least data the instrument needs while the door is closed so a Sound using the feature still loads, saves, maps and shows it: the feature's param specs, its codec's disk codes, its page and panel tables and its closed-door labels (as SEM's TYPE and MODE today, and VA NOISE's ~400–500 B on #734). This is data, not code: tables and strings, and no functions beyond the stubs. The feature's ADR measures it (each set's bank-1 bytes before and after) and lists what it is, and the PR raises each set's line by that measured amount, the comment naming that ADR.
  2. **Code that must run before bank 2 is open:** the boot and reset path, clocks and the MPU, DFU and the update path, the judge, the door check and bank 2's installer (including the card and FAT reads it needs), the panic and fault handlers, and the text a closed bank 2 has to show (0137: the fonts stay in bank 1 to say `NEEDS UPDATE`).
  3. **The audio interrupt's hot path, where bank 2's cost is measured and unacceptable:** a bench row from each bank (as Gate 3's), recorded in the exception's ADR. Bank 2's being slower is never assumed.
  4. **Toolchain or dependency drift:** a `rustc` or crate update that grows bank 1 with no new code. The budget line names the version.
  5. **Fixes** to existing bank-1 code, within the guard's slack. A fix that needs more is moved behind a door, or it qualifies under 2 or 3.
- **Each exception gets a line in an ADR** (this one's list, or a later ADR that adds one; for a closed door's data, the feature's own ADR), and the budget line it raises names that ADR.
- **The guard is `just check-bank1 <set>`.** It is run by `just check` after each bank-2 firmware build (default, `orbit`, `bank2-gate` and every `bench_sets` entry). It cuts the ELF's bank 1 as `bank2-split` does (`rust-objcopy -R .bank2`) and checks its size against `tools/bank1-budget.txt` with `tools/bank1_budget.py`:
  - A set more than 64 B over its line fails. The slack covers alignment and inlining noise. The message says to move the code behind a door, or, for a closed door's data, to raise the line by what the feature's ADR measured.
  - A set more than 1 KB under its line also fails, telling you to lower the line. Room freed by a move stays bank 2's: past 1 KB, the next feature can't quietly take it back. A PR that moves code out lowers its lines in the same PR: `BANK1_LOWER=1 just check` writes each set's measured size into its line as it builds it (it only ever lowers; a set over its line still fails).
  - Every line carries a `# ADR NNNN` comment, and a line without one fails to load. A raise is an edit to that file naming the exception's ADR. Lowering a line needs only the comment that is already there.
  - `tools/test_bank1_budget.py` checks the tool and that the file covers exactly the sets `check` measures.
- **Sets without bank 2 carry no budget.** On main these are `--no-default-features` and `sd-probe`: code moved behind a door lands in their bank 1, so they would punish the move. 0194 gives `--no-default-features` bank 2, and it takes a line then.
- **The budgets are measured, not reasoned about.** They are `just check`'s plain release builds at origin/main `eb0cdf0f`, after #734 (bank-1 `.bin` bytes):

  | Set | Bank 1 (B) | Free of 917,504 (B) |
  |---|---|---|
  | `orbit` | 886,120 | 31,384 |
  | `default` | 865,716 | 51,788 |
  | `bank2-gate` | 867,600 | 49,904 |
  | `fault-inject` | 838,924 | 78,580 |
  | `bench-algo,bank2` | 877,564 | 39,940 |
  | `bench-algo,master-tape,bank2` | 873,528 | 43,976 |
  | `bench-routing,bank2` | 874,816 | 42,688 |
  | `bench-memory,bank2` | 863,652 | 53,852 |
  | `bench-stress,bank2` | 877,288 | 40,216 |
  | `bench-orbit,bank2` | 868,828 | 48,676 |
  | `bench-tape,bank2` | 848,168 | 69,336 |
  | `filter-probes` | 892,932 | 24,572 |

## Alternatives considered
- **Keep 0163's ~1.5 KB threshold:** that is how bank 1 filled. Many small additions under the threshold add up to the same overflow, followed by another rescue move.
- **Guard only the free-space floor (fail under ~40 KB free):** this allows growth until the floor, then forces an emergency move. The owner wants every new feature in bank 2 from the start, not a floor.
- **A per-PR diff budget instead of a checked-in file:** it needs a base build in every check, doubling the firmware builds. It also can't record which ADR allowed a raise.
- **Guard the release builds only (`release-bin`, path-remapped):** the bench sets would escape the guard, and the bench harness is where bank 1 overflowed last (0194).

## Consequences
- A feature PR that touches bank 1 by more than the slack fails `just check` until its code is behind a door or an ADR names its exception. #734 (NOISE, 0197) merged just before the freeze with its code behind `NoiseDoor`: its closed-door data (+592 B on the default build) is in the baseline at `eb0cdf0f`, so it needs no exception line. #739 (LPG) lands with its code in bank 2 and raises the lines only by its closed-door data, measured in its ADR.
- A closed bank 2 silences or labels more of the instrument as features move. This is acceptable because every install carries the `.bk2` (0137, 0194) and closed doors already say `NEEDS UPDATE` (0141).
- The budgets are the owner's machine's plain builds. Cargo registry and rustup paths in panic messages are in the `.bin`: 25 of them in bank 1 (bank2-gate at `f745c0a1`), so each character of `$HOME` length moves a set by about 25 B, and a `$HOME` three characters longer than `~` fails the slack. The checkout's own path is not in it (workspace paths are relative; a worktree at another path measures the same bytes). The toolchain is unpinned nightly, so a `rustup update` moves every line (point 4). A second build machine needs its own measurement, or the check moves to path-remapped builds as `release-bin`'s.
- When tape-streaming next merges main, it takes this file. 0194 renames the sets (`bench-algo,bank2` becomes `bench-algo`) and gives `--no-default-features` bank 2, so its merge re-measures the lines under this ADR.
- What can move out of bank 1 today, largest first, is planned in private issue #741. Each move lowers its set's lines in the same PR.

## Sources
- Owner, 2026-10-21. ADR 0137 (the door), 0141 (closed-door labels), 0142 and 0174 (Gate 3: bank 2 never slower), 0163 (the ~1.5 KB rule), 0194 (bank 2 required, on tape-streaming).
- `tools/bank1_budget.py`, `tools/bank1-budget.txt`, the Justfile's `check-bank1` and `check`.
- Inventory: (private notes) (`rust-nm --size-sort` on the ORBIT ELF at `f745c0a1`).
