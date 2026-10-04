# 0092. Put project files onto the card over the USB console

- **Status:** Proposed
- **Deciders:** project owner (2026-10-04: the card stays in the case,
  so projects, and later Sounds and TAPE samples, travel over USB; the
  first need is a project file onto the unit); firmware; the adversarial
  review of the first cut (2026-10-04: no temporary file or rename, the
  unit's own A/B save, no lost clusters on a device error)

## Context
On the owner's unit the SD card sits inside the case with its top
trimmed off: in practice it can't be removed, and it is their only
card, so a lost project or lost clusters are the worst outcome. Files
must reach it over USB. The console (ADR 0068) takes one 64-byte line a
request. Our FAT layer (ADR 0048) has no rename and a `write` that isn't
atomic; ADR 0045's A/B pairs are how the unit's own saves stay safe. The
UI loop runs the console; audio, MIDI DIN and the watchdog's kicks (ADR
0034) run in interrupts above it. `card` already holds the UI loop for
up to 45 s with every card operation under one deadline. A first cut
wrote a temporary file and renamed it over the target; review found a
power cut mid-rename could lose the target, and the rename and its tests
cost 2.7 KB. It is replaced by this.

## Decision
- **`ls <dir>`:** `/CHIMERA`, `/CHIMERA/PROJECTS`, `/CHIMERA/SOUNDS` or
  `/CHIMERA/BENCH`, any case, a trailing `/` allowed. One `NAME size`
  line a file the FAT layer can name (8.3, A-Z 0-9), then `OK`. A missing
  directory is `ERR ls: FILE NOT FOUND`.
- **`put <path> <size> <crc32>`:** three arguments, the first command to
  take more than one. `<size>` is decimal and `<crc32>` up to 8 hex
  digits of CRC-32/ISO-HDLC (`zlib.crc32`) over the bytes sent.
  - `<path>` is `/CHIMERA/PROJECTS/P#######.A` or `.B`, any case. The
    letter names the pair; the unit picks the side, as a save does.
  - Before anything is read, the unit answers `ERR put: not a project or
    sound name`, `ERR put: a directory, not a file`, or, for any file in
    `/CHIMERA/SOUNDS`, `ERR put: no Sound library yet: projects only`
    (no firmware loads `S#######` files yet; they join with the library).
  - Over 1 MiB (`PUT_MAX_BYTES`) or under 32 bytes is refused. So is the
    project the UI holds: `ERR put: project is open`.
  - Any other path, `//` after `/CHIMERA`, `/CHIMERA/SYSTEM.A` and the
    bench files never parse: `ERR put takes …`.
- **Then the unit's own save.** Every card operation is capped at what is
  left of `PUT_DEADLINE_MS` (30 s, from the request line to the answer).
  1. Mount, make `/CHIMERA` and `/CHIMERA/PROJECTS`, then `READY`.
  2. The host sends the file as base64 lines of at most 64 characters
     (48 bytes), each a multiple of 4. Padding is allowed only where the
     file ends; a line `abort` ends the transfer.
  3. The first 28 bytes must be a project header this firmware reads
     (`peek_header`): `WRONG FILE TYPE`, `NEEDS NEWER FIRMWARE` and the
     like are refused before a byte is written.
  4. `save_ab` writes the side a load doesn't keep. It judges the pair
     with `ProjectCheck`, and a pair a load refuses as needing newer
     firmware is never written. The new side gets the newest generation
     plus 1 and the header's name, so it is the one that loads; the
     file's own generation is dropped. The records are copied as sent,
     and the unit writes its own trailer.
  5. While the bytes stream, a `Framer` and a `ProjectCheck` read them
     as a load would. If the file's own trailer, the host's CRC or the
     check fails, the side ends before its trailer: torn, and never
     loaded. The old side still loads, and nothing that needs newer
     firmware can shadow it.
  6. The records go to the card a 512-byte block at a time. Each block
     must arrive within `PUT_STALL_MS` (500 ms) of the last block, so
     the wait for the host never sits inside the card's 600 ms idle
     limit. The data stops 1 s before the write's own cap, and the
     write keeps 3 s back for the check after it.
  7. `peek_entry` reads both headers as the project list does. The answer
     is `OK` only if they show the new generation as the one that loads.
