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
3. **Extract gradually and bit-identically.** First move the blocks already
   shared or clearly generic (TPT one-pole/SVF stages, saturators, halfband,
   DC blocker, limiter, envelopes, LFOs, the `Motion` glide, the waveguide
   delay line). After that, new technique code lands in `chimera-dsp`, and
   product code moves out when it is next touched. Every move keeps the audio
   goldens bit-identical and the audio-ISR guard green.

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
