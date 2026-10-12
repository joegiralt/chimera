# Phase A: effect sends saved with the Sound

Owner decision 2026-10-11, for alpha. Phase B (configurable FX slots,
#461) is out of scope; this spec's storage is built so it needs no second
format break.

## Problem

Hearing reverb takes three knobs in three places: the Part's send (mixer
SENDS › REV), the reverb's MIX (its return) and the reverb's setup. Every
effect's MIX defaults to 0 (`ChorusParams`, `DelayParams`, `ReverbParams`
`Default`; chorus MODE OFF too), so a send turned up on a NEW project is
silent. The sends live in `PartParams::sends` (`chimera-engine/src/part.rs`,
ids 5–7, `Block(PART)` code 25), so a Sound loaded into another Part, or
from the library (ADR 0119), loses its amount, and three Sounds can't keep
three sends. ADR 0120 just removed SPACE, the duplicate reverb-send cell.

Model: the Digitone. A Sound keeps its send amounts; the effects stay at
project level (Chimera: `FxParams`, the FX bus).

## Decisions

### 1. Storage: three params on the Sound's OUT block

| | |
|---|---|
| Block | `OutParams` (`BlockRef::Out`, code 18), already in every engine's `ParamSnapshot`. No new block code, no new `BlockRef`. |
| Fields | `pub sends: [f32; FX_SENDS]` (0..1), indexed by FX slot, `FxBus` order |
| Ids | `OutParams::SEND1` = 4, `SEND2` = 5, `SEND3` = 6 |
| Frozen idents | `SEND1`, `SEND2`, `SEND3` (fixture `disk_codes_v1.txt`: `B 18 4 SEND1` …, appended) |
| Meaning | amount into FX slot n. Phase A: slot 1 chorus, 2 delay, 3 reverb, fixed. Phase B: slot n holds the project's chosen effect, defaulting to today's; old Sounds mean the same. A fourth slot appends `SEND4` = 7. |
| Neutral | 0 (`Sound::neutral` unchanged in effect; `neutral_v1.txt` gains the three fields at 0). Sparse encoding omits them at 0, so every Sound saved before this decodes with sends 0, and `factory.bin` is byte-identical. |
| Spec | `ParamSpec::continuous(.., 0.0, 1.0, 0.0, 1.0/128.0, false, Smoothing::Own)`, `ValFmt::Uni`, `FocusGlyph::LevelBar`: as today's CHR/DLY/REV. |
| Modulatable | no (ADR 0010: the mixer applies them, the voice doesn't read them) |
| Motion | `Continuous(Own)` (ADR 0112): the mixer's `MixState` ease smooths them, as now. Not `Knobs`: `KNOBS` doesn't grow. A Sound load eases from the old sends to the new ones; never a snap. |

Why OUT and not a new SENDS block: same codec path (`encode_sound` walks
`BlockRef::ALL`), no new variant to thread through every `match`, ~0 flash.
Why slot-indexed idents and not CHR/DLY/REV: Phase B re-points slots, the
Sound's bytes stay.

`PartParams` loses `sends` and specs 5–7; ids 5–7 are retired, never
reused (`RETIRED` gains `(25, 5)`, `(25, 6)`, `(25, 7)`; the fixture keeps
their lines). SNAP and HELD keep ids 8 and 9.

### 2. Audio

`mix_parts` (`instrument.rs`) reads `part.params.out.sends` (the published
Sound) where it read `part.mix.sends`. Same ease, same sums, same order:
output is bit-identical for equal values. A send edit now writes the
Sound, so its `Tracked` stamp moves and the publisher diffs that Part on
the UI thread (ADR 0116); no `KnobMove` is sent (not a knob).

A Sound load lands its knobs (ADR 0107), but its sends ease through
`MixState` from the old Part's values: a deliberate exception (never snap),
recorded in ADR 0123.

### 3. Pages: one value, two views (owner, 2026-10-11)

AMP (`FOLDER`, id 9) is rebuilt; SYM moves to a sub-page under it.

| Page | Row 1 | Row 2 |
|---|---|---|
| AMP (every chain: ALGO, MODAL, TAPE) | CHORUS · DELAY · REVERB (`Out` SEND1–3) | FOLD · VEL · MIX (`Folder::FOLD`, `Out::VCA_VEL`, `Folder::MIX`) |
| AMP › SYM (new sub-page, `ChainBlock::with_subs(&FOLDER, &[&FOLDER_SYM])`, EDIT down; id 98, short `SYM`) | SYM · — · — | — · — · — |
| Mixer SENDS (id 30, home per ADR 0057, unchanged place) | CHORUS · DELAY · REVERB (the same three addresses) | — |

Both pages bind `ParamSlot::param(BlockRef::Out, SENDn)`; `part_block`
already maps `Out` to the Part's Sound, so the mixer's and the Sound's
cells are one value. The SENDS page keeps its `EffectsFlow(Sends)` viz.

Labels: the specs' own, full words, fixed (owner: no three-letter
abbreviation when the word fits). Measured with `draw::text_width`
(`FONT_LABEL`, `LABEL_TRACKING`) against `CELL_BAR_W` = 62 px:

