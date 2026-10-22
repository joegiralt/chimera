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
A streamed load that meets an unsealed source reads it whole once through
`put`'s `SourceCheck` (`read_source_head` with `Reach::Whole`: its head
naming the stem, its samples hashing to it), then seals it, and streams
it as a sealed one. One that fails stays
unsealed and refused (`READ FAILED`), as before. A project load's pass 2
(`StreamLoad::found`) hands an unsealed file to that first pass instead of
skipping it. The pass and `read_source_head` are bank 2's; bank 1 gains only
`HeadOnly`'s reach test (its sink stays in bank 1, as every sink's
vtable must).

`factory-v1.zip` ships the sources with ordinary (writable) permissions:
it does not rely on the attribute surviving a copy.

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
  the whole file once (the factory sources are 15–48 KB); a large one
  takes longer that once.
- A damaged unsealed file is still refused, never sealed.
- Amends #761 m10's refusal: unsealed is now "not yet checked", not
  "corrupt".

## Sources
- `chimera-engine/src/project/streamed/load.rs` (`StreamLoad::pass`),
  `chimera-engine/src/project/streamed.rs` (`StreamLoad::found`),
  `chimera-engine/src/storage/source_file.rs` (`Reach`, `read_source_head`)
- `chimera-fat/src/dir.rs` (`ATTR_READ_ONLY`), `chimera-fat/src/fs.rs` (`runs`)
- Tests: `tape_part_load_test` (`an_unsealed_file_that_checks_out_streams_and_is_sealed`,
  `an_unsealed_damaged_file_doesnt_stream`), `tape_source_cell_test`
  (`an_unsealed_file_streams_from_the_cell_once_checked`)
