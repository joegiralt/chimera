# 0032. Modal's string is the owner's own code; Rings-derived parts keep Mutable's MIT notice

- **Status:** Accepted (2026-09-27)
- **Deciders:** project owner

## Context
The 2026-09-27 audit (#56) found `dsp/modal.rs` saying its Karplus-Strong string was "ported from Ambika custom firmware". Mutable Instruments' Ambika firmware is GPLv3, while Chimera is MIT, so a port of it would not be allowed here.

The same file's SVF bandpass, fast tangent, cosine oscillator and bow table are described as "matching Rings/stmlib". Mutable's Eurorack code, Rings and stmlib included, is MIT, which requires its copyright and permission notice to travel with substantial portions.

## Decision
- **The KS+ string (STRING mode) is the owner's own code.** It comes from `voicecard/karplus.h` in the owner's Carcosa firmware for the Ambika.
  - Every commit to that file is the owner's (2026-04-18 to 2026-04-20).
  - It copies nothing from Emilie Gillet's Ambika code. The original Ambika had no Karplus-Strong engine.
  - The file's GPLv3 header came from the surrounding project, not from any third-party code.
  - As its sole author, the owner relicenses it to Chimera under MIT.
- **Rings- and stmlib-derived parts keep Mutable's MIT notice.** `modal.rs` carries the Mutable Instruments copyright and MIT permission notice, and says which parts it covers.
- **No GPL code is in Chimera.** Code ported from any GPL source needs its own ADR before it lands.

## Alternatives considered
- **Rewriting the string from scratch.** Unnecessary: the owner holds the copyright.
- **Relicensing Chimera as GPLv3.** Unnecessary, and it would constrain future work.

## Consequences
- Closes #56.
- Modal 2, which is built on Rings and Elements, needs only the same MIT notice on whatever it ports.
- #94 (the tracked stock PreenFM3 bootloader binary) is a separate question and is untouched here.

## Sources
- `~/dev/ambika/voicecard/karplus.h`: the owner's Carcosa firmware; git log shows sole authorship.
- Mutable Instruments Eurorack, Rings and stmlib: MIT, Copyright 2014–2015 Emilie Gillet.
- Issue #56.
