# 0215. Move MODAL's block to bank 2

- **Status:** Proposed
- **Deciders:** owner (0.3 efficiency pass, #793: "the sound must not change"), firmware

## Context
The 0.3 audit of the voice paths (#793, (private notes)) found MODAL's biggest savings, and they all leave the output bit-identical: RES's modes run a block at a time with their state in registers, the weights' oscillator in locals, `compute_filters` skipped when its inputs are still, BOWED's state in locals and SYMP's halo settings cached. All of MODAL's per-block path was bank 1's. `EngineSlot::render` had `ModalEngine::render`, `render_modal` and `render_sympathetic` inlined. `StringVoice::render` and `render_bowed` sat beside them. Bank 1 is frozen (0198), so every one of those edits would be a bank-1 edit. ALGO's block moves to bank 2 for the same reason (0212, #802, still open as this is written).

## Decision
- **MODAL's block is bank 2's**, behind `Bank2Door::modal` (`dsp::modal::ModalDoor { render }`). `ModalEngine::render` is bank 1's side. An idle voice still fills zeros without crossing. An active voice makes one indirect call a block to `modal::kern::render`, which is `ModalEngine::block` inlined: the macros' ease, the retune, the model's loop (RES and RES48's bank, STRING, BOWED, SYMP), the DC blocker and the voice's lifetime, exactly as `render` ran them.
- **What `bank2.x` places:** `*5modal4kern*`, and the loops only the block reaches: `render_bowed`, `StringVoice::render` and `StringVoice::colour`. Text and rodata. `check-door-placed` checks all four.
- **Bank 1 keeps** the engine's state, its constructor, the note-on and glide (`strike`, `fit`, `strike_clear`), `compute_filters` (the note-on sets the bank's filters too), the tuning, the chords, `cost` and the params. Bank 2 calls them in place, so no bank-1 code calls into bank 2.
- **Closed, MODAL is silent and its voices end**, as TAPE's are. The block fills zeros and clears the voice's `active`, so the voice frees after one block. A MODAL Part's header warns `UPDATE`, as a TAPE Part's does (`bank2::closed_part`). MODE reads `NEEDS UPDATE`, as TAPE's MODEL does (`closed_text`). No screen golden draws a MODAL Part with the door closed, so none changes. Edits, loads and saves are bank 1's and still work. A firmware built without bank 2 no longer exists (0194). There, too, MODAL would be silent.
- **Host builds without `bank2`** (the tests, the desktop) call the same `kern::render` in place.
- **Proof.** `chimera-engine/tests/modal_door_test.rs` hashes the DAC and Part 1's bus over 60 cases: each model's INIT; the bank at MODES 16, 24, 32 and 48 over four STRUCTURE and BRIGHT settings, low and high (where modes drop past 0.49 of the rate); POS, STRUCTURE and DAMP under LFOs, alone and with the pitch moving every block; BRIGHT alone; STRING, BOWED and SYMP with POS, BRIGHT and DAMP moving, bent, long and high. Each runs a chord, a re-strike, a new note and a release over 600 blocks. The expected values come from main (bc20ae7c). Bank 1's own path and the host's open door (`--features bank2,test-support`) both match them. Closed, every model is silent and frees its voices after one block. The 230 factory phrase renders through the full engine (the 12 of the five MODAL Sounds among them) and every golden, render and screen hash are unchanged against main.
- **Cost.** One door call a voice a block: about 10 cycles, or 0.16 a sample (0208, 0212). Bank 2 runs from the same flash at the same wait states as bank 1, and Gate 3 found it never slower (0142, 0174). `ModalEngine::cost` stands until the unit bench reads it.

## Alternatives considered
- **An exception to the freeze for MODAL's edits:** most are size-neutral or shrink, but the block-wise bank adds about 1 KB (it keeps the gliding loop), and each later MODAL edit would need its own exception.
- **Moving only the bank's loop (`render_modal`):** STRING's, BOWED's and SYMP's wins would stay bank-1 edits, and the per-block prep (`retune`, the macros) would stay frozen.
- **A bank-1 MODAL for a closed door:** that keeps the code in bank 1, the opposite of the move. A closed door already silences TAPE and the reverb, and ALGO too once 0212 (#802) merges; every install carries bank 2.

## Consequences
- **Flash**, `just check`'s release builds at main (bc20ae7c) and after; bank 1 the `.bin` (`rust-objcopy -R .bank2`), bank 2 the `.bk2`:

  | Set | Bank 1 before | Bank 1 after | Bank 1 Δ | Bank 2 before | Bank 2 after | Bank 2 Δ |
  |---|---|---|---|---|---|---|
  | `default` | 874,480 | 855,904 | −18,576 | 315,232 | 333,984 | +18,752 |
  | `orbit` | 891,172 | 872,588 | −18,584 | 343,136 | 361,856 | +18,720 |
  | `bank2-gate` | 876,268 | 857,684 | −18,584 | 315,232 | 333,984 | +18,752 |
  | `fault-inject` | 848,440 | 829,848 | −18,592 | 315,264 | 333,984 | +18,720 |
  | `bench-algo` | 892,704 | 874,048 | −18,656 | 335,008 | 353,760 | +18,752 |
  | `bench-algo,master-tape` | 869,760 | 851,096 | −18,664 | 335,104 | 353,856 | +18,752 |
  | `bench-routing` | 888,960 | 870,296 | −18,664 | 340,992 | 359,744 | +18,752 |
  | `bench-memory` | 891,652 | 872,996 | −18,656 | 323,392 | 342,144 | +18,752 |
  | `bench-stress` | 893,712 | 875,048 | −18,664 | 334,976 | 353,728 | +18,752 |
  | `bench-orbit` | 866,908 | 848,228 | −18,680 | 350,304 | 369,024 | +18,720 |
  | `bench-tape` | 856,040 | 837,368 | −18,672 | 336,800 | 355,520 | +18,720 |
  | `filter-probes` | 849,800 | 831,208 | −18,592 | 370,336 | 389,056 | +18,720 |

  Each line in `tools/bank1-budget.txt` comes down to its measured size, so the freed room stays bank 2's. Merged onto main after 0213 (the delay and chorus loops) and 0212 (ALGO's block), with the rest of #793's voice wins in the same lowering (the filters' VMAXNM and VSQRT, −2.1 KB; BURST's TILT share, +0.2 KB), each line comes down 20.8 to 21.4 KB from main's (`default` 829,264 → 808,136).
- The efficiency work on MODAL (#793) now edits bank 2, except `compute_filters`, which the note-on shares.
- A closed bank 2 silences MODAL. ABOUT says so, and so does a MODAL Part's header.
- The stack check charges `kern::render` as an address-taken function. Every set stays under its stack.

## Sources
- #793; (private notes).
- ADR 0137 and 0141 (the door, closed labels), 0142 and 0174 (bank 2's cost), 0194 (bank 2 required), 0198 (the freeze), 0208 (ORBIT's move), 0212 (ALGO's, the pattern, #802).
- `chimera-engine/src/dsp/modal/mod.rs` (`ModalDoor`, `kern`), `chimera-stm32/build.rs`, `chimera-engine/tests/modal_door_test.rs`.