| Label | px | Short |
|---|---|---|
| `CHORUS` | 52 | none needed |
| `DELAY` | 40 | none needed |
| `REVERB` | 48 | none needed |
| `SYM` | 26 | — (`SYMMETRY` is 66: too wide) |

A test pins each ≤ `CELL_BAR_W` (the pattern of
`reverb_panel_test::labels_and_values_fit_their_cells`).

Focus-band hints (ADR 0086), one per cell, static strings, drawable marks
only (`ui/draw.rs`: `· › ● ◦ ◂ ▸ ▴ ▾`; no `→`/`↓`):

| Cell | Hint (`Gesture(keys, text)`) | Source |
|---|---|---|
| REVERB | `("REVERB ▸", "<TYPE>")`, e.g. `QUAD` (NEW's TYPE) or `HALL` | `REV_HINTS: [Hint; 6]`, indexed by `SlotCtx::rev` |
| CHORUS | `("CHORUS ▸", "<MODE>")`: `OFF` `I` `II` `I+II` | `CHORUS_HINTS: [Hint; 4]`, new `SlotCtx::chorus: ChorusMode` |
| DELAY | none | — |
| FOLD, VEL, MIX on AMP | `("EDIT ▾", "SYM")` | `SYM_HINT` |

`View::hint` takes `&SlotCtx` (two callers, `renderer.rs`). The cell's own
hint is the only one shown, so nothing competes. In Phase B the three
labels become the slot names.

### 3a. SYM: what the code does (finding) and the fix

> **Reverted 2026-10-11 by the owner's verdict** (the A/B was inaudible): the fix below was dropped and today's fold law stays, bit-identical to main. The text records the finding and the dropped fix.

SYM is wired end to end: `FolderParams::symmetry` → `Wavefolder::process`
(`dsp/wavefolder.rs`), a `Knobs` glide (`knobs.rs`), ramps and cost in
`voice.rs`. It is not a dead param. It is inaudible over most of FOLD:

- today's wet is `fw((x + b)·g) − fw(b·g)`, `g = 1 + 6·FOLD²`,
  `b = (SYM − 0.5)·0.5`. `fw` is linear on (−1, 1), so until
  `(x + b)·g` leaves it the `rest` term cancels the bias exactly: SYM
  changes nothing, bit for bit;
- the stage is off at FOLD < 0.001 (`stage_runs`): at the default FOLD 0
  SYM does nothing at all;
- the bias scales with `g`, so as FOLD rises `b·g` crosses the fold point
  ±1, where the slope of `fw` flips: a small signal's wet changes sign
  between two FOLD ticks. Today's worst step between adjacent FOLD ticks
  at SYM 1 and peak 0.01 is +2.8 dB re the signal (a jump).

Algebra at the fold point: `fw(1 + d) − fw(1) = −|d|`, a full-wave
rectifier with DC, not a polarity flip; a flip happens only at
`b·g ≡ 2 (mod 4)`.

**The fix: a bias-only law, `g` unchanged.** The bias moves to post-gain
units and ramps in with FOLD:

`wet = fw(x·g + B) − fw(B)`, `B = (SYM − 0.5)·1.8·min(1, FOLD/0.25)`

- |B| ≤ 0.9 < 1: never at the rectifier point, never near the flip at 2.
- `fw` is 1-Lipschitz, so `|wet| ≤ g·|x|`: the wet level is bounded by
  today's linear path; no level rise anywhere; MODAL's tail gets no new
  gain (the `stuck_ok` FOLD exemption, keyed on `g`, holds unchanged; the
  sweep confirms, and a SYM clause is added only with a measured reason).
- B doesn't move with `g`: no sign flip as FOLD turns.
- FOLD 0 (any SYM) and SYM 0.5 (any FOLD) are bit-identical to today
  (stage off; B = 0 is today's expression).

Host measurement (sine, MIX 0.5; residual of SYM 1 vs SYM 0.5 re the
signal; level = max over SYM and FOLD of out vs dry; step = worst change
between adjacent FOLD ticks at SYM 1):

| Peak | Law | FOLD 0.1 | 0.25 | 0.5 | 1.0 | Max level | Worst FOLD step |
|---|---|---|---|---|---|---|---|
| 0.01 | today | −111 | −112 | −114 | +5 | 12.0 dB | **+2.8 dB** |
| 0.01 | new | −109 | −110 | −114 | −121 | 12.0 dB | −26.6 dB |
| 0.1 | today | −130 | −132 | −134 | +5 | 12.0 dB | −14.6 dB |
| 0.1 | new | −128 | **−16** | **−5** | 0 | 12.0 dB | −15.4 dB |
| 0.3 | today | −139 | −141 | −7 | +4 | 10.1 dB | −20.0 dB |
| 0.3 | new | −138 | **−5** | −1 | +7 | 10.2 dB | −20.5 dB |
| 0.7 | today | −53 | −14 | −1 | +2 | 3.3 dB | −24.7 dB |
| 0.7 | new | −24 | −3 | +2 | +1 | 3.5 dB | −26.4 dB |

SYM is heard from FOLD 0.25 at peak 0.1 (today from 1.0), the FOLD sweep
is never steppier than today's (and loses today's jump), the level bound
is today's, and tiny tails are untouched. Rejected: retuning `g` (moves
every FOLD > 0 Sound, raises low-level wet up to +20 dB, moves the MODAL
tail exemption); ramp 0.1 (−14.9 dB step at 0.3, steppier than today);
|B| ≤ 1 (touches the rectifier point: −2.3 dB step).

