# 0157. FUZZ lives in bank 2

- **Status:** Proposed
- **Deciders:** owner (ruling 2026-10-19: FUZZ's kernel behind the door), firmware

## Context
FUZZ (ADR 0156) is a kernel of about 3.3 KB of code and a 2 KB law table, read by one call per voice per block (`Fuzz::run`). Built into bank 1 it took 5,104 B of the ORBIT build's free flash (49,156 to 44,052 B). ADR 0142 put the reverb's engines behind bank 2's door for the same reason, with the same shape: one entry, no shared statics beyond what bank 2 may call into bank 1.

## Decision
- **`Fuzz::run` is the door's FUZZ entry** (`Bank2Door::fuzz`, a `FuzzDoor { run }`), `#[inline(never)]`, with the steady loop `Fuzz::steady`. With `bank2`, `bank2.x` links `voice_fx::fuzz`'s text and rodata (`TABLE`, the law's 2,048 B) into `.bank2` (`*8voice_fx4fuzz*`). Without `bank2` the glob is left out and `play` calls `run` in bank 1 (the desktop, the host tests, the bench sets without `bank2`).
- **`Fuzz::play` is bank 1's side**, which the voice's slot calls: inside `DOOR_CELL.with` it calls the door's `run`. Closed, the block stays dry and the pedal's state waits as it was. No heap, no blocking, no panic on that path.
- **Closed, the FX TYPE reads `NEEDS UPDATE` at FUZZ** (ABOUT's copy): the focus band whole, the TYPE cell `UPDATE`. `chimera_core::bank2::closed_text` decides it through `FxType::in_bank2`; OFF, SOFT and FOLD read their names. A Sound with FUZZ loads and plays dry, with its pots kept.
- **Bank 1 helpers are inlined, not passed.** `fast_exp2` and `level_gain` (through `pot_level`) inline into `run` and `steady`, so the kernel's only reference into bank 1 is the compiler's `__aeabi_memclr4` (the reset on a shut gate), as the reverb's engines call into bank 1. The door-check reports no relocation from bank 1 into bank 2, and `check-door-placed` (the Justfile) fails if `Fuzz::run` or `Fuzz::steady` is linked outside `.bank2`.
- **The host tests** cross the host's own door (`open_host`): `fuzz_proxy_test` compares `play` open with `run` bit for bit, and closed with the dry block. `just test` runs it with the other proxies.

## Alternatives considered
- **Leave FUZZ in bank 1:** 5.1 KB of the ORBIT build's room (ADR 0142's reasoning), while the TYPE has no use beside a pedal.
- **Pass `fast_exp2` and `level_gain` through the door:** two more entries and pointer calls in the sample loop's prologue for a few hundred bytes of duplicated code; both inline.
- **Move the slot's whole `VoiceFx` behind the door:** SOFT and FOLD would go silent closed too.

## Consequences
- Bank 1 of the ORBIT build: 881,956 to 876,572 B, 35,548 to 40,932 B free (5,384 B recovered), measured on the merge of `origin/main` at d5a40607. The `.bk2` body is 170,944 B (the file 171,200 B), about 5.4 KB more than it would be without FUZZ.
- While bank 2 isn't open FUZZ is silent as an effect (the voice plays dry) and its TYPE says so; the other three TYPEs are unaffected.
- The seal changes with every firmware change, as ADR 0142 says; the update installs both banks in one step.
- The unit's bench of FUZZ (Task 13) runs from bank 2, as it ships.

## Sources
- ADR 0137, 0142, 0156; issue #614.
- `chimera-engine/src/dsp/voice_fx/fuzz.rs` (`Fuzz::play`, `FuzzDoor`), `chimera-engine/src/bank2.rs`, `chimera-stm32/build.rs` (`bank2.x`), `chimera-engine/tests/fuzz_proxy_test.rs`, `chimera-core/src/bank2.rs` (`closed_text`).
