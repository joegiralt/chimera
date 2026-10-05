# 0094. Sweep lost clusters once at boot, from a bitmap in the arena's gap, dry until the owner's go

- **Status:** Proposed
- **Deciders:** project owner (the card is sealed in the unit and is their
  only one: no write on it that could free a reachable cluster);
  firmware; two adversarial reviews of the design (2026-10-04, 2026-10-05)

## Context
A power cut mid-save can leave a new file's clusters marked used with no
directory entry: up to the whole new chain, 343 clusters for a 175 KB
project on the 512 B-cluster test card (#360). ADR 0048's layer writes
the FAT before the entry, so a cut never cross-links or truncates, but
nothing ever frees what it strands, and on the owner's card that space
is gone for good. A device error mid-session is different: the store is
still running and frees the chain itself (`drop_chain`, #359), except
when the chain's next cluster sits in the next FAT sector (#362): `alloc`
marks the new cluster end-of-chain before it links the previous one, and
a failed flush of the link left `drop_chain` stopping one short.

The test oracle `fat_check` walks only `/CHIMERA`, so it can't judge a
whole-volume sweep. The card may hold the stock firmware's folders and a
computer's files (`SYSTEM~1`, `TRASHE~1`, macOS `._` files, `BANK_1.BNK`),
whose 8.3 names `dir::parse` gives as `Slot::Other`. The store's RAM is
2 KB (ADR 0048); a reachability bitmap needs a bit a cluster, 61 KB for a
16 GB card of 32 KB clusters. Every card operation runs under
`SD_IDLE_MS` (600) and `SD_OP_CAP_MS` (10 s).

## Decision
- **#362 at its source, not by reordering.** `alloc` keeps writing the
  end mark before the link: linking first would let a cut leave a live
  chain (a growing directory) pointing at a free cluster, which the next
  `alloc` cross-links. `Table` keeps `fresh: Option<(u32, u32)>`, the
  `(prev, c)` of the last `alloc` whose clusters sit in different FAT
  sectors, cleared once a flush carrying the link lands in FAT 1; `alloc`
  flushes first while one is pending, so there is at most one.
  `free_chain` returns its last freed cluster and whether it ended on an
  end mark. `drop_chain` frees `c` only if its walk freed `prev`, ended
  there on an end mark, and `c` still reads end-of-chain. Its orphan is
  `(start, Option<(prev, c)>)`, and `heal` applies the same rule. Each FAT
  sector FAT 1 takes during the free moves the orphan's start to the
  first cluster the card still holds, so a free a device error stops
  partway strands nothing. Orphans and stale sectors stay per `VolumeId`,
  as ADR 0048 keeps them: a card a computer changed under the same id is
  out of scope.
- **The sweep runs once, at boot, in one card operation.** A power cut is
  always followed by a boot. Device errors mid-session are #362's fix and
  the orphan's. A card swapped in from a computer gets no sweep: its disk
  check is the computer's job.
- **The bitmap is the arena's gap.** Before the project loads, the gap is
  unused: `Pool::sweep_scratch` lends it (`ArenaWriter::gap_mut`) and
  asserts no source is published; the sweep zeroes the bits it uses. On
  the host a test lends a `Vec`. More clusters than bits is `NoRoom`,
  decided from the `Layout` before any FAT or directory read.
- **Four phases:** compare, mark, count, free.
  - Compare: FAT 1 and FAT 2 sector by sector. More than one differing
    sector is a doubt; the one a cut can leave, `D`, keeps its FAT 2
    copy in RAM.
  - Mark: every directory from the root, depth first on an explicit
    stack, and each live entry's chain in full, over the union of both
    copies through `D`.
  - Count: one pass over FAT 1, plus `D`'s FAT 2 copy. A cluster in
    `2..clusters + 2` is lost if its bit is clear and its value is a link
    or an end mark in both copies. Free, cluster 1, out-of-range, bad and
    reserved values stay.
  - Free (`Sweep::Free` only): `D`'s FAT 2 copy from FAT 1 first, then
    FSInfo's free count to unknown, then each lost cluster freed in every
    FAT, sector by sector, under the same deadline. FAT 1 is
    authoritative for writing. A stop partway is safe, because every
    cluster it frees was proven unreachable before the first write.
- **The mark phase reads raw entries.** It never uses `dir::parse`,
  `Fs::scan` or `FileName`. It reads byte 0 and byte 11 of each 32-byte
  entry: `0x00` ends the directory, `0xE5` is deleted, `attr & 0x3F ==
  0x0F` is a long-name slot, `0x08` without `0x10` is a label. Everything
  else is live, a `0x05` lead byte included, and its chain is marked (the
  high word at byte 20 on FAT32). A chain longer than its file is never
  trimmed. A directory's chain is marked when it is pushed, so a loop
  back to an ancestor is a cross-link. A first-level `..` may be 0 or the
  root cluster.
- **Any doubt ends the sweep before its first write**, and nothing is
  freed (`Doubt`):
  1. FATs that differ in more than one sector, or `D` sending a reached
     cluster two ways (`FatsDiffer`).
  2. A device error reading a directory or the FAT (`Unreadable`).
  3. A broken directory (`DirBroken`). This covers a free, reserved, bad
     or out-of-range link; a subdirectory at cluster 0; a label or long
     name slot holding a cluster; a label that is also a directory; a
     live entry after `0x00`; and a `.` or `..` out of place or naming
     the wrong cluster.
  4. A file whose chain breaks, or is shorter than its length
     (`FileShort`).
  5. A cluster reached twice, loops included (`CrossLinked`).
  6. Deeper than `MAX_DEPTH = 8`, or more than `MAX_DIRS = 256`
     directories below the root (`TooDeep`). Every directory on the
     volume is walked; none is ever skipped.
  7. More clusters than bits (`NoRoom`).
  8. The deadline, a card that left, or a swapped volume (`Card`).
- **Dry first.** The firmware runs `Sweep::DryRun`. That goes through
  `ReadOnly<B>`, whose write is refused, and never calls `will_change`,
  so it does no heal and no FSInfo patch. `status` prints its `sweep`
  line: the mode, `lost`, `freed` and `healed` or the doubt's word, spc,
  clusters, FAT bytes, and the ms for compare, mark and count.
  `Sweep::Free` is behind the `chimera-stm32` feature `sweep-free`, which
  carries `compile_error!("owner's go required")`. Only the commit that
  enables freeing removes that line. If compare, mark and count together
  take more than about 2 s on the owner's card, a dirty-bit trigger is
  decided in a follow-up before `sweep-free`.

## Alternatives considered
- **Link before marking the new cluster's end (#362 by reordering).**
  A cut between the two writes leaves a live directory chain pointing at
  a free cluster, which the next `alloc` hands out again: a cross-link.
  That is worse than a lost cluster.
- **Sweep at every mount.** Mid-session faults are already handled by
  `drop_chain` and the orphan. A sweep per mount would put a full FAT
  pass under every card operation's deadline. A computer's card is the
  computer's to check.
- **Grow `fat_check` into the oracle.** It walks only `/CHIMERA`, and the
  code under test can't be its own judge. `fsck.fat -n` is the oracle:
  `sweep_matches_fsck`, ≥ 200 seeds of random raw trees on `mkfs.fat`
  images, checks it reclaims exactly the injected clusters, and that
  after the sweep it reclaims nothing and counts the same clusters used.
- **Parse entries with `dir::parse`.** It gives `Slot::Other` for names
  outside A-Z and 0-9, which a sweep would read as no entry, freeing a
  computer's files.
- **A bitmap in the store, or a FAT-walk without one.** The store has
  2 KB. Reachability without a bitmap means a walk per cluster, far over
  the deadline.
- **Trust FAT 1 alone when FAT 2 differs.** A cut between FAT 1's and FAT
  2's write leaves `D`. Taking the union of both copies for reachability,
  and freeing only what both copies hold as lost, never frees a cluster
  either copy links to a live chain. What only FAT 1 holds goes at the
  next boot, once `D` is copied.

## Consequences
- A cut save's lost clusters are freed at the next boot once `sweep-free`
  ships. A cut between FAT 1's and FAT 2's write of a sector needs two
  boots: one to copy the sector, one to free what only FAT 1 held.
- Boot time grows by the dry run until the trigger is decided. It reads
  both FATs, every directory, and FAT 1 once more. Its time is in
  `status`.
- A card with any doubt is never swept, so its lost clusters stay. A
  computer's disk check, or a reformat, is the remedy, and `status` names
  the doubt.
- The arena's gap is scratch at boot only. Anything that publishes a
  source before the sweep trips `sweep_scratch`'s assert.
- Leaks are counted only per `VolumeId`. A computer's edit under the same
  id between two of the unit's operations could, in principle, make a
  kept orphan stale; ADR 0048 already accepts this.

## Sources
- #360 private issue #360,
  #362 private issue #362
- ADR 0048 (own FAT layer), ADR 0087 (the arena), ADR 0045 (A/B saves)
- `chimera-fat/src/sweep.rs`, `chimera-fat/src/fat.rs`,
  `chimera-fat/tests/sweep_test.rs`, `chimera-fat/tests/dosfstools_test.rs`
- Microsoft FAT32 File System Specification 1.03 (directory entry, LFN
  and `0x05` rules); dosfstools 4.2 `fsck.fat -n`
