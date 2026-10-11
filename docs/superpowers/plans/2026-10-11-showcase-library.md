# Showcase Library Implementation Plan

**Goal:** A card-only showcase factory library for the alpha: a read-only FACTORY tab with one-line descriptions, Sounds and demo projects built from committed text Sources on the host, QA'd by render gates and the owner's ear; plus a TX81Z importer and a private pipeline whose 16 ports never enter git or the public set until #410 clears.

**Architecture:**
- **Firmware** (`chimera-engine`, `chimera-core`): the `Note` record and a 128 B peek; `LibId` split into `UserId`/`FactoryId`; the project id floor; a third browser tab, FACTORY, sharing LIBRARY's page static and load path.
- **Host** (`chimera-desktop`): a `factory-sounds` bin that parses Sources (`listen::tables`), builds card files through the real encoders, renders through the real `Instrument`, gates them and writes the owner's page. A `--private` mode reads port Sources from `$CHIMERA_TX_PORTS` and writes only to `target/factory-sounds/private/`.
- **Privacy walls:** `no_private_ports_in_index` (git index scan, in `just test`/`just check`), the public build's refusal of `from`, the release's port-id check.

**Tech Stack:** Rust `no_std` engine and core; `chimera-hal` `MemStore`/`DirStore` test stores; `listen` (tables, `write_wav`, `lufs`, `qa`, `page.html`); `chimera-utils`; `tools/chimera-usb.py`; `just`.

**Spec:** `docs/superpowers/specs/2026-10-11-showcase-library-design.md` (Approved 2026-10-11, owner decisions at its top)

## Global Constraints

- No new DSP (alpha sound freeze). Showcase engines: VA, ALGO, MODAL, effects. TAPE stays COMING SOON.
- Zero flash for content: every Sound and demo lives on the card. NEW's 8 built-in ALGO Sounds are untouched.
- `Note` is `RecordTag::Note = 0x001D`, not critical, written third (`Engine`, `Build`, `Note`), 1–40 B, chars `A`–`Z`, `0`–`9`, space and `. , ' - + / : ( ) & %`, and must fit `CONTENT_W` in `FONT_LABEL` at `LABEL_TRACKING` (`draw::text_width`). Never written in a project; skipped in every project context; never decoded into `Sound`.
- Peek: `LIB_PEEK` 64 → 128 B.
- Ids: user `S0000001`–`S8999999`; TAPE factory `S9000001`–`S9099999`; showcase `S9100001`–`S9199999`; ports reserved `S9100010`–`S9100025`. Demo projects `P0000001`–`P0000009`; a project save allocates `max(top, 9) + 1`.
- **Ports are private:** no bank file, conversion, Yamaha voice name or port Source (mutated or not) is ever tracked or staged. Port Sources live in `$CHIMERA_TX_PORTS` (default (private notes)); banks in `$CHIMERA_TX_BANKS`; private builds in `target/factory-sounds/private/`. Yamaha TX81Z factory banks only.
- Renders, `report.txt`, `index.html` and `listening.md` go in (private notes) (`$CHIMERA_SHOWCASE_LEDGER` overrides). Never `/tmp` or the scratchpad.
- UI copy is exact: `PROJECT  LIBRARY  FACTORY  B ◂▸`, `NO FACTORY SOUNDS`, `EDIT LOADS · +0.3K`, `WON'T FIT`, `COMING SOON`, `FACTORY · 0.1.0 1A2B3C4D`, `‹ PREVIOUS`, `NEXT ›`. Tab names are never abbreviated (full-word labels).
- No `unsafe` without `// SAFETY:`; no allocation or blocking on the audio side; card I/O only in `card_work`, behind BUSY.
- Touch `chimera-engine/src/lib.rs` before `chimera-core` tests after an engine change.
- Commits: terse, no type prefix; stage by path.
- Flash and AXI deltas reported in one line (Tasks 4 and 15); no byte-shaving rounds.
- `just check` green at the end of every code task.

## Review Focus

