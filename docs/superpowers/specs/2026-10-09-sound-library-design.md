# Sound library (alpha): Design

- **Status:** Approved by the owner 2026-10-10 (§ Owner decisions)
- **Date:** 2026-10-09
- **Builds on:** `2026-09-28-projects-storage-design.md` § Library (plan 3, never built). This spec takes the alpha slice of it and leaves out tags, `SNDINDEX`, overwrite, rename and delete.
- **Ships in:** alpha

## Intent

Testers save a good patch, update the firmware, and come back to it: to check it still works, or to compare it before and after a fix. They also carry Sounds from one project to another. The library is the place for that. It lives on the card, outside every project.

**In alpha:**
1. Save a Part's Sound to `/CHIMERA/SOUNDS`, one file per Sound, with a name.
2. Browse the library from any project and load a Sound into the Part (through the pool, ADR 0043).
3. Each file is stamped with the build it was saved on (`0.1.0 02e440e2`), and the browser shows it.

Loading always works after an update. Unknown or removed params load safely; the sound may change with fixes (no-legacy-compat rule).

**Not in alpha:** rename, delete, overwrite or organise on the unit (USB only); tags and `SNDINDEX`; the ~250-Sound factory library (#317). Factory files get their own id range now (§ Files), so #317 slots in without a format change.

**Done when:**
- MIX + EDIT in the LIBRARY tab (or SETTINGS › PART › SAVE TO LIBRARY) names and saves the active Part's Sound as a new file, on the desktop and on the chip;
- the LIBRARY tab lists the card's Sounds newest first, each row showing its name and engine, and the selected row showing its build stamp;
- EDIT on a row copies the Sound into the pool and loads it into the Part, through the guard, or refuses with a reason and changes nothing;
- every v1 Sound fixture and a stamped library fixture load and render identically (`codec_compat_test`);
- #405's stock-files test passes with library saves, listings and loads added;
- the owner saves a Sound on one build, flashes the next, and loads it back by ear (UAT).

## Files

ADR 0045 already reserves the name, `FileKind` 1 (Sound) and the format. The library adds nothing to the envelope.

| Item | Rule |
|---|---|
| Path | `/CHIMERA/SOUNDS/S#######.A` and `.B` (ADR 0045, `Dir::Sounds`) |
| Display name | The header's `Name<16>`; the 8.3 stem is only an id |
| User ids | `S0000001`…`S8999999`. A save takes the highest user id on the card + 1, from a directory pass in the same `Card::run` as the write, so two saves can't collide |
| Factory ids | `S9000001`… (`factory_tapes::sound_stem` already writes these). A user save ignores them when finding the highest id |
| Duplicate names | Allowed (ADR 0045). No NAME EXISTS prompt: a save never overwrites, and the stamp tells two saves of `BASS-1` apart |
| Ids exhausted | `NO SOUND IDS LEFT`, nothing written |
| Any other name in `SOUNDS/` | Ignored by the listing, never opened, never written |

**Body:** `Engine`, then **`Build`**, then `encode_sound`'s records unchanged (sparse against `Sound::neutral`, ADR 0087). One encoder still writes the arena, projects and library files: the library writer splits `encode_sound` at the `Engine` record and puts `Build` between.

**`Build` record** (new, `0x001C`, not critical):

| Bytes | Field |
|---|---|
| 12 | version, ASCII, NUL-padded (`CARGO_PKG_VERSION`, `0.1.0`) |
| 8 | hash, ASCII, NUL-padded (`CHIMERA_HASH`: 8 hex digits, `dev` on host builds, `unknown` without git) |
| 1 | flags: bit 0 dirty (`CHIMERA_DIRTY`); other bits 0 |

- It is the second record because `SoundCheck` skips an unknown record only after `Engine` (today's firmware reads a stamped file and ignores the stamp). Header 28 + `Engine` 5 + `Build` 25 = 58 B, so a listing reads the first **64 B** of a side and has name, engine and stamp.
- A file without it (the v1 fixtures, today's factory TAPE Sounds) reads as `NO STAMP`.
- `Build` in a project's `Slot` or `Part` context is skipped, never written there.
- `SoundCheck::seen` is a `u8` with all eight bits taken; it becomes a `u16` for `Build`'s once-bit. A second `Build` is `Corrupt`.
- Record size is far under ADR 0102's 640 B. A Sound file is at most `MAX_SOUND_COST` + 61 B.

**Writes are A/B (ADR 0045).** A first save writes `.A` at generation 1. `write_target`, `pick` and the two-pass load are reused as they are. A/B matters for `put` replacing a file today and for overwrite later.

## Browser

The Sound rung's browser gets two tabs: **PROJECT** (today's list: 128 pool slots, then one INIT per engine) and **LIBRARY**. **Encoder B** turns between them. That is the projects spec's ruling ("B picks the source"); MINUS/PLUS already move between Parts there (ADR 0081), and A scrolls. The tab is remembered per session, not per Part, and resets to PROJECT at boot.

**Title row:** `PROJECT  LIBRARY`, the current tab in `INK`, the other in `FAINT`, and `B ◂▸` after them (the ADR 0086 marks): the turn's hint is on screen in both tabs.

**LIBRARY rows:** name, then the engine tag (as PROJECT's rows). The cost column shows the Sound's estimated pool cost (§ Load).

| Row | Look | EDIT |
|---|---|---|
| A Sound | Normal | Loads (§ Load) |
| `NEEDS NEWER FIRMWARE` side | Greyed, name kept | Refused: `NEEDS NEWER FIRMWARE` |
| Torn or damaged, no side readable | Greyed, `—` for name | Refused with the `FileError` message |
| `‹ NEWER` (first row past page 1) | `MID` | Lists the previous 48 |
| `OLDER ›` (last row, when more) | `MID` | Lists the next 48 |
| Empty library | One dimmed row: `NO SOUNDS YET · MIX + EDIT SAVES ONE` | Nothing |
| No card or an unreadable one | One dimmed row: the `StoreError` message (`NO CARD`, `CARD IS EXFAT: FORMAT FAT32`…) | Nothing |

**Order and paging.** User Sounds, **newest first** (highest id first), 48 a page (`MAX_LISTED`, as LOAD PROJECT). Newest first because the tester's question is about the patch they just saved. The paging rows are list rows, run by EDIT like LOAD PROJECT's `MORE ON CARD`, so no new key. Each page is one card job behind BUSY: one directory pass that keeps the 48 highest ids below the page's floor, then a 64 B peek of each side the pass saw. Factory ids (`S9…`) are left out of LIBRARY; #317 adds a FACTORY tab on B's turn with the same listing over the other range.

**The three hint lines (ADR 0086),** in LIBRARY:

| Line | Text |
|---|---|
| 1 | `EDIT LOADS · +0.3K`, or `WON'T FIT` in `ALERT` |
| 2 | `MIX + EDIT SAVES TO LIBRARY` |
| 3 | The selected row's stamp: `SAVED ON 0.1.0 02E440E2`, `SAVED ON THIS BUILD`, or `NO STAMP` |

- Line 3 is information, not a hint: it draws with HINTS off too.
- `+` after the hash marks a dirty build.
- MIX + MINUS does nothing in LIBRARY (no delete in alpha), so it has no hint there.
- The pool meter stays, with the selected row previewed in `ACCENT`, or `ALERT` when it won't fit (ADR 0087's preview).

**Newer and older builds.** The stamp is shown, never judged: a git hash has no order, and every alpha build is `0.1.0`. What loads is decided by the format alone:
- a file from an older build always loads (frozen codes, migrations, neutral base);
- a file from a newer build loads unless it carries something critical this build doesn't know (an engine it lacks: a VA Sound on a pre-VA build). That is greyed `NEEDS NEWER FIRMWARE`, with its stamp, so the tester knows which build to go back to;
- params, blocks, records or routes the decoder skipped are counted, and the load's toast says `LOADED · SOME PARAMS SKIPPED`.

## Save

**Keys.** MIX + EDIT in the LIBRARY tab, or SETTINGS › PART › **SAVE TO LIBRARY** (a sixth row after LIVE SNAP). Same keys as PROJECT's save, so the meaning follows the tab, as the Digitakt's sound browser does with its pool and +Drive.

1. NAMING opens (`NamingFor::SaveToLibrary`, title `SOUND NAME`, crumb `SAVE TO LIB`), pre-filled with the Part's Sound name. SEQ saves, MENU cancels (modal, ADR 0081).
2. `Job::LibrarySave` under BUSY, one `Card::run`:
   - `make_dir(Chimera)`, `make_dir(Sounds)`: made on the first save, never at mount;
   - one directory pass for the highest user id;
   - write `.A`, generation 1: header with the typed name, `Engine`, `Build`, the records, the trailer; flush;
   - pass 1 on what was written (the kind's check, as `put` does). A failure deletes that side, so the card is as it was.
3. Toast `SAVED TO LIBRARY`. The listing re-reads page 1 with the cursor on the new row.

- The Part's own Sound is saved, edited or not, without its mix. The Part, its Origin, its marks and the project don't change: the library is outside the project (ADR 0043).
- The Part keeps its name; the typed name is the file's only.
- A TAPE Sound saves its `SourceRef` only. Its source is already on the card in `/CHIMERA/TAPES`, write-once (ADR 0087); a save never writes sample data.

| Fails with | Says | Card |
|---|---|---|
| No card, unsupported card, timeout, I/O | `StoreError::message` toast | Unchanged |
| `Full` | `CARD FULL` | The partial side deleted (best effort; a cut leaves a torn row, below) |
| `VolumeChanged` | `CARD CHANGED`; the listing re-reads | Unchanged |
| Ids exhausted | `NO SOUND IDS LEFT` | Unchanged |
| Cut mid-write | — | A torn `.A`: listed greyed `FILE CHECKSUM FAILED`; the next save takes the next id, never this one; removed over USB |

## Load

EDIT on a library row:

1. **Guard first, no card.** `ReplaceGuard::check` with `PartFrom::Library(LibId)` (the projects spec's `PartFromLibrary`). An Edited Part asks **SAVE PART FIRST / REPLACE / CANCEL**, as a slot load does. `LibId` is the id and `VolumeId`; the `Pending` holds the id, never parsed data (ADR 0043).
2. **`Job::LibraryLoad(Confirmed<PartSource>)`** under BUSY:
   - two-pass load of the pair (`load_ab`, `pick`) with `SoundDecoder` onto the pool's scratch Sound (`Pool::scratch`). The decoder's staged Sound sits on the job's stack, within ADR 0025's 8 KB frame;
   - **dedupe:** a pool slot whose `sound_crc` equals the decoded Sound's is reused, not copied again;
   - **budget:** else the exact cost (the Sound's sparse encoding + its 16 B name, plus a TAPE source the pool doesn't hold, from its `SourceRef`) against `Pool::free()` and a free slot. Over it: refused before anything is stored, `POOL FULL` then `NEEDS 1.2K, 0.4K FREE` in the browser's band (ADR 0087's refusal, unchanged);
   - a TAPE source the pool doesn't hold is read from `/CHIMERA/TAPES/<CRC>.SRC` by SOURCE's card-row loader (4 KiB a UI pass, BUSY, MENU cancels). Not on the card: the Sound loads with the source absent and says `SOURCE MISSING`, as a project load does (ADR 0099);
   - `Project::adopt(confirmed, &scratch)` stores the slot (first free) and loads the Part from it in one step. It re-checks the witness: `Changed` refuses and drops the slot it stored, so the pool and Part move together or not at all.
3. The browser goes to the Part's pages, as a PROJECT load does. Toast `LOADED TO SLOT 12`, or `LOADED · SOME PARAMS SKIPPED`.

- The Part's Origin is the new slot, so it reads Clean. The pool changed, so the project reads Modified (ADR 0043).
- **Estimate shown before EDIT:** the row's cost is the file's length − header − trailer − `Build` + 16. This firmware re-encodes the Sound and drops what it doesn't know, so the exact cost is at most the estimate. A TAPE source's cost isn't in the estimate: its `SourceRef` sits at the end of the file, past the 64 B peek. The refusal after the read covers it.

| Fails with | Says | State |
|---|---|---|
| Card errors | `StoreError::message` | Pool and Part unchanged |
| `NeedsNewerFirmware`, `BadCrc`, `Truncated`, `Corrupt`, `Bounds`… | `FileError::message` | Unchanged (pass 2 commits only after a clean pass 1, ADR 0045) |
| Over budget or no free slot | `POOL FULL` / `NEEDS …` | Unchanged |
| Witness moved | `prompt::refused(Changed)` | Unchanged |
| MENU during the source read | `CANCELLED` | Unchanged (the slot isn't stored until the source is in) |

## Audio and card rules

- All card work runs in `card_work` on the UI loop behind BUSY, never in the audio path. The audio plays its last snapshot meanwhile (ADR 0021).
- A library load is a Part replace, not a project swap: no epoch bump (ADR 0046/0099 don't apply). An engine change fades through `Voice::render`'s switch fade, as any slot load does.
- A TAPE source lands in the arena's gap while voices play, which SOURCE's card-row load already does (ADR 0087). No allocation; no new statics on the audio side.
- **Stock files (#405):** the library writes only `/CHIMERA/SOUNDS/S#######.A|B`, opens only those names, and makes `SOUNDS` only on the first save. ADR 0094's sweep is unchanged: it already walks `/CHIMERA`.
- **Card inside the case:** files move over USB. The console gets:

| Command | Does |
|---|---|
| `ls sounds` | The library's files and sizes, as `ls tapes` |
| `put SOUNDS/S0000012.A <size> <crc32>` | A Sound file saved the unit's way (A/B, next generation, the kind's check after the write), as a project `put` |
| `rm SOUNDS/S0000012` | Deletes the pair, older side first (ADR 0045's delete order). Refused for any other directory |
| `get SOUNDS/S0000012` | The picked side's bytes, base64, then its CRC |

## Memory and flash

- **RAM:** the library listing, 48 entries × 48 B (id, name, engine, stamp, length, state) ≈ 2.3 KB of AXI, a static like LOAD PROJECT's listing. ADR 0087's linker margin (32 KiB shipping) checks it; report the delta.
- **Stack:** the load job holds one staged Sound (`SoundDecoder`) and borrows the pool's scratch: within the 8 KB frame.
- **Flash:** a few KB (listing, tab, jobs, console verbs). Report it in a line (flash rule).

## Code layout

- `chimera-engine/src/storage/library.rs`: `LibId`, the id ranges, `Build` encode and peek, `library_file(id)`, `save_library_sound`, `list_library(floor)`, `load_library_sound`. Pure apart from the `Store` calls.
- `chimera-engine/src/storage/record.rs`: `RecordTag::Build` (`0x001C`); `sound.rs`: the once-bit and the skip counter.
- `chimera-engine/src/project/`: `PartFrom::Library`, `Project::adopt`.
- `chimera-core/src/ui/browser.rs`: the tab, LIBRARY rows, line 3.
- `chimera-core/src/ui/settings/`: `Job::LibraryList`, `Job::LibrarySave`, `Job::LibraryLoad`; `NamingFor::SaveToLibrary`; the PART row.
- `chimera-core/src/console/`: `ls sounds`, `put`, `rm`, `get`.
- No `chimera-hal` change: `Dir::Sounds` exists, and the desktop's `DirStore` maps it already.

## Testing

- **Format:** golden for `Build`'s tag and layout; a stamped Sound loads on today's decoder with the stamp skipped (a fixture written by today's encoder plus `Build`); `Build` before `Engine` is `Corrupt`; two `Build`s are `Corrupt`; a project Sound with `Build` loads.
- **Fixtures:** a stamped `library_1.snd` joins `tests/fixtures/v1/`. Every v1 `.snd` loads through the library loader and renders identically (`v1_fixtures_render_identically`).
- **Skips counted:** a Sound with an unknown param id, an unknown block code and an unknown non-critical record loads, and its count is 3.
- **Ids:** with `S0000003`, `S9000001` and a stock `BANK_1.BNK` on the card, a save writes `S0000004`. `S8999999` present: `NO SOUND IDS LEFT`, nothing written.
- **Listing:** newest first; 100 Sounds page as 48, 48, 4 with `‹ NEWER` and `OLDER ›` where they apply; a torn side and a newer-firmware side list greyed with their messages; names outside `S#######.A|B` are never opened (a counting store).
- **Save failures:** `Full` mid-write leaves the directory as before; a cut at every block of a save leaves either no file or a greyed torn one, never a wrong load (the storage tests' torn-write harness).
- **Load:** guard prompts on an Edited Part; dedupe reuses an equal slot; over budget refuses with POOL FULL and the pool, Part and meter unchanged; a moved witness refuses and leaves no stored slot; a missing TAPE source loads with `SOURCE MISSING`; MENU in the source read leaves everything as it was.
- **Stock files (#405):** the card-image test adds a save, a listing of every page and a load, and still finds every stock file byte-identical and nothing written outside `/CHIMERA/SOUNDS`.
- **UI:** screen goldens for both tabs, an empty library, NO CARD, a greyed row, the paging rows, `WON'T FIT`, line 3's three forms, and NAMING's SAVE TO LIB. `dpad_nav_test` adds B's turn and MIX + EDIT in LIBRARY. The hint-width test covers the new lines.
- **Desktop QA:** save on one build, rebuild with a changed param default and a retired param, load it back: it loads, says what it skipped, and renders.
- **On the chip:** one page's listing time with 500 files in `SOUNDS/`, measured at ship (expected about 0.5 s: a directory lookup per side read, ADR 0099's 5–12 ms). Over 1.5 s files `SNDINDEX` (plan 3) as a GitHub issue.

## Later (not alpha)

- Rename, overwrite and delete on the unit, each behind a confirm (projects spec decision 9); MIX + MINUS gets its library meaning then.
- Tags, the engine filter on C, and `SNDINDEX` (projects spec § Library index).
- The FACTORY tab over `S9…` (#317), read-only.
- Audition a library Sound before loading it.
- A `Build` stamp on project files.

## ADRs

One new ADR, **0119** (0113 is modal-exciters', 0114 is on main, 0115–0117 are knobs-rare's, 0118 is VA's), records:
- the library file: `FileKind` 1 in `/CHIMERA/SOUNDS`, user ids `S0000001`–`S8999999`, factory ids from `S9000001`, a save always a new id, duplicate names allowed;
- the `Build` record (`0x001C`, not critical, second after `Engine`) and that a stamp is shown, never used to judge a file;
- a library load is library → pool → Part through `ReplaceGuard` with `PartFrom::Library`, deduped by `sound_crc`, refused whole when over the budget;
- the browser's PROJECT and LIBRARY tabs on encoder B, newest first, 48 a page, and the LIBRARY hint lines. This amends ADR 0086's browser rows; ADR 0081's key map is unchanged;
- the console's `ls sounds`, `put`, `rm` and `get` for library files.

ADR 0045 already covers the format, names and A/B; ADR 0043 the copy rule and guard; ADR 0087 the budget and the meter's preview. None of them is edited.

## Owner decisions (2026-10-10)

All four open questions were answered with the recommendation:
1. Loading lives in a PROJECT/LIBRARY tab on encoder B in the Sound browser.
2. Newest first, 48 a page.
3. The USB console gets `ls sounds`, `put`, `rm` and `get` for library files.
4. The stamp is version + hash only, with `THIS BUILD` when equal. No commit count, and no OLDER/NEWER claim.
