# 0045. Store cards in 8.3 A/B files of versioned TLV records

- **Status:** Proposed
- **Deciders:** owner (projects and storage spec, 2026-09-28; plan 1 review), firmware

## Context
Chimera saves SYSTEM now, and projects and library sounds in plans 2 and 3,
to an SD card the user may pull at any moment, carry to a computer, or fill
with files from a newer firmware. The constraints:

- Our FAT layer (ADR 0048) has 8.3 names only, no rename, and a `write` that
  is not atomic. A FAT rename isn't atomic anyway.
- RAM: no staging copy and no serialised buffer. A save streams from live
  state; a load streams from the card.
- Every firmware must read every older file identically, and must refuse,
  never misread, a file that needs something it doesn't know.
- The parser never panics on any bytes a card can hold.

## Decision

### Names
8.3 names keyed by id, the display name in the header:
`/CHIMERA/SYSTEM.A` `.B`, `/CHIMERA/PROJECTS/P0000001.A` `.B`,
`/CHIMERA/SOUNDS/S0000001.A` `.B`. A `ProjectId` is 1..=9 999 999, so its stem
`P` + 7 digits always fits. Duplicate display names are allowed.

### File layout
All integers are little-endian.

| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `CHIM` |
| 4 | 2 | format version, `1` |
| 6 | 1 | kind (`FileKind`) |
| 7 | 1 | flags, `0` |
| 8 | 4 | generation |
| 12 | 16 | display name, NUL-padded; 16 NULs is no name |
| 28 | … | records: `u16` tag, `u16` length, the payload |
| len − 4 | 4 | CRC32 trailer |

- The trailer is CRC-32/ISO-HDLC (reflected poly 0xEDB88320, init and
  xorout 0xFFFF_FFFF) over bytes `0..len − 4`. It is a trailer so the writer
  streams and never seeks back.
- **The envelope is frozen.** The magic at offset 0, the version at offset 4
  and the CRC-32 trailer over `0..len − 4` never change, in any version, so
  every reader can verify every file before judging it.
- **The version bumps only for a framing change that breaks compatibility.**
  New content is a new record, and the critical bit when an older reader
  must not load the file without it.
- A version above 1 is `NeedsNewerFirmware`; version 0 or flags ≠ 0 is
  `Corrupt`. A kind this firmware doesn't know is `WrongKind`.
