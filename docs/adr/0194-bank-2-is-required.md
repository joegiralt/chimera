# 0194. Bank 2 is required

- **Status:** Proposed (supersedes in part 0141 point 4, 0142, 0157, 0163, 0174 and 0177: each one's firmware build without `bank2`)
- **Deciders:** owner (2026-10-21), firmware

## Context
Bank 2 began as an optional feature (0137): `chimera-stm32`'s `bank2`, in `default`, so `--no-default-features` built a firmware that played TAPE, the reverb, VA's MOD path, the LFO raster, FUZZ, FB and SEM from bank 1. Each move into bank 2 since (0142, 0157, 0163, 0175, 0177, 0190, 0192) kept that path working and checked it in `just check` and `stack-check`.

After main was merged into tape-streaming (#723, at 7e2cd6a3) that build no longer fit FLASH: 11,664 B over. Three bench builds overflowed bank 1 even with `bank2`: `bench-algo,bank2` by 4,316 B, `bench-stress,bank2` by 4,336 B and `bench-routing,bank2` by 1,024 B. `bench-algo` also overflowed RAM_D2 by 924 B.

Every real install includes bank 2: a release is the `.bin` and its sealed `.bk2` (0137), and every bench run already needed its `.bk2` installed (0174). A build without bank 2 only exercises a configuration no one runs.

## Decision
- **Bank 2 is in every firmware build; it is not a feature.** `chimera-stm32` has no `bank2` feature: its `chimera-board`, `chimera-core` and `chimera-engine` dependencies carry `bank2` themselves, and `build.rs` writes every `bank2.x` glob and door slot unconditionally. `--no-default-features` now means the fewest features with bank 2. `bank2-split` fails an ELF with an empty `.bank2`. `tools/test_justfile.py` checks that `bank2` stays out of the firmware's features.
- **No code is cut to make a build fit.** The firmware without bank 2 is no longer built or checked. `just check`, `stack-check` and `clippy` keep `--no-default-features` as the fewest-features build, which now has bank 2. Their bench sets drop `,bank2` because it is implied now.
- **The bench is bank 2's.** `chimera_stm32::bench`'s text and rodata, and its tables of rows (`PATCHES`, `VA`, `FORMANT`, `FX`, `REV`, `ROUTING`, `TAPE_ROWS`, `STREAM_ROWS`, as `.bank2.rodata.bench` statics), link into bank 2 in a bench build. Bank 1 enters through `BenchDoor` (`enter`, magic `B2BN`) at `BENCH_DOOR_ADDR` (0x08100300), after the UI's table. When the door is closed, the bench doesn't run and the console's `bench` says so. Bank 1 keeps `main::bench` and `streamed_frames`, the one closure whose `dyn Fn` vtable bank 1 holds. The code the bench times stays where the shipping build has it. Only the harness moves, and Gate 3 found bank 2 never slower (0142).
- **The stack check charges the bench's entry only where it is entered.** Only the door's table holds `bench::enter`'s address, so `tools/stack_depth.py` leaves it out of the address-taken functions (`DOOR_ONLY`). Its caller, `main`'s `bench` (inlined into `synth`), charges its pointer calls the deeper of the entry and the usual charge. Without this, every pointer call would be charged the whole bench, the audio interrupt's included, though the bench runs before the audio starts.
- **The bench's scope moves to AXI** (`.uninit`, NOLOAD), out of RAM_D2, so `bench-algo`'s kernel rows fit D2. The bench never times the scope.
- **Host builds keep `bank2` as a feature.** The desktop and the host tests run without it, and the proxy and door tests run with it (`--features bank2,test-support`). Host `--no-default-features` test runs stay.

## Alternatives considered
- **Cut code until the no-bank-2 build fits:** the owner ruled it out. It spends bank 1 on a build that never ships.
- **Keep `bank2` as a default feature, and only stop building without it:** `--no-default-features` would quietly build the no-bank-2 firmware again. Removing the feature makes that impossible.
- **Move the card bench (`card_speed`, 16.9 KB) instead:** it is reached from the console's `card` in bank 1 and needs a board door entry. Moving `bench` alone is enough for every set.

## Consequences
- Bank 1, with bank 2 required (B free of 917,504): default 33,396; `orbit` 12,584; `--no-default-features` 99,252; `fault-inject` 59,132; `bench-algo` 14,812; `bench-algo,master-tape` 36,956; `bench-routing` 18,772; `bench-memory` 16,232; `bench-stress` 14,060; `bench-orbit` 34,232; `bench-tape` 51,836; `filter-probes` 6,084. Every one passes `door-check`.
- A bench build needs its `.bk2` installed before it can run at all. Before, it ran with the reverb and TAPE silent.
- Firmware code may assume the door's tables are linked. Only the door's open/closed state varies at run time (0141's labels).
- The no-bank-2 firmware paths that 0141 point 4, 0142, 0157, 0163, 0174 and 0177 describe are now host-only (the desktop, the host tests).

## Sources
- PR #723 (merge main into tape-streaming), its `just check` at 7e2cd6a3.
- ADR 0137 (bank 2), 0142 (Gate 3: bank 2 never slower), 0174 (benches with bank 2), 0175 (the board and UI door slots).