- **Any failure after `READY`** reads and drops the host's remaining data
  until 100 ms of quiet (5 s at most), then answers `ERR put: <why>`. The
  pair loads as before, and its kept side is untouched.
- **A write a device error stops frees its chain** (fixed in `Fs::write`
  for every save, not only `put`). This covers the new chain and the old
  one the entry no longer holds. `Table::drop_chain` frees it at once.
  If the card refuses that write too, the chain is kept with the volume,
  like a stale FAT 2 sector, and freed by the next change on that card.
- **Read-only files:** a file entry with the read-only attribute is never
  rewritten or deleted: `StoreError::ReadOnly`, `FILE IS READ-ONLY`.
- **Types:** `Unit` names its card as an associated type
  (`type Files: Files`, `NoCard` where there is none), so `answer` is
  generic and nothing goes through a vtable. `Files` is `Store` plus
  `cap`, each operation's time cap, which reaches the SD bus through
  `Medium::cap` and `SdBus::set_cap`. `Out` gains `fresh`, `recv` and a
  required `now`.
- **Host:** `python3 tools/chimera-usb.py put <local> <remote>` (a
  `<remote>` ending in `/` takes the local name, upper-cased), waits up
  to 35 s for `READY` and for the answer, shows progress on stderr,
  refuses over 1 MiB before sending, and never retries: one `put`, its
  `ERR` reason, exit 1. `ls` waits 15 s for a cold card. `just put` and
  `just ls` run them.
- **Shipping:** in the default firmware behind `card-put`, a default
  feature of `chimera-stm32`. On a sealed card it is the only way a file
  gets on.

## Alternatives considered
- **A temporary file renamed over the target** (the first cut): a power
  cut between freeing the target's entry and renaming lost the target,
  and the rename cost 2.7 KB the A/B save makes unnecessary.
- **Keep the file's own generation:** an older file would sit hidden
  behind its sibling and never load.
- **Delete a new side the check refuses, after the write:** it keeps
  `delete` out of line for 2 KB. Ending the side before its trailer does
  the same with nothing to undo.
- **Count free space first:** a whole-FAT read, about 3 s on a 32 GB
  card and 1 KB of flash. `Full` from the write leaves a torn side, and
  the old one loads.
- **Hex lines:** 32 bytes a 64-character line, 2x on the wire against
  base64's 1.33x, for a decoder no smaller.
- **Raw binary after a length, as `shot` sends:** a lost byte desyncs
  the console for good, and nothing could be typed or aborted.
- **Acks per chunk:** USB CDC's bulk OUT already NAKs while the unit
  isn't reading.
- **The stock firmware's USB mass storage:** a second USB class, and the
  card mounted by the host while the synth writes it.

## Consequences
- In the release firmware (fat LTO), against main: 9,992 B of `.text` and
  852 B of `.rodata`. Without `card-put`: 5,000 B and 444 B. Of that, about
  900 B is the chain fix and the read-only refusal in the FAT layer,
  which every save carries. The rest is the `ls` and `put` rows of the
  command table, which `chimera-core` keeps in every build. `card-put`
  itself is about 5.4 KB. This is over the review's 6 KB target: the
  save path reused, the check while streaming, and `ls` (about 0.9 KB)
  are where it goes. Nothing is in static RAM. The put session's buffers
  (a block, a line, a `Framer`) are on the UI loop's stack.
- **A power cut mid-write still loses clusters.** Up to all of the new
  side's clusters can be lost (ADR 0048: no entry holds them until the
  write ends). This is true of every save, and only fsck or a
  mount-time sweep reclaims them. The pair still loads its old or new
  generation, and nothing is cross-linked or longer than its chain.
- 1 MiB at most for now: TAPE's samples will need the limit, the
  deadline and a stream of their own, in their own ADR.
- A host that takes over 500 ms for a 512-byte block fails the put.
- `get` is left for a follow-up.

## Sources
- ADRs 0034, 0045, 0046, 0048, 0068; `chimera-core/src/console/put.rs`,
  `chimera-fat/src/fs.rs` (`replace`), `chimera-fat/src/fat.rs`
  (`drop_chain`, `heal`), `tools/chimera-usb.py`.
- RFC 4648 (base64); Microsoft FAT specification (fatgen103), the
  read-only attribute.
