# 0123. A Sound keeps its effect sends; the returns default audible

- **Status:** Proposed
- **Deciders:** owner (2026-10-11), firmware

## Context
Hearing reverb took three knobs in three places: the Part's send (mixer SENDS), the reverb's MIX and its setup. Every effect's MIX defaulted to 0, so a send turned up on a NEW project was silent. The sends lived in `PartParams` (ids 5-7), so a Sound moved to another Part, or loaded from the library (ADR 0119), lost its amounts. ADR 0120 had just removed SPACE, the duplicate reverb-send cell. SYM, on AMP, was inaudible over most of FOLD.

## Decision
- **The sends are the Sound's.** `OutParams::SEND1`, `SEND2`, `SEND3` are ids 4, 5, 6 (idents frozen), `f32` 0..1, neutral 0, sparse (a Sound with sends at 0 writes no bytes, so old Sounds and `factory.bin` are unchanged). They are slot-indexed, not CHR/DLY/REV: slot n is the amount into FX slot n (today 1 chorus, 2 delay, 3 reverb). Not modulatable (ADR 0010).
- **Phase B meaning change (#461).** When FX slots become configurable, a Sound moved to a project whose slot n holds another effect sends its amount there. No format break; the meaning changes, and that is accepted here.
- **`PartParams` loses its sends.** Ids 5-7 of `Block(PART)` are retired, never reused (`RETIRED`); SNAP and HELD keep 8 and 9. This supersedes in part ADR 0120 ("the reverb send lives on SENDS only") and the `PartParams` sends of the instrument-core spec, and amends ADR 0043 (a Part's mix no longer holds sends) and ADR 0044 (SENDS on the Part rung edits the Sound; 0057 is superseded by 0044).
- **Part-close migration.** A pre-change project's Part scans ids 5-7 into `ProjectCheck::old_sends`; at the Part's close, if the staged Sound's sends are all 0, the old values (clamped) move into it. The file opens Modified (`lossy`) only when the migrated sends are not all 0: a file whose old sends are all 0 loses nothing and keeps its old PART bytes until the next save. A pre-change Sound or library file has no SEND ids: sends 0. Migrated sends are not written to pool slots; each Part holds its own Sound inline.
- **Returns at unity.** A NEW project has chorus MODE I and every FX MIX 1.0 (struct `Default` and spec default agree). The send sets the amount, MIX trims the return. Stored projects keep their stored FX. `FxParams::OFF` is the dry preset for tests and benches. A Sound with sends at 0 sounds as before.
- **Ease, an exception to ADR 0107's landing.** The sends are `Continuous(Smoothing::Own)` (ADR 0112): the mixer's `MixState` ease, the same 20 ms one-pole as LEVEL and PAN, reaches 0.99 in about 100 ms. A Sound load lands its knobs (0107) but eases its sends from the old Part's values; never a snap. `KNOBS` does not grow.
- **AMP and SYM.** On every chain using `FOLDER` (ALGO, MODAL, TAPE, VA), AMP is CHORUS · DELAY · REVERB over FOLD · VEL · MIX; SYM sits alone on a sub-page one EDIT down (id 98). The mixer SENDS page binds the same three addresses (`Out` SEND1-3): one value, two views. Hints (ADR 0086): REVERB names its TYPE, CHORUS its MODE, FOLD/VEL/MIX `EDIT ▾ SYM`. Labels are full words and fit `CELL_BAR_W`.
- **SYM's bias-only law.** `wet = fw(x·g + B) - fw(B)`, `B = (SYM - 0.5)·1.8·min(1, FOLD/0.25)`, `g` unchanged. |B| ≤ 0.9, so never at the rectifier point or the flip; `|wet| ≤ g·|x|`, so no level rise beyond the linear bound. FOLD 0 and SYM 0.5 are bit-identical to before. The level gate is per peak (max over SYM and FOLD within 0.5 dB of the old law); the one miss is +0.58 dB at peak 0.3, FOLD 0.77, under the linear bound. Sounds with FOLD > 0 and SYM ≠ 0.5 sound different (no legacy compat); the factory has none. The owner A/Bs it by ear before merge.
- **KIT later.** Per-key Sounds with their own sends would need per-voice send sums: a CPU cost, out of scope here.

## Alternatives considered
- **A new SENDS block:** a new `BlockRef` through every `match` for the same codec path.
- **CHR/DLY/REV idents:** Phase B would re-point slots and orphan the bytes.
- **A `Translation` for the migration:** it sees only the block being decoded; the Sound is not finished until the Part closes.
- **Easing in 50 ms:** the mixer ease is shared with LEVEL and PAN; changing it moves both.
- **Retuning SYM's gain:** moves every FOLD > 0 Sound and raises low-level wet up to +20 dB.

## Consequences
- A send edit marks the Sound edited; anything that replaces a Part's Sound (pool or library load, RELOAD FROM PROJ, INIT, an engine change) replaces its sends.
- Old FX MIX values stay, so an old project's sends stay silent until MIX is raised.
- Flash ~0; `MAX_SOUND_COST` +16 B; `ParamSnapshot` +12 B, `PartParams` -12 B.
- Per-Part MIDI mapping of sends (#247, ADR 0128) needs nothing special: they are Sound params; CC 91/93 are candidate defaults.

## Sources
- `docs/superpowers/specs/2026-10-11-sends-with-the-sound-design.md`; #247, #461; ADR 0010, 0043, 0044, 0086, 0107, 0112, 0119, 0120, 0128.
