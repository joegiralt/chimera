# 0114. Keep the DSP techniques in a curated library, `chimera-dsp`, under the products

- **Status:** Proposed
- **Deciders:** owner (2026-10-08); firmware
- **Depends on:** ADR 0112 (parameter `Motion`, Proposed) and ADR 0095 (reverb
  integration, Proposed, unmerged). Accept 0112 and 0095 before this one; the
  Motion split and the exemption list below refer to them.

## Context
`chimera-engine/src/dsp/` (64 files) is organised by product: `algo/`, `modal/`,
`tape_engine/`, `filter/`, the reverb types, the voice and the FX bus. The
reusable techniques are buried inside them: the TPT stages in `k2040.rs` and
`ladder.rs`, the saturators in `filter/coef.rs`, the waveguide string in
`modal/`, the reverb kernels in the reverb types, the glide kernel in
`ease.rs`/`glide.rs`.

The owner plans to keep developing this DSP and sell it as plugins (Fors /
Sinevibes model), and to port it to other hosts (the Blackbox, a plugin
shell). The techniques are the asset; the products are compositions of them.

The owner also set the rule for what goes in: "we have to be very choicey about
what DSP blocks we have. It'd be very easy to look at the Faust DSP library and
steal that and build everything from there. I don't know if that's what we
want." A library is valuable for what it leaves out, and for owning cleanly
what it keeps.

Today the DSP is hard-wired to the firmware: `chimera_hal::SAMPLE_RATE` (48 kHz)
and `BLOCK_SIZE` (64) are compile-time constants used across the blocks, and
several "generic" blocks import product types (`EnvParams`, `FilterParams`,
`Cost`, `Clip`, the parameter `Block` trait). A plugin needs any rate and any
buffer size, so the library has to be written for that.

## Decision
1. **Two layers.** A new crate, `chimera-dsp`, holds technique blocks organised
   by kind, created as techniques arrive (e.g. `oscillators/`, `filters/`,
   `nonlinear/`, `reverbs/`, `physical/`, `modulation/`, `util/`; no empty
   folders). `chimera-engine` stays the product layer (ALGO, MODAL, TAPE, the
   voice, the FX bus, the parameter model, `Knobs`, `Cost`, `Clip`) and is
   composed from `chimera-dsp`. A future plugin is another product layer.
   A crate, not a `dsp::lib` module with a lint, because the compiler then
   enforces "knows nothing of Parts": the library cannot `use` product types.
   (The word "block" in `chimera-engine::block::Block` is the parameter trait;
   library items are called blocks informally and share no trait.)
2. **Platform.** `chimera-dsp` is `no_std`, `f32`, scalar (no SIMD), depends on
   `libm` only (not `chimera-hal`), with an optional `std` feature for host
   builds. Math goes through one `math` module (libm by default; goldens hold
   only under libm). Blocks take the sample rate at `prepare(sr: SampleRate)`
   (a newtype; coefficients computed there, never per sample) and process any
   block length from 1 up to a `MAX_BLOCK` constant. The firmware calls
   `prepare` once at boot at 48 kHz, so its goldens stay bit-identical. A
   block that cannot take a runtime rate yet stays in the product layer. Each
   block is tested at 44.1, 48, 96 and 192 kHz (response, tuning, stability;
   goldens at 48 kHz only). Every recursive block is denormal-safe without
   FTZ (a tested tail into silence at 192 kHz stays in range, or an explicit
   flush); a plugin shell also sets FTZ/DAZ. Per-sample functions are
   `#[inline]` so builds without fat LTO keep them inlined.
