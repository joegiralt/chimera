# 0035. Every connection from a modulator to the sound is a matrix route

- **Status:** Proposed
- **Deciders:** project owner (2026-09-27, the filter-routing spec; 2026-09-28, priming the hidden destinations)

## Context
The FLT page showed ENV, KEY and FM knobs the DSP never read (#121, #112), the amp envelope ran as a mod source that nothing put on the VCA (ADR 0022), and routing lived in two places: a few fixed knobs and the matrix. The filter models (#123–#127) each want a different panel over the same connections.

## Decision
- Every voice runs six modulators, always: ENV 1–3 (Envelope A or B) and LFO 1–3 (CLASSIC or FUNC). A slot drives only what the matrix routes from it.
- The matrix has eight sources in this stored order: ENV1, LFO1, ENV2, ENV3, LFO2, LFO3, VEL, NOTE; indices 0 and 1 keep their old meaning. NOTE is (note − 60) / 120.
- A route exists apart from its amount (a presence bit per cell): a route at 0 is not a deleted route. MIX+MINUS on a matrix cell deletes a route; the audio thread's sums ignore the bits.
- CUTOFF's routes sum in octaves: `fc = clamp(base · 2^(10·Σ), 20 Hz, min(20 kHz, 0.49·fs))`; Σ = 0 leaves `base` bit for bit. This supersedes ADR 0010's linear law for CUTOFF only. The filter ramps `g` across a block.
- The VCA is a hidden destination on the Out block: the sum of its routes, clamped 0..1 and applied per sample, times AMP's VEL term. It is a sum, not a product. The gain ramps per sample, a LEVEL change on a routed ENV slot included, so no block edge steps it.
- With no VCA route the engine decides, by an exhaustive match on `EngineType` with no wildcard: Algo and Modal pass through (today's expression, bit for bit). VA's arm (a gate) and its default route ENV 2 → VCA at 100 % are #148, so adding VA does not compile until they are decided.
- With routes, a routed source holds the voice: an ENV slot per its TYPE and FORM, any other source while the key is held. At the end of the first block in which none holds it, the voice ends: at once, back to fresh, if its last gain is 0, else through ADR 0027's 128-sample fade. An inactive engine always ends it. This supersedes, in part, ADR 0022's note that no engine puts the amp envelope on the VCA.
- ENV destinations reach their slot a block late. On a note's first block they are summed again with that note's VEL and NOTE (and the last block's ENV and LFO values), so VEL or NOTE → an envelope holds from sample 1.
- Every new Sound carries ENV 1, LFO 1 and NOTE → CUTOFF at 0 (NOTE at the kind's key default).
- The filter page's ENV, LFO and KEY knobs are views of those routes: an absent route shows a dash; turning it creates it (and the CUTOFF column, or reports MATRIX FULL). KIND never edits the matrix.
- The hidden destinations, which have no page, are primed from the cells that own them (owner's decision): on an A page, MIX+PLUS on A, D, R or H primes that slot's TIME, and on S its LEVEL. AMP's VEL primes the VCA, even while VEL is dimmed: the one exception to ADR 0037's dimming rule, since there is no other way in.
- `Voice::cost` adds `ModRouting::cost`, and bills the folder (43) and drive (65) stages once stored at 0.001 or more or routed. The model must never undercount. Each term was measured on the bench-t13c run (rev V, 480 MHz), rounded up and never below 1; the derivations are in the plan's `## Measured`:
  - every voice: BASE 47; FUNC 5 per LFO slot of type FUNC, routed or not;
  - SLIDE 33 per ENV slot with a moving route into TIME, RISE, FALL or SHAPE;
  - DEST_FIRST 12 for the first destination other than the VCA with a nonzero route, then DEST 1 for each further one;
  - with any VCA route, CLAMP 24, then per routed source: ENV_A 28; ENV_B 98 (the costliest of B VCA, B LFO and B GLIDE, so it carries the glide), plus CURVE 1 for ENV mode with SHAPE off centre or routed, or BURST 76 for BURST mode (over the steady B); LEVEL 10 for a moving route into that slot's LEVEL; OTHER 8 for a non-ENV source.
  - DRIVE 65 and DEST_FIRST 12 are billed high until the bench's DRIVE LO and 1 CUTOFF rows are read. `the_model_bills_every_routing_row_high` checks each ROUTING row is billed at or above its reading.

## Alternatives considered
- **Fixed ENV/LFO SOURCE selectors on the filter** (the spec's first version): two sources of truth for routing.
- **A VCA product with depth** (`1 − a + a·s`): an idle envelope leaves a floor of `1 − a`, so the voice would end on a step, and VCA would be the one destination that multiplies.
- **An amp envelope wired to the VCA:** the SH-101 one-envelope feel would need a special case; here it is two matrix edits.
- **Folding the MODS row's unexplained ~38 cycles into BASE:** every voice would pay it; it is billed as the conditional terms above.
- **Dropping VEL for a note's first block** instead of re-summing: VEL → envelope would miss the attack.

## Consequences
- The matrix is at its source cap (8); a ninth source raises `MAX_MOD_SOURCES` to 16 and `present` to `u16`.
- Tremolo on top of an envelope clamps at 1 and can leak after release until the voice ends; a multiplicative "VCA MOD" destination is a later ADR if wanted.
- An LFO- or VEL-gated note is cut about 2.7 ms after key-up (the fade): the no-drone guarantee's price. A lone negative VCA route holds a silent voice until its ENV idles.
- The i8 amount on CUTOFF is 10/127 of an octave per step.
- The re-sum costs 32 B per voice and one pass per note-on; an LFO → ENV destination still lags a block on a fresh note.
- A route into VCA raises a Sound's cost and can shed held notes (ADR 0026, #31). The pool is eight voices (ADR 0040): on rev V the TX and single-oscillator factory Sounds get eight, the MORPH Sounds and A16 ∪ A17 (889) six; A16 ∪ A17 with FOLD and DRIVE, not a factory shape, gets five.
- The pool pushed the per-voice code from 13.4 KB to 18.3 KB, past the 16 KB I-cache. Trimming its hot spots brought 1 OP from 552 to 481 cycles; running the voice from ITCM is #150 and needs its own ADR.

## Sources
`docs/superpowers/specs/2026-09-27-filter-routing-design.md` § 2–5; plan `docs/superpowers/plans/2026-09-28-filter-routing.md` (`## Measured`); #120, #121, #140, #148, #150.
