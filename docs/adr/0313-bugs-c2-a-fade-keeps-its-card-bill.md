# 0313. The bugs-c2 batch: a fade keeps its card bill, the stream rows follow their card, SOURCE names whose heads it keeps

- **Status:** Proposed
- **Deciders:** firmware (bugs-c2 batch); the owner ruled #764 and #599 on 2026-11-22 ("go with your recommendations")

## Context
- #764: `admit_stream` and `admit_mid` dropped a stolen voice's bill as soon as it was stolen and ranked a dying voice first among victims, but a stolen voice keeps streaming through its fade. Under hammered chords the card's load passed `STREAM_BUDGET_BPS` (a host model of #705's run read 376,953 B/s against 315,978). That is the likeliest cause of #705's two underruns.
- #599: a streamed source two Parts play keeps one pair of RAM heads, placed for the first TAPE Part that plays it. The other Part's START and loop start cold, and nothing said so.
- #1083: with no fault set, a card op's mount that found another card left the old card's stream rows readable for up to a pass.

## Decision
- **A fade keeps its card bill until its voice is freed, and a dying voice is never a victim (#764).** `stream::admit::admit_on` admits a note onto a voice's ring. The ring already carries a fade or nothing, and the two bill the larger. Every other voice's bill counts, a dying one's too. A steal frees one live voice, released first by `Age::rank`, and the note takes that voice, queued behind its fade, only if the larger of the two bills fits. Otherwise the note is refused and nothing is stolen, since a second victim would free nothing before its fade ends. `pick_streamed` first tries the allocator's pick, then a fade already running (a cut's, a shed's, with no steal), then one victim. A note bound to its own voice (a MONO retrigger, a re-strike) can't move, so it is refused instead of stealing. `admit_mid` steals released voices only while the live voices leave no room, never when even all of them wouldn't make room. The raise then waits at its cap until the fades end.
- **Steal counts are pinned:** at most one steal per note-on. The victim is the top-ranked one (`Age::rank`); a note that doesn't fit behind its fade is refused with nothing stolen, and no lower-ranked victim is tried. A note can't move off its own voice (a MONO retrigger, a re-strike) or past its Part's cap (ADR 0281): no fade and no victim for it, refused when its own ring has no room. A MONO retrigger needing room is refused: owner ruling R2 (2026-11-18) now reads "the same as AUTO", and both refuse while a fade would run.
- **The stream rows follow their card (#1083).** `CardFault::Moved`: `FatStore::mount` (and the desktop's `DirStore`) raises it whenever the volume differs from the last mount's, through `Medium::moved`, before the caller sees the volume. The card IRQ (`serving`) and the decks treat it as a fault. `Pool::streams_on` holds the volume the rows are on. `Project::follow_card(v)` drops rows on another volume (`lose_streams`), and both `SourceCell::recover` (every pass, card `Ready`) and the streamed commit (on the read's volume) call it. A `StreamLoad` keeps the volume its first pass read and refuses a pass on another, rows or none. `recover` mounts again under `Moved` (no re-init), so `card` and the rows follow the volume the store holds even after a mount that bypassed `card` (the console's, a sweep's, the install's), then clears it. `Moved` is raised before `mounted` publishes the card's address, and never over a pending `Read`, whose re-init follows the new card all the same. This replaces ADR 0309's `SourceCell::vol`. `Store::clear_read_fault` has no default: every store says what it clears.
- **A quiet's warm return needs the head to hold the block and its fade ahead.** Otherwise the cursor starts cold, with no underrun counted.
- **SOURCE names whose heads it keeps (#599).** The first-Part rule stays. On another Part playing the same streamed source, touching SOURCE shows `PART n · HOLDS THE RAM HEADS` (`view::HEADS_HINTS`, from `view::heads_of`), inside the band.
- Minors from #1081's review: `StreamedPass` (was a second `Streamed`), and `StreamLoad::found` returns `NotFound::{File, NoRoom}`.

## Alternatives considered
- Steal several voices for one note and admit it later: notes are refused, not deferred, so the victims would be lost for nothing.
- Stamp every registry row with its volume: more bytes per row for the same answer as one `Pool::streams_on`.
- A head pair per Part (#599): an arena cost the owner declined.

## Consequences
- On a full card a note can be refused where it used to steal and briefly overrun. #705 and the B4 stress run are owed on the unit (#1026).
- A card swap fades the streams for one pass, then they read SOURCE MISSING.
- Flash, measured on 0.4 at decc5b19d: bank 1 +24 B over 0.4's ORBIT line (911,148 B), −160 B default; bank 2 +2,624 B ORBIT, +2,592 B default. Bank 1 keeps only what runs closed or has to: `CardFault::Moved`, `FatStore`'s last volume and the door calls. The heads hint (`notes::Ask::Heads`), `follow_card` and the streamed pass (`LoadAsk`, one door entry: the load door's table is full) are bank 2's.

## Sources
- Issues #599, #705, #764, #1083; ADR 0183 (steal order), 0281 (caps), 0309.
