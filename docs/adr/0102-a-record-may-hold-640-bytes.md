# 0102. A record may hold 640 B (supersedes in part 0045)

- **Status:** Proposed
- **Deciders:** firmware (filter-types Task 11)

## Context
ADR 0045 bounds a known record's payload at `MAX_RECORD_LEN` = 512 B, the framer's one buffer. A Sound's `Routes` record is 4 B a route, and the matrix held at most 8 sources × 16 columns = 128 routes: 512 B exactly. ADR 0096's four permanent columns (FREQ, Q, VCA, FOLD) sit outside the registry's 16, so a full matrix is 8 × 20 = 160 routes, 640 B. At 512 the writer's `RecordBuf` would panic on save.

## Decision
- `MAX_RECORD_LEN` is **640 B**: 8 × (16 + 4) × 4. `storage/sound.rs` asserts that the largest `Routes` record fits.
- Only the bound changes. The rest of 0045 (TLV records, frozen codes, A/B files, the two-pass load) stands. The disk format gains no field: a fixed column's route is an ordinary route record with its address.

## Alternatives considered
- **Cap a Sound at 128 routes:** a matrix with free cells that refuses a route, or a save that drops one. Both lose what the owner patched.
- **A second `Routes` record for the fixed columns:** a new tag and decoder path for what one more 128 B already carries.
- **Keep the four fixed columns inside the 16:** evicts routes from full patches (spec § 7 "Why reserved").

## Consequences
- The framer's buffer, `SoundCheck`'s staged routes and the writer's `RecordBuf` each grow 128 B; none is an AXI resident.
- The stack check (ADR 0025) charges `encode_sound`'s frame to every pointer call, three deep, so its three `RecordBuf`s became one, reused: `bench,orbit` nested 58,064 B then (60,720 before the fixed columns); three 640 B buffers had reached 65,816 B, past the limit. Measured since: 59,560 B on the finished branch, 62,532 B of 65,280 merged with the sound pool (main alone 62,416); default 51,260 B.
- A file written with more than 128 routes is `Bounds` on firmware from before this ADR. There are no users yet (old patches must load safely, not the reverse).

## Sources
- ADR 0045, ADR 0096; spec `docs/superpowers/specs/2026-10-05-filter-types-design.md` § 7 "Four permanent columns".
