# 0128. Pitch bend and sustain are per Part; every other CC is the Part's MIDI map

- **Status:** Accepted (2026-10-11)
- **Deciders:** owner (#437, 2026-09-30), firmware

## Context
The MIDI input dropped pitch bend and every CC (#437). Two controls a keyboard player expects had to land for alpha, and the owner ruled that no other CC ever reaches a parameter through a global chart: CC control is per Part (#247).

## Decision
- **Only pitch bend and CC 64 are routed in.** The parser's other CCs (mod wheel, volume, pan, anything) stay dropped at `InEvent::from_midi` until the per-Part MIDI map (#247) takes them. There is no global CC chart, here or later.
- **BEND is a Part (mix) parameter,** `PartParams::BEND`, whole semitones 0..=24, default 2, saved with the Part's `Block(Part)` record. It is performance routing like CH and MODE, not part of the Sound, so loading a Sound never changes a player's wheel. A file written before it reads the default.
- **The wheel is per MIDI channel; the range is per Part.** Every Part on the channel bends by its own BEND. The reach is a `Knobs` field, glided as a KNOB (30 ms, ADR 0112) while the Part sounds and landed while it is silent. It is added to the voice's copy of PITCH after the routes, so every engine follows through the one pitch path (ADR 0042), past PITCH's range.
- **Sustain is per channel.** CC 64 at 64 or above holds the voices whose last key goes up, until the pedal is up. A key struck on a held voice takes it back; a steal takes it as any held voice. ORBIT's ring notes are not the player's keys: the pedal never holds them. MODAL, whose note-off never damps (rings like Rings), only keeps its slot held; TAPE's ONESHOT likewise.
- **The queue carries them.** `InEvent::Bend` and `InEvent::Sustain` share the note queue's order with the notes. A pedal up has the note-off reserve: dropped, it would hold notes.

## Alternatives considered
- **A global CC chart:** ruled out by the owner.
- **BEND on the Sound:** a Sound load would move a player's wheel range.
- **A bend a step per message:** a zipper; the wheel sends hundreds a second and CLAUDE.md says never snap.

## Consequences
- Any wheel may move any Modal voice, which retunes its strings each block (ADR 0042), so every Modal voice bills `ModalEngine::PITCH` (30 cycles/sample, +23 `HALO_PITCH` on SYMP) whether or not a route or a glide steal does: the INIT Modal voice goes 446 → 476, and the allocator fits one fewer voice on some Modal Sounds. A Part with BEND 0 still pays; the cost is worst-case, not per Part.
- The MIDI map (#247) adds CCs without touching this path.

## Sources
- #437, #247; ADR 0042, 0112.
