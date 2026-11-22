# 0309. The bugs-c batch: the UI clears a stream read fault, POOL FULL, a warm return from a quiet

- **Status:** Proposed
- **Deciders:** firmware (bugs-c batch, 0.4 bug sweep); the owner rules on the items listed under Consequences

## Context
The 0.4 bug sweep's TAPE batch (C) found these live on origin/0.4:

- #610: `SharedCard::mounted` cleared the card IRQ's read fault inside the mount, so the IRQ could read again before `lose_streams` freed another card's stream rows; only `recover`'s own mount ran `lose_streams`; and NO CARD during a fault forgot the volume (`Card::Absent`), so the same card back dropped every streamed source.
- #765: a project load's pass 2 left a source absent when it fit whole after another's read missed (`LoadKind::Whole => continue`), and a source that didn't fit (heads with no room, a whole read with no room) read SOURCE MISSING.
- #768: a failed SRAM4 self-test was only reported; the stream rings in it were still lent.
- #608: a write's quiet always ended cold, dropping a RAM head that still held the read.
- #524: SCRUB's jog moved POSITION without the hand's `cc_hand`, so a mapped POSITION ramp fought the reel.

## Decision
- **The UI clears a stream read fault (#610).** `Medium::mounted` no longer touches the fault. `Store::clear_read_fault` (a default no-op; `FatStore` to `SharedCard`, the desktop's `DirStore`) clears it, and only `SourceCell::recover` calls it, after a successful re-init and mount and after any `lose_streams`. Ordering: `clear_all`'s row writes (SeqCst CAS and stores, epoch `Release`) are sequenced before the fault's `Release` store; the card IRQ's `serving()` loads the fault `Acquire` before it reads a row, so its first read after the clear sees the rows freed or stale.
- **Streams follow the volume the UI last saw.** `SourceCell::vol` holds the volume mounted at the last pass; a pass that finds `Card::Ready` on another volume, by `recover`'s mount or any card op's since, runs `lose_streams`. NO CARD changes nothing there, so the same card back keeps its rows. A `Stuck` backoff ends at once when a card op's mount finds a card (that mount no longer clears the fault).
- **`Absence::NoRoom` (#765)**: a source on the card that the load had no room for, whole or as heads, reads `POOL FULL` on the focus band and `NO ROOM` in its cell; `Missing` is kept for a damaged file or one naming another source. Pass 2's streamed pass (`LoadDoor::sources`) returns `Streamed { n, whole }`, the sources that fit whole once a whole read missed, and `load_into` reads those whole after it (bank 1's `read_sources`, no new monomorph in bank 1).
- **Only a passed SRAM4 lends stream rows (#768)**: `Sram4::lends_streams` is `Ok` only; on `Failed` (or `Untested`) the registry is never lent, nothing streams, a source too big for the arena reads STREAMS FULL, and whole sources play. `status` already prints `sram4 FAIL`.
- **A short quiet returns warm (#608)**: on release, a cursor whose RAM head still holds the read plays on from it, its gain rising over `DROP_FADE`; past the head it starts cold as before.
- **The jog is the hand (#524)**: `jog_pass` calls `cc_hand` on POSITION whenever it moves it.

## Alternatives considered
- Keep the clear in `mounted` and check volumes in the card IRQ: puts a volume compare on the interrupt's path and still leaves `lose_streams` unrun on an op's mount.
- Track the streams' volume in the registry or project: more state in bank 1's project than one `Option<VolumeId>` in the cell, for the same answer.
- Read a now-fitting source whole inside bank 2's streamed pass: needs `read_source` over `dyn Store`, a new monomorph that would land in bank 1.
- A dedicated SRC label for a failed SRAM4: a UX call, left to the owner; STREAMS FULL is what an unlent registry already reads.

## Consequences
- A failed or slow re-init is retried at its backoff or at the next card op's mount, never cleared silently by that mount.
- The whole-fit re-read runs after the streamed pass, so a whole source after a streamed one in id order may find the heads took its room and read POOL FULL; the plan's costing by Part params (#600) is the full fix.
- Cell and band text gain `NO ROOM` and `POOL FULL` for a didn't-fit source; no golden shows one.
- For the owner: #599 (a head pair per Part, an arena cost), #764 (a dying voice keeps its bill: a steal-count change wanting the unit's B4 run), #790 (SCRUB/GRAIN's DC, a sound change), #705 (unreproduced on the unit), #600 (library loads and pass 1's plan stream: a bank-2 door for library loads).

## Sources
- Issues #524, #608, #610, #765, #768; slice 1 spec §§ 5.5, 5.6, 7.3, 9.
- ADR 0087 (sources held while sounding), 0137 (the load door), 0194 (bank 2 required).
