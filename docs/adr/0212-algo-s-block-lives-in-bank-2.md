# 0212. Move ALGO's block and its kernel to bank 2

- **Status:** Proposed
- **Deciders:** owner (0.3 efficiency pass, #793: "the sound must not change"), firmware

## Context
The 0.3 efficiency audit of ALGO's voice (#793) found savings that leave the output bit-identical: a held envelope skipped on a silent operator, each operator's pitch taken once, no 1.5 KB clear a block, a kernel loop with no PM. Each one is an edit to code in bank 1, and bank 1 is frozen (0198): the default build sat 16 B over its line. The prototypes grew bank 1 by up to 2,136 B through inlining churn alone. ALGO's per-block path was all bank 1's: `EngineSlot::render` with `AlgoEngine::render` inlined (22.5 KB) and `Kernel::render_ops` (15.4 KB), with no door on it.

ALGO is the default engine: INIT and every built-in Sound play it.

## Decision
- **ALGO's block is bank 2's**, behind `Bank2Door::algo` (`dsp::algo::engine::AlgoDoor { render }`). `AlgoEngine::render` is bank 1's side: an idle voice still fills zeros without crossing, and an active voice makes one indirect call a block to `engine::kern::render`. That call is `AlgoEngine::block` inlined: the swap, the envelope rates, the mips, the operator blocks, the norm, the kernel and the voice's lifetime, exactly as `render` ran them. `bank2.x` links `*4algo6engine4kern*` and the whole of `*4algo6kernel*` (text and rodata). `check-door-placed` checks `algo::engine::kern::` and `Kernel>::render_ops`.
- **Bank 1 keeps** the engine's state, its constructor, `note_on` and the helpers it shares with the block (`mip_target`, `formant_blocks`, `noise_law`, `target_gain`, `seed_ramps`, `formant_fc`), `cost`, the params, the plan and the envelopes' out-of-line steps. Bank 2 calls them in place, so no bank-1 code calls into bank 2.
- **Closed, ALGO is silent and its voices end.** The block fills zeros and clears the voice's `active`, so the voice frees as a TAPE voice does with its door closed. Edits, loads and saves are bank 1's and still work. ABOUT already reads `NEEDS UPDATE` for a closed bank 2 (0141), and that is the warning. An ALGO Part's header stays as it is: an `UPDATE` there, as on a TAPE Part, would change a closed-door screen golden (`mixer_fx_delay_grain_needs_update`, whose Part plays ALGO), and this move changes nothing a user sees with the door open or closed except the silence. The header warning is tracked in private issue #803. The firmware without bank 2 no longer exists (0194). With the door closed, ALGO goes quiet the same way the reverb, TAPE, FUZZ, SEM and ORBIT already do. Every install carries the `.bk2` (0137).
- **Host builds without `bank2`** (the tests, the desktop) call the same `kern::render` in place.
- **Proof.** `chimera-engine/tests/algo_door_test.rs` hashes Part 1's bus for ALGO INIT and the 12 built-in Sounds (a 4-note chord, a re-strike, a release, 400 blocks). The expected values come from main (bc20ae7c). Bank 1's own path and the host's open door (`--features bank2,test-support`) both match them. Closed, every one is silent and frees its voices after one block. The efficiency ledger's 44-case DAC hash and the 59 factory ALGO Sounds' 144 phrase renders through the full engine are unchanged against main.
- **Cost.** One door call a voice a block: the cell's load, a branch, the table's load and an indirect call, about 10 cycles, or 0.16 a sample. Bank 2 runs from the same flash at the same wait states as bank 1, and Gate 3 found it never slower (0142, 0174). The unit bench confirms it before `COST_BASE` or `COST_OP` move.

## Alternatives considered
- **Pin the inlining instead (`#[inline(never)]` on the helpers, `detune_at` out of line):** this fights LTO on every later edit, and each edit to the kernel stays a bank-1 edit under the freeze.
- **Move only the kernel (`render_ops`, `run_one`, `link_sum`), as the audit's #8 had it:** the per-block prep (`op_cycles`, the mips) is where prototype #3 grew bank 1. That code would stay frozen.
- **Keep a bank-1 ALGO for a closed door:** that keeps the 36 KB in bank 1, which is the opposite of the move. A closed door already silences TAPE and the reverb, and every install carries bank 2.

## Consequences
- **Flash**, `just check`'s release builds at main (bc20ae7c) and after; bank 1 the `.bin` (`rust-objcopy -R .bank2`), bank 2 the `.bk2`:

  | Set | Bank 1 before | Bank 1 after | Bank 1 Δ | Bank 2 before | Bank 2 after | Bank 2 Δ |
  |---|---|---|---|---|---|---|
  | `default` | 874,480 | 838,328 | −36,152 | 315,232 | 351,744 | +36,512 |
  | `orbit` | 891,172 | 855,028 | −36,144 | 343,136 | 379,616 | +36,480 |
  | `bank2-gate` | 876,268 | 840,108 | −36,160 | 315,232 | 351,744 | +36,512 |
  | `fault-inject` | 848,440 | 812,272 | −36,168 | 315,264 | 351,744 | +36,480 |
  | `bench-algo` | 892,704 | 853,936 | −38,768 | 335,008 | 373,760 | +38,752 |
  | `bench-algo,master-tape` | 869,760 | 830,984 | −38,776 | 335,104 | 373,856 | +38,752 |
  | `bench-routing` | 888,960 | 853,048 | −35,912 | 340,992 | 377,248 | +36,256 |
  | `bench-memory` | 891,652 | 855,748 | −35,904 | 323,392 | 359,648 | +36,256 |
  | `bench-stress` | 893,712 | 857,800 | −35,912 | 334,976 | 371,232 | +36,256 |
  | `bench-orbit` | 866,908 | 830,996 | −35,912 | 350,304 | 386,560 | +36,256 |
  | `bench-tape` | 856,040 | 820,120 | −35,920 | 336,800 | 373,056 | +36,256 |
  | `filter-probes` | 849,800 | 813,632 | −36,168 | 370,336 | 406,816 | +36,480 |

  Each line in `tools/bank1-budget.txt` comes down to its measured size, so the freed room stays bank 2's. Merged over main at 5ec76841 (after 0213 and 0214), with the rest of the 0.3 ALGO passes in, the lines come down again from main's: `default` 865,904 → 829,264 (−36,640), `orbit` 882,780 → 846,100, `bench-algo` 883,648 → 844,648, every set by 36.5 to 39.0 KB.
- The efficiency work on ALGO (#793) now edits bank 2 only. Its inlining churn no longer touches the frozen bank.
- A closed bank 2 silences the default engine. A unit whose `.bk2` is missing plays only what bank 1 still holds until it is updated. ABOUT says so. A header warning on ALGO Parts is a UI change for its own PR (private issue #803).
- The bench's kernel rows (`bench-algo`) already ran from bank 2 (0194). They now time kernel code that is itself in bank 2, as the shipping build runs it.
- The stack check charges `kern::render` as an address-taken function. Every set stays under its stack.

## Sources
- #793; (private notes) (the audit, its #8).
- ADR 0137 and 0141 (the door, closed labels), 0142 and 0174 (bank 2's cost), 0194 (bank 2 required), 0198 (the freeze), 0208 (ORBIT's move, the pattern).
- `chimera-engine/src/dsp/algo/engine.rs` (`AlgoDoor`, `kern`), `chimera-stm32/build.rs`, `chimera-engine/tests/algo_door_test.rs`.
