# 0210. Check and seal a hand-copied TAPE source on its first stream

- **Status:** Proposed
- **Deciders:** project owner, Claude (release packaging for 0.2.0)

## Context
A source's seal is the FAT read-only attribute (`ATTR_READ_ONLY`, 0x01)
on its directory entry (ADR 0087, ADR 0106): `put` sets it once the file
read back whole and its samples hashed to its name. A streamed load reads
only a source's heads, so it trusts the seal and refused an unsealed file
as corrupt (#761 m10, #770).

0.2.0 testers install the factory set by copying `factory-v1.zip`'s
CHIMERA folder over the bootloader's SD card access. Whether the host
keeps the read-only bit through an unzip and a copy depends on the OS,
the unzip tool and the file manager, so a hand-copied source arrives
unsealed in general. Whole loads are unaffected: they check every byte
against the CRC on each load. But a source that streams (too big for the
pool's free room) would be refused forever: nothing on the unit sealed it.

## Decision
A streamed load that meets an unsealed source checks it whole once
through `put`'s `SourceCheck` (its head naming the stem, its samples
hashing to it) before streaming it: a `Check` phase of `StreamLoad`,
`read_source_check` feeding the pass's budget a pass from where the last
one stopped. Once every byte passed, on the card and the file as found
(the registry's epoch and the file's first block, as the commit checks),
the source is sealed and its heads read as a sealed one's. One that fails
stays unsealed and refused (`READ FAILED`), as before. The seal is best
effort: a card that won't take it streams the checked file anyway and
checks it again on its next load. A project load's pass 2
(`StreamLoad::found`) hands an unsealed file to that phase instead of
skipping it. The phase and `read_source_check` are bank 2's; bank 1
gains only `HeadOnly`'s reach test (its sink stays in bank 1, as every
sink's vtable must).

`factory-v1.zip` ships the sources with ordinary (writable) permissions:
it does not rely on the attribute surviving a copy. Once sealed they are
read-only on the card, so the release notes and the README tell testers
to Skip a `.SRC` already there on an update (its name is its CRC: the
same bytes) rather than Replace it.

## Alternatives considered
- **Ship them sealed in the zip:** the attribute doesn't survive every
  unzip and copy, and a read-only file makes the guides' Replace step
  prompt or fail on the next update.
- **Seal every source on a card scan or at boot:** reads every source on
  the card each time a new one appears, for files that may never stream.
- **Seal after a whole load:** bank 1 code (frozen, ADR 0198), and a
  source that streams from the first load is never whole-loaded.

## Consequences
- A hand-copied source streams like a `put` one. Its first stream reads
  the whole file once, bounded per UI frame: the SOURCE cell's budget is
  `STEP_BYTES` (4 KiB) a frame, one `read_at` of eight blocks after the
  file's directory entry and cluster chain are found. ADR 0172's M1
  measured a random 4 KB read at median 5.3 ms, max 6.7 ms, so no frame
  waits on much more than that. At `UI_FPS` (20) that is 80 KB/s: about
  13 s a MiB, once; the factory sources (15–48 KB) take 4–12 frames, up
  to 0.6 s. The cell shows the load pending, as for any streamed load.
  Each read is one 4 KiB card operation, far under `SD_OP_CAP_MS` (10
  s), so a large reel never times out. A project load checks it in `ARENA_BYTES` (160 KiB) passes
  inside its one card operation, as its whole loads read.
- The seal is a directory-entry write: like a save, it quiets any
  streams playing (their ~20 ms fade) while the card is written, once a
  source.
- A damaged unsealed file is still refused, never sealed.
- Amends #761 m10's refusal: unsealed is now "not yet checked", not
  "corrupt".

## Sources
- `chimera-engine/src/project/streamed/load.rs` (`StreamLoad::find`, `check`),
  `chimera-engine/src/project/streamed.rs` (`StreamLoad::found`),
  `chimera-engine/src/storage/source_file.rs` (`read_source_check`)
- `chimera-fat/src/dir.rs` (`ATTR_READ_ONLY`), `chimera-fat/src/fs.rs` (`runs`)
- Tests: `tape_part_load_test` (`an_unsealed_file_that_checks_out_streams_and_is_sealed`,
  `an_unsealed_damaged_file_doesnt_stream`), `tape_source_cell_test`
  (`an_unsealed_file_streams_from_the_cell_once_checked`,
  `an_unsealed_damaged_file_doesnt_stream_from_the_cell`),
  `a_large_unsealed_source_is_checked_a_share_a_pass`
