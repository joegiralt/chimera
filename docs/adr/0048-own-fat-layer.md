# 0048. Own the FAT layer; keep embedded-sdmmc only as the SD block driver

- **Status:** Proposed
- **Deciders:** owner (2026-09-29, after the review of b1ee1c9), firmware

## Context
Plan 1 (storage foundation) first put `embedded-sdmmc` 0.10's `VolumeManager`
behind the `Store` trait (commit b1ee1c9). The review of that commit, checked
against the 0.10.0 source, found defects in the library's FAT layer. A
wrapper can't fix them:

- **C1: allocation past the volume.** `find_next_free_cluster`
  (`src/fat/volume.rs:1089-1161`) checks `current_cluster < end_cluster` once
  per FAT block. Inside a block it scans to the block's end. So a zero entry at
  or past `count + 2` in the FAT's last block is returned as free, and
  `cluster_to_block` maps it past the partition's end. Formatters zero the
  FAT's tail, so this affects about 255/256 of FAT16 cards and 127/128 of
  FAT32 cards. On FAT32 it triggers as soon as every cluster above FSInfo's
  next-free hint is in use.
- **I1: delete leaks the chain.** `delete_entry_in_dir`
  (`src/volume_mgr.rs:878`) reaches `delete_entry_in_block`
  (`src/fat/volume.rs:1059-1086`), which writes 0xE5 and never frees the FAT
  chain.
- **I2: FSInfo written on every operation.** `close_volume`
  (`src/volume_mgr.rs:374-399`) always calls `update_info_sector`
  (`src/fat/volume.rs:178-207`), and so does `flush_file`
  (`src/volume_mgr.rs:1216`). On FAT32 every operation rewrites FSInfo,
  reads included. With a mount per operation, that is one write per `Store`
  call.
- **A panic on a bad link.** `cluster_to_block` (`src/fat/volume.rs:367-392`)
  computes `c - 2`, which underflows when a chain links to cluster 0 or 1.
  b1ee1c9's mutation fuzz found this.
- **The last free cluster.** `alloc_cluster` (`src/fat/volume.rs:1164-1241`)
  looks for the *next* free cluster after taking one, and fails the
  allocation when there is none. So the last free cluster is never used, and
  each failed allocation leaks the cluster it had already marked end-of-chain.
  It also decrements the FSInfo free count unchecked (`:1228`).
- **An FSInfo loop.** An FSInfo sector with a bad signature fails
  `InfoSector::create_from_bytes` (`src/fat/info.rs:52-64`) as
  `Error::FormatError` when the volume opens (`src/fat/volume.rs:1475-1482`).
  Mapped to `Io`, that re-inits on every try, forever.

b1ee1c9 already carried its own MBR/BPB parser, a chain-walking guard and
remappings of library errors, because the library couldn't be trusted with a
card's bytes. Fixing C1, I1 and I2 on top of that means a second FAT
allocator, a second chain freer and skipping `close_volume`. That leaves the
library little beyond its SD driver.

The SD driver half (`SdCard`, which implements `BlockDevice` over SPI) works.
The Task 2 probe read and wrote a FAT32 SDHC card through it at 12.5 and
25 MHz.

## Decision
- `chimera-fat` implements its own FAT16/FAT32 file layer.
- `embedded-sdmmc` stays a dependency for `SdCard`, `BlockDevice`,
  `AcquireOpts` and `SdCardError` only. No build links `VolumeManager`.

The layer's scope is what `chimera_hal::store::Store` needs, and nothing more:
- the fixed folders `Dir::{Chimera, Projects, Sounds}`;
- 8.3 short names (`FileName`), with no LFN. LFN, label and dot entries are
  skipped. A delete also marks the LFN run in front of its entry, when the
  run's checksum matches;
- streamed read, streamed write (truncate and replace), delete, make_dir and
  list.

Shape: a functional core, host-tested with no hardware, under a thin shell.
- **Pure:** `volume.rs` (MBR, BPB and `Layout`), `dir.rs` (the entry codec)
  and `fsinfo.rs`.
- **Core:** `fat.rs` (`Table`: one cached FAT sector, the allocator, bounded
  chain walks) and `fs.rs` (`Fs`), both over a `Blocks` trait of
  volume-relative 512 B reads and writes.
- **Shell:** `store.rs` (`FatStore<D: Medium>`). It mounts per operation,
  checks the `VolumeId`, maps errors, calls `reinit`, and bounds every block
  to the partition.

The invariants, each pinned by a test in plan 1, Tasks 4a and 4b:
1. A data cluster is one in `2..count + 2`. The allocator scans exactly that
   range, the last free cluster included. A failed allocation writes nothing.
2. Every chain walk is bounded by the cluster count and checked link by
   link. A broken link or a loop is `Corrupt`, never a panic.
3. Every FAT change goes to every FAT copy, and a FAT sector is written only
   when it changed. The FAT is flushed before any directory entry that points
   into it.
