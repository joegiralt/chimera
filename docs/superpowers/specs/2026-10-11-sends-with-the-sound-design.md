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
output is bit-identical for equal values. `PartAudio` loses 12 B in `mix`
and gains 12 B in `params`. A send edit now writes the Sound, so its
`Tracked` stamp moves and the publisher diffs that Part on the UI thread
(ADR 0116); no `KnobMove` is sent (not a knob). No audio-thread cost.

### 3. Pages: one value, two views

| Where | Page | Cells |
|---|---|---|
| Mixer (Part rung) | SENDS (`block_registry::SENDS`, id 30, home per ADR 0057) | A B C = SEND1 SEND2 SEND3; D E F empty |
| Sound (every chain: ALGO, MODAL, TAPE) | AMP › SENDS: AMP (`FOLDER`) becomes `ChainBlock::with_subs(&FOLDER, &AMP_SUB_PAGES)`, `AMP_SUB_PAGES = [&SENDS]`; EDIT goes down to it | the same `BlockDef` |

The same def in both chains (as FILTER is shared): same cells, same
`EffectsFlow(Sends)` viz, zero new page code. AMP's own page is full in
the places that matter (FOLD SYM MIX / VEL, two cells free), so three
sends don't fit on it without splitting them; the sub-page keeps them
together (open question 1).

Cells bind through a new `SlotBinding::Send(k)` (`ParamSlot::send(k)`),
resolved in `ui::view` to `ParamAddr(Out, SEND1 + k)` with a label from
`SlotCtx`:

| Slot | Label | Short (ADR 0053) |
|---|---|---|
| 1 | `CHORUS` | `CHR` |
| 2 | `DELAY` | `DLY` |
| 3 | the reverb TYPE's name: `QUAD` `ROOM` `HALL` `BLOOM` `GATE` `REVERSE` (`SlotCtx::rev`, already read) | `REVERSE` → `RVRS` |

Phase B swaps the table for "the effect slot n holds". The SENDS page
reaches the Sound through `PartEdit`/`PartRead` (`part_block` already maps
`Out` to the Sound), so the mixer's and the Sound's views are one value.

Hint (ADR 0086 / hidden-gestures rule): on AMP, the focus band's hint row
names the sub-page ("EDIT ↓ SENDS"), as other sub-pages do.

### 4. Returns default audible

`FxParams::default()` (NEW, `Performance::new`, `put_new_fx`):

| Effect | Was | Now |
|---|---|---|
| Chorus MODE | OFF | I |
| Chorus MIX | 0 | 1.0 |
| Delay MIX | 0 | 1.0 |
| Reverb MIX | 0 | 1.0 |

Unity return, as a send/return mixer: the send sets the amount, MIX trims
the return (open question 2). Every other FX default unchanged. A Sound
with sends at 0 sounds as before, so the factory and ADR 0063/0109 levels
don't move. CPU: none billed. `FxBus::COST` is reserved whether or not an
effect is on, and the effects run at MIX 0 anyway (ADR 0061).

Existing projects keep their stored FX: `put_fx` writes FX blocks dense
(`encode_block(.., None, ..)`), so a stored MIX 0 stays 0.

`FxParams::OFF` (a `const`, the old default) for the tests and bench rows
that measure the dry path.

### 5. Loading old projects and Sounds

No-legacy-compat rule: loads safely, sound may change.

| File | What happens |
|---|---|
| Library Sound / pool slot, pre-change | no SEND ids: sends 0 |
| Project Part, pre-change | its Sound decodes (sends 0), then `Block(PART)` carries retired ids 5–7. A `Translation` for code 25 hands them to the Part-context target, which writes them into that Part's Sound's `SEND1..3` **if all three are 0** (the Sound has none). Sends move with the Part's Sound. |
| Project, post-change | `Block(PART)` has no 5–7; nothing to migrate |
| Retired ids outside a Part (none exist) | dropped, as any retired id |

Consequences of the migration:

- The load is `lossy` (`ProjectCheck::lossy`): a save writes other bytes,
  so the project opens not-Saved; one save converts it.
