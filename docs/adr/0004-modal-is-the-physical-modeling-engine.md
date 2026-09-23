# 0004. Modal is the physical-modeling engine; Rings/Elements are its reference

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
Mutable Instruments Rings and Elements are open-source physical modeling
designs built for STM32F4-class chips, so porting them is translation work,
not research. Chimera already has a working Modal engine (up to 48 modes,
Karplus-Strong with sympathetic strings). CPU, not difficulty, is the limit:
Elements used most of a 168 MHz F4 for one voice.

## Decision
No separate "Resonator" engine. Improve Modal using the MI code as a
reference (and possibly port parts of it). Verify the MI firmware license in
`pichenettes/eurorack` before copying any code, and carry its notice.

## Alternatives considered
- **A full Rings/Elements port as a new engine** — duplicates Modal.
- **Drop physical modeling** — it was wrongly called "a research project".

## Consequences
Modal is the heavy engine: ~66 KB per voice today (8 × `[f32; 2048]` string
buffers). Its buffers must move to a shared pool before polyphony.
Known issues: plays an octave high and rings after note-off
(`docs/issues/003-modal-sanity-gate.md`).

## Sources
- https://github.com/pichenettes/eurorack (Rings, Elements)
- `chimera-core/src/dsp/modal.rs`