1. **A port Source in the tree under another name** (e.g. `tools/factory-sounds/fm.toml` gaining a `from` line): the index scan matches any `.toml` on content, not path. Test in Task 1: `content_match_beats_path`.
2. **A card holding a stray user file in the `S91…` range or a factory file in LIBRARY:** LIBRARY lists only `UserId`, FACTORY only `FactoryId`, both by parse, not by prefix string. Test in Task 4: `tabs_partition_ids`.
3. **A factory Sound loaded then saved:** it becomes a `UserId` file with no `Note`. Test in Task 3: `factory_load_then_save_is_user_without_note`.
4. **A demo project naming a Sound that failed to build or was renamed:** the build fails with the project's line, not a silent empty Part. Test in Task 6: `demo_part_unknown_sound_fails`.
5. **An older firmware reading a described Sound:** loads with `LOADED · SOME PARAMS SKIPPED`, not refused. Test in Task 2: `old_build_skips_note` (decode with `0x001D` treated as unknown).

---

### Task 1: Privacy guard and ignores

**Files:**
- Create: `chimera-desktop/src/privacy.rs`, `chimera-desktop/tests/private_ports_test.rs`
- Modify: `chimera-desktop/src/lib.rs` (`pub mod privacy;`), `.gitignore`

**Interfaces:**
- Produces: `privacy::offences(files: &[(&str, &[u8])]) -> Vec<String>` (path plus reason); `privacy::PORT_IDS: RangeInclusive<u32> = 9_100_010..=9_100_025`.

- [ ] **Step 1: Write the failing tests** in `private_ports_test.rs`:
  - `no_private_ports_in_index`: runs `git ls-files -z` at the workspace root, reads each file, asserts `offences(..)` is empty.
  - `staged_port_source_is_caught`: `git init` a repo in `std::env::temp_dir()/<unique>`, write `x.toml` with `from = "tx81z 1a2b3c4d #7"`, `git add` it, run the same scan, assert one offence naming `x.toml`.
  - `content_match_beats_path`: `offences(&[("tools/factory-sounds/fm.toml", b"from = \"tx81z ab #1\"")])` has one entry.
  - `bank_files_and_private_dir_are_offences`: `a.syx`, `B.SYX`, `c.bnk`, `d.mix`, `e.wav`, `tools/factory-sounds/private/x.toml` each offend; `tools/factory-sounds/va.toml` with no `from` does not; `chimera-desktop/fuzz/seeds/read_wav/float32.wav` does not; a `.md` quoting `from = "tx81z` does not (the content match is `.toml` only).
- [ ] **Step 2:** `cargo test -p chimera-desktop --test private_ports_test` fails (no module).
- [ ] **Step 3:** Implement `offences`. `.gitignore` gains `*.syx`, `*.SYX`, `*.bnk`, `*.mix`, `*.wav`, `!/chimera-desktop/fuzz/seeds/**/*.wav`, `/tools/factory-sounds/private/`. The tracked fuzz seeds under `chimera-desktop/fuzz/seeds/` are the one allowed `*.wav` prefix.
- [ ] **Step 4:** Tests pass; `just test` runs them (it already runs `chimera-desktop --tests`).
- [ ] **Step 5:** Commit `Guard: no TX81Z port, bank or private Source ever in the index`.

### Task 2: `Note` record and 128 B peek (ADR 0133)

**Files:**
- Modify: `chimera-engine/src/storage/record.rs` (`RecordTag::Note`), `chimera-engine/src/storage/library.rs` (`encode_library_sound`, `Peek`, `peek_library`, `LIB_PEEK`), the decoder's skip list and project contexts
- Create: `docs/adr/0133-factory-sounds-list-in-a-read-only-factory-tab-and-carry-a-one-line-description.md`, row in `docs/adr/README.md`; `chimera-engine/tests/fixtures/factory_1.snd`
- Test: `chimera-engine/tests/library_format_test.rs`