- **The CRC comes first.** Every verdict above and below (`NeedsNewerFirmware`,
  `Bounds`, `WrongKind`, `BadName`, `Corrupt`, a decoder's own) waits until
  the CRC is known. After the first one the reader hashes the rest of the
  body unparsed. A CRC failure is `BadCrc` whatever else was found, so one
  flipped bit reads as torn and never shadows the other A/B side as "needs
  newer firmware". Torn means exactly `BadCrc`, or `Truncated`: a file under
  32 B, or a stream shorter than its length. A bad magic under a valid CRC is
  `BadMagic`; with a failing one it is `BadCrc`, torn.
  The file's length comes from the directory entry, never from its own
  bytes, so every file can be hashed. A foreign file therefore reads as
  "FILE CHECKSUM FAILED", not "NOT A CHIMERA FILE"; `BadMagic` needs a CRC
  that happens to match.
- **Two passes.** Record events before the CRC is checked are provisional.
  A decoder applies them only on a second pass, after the first pass's
  `finish` is Ok.
- The name follows `Name<16>`: A–Z a–z 0–9, space and `-`, no leading or
  trailing space, no byte after the first NUL.
- `FileKind`: 1 Sound, 3 System. **Reserved:** 2 Project, 4 Tags, 5 Index.
  A reserved code is never given another meaning.

### Records
| Tag | Code | Critical | Payload |
|---|---|---|---|
| `Block` | 0x0001 | no | block code `u8`, then (`ParamId` `u8`, value 4 B LE)* |
| `Engine` | 0x8002 | **yes** | engine code `u8`; the first record of a Sound file |
| `Registry` | 0x0003 | no | (block code, param id, label [8])* |
| `ModDests` | 0x0004 | no | `num_sources` `u8`, then (block code, param id)* |
| `Routes` | 0x0005 | no | (source code, block code, param id, amount `i8`)* |
| `LastProject` | 0x0006 | no | `ProjectId` `u32` LE |

- **Must-understand.** Tag bit 15 (`CRITICAL`, 0x8000) marks a record a
  reader must understand. An unknown critical record makes the whole file
  `NeedsNewerFirmware`: it is listed greyed and never loaded altered. An
  unknown non-critical record is skipped by its length, however long.
- **Bounds.** A known record's payload is at most `MAX_RECORD_LEN` = 512 B,
  the one buffer the reader has. Every length is bounded by what is left of
  the file before the trailer. Past either bound is `Bounds`.
- **Frozen codes.** Record tags, `FileKind`s, block codes, param ids, mod
  source codes and every stored enum's code are exhaustive `match` tables,
  pinned by golden tests. A code is never reused: a retired code goes on
  the retired-ids list, which starts empty in v1.
- **Values.** Enums by frozen code, never by UI position; continuous values
  as `f32` in spec units, clamped and quantised through their `ParamSpec`. A
  NaN or infinite value takes the default. An enum code out of range takes
  the default in a non-critical record and greys the file in a critical one.
- **Neutral defaults.** Decoding starts from a frozen base: every param at
  its neutral default, no routes, an empty registry. Not `Sound::init`,
  whose routes may change. A param added after v1 has a neutral default that
  reproduces v1's sound.
- **Migration.** A rescaled param, or one whose meaning changes, gets a new
  `ParamId` and a pure `migrate(old) -> new`; the old id is retired.
- **Fixtures.** A corpus of real v1 files lives in
  `chimera-core/tests/fixtures/v1/`. Every later version loads it and renders
  it identically.

### A/B generations
- Every file is a pair, `.A` and `.B`, each with a `u32` generation that
  wraps; newer is serial-number order, `(a − b) as i32 > 0`. The first save
  is generation 1.
- **Pick.** The reader takes the newest side that decodes (a tie takes A).
  It falls back to the other side on every error except
  `NeedsNewerFirmware`: a file that needs newer firmware is refused, never
  shadowed by an older one. Both sides broken gives the newest side's error,
  not "missing"; missing means neither file exists.
- **A side with no header.** A file whose CRC matches but whose header this
  firmware can't read (a newer format version, a bad magic, kind or name)
  has no generation to order it by. One that needs newer firmware is taken
  as the newest: refused, and never written over. Any other falls back.
- **Two passes, staged.** Pass 2 applies only if its CRC equals pass 1's; a
  decoder stages pass 2 and commits it to the target at its `end`, so a
  card changed between the passes reads as `BadCrc` and leaves the target
  as it was.
- **`write_target`.** A save truncates and rewrites the side the reader
  doesn't keep (the older, missing or broken one) with the newest generation
  + 1, then flushes. It judges both sides by the kind's own pass 1, the
  check a load makes, so a side a load would reject is broken to the save
  too, and the side a load would keep is never written.
- **Never shadow, never write.** When a load refuses the pair because a
  side needs newer firmware (its header read or not), a save refuses too,
  with `NeedsNewerFirmware`, and writes nothing: whatever it wrote would sit
  behind that side, never loaded. The file lists greyed as "NEEDS NEWER
  FIRMWARE"; deleting the pair, which still works, is how the user
  overrides it. A side that needs newer firmware but is older than a valid
  one shadows nothing, and a save writes over it.
- **Delete.** The older side first, then the newer. A cut in between leaves
  one valid file, which reads as "not deleted".

### The atomic-block assumption
A/B is safe only if a 512 B block write is all-or-nothing and disturbs no
other block. SD cards don't promise this. `.A` and `.B` share a directory
sector and, when small, FAT sectors, so a block torn there can lose both
sides. We accept it: the data blocks, where nearly all writes land, each
belong to one side. Plan 1 Task 11's torn-block test measured it on a
FAT16 card with the pair in one directory block and FAT sector 0: a
garbage tear of either shared block can lose the pair (4 of the 18 torn
writes of one save); a tear that leaves the block's second half old never
did, and no tear ever loaded wrong data.

### Card access
- SD in SPI mode on SPI2 (SCK PA9, MISO PB14 pulled up, MOSI PB15), polled,
  no DMA: the stack is in DTCM, which DMA1 and DMA2 can't reach, and D2 is
  full.
- The SPI1/2/3 kernel clock moves to **PLL2_P at 100 MHz**: 390.6 kHz for
  init (÷256), 25 MHz fast (÷4), and exactly 50 MHz for the display (÷2).
  The stock firmware runs SPI1, SPI2 and USART1 from PLL2 too.
- **Deadlines** instead of one per operation: `SD_ACQUIRE_MS` = 1 500 per
  acquire, with a presence check first and one fresh retry with a card
  present; `SD_IDLE_MS` = 600 with no block moved (above SDHC's 500 ms write
  busy); `SD_OP_CAP_MS` = 10 000 as a backstop.
- **Mount per operation.** Every `Store` call opens the volume, checks its
  id, acts and closes every handle, on success and on error.
- **Volume id and the boot-sector gate.** The `VolumeId` (serial and label)
  and the FAT16/FAT32 classification come from our own MBR and BPB parse
  (ADR 0048). A boot sector that fails validation never reaches the FAT
  layer. A mismatched id is `VolumeChanged`.
- **`Ready` is a capability.** Store operations take a `&Ready`, which is
  neither `Copy` nor `Clone` and is lent only inside `Card::run`.

### Measured (plan 1 Task 2 probe, 2026-09-29, rev V)
- **CS is PE12**: the card answered on it.
- **SPI mode is MODE_3.** MODE_0's first cold acquire failed; MODE_3 then
  acquired in 140 ms. MODE_1 is not an SD mode and timed out.
- **Fast clock 25 MHz**: write 363 KB/s, read 1 333 KB/s, worst gap
  2 336 µs, well inside `SD_IDLE_MS`.

### Pending
- **Card-detect: unknown.** The plan builds on "no card-detect line": `Absent`
  is inferred from a failed acquire. The only source is a transcription of
  the stock firmware (Ixox/preenfm3), which lists no card-detect pin; the
  probe couldn't test it. The owner confirms it against the schematic or the
  CubeMX `main.c` at plan 1's Task 13 STOP. A line, if found, is a follow-up
  issue.

This ADR stays Proposed until it holds no pending item.

## Alternatives considered
- **One file with rename-over.** Our FAT layer has no rename, and a FAT
  rename is two directory writes, not atomic.
- **A journal or log-structured file.** More writes, more code and a replay
  path, for the same atomic-block assumption.
- **The CRC in the header.** The writer would seek back or stage the file in
  RAM, which the RAM budget forbids.
- **Fixed-offset binary structs.** Every added param moves every offset; old
  files would need a converter per version. TLV records skip what they
  don't know.
- **A text format (JSON, INI).** A parser, float formatting and 3–5× the
  size, for files nobody edits by hand.
- **Enums by UI position.** Reordering a menu would change saved sounds.
- **Long file names.** LFN entries add code to our FAT layer (ADR 0048
  leaves them out); ids in 8.3 names are enough when the name is in the
  header.

## Consequences
- Old files load forever: codes are frozen, ids are never reused, and
  migrations are pure functions tested against the fixture corpus.
- A newer file on an older firmware is greyed, never half-loaded.
- Loads take two passes (check, then apply), so a torn file never touches
  live state. A save and a delete run pass 1 on both sides first, the check
  a load makes: a save never writes, and a delete never removes first, the
  side a load would keep.
- A torn directory or FAT sector can still lose a pair (the atomic-block
  assumption above).
- Adding a record, a kind or a param is additive; changing one's meaning
  costs a new code.

## Sources
- `docs/superpowers/specs/2026-09-28-projects-storage-design.md` § Storage
  (Layout, A/B saves, Format).
- `docs/superpowers/plans/2026-09-28-storage-foundation.md`: Global
  Constraints, Decisions, Task 6, Task 11 and § Measured.
- `chimera-core/src/storage/` (`frame.rs`, `record.rs`, `crc.rs`),
  `chimera-core/src/name.rs`, `chimera-stm32/src/sd.rs`,
  `chimera-stm32/src/clocks.rs`.
- ADR 0048 (our FAT layer), ADR 0025 (DTCM stack), ADR 0013 (budgets).
- CRC-32/ISO-HDLC: the "CRC RevEng" catalogue, check value 0xCBF43926.
- Stock firmware: github.com/Ixox/preenfm3 (`MX_SPI2_Init`,
  `MX_GPIO_Init`; consulted for pins and clocks only, no code taken).
