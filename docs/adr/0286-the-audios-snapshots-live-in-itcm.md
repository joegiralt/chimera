# 0286. Keep the audio's Performance snapshots in ITCM, with an 8 KiB ITCM margin and a CPU-only list

- **Status:** Proposed
- **Deciders:** owner (2026-11-18: free AXI by moving CPU-only state to ITCM), firmware

## Context
On origin/0.4 (1ff9f2a2) the ORBIT build's AXI sat 44 B over ADR 0087's
32 KiB margin (`__sheap` 0x24077FD4). The per-Part EQ (#342) needs 72 B,
and SMART EQ (#396) and tester features more. ITCM (ADR 0266) held 17,432 B
of its 63 KiB (`ED`, `KITS`, `KIT_VIEW`), with 47,080 B free and no margin.

The AXI residents by size: the arena (160 KiB, stays), the framebuffer
(150 KiB, SPI DMA's, and bigger than ITCM), the FX bus (110 KiB, the delay
line), then `shared::AUDIO`, the triple-buffered `AudioShared` (23,104 B).
Everything after it is under 6 KB. The audit is in
`docs/superpowers/specs/2026-11-18-axi-room-design.md`.

`AUDIO` is CPU-only: the UI's `Writer` publishes, the audio interrupt's
`Reader` reads, and the bench lends slot 0 (`bench_audio`) before
`take_audio`. No peripheral or DMA stream addresses it. It is already
`MaybeUninit`, built whole in place by `take_audio`, so ITCM's NOLOAD
changes nothing that reads it.

## Decision
- **`shared::AUDIO` lives in `.itcm`.** Its type, protocol and builder are
  unchanged; only its section moves.
- **ITCM keeps 8 KiB free.** `itcm.x` asserts
  `ORIGIN(ITCM) + LENGTH(ITCM) - __eitcm >= 8K`, as ADR 0087's bench
  margin. `hw::ITCM_STATE` (63 KiB) and `hw::ITCM_MARGIN` (8 KiB) mirror it,
  and `memory_budget_test::itcm_residents_fit` reports the residents and
  asserts they fit.
- **ITCM holds only listed CPU-only state.** `just check-itcm <elf>` fails
  when a data symbol in ITCM is not `AUDIO`, `KITS`, `ED` or `KIT_VIEW`, or
  when `AUDIO` is not there. It runs in every `bank2-split` and on every set
  `just check` builds and budget-checks (default, ORBIT, bank2-gate, ORBIT
  with the config writer, each bench set). Adding to ITCM means adding to
  that list, which names this ADR.

## Alternatives considered
- **More, smaller moves** (`ENGINE` 5.9 KB, `ORBIT`'s view 0.9 KB, the MIDI
  control rings 4.6 KB, the USB audio rings 8.2 KB). `AUDIO` alone frees
  5.8 times the 4 KB asked. `ENGINE` and `ORBIT` qualify the same way and
  stay as the next round's reserve. The rings are zero-initialised `.bss`
  and would each need a build-in-place first, and the USB ones sit on the
  USB interrupt's FIFO path.
- **Raising or lowering the AXI margin.** ADR 0087's 32 KiB is the UI's and
  the card's growth room; this frees room under it instead.
- **No ITCM margin.** ITCM would then be the next wall with no warning, and
  ADR 0266 keeps room for a hot kernel.

## Consequences
- **AXI** (0x24080000 − `__sheap`, measured on every set `just check`
  builds, clean stamp), +23,104 B each; "(8 KiB)" marks a bench set,
  whose margin is 8 KiB:

  | Set | Free before | Free after | Above margin after |
  |---|---:|---:|---:|
  | default | 34,596 | 57,700 | 24,932 |
  | orbit | 32,812 | 55,916 | 23,148 |
  | bank2-gate | 34,588 | 57,692 | 24,924 |
  | orbit,usb-config-writer | 37,456 | 60,560 | 27,792 |
  | fault-inject | 46,840 | 69,944 | 37,176 |
  | bench-algo | 9,252 | 32,356 | 24,164 (8 KiB) |
  | bench-algo,master-tape | 29,364 | 52,468 | 44,276 (8 KiB) |
  | bench-routing | 15,940 | 39,044 | 30,852 (8 KiB) |
  | bench-memory | 15,932 | 39,036 | 30,844 (8 KiB) |
  | bench-stress | 15,932 | 39,036 | 30,844 (8 KiB) |
  | bench-orbit | 36,756 | 59,860 | 51,668 (8 KiB) |
  | bench-tape | 41,692 | 64,796 | 56,604 (8 KiB) |
  | filter-probes | 46,840 | 69,944 | 37,176 |

- **ITCM** on every set: 17,432 → 40,536 B used, 23,976 B free, 15,784 B
  above its margin.
- **Flash:** bank 1 is byte-identical in size on every set above (clean
  stamp); no budget line moves. Bank 2 unchanged.
- **Speed:** the audio reads its Part params from the snapshot each block.
  ITCM is zero wait; AXI is zero wait on a D-cache hit and a line fill on a
  miss, so the move can only remove misses. Not measured on the unit (no
  flash in this change); expected within the bench's noise.
- **Sound:** identical; placement only. The host goldens do not see it.
- **Unit check after merge** as ADR 0266's: boot, play, ORBIT, a bench
  build's first slot.

## Sources
- ADR 0021 (triple buffer), 0087 (AXI margin), 0198 (bank 1 budget), 0265,
  0266 (ITCM), 0276 (`ed` in ITCM).
- `docs/superpowers/specs/2026-11-18-axi-room-design.md`.
- RM0433 §2.3 (ITCM, CPU-only); ARM Cortex-M7 TRM (TCM data access).