3. **Controls are plain numbers.** A block's docs state which inputs it
   smooths itself (these become `Smoothing::Own` in the product's `ParamSpec`),
   which it reads only at a trigger, and which it expects already smoothed. The
   product layer declares `Motion` (ADR 0112); the library only provides the
   one-pole glide kernel (`ease`/`glide`). `Knobs`, built from the registry,
   stays in `chimera-engine`.
4. **Provenance is a type, per file.** Each module declares
   `pub const PROVENANCE: &[Provenance] = &[...]` (several entries for mixed
   files, each naming what it covers). `Provenance` is a closed enum:
   - `Textbook { cite }`: a paper or the mathematics.
   - `Permissive { licence, notice }`: MIT/BSD/STK-style code, `notice` from
     `include_str!("../licences/...")`. CC-BY material taken verbatim
     (coefficients, a table) is recorded here too, attribution kept.
   - `CircuitModel { circuit, refs, check }`: schematic, datasheet or SPICE, no
     third-party code.
   - `Measured { session }`: the owner's bench capture of analog hardware.
   - `Original { author }`: "owner", or "AI-assisted, owner-directed and
     reviewed", stated honestly for counsel.
   - `OwnerExempt { adr, ruling }`: the closed exemption list in point 6.
   There is no GPL, LGPL or "unresolved" variant, so a copyleft source cannot
   be written down. A unit test walks `chimera-dsp/src/**/*.rs` (plain
   `std::fs` and a string match) and fails on any file without the
   declaration, or whose `licences/` notices differ from `THIRD_PARTY.md`.
   Unused consts cost nothing in firmware (LTO drops them), and the same
   consts generate each product's third-party notices, which ship with the
   product (about box or docs), not only in the repo.
5. **Two tiers, derived from provenance** (owner, 2026-10-08, quoted):
   - **Commodity:** "standard, well-documented techniques everyone uses (BLEP
     oscillators, FM, formant, TPT filters, envelopes, LFOs, delay lines,
     halfband, DC blockers)". `Textbook` and `Permissive`. Plumbing: correct,
     fast, clean, not distinctive.
   - **Signature:** "modelled from something the owner has played and
     measured", or a sound tuned by parameters and voltages. `CircuitModel`,
     `Measured`, `Original`. Never another company's model, emulation or
     decompiled firmware (see the exemption). LADDER is `CircuitModel`
     (Zavalishin's TPT ladder, Oyama's DAFx-26 paper, CC-BY 4.0, checked
     against ngspice, ADR 0104); the 2040 is `CircuitModel` (SSM2040
     datasheet, Kobol BOM, ADR 0105).
   The tier is `Provenance::tier()`, not a second declaration. It is metadata
   for naming, marketing and diligence, not a crate or module boundary.
   No third-party marks in product names; part numbers only descriptively, in
   docs. Commodity blocks are described by technique ("BLEP saw"); signature
   blocks get our own names.
6. **What may go in (rule and exemption).**
   - **Rule, for all future work:** never derived from GPL or LGPL code; never
     tables read from a device's firmware, ROM or a decompilation of either.
     Reference libraries (Faust, STK, Mutable Instruments, JUCE, others) are
     for reading and A/B, not copying. Facts from a manual or from measuring a
     device's output (TX81Z ratios, preset metrics) are allowed. Captures are
     allowed of analog circuits and of what a device synthesises, not of a
     device's stored samples, ROM tables or firmware. A new block is checked
     against the well-known GPL code in its technique area and the check is
     noted. A technique published within the last 20 years carries a "patent:
     checked / not checked" line (TAPE's STRETCH and SCRUB are in
     patent-heavy territory); the plugin release checklist clears every "not
     checked".
   - **Owner's ruling, 2026-10-08 (binding): "a complete pass" for the
     MidiVerb-derived reverbs.** The exemption list is closed and is exactly:
     ROOM's and BLOOM's delay/allpass/tap structure tables, and HALL's
     `DIFFUSER_MS` (ADR 0095). They are allowed in `chimera-dsp` and in paid
     plugins and carry `OwnerExempt { adr: 0095, ruling: 2026-10-08 }`, so the
     generated third-party notices list them. They are not quarantined. The
     legal question is tracked on #410 for the owner's counsel and is not a
     blocker. Adding to the list needs a new ADR.
