# 0171. Prepare samples in PREP, a mode of its own, and keep each prep in a sealed `.PRP` beside its `.SRC`

- **Status:** Proposed
- **Deciders:** owner (decisions 2, 3 and 8, 2026-10-15), firmware

## Context
TAPE becomes a sampler with a preparation stage (#476). Every serious sampler keeps the audio untouched and saves the preparation beside it: the Octatrack's `.ot`, the MPC's program, the Blackbox's preset. Rewriting the audio is a separate, explicit act. The owner ruled that PREP is its own mode with its own UI language (as SETTINGS has, ADR 0066), that a `.PRP` beside the `.SRC` holds the prep, and that sound-changing tools render new files and never overwrite. Sources are sealed, write-once and named by CRC (ADR 0087, 0093, 0106). The FAT layer has no rename (ADR 0048). Mutable files are A/B pairs (ADR 0045). The owner power-pulls the unit.

## Decision
- **Prep types:** RAW (trim, loop, root), KIT (up to 64 slices with key policy), ZONES (key ranges and roots over a zone set's layout), REEL (up to 32 splices, 8 programs).
- **The `.PRP`:** `/CHIMERA/TAPES/<CRC>.PRP`, `FileKind::Prep` = 7.
  - Records: `PrepHead` (the source's `SourceRef`, the type, a name), then the type's records (`PrepRaw`, `PrepSlices` ×2 for 64 slices under the 640 B record bound, `PrepZones`, `PrepReel`), and a non-critical `PrepView` (the overview).
  - **Sealed, write-once, named by a CRC** over the ref, the type and its records. The name and the view are labels, outside the CRC. It is written by `write_source`'s rules: write, read back, verify, seal.
  - A source may have several preps. An edit writes a new `.PRP`, then deletes the one it replaces (SAVE) or keeps it (NEW).
- **The Sound copies the prep** when it is picked, as it copies ROOT and PERIOD (ADR 0093). The audio never reads a `.PRP`. KIT and REEL tables reach the audio as **prep rows** in the pool's `SourceTable` (gate, `sounding`, meter: ADR 0087).
- **Audio changes render a new `.SRC`:** fades, gain, normalise, baked reverse, crush, a STRETCH bake, RESAMPLE. They render twice (once for the CRC, once to write), with TPDF dither on the device. Nothing is overwritten. WAV decode and sample-rate conversion stay on the host.
- **PREP is a mode** reached by MIX + EDIT on a TAPE Part's SOURCE or PREP cell, from SETTINGS › SAMPLES (EDIT), and after RESAMPLE.
  - Its chain is TRIM · CUT · LOOP · MAP · REEL · MAKE (MINUS/PLUS along, skipping pages the type doesn't use but never CUT, where the type is chosen: KIT or REEL).
  - Its language is a workbench: a 216 × 120 waveform with numbered amber marks (`WARN`, used for marks only in PREP), a leader strip of the chain, a readout line that also carries MIX hints, six compact dials, and a **tool bar labelling B1–B6**, which no other mode labels.
  - B6 auditions (PLAY/STOP, as everywhere); MIX + PLUS saves; MENU leaves, asking KEEP EDITING / DISCARD.
  - **Precision** (owner, 2026-10-15, from the UX review): one view rule (the view centres on the last-touched mark; a detent moves one pixel at every zoom; ZOOM eases down to 1 sample a pixel; MIX + ZOOM scrolls). EDIT opens **FOCUS** on any mark (FINE to the sample, directional zero-crossing jumps, play from and up to the mark, height zoom, the loop seam); SEQ returns. A persistent head with HERE tools; a played key selects its slice. Positions read to the sample; LOOP and GRID take BEATS at a TEMPO. SNAP goes to the next crossing in the direction of the turn, so it can't trap a mark.
  - **Undo:** 8 levels of the prep's marks (about 11 KB in AXI), MIX + SEQ undo and MIX + EDIT redo, a hint after each edit. PREP has no MIDI learn, so MIX + SEQ is free there. Renders aren't undone: they never overwrite, and an unwanted one is deleted in SAMPLES. Undo replaces the hand-edit prompt.
  - **Nothing depends on pushing an encoder** (ADR 0140: optional hardware); a push may only be a shortcut.
- **SETTINGS › SAMPLES** is the library in SETTINGS' list language: sources and their preps by category, DELETE (refused while a saved Sound uses the source), RESAMPLE, CARD TEST, CLEAN UP (#398, plus: a `.PRP` whose source is gone is deleted).
- **A card write in PREP fades streamed notes out and back in** (ADR 0172).

## Alternatives considered
- **A `.PRP` named after its source, rewritten in place.** One file per source can't hold a KIT and a REEL of the same audio without a merge, and an in-place rewrite needs an A/B pair (two files a source) on a card that is power-pulled. Content naming reuses the sealing code `put` has and matches the `.SRC` beside it.
- **Prep only inside Sounds, no library file.** A prep couldn't be reused across Sounds and projects, and PREP would edit a Part, not a sample.
- **A fine page per stage, or push + turn for fine moves.** Six pages become twelve, each re-implementing zoom and nudge; push encoders are optional hardware, and a tenth step can't reach the sample on a long file. One FOCUS sub-view on the D-pad's idle axis serves every mark.
- **PREP as a page in the TAPE Part's chain.** The owner chose a mode with its own language: the waveform needs the screen, and B1–B6 are Part buttons everywhere else.
- **Destructive edits, as the MPC's process commands.** The owner ruled: render new files, never overwrite.

## Consequences
- `.PRP` files are under 2 KB; a KIT or REEL with only marks saves instantly.
- A project plays the same on any card, `.PRP` or not.
- The TAPES directory holds two kinds; SAMPLES, `put`'s path class and CLEAN UP learn `.PRP`.
- PREP's UI is about 35–45 KB of flash, so it lives in bank 2. UI code in bank 2 needs its own ADR, owed by the PREP slice: ADR 0137's door holds DSP today.
- On-device renders amend ADR 0093's "dither is host code" for those renders only.

## Sources
- `docs/superpowers/specs/2026-10-15-tape-sampler-design.md` §§ 3, 4.
- (private notes) §§ 1, 2.2, 4.
- Octatrack manual OS 1.25 (AUDIO EDITOR, the `.ot` file); Akai MPC (Sample Edit, Chop, Convert to program); 1010music Blackbox (preset XML beside WAVs). Behaviour only (ADR 0114).
- (private notes) (the adversarial UX review); `mockups/index.html` (Proposed: precision) and `walkthrough.html#pro`.
- ADR 0044, 0045, 0048, 0066, 0081, 0086, 0087, 0093, 0102, 0106, 0137, 0140, 0143; issue #398.