**Interfaces:**
- Produces: `pub struct Note([u8; 40], u8)` with `Note::new(&str) -> Option<Note>` (charset and 1–40 B; width is the host's check) and `as_str() -> &str`; `encode_library_sound(s, stamp, note: Option<&Note>, w)`; `Peek.note: Option<Note>`; `LIB_PEEK = 128`.

- [ ] **Step 0:** Re-run `git for-each-ref --format='%(refname)' refs/heads refs/remotes | xargs -I{} git ls-tree --name-only {} docs/adr/ | sort -u`; take the next two free numbers (expected 0133, 0134) and use them throughout.
- [ ] **Step 1: Failing tests:** `note_round_trips` (peek returns the text; decode equals the same Sound encoded without a note); `note_41_bytes_ignored`; `note_bad_char_rejected_by_new`; `second_or_misplaced_note_ignored`; `note_before_engine_is_corrupt`; `note_in_project_contexts_skipped` (Slot, Part, Fx; skip count 0); `old_build_skips_note` (decoder with `0x001D` unknown → `unknown_non_critical_skipped == 1`); `note_golden` (tag `0x1D 0x00`, length, bytes); `v1_fixtures_render_identically` gains `factory_1.snd`.
- [ ] **Step 2:** `cargo test -p chimera-engine --test library_format_test` fails.
- [ ] **Step 3:** Implement. Unprintable bytes in a peeked note map to `?` at draw time (Task 4), not here.
- [ ] **Step 4:** Engine and core tests pass.
- [ ] **Step 5:** Commit `Note record: a factory Sound's one-line description` (with ADR 0133).

### Task 3: `UserId`/`FactoryId` and the project id floor

**Files:**
- Modify: `chimera-engine/src/storage/library.rs` (`LibId`), `chimera-engine/src/project/library.rs` (`save_library_sound`, `save_ab`, `free_id`, `LibFile`), `chimera-engine/src/project/store.rs:154` and `:803`, the console `put`/`rm` callers
- Test: `chimera-engine/tests/library_store_test.rs`

**Interfaces:**
- Produces: `UserId` (1..=8_999_999), `FactoryId` (9_000_001..=9_999_999), `enum LibId { User(UserId), Factory(FactoryId) }` with `LibId::parse(&str)`; `save_library_sound` takes `UserId`; `LibFile` carries `LibId`; `PROJECT_DEMO_TOP: u32 = 9`.

- [ ] **Step 1: Failing tests:** `compile_fail` doctest on `save_library_sound` with a `FactoryId`; `save_on_empty_card_writes_p0000010` (both allocation sites); `save_with_p0000003_present_writes_p0000010`; `save_with_p0000012_present_writes_p0000013`; `factory_load_then_save_is_user_without_note`.
- [ ] **Step 2:** Run; fail.
- [ ] **Step 3:** Implement; allocation is `ProjectId::new(top.max(PROJECT_DEMO_TOP) + 1)`.
- [ ] **Step 4:** `cargo test -p chimera-engine -p chimera-core --features chimera-hal/testkit` passes.
- [ ] **Step 5:** Commit `Factory and user Sound ids are separate types; demos own P0000001-9`.

### Task 4: The FACTORY tab

**Files:**
- Modify: `chimera-core/src/ui/library.rs`, `chimera-core/src/ui/browser.rs` (title, line 2 at `SAVE_Y`, line 3 at `STAMP_Y`), the shared page static (key gains the tab)
- Test: `chimera-core/tests/library_ui_test.rs`, `dpad_nav_test.rs`, `screen_golden_test.rs`, `light_golden_test.rs`

**Interfaces:**
- Consumes: `LibId`, `Peek.note`, the LIBRARY load path (`LibraryPart`, guard, `adopt`).
- Produces: `enum BrowserTab { Project, Library, Factory }`; FACTORY rows lowest id first, 48 a page above a floor.

- [ ] **Step 1: Failing tests:** `tabs_partition_ids` (card with `S0000005`, `S9100001`, `S9150000`: LIBRARY lists one, FACTORY two); `b_turns_three_tabs_and_keeps_tab_for_session` (boot is PROJECT); `factory_mix_edit_and_mix_minus_write_nothing` (counting store); `factory_load_goes_through_guard_and_adopt`; `factory_tape_row_greyed_says_coming_soon`; `factory_pages_48` (50 files: `NEXT ›`, page 2 has 2); `title_fits_content_w` (text-width; if it fails, move `B ◂▸` to hint line 1, never abbreviate); `note_unprintable_draws_question_mark`; goldens dark and INVERT: described row, empty (`NO FACTORY SOUNDS`), NO CARD, greyed COMING SOON, three-tab title.
- [ ] **Step 2:** Run; fail.
- [ ] **Step 3:** Implement; description draws in `INK` with HINTS off too; stamp line `FACTORY · <version> <commit>`.
- [ ] **Step 4:** `SCREEN_DUMP=$PWD/target/screens just screens`; view the new goldens; `just check` green.
- [ ] **Step 5:** Report flash and AXI deltas in one line. Commit `FACTORY tab: read-only factory Sounds with their descriptions`.

### Task 5: Sources and the public build (ADR 0134)

**Files:**
- Modify: `chimera-desktop/src/listen/mod.rs` (`tables` accepts dotted keys), `Justfile` (`factory-sounds`)
- Create: `chimera-desktop/src/bin/factory-sounds.rs`, `chimera-desktop/src/factory_sounds/{mod.rs,source.rs,build.rs}`, `chimera-desktop/tests/fixtures/factory_sounds/golden.toml` and its `.A`, `tools/factory-sounds/{va,fm,modal,projects}.toml` (empty but for a header comment), `docs/adr/0134-factory-sounds-are-text-sources-built-and-checked-on-the-host.md` + README row
- Test: `chimera-desktop/tests/factory_sounds_test.rs`

**Interfaces:**
- Consumes: `encode_library_sound`, `Note`, `Sound::neutral`, `disk_codes_v1.txt` idents, the mod matrix and ALGO tap route tables.
- Produces: `struct Source { id: FactoryId, name, engine, note: Note, demo: PartMode, plays: Vec<Script>, home: u8, params, mods, taps, trim: f32, from: Option<String>, edits: Option<String>, level_by_ear: Option<String>, placeholder: bool }`; `source::parse(text, file) -> Result<Vec<Source>, String>` (errors carry `file:line`); `source::to_sound(&Source) -> Result<Sound, String>`; `source::fmt(text) -> String`; `build::public(dir: &Path, stamp: &BuildStamp) -> Result<Manifest, String>` writing `target/factory-sounds/public/CHIMERA/SOUNDS/` and `manifest.json`.

- [ ] **Step 1: Failing tests:** `golden_builds_to_committed_bytes` (fixed stamp); `fmt_is_idempotent`; `text_sound_text_identical`; `every_committed_source_builds`; bad Sources each fail with their line: `unknown_ident`, `unroutable_destination`, `duplicate_id`, `duplicate_name`, `id_outside_range` (an `S9000005` or `S9200000`), `description_too_wide` (via `draw::text_width`), `description_bad_char`, `public_refuses_from`; `placeholder_skipped`.
- [ ] **Step 2:** Run; fail.
- [ ] **Step 3:** Implement. `fmt` appends `# <display value>` comments; comments are ignored on read.
- [ ] **Step 4:** Tests pass; `just factory-sounds` builds an empty set cleanly.
- [ ] **Step 5:** Commit `factory-sounds: text Sources built through the real encoder`.

### Task 6: Demo projects

**Files:**
- Modify: `chimera-desktop/src/factory_sounds/build.rs`
- Create: `chimera-desktop/src/factory_sounds/project.rs`
- Test: `chimera-desktop/tests/factory_sounds_test.rs`

**Interfaces:**
- Consumes: `encode_project`, NEW's template, the public `Source` list.
- Produces: `project::parse(text) -> Result<Vec<Demo>, String>`; `build::public` also writes `CHIMERA/PROJECTS/P000000n.A`.

- [ ] **Step 1: Failing tests:** `demo_round_trips` (decode the file: `Fx` has REVERB ROOM, Parts 1–2 hold the named Sounds on CH 1–2, POLY); `demo_part_unknown_sound_fails`; `demo_part_private_sound_fails` (names an id in `PORT_IDS`); `demo_id_outside_1_to_9_fails`; `mono_part_mode_set`.
- [ ] **Step 2–4:** Fail, implement, pass.
- [ ] **Step 5:** Commit `Demo projects built from projects.toml`.

### Task 7: Private build

**Files:**
- Modify: `chimera-desktop/src/factory_sounds/build.rs`, `Justfile`
- Test: `chimera-desktop/tests/factory_sounds_test.rs`

**Interfaces:**
- Produces: `build::private(ports: &Path, stamp) -> Result<Manifest, String>` writing only `target/factory-sounds/private/CHIMERA/SOUNDS/`; CLI `factory-sounds --private` (also `fmt --private`, `qa --private`).

- [ ] **Step 1: Failing tests** (port Sources written to a temp dir in the test): `private_requires_from_and_edits`; `private_edits_name_a_category` (filter, fold, LFO, envelope or effects); `private_id_must_be_in_port_range`; `private_never_writes_public_dir`; `private_unset_env_is_a_clear_error` (names `CHIMERA_TX_PORTS`).
- [ ] **Step 2–4:** Fail, implement, pass. The edits re-import check lands in Task 9.
- [ ] **Step 5:** Commit `factory-sounds --private: ports build outside the public set`.

### Task 8: Renders, QA gates and the owner's page

**Files:**
- Create: `chimera-desktop/src/factory_sounds/qa.rs`
- Modify: `chimera-desktop/src/listen/mod.rs` (page rows generalised beyond `TapeModel`), `chimera-utils` (`stim <dir>`)
- Test: `chimera-desktop/tests/factory_sounds_test.rs`, `chimera-utils` tests

**Interfaces:**
- Consumes: `listen::{write_wav, lufs, qa}`, the headless `Instrument` path `loudness_test` uses.
- Produces: `qa::render(&Source, Script) -> Vec<f32>` (48 kHz); `qa::gates(&Source) -> Report` (per gate: value, pass); `qa::ledger() -> PathBuf` (`$CHIMERA_SHOWCASE_LEDGER` else (private notes)); `fmt --level` writes `trim`.

- [ ] **Step 1: Failing tests:** scripts `bass`, `stab`, `short`, `long`, `texture`, `glide` have the spec § 4 durations and notes; `loudness_gate` (−11 LUFS ± 2 LU at home note v100 held 1 s, unless `level = "by ear: …"`); `limiter_gate` (≤ 1 dB single, ≤ 3 dB `stab`); `click_dc_finite_gate`; `aliasing_gate` (≥ 40 dB at C6 and C7, harmonic Sounds only); `tail_gate` (−90 dBFS by release + 2 s; MODAL 10 s); `tap_gate` (≥ 6 dB spectral error with vs without tap); `samey_flags_pair_within_3db_on_32_erb`; `index_only_when_all_pass`; `ledger_never_tmp` (rejects a ledger path under `/tmp` or the scratchpad); `stim_writes_listening_set` (hats, snare WAVs).
- [ ] **Step 2–4:** Fail, implement, pass. The page lists name, description, demo mode, a player per render, LUFS, peak, `edits`; private ports in their own PRIVATE section.
- [ ] **Step 5:** Commit `factory-sounds qa: render gates and the listening page in the ledger`.

### Task 9: TX81Z importer (#302 slice)

**Files:**
- Bring over: `git checkout importer-spec -- docs/superpowers/specs/2026-10-04-patch-importer-design.md docs/adr/0088-import-patches-on-the-host.md` (docs only; add 0088 to the README if missing)
- Create: `chimera-engine/src/import/{mod.rs,tx81z.rs}` (`no_std` VMEM/VCED/ACED parser), `chimera-desktop/src/import/tx81z.rs` (the map, emitting Sources)
- Modify: `chimera-desktop/src/bin/chimera-import.rs` (subcommand `tx`), `Justfile` (`import-tx`), `factory_sounds/build.rs` (edits verification)
- Test: `chimera-engine/tests/tx81z_parse_test.rs`, `chimera-desktop/tests/import_tx_test.rs`

**Interfaces:**
- Produces: `tx81z::parse(&[u8]) -> Result<Bank, ParseError>` (32 voices, checksum warning not error); `import::tx81z::to_source(&Voice, from: &str) -> (Source, Vec<Loss>)`; `just import-tx <bank.syx> [--voices 1,4,9-12] [--out DIR]` (default `$CHIMERA_TX_PORTS`), writing `<voice-number>.toml` and `import-report.txt`, never a Yamaha name.

- [ ] **Step 1: Failing tests:** importer spec § 8.1 for TX81Z (fixtures built in code; `parse_never_panics` proptest; `bad_checksum_warns`); § 8.2 round trip for E rows; the § 6 map rows (ALG n → T n, ops 5–6 LEVEL 0, CRS/FINE via `dsp/algo/tx.rs` indices, DET, OUT, envelope 1:1, KVS → VEL, FBL → op 4 FDBK, OSW → W1–W8, TRPS, LFO → LFO 1 routes); each unmappable item logs one `Loss`; `out_in_work_tree_refused` and `out_ignored_by_git_allowed` (`git check-ignore`); `source_has_no_voice_name`; `edits_verified` (`--private` with `$CHIMERA_TX_BANKS` set fails a port whose named categories equal the re-import).
- [ ] **Step 2–4:** Fail, implement, pass. `just check` green.
- [ ] **Step 5:** Commit `TX81Z importer: banks to private text Sources`.

### Task 10: Install and release

**Files:**
- Modify: `Justfile` (`put-factory-sounds`, `release` packs `factory-v1.zip`)
- Test: `tools/test_chimera_usb.py` (only if `put` changes), a `release --dry-run` check

**Interfaces:**
- Produces: `put-factory-sounds dir="target/factory-sounds/public"` putting `dir/CHIMERA/SOUNDS/*.A` into `/CHIMERA/SOUNDS/` and `dir/CHIMERA/PROJECTS/*.A` into `/CHIMERA/PROJECTS/` with `--over`; `factory-v1.zip` = `CHIMERA/…` + `manifest.json`.

- [ ] **Step 1:** Add the recipes; `release` refuses a zip listing any `S91000(1[0-9]|2[0-5]).A`, and the release notes give both installs (USB console; or the stock bootloader's SD-card-access mode, copying the `CHIMERA` folder to the card root) and the demos' "What to try" column.
- [ ] **Step 2:** Verify: plant `S9100012.A` in a copy of the public dir; `just release 0.1.0 --dry-run` refuses it; without it, the dry run lists the zip.
- [ ] **Step 3:** Commit `Factory set install over USB or the bootloader's card mode`.

### Task 11: Author the VA 9 (content)

**Files:** `tools/factory-sounds/va.toml` (`S9100001`–`S9100009`, spec § 7 table)

Content work, judged by Task 8's gates and the owner's page, not by unit tests. Waits on `sound-sends` for Source-level sends; until then, sends live in the demo Parts.

- [ ] **Step 1:** Write the 9 Sources with the spec's names, descriptions and modulation; every Sound uses velocity on level or tone; short Sounds get a low-pass-gate action.
- [ ] **Step 2:** `just factory-sounds fmt --level && just factory-sounds && just factory-sounds qa`; every gate passes, or a by-ear `level` with its reason.
- [ ] **Step 3:** Commit `VA showcase: nine Sounds`.

### Task 12: Vocal 2 and MODAL 4 (content)

**Files:** `tools/factory-sounds/fm.toml` (`S9100026`–`S9100027`, plus placeholders `S9100028`–`S9100030`), `tools/factory-sounds/modal.toml` (`S9100031`–`S9100034`)

- [ ] **Step 1:** Write them per spec § 7 (vocal: 2040 LP4 REV2 with an op 5/op 6 audio-rate tap on a sounding operator; MODAL models by their frozen idents).
- [ ] **Step 2:** QA as Task 11; `tap_gate` passes for both vocal Sounds.
- [ ] **Step 3:** Commit `Vocal and MODAL showcase Sounds`.

### Task 13: The 4 demo projects (content)

**Files:** `tools/factory-sounds/projects.toml` (`P0000001`–`P0000004`, spec § 5 table: public Sounds only)

- [ ] **Step 1:** Write them; QA renders `stab` and `short` through every reverb TYPE, CHORUS I, II, I+II and the delay, then `chimera-utils stim` hats and snare through the bus.
- [ ] **Step 2:** All gates pass. Commit `Effects demo projects`.

### Task 14: Port and mutate the FM 16, privately (content)

**Files:** none in git. Sources in `$CHIMERA_TX_PORTS`; the bank-to-voice record in (private notes).

- [ ] **Step 1:** `just import-tx` from the Yamaha TX81Z factory banks; pick the nearest voice per spec § 7 row; mutate per its `edits`.
- [ ] **Step 2:** `just factory-sounds --private && just factory-sounds qa --private`; gates pass.
- [ ] **Step 3:** `git status --porcelain` shows nothing from this task; `just test` passes `no_private_ports_in_index`. No commit.

### Task 15: QA pass, owner listening, UAT

- [ ] **Step 1:** Full `just factory-sounds qa --private`; `index.html` in the ledger; publish it as a private artifact for the owner, with renders alongside.
- [ ] **Step 2:** The owner writes verdicts in the ledger's `listening.md`; fix the Sources (at least two rounds expected); re-QA.
- [ ] **Step 3:** `just put-factory-sounds` (public) and, for the owner's unit only, `just put-factory-sounds target/factory-sounds/private`, for the Sounds the owner kept.
- [ ] **Step 4:** `just check` green; flash and AXI deltas for the branch in one line.
- [ ] **Step 5:** Commit any Source fixes `Showcase: fixes from the listening round`.

**Deferred (after #243):** fill `S9100028`–`S9100030`, re-run QA for them. **After #410 clears:** a new ADR supersedes 0134's port rule; ports move into `tools/factory-sounds/fm.toml` and the guard narrows to banks and voice names.
