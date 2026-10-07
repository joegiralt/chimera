# 0106. Put TAPE sources onto the card in checked chunks, written once and sealed

- **Status:** Accepted (owner, 2026-10-07)
- **Deciders:** project owner (2026-10-05: chunked from the start, a re-run
  is the retry; 2026-10-07: sources may go onto their card before #360's
  sweep frees, accepting its risk); firmware (TAPE plan Task 18)

## Context
TAPE plays sources from the card (ADR 0087, ADR 0093): files
`/CHIMERA/TAPES/<CRC>.SRC`, write-once and sealed read-only, named by the
CRC over their structure and samples. The owner's card is sealed in the
unit, so a source reaches it only over USB, and ADR 0092's `put` takes
project files only. A source isn't a project: it has no A/B pair and no
generation, and nothing rewrites it once sealed, so the A/B save doesn't
apply; `write_source`'s rules do. The TAPE spec (§ 6, "What TAPE needs from
`put`") asks for a second path class, a streaming `SourceCheck`, those
rules, and data in chunks each with its own CRC and deadline. The core's
largest source is 1 MiB, ADR 0092's limit: 28 s at 24 kHz. ADR 0092's one
30 s deadline covers a 130 KB project; at the card's 363 KB/s write
(ADR 0045) a megabyte leaves too little of it for a slow host, and a
failure late in one stream throws away the lot with no word on how far
it got.

## Decision
This amends ADR 0092: its project `put` is unchanged.
- **Path:** `put /CHIMERA/TAPES/XXXXXXXX.SRC <size> <crc32>`, eight hex
  digits, any case; `size` at most 1 MiB (`PUT_MAX_BYTES`), at least 32;
  `crc32` (CRC-32/ISO-HDLC) over the whole file. The class compiles only
  with `chimera-core`'s `source-writer`, which `chimera-stm32`'s
  `card-put` now enables; without it the path gets the usage line.
- **First answer**, after making `/CHIMERA` and `/CHIMERA/TAPES`
  (`source_target`, shared with `write_source`):
  - `OK PRESENT` for a file under that name whose head names the stem and
    whose samples hash to it; sealed now if it wasn't. No data is sent.
  - `ERR put: damaged` for a sealed file that fails that check: never
    rewritten (card cleanup is #267).
  - otherwise `READY`, after deleting an unsealed file that fails (the
    torn remains of a cut write).
- **Data in 256 KiB chunks** (`PUT_CHUNK_BYTES`; the last may be
  shorter). Each is ADR 0092's base64 lines (48 bytes a line; padding
  only where a chunk ends), then `end <crc32 of the chunk>`. The unit
  answers `ACK <bytes so far>`, or `ERR put: chunk` for a CRC that isn't
  the chunk's or a line that runs past it. The host waits for each `ACK`
  before the next chunk.
- **Deadlines:** the request line to `READY` has 30 s
  (`PUT_DEADLINE_MS`). Each chunk then has its own 30 s, from `READY` or
  the last `ACK` to its `end`. ADR 0092's 500 ms stall rule holds within
  and across chunks: each 512-byte block must arrive within 500 ms of the
  last. The card write is capped at one deadline per chunk plus 1 s; the
  read back and the seal after it have a 30 s of their own.
- **Checked as it streams:** each block goes through `SourceCheck` before
  it is written, so a file that isn't a `Source`, whose head names
  another stem, or whose frames overrun is refused at the block that
  shows it. The last block, holding the file's trailer, is written only
  once every chunk's CRC, the whole-file CRC and the check have passed.
- **After the last chunk:** read back whole (`verify_and_seal`: the head
  names the stem, the samples hash to it), then sealed, then `OK`.
- **Any failure past `READY`** deletes the unsealed file, drains the
  host's data as ADR 0092 does, and answers `ERR put: <why>`. A re-run is
  the retry: a sealed file answers `OK PRESENT`, a torn one is replaced.
- **Resume is reserved** for TRON: `put … resume` answers
  `ERR put: no resume`. TRON's ADR adds it, with a `Store` append, an
  8 MiB cap and zone sets.
- **`ls tapes`** lists `/CHIMERA/TAPES` with sizes, as `ls` lists
  PROJECTS.
- **Host:** `chimera-usb.py put <file> /CHIMERA/TAPES/` sends the chunks
  and waits 35 s for each `ACK`; it prints `<path> <size> PRESENT` for a
  file already there. `just put-tape <file>` runs it; `just
  put-factory-tapes [dir]` puts every `.SRC` of the factory set
  (default `target/tapes-v1`), each answering `OK PRESENT` on a re-run.
  The factory TAPE Sounds stay off the card until the card library
  reads `/CHIMERA/SOUNDS` (#317).

## Alternatives considered
- **One 1 MiB stream with a deadline scaled to the size:** one stall late
  in the file loses all of it, and the host learns nothing until the
  end; a chunk's `ACK` says how far it got and its CRC catches a
  corrupted run at the chunk.
- **Resume now:** needs a `Store` append the FAT layer doesn't have, and
  the core's 1 MiB files re-send in seconds. TRON's 8 MiB zone sets need
  it; their ADR adds it.
- **The A/B save, as projects:** a source is write-once and named by its
  CRC, so a second side is never needed, and nothing may rewrite a sealed
  file.
- **A `/CHIMERA/SOUNDS` path class for the factory TAPE Sounds:** no
  firmware reads card Sounds yet (#317); the files would sit unread, and
  their on-card form is that library's decision.

## Consequences
- The firmware writes TAPE sources. A power cut mid-put can lose the new
  file's clusters until the boot sweep frees them (#360, ADR 0094, still
  dry by default); the owner accepted that on 2026-10-07. Tests cut power,
  fail the device, and land-then-fail at every block a two-chunk put
  writes: nothing is cross-linked or short, a failed put leaves no
  source that reads, the sweep finds only the cut's own clusters, and the
  re-run lands it once, whole.
- FLASH: the shipping build gives up 5.8 KB for the source class
  (52,856 B free on main, 47,104 B after; with ORBIT 31,144 B to
  25,392 B). The bench builds, which had no room for it, no longer link
  `ls` and `put` at all: they answer "not in this build", and gain the
  room the project `put` held.
- A source put holds the UI loop for its data, up to 30 s a chunk, as
  `put` and `card` already do; audio and MIDI run above it.
- A host must turn each `ACK` round within the 500 ms stall rule.
- TRON adds `resume` without changing this protocol's lines.

## Sources
- `docs/superpowers/specs/2026-10-04-tape-core-design.md` § 6, § 7; TAPE
  plan Task 18.
- ADRs 0045, 0048, 0087, 0092, 0093, 0094.
- `chimera-core/src/console/put.rs`, `chimera-core/src/storage/source_file.rs`
  (`source_target`, `verify_and_seal`, `SourceCheck`), `tools/chimera-usb.py`;
  tests `chimera-core/tests/console_put_tape_test.rs`,
  `chimera-fat/tests/console_put_test.rs`, `tools/test_chimera_usb.py`.
