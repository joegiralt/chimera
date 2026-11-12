# VARIETY on every engine (0.4): design

Status: owner direction, 2026-11-12. Reference: OP-XY 1.2 drumline ("randomisation will change all parameters on the selected sound after every hit, the amount adjusts to what extent"; a per-track RANDOM on top). Ours is our own design.

## Owner's rulings
- One more parameter on every engine: **VARIETY**, per Sound (ALGO, MODAL, VA, TAPE, DRUM).
- **Never touches pitch on melodic engines.** DRUM machines may vary pitch a little.
- **No FUNK** (per-trigger timing): a sequencer thing, not an engine's.

## Proposal (Claude's, to confirm in the ADR)
- At each note-on / hit, every cell in the engine's curated VARIETY set gets a random offset in ±VARIETY × the cell's own weight, drawn once from the voice's seed and latched for the note (no wobble inside a note). VARIETY 0 is bit-identical to today.
- Curated per engine, musical by construction (as RANDOMIZE): e.g. ALGO MORPH, VOWEL, modulator levels, filter; MODAL STRUCTURE, BRIGHTNESS, POSITION, DAMP; VA WAVE/SHAPE, cutoff, envelope times; TAPE POSITION, SIZE, DRIFT; DRUM each machine's knobs (pitch small).
- DRUM: VARIETY per pad, and a kit-wide VARIETY macro scaling every pad (OP-XY's track RANDOM), with the planned kit macros.
- Cost: per voice, one latched offset per varied cell (tens of bytes), applied through the existing per-voice mod sums; no per-sample CPU.
- Open: the cell VARIETY sits on per engine (pages are full; PIT's F cell is RESERVE's), the weights, MIDI map/NRPN address.

## Next
ADR, then a plan task in 0.4 (after DRUM slice 1; DRUM's per-pad VARIETY joins the pad model).
