# 0119. A Sound library on the card: save a Part's Sound, load it back from any project

- **Status:** Proposed
- **Deciders:** owner (design approved 2026-10-10), firmware
- **Amends:** [0081](0081-one-d-pad-on-every-chain.md) and [0043](0043-projects-own-their-sounds.md) (both Proposed).
- **Supersedes in part:** [0086](0086-every-gesture-shows-a-contextual-hint-in-the-focus-band.md), for the Sound browser's hint rows.

## Context
Testers save a good patch, update the firmware and come back to it, or carry a Sound from one project to another. ADR 0045 reserved `FileKind` 1 and `/CHIMERA/SOUNDS`, and nothing wrote or read them. Loading must work after an update: a Sound saved on a build that knows more or fewer params has to load (no-legacy-compat rule), and the card must stay safe: stock PreenFM3 files are never touched (#405).

## Decision
1. **The library file.** `FileKind` 1 in `/CHIMERA/SOUNDS`, `S#######.A|B` (ADR 0045's pair). User ids `S0000001`-`S8999999`, a save allocating top + 1, then the lowest free; factory ids from `S9000001`, never allocated by a save. A save is always a new id; duplicate names are allowed. `SOUNDS` is made on the first save, never at mount.
2. **The `Build` record.** `0x001C`, not critical, written second after `Engine`: version, 8-character hash, dirty flag. It never refuses a file, in any context of any file kind; the stamp is shown, never used to judge a file.
3. **A library load.** Library, then pool, then Part, through a third sealed replace target, `LibraryPart`, deduped by `sound_crc`. It is refused whole when over the budget, and committed with its TAPE source in one step by `Project::adopt`. No card job, project load or SOURCE load starts while one is pending.
4. **The browser.** PROJECT and LIBRARY tabs on encoder B, newest first, 48 a page, cached per volume; the LIBRARY hint lines.
5. **The console.** `ls sounds`, `put` (refuses an existing id without `--over`), `rm` and `get`, confined to `SOUNDS/`.

**Amended and superseded:**
- 0081: MIX + EDIT on the Sound rung saves to the browser tab's source.
- 0043: `adopt` is a second public Part load under the same witness rule, and `LibraryPart` a third replace target.
- 0086: the browser's hint rows (the LIBRARY lines; MIX + MINUS in PROJECT only).
- 0045 (format, names, A/B) and 0087 (budget, meter preview) are unchanged.

## Alternatives considered
- **Judge a file by its stamp** (refuse older or newer builds): a stamp says nothing about what a build can read, and it would strand a tester's Sounds on every update. The decoder skips what it does not know and counts it, so `LOADED · SOME PARAMS SKIPPED` says so.
- **Overwrite or de-duplicate by name:** a save never overwrites; the stamp tells two saves of one name apart. Rename, delete and overwrite on the unit wait for a confirm design.
- **Publish a TAPE source when its file is read:** a Part edited meanwhile would leave a source and no slot. `adopt` re-checks the witness, then commits source, slot and Part together.
- **A `SNDINDEX` file now:** a page lists in one directory pass and 48 peeks; the index is filed if the chip measures over 1.5 s with 500 files.

## Consequences
- flash: 116540 B free (-34004); AXI: 39236 B free (-3032), against origin/main (150544 B and 42268 B free), `chimera-stm32` release.
- A Sound saved on a newer build loads on an older one, with unknown params skipped and counted. A param retired with its id listed counts 0; a migrated or translated one counts 0.
- Desktop QA (headless, a scratch build with one param's id and default moved under a migration and another dropped) loaded a saved Sound back with the toast and a non-silent render.
- The library adds nothing to the audio side; card I/O runs only on the UI loop behind BUSY.
- On the chip, at ship: one page's listing time with 500 files, and the owner's save-on-one-build, load-on-the-next UAT.

## Sources
- `docs/superpowers/specs/2026-10-09-sound-library-design.md` (owner decisions 2026-10-10).
- ADRs 0043, 0045, 0081, 0086, 0087, 0099, 0102; issue #405 (stock files).
- `chimera-fat/tests/stock_files_test.rs`, `chimera-core/tests/library_*_test.rs`.