Sound change: Sounds with FOLD > 0 and SYM ≠ 0.5 sound different
(no-legacy-compat rule). The factory has no FOLD > 0 (scanned), so its
goldens hold. **The owner A/Bs the new SYM by ear before merge.**

Gate (tests): SYM 1 vs 0.5 residual ≥ −20 dB at peak 0.1 and FOLD ≥ 0.25;
max level within 0.5 dB of today's per (peak, FOLD); worst adjacent-tick
step ≤ today's + 0.5 dB at every peak; FOLD 0 and SYM 0.5 bit-identical;
`the_folder_sym_puts_no_dc_on_the_dac` (`param_sweep_test.rs`) holds.

### 4. Returns default audible

The block defaults change, spec and struct together (one truth:
`block_test::assert_defaults` requires `Default` == spec `default`; NEW,
`Performance::new`, `put_new_fx` and a reset all agree):

| Param | Was | Now |
|---|---|---|
| `ChorusParams::mode` / CHORUS MODE spec default | OFF (0) | I (1) |
| `ChorusParams::mix` / MIX | 0 | 1.0 |
| `DelayParams::mix` / MIX | 0 | 1.0 |
| `ReverbParams::mix` / MIX | 0 | 1.0 |

Unity return, as a send/return mixer: the send sets the amount, MIX trims
the return (owner, 2026-10-11). Every other FX default unchanged. A Sound
with sends at 0 sounds as before, so the factory and ADR 0063/0109 levels
don't move. CPU: none billed: `FxBus::COST` is reserved whatever the MIX
and `rev_bill` follows the TYPE (ADR 0061).

