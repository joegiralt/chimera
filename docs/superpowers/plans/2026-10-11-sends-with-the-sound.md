# Sends with the Sound (Phase A) Implementation Plan

**Goal:** Move the chorus, delay and reverb sends from the Part's mix into the Sound. Show them on AMP and on the mixer's SENDS as one value. Make the effect returns default audible. Migrate old projects. Fix SYM.

**Architecture:**
- **Storage.** The sends are three params on the Sound's OUT block (code 18, ids 4–6, idents `SEND1`–`SEND3`, slot-indexed for #461). They are stored sparse, so old Sounds read 0.
- **Audio.** `mix_parts` reads them from the published Sound, with the same ease as before.
- **Migration.** A pre-change project's PART ids 5–7 are scanned out of the payload and written into the Part's Sound when the Part closes.
- **UI.** AMP is rebuilt and SYM moves to a sub-page.
- **SYM.** It gets a bias-only fold law.

**Tech Stack:** Rust `no_std` (`chimera-engine`, `chimera-core`), `chimera-stm32` bench, the TLV card format (ADR 0045), u8g2 faces.

**Spec:** `docs/superpowers/specs/2026-10-11-sends-with-the-sound-design.md` (owner answers 2026-10-11; review (private notes) resolved in its last section). Read it and `CLAUDE.md` before any task.

