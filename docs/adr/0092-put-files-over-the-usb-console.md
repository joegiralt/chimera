# 0092. Put files onto the card over the USB console

- **Status:** Proposed
- **Deciders:** project owner (2026-10-04: the card stays in the case,
  so projects, Sounds and later TAPE samples travel over USB; the
  first need is a project file onto the unit); firmware

## Context
On the owner's unit the SD card sits inside the case with its top
trimmed off: in practice it can't be removed. Files must reach it over
USB. The console (ADR 0068) is read-only but for `dfu` and the bench's
`card`, and takes one 64-byte line a request. Our FAT layer (ADR 0048)
has no rename and a `write` that isn't atomic, which ADR 0045's A/B
pairs work around for the unit's own saves. The UI loop runs the
console; audio, MIDI DIN and the watchdog's kicks (ADR 0034) run in
interrupts above it. `card` already holds the UI loop for up to 45 s
with every card operation under one deadline.

## Decision
- **`ls <dir>`:** `/CHIMERA`, `/CHIMERA/PROJECTS`, `/CHIMERA/SOUNDS` or
  `/CHIMERA/BENCH`, any case. One `NAME size` line a file the FAT layer
  can name (8.3, A-Z 0-9), then `OK`. A missing directory is
  `ERR ls: FILE NOT FOUND`.
- **`put <path> <size> <crc32>`:** three arguments, the first command to
  take more than one. `<path>` is `/CHIMERA/PROJECTS/<8.3>` or
  `/CHIMERA/SOUNDS/<8.3>`, any case, stored upper case; `<size>` is
  decimal, at most 1 MiB (`PUT_MAX_BYTES`); `<crc32>` is up to 8 hex
  digits of CRC-32/ISO-HDLC (`zlib.crc32`, ADR 0045's CRC). Any other
  path, `..`, a long name, a second `/`, `PUT.TMP` itself, `/CHIMERA`
  (so SYSTEM) and `/CHIMERA/BENCH` never parse: `ERR put takes …`.
  `/CHIMERA/IMPORT` does not exist yet; it joins when it does.
- **Then, in order,** each card operation capped at what is left of
  `PUT_DEADLINE_MS` (30 s, from the request line to the answer):
  1. mount, make `/CHIMERA` and the target's directory;
  2. free space from the FAT: under the file plus `PUT_ROOM_BYTES`
     (1 MiB) is `ERR put: would leave under 1 MiB free`, before
     anything is written;
  3. `READY`. The host now sends the bytes as base64 lines of at most
     64 characters (48 bytes), a multiple of 4, padding only in the
     file's last quad. A line `abort` ends it;
  4. the lines stream into `PUT.TMP` in the target's directory, a
     512-byte block at a time, so the wait for the host never sits
     inside a card write's idle limit (`SD_IDLE_MS`, 600 ms). The write
     keeps 6 s of the deadline back;
  5. exactly `<size>` bytes whose CRC matches, or it fails;
  6. `PUT.TMP` is renamed over the target, keeping 3 s back;
  7. `OK`.
- **Any failure** (a bad or long line, more bytes than `<size>`, no byte
  for `PUT_STALL_MS` = 500 ms, which is how a short transfer ends,
  `abort`, the CRC, the deadline, a card error) deletes `PUT.TMP` under
  the time kept back, reads and drops the host's remaining data until
  100 ms of quiet (5 s at most), then answers `ERR put: <why>`. The
  target is untouched. A cut of power can leave `PUT.TMP`; the next
  `put` to that directory writes over it, and neither list shows it.
- **Rename over (`Fs::rename`):** both entries in one directory. When
  they share a directory block, one block write frees the target's
  entry and renames the temporary one: atomic under ADR 0045's
  atomic-block assumption. When they don't, the target's entry is freed
  first, then the temporary one renamed, so a cut between leaves the new
  bytes whole in `PUT.TMP`, the old target gone, and never two entries
  under one name. Then long names that spoke for either short name, then
  the target's old chain is freed and the FAT flushed. `fsck.fat` passes
  after renames near and far in the directory (`just test-fat-tools`).
- **`Files`** in `chimera-hal`: `Store` plus `free_bytes`, `rename` and
  `cap` (each operation's time cap; `None` puts the medium's own back).
  `FatStore` implements it; `Medium::cap` reaches `SdBus::set_cap` on the
  unit. `Unit::files` lends it to `answer`; the desktop sim has none
  (`ERR put is not in this build`).
- **`Out`** gains `fresh` (restart the answer's clock after card work),
  `recv` and `now`, defaulted for an `Out` that never hears from the
  host. The unit's `recv` polls the port in the UI loop.
- **The UI holds** while `ls` or `put` runs, as for `card`: at most the
  30 s deadline (a project, about 130 KB, takes about 1 s plus the free
  space count). Audio, MIDI DIN and the watchdog run on.
- **Host:** `python3 tools/chimera-usb.py put <local> <remote>` (a
  `<remote>` ending in `/` takes the local name, upper-cased), waits up
  to 35 s for `READY` and for the answer, shows progress on stderr,
  refuses over 1 MiB before sending, and never retries: one `put`, its
  `ERR` reason, exit 1. `ls` waits 15 s for a cold card. `just put` and
  `just ls` run them.
- **Shipping:** in the default firmware with `usb-console`, not behind a
  dev feature. On a sealed card it is the only way a file gets on.

## Alternatives considered
- **Hex lines:** 32 bytes a 64-character line, 2x on the wire against
  base64's 1.33x, for a decoder no smaller.
- **Raw binary after a length, as `shot` sends:** a lost byte desyncs
  the console for good, and nothing could be typed or aborted.
- **Acks per chunk:** USB CDC's bulk OUT already NAKs while the unit
  isn't reading; the host just writes.
- **The whole file in RAM, then one write:** a project is about 130 KB
  and TAPE's samples are larger; no RAM is free for it.
- **Writing the target in place, or its A/B side directly:** a failed
  transfer would leave a broken target, and a put file is not always
  one of a pair.
- **A dev feature:** the card is sealed on the one unit that needs it.
- **The stock firmware's USB mass storage:** a second USB class, and the
  card mounted by the host while the synth writes it.

## Consequences
- 12,568 B of `.text` and 616 B of `.rodata` in the release firmware
  (fat LTO): the put session, `Fs::rename`, and `FatStore`'s `Store`
  methods now out of line behind `dyn Files`. No static RAM; the put
  session's buffers (a block, a line) are on the stack.
- The free-space count reads the whole FAT: a few seconds on a 32 GB
  card, inside the deadline, every `put`.
- 1 MiB at most for now: TAPE's samples will need the limit and the
  deadline raised, or a streaming deadline, in their own ADR.
- A project loads only as `P#######.A` or `.B` (ADR 0045): a file put
  under another name sits on the card unseen. The host tool doesn't
  rename; the owner picks an unused id (`just ls`). A put `.A` beside an
  existing `.B` is judged by generation like any pair.
- A host that pauses over 500 ms mid-transfer fails the put.
- `get` is left for a follow-up.

## Sources
- ADRs 0034, 0045, 0048, 0068; `chimera-core/src/console/put.rs`,
  `chimera-fat/src/fs.rs` (`rename`), `tools/chimera-usb.py`.
- RFC 4648 (base64); Microsoft FAT specification (fatgen103), directory
  entries and long-name checksums.
