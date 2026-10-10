# Sound library (alpha): Design

- **Status:** Approved by the owner 2026-10-10 (§ Owner decisions); adversarial review fixes applied 2026-10-10 (private notes, § Review resolutions)
- **Date:** 2026-10-09
- **Builds on:** `2026-09-28-projects-storage-design.md` § Library (plan 3, never built). This spec takes the alpha slice of it and leaves out tags, `SNDINDEX`, overwrite, rename and delete on the unit.
- **Ships in:** alpha

## Intent

Testers save a good patch, update the firmware, and come back to it: to check it still works, or to compare it before and after a fix. They also carry Sounds from one project to another. The library is the place for that. It lives on the card, outside every project.

**In alpha:**
1. Save a Part's Sound to `/CHIMERA/SOUNDS`, one file per Sound, with a name.
2. Browse the library from any project and load a Sound into the Part (through the pool, ADR 0043).
3. Each file is stamped with the build it was saved on (`0.1.0 02e440e2`), and the browser shows it.

Loading always works after an update. Unknown or removed params load safely; the sound may change with fixes (no-legacy-compat rule).

**Not in alpha:** rename, delete, overwrite or organise on the unit (USB only); tags and `SNDINDEX`; the ~250-Sound factory library (#317); the per-Part CC map (#247). Factory files get their own id range now (§ Files), so #317 slots in without a format change.

**Done when:**
- MIX + EDIT in the LIBRARY tab (or SETTINGS › PART › SAVE TO LIBRARY) names and saves the active Part's Sound as a new file, on the desktop and on the chip;
- the LIBRARY tab lists the card's Sounds newest first, each row showing its name and engine, and the selected row showing its build stamp;
- EDIT on a row copies the Sound (and a TAPE Sound's source) into the pool and loads it into the Part, through the guard, or refuses with a reason and changes nothing;
- every v1 Sound fixture and a stamped library fixture load and render identically (`codec_compat_test`);
- #405's stock-files test passes with library saves, console `put`/`rm`, listings and loads added;
- the owner saves a Sound on one build, flashes the next, and loads it back by ear (UAT).

## Files

ADR 0045 already reserves the name, `FileKind` 1 (Sound) and the format. The library adds nothing to the envelope.

| Item | Rule |
|---|---|
| Path | `/CHIMERA/SOUNDS/S#######.A` and `.B` (ADR 0045, `Dir::Sounds`) |
| Display name | The header's `Name<16>`; the 8.3 stem is only an id |
| User ids | `S0000001`…`S8999999`. A save takes the highest user id on the card + 1; when that is past `S8999999`, the lowest free user id. The directory pass runs in the same `Card::run` as the write, so two saves can't collide, and one huge id can't block saves |
| Factory ids | `S9000001`…`S9999999` (`factory_tapes::sound_stem` already writes these). Never allocated by a save; left out of LIBRARY |
| Duplicate names | Allowed (ADR 0045). No NAME EXISTS prompt: a save never overwrites, and the stamp tells two saves of `BASS-1` apart |
| Every user id taken | `NO SOUND IDS LEFT`, nothing written |
| Any other name in `SOUNDS/` | Ignored by the listing, never opened, never written |

**Body:** `Engine`, then **`Build`**, then the rest of `encode_sound`'s records unchanged (sparse against `Sound::neutral`, ADR 0087). One encoder still writes the arena, projects and library files: `encode_sound` is split after its `Engine` record, and the library writer puts `Build` between the two parts. Host writers (the TAPE factory set, the importer #302) write user- or factory-range files through the same writer and stamp what they were built from (`dev` on host builds).

**`Build` record** (new, `0x001C`, not critical):

| Bytes | Field |
|---|---|
| 12 | version, ASCII, NUL-padded (`CARGO_PKG_VERSION`, `0.1.0`); `const _: () = assert!(VERSION.len() <= 12)` |
| 8 | hash, ASCII, NUL-padded (`CHIMERA_HASH`: 8 hex digits, `dev` on host builds, `unknown` without git) |
| 1 | flags: bit 0 dirty (`CHIMERA_DIRTY`); other bits 0 when written, ignored when read |

- **Informational: `Build` never refuses a file.** The decoder reads it as it reads an unknown record. A payload under 21 B, a second `Build`, or `Build` anywhere but second shows `NO STAMP`; bytes past 21 are ignored (a later build may grow it); any byte outside printable ASCII draws as `?`.
- It is the second record because `SoundCheck` skips an unknown record only after `Engine` (`sound.rs`, since 13cd8349). Every alpha build already reads a stamped file and ignores the stamp; `unknown_non_critical_skipped` pins that and stays.
- Header 28 + `Engine` 5 + `Build` 25 = 58 B, so a listing peeks the first **64 B** of a side and has name, engine and stamp. Shorter files (an unstamped Sound can be 50 B) are peeked whole.
- A file without it (the v1 fixtures, today's factory TAPE Sounds) shows `NO STAMP`.
- `Build` inside a project's `Slot` or `Part` is skipped; projects never write it.
- `SoundCheck::seen` needs no new bit: `Build` is never once-checked.
- Far under ADR 0102's 640 B. A Sound file is at most `MAX_SOUND_COST` + 37 B (header 28 + `Build` 25 + trailer 4 − the 16 B name the cost counts).
- **Later records:** the per-Part CC map (#247, "saved with the Part, in the project file and in library sounds") becomes a non-critical Sound-level record after `Build`, so alpha files simply lack it. `0x0C`–`0x0F` stay reserved for the project's own records.

**Writes are A/B (ADR 0045).** A first save writes `.A` at generation 1 through `save_ab`. `pick`, `write_target` and the two-pass load are reused. A/B matters for overwrite later and for a console `put --over`.

## Browser

The Sound rung's browser gets two tabs: **PROJECT** (today's list: 128 pool slots, then one INIT per engine) and **LIBRARY**. **Encoder B** turns between them (the projects spec's "B picks the source"). MINUS/PLUS still move between Parts (ADR 0081), and A scrolls. The tab is kept for the session, not per Part, and is PROJECT at boot.

**Title row:** `PROJECT  LIBRARY  B ◂▸`, then `· P1` at the right. The current tab is `INK`, the other `FAINT`; the marks are ADR 0086's. `P n` keeps the Part on screen while MINUS/PLUS move it (it replaces `LOAD SOUND · PART n`).

**LIBRARY rows:** name, the estimated cost (§ Load), the engine tag (as PROJECT's rows).

| Row | Look | EDIT |
|---|---|---|
| A Sound | Normal | Loads (§ Load) |
| A side whose header needs newer firmware | Greyed, name kept | Refused: `NEEDS NEWER FIRMWARE` |
| A side under 32 B by the directory, or a bad header (magic, kind, name) | Greyed, `—` for name, the `FileError` message (`FILE IS CUT SHORT`…) | Refused with that message |
| A side that peeks fine but fails its CRC or decode | Normal: the peek has no CRC | Refused on EDIT with the `FileError` message; never a wrong load |
| `‹ NEWER` (first row, past page 1) | `MID` | Lists the previous 48 |
| `OLDER ›` (last row, when more) | `MID` | Lists the next 48 |
| Empty library | One dimmed row: `NO SOUNDS YET` (line 2 gives the keys) | Nothing |
| No card or an unreadable one | One dimmed row: the `StoreError` message (`NO CARD`, `CARD IS EXFAT: FORMAT FAT32`…) | Nothing |

**Order and paging.** User Sounds, **newest first** (highest id first), 48 a page (`MAX_LISTED`). Newest first because the tester's question is about the patch they just saved. The paging rows are new behaviour (LOAD PROJECT's `MORE ON CARD` runs nothing); they are list rows run by EDIT, so no new key.

A page is one card job behind BUSY:
1. one directory pass keeps the 48 highest user ids below the page's floor, with the sides each has and their sizes;
2. a peek of each side the pass saw, no other. A missing name would scan the whole directory.

The page is cached for the session, keyed by `VolumeId`. A save, a console `put` or `rm`, or a card change drops it, and so does any `VolumeChanged`. Turning B back to LIBRARY shows the cache without card work.

**MIX + MINUS** acts in PROJECT only (clear a slot). In LIBRARY it does nothing (no delete in alpha), so it has no hint there.

**The three hint lines (ADR 0086),** in LIBRARY:

| Line | Text |
|---|---|
| 1 | `EDIT LOADS · +0.3K`, or `WON'T FIT` in `ALERT` |
| 2 | `MIX + EDIT SAVES TO LIBRARY` |
| 3 | The selected row's stamp: `SAVED ON 0.1.0 02E440E2`, `SAVED ON THIS BUILD`, or `NO STAMP` |

- Line 3 is information, not a hint: it draws with HINTS off too.
- `+` after the hash marks a dirty build.
- **`THIS BUILD`** only when both hashes are equal, neither is dirty, and the hash is neither `dev` nor `unknown`. Otherwise the stamp shows, so desktop saves (`dev`) always show it.
- The pool meter stays, with the selected row previewed in `ACCENT`, or `ALERT` when it won't fit (ADR 0087's preview).

**Newer and older builds.** The stamp is shown, never judged: a git hash has no order, and every alpha build is `0.1.0`. What loads is decided by the format alone:
- a file from an older build always loads (frozen codes, migrations, the neutral base);
- a file from a newer build loads unless it carries something critical this build lacks (an engine: a VA Sound on a pre-VA build). That is greyed `NEEDS NEWER FIRMWARE`, with its stamp, so the tester knows which build to go back to;
- what pass 2 dropped is counted (below), and the load says `LOADED · SOME PARAMS SKIPPED`.

**The skip count** counts, in pass 2 only:
- an unknown block code;
- an unknown param id that isn't retired;
- an enum code this build doesn't know and that isn't retired;
- an unknown non-critical record (`Build` isn't one);
- an unknown `ModSource`;
- a route or registry address `addr_of` can't resolve.

A param a migration or translation consumes, or a readdress (DRIVE onto LEVEL), is not a loss and doesn't count. A block this engine doesn't hold isn't counted either: today's encoder never writes one.

## Save

**Keys.** MIX + EDIT in the LIBRARY tab, or SETTINGS › PART › **SAVE TO LIBRARY** (a sixth row, after LIVE SNAP). On the Sound rung MIX + EDIT saves to the source the tab shows: the pool in PROJECT (unchanged), the library in LIBRARY. This amends ADR 0081's Sound-rung row (ADR 0119). The Digitakt's sound browser does the same with its pool and +Drive.

1. NAMING opens (`NamingFor::SaveToLibrary`, title `SOUND NAME`, crumb `SAVE TO LIB`), pre-filled with the Part's Sound name. SEQ saves, MENU cancels (modal, ADR 0081).
2. `Job::LibrarySave` under BUSY, one `Card::run`:
   - `make_dir(Chimera)`, then `make_dir(Sounds)`: made on the first save, never at mount;
   - one directory pass for the id (§ Files);
   - `save_ab` onto the empty pair: `.A`, generation 1. The header holds the typed name, then `Engine`, `Build`, the records and the trailer, then a flush;
   - pass 1 on what was written, as `put` checks. A failure deletes that side, so the card is as it was.
3. Toast `SAVED TO LIBRARY`. The page cache drops, and page 1 re-reads with the cursor on the new row.

- The Part's own Sound is saved, edited or not, without its mix. The Part, its Origin, its marks and the project don't change: the library is outside the project (ADR 0043).
- The Part keeps its name; the typed name is the file's only.
- A TAPE Sound saves its `SourceRef` only. Its source is already on the card in `/CHIMERA/TAPES`, write-once (ADR 0087); a save never writes sample data.

| Fails with | Says | Card |
|---|---|---|
| No card, unsupported card, timeout, I/O | `StoreError::message` toast | Unchanged |
| `Full` | `CARD FULL` | The partial side deleted. If that delete fails too, the row lists and EDIT refuses it |
| `VolumeChanged` | `CARD CHANGED`; the cache drops | Unchanged |
| Every user id taken | `NO SOUND IDS LEFT` | Unchanged |
| Cut mid-write | — | A zero-length or short `.A`, greyed `FILE IS CUT SHORT` or refused on EDIT. The next save takes the next id, never this one; `rm` over USB removes it |

## Load

EDIT on a library row:

1. **Guard first, no card.** `ReplaceGuard::check` with a new sealed target **`LibraryPart { part, file: LibFile }`**. It is a third `Target` beside `PartSource` and `ProjectSource`, with `PartSource`'s `at_risk` and `witness`. `load_part` never sees a library source, so nothing can reach it untyped. `LibFile` is the id, the `VolumeId` and the listed `Seen` (generation and name, as `OverwriteTarget` holds), never parsed data (ADR 0043).
2. **Answers.** A Clean or Stale Part goes straight on. An Edited Part asks **SAVE PART FIRST / REPLACE / CANCEL**:
   - REPLACE confirms (`Pending::anyway`);
   - SAVE PART FIRST runs `save_first`'s save, then re-checks through `save_then`;
   - CANCEL drops it.

   Either confirmation queues `Job::LibraryLoad(Confirmed<LibraryPart>)`. It never calls `replace_part`.
3. **`Job::LibraryLoad`** under BUSY, in one card job (several UI passes when a source is read):
   - **The file.** The listed `Seen` is re-checked against the headers: a side changed is `FILE CHANGED`, a missing pair `FILE IS GONE`, and either drops the cache. Then a two-pass load with `load_ab_in_place` and a `LibraryDecoder` (`SoundCheck` plus the skip count) decodes onto the pool's scratch Sound (`Pool::scratch`). Pass 2 writes scratch only after a clean pass 1, so no whole `Sound` sits on the stack (`just stack-check`'s frame limit).
   - **Dedupe.** A pool slot whose `sound_crc` equals the decoded Sound's is reused, not copied again.
   - **Budget.** Against the pool as it is after any SAVE PART FIRST, so a save that took the room refuses here. The load needs:
     - the Sound's exact cost (sparse encoding + 16 B name, rounded to 16 B) and a free Sound slot, unless deduped;
     - for a TAPE Sound whose source the pool doesn't hold, the source's cost (from its `SourceRef`) and a free source slot (32).

     Over either: refused before anything moves, `POOL FULL` then `NEEDS 1.2K, 0.4K FREE` in the browser's band (ADR 0087's refusal, unchanged).
   - **The source.** A TAPE source the pool doesn't hold is read from `/CHIMERA/TAPES/<CRC>.SRC` into the arena's staging (`Project::source_staging`) by SOURCE's card-row reader, 4 KiB a UI pass behind BUSY; MENU cancels. It is **not published**: the staging is unpublished until `adopt`. Not on the card: the Sound loads with the source absent and says `SOURCE MISSING`, as a project load does (ADR 0099). This supersedes the pool spec's `SOURCE NOT ON CARD` refusal for library loads.
   - **Commit.** `Project::adopt(Confirmed<LibraryPart>, staged: Option<(SourceRef, usize)>) -> Result<Adopted, AdoptError>` re-checks the witness first, then the budget. Only after both does it commit the staged source, store the slot from scratch (first free) and load the Part from it. A refusal changes nothing (`Changed`, `Full`); there is no orphan source and no stored slot.
   - **While a load is pending** (scratch or staging held), every other card job, project load and console card verb is refused (`card_pending`; the console answers `ERR busy`). MIDI and knobs still edit the Part; a moved witness refuses at `adopt`.
4. The browser goes to the Part's pages, as a PROJECT load does. One toast, by precedence: `SOURCE MISSING`, then `LOADED · SOME PARAMS SKIPPED`, then `LOADED TO SLOT 12`.

- The Part's Origin is the new (or deduped) slot, so it reads Clean. The pool changed, so the project reads Modified (ADR 0043).
- `adopt` is a second public Part load beside `replace_part`, under the same witness re-check. This amends ADR 0043's "only public Part load" (ADR 0119).
- **Slot choice:** always the first free slot (or the deduped one), and the Part is always replaced. That is an alpha deviation from #317's "pick a library Sound, then a pool slot" and the projects spec's "EDIT on an empty slot aims LIBRARY at it"; #317 adds "add to slot".
- **Estimate shown before EDIT:** the row's cost is about the file's length − header − trailer − `Build` + 16, rounded to 16 B. It is not a bound. A translation can add an entry: v1 Modal's EXCITE sets BURST. A TAPE source isn't in it, because its `SourceRef` sits past the peek. The check after the read is the real one.

| Fails with | Says | State |
|---|---|---|
| Card errors | `StoreError::message` | Pool and Part unchanged |
| `NeedsNewerFirmware`, `BadCrc`, `Truncated`, `Corrupt`, `Bounds`… | `FileError::message` | Unchanged (pass 2 runs after a clean pass 1; scratch only) |
| File re-written or removed since listed | `FILE CHANGED` / `FILE IS GONE`; re-list | Unchanged |
| Over budget, no Sound slot, no source slot | `POOL FULL` / `NEEDS …` | Unchanged |
| Witness moved | `prompt::refused(Changed)` | Unchanged; staging dropped |
| MENU during the source read | `CANCELLED` | Unchanged; staging dropped |

## Audio and card rules

- All card work runs in `card_work` on the UI loop behind BUSY, never in the audio path. The audio plays its last snapshot meanwhile (ADR 0021).
- A library load is a Part replace, not a project swap: no epoch bump (ADR 0046/0099 don't apply). An engine change fades through `Voice::render`'s switch fade, as any slot load does.
- The TAPE source is staged in the arena's gap while voices play and published with `Release` in `adopt`, as SOURCE's card-row load does (ADR 0087). No allocation; no new statics on the audio side.
- **Stock files (#405):** the library writes only `/CHIMERA/SOUNDS/S#######.A|B`, opens only those names, and makes `SOUNDS` only on the first save. ADR 0094's sweep walks every directory on the volume and is unchanged.
- **Card inside the case:** files move over USB. The console gets these verbs, all behind `card-put`, absolute paths only, any case:

| Command | Does |
|---|---|
| `ls sounds` | The library's files and sizes, as `ls tapes` |
| `put /CHIMERA/SOUNDS/S#######.A <size> <crc32>` | A Sound file saved the unit's way (A/B, next generation, the kind's check after the write), as a project `put`. An id with a pair already on the card refuses, `ERR put: exists`; `put --over` replaces it. The host script takes `ls sounds`' top user id + 1. `S9…` ids are accepted (factory install) under the same rule |
| `rm /CHIMERA/SOUNDS/S#######` | Deletes the pair, older side first (ADR 0045's delete order). A torn pair deletes; a missing one answers `ERR rm: not found`. `S9…` refuses in alpha (`ERR rm: factory`) |
| `get /CHIMERA/SOUNDS/S#######` | The picked side's bytes, base64, then its CRC-32. Both sides torn answers `ERR get:` with the `FileError` message |

Any other directory, `..`, `//` or a malformed stem is `ERR <verb> takes …`. A `put` or `rm` drops the browser's page cache.

## Memory and flash

- **RAM:** the library page, 48 entries × about 52 B (id, name, engine, stamp, `Seen`, sides, length, state), about 2.5 KB of AXI, a static like LOAD PROJECT's listing. ADR 0087's linker margin (32 KiB shipping) checks it; report the delta.
- **Stack:** no whole Sound: the decoder is `SoundCheck` (with its 640 B route buffer) over the pool's scratch. `just stack-check` covers the job.
- **Flash:** a few KB (listing, tab, jobs, console verbs). Report it in a line (flash rule).

## Code layout

- `chimera-engine/src/storage/library.rs`: `LibId`, the id ranges and allocation, `BuildStamp` (a value; the engine has no build env), its encode and lenient peek, `library_file(id)`, `save_library_sound`, `list_library(floor)`, `LibraryDecoder`. Pure apart from the `Store` calls.
- `chimera-engine/src/storage/record.rs`: `RecordTag::Build` (`0x001C`). `sound.rs`: the split of `encode_sound`, `Build` skipped, and the skip count.
- `chimera-engine/src/project/library.rs`: `LibraryPart`, `LibFile`, the scratch-decoding load step (scratch is `pub(in crate::project)`), and `Project::adopt`.
- `chimera-core/src/ui/about_page.rs`: `BuildStamp::THIS` from `CHIMERA_HASH`/`CHIMERA_DIRTY`, passed into the engine's writer and comparer.
- `chimera-core/src/ui/browser.rs`: the tab, LIBRARY rows, line 3.
- `chimera-core/src/ui/settings/`: `Job::LibraryList`, `Job::LibrarySave`, `Job::LibraryLoad`; `NamingFor::SaveToLibrary`; the PART row.
- `chimera-core/src/console/`: `ls sounds`, `put`, `rm`, `get`.
- No `chimera-hal` change: `Dir::Sounds` exists, and the desktop's `DirStore` maps it already.

## Testing

- **Format:** a golden for `Build`'s tag and layout. `Build` with 30 B and with 3 B both load (the stamp's first 21 B, or `NO STAMP`). Two `Build`s load. `Build` before `Engine` stays `Corrupt` (an unknown record before `Engine`, today's rule). A project Sound with `Build` loads. `unknown_non_critical_skipped` (old builds skip it) stays.
- **Fixtures:** a stamped `library_1.snd` joins `tests/fixtures/v1/`. Every v1 `.snd` loads through the library loader and renders identically (`v1_fixtures_render_identically`).
- **Skip count:** a Sound with an unknown param id, an unknown block code and an unknown non-critical record counts 3. Every v1 fixture and factory Sound, saved and loaded on one build, counts 0. A retired, translated param counts 0.
- **Ids:** with `S0000003`, `S9000001` and a stock `BANK_1.BNK` on the card, a save writes `S0000004`. With `S8999999` and `S0000001` present, a save writes `S0000002`. With every user id taken (a fake store), `NO SOUND IDS LEFT`, nothing written.
- **Listing:** newest first. 100 Sounds page as 48, 48, 4, with `‹ NEWER` and `OLDER ›` where they apply. A zero-length side and a newer-firmware header list greyed with their messages. Names outside `S#######.A|B` and sides the directory pass didn't see are never opened (a counting store). The cache serves a second open with no card read, and a save drops it.
- **Save failures:** `Full` mid-write leaves the directory as before. A cut at every block of a save leaves either no file or one greyed or refused on EDIT, never a wrong load (the storage tests' torn-write harness).
- **Load:**
  - the guard prompts on an Edited Part;
  - REPLACE and SAVE PART FIRST each queue the load;
  - SAVE PART FIRST that leaves too little room refuses with POOL FULL and keeps the save;
  - dedupe reuses an equal slot;
  - over budget, and a full source table, refuse with the pool, Part and meter unchanged;
  - a translated file whose exact cost exceeds its estimate refuses cleanly;
  - a witness that moves after the source read leaves no source and no slot;
  - a missing TAPE source loads with `SOURCE MISSING`;
  - MENU in the source read leaves everything as it was;
  - a file rewritten or removed after listing refuses `FILE CHANGED` / `FILE IS GONE`.
- **Console:** `ls sounds`; `put` onto a free id, onto an existing id (`ERR put: exists`) and with `--over`; `rm` of a pair, of a torn pair, of a missing one and of `S9…`; `get` of a pair and of a pair with both sides torn; `..`, `//`, `/CHIMERA/PROJECTS/…` with `rm`, and lowercase paths. Each card verb answers `ERR busy` while a library load is pending.
- **Stock files (#405):** the card-image test adds a save, a console `put` and `rm` of a Sound, a listing of every page and a load. It still finds every stock file byte-identical and nothing written outside `/CHIMERA/SOUNDS`.
- **Fuzz:** a stamped seed for `decode_sound`, and a `peek_library` target over arbitrary 0–64 B.
- **UI:**
  - screen goldens: both tabs with `P n`, an empty library, NO CARD, a greyed row, the paging rows, `WON'T FIT`, line 3's three forms, and NAMING's SAVE TO LIB;
  - `dpad_nav_test`: B's turn, MIX + EDIT in LIBRARY, and MIX + MINUS doing nothing in LIBRARY;
  - the hint-width test covers the new lines.
- **Desktop QA:** save on one build. Rebuild with a changed param default and a retired param, and load it back: it loads, says what it skipped, and renders.
- **On the chip:** one page's listing time with 500 files in `SOUNDS/`, measured at ship. Expect about 0.5 s: about 48 lookups at ADR 0099's 5–12 ms each. Over 1.5 s, file `SNDINDEX` (plan 3) as a GitHub issue.

## Later (not alpha)

- Rename, overwrite and delete on the unit, each behind a confirm (projects spec decision 9). MIX + MINUS gets its library meaning then.
- Tags, the engine filter on C, and `SNDINDEX` (projects spec § Library index).
- The FACTORY tab over `S9…` (#317), read-only, and "add to slot".
- The CC map record in library Sounds (#247).
- Audition a library Sound before loading it.
- A `Build` stamp on project files.

## ADRs

ADR **0119** is new. The numbers before it are taken: 0113 by modal-exciters, 0114 on main, 0115–0117 by knobs-rare, 0118 by VA. It records:
- **The library file:**
  - `FileKind` 1 in `/CHIMERA/SOUNDS`;
  - user ids `S0000001`–`S8999999`, allocated top + 1, then lowest free;
  - factory ids from `S9000001`;
  - a save is always a new id, and duplicate names are allowed.
- **The `Build` record:** `0x001C`, not critical, second after `Engine`. It never refuses a file, and the stamp is shown, never used to judge one.
- **A library load:**
  - library → pool → Part through the third sealed target `LibraryPart`, deduped by `sound_crc`;
  - refused whole when over the budget;
  - committed with its TAPE source in one step by `Project::adopt`.
- **The browser:**
  - PROJECT and LIBRARY tabs on encoder B, newest first, 48 a page, cached per volume;
  - the LIBRARY hint lines.
- **The console's** `ls sounds`, `put` (refuses an existing id without `--over`), `rm` and `get`.

**What it amends or supersedes:**
- ADR 0081, Proposed: amended. MIX + EDIT on the Sound rung saves to the tab's source.
- ADR 0043, Proposed: amended. `adopt` becomes a second public Part load under the same witness rule, and `LibraryPart` is a third replace target.
- ADR 0086, Accepted: superseded in part, for the browser's hint rows (the LIBRARY lines; MIX + MINUS in PROJECT only).
- ADR 0045 (format, names, A/B) and ADR 0087 (budget, meter preview) are unchanged.

## Owner decisions (2026-10-10)

All four open questions were answered with the recommendation:
1. Loading lives in a PROJECT/LIBRARY tab on encoder B in the Sound browser.
2. Newest first, 48 a page.
3. The USB console gets `ls sounds`, `put`, `rm` and `get` for library files.
4. The stamp is version + hash only, with `THIS BUILD` when equal. No commit count, and no OLDER/NEWER claim.

## Review resolutions

Every finding in (private notes) was accepted.

| # | Finding | Resolution |
|---|---|---|
| M1 | ADR 0081/0043 contradicted | 0119 amends both (both Proposed); 0086 superseded in part (Accepted) |
| M2 | TAPE source published before `adopt` | Source staged unpublished; `adopt` re-checks, then commits source, slot and Part; other card work refused while pending |
| M3 | No route from the prompt to the job | `LibraryPart` target; REPLACE / SAVE PART FIRST queue `Job::LibraryLoad`; budget after the save |
| M4 | `Build` could refuse files | Lenient: never refuses; first 21 B; `?` for non-printables; version assert |
| M5 | Console `put`/`rm` lose data | `put` refuses an existing id without `--over`; lowest-free fallback; `rm` refuses `S9…`; absolute paths; `card-put`; tests |
| m6 | Skip count undefined | Defined; pass 2 only; zero-count test; toast precedence |
| m7 | Estimate not a bound | "About", 16 B rounding, source slot in the budget, test |
| m8 | Sound on the stack | In-place decode into scratch; `just stack-check` cited |
| m9 | Layout wouldn't compile | Scratch step in `project/`; `BuildStamp` passed from core |
| m10 | `THIS BUILD` too broad | Equal, clean, and not `dev`/`unknown` |
| m11 | Listing verdicts overstated | Peek verdicts only; CRC on EDIT; messages corrected |
| m12 | Listing cost; no cache | Peek seen sides only; per-volume session cache |
| m13 | Stale `LibId` | `LibFile` carries `Seen`; `FILE CHANGED` / `FILE IS GONE` |
| m14 | Browser details | MIX + MINUS gated; `P n` kept; empty row shortened; paging rows new |
| m15 | #247, #317, pool spec, #302 | Paragraphs added in § Files and § Load |
| m16 | Small inaccuracies | Sweep scope, + 37 B, old-build proof, fuzz targets |