**Base and ordering.** The Sound library (#459, `sound-library`) and VA (`va-engine`) merge to `main` first. Rebase `sound-sends` onto that `main` before Task 1. If either hasn't merged yet:
- **VA not merged:** `MAX_SOUND_COST` is 2,288 → 2,304 (not 2,352 → 2,368). The VA chain's `FOLDER` doesn't exist yet, so Task 5 touches three chains, not four. When VA lands, its chain gets the same `with_subs`.
- **Library not merged:** skip the library round trip in Task 2. The library branch then picks up the sends with no code of its own (`encode_sound`).
- **Either way:** ADR 0123 and page id 98 are free on every branch, and fixture lines are appended only. Whichever lands second rebases the `disk_codes_v1.txt` and `neutral_v1.txt` appends.

## Global Constraints

- No `unsafe` without a `// SAFETY:` comment. No heap, no panic and no blocking in the audio path. No libc.
- Never snap. The sends are `Motion::Continuous(Smoothing::Own)` (the mixer's `MixState` ease). SYM keeps its `Knobs` glide and ramps.
- Type-driven: ids and labels are consts. Old ids are retired in `RETIRED`, never reused. A choice is never encoded as a bare number where an enum exists (`ChorusMode`, `ReverbType`).
- Commits: terse subject, no type prefix. Stage by path. **Never commit `chimera.bin`.**
- Before running tests after an engine edit: `touch chimera-engine/src/lib.rs` (stale-build guard).
- Worktrees and files are never in `/tmp`. Ledgers go in (private notes).
- Codes are forever. Fixture files only get lines appended. The pre-change fixture (Task 1) is committed before PART 5–7 are removed (Task 2).
- Report the flash delta in one line (`just firmware` size, before and after).
- `just check`, `just test` and `just clippy` are green at the end.

## Review Focus

1. **A pre-change project whose Part's Sound already has non-zero sends.** The Sound's own sends win. Task 3 tests it.
2. **A PART record before `Engine` inside a Part.** The migrated sends still land, because they are applied at `close` and not during the decode. Task 3 tests it.
3. **A NaN, an infinity or an out-of-range value in old PART ids 5–7.** NaN and infinities are dropped (treated as 0). Finite values are clamped to 0..1. Task 3 tests it.
4. **Loading a Sound into a playing Part whose old sends differ.** The send eases and never snaps: per-sample steps stay bounded over the first blocks. Task 2 tests it.
5. **A FOLD sweep at SYM 0 or 1 on a quiet ringing tail (peak 0.01).** No jump between adjacent FOLD ticks, where today's law jumps +2.8 dB. Task 7 tests it.

---

### Task 1: Freeze a pre-change project fixture

**Files:**
- Modify: `chimera-core/tests/codec_compat_test.rs` (writer and pins)
- Create: `chimera-core/tests/fixtures/v1/sends.prj` (generated)

**Interfaces:**
- Produces: `SENDS_FIXTURE: &str = "sends.prj"`. Part 0's mix sends are `[0.25, 0.5, 0.75]`. Part 1's are `[0.5, 0.0, 0.0]`, and its Sound is a Modal INIT. Parts 2–5 have sends 0. The FX are as NEW today (MIX 0). Task 3 reads it.

- [ ] **Step 1:** In `write_v1_fixtures`, add a `sends_fixture_project()` encoded by `encode_project` and written with `write_fixture(SENDS_FIXTURE, ..)`. Print `sends (len, fnv)`.
- [ ] **Step 2:** Run `FIXTURE_WRITE=1 cargo test -p chimera-core --test codec_compat_test -- --ignored --nocapture`. Expect `sends (…, 0x…)` printed and the file created.
- [ ] **Step 3:** Add `sends_fixture_is_frozen()`. It asserts the file's length and `fnv1a_bytes` equal the printed pair.
- [ ] **Step 4:** Run `cargo test -p chimera-core --test codec_compat_test`. Expect PASS.
- [ ] **Step 5:** Commit: `git add chimera-core/tests/codec_compat_test.rs chimera-core/tests/fixtures/v1/sends.prj && git commit -m "Freeze a project with sends in its mix"`.

### Task 2: The sends move to the Sound's OUT block

**Files:**
- Modify: `chimera-engine/src/params.rs` (`OutParams`, `OUT_SPECS`)
- Modify: `chimera-engine/src/part.rs` (drop `sends`, `SEND_*` and specs 5–7)
- Modify: `chimera-engine/src/storage/codes.rs` (`RETIRED`)
- Modify: `chimera-engine/src/storage/sound.rs` (`MAX_SOUND_COST`)
- Modify: `chimera-engine/src/instrument.rs` (`mix_parts`)
- Modify: `chimera-core/src/ui/block_registry.rs` (SENDS cells only)
- Modify: `chimera-stm32/src/bench.rs`
- Modify: `chimera-core/tests/fixtures/disk_codes_v1.txt`, `neutral_v1.txt` (append)
- Test: `part_test`, `sound_codec_test`, `instrument_test`, `gain_staging_test`, `loudness_test`, `load_protocol_test`, `param_sweep_test`, `param_declarations_test`, `part_page_test`, `part_button_test`, `codec_compat_test`, `tests/common/sweep.rs`, `chimera-core/src/ui/mod_grid.rs` (test)

**Interfaces:**
- Produces:
  - `OutParams { volume, pan, vca, vca_vel, pub sends: [f32; FX_SENDS] }`.
  - `OutParams::SEND1 = ParamId(4)`, `SEND2 = ParamId(5)`, `SEND3 = ParamId(6)`, and `OutParams::SENDS: [ParamId; FX_SENDS] = [SEND1, SEND2, SEND3]`.
  - `OUT_SPECS: [ParamSpec; 7]`. Ids 4–6 are `continuous(id, LABEL, ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false, Smoothing::Own)`, with `.ident("SEND1"|"SEND2"|"SEND3")` and `.glyph(FocusGlyph::LevelBar)`. The labels are `"CHORUS"`, `"DELAY"` and `"REVERB"`.
  - `PartParams` has no `sends` and no `SEND_*` consts. `PART_SPECS: [ParamSpec; 7]`. SNAP stays `ParamId(8)` and HELD stays `ParamId(9)`.

- [ ] **Step 1: Write the failing tests**
  - `part_test::part_has_no_sends`: `PART_SPECS.iter().all(|s| ![5, 6, 7].contains(&s.id.0))`.
  - `sound_codec_test::sends_round_trip_and_cost_nothing_at_zero`:
    - a Sound with `params.out.sends = [0.25, 0.5, 1.0]` decodes `bits_eq`;
    - `Sound::init(Algo)`'s encoding is byte-equal to the bytes from before this task (`sends` 0 writes nothing);
    - with the library merged, the same check through a library save and load.
  - `instrument_test`: rename the sends cases to write `shared.parts[0].params.out.sends`. The existing bit-identity asserts stand.
  - `instrument_test::a_sound_load_eases_its_sends` (Review Focus 4):
    - Part 0 plays with sends `[0, 0, 0]`;
    - publish a Sound with `[1, 1, 1]` and a moved `loads`;
    - feed Part 0's bus a constant 1.0. The reverb send bus's last sample in the first block after the load is < 0.5, and it reaches ≥ 0.99 within 50 ms. The send eases rather than snapping.
- [ ] **Step 2:** Run `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --test part_test --test sound_codec_test --test instrument_test`. Expect FAIL (no `SEND1`; `sends` still on `PartParams`).
- [ ] **Step 3: Implement**
  - Change everything listed under Interfaces.
  - `RETIRED` gains `(25, 5), (25, 6), (25, 7)` under a comment: "PART's sends moved to OUT (ADR 0123)".
  - `mix_parts` reads `let [s0, s1, s2] = part.params.out.sends;`.
  - `MAX_SOUND_COST` += 16: 2,304, or 2,368 with VA.
  - SENDS's cells bind `ParamSlot::param(BlockRef::Out, OutParams::SENDn)`.
  - Every `mix.sends` site moves to the Sound: `rg 'mix\.sends|SEND_(CHORUS|DELAY|REVERB)'` must return nothing.
  - `tests/common/sweep.rs` `Param::performance()` also returns true for `BlockRef::Out` with `param` in `OutParams::SENDS`. The sweep's 0.5 wet baseline writes `sound.params.out.sends` after the patch is applied.
  - `param_declarations_test` lists SEND1–3 as `Own`: "the mixer's ease (`MixState`)".
  - `mod_grid.rs`'s `space` assert takes `OutParams::SEND3`.
  - `codec_compat_test`'s pinned mix rows drop ids 5–7.
- [ ] **Step 4:** Append the fixture lines `B 18 4 SEND1 # CHORUS`, `B 18 5 SEND2 # DELAY` and `B 18 6 SEND3 # REVERB` (keep PART 5–7's lines). Add the three neutral fields to `neutral_v1.txt` at 0.
- [ ] **Step 5:** Run `touch chimera-engine/src/lib.rs && just test`. Expect PASS. `disk_codes_test` and `sound_codec_test::neutral_is_a_contract` are green, and the `Voice` and `VOICE_RAM_BUDGET` const asserts compile.
- [ ] **Step 6:** Commit with the subject "Sends live on the Sound's OUT block".

### Task 3: Old projects migrate their sends into each Part's Sound

**Files:**
- Modify: `chimera-engine/src/project/codec.rs` (`ProjectCheck`, PART arm, `close`, `restart`)
- Test: `chimera-core/tests/sends_migration_test.rs` (new)

**Interfaces:**
- Consumes: `OutParams::sends` (Task 2) and `SENDS_FIXTURE` (Task 1).
- Produces:
  - `ProjectCheck::old_sends: Option<[f32; FX_SENDS]>`;
  - private `fn old_sends(payload: &[u8]) -> Option<[f32; FX_SENDS]>` in `codec.rs`, with `const OLD_SEND_IDS: [u8; FX_SENDS] = [5, 6, 7];`.

- [ ] **Step 1: Write the failing tests** in `sends_migration_test.rs` (use `common::codec_util::{decode_project_lossy, encode_project}`):
  - `old_sends_land_in_each_parts_sound`: decode `sends.prj`. Part 0's `sound.params.out.sends == [0.25, 0.5, 0.75]`, Part 1's `[0.5, 0.0, 0.0]`, the rest 0. The returned `lossy` is `true`, and `project_status(&p, template) == ProjectStatus::Modified`.
  - `a_sound_with_sends_keeps_them`: take `sends.prj`'s bytes and splice an OUT block with `SEND1 = 0.1` into Part 0's Sound records. Part 0 ends `[0.1, 0.0, 0.0]`.
  - `part_before_engine_still_lands` (Review Focus 2): Part 0's records reordered so `Block(PART)` comes before `Engine` (`Block(PART)` is `ProjectCheck`'s arm, not `SoundCheck`'s, so the order is legal). The decode is Ok and the sends are `[0.25, 0.5, 0.75]`.
  - `bad_old_sends_are_safe` (Review Focus 3): PART ids 5/6/7 = NaN, +inf and 3.0 give sends `[0.0, 0.0, 1.0]`, and nothing panics.
  - `a_new_project_is_not_lossy`: `encode_project(&Project::new())` round-trips with `lossy == false`.
- [ ] **Step 2:** Run `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --test sends_migration_test`. Expect FAIL (sends 0, `lossy` false).
- [ ] **Step 3: Implement**
  - In the PART arm, before `decode_block`: `if let Some(s) = old_sends(p) { self.old_sends = Some(s); self.lossy = true; }`. This runs in both passes.
  - `old_sends` walks `p[1..].chunks(ENTRY_LEN)`. It maps ids 5–7 to slots 0–2, drops non-finite values as 0, clamps finite values with `clampf(v, 0.0, 1.0)`, and returns `None` when none of the three ids is present.
  - In `close`, at `Context::Part(n)`: after `self.sound.finish(staged)?`, if there is a target, the Part's `sound.params.out.sends == [0.0; 3]`, and `self.old_sends` is `Some(s)`, write `s` through `Tracked`'s `DerefMut`. Then clear `self.old_sends`. Also clear it in `restart` and when a Part context opens.
- [ ] **Step 4:** Run the same command. Expect PASS. Then run `just test`. Expect PASS.
- [ ] **Step 5:** Commit with the subject "Old projects move their sends into each Part's Sound".

### Task 4: The effect returns default audible

**Files:**
- Modify: `chimera-engine/src/dsp/chorus.rs`, `delay.rs` and `reverb.rs` (`Default` and spec defaults)
- Modify: `chimera-engine/src/dsp/fx_bus.rs` (`FxParams::OFF`)
- Test: `block_test`, `reverb_test`, `fx_bus_test`, `loudness_test`, and the audited call sites

**Interfaces:**
- Produces: `pub const OFF: FxParams` on `FxParams`. It is every block's new `Default` with chorus `mode: 0, mix: 0.0`, delay `mix: 0.0` and reverb `mix: 0.0`. `ChorusParams`, `DelayParams` and `ReverbParams` each get a `const fn` default, so that `OFF` can be a `const`.

- [ ] **Step 1: Write the failing tests**
  - `fx_bus_test::new_returns_are_audible`: `FxParams::default()` has `chorus.mode == 1` and `chorus.mix`, `delay.mix`, `reverb.mix` all `== 1.0`. `FxParams::OFF` has those MIXes at `0.0` and `chorus.mode == 0`.
  - `fx_bus_test::a_send_on_new_is_heard`: a NEW `Project`, Part 0 playing INIT with `out.sends = [0.0, 0.0, 0.5]`. The FX return RMS on pair 1 is > −60 dBFS within 0.5 s.
  - Change `reverb_test`'s `p.mix == 0.0` pin to `1.0`.
  - `loudness_test`'s "six Parts, sends up" case uses NEW's FX and stays at or under the ceiling.
- [ ] **Step 2:** Run `touch chimera-engine/src/lib.rs && cargo test -p chimera-core --test fx_bus_test --test reverb_test`. Expect FAIL.
- [ ] **Step 3: Implement** the defaults (struct and spec together, so `block_test::assert_defaults` holds). Then audit every `FxParams::default()`, `ChorusParams::default()`, `DelayParams::default()` and `ReverbParams::default()` call site (`rg`, about 110). Each test that measures a dry path, or assumes an effect is silent, takes `FxParams::OFF` or sets that MIX to 0 explicitly. The bench rows in `chimera-stm32/src/bench.rs` keep their MIX writes.
- [ ] **Step 4:** Run `touch chimera-engine/src/lib.rs && just test`. Expect PASS. Re-bless the `mixer_fx_*` goldens (their MIX values change). Log each re-blessed name in the commit body.
- [ ] **Step 5:** Commit with the subject "Effect returns default to unity; FxParams::OFF for dry tests".

### Task 5: AMP is rebuilt; SYM gets a sub-page

**Files:**
- Modify: `chimera-core/src/ui/block_registry.rs` (`FOLDER`, `FOLDER_SYM`, the chains)
- Test: `part_page_test`, `dpad_nav_test`, `header_map_test`, `screen_golden_test`, and a new `amp_page_test.rs`

**Interfaces:**
- Produces:
  - `pub static FOLDER_SYM: BlockDef` (`id: 98`, `name: "Symmetry"`, `short: "SYM"`, `CellGrid`, `VizType::None`, params `[param(Folder, SYMMETRY), EMPTY × 5]`);
  - `static AMP_SUB_PAGES: [&BlockDef; 1] = [&FOLDER_SYM]`.
  - `FOLDER.params` is `[Out SEND1, Out SEND2, Out SEND3, Folder FOLD, Out VCA_VEL, Folder MIX]`.
  - Every chain's `ChainBlock::page(&FOLDER)` becomes `ChainBlock::with_subs(&FOLDER, &AMP_SUB_PAGES)`. That is ALGO, MODAL, TAPE, and VA if merged.

- [ ] **Step 1: Write the failing tests** in `amp_page_test.rs`:
  - `amp_rows`: `FOLDER.params[i].label()` for i in 0..6 equals `["CHORUS", "DELAY", "REVERB", "FOLD", "VEL", "MIX"]`.
  - `amp_and_sends_are_one_value`: on Part 0, nudging AMP cell 2 by 3 makes the SENDS page's cell 2 read `3.0 / 128.0` (through `PartRead`). Both addresses are equal.
  - `sym_is_one_edit_down`: on every chain whose blocks hold `FOLDER`, the AMP node's `sub_page_count() == 2` and `active_def(1).id == 98`.
  - `labels_fit`: for `"CHORUS"`, `"DELAY"`, `"REVERB"` and `"SYM"`, `draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING) <= theme::CELL_BAR_W`. Today these measure 52, 40, 48 and 26 of 62.
  - `part_page_test`, ADR 0120's test: no engine home page addresses `OutParams::SEND3`, and SENDS and AMP do.
- [ ] **Step 2:** Run `cargo test -p chimera-core --test amp_page_test`. Expect FAIL.
- [ ] **Step 3: Implement** everything listed under Interfaces. `dpad_nav_test` gets EDIT on AMP → SYM and EDIT on SYM → AMP (wraps).
- [ ] **Step 4:** Run `just test`. Re-bless the AMP goldens (every chain) and `mixer_sends`, and add an `amp_sym` golden. Expect PASS.
- [ ] **Step 5:** Commit with the subject "AMP: the three sends over FOLD, VEL, MIX; SYM one page down".

### Task 6: Focus-band hints for the sends and SYM

**Files:**
- Modify: `chimera-core/src/ui/view.rs` (`SlotCtx`, `View::hint`, the tables)
- Modify: `chimera-core/src/ui/renderer.rs` (the caller at `hint::focus(display, f.hint(view.hint()))`)
- Test: `chimera-core/tests/hint_test.rs`

**Interfaces:**
- Produces:
  - `SlotCtx::chorus: ChorusMode`, read in `SlotCtx::read` as `ChorusMode::from_u8(get(BlockRef::Chorus, ChorusParams::MODE) as u8)`;
  - `pub fn hint(&self, ctx: &SlotCtx) -> Hint` on `View`;
  - `const REV_HINTS: [Hint; 6]`, in `ReverbType::ALL` order. Each is `&[Gesture("REVERB ▸", NAME)]` with NAME from `ReverbType::NAMES`.
  - `const CHORUS_HINTS: [Hint; 4]`, each `&[Gesture("CHORUS ▸", NAME)]` with NAME from `chorus::MODE_NAMES`.
  - `const SYM_HINT: Hint = &[Gesture("EDIT ▾", "SYM")]`, used for `Folder FOLD`, `Folder MIX` and `Out VCA_VEL`.
  - DELAY has no hint. MUTE and route hints are unchanged.

- [ ] **Step 1: Write the failing tests**
  - `reverb_hint_names_the_type`: for each `ReverbType`, the REVERB cell's hint is `[Gesture("REVERB ▸", ReverbType::NAMES[t.index()])]`. NEW's is `QUAD`.
  - `chorus_hint_names_the_mode`: likewise over `MODE_NAMES`.
  - `fold_vel_mix_hint_sym`: those three cells give `SYM_HINT`.
  - `hints_draw`: every hint's text is drawable, which means each char is ASCII or one of `draw`'s marks (`▸`, `▾`), and the hint fits the focus band (the existing `hint_test` width helper).
- [ ] **Step 2:** Run `cargo test -p chimera-core --test hint_test`. Expect FAIL.
- [ ] **Step 3: Implement** everything listed under Interfaces. The renderer passes `&f.ctx`.
- [ ] **Step 4:** Run `just test`. Expect PASS.
- [ ] **Step 5:** Commit with the subject "Send cells hint the effect they feed; AMP hints SYM".

### Task 7: SYM's bias-only fold law

**Files:**
- Modify: `chimera-engine/src/dsp/wavefolder.rs` (all three paths: steady, glide sub-blocks, ramp and tapped)
- Test: the `wavefolder.rs` unit tests, plus `param_sweep_test`

**Interfaces:**
- Produces:
  - `const SYM_SPAN: f32 = 1.8`;
  - `const SYM_RAMP: f32 = 0.25`;
  - `fn sym_bias(sym: f32, fold: f32) -> f32 { (sym - 0.5) * SYM_SPAN * (fold / SYM_RAMP).min(1.0) }`.
  - Every path computes `folded = fold_wave(dry * gain + b) - fold_wave(b)` with `b = sym_bias(sym, fold)`. `gain = 1.0 + fold * fold * 6.0` is unchanged. In the ramp path, `rest(fold, sym) = fold_wave(sym_bias(sym, fold))`.

- [ ] **Step 1: Write the failing tests** in `wavefolder.rs`'s `mod tests`. Each uses a reference `old_wet(x, fold, sym)` for today's law. Sine via `(n · 0.0573).sin()`, 1,024 samples, MIX 0.5, run through `Wavefolder::process` block by block.
  - `sym_is_heard_from_quarter_fold`: at peak 0.1 and FOLD ∈ {0.25, 0.5}, the residual RMS of SYM 1 vs SYM 0.5, re the SYM 0.5 output, is ≥ −20 dB.
  - `fold_zero_and_sym_half_are_todays`: FOLD 0 (any SYM) and SYM 0.5 (FOLD ∈ {0.01, 0.25, 0.5, 1.0}) give output bit-identical to `old_wet`'s path.
  - `no_level_rise`: for peaks {0.01, 0.1, 0.3, 0.7}, the max over SYM {0, 0.25, 0.5, 0.75, 1} and FOLD k/128 of out-RMS/dry-RMS is ≤ today's max + 0.5 dB.
  - `fold_sweep_never_steppier` (Review Focus 5): for each peak, the worst RMS change between adjacent FOLD ticks at SYM 1, re the dry RMS, is ≤ max(today's, −20 dB) + 0.5 dB. At peak 0.01 it is ≤ −20 dB (today +2.8).
  - `silence_folds_to_silence`: the existing test stands.
- [ ] **Step 2:** Run `touch chimera-engine/src/lib.rs && cargo test -p chimera-engine wavefolder`. Expect FAIL on `sym_is_heard_from_quarter_fold` and `fold_sweep_never_steppier`.
- [ ] **Step 3: Implement** everything listed under Interfaces.
- [ ] **Step 4:** Run `just test`, including `param_sweep_test`, `the_folder_sym_puts_no_dc_on_the_dac` and `stuck_ok`. Expect PASS. If a FOLD/SYM case fails `stuck_ok`, add a SYM clause only with its measured reason, and record it in the ledger.
- [ ] **Step 5:** Render the owner's A/B pair on the desktop (`just desktop` headless render or a `chimera-utils` recipe): the INIT ALGO stab at FOLD 0.3, with a SYM sweep 0 → 1, on today's law and on the new one. Write them to (private notes). **Owner A/B by ear before merge.**
- [ ] **Step 6:** Commit with the subject "SYM biases after the gain, ramped in with FOLD".

### Task 8: ADR, issue note, QA, flash

**Files:**
- Create: `docs/adr/0123-a-sound-keeps-its-effect-sends.md`
- Modify: `docs/adr/README.md`

- [ ] **Step 1:** Write ADR 0123 from `docs/adr/0000-template.md`, following the spec's ADR note: it supersedes 0120 in part and amends 0043 and 0044. It records:
  - the slot-indexed SEND1–3 and the Phase B meaning change;
  - the Part-close migration;
  - the returns at unity;
  - AMP and SYM;
  - the bias-only law;
  - the ADR 0107 easing exception;
  - KIT's per-voice cost later.

  Add the ADR to the README.
- [ ] **Step 2:** `gh issue comment 247 -R joegiralt/chimera --body "…"`: the sends are Sound params (`Out` SEND1–3), mappable per Part, and CC 91/93 are candidate defaults.
- [ ] **Step 3: Desktop QA**, logged in the ledger:
  - a NEW project with AMP › REVERB up is audible;
  - the mixer's SENDS shows the same value;
  - saving the Sound to the pool and loading it into Part 2 brings the send;
  - `sends.prj` opens Modified, with the sends on the Sounds;
  - SYM sweeps at FOLD 0.3 are audible and smooth.
- [ ] **Step 4:** Run `just check`, `just clippy` and `just firmware`. Expect all green. Report the flash delta against the base in one line.
- [ ] **Step 5:** Commit with the subject "ADR 0123: a Sound keeps its effect sends".
