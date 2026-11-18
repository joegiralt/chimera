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
- **ITCM's state is zeroed in whole 64-bit words at boot, before any
  build into it.** The H7's ITCM is ECC-protected (RM0433 § 2.4, the
  TCMs' ECC), and ST's AN5342 advises initialising ECC RAM before sub-word
  accesses: a byte or halfword store is a read-modify-write of the ECC
  word, and a power-on word's ECC is noise. The in-place builds (`Kits`,
  `EdState`, `AudioShared`) store bytes. `shared::zero_itcm` writes
  `__sitcm`..`__eitcm` with aligned 64-bit stores (one `strd` each, the
  ITCM's word) right after `cache::init` in `boot`, at full clock, once.
  It returns `ItcmZeroed`, a token only it makes, which `take_ui`,
  `take_audio`, `take_ed` and `bench_audio` each take, so no build into
  ITCM can be written before the zeroing. sd-probe has no ITCM state and
  no zeroing.
- **ITCM keeps 8 KiB free.** `itcm.x` asserts
  `ORIGIN(ITCM) + LENGTH(ITCM) - __eitcm >= 8K`, as ADR 0087's bench
  margin, written by `build.rs` from one constant, `ITCM_MARGIN_KIB`.
  `hw::ITCM_STATE` (63 KiB) and `hw::ITCM_MARGIN` (8 KiB) mirror `memory.x`
  and that constant, and `memory_budget_test::itcm_geometry_is_hws` reads
  both files and holds them equal. The linker's assert is the proof;
  `itcm_residents_fit` is a conservative host estimate (host `usize` is
  8 B, each rounded up to 8 though `.itcm` packs them at 4) that prints the
  sizes.
- **ITCM holds only listed CPU-only state, one list.**
  `tools/itcm-residents.txt` names the residents (`ED`, `KITS`,
  `KIT_VIEW`, `AUDIO`) and their types. `just check-itcm <elf>`
  (`tools/check_itcm.py`) reads `__sitcm`/`__eitcm` from the ELF and fails
  when a symbol in that range is data not on the list, or any non-data
  kind (weak `V`/`W`, text, read-only, common); absolute `A` symbols are
  linker constants, not storage, and are not judged. A resident with a
  third field must be in ITCM wherever that symbol is linked: `AUDIO`
  wherever `take_audio` is, so sd-probe, which links no synth and leaves
  ITCM empty, passes. `memory_budget_test::itcm_residents_are_the_files`
  holds the host test's types to the file. The check runs in every
  `bank2-split` (sd-probe's among them, which `just check` now runs as
  `flash-sd-probe` does) and on every set `just check` budget-checks.
  Adding to ITCM means adding a line there, under an ADR.

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
- **Boot:** the zeroing is 5,067 `strd`s in a four-instruction loop, about
  15,000 to 20,000 cycles by its disassembly, some 40 µs at 480 MHz (about
  50 µs on rev Y). Not timed on the unit.
- **Bench baselines may move:** the bench rows whose kernels read the shared
  slot (`AudioShared` through `Reader`, and the bench's own slot 0) now read
  ITCM, so their recorded cycle counts may shift, down if anything. A
  baseline that moves after this change is this placement, not a
  regression.
- **Unit check after merge** as ADR 0266's: boot, play, ORBIT, a bench
  build's first slot.

## Sources
- ADR 0021 (triple buffer), 0087 (AXI margin), 0198 (bank 1 budget), 0265,
  0266 (ITCM), 0276 (`ed` in ITCM).
- `docs/superpowers/specs/2026-11-18-axi-room-design.md`.
- RM0433 § 2.3 (ITCM, CPU-only) and § 2.4 (embedded SRAM and TCM ECC);
  ST AN5342, *How to use error correction code (ECC) management for
  internal memories protection on STM32 MCUs* (initialise ECC RAM before
  sub-word accesses). Cited from the review; two fetches of AN5342 from
  st.com timed out, so its section numbers are unchecked here. ARM
  Cortex-M7 TRM (TCM interfaces, 64-bit ITCM, RMW).
- `tools/itcm-residents.txt`, `tools/check_itcm.py`.
