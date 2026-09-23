# 0002. Monomachine-inspired engines are our own implementations

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
The Elektron Monomachine's machine types (SWAVE, SID-6581, VO-6, DPRO
wavetables, GND, FM+) are a good menu of cheap, characterful engines for the
STM32H750. gearmulator-md-mm emulates the real Monomachine by running
Elektron's copyrighted ROMs on an emulated DSP56300; it contains no readable
engine source. The Monomachine's character comes largely from implementation
quirks (24-bit fixed point, aliasing, tuning of ranges), so a faithful clone
means months of A/B work with no source to read.

## Decision
Implement each machine *type* from textbook techniques. No Elektron code,
ROM data or wavetables (DigiPRO waves included); wavetables are generated in
code. gearmulator is a listening reference only. The SID-style engine is
written from scratch (reSID is GPL).

## Alternatives considered
- **Faithful clones** — reverse-engineering cost far exceeds the value.
- **Running the emulator on the device** — needs a JIT and a desktop-class
  CPU; also needs the ROMs, so it could never be distributed.

## Consequences
~80% of the sound for ~10% of the effort; freely distributable firmware.
Planned engine list and order: see `docs/superpowers/specs/2026-09-23-engine-pivot-design.md`
(sub-project 4; partly superseded by 0003 and 0004).

## Sources
- https://github.com/joelanders/gearmulator-md-mm (fork of dsp56300/gearmulator)
- Monomachine manual (machine descriptions)