- A Part whose sends were non-zero now differs from its origin slot (the
  slot's Sound has none), so its Sound reads edited (`Origin::Slot` crc).
  True: the pool copy has no sends. Saving the Sound over its slot clears
  it. Not migrated into slots: two Parts sharing a slot can hold different
  sends.
- A pool or library Sound loaded into a Part now brings its sends (0 for
  every old one): a Part that had sends goes dry on a Sound load. That is
  the decision.
- The old FX MIX values stay (0 in most projects), so an old project's
  sends stay silent until MIX is raised. Accepted: no users.

### 6. Interactions

| Area | Effect |
|---|---|
| ORBIT scenes / rings | none: rings target Parts, no scene or step holds a param lock. Several rings on one Part share its Sound's sends, as before. |
| MIDI map (#247, not built) | sends become Sound params (`Out` SEND1–3), mappable per Part like any other; a mapped CC dirties the Sound. #247's spec notes it (CC 91/93 are the GM reverb/chorus sends: a candidate default). |
| Sound library (ADR 0119, unmerged) | nothing to do: library files are `encode_sound`, so sends ride along. Merge order free; whichever lands second rebases the fixtures. |
| SPACE (ADR 0120) | not reinstated: the AMP view is labelled by the effect it feeds, on a Sound page beside the mixer's, not a cell on the engine's home page. ADR 0120 is superseded in part ("lives on SENDS only"). |
| Edit marks | a send edit marks the Sound edited (it is the Sound's now), not the mix. |
| Delay's REV SEND (`DelayParams::REV_SEND`) | unchanged, project level |

### 7. Cost

| | |
|---|---|
| Flash | ~0: three specs (+), three specs (−), one binding variant; the def is shared |
| RAM | `ParamSnapshot` +12 B, `PartParams` −12 B: `AudioShared` and `Project` net 0. Voice `played` +12 B, 8 voices +96 B D2; `Voice` today 6,264 B against 2,112 + 4,248 = 6,360 (ADR 0101), 96 B left, so it fits; the const asserts check it. |
| Pool | `MAX_SOUND_COST` 2,288 → 2,304 (three entries, 15 B, rounded to 16; `sparse_bound` pins) |
| Records | OUT's record ≤ 36 B, far under 640 (ADR 0102) |
| CPU | 0 |

## Done when

- [ ] `OutParams` holds SEND1–3 (ids 4–6, idents frozen); `PartParams` has no sends; `(25, 5..7)` retired.
- [ ] `mix_parts` and the bench read the Sound's sends.
- [ ] Mixer SENDS and AMP › SENDS show one value; labels CHORUS, DELAY, the reverb TYPE.
- [ ] NEW: chorus MODE I, every MIX 1.0; a send turned up on a NEW project is heard with no other knob.
- [ ] A pre-change project loads with each Part's sends in its Sound, not-Saved; a pre-change Sound loads with sends 0.
- [ ] Saving a Sound to the pool or the library and loading it into another Part brings its sends.
- [ ] ADR 0123 in `docs/adr/` and `README.md`.
- [ ] `just check`, `just test`, `just clippy` green; flash delta in one line.

## Testing

| Test | Asserts |
|---|---|
| `disk_codes_v1.txt`, `neutral_v1.txt` | appended lines only; retired PART lines kept |
| `part_test` | PART has no send spec; `no_part_param_is_modulatable` stands |
| `param_declarations_test` | SEND1–3 listed `Own`, "the mixer's ease" |
| sound round trip | sends survive `encode_sound`/decode bit for bit; at 0 they write no bytes |
| new `sends_migration_test` | v-old project bytes (fixture written by today's encoder) → each Part's sends in its Sound; a Sound already with sends keeps them; load is lossy |
| `instrument_test`, `gain_staging_test`, `loudness_test`, `param_sweep_test`, `load_protocol_test` | move `mix.sends` to `params.out.sends`; bit-identical output for the same values; FX-off cases use `FxParams::OFF` |
| `loudness_test` | six Parts, sends up, NEW FX defaults: at or under the ceiling (ADR 0109) |
| `part_page_test`, ADR 0120's test | SENDS cells and AMP › SENDS cells address `Out` SEND1–3; MODAL/TAPE home pages still have no send |
| `dpad_nav_test`, `header_map_test` | EDIT on AMP reaches SENDS; mixer home unchanged |
| `screen_golden_test` | `mixer_sends` and a new `amp_sends` golden, labels CHORUS/DELAY/HALL |
| desktop QA | NEW project, turn AMP › SENDS › reverb up: audible; same value on mixer SENDS; load the Sound into another Part: send follows |

## ADR note

ADR **0123** (0118 va-engine, 0119 sound-library, 0122 flash-diet-2 are
taken on branches): "A Sound keeps its effect sends; the returns default
audible". Supersedes in part 0120 (the reverb send on SENDS only) and the
`PartParams` sends of the instrument-core spec / ADR 0043's "mix" contents;
amends 0057 only in that SENDS now edits the Sound. Records: OUT SEND1–3
slot-indexed for #461, retired PART 5–7, the Part-context migration, the
FX defaults.

## Open questions for the owner

1. **Placement.** AMP › SENDS sub-page (same page as the mixer's), or the
   two free AMP cells for DELAY and REVERB only with chorus on the mixer?
   Recommend the sub-page: all three together, one page shared.
2. **Return default.** Unity (1.0), or 75 %? Recommend unity: the send is
   the amount, and the limiter (ADR 0109) holds the ceiling.
3. **Reverb label.** The TYPE's name (HALL), or REVERB? Recommend the
   TYPE: it says what the send is heard as, and it is Phase B's rule.
