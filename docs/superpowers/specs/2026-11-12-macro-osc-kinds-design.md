# Macro oscillator KINDs for VA (Plaits-inspired): design note

Status: owner direction 2026-11-12, host prototypes only. Not scheduled into 0.4 until the DRUM slices land and the real bank-1 figure is known.

- VA's oscillator slots gain macro KINDs beside DIGI / EXPNDR / SEM VCO, after Mutable Instruments Plaits' macro oscillator. With a macro KIND, the slot's WAVE/SHAPE (and the KIND's own cell) become the model's HARMONICS / TIMBRE / MORPH.
- Owner picked three (2026-11-12, "could be interesting"): **HARMONIC** (additive), **SWARM** (8-voice detuned cloud), **PARTICLE** (random impulses through a resonant filter). VA already has 2-op FM (MODE FM / LOOP FM), waveshaping (the sine fold under SHAPE) and formants (ALGO's VOWEL), so Plaits' versions of those add nothing; wavetable and speech are too big for flash.
- Provenance (Claude's default, owner may override): our own implementations from Plaits' published structure and behaviour, as DRUM did with the Machinedrum. Plaits is MIT, so it may be read and A/B'd by ear; no code or tables copied.
- Host-first: prototypes in chimera-utils, rendered at HARMONICS/TIMBRE/MORPH settings on short musical phrases (ab-listening-stimuli), auditioned by the owner, then typed ports.