4. Read-only operations write no block.
5. A write runs in this order: reset the entry to empty, free the old chain,
   write the data, flush the FAT, write the entry. Delete marks the entry
   first, then frees the chain. A cut leaves lost clusters, never a
   cross-link and never a length over a short chain.
6. FSInfo is advisory. It is never needed to mount, and a bad one is never
   read or written. On a valid one, the free count is set to 0xFFFF_FFFF
   ("unknown") once, on the first FAT-changing operation, and nothing else in
   it is written. Its next-free is only the scan's first start point, and the
   scan checks every entry it takes.

A computer's `fsck.fat` (dosfstools) checks the layer's output. That check is
a hard requirement of `just test` and `just check`.

## Alternatives considered
- **Keep wrapping `embedded-sdmmc`.** b1ee1c9's approach, with more guards:
  - C1 needs a FAT scan before every allocation, so we would write the
    allocator anyway, beside the library's.
  - I1 needs a chain freer that writes FAT blocks behind the library's block
    cache: two writers of one FAT, with no cache coherence.
  - I2 can't be avoided without skipping `close_volume`, which leaks the
    volume slot.

  We would own most of a FAT layer and still carry the library's. A patched
  fork means maintaining the whole library. Reporting the defects upstream is
  still worth doing (a follow-up), but a fix there can't gate this plan.
- **ChaN's FatFs through C FFI.** It is mature, its license is BSD-style, and
  it keeps FSInfo right. But every card operation would cross an `unsafe` C
  boundary. The firmware and host tests would need a C toolchain (`cc`). Its
  configuration surface (LFN, multi-volume, its own cache) is far beyond
  what's needed, and its `FRESULT`s still need our device-error
  classification. Its flash cost is also comparable to what we'd write.
- **The `fatfs` crate (rafalh/rust-fatfs).** Its released line needs `std`,
  or `alloc` plus a `core_io` shim; the no-alloc `no_std` work sits on an
  unreleased branch. We would pin a git revision, bring in LFN and `alloc`
  that the rules (no heap) don't allow, and still audit its allocator and
  FSInfo handling.
- **A non-FAT format of our own.** The card would no longer mount on a
  computer, and the spec needs it to (files are copied on and off; exFAT
  gets "FORMAT FAT32").

## Consequences
- We own about a thousand lines of FAT code and its correctness. Three
  things check it:
  - tests on RAM images;
  - `embedded-sdmmc` as a second reader and writer, away from the free-space
    edge;
  - dosfstools: `mkfs.fat` makes images we must read, and `fsck.fat -n` must
    pass images we wrote, including every power-cut image.

  A machine without dosfstools can't run `just test`.
- RAM drops. The store is one 512 B block buffer, one 512 B FAT-sector
  cache, a hint and the driver, under a 2 KB `STORE_RESERVE`. That replaces
  the plan's 4 KB for `VolumeManager`. There is no heap and no handle table.
- The first allocation after power-on scans the FAT from FSInfo's next-free,
  or from cluster 2. On a large, nearly full card that is seconds. A 32 GB
  card with 32 KB clusters has a 4 MB FAT, about 3 s at the 1.3 MB/s the
  probe read, which is under the 10 s `SD_OP_CAP_MS`. After that, a RAM hint
  keyed by the volume id keeps scans short. If the Task 13 STOP measures it
  as too slow, a follow-up keeps next-free in FSInfo too.
- Once Chimera has written to a card, a computer sees FSInfo's free count as
  unknown and recounts free space on mount. `fsck.fat -n` accepts this with
  exit 0 (checked with dosfstools 4.2).
- Out of scope, by design: long names (a file a computer names
  `my song.a` is invisible), FAT12, exFAT, rename, timestamps (every entry
  is 2026-01-01 00:00), and free-space queries.
- A cut can leave lost clusters, which a computer's disk check reclaims. The
  write order means it never costs the other file of an A/B pair (spec
  § A/B saves, ADR 0045).
- Flash: `VolumeManager` is no longer linked. Task 13's STOP records the
  `.text` size before and after.

## Sources
- Plan: `docs/superpowers/plans/2026-09-28-storage-foundation.md`, Tasks 4a
  and 4b, § Review response "Task 4 review (b1ee1c9)".
- Spec: `docs/superpowers/specs/2026-09-28-projects-storage-design.md`,
  § Storage.
- Commit b1ee1c9 and its review (`.superpowers/sdd/2026-09-28-storage-foundation/`
  `task-4-report.md`, `review-ff6e3d3..b1ee1c9.diff`).
- `embedded-sdmmc` 0.10.0 (MIT/Apache-2.0), the files and lines cited above.
- Microsoft, *FAT32 File System Specification* (fatgen103, 2000): cluster
  count classification, the FSInfo signatures, and 0xFFFF_FFFF as "unknown".
- dosfstools 4.2 (`mkfs.fat`, `fsck.fat`), GPL-3.0, used as test tools only;
  none of its code is used.