7. **A block enters `chimera-dsp` only when:**
   - **A product needs it now:** it lands in the same PR as its first caller
     in a product, and a test fails if a `pub` item has no product caller
     (or items stay `pub(crate)` behind a facade).
   - **It is checked:** commodity by analytic or property tests (transfer
     function, impulse response, tuning, stability at the four rates);
     signature by a chimera-utils A/B against the SPICE/circuit model or the
     measured capture, plus the owner's ear.
   - **It is real-time safe** (no allocation, bounded state, NaN-safe) and
     tested.
   - **Cost** is a host instruction-count/cycle estimate at entry; on-unit
     cycles come at ship through the product's voice bill (`Cost` stays in
     `chimera-engine`; hardware checks happen at ship).
   `Measured` blocks come from a capture rig in chimera-utils, tracked in
   issue #425; it waits for the first such block.
8. **Stability of the sound.** `chimera-dsp` is versioned (semver, in-repo).
   Its goldens are the contract: a change that moves a golden is breaking, and
   lands as a new block or as a `Revision` taken at construction. The firmware
   takes the newest revision; a plugin pins the revision in its saved state.
9. **Extract gradually and bit-identically.** First wave, the product-free
   leaves: `dc_blocker`, `filter/coef` (`tan_accurate`, `g_of`), the
   `ease`/`glide` kernel, `halfband`, `fast_tanh` and the saturators. The
   envelopes, LFOs, filters (`k2040`, `ladder`) and limiter move only once
   their params, disk codes, `Cost` and `Clip` are split off (each is an API
   split, not a file move), each with an on-unit bench row for its hot path
   and the filter probes' static loop counts unchanged. After that, new
   technique code lands in `chimera-dsp` and product code moves out when next
   touched. Every move keeps the audio goldens bit-identical and the audio-ISR
   guard green.

## Alternatives considered
- **Leave the DSP organised by product.** Simplest today, but the techniques
  stay entangled with Chimera's voice and parameter model, which blocks reuse
  in plugins and on the Blackbox.
- **A `dsp::lib` module inside `chimera-engine` with a use-direction lint.**
  Cheapest, but a lint can be waived; a crate cannot import product types.
- **Build on an existing library (Faust's libraries, STK, Mutable's stmlib,
  JUCE's DSP module).** Fast breadth, but mixed and often copyleft licences
  (Faust's libraries carry per-function licences, some LGPL), generic quality,
  and nothing distinctive. The owner rejected bulk import.
- **One big extraction now.** Touches every DSP file at once while several
  branches are in flight; gradual extraction gets the same result without
  conflicts.
- **Quarantine the MidiVerb tables (`Encumbered` variant).** Considered in
  review; overruled by the owner's 2026-10-08 ruling above.

## Consequences
- The company's core asset becomes one crate with a clear ownership record,
  ready to sit under a plugin shell (CLAP/VST3) or another device. Every file
  carries a provenance const; the per-block cost is that const, a file-walk
  test and analytic tests for commodity blocks.
- Making blocks rate- and block-size-independent is real work and gates what
  can move first; goldens are guarded by the 48 kHz `prepare` path.
- Sound changes in the library are now breaking changes, handled by revisions.
- Some breadth is deliberately missing: a technique arrives only with a
  product that needs it.
- On firmware, performance is unchanged: `chimera-dsp` takes the release
  default (opt-level 2), fat LTO inlines across it, and ADR 0108's rules hold
  (no small crate instantiates a generic the audio runs). It gets
  `[profile.dev.package.chimera-dsp] opt-level = 1` beside the engine's, and
  the stack-check guard covers it.
- The MidiVerb exemption is a recorded owner decision with an open legal
  question (#410), not a cleared one.

## Sources
- Owner's direction, 2026-10-08 (DSP company plan; the curation and tier
  rules; the MidiVerb ruling).
- ADR 0108 (crate split by optimisation profile), ADR 0112 (parameter
  `Motion`), ADR 0095 (reverb integration, MidiVerb tables), ADR 0104/0105
  (LADDER, 2040 models), ADR 0023 (TX81Z ratios from the manual).
- ADR 0032 (MODAL provenance: Mutable's MIT notice), ADR 0098/0100
  (chimera-utils A/B tool).
- Issues #410 (MidiVerb legal question), #425 (capture rig).
