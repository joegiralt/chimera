# 0094. Sweep lost clusters at the boot after an unclean change, or on demand, from a bitmap in the arena's gap, dry until the owner's go

- **Status:** Proposed
- **Deciders:** project owner (the card is sealed in the unit and is their
  only one: no write on it that could free a reachable cluster; trigger
  ruling 2026-10-08, #375); firmware; two adversarial reviews of the
  design (2026-10-04, 2026-10-05) and a safety review before unlocking
  (2026-10-08)

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
- **The sweep runs at the boot after an unclean change, and on demand,
  in one card operation.** A power cut is always followed by a boot.
  Device errors mid-session are #362's fix and the orphan's, and leave
  the mark dirty too. A card Chimera never wrote (no `/CHIMERA`) gets no
  sweep: its disk check is the computer's job.
- **The mark: `/CHIMERA/SWEEP.MRK`, one block, written in place.** Clean
  is exactly `mark::CLEAN`; `mark::DIRTY` is its complement, different
  in every byte. Anything else, the wrong length, or a chain that doesn't
  hold the length reads dirty. Read at boot, it is one of:
  - `Clean`: every change since the last settled sweep finished;
  - `Dirty`: a change began and didn't, or a sweep left work or a doubt;
  - `Missing`: `/CHIMERA` with no mark (older firmware, or the mark's own
    making was cut);
  - `Unused`: no `/CHIMERA`;
  - `Doubted(n)`: dirty, and the last `n` boots' sweeps doubted in a row;
  - `Asked(n)`: a sweep asked for, carrying the doubt count `n`.

  `Dirty`, `Missing`, `Asked` and `Doubted` below `DOUBT_HALT` (3) owe a
  sweep. The variants are `DIRTY` with its first byte moved:
  `doubted(n)` by `n`, `asked(n)` by `0x10 | n`. Each still differs from
  `CLEAN` in every byte, so a torn write of one reads plain `Dirty`. That
  is a shorter streak and one more sweep, never a clean read.
- **Every change marks itself, in `FatStore`.** `write`, `delete`,
  `make_dir`, `seal` and `remove_dir` all go through `FatStore::change`.
  It reads the mark first (a few block reads). If the mark is clean, the
  partition's block writer writes `DIRTY` to the mark's block before the
  change's first write of any kind: heal, FSInfo, FAT, entry or data.
  The change only goes ahead once that write has returned. `CLEAN` goes
  back only after all three hold:
  - the change returned `Ok`;
  - every one of its writes has returned;
  - the FAT cache holds no stale sector or orphan for the next change.

  The mark is per change, not per save. A cut between two changes of a
  save strands nothing, because each change leaves the FAT whole, and
  the sweep's job is the FAT, not a save's content check. Other rules:
  - A change that writes nothing (a `NotFound`, an existing directory)
    writes no mark.
  - A mark that isn't clean when a change starts is left as it is. Only
    a settled sweep clears a debt.
  - `write` and `delete` refuse the mark's own name (`ReadOnly`).
    `remove_dir` removes only empty directories, so the mark's cluster
    stays the mark's for any change.
- **Why the mark is safe across a cut.**
  - It is written in place, so its writes change no FAT value or entry
    and strand or cross nothing.
  - `DIRTY` lands before anything else of the change. The SD card
    answers a single-block write only once it is programmed. In SPI mode
    the card's write cache is off unless enabled, and Chimera never
    enables it.
  - A torn or partial write of either value, or noise, isn't `CLEAN`, so
    it reads dirty and costs one extra sweep, never a missed one.
  - The mark is made only by `Store::settle`, as an ordinary `Fs::write`
    after a settled sweep. A cut there leaves it `Missing` or dirty,
    which owes a sweep.
  - The one change left unmarked is the first `make_dir(Chimera)` on a
    card Chimera never wrote. It can strand at most that one cluster,
    and the card is then `Missing`, so the next boot sweeps.

  Tests: `mark_test` cuts or tears every write of a new file, a rewrite,
  a delete, a directory and the mark's own making. Once any write
  landed, the mark owes a sweep. The boots after leave no lost cluster
  and a clean mark, or, where noise tore a directory or FAT block, a
  doubt with nothing written.
- **The trigger.** `storage::boot_sweep` mounts the card, reads the mark,
  and sweeps only when the mark owes one or can't be read. A swept
  card's mark is cleared only when `Swept::settled` holds: no stale
  sector, and every lost cluster freed, so none in `DryRun`. A run that
  leaves work without a doubt puts a doubt count back to plain `Dirty`.
  - In the dry build, a card with lost clusters keeps owing, sweeps dry
    at every boot as before, and never writes its mark.
  - A dry run that finds nothing clears it.
- **Three doubting boots in a row stop the automatic sweep** (owner
  ruling 2026-10-08).
  - A boot whose sweep doubts on a volume it read counts one more, in
    place: `Dirty` → `Doubted(1)` → … → `Doubted(3)`. No card, a swap or
    an unmountable card counts nothing.
  - A card in doubt allocates nothing, so a missing mark, or one that
    isn't one block, keeps no count. That card goes on sweeping at every
    boot.
  - At `Doubted(3)` the boot skips the sweep, writes nothing, and warns.
    `status` adds `warn sweep stopped: …`, and the boot toasts `CARD
    NEEDS A CHECK: SWEEP OFF`.
  - A boot that runs without a doubt ends the streak. A settled run
    clears the mark.
  - A count write cut or torn reads the old count, the new one, or plain
    `Dirty`, never `Clean` (`a_cut_count_write_never_reads_clean`).
- **On demand is a request, swept at the next boot.** The bitmap is
  60 KB on the owner's card. With a project loaded the arena's gap is
  smaller, and the console's first in-place sweep answered `doubt
  no-room` on the unit (Phase A, 2026-10-08). So nothing sweeps in the
  running synth.
  - The console's `sweep` (`console::ask_sweep`) refuses over unsaved
    changes, which a restart would lose. Otherwise it sets the mark
    `Asked(n)`, keeping the doubt count, and answers `OK`. The shell
    then syncs SYSTEM and restarts.
  - The boot sweeps an `Asked` card even when halted, with the gap free.
    If that sweep doubts too, the carried count is put back, so a
    requested sweep's doubt counts no boot.
  - The request writes in place, or makes the mark if it is `Missing`.
    No `/CHIMERA` is `ERR … nothing to sweep`.
  - SETTINGS › SYSTEM › STORAGE › CLEAN UP is the same request plus a
    restart, once STORAGE exists (#267).
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
    stack, and each live entry's chain in full. Through `D`, a reached
    cluster's FAT 2 value may differ from FAT 1's only the way a FAT
    1-first write leaves it. Either FAT 2 holds the end mark a link
    replaced, or it holds free where an allocation took the cluster.
    Then FAT 1's link is followed. Any other difference is
    `FatsDiffer`: FAT 2 linking past FAT 1, a free or garbled FAT 1
    under a live chain (a torn sector), or two different links. The free
    phase copies FAT 1 over FAT 2. Following FAT 2's longer chain would
    mark a directory's tail at this boot and lose its files at the next.
    Test: `a_dir_tail_only_fat2_links_keeps_its_files`.
  - Count: one pass over FAT 1, plus `D`'s FAT 2 copy. A cluster in
    `2..clusters + 2` is lost if its bit is clear and its value is a link
    or an end mark in both copies. Free, cluster 1, out-of-range, bad and
    reserved values stay.
  - Cap (`Sweep::Free` only): more lost clusters than `max_lost` is
    `TooMany(n)`, and nothing is written. `max_lost` is
    `2 × (⌈MAX_CHAIN_BYTES / cluster⌉ + 1)`: two cuts of the longest
    chain Chimera writes, each with a directory cluster.
    - `MAX_CHAIN_BYTES` is 4 MiB, the bench's card test file. A `put` is
      at most 1 MiB and a project about 175 KB, and `put` and the bench
      assert they fit.
    - A cut strands at most one chain. `Fs::write` frees the old chain
      and flushes it before it allocates the new one.
    - Every unclean change is followed by a sweeping boot, so one cut is
      the rule; the second covers a cut during that boot.
    - The cap is 258 clusters on the owner's 32 KB clusters and 16,386
      on 512 B ones.
    - A mark bug that misses a directory frees nothing past the cap. A
      chain longer than it only stays lost.
    - A dry run reports every lost cluster.
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
     live entry after `0x00`; a subdirectory whose first block lacks `.`
     in slot 0 and `..` in slot 1; and a `.` or `..` anywhere else or
     naming the wrong cluster.
  4. A file whose chain breaks, or is shorter than its length
     (`FileShort`).
  5. A cluster reached twice, loops included (`CrossLinked`).
  6. Deeper than `MAX_DEPTH = 8`, or more than `MAX_DIRS = 256`
     directories below the root (`TooDeep`). Every directory on the
     volume is walked; none is ever skipped.
  7. More clusters than bits (`NoRoom`).
  8. The deadline, a card that left, or a swapped volume (`Card`).
  9. `Free` finding more than `max_lost` (`TooMany`).

  A block past the volume met in the walk is `DirBroken`, never skipped.
- **Dry first.** The firmware runs `Sweep::DryRun`. That goes through
  `ReadOnly<B>`, whose write is refused, and never calls `will_change`,
  so it does no heal and no FSInfo patch. `status` prints its `sweep`
  line: the mode, `lost`, `freed` and `healed` or the doubt's word, spc,
  clusters, FAT bytes, and the ms for compare, mark and count.
  `Sweep::Free` is behind the feature `sweep-free`, in `chimera-board`
  and in `chimera-stm32`, which turns on `chimera-board`'s. Each crate
  carries `compile_error!("owner's go required")`, so the lock holds
  whichever crate is built with it. Only the commit that enables freeing
  removes both lines. `status` reports the run:
  - `stale`: FAT 2 had one sector behind FAT 1;
  - `healed`: that sector was copied, never in a dry run;
  - `then`: what became of the mark.
- **The timing gate, measured.** The gate was: more than about 2 s on the
  owner's card means a trigger before `sweep-free`. On that card (16 GB,
  32 KB clusters, 485,936 of them):
  - The every-boot dry run measured about 4.3 s (#375), and 3.2 s
    (compare) + 1.6 s (count) + mark by the safety review. Free adds a
    FAT pass of about 1.6 s and its writes, under `SD_OP_CAP_MS` = 10 s.
  - 16 KB-cluster cards timed out every boot.

  The trigger above is that decision. A clean boot costs the mount and
  the mark's reads, 9 blocks on the test card
  (`a_clean_boot_reads_a_few_blocks_and_writes_none`): about the root's
  and `/CHIMERA`'s blocks up to their entries, a FAT sector and the mark,
  milliseconds at 25 MHz.

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
  2's write leaves `D`, and FAT 2 may hold a link a reached cluster still
  needs. Freeing only what both copies hold as lost keeps that rule for
  unreached clusters. What only FAT 1 holds goes at the next boot, once
  `D` is copied.
- **The union of both copies through `D` (this ADR's first draft).** It
  followed a link either copy held. FAT 2 linking a directory's tail past
  FAT 1, through a writer that puts FAT 2 first, a card that drops an
  acknowledged FAT 1 write, or a torn FAT 1 sector, was marked at one
  boot. `D`'s copy then made FAT 2 FAT 1's, and the next boot freed the
  files in that tail. A recoverable leak became loss.
- **Sweep at every boot.** It was the dry run's first shape: 4.3 s on the
  owner's card, and a timeout at every boot at 16 KB clusters (#375).
- **A dirty bit elsewhere.**
  - FAT[1]'s clean-shutdown bit, the volume's own dirty flag, makes a
    computer offer a disk check at every mount while it is set. It also
    shares FAT sector 0 with `D`'s compare.
  - FSInfo's reserved bytes are "must be zero" to every other
    implementation, and FAT16 has no FSInfo.
  - A mark file made and deleted per save is itself a change that
    allocates, so a cut can strand its cluster.
  - A field in `SYSTEM` is an A/B file write per save.
  - A mark per save rather than per change needs every caller to
    bracket its changes. In `FatStore` it covers every path that writes,
    the console's `put` among them.

## Consequences
- A cut change's lost clusters are freed at the next boot once
  `sweep-free` ships. A cut between FAT 1's and FAT 2's write of a sector
  needs two boots: one to copy the sector, one to free what only FAT 1
  held. The mark stays dirty until the second boot, because a stale
  sector isn't settled.
- A clean boot no longer sweeps. A card whose sweep ends in a doubt keeps
  its mark dirty and sweeps at three boots, then stops and warns until
  its doubt is fixed (a computer's disk check, or a reformat) and a
  requested sweep settles.
- A FAT 1 sector torn by a cut, or FAT 2 ahead of FAT 1 under a live
  chain, is now a doubt, not a union. Such a card isn't swept, and the
  next change that writes the sector copies FAT 1, garbled or not, to
  FAT 2, as before this ADR.
- On-demand sweeping costs a restart; unsaved changes refuse it.
- Every change that writes costs two more block writes, `DIRTY` and
  `CLEAN`, plus the mark's few reads.
- A sweeping boot grows by the run. It reads both FATs, every directory
  and FAT 1 once more, and its time is in `status`.
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
- `chimera-fat/src/sweep.rs`, `chimera-fat/src/mark.rs`,
  `chimera-fat/src/fat.rs`, `chimera-fat/src/store.rs`,
  `chimera-engine/src/storage/sweep.rs`, `chimera-fat/tests/sweep_test.rs`,
  `chimera-fat/tests/mark_test.rs`, `chimera-fat/tests/dosfstools_test.rs`
  (FAT32 at spc 2 and spc 64 too)
- #375 private issue #375 (the owner's
  trigger ruling), #267 private issue #267
  (STORAGE)
- Microsoft FAT32 File System Specification 1.03 (directory entry, LFN
  and `0x05` rules); dosfstools 4.2 `fsck.fat -n`
