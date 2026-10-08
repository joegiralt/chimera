# 0114. Keep the DSP techniques in a curated library, `chimera-dsp`, under the products

- **Status:** Proposed
- **Deciders:** owner (2026-10-08); firmware

## Context
`chimera-engine/src/dsp/` (64 files) is organised by product: `algo/`, `modal/`,
`tape_engine/`, `filter/`, the reverb types, the voice and the FX bus. The
reusable techniques are buried inside them: the TPT stages in `k2040.rs` and
`ladder.rs`, the saturators in `filter/coef.rs`, the waveguide string in
`modal/`, the reverb kernels in the reverb types, the glide in `knobs.rs`.

The owner plans to keep developing this DSP and sell it as plugins (Fors /
Sinevibes model), and to port it to other hosts (the Blackbox, a plugin
shell). The techniques are the asset; the products are compositions of them.

The owner also set the rule for what goes in: "we have to be very choicey about
what DSP blocks we have. It'd be very easy to look at the Faust DSP library and
steal that and build everything from there. I don't know if that's what we
want." A library is valuable for what it leaves out, and for owning cleanly
what it keeps.

## Decision
1. **Two layers.** A new crate, `chimera-dsp` (no_std, opt-level 2 under
   ADR 0108's rules), holds technique blocks organised by kind:
   `oscillators/`, `filters/`, `nonlinear/`, `reverbs/`, `physical/`,
   `spectral/`, `modulation/`, `util/`. A block knows nothing of Chimera's
   parameters, voices, Parts or UI. `chimera-engine` stays the product layer
   (ALGO, MODAL, TAPE, the voice, the FX bus, the parameter model) and is
   composed from `chimera-dsp`. A future plugin is another product layer.
2. **A block enters `chimera-dsp` only when all of these hold:**
   - **A product needs it now.** No speculative modules, no empty folders:
     the tree grows as techniques are written.
   - **Its provenance is clean and recorded** in a header at the top of the
     file: owner-original, or implemented from a paper or the mathematics
     (cited), or derived from permissive code (MIT, BSD, STK-style) with its
     notice kept. Never derived from GPL or LGPL code. Reference libraries
     (Faust, STK, Mutable Instruments, JUCE, others) are for reading and for
     A/B comparison, not for copying.
   - **It meets the bar:** checked against a reference (analytic, SPICE,
     hardware recording, or the original algorithm) through chimera-utils;
     real-time safe (no allocation, bounded state, NaN-safe); every control
     declares its `Motion` (ADR 0112); tested; its cost measured on the unit.
3. **Two tiers, and every block is labelled with its tier** (owner, 2026-10-08):
   - **Commodity:** standard, well-documented techniques everyone uses (BLEP
     oscillators, FM, formant, TPT filters, envelopes, LFOs, delay lines,
     halfband, DC blockers). Implemented from the textbook or paper, or from
     permissive code with its notice kept. Plumbing: correct, fast, clean,
     not distinctive.
   - **Signature:** modelled from something the owner has played and
     measured: real hardware on the bench (a 3340-style VCO, the Kobol, the
     SE-02), driven and recorded through the capture rig and iterated by ear
     until it matches, or a sound the owner has tuned by parameters and
     voltages. Its provenance is the owner's own measurement session (rig
     settings, recordings kept outside the repo, fit results), recorded in the
     file. Never another company's model, emulation or decompiled firmware.
   - **The label is a type, not a comment** (CLAUDE.md: type-driven first):
     every block declares its tier and provenance in code, e.g. a required
     `const TIER: Tier` (`Commodity { source }` / `Signature { session }`) on
     the block trait, and a test lists every block so an unlabelled one fails
     to build or test. Product names follow the tier: commodity blocks are
     described by technique ("BLEP saw"); signature blocks get our own names,
     never a third party's trademark ("3340-style", not "CEM").
4. **The capture rig** (chimera-utils) is the tool that makes signature blocks:
   drive (DC-coupled CV for pitch/PWM/sync/FM, MIDI, gates), capture at 96 or
   192 kHz with fixed gain, analyse (waveform per output, harmonics vs level,
   pitch tracking across CV, drift and warm-up, sync, PWM linearity, FM
   response), fit, and render A/B pages for the owner's ear.
5. **Extract gradually and bit-identically.** First move the blocks already
   shared or clearly generic (TPT one-pole/SVF stages, saturators, halfband,
   DC blocker, limiter, envelopes, LFOs, the `Motion` glide, the waveguide
   delay line). After that, new technique code lands in `chimera-dsp`, and
   product code moves out when it is next touched. Every move keeps the audio
   goldens bit-identical and the audio-ISR guard green. Today's blocks are
   labelled as they move: e.g. the 2040 (REV2/EXP from the SSM2040/Kobol
   schematics) and LADDER (checked against SPICE) are signature-class
   circuit models; the TPT stages, saturators and envelopes are commodity.

## Alternatives considered
- **Leave the DSP organised by product.** Simplest today, but the techniques
  stay entangled with Chimera's voice and parameter model, which blocks reuse
  in plugins and on the Blackbox.
- **Build on an existing library (Faust's libraries, STK, Mutable's stmlib,
  JUCE's DSP module).** Fast breadth, but mixed and often copyleft licences
  (Faust's libraries carry per-function licences, some LGPL), generic quality,
  and nothing distinctive. The owner rejected bulk import.
- **One big extraction now.** Touches every DSP file at once while several
  branches are in flight; gradual extraction gets the same result without
  conflicts.

## Consequences
- The company's core asset becomes one crate with a clear ownership record,
  ready to sit under a plugin shell (CLAP/VST3) or another device.
- Every block carries a provenance header and a reference check, which costs a
  little per block and pays off in any licensing or due-diligence review.
- Some breadth is deliberately missing: a technique arrives only with a product
  that needs it.
- Performance is unchanged: `chimera-dsp` builds at opt-level 2 like
  `chimera-engine`, and the stack-check guard (ADR 0108) covers it.

## Sources
- Owner's direction, 2026-10-08 (DSP company plan; the curation rule above).
- ADR 0108 (crate split by optimisation profile), ADR 0112 (parameter `Motion`).
- ADR 0032 (MODAL provenance: Mutable's MIT notice), ADR 0098/0100
  (chimera-utils A/B tool).
