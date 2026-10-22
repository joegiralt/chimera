# 0213. Move the TAPE delay's and the chorus's loops to bank 2

- **Status:** Proposed (amends 0202's closed door: GRAIN no longer plays as TAPE closed, the delay is silent)
- **Deciders:** owner (#793: 0.3's efficiency wins, the sound unchanged), firmware

## Context
#793's bus audit (private notes) found the TAPE delay's and the chorus's sample loops inlined into `instrument::mix_parts` in bank 1: about 3.6 KB and 2.2 KB. Bank 1 is frozen and full (ADR 0198), and the idle skips #793 adds to both loops are new code, so they can't land there. The reverb (0142), GRAIN (0202) and FUZZ (0157) already run once a block through the door.

## Decision
- **One door call a block each.** `Bank2Door::delay` (`DelayDoor { run }`, replacing 0202's `GrainDoor`) and `Bank2Door::chorus` (`ChorusDoor { process }`). `TapeDelay::run` and `JunoChorus::process_wet` are bank 1's side, as `Reverb::play` is: with `bank2` they call the door; without it they call the kernel directly (the host, and firmware without bank 2).
- **What moves.** `delay::tape` (`run`: the TYPE dispatch to `grain::run`, the TIME fade's start, the eases, and TAPE's four `span` loops) and `chorus::kern` (`process`: the eases and every mode's loop). `bank2.x` places them with GRAIN's glob (`*5delay4tape*`, `*6chorus4kern*`), and `check-door-placed` checks `dsp::delay::tape::` and `dsp::chorus::kern::`. The params, specs, `Tap`, `Transport` and the eases stay in bank 1. TAPE's voices use `Tap::speed_cents` and `Transport` too.
- **Closed, both are silent.** The delay's return is zeros (an insert passes its dry), and the chorus returns `Stereo::SILENT`. Their state is left alone, as the reverb's is. The delay's TYPE and the chorus's MODE read `NEEDS UPDATE` at every value (`bank2::closed_text`). Under 0202, a closed GRAIN played as TAPE because TAPE's loop was bank 1's. It isn't now.
- **Proof.** The host without `bank2` calls the same `tape::run` and `kern::process` that the door holds. So `delay_tape_hash_test`, the screen goldens and every render hash pass unchanged, and nothing was re-recorded. `grain_proxy_test` covers the delay and the chorus heard through the door and silent closed. `reverb_proxy_test`'s closed bus is now all silence.
- **Cost.** About 10 cycles a door call (0208), two more a block. Bank 2 is never slower (0142, 0174).

## Alternatives considered
- **Keep the loops in bank 1 and add the skips there:** that is new code in a frozen bank (0198).
- **Closed, keep TAPE in bank 1 and move only the skip:** most of the 5.5 KB would stay. A closed door already says `NEEDS UPDATE` for the reverb, and every install carries the `.bk2` (0194).

## Consequences
- **Flash**, `just check`'s plain release builds, bank 1 before (origin/main `bc20ae7c`'s lines) → after, with #793's DAC round (`vrinta`, −128 to −184 B) in the same lowering. Each line comes down by its Δ, through `BANK1_LOWER=1`, its comment naming this ADR.

  | Set | Bank 1 line → measured (B) | Δ |
  |---|---|---|
  | `orbit` | 891,356 → 883,388 | −7,968 |
  | `default` | 874,464 → 866,528 | −7,936 |
  | `bank2-gate` | 876,300 → 868,460 | −7,840 |
  | `fault-inject` | 848,456 → 840,456 | −8,000 |
  | `bench-algo` | 892,880 → 884,424 | −8,456 |
  | `bench-algo,master-tape` | 869,928 → 861,544 | −8,384 |
  | `bench-routing` | 888,944 → 880,672 | −8,272 |
  | `bench-memory` | 891,692 → 883,372 | −8,320 |
  | `bench-stress` | 893,808 → 885,424 | −8,384 |
  | `bench-orbit` | 867,284 → 859,060 | −8,224 |
  | `bench-tape` | 856,064 → 847,832 | −8,232 |
  | `filter-probes` | 849,824 → 841,816 | −8,008 |

  Bank 2 grows about 8.5 KB (default: 315,232 → 323,776 B). The loops' callers had inlined copies, so it grows by more than 0.3's audit estimated.
- In a `bank2-gate` build, a live door closed under a ringing delay holds its line. A reopened door plays the tail from where it stopped.
- The idle skips #793 adds to both loops (the delay's transport stepped exactly, the chorus's phases and write heads) are bank 2's.

## Sources
- #793; (private notes). ADR 0061 (effects run at MIX 0), 0137 and 0141 (the door, its labels), 0142 and 0174 (bank 2's cost), 0198 (the freeze), 0202 (GRAIN's door), 0208 (a move's lowering).
- `chimera-engine/src/dsp/delay/tape.rs`, `chimera-engine/src/dsp/chorus.rs` (`kern`), `chimera-engine/tests/grain_proxy_test.rs`.
