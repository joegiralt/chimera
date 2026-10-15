# 0142. The reverb engines live in bank 2

- **Status:** Proposed
- **Deciders:** owner (one shipped firmware, the ORBIT build; bank 1 room), firmware

## Context
The one firmware a release ships is the ORBIT build (`chimera-<version>.bin`). Its bank 1 had 2,760 B of the 917,504 B left at `cc2397e8`: no room for the next feature or fix. ADR 0137's door already plays TAPE's DSP from bank 2. Of what could follow it (private notes), the reverb's TYPE engines are the largest seam with one entry: `Reverb::process`, called once a block from `FxBus::process`, reaching the ROOM, HALL, BLOOM, GATE and REVERSE engines, which nothing else in bank 1 calls. No vtables, no shared statics. Moving the whole FX send bus would free more but silence chorus and delay too; the VA or ALGO DSP would silence INIT.

## Decision
- **`Reverb::process` is the door's reverb entry** (`Bank2Door::reverb`, a `ReverbDoor { process }`), `#[inline(never)]`. With `bank2`, `bank2.x` links it into `.bank2` with `rev::{room, bloom, burst, loop_engine}`, `run_loop`'s monomorphs and `rev::ramp`, text and rodata (their switch tables follow their names). Without `bank2` the globs are left out and it stays in bank 1.
- **Bank 1 keeps** the constructors, `force_restart`, `of_type`, the arena, `TailGate`, `RevBill`, `rev::parts` (`pow2`, `lg2`, `Wander`), `rev::sync`, the half-bands, and everything the UI reads (`room::tone_shown_hz`, `DIFFUSER_MS` and the rest are inlined or consts). Bank 2 calls into them; bank 1 never calls into bank 2 but through the door (`door-check`).
- **`Reverb::play` is bank 1's side**, which `FxBus::process` calls: inside `DOOR_CELL.with` it calls the door's `process`; closed, the reverb's wet is silent (`Stereo::SILENT`) and its state waits as it was. Chorus, delay, the master section and the dry path play on. No heap, no blocking, no panic on that path.
- **Closed, the REVERB TYPE reads `NEEDS UPDATE`** (ABOUT's copy, owner decision 8): the focus band whole, the TYPE cell, a column wide, `UPDATE`. `chimera_core::bank2::closed_text` decides it (any `Bank2Status` but `Open`; no status, the desktop, never). Open, nothing changes on screen.
- **The stack check reads `.bank2`'s words** (`-j .bank2` in its data dump), so every door entry, TAPE's and the reverb's, is an address-taken function charged to the pointer calls.
- **The bench can A/B the banks:** `bench-stress` plays the reverb from bank 1, `bench-stress,bank2` the same code from bank 2; both in `bench_sets`, built and door-checked by `just check`.

## Alternatives considered
- **The whole FX send bus (`FxBus::process`) behind the door:** about 25 KB more, but a closed bank 2 silences chorus and delay too; kept for when bank 1 needs it.
- **VA or ALGO DSP:** fits, but INIT (VA) or FM goes silent closed: a near-mute unit.
- **Byte-shaving bank 1:** no seam frees 37 KB; rejected by the flash-counting rule.
- **The UI's helpers (`pow2`, `lg2`, `sync::index_of`) moved too:** the UI calls them, so they would need proxies or copies for a few hundred bytes.

## Consequences
- Bank 1 of the ORBIT build: 914,744 → 841,248 B (76,256 B free); default build 894,060 → 820,740 B. The `.bk2` body grows from 47,520 to 121,152 B.
- While bank 2 isn't open (empty, stale, torn, a dev build with no seal) the reverb is silent and its TYPE says so; a Sound or project with reverb loads and plays dry and with chorus and delay.
- Reverb now runs from bank 2 every audio block. Same flash, same wait states and an MPU region cached as bank 1's, so parity is expected but unmeasured: Gate 3 runs `bench-stress` against `bench-stress,bank2` and compares the REV rows and BUS.
- Every bank-2 function bakes bank-1 addresses, and the reverb calls into bank 1 often (`TailGate`, the half-bands, `pow2`): almost any firmware change changes the seal. Each release therefore ships the `.bk2` beside the `.bin`, to be copied to the card root (ADR 0137's one-action update); "a firmware change that leaves bank 2's bytes alone keeps its seal" is now rare.

## Sources
- (private notes) (candidate B), `docs/superpowers/plans/2026-10-13-bank2.md`; ADR 0137; issue #460.
- `chimera-engine/src/dsp/rev/mod.rs` (`Reverb::play`, `ReverbDoor`), `chimera-engine/src/bank2.rs`, `chimera-stm32/build.rs` (`bank2.x`), `chimera-engine/tests/reverb_proxy_test.rs`.