Existing projects keep their stored FX: `put_fx` writes FX blocks dense,
so a stored MIX 0 stays 0.

`FxParams::OFF` (a `const`: every MIX 0, chorus OFF, the rest as default)
for tests and bench rows that measure the dry path. The ~110 `*Params::default()`
call sites in tests are audited: those that assume silence take `OFF` (or
set MIX 0); `reverb_test`'s `p.mix == 0.0` pin becomes 1.0.

### 5. Loading old projects and Sounds

No-legacy-compat rule: loads safely, sound may change.

| File | What happens |
|---|---|
| Library Sound / pool slot, pre-change | no SEND ids: sends 0 |
| Project Part, pre-change | see below: the Part's old sends move into its Sound |
| Project, post-change | `Block(PART)` has no 5–7; nothing to migrate |

Mechanism (a `Translation` can't: it sees only the block being decoded,
and the Sound isn't finished until `close`):

1. `ProjectCheck::step`'s PART arm (`codec.rs`, `Context::Part`) scans the
   payload for ids 5–7, as `SoundCheck` scans FILTER for FM3/QM
   (`sound.rs`), and stashes the finite values in
   `ProjectCheck::old_sends: Option<[f32; 3]>` (missing ids 0). It runs in
   both passes, with or without a target.
2. Any of ids 5–7 present sets `self.lossy = true`: every pre-change project
   (PART is written dense) opens not-Saved; one save converts it.
3. `RETIRED` gains `(25, 5..7)`, so `decode_block` skips them.
4. `close`, at `Context::Part`, after `self.sound.finish(staged)`: if the
   staged Sound's `out.sends` are all 0, write `old_sends` (clamped 0..1)
   into them. Then clear `old_sends`. Record order inside the Part doesn't
   matter (a PART before `Engine` still lands, since `engine_record`'s
   reset happens before `close`).
5. `restart` clears `old_sends`.

Consequences:

- A Part with non-zero migrated sends reads Edited: a Slot-origin one
  because its slot's Sound has none; an Init-origin one because it no
  longer `bits_eq` `Sound::init` (`marks.rs`). True in both cases.
- Not migrated into pool slots: each Part stores its own Sound inline, and
  two Parts sharing a slot can hold different sends.
- Anything that replaces a Part's Sound replaces its sends: a pool or
  library load, RELOAD FROM PROJ (a migrated Part's slot has none, so it
  goes dry), INIT, and an engine change (`PartFrom::Init`). That is the
  decision: the sends are the Sound's.
- Old FX MIX values stay (0 in most projects), so an old project's sends
  stay silent until MIX is raised. Accepted: no users.

### 6. Interactions

| Area | Effect |
|---|---|
| ORBIT scenes / rings | none: the runner reads only `mix.channel`; no scene or step holds a param lock. Several rings on one Part share its Sound's sends, as before. |
| MIDI map (#247, not built) | sends become Sound params (`Out` SEND1–3), mappable per Part like any other; a mapped CC dirties the Sound. A comment on #247 records it (CC 91/93, the GM reverb/chorus sends, as a candidate default). |
| Sound library (#459, ADR 0119) | nothing to do: library files are `encode_sound`, so sends ride along. |
| VA (va-engine) | its chain also uses `FOLDER`: AMP's new layout and SYM sub-page apply to VA too. |
| SPACE (ADR 0120) | not reinstated as such: AMP's cells are labelled by the effect they feed and edit the same value as the mixer's SENDS; no engine home page gets one. ADR 0120 superseded in part ("lives on SENDS only"). |
| Edit marks | a send edit marks the Sound edited (it is the Sound's now), not the mix. |
| Delay's REV SEND (`DelayParams::REV_SEND`) | unchanged, project level |
| Phase B (#461) | sends are slot-indexed: a library Sound moved to a project whose slot n holds another effect sends its amount there. No format break; a meaning change, accepted in ADR 0123. |
| KIT (later) | per-key Sounds with their own sends would need per-voice send sums (CPU); out of scope, noted in ADR 0123. |

### 7. Cost

| | |
|---|---|
| Flash | ~0: three specs (+), three specs (−), one sub-page def, two hint tables, SYM's law (same op count) |
| RAM | `ParamSnapshot` +12 B, `PartParams` −12 B: `AudioShared` and `Project` net 0. `Pool::scratch` and `SoundDecoder::staged` +24 B AXI. Voice `played` +12 B, 8 voices +96 B D2; `Voice` is 6,264 B against 2,112 + 4,248 = 6,360 (ADR 0101, chain 2,016 of 2,112): fits; the const asserts check it. |
| Pool | `MAX_SOUND_COST` +16 B over the base (three entries, 15 B, rounded): main 2,288 → 2,304; with VA merged 2,352 → 2,368. `sparse_bound` pins. |
| Records | OUT's record ≤ 36 B, far under 640 (ADR 0102) |
| CPU | ~0: each voice clones 12 B more of `ParamSnapshot` a block (`voice.rs`), negligible |

## Done when

- [ ] `OutParams` holds SEND1–3 (ids 4–6, idents frozen); `PartParams` has no sends; `(25, 5..7)` retired.
- [ ] `mix_parts` and the bench read the Sound's sends.
- [ ] AMP (every chain using `FOLDER`, VA too) is CHORUS · DELAY · REVERB / FOLD · VEL · MIX; AMP › SYM holds SYM; mixer SENDS shows the same three values.
- [ ] Labels CHORUS, DELAY, REVERB fit `CELL_BAR_W`; REVERB's hint names the TYPE, CHORUS's the MODE, FOLD/VEL/MIX hint `EDIT ▾ SYM`.
- [x] SYM's bias-only law reverted by the owner (2026-10-11); today's law stays.
- [ ] NEW: chorus MODE I, every MIX 1.0; a send turned up on a NEW project is heard with no other knob.
- [ ] A pre-change project loads with each Part's sends in its Sound, `lossy`, `ProjectStatus::Modified`; a pre-change Sound loads with sends 0.
- [ ] Saving a Sound to the pool or the library and loading it into another Part brings its sends.
- [ ] ADR 0123 in `docs/adr/` and `README.md`; comment on #247.
- [ ] `just check`, `just test`, `just clippy` green; flash delta in one line.

## Testing

| Test | Asserts |
|---|---|
| `disk_codes_v1.txt`, `neutral_v1.txt` | appended lines only; retired PART lines kept |
| `part_test` | PART has no send spec; `no_part_param_is_modulatable` stands |
| `block_test`, `reverb_test`, `chorus`/`delay` tests | defaults == spec defaults; reverb MIX pin 1.0 |
| `param_declarations_test` | SEND1–3 listed `Own`, "the mixer's ease" |
| sound round trip | sends survive `encode_sound`/decode bit for bit; at 0 they write no bytes |
| new `sends_migration_test` | a pre-change project fixture, **generated in a commit before PART 5–7 are removed** (after that no encoder writes them): each Part's sends land in its Sound; a Sound already holding sends keeps them; PART placed before `Engine` still lands; `ProjectDecoder::lossy()` and `ProjectStatus::Modified` |
| `instrument_test`, `gain_staging_test`, `loudness_test`, `load_protocol_test` | `mix.sends` → `params.out.sends`; bit-identical output for the same values; FX-off cases use `FxParams::OFF` |
| `param_sweep_test` | `Param::performance()` (`tests/common/sweep.rs`) counts `Out` SEND1–3 as wet; the 0.5 wet baseline writes the Sound's sends after the patch |
| `loudness_test` | six Parts, sends up, NEW FX defaults: at or under the ceiling (ADR 0109) |
| `part_page_test`, ADR 0120's test | SENDS cells and AMP's row 1 address `Out` SEND1–3; no engine home page has a send |
| `dpad_nav_test`, `header_map_test` | EDIT on AMP reaches SYM; mixer home unchanged |
| `screen_golden_test` | `mixer_sends`, `mixer_fx_*` (MIX values change) and AMP (every chain) re-blessed; new AMP › SYM golden |
| label width test | CHORUS, DELAY, REVERB, SYM ≤ `CELL_BAR_W` |
| `hint_test` | REVERB's hint per TYPE, CHORUS's per MODE, FOLD/VEL/MIX `EDIT ▾ SYM`; every hint fits and draws |
| `wavefolder` unit + `param_sweep_test` | § 3a gate |
| desktop QA | NEW project, AMP › REVERB up: audible; same value on mixer SENDS; the Sound into another Part: send follows; SYM sweep at FOLD 0.3 audible, FOLD sweep smooth |

## ADR note

ADR **0123** (0118 va-engine, 0119 sound-library, 0122 flash-diet-2 are
taken on branches): "A Sound keeps its effect sends; the returns default
audible". Supersedes in part 0120 (the reverb send on SENDS only) and the
`PartParams` sends of the instrument-core spec; amends 0043 (a Part's
"mix" no longer holds sends) and 0044 (SENDS on the Part rung now edits
the Sound; 0057 is superseded by 0044). Records: OUT SEND1–3 slot-indexed
for #461 and the meaning change that brings; retired PART 5–7 and the
Part-close migration; the FX defaults; AMP's layout with SYM on a
sub-page; SYM's bias-only law; sends easing on a Sound load as a
deliberate exception to ADR 0107's landing; KIT's per-voice cost later.

## Owner's answers (2026-10-11)

1. Placement: AMP row 1 CHORUS · DELAY · REVERB, row 2 FOLD · VEL · MIX;
   SYM to a sub-page under AMP; SYM's fix is in scope (§ 3a).
2. Returns: MIX 1.0 in a NEW project (§ 4).
3. Labels: full words, fixed; the hint names the reverb TYPE (§ 3).

## Review resolutions

Review: (private notes) (approve after revision).

| Finding | Resolution |
|---|---|
| M1 Translation can't reach the Sound | Accepted: PART-payload scan → `old_sends`, applied at the Part's `close` (§ 5); PART-before-Engine test |
| M2 nothing sets `lossy` | Accepted: the scan sets it on any of ids 5–7 (§ 5); test asserts `lossy()` and `Modified` |
| M3 fold algebra, no level/ring bound | Accepted: algebra corrected (`−|d|`); bias-only law measured against a gain retune and chosen; level, continuity, tail bounds and gate (§ 3a); bit-identity reworded (FOLD 0 any SYM; SYM 0.5 any FOLD) |
| m1 hint glyphs, static, chorus mode, which wins | Accepted: `▸ ▾`, static tables, `SlotCtx::chorus`, one hint per cell (§ 3) |
| m2 engine change / INIT / RELOAD, Init-origin Edited | Accepted: listed (§ 5) |
| m3 which defaults change | Accepted: block and spec defaults change together; `FxParams::OFF`; call-site audit (§ 4) |
| m4 sweep wet set, `mixer_fx_*` goldens, fixture before removal | Accepted (Testing) |
| m5 RAM: scratch, staged, voice clone | Accepted (§ 7) |
| m6 va-engine cost and fourth chain | Accepted: "+16 B over the base"; every chain using `FOLDER` (§ 6, § 7) |
| m7 ADR 0057 → 0044; no #247 spec | Accepted: cite 0044; comment on #247 |
| m8 Phase B meaning change; KIT | Accepted: recorded for ADR 0123 (§ 6) |
| m9 ADR 0107 exception | Accepted: § 2 and ADR 0123 |

