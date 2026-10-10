# 0118. Build VA as two variable-wave oscillators, an operator, a sub and STABILITY

- **Status:** Proposed
- **Deciders:** owner (the spec approval, 2026-10-09), firmware
- **Amends:** [0056](0056-modal-resonators-share-four-macros.md) (its cost-counting method; VA gets its own, point 9)

## Context
The VA engine slot was a stub with a draft of eleven one-per-voice models (#130-#136, #219). The spec (`docs/superpowers/specs/2026-10-09-va-variable-wave-design.md`) replaced it with one voice shape that covers the classic analog territory: two oscillators that morph their wave, a combine MODE, a sub and a STABILITY knob for analog character. It had to sit beside the reverb bus at six voices (ADR 0040) and keep every wave clean enough to hear.

## Decision
1. **Architecture.** VA is two variable-wave oscillators, an operator (MODE: MIX, SYNC, RING, XOR, BITS), a sub locked to OSC 1 and STABILITY. It is an open nod to the Mutable Instruments Ambika's architecture, taken from its manual only: no GPL code and no wavetables (ADR 0114).
2. **One level.** Every wave, at every SHAPE, is DC-free at one RMS, R = 0.2, with the anchors' fundamentals in phase. The operators scale to stay within 0.9. `VA_CHAIN_GAIN = 1/(R*sqrt(3))` puts the saw at +-1 into the chain (the 2040's calibration); `VA_OUT_GAIN` sets the loudness.
3. **Alias gates and exemptions.** MIX and SYNC pass the -45 dB gate at C6, and RING detuned at C5. XOR and BITS are exempt: they are bitwise and quantising by design. RING's coincident edges (phase-locked, C6: -31...-38 dB) and RING above SHAPE 1/2 (C5: -37...-43 dB) are known exceptions, tracked in private issue #449 and private issue #453.
4. **STABILITY.** The per-voice character seeds from a splitmix32 hash of (Part, slot, salt). STAB 1 renders bit-identically. Below 1 the character (trim, HF tracking, scale error) recalls from the seed, while drift and free-run phase differ per take. VA's state rides the idle slot rebuild as TAPE's strip does (`VaCarry`). The phase reset switches at exactly STAB = 1, a threshold on a continuous knob.
5. **Code home.** The technique files (`blep.rs`, `osc.rs`, `stability.rs`) are written for `chimera-dsp` (no product types, `prepare(rate)`, `PROVENANCE`) and stay in `chimera-engine/src/dsp/va/` until ADR 0114 is accepted; `dsp/provenance.rs` moves with them.
6. **Rulings that constrain later work:** the sine's SHAPE crossfade, the split saw, the one-sample pulse floor, signed threshold crossings, the three-part sync split, the raised-cosine MODE duck (two blocks each way), `Corrector` as the one kernel slot, and no cell hints without a gesture.
7. **PIT under the VA node.** The pitch page hangs under the VA node, as ADR 0042 allows for any engine node.
8. **Defaults.** VA's filter default is the 2040 (LP24, VOICE REV2) per accepted ADR 0096. The neutral fixture maps its `kind:` line per engine (`default_for`) instead of appending a row per engine as ADR 0096 says: one text, no existing line changed.
9. **VA count method (amends ADR 0056).** The bill is ADR 0056's arithmetic (instructions x 1.46 x 1.1, plus 12 for each `vdiv`/`vsqrt`) on the release ELF disassembled by `llvm-objdump`:
   - each oscillator role (OSC 1, OSC 2 plain, the SYNC slave) bills its dearest wave pair: the cheapest round of the pair loop with no event, plus each branch's side at its event rate;
   - event rates are per edge, at the maximum increment 0.45 a sample, each stretched by the half-cent jitter: the wrap 0.45; an edge forward 0.45 + 0.225/64; an edge backward 0.225/64; a sync reset 0.45;
   - block work (/64) is the dearest straight path through `VaEngine::render` and its MODE's `render_*`, loops billed at their trip count (the duck: 4 trips every block);
   - the worst case is two WAVE segment crossings per oscillator per block, each billed as a whole extra run;
   - the MODE duck is billed by trip count, every block;
   - exclusions: `Stability::prepare` (runs on a sample-rate change only), `end_shape` (tests only), and `__aeabi_memclr4` at its real cost (69 instructions);
   - bills round up to 10, LP24's 95 carried in each engine term.
10. **The honest bills** (cycles a sample, LP24 included): MIX 610, SYNC 720, RING 620, XOR 630, BITS 620. They fit 6-7 voices on rev V (SYNC 6; SYNC 5 beside ORBIT) and 4-5 on rev Y. ADR 0040's six voices on rev Y is **unmet for VA**; the cost work is tracked in private issue #444 and the per-Sound billing in private issue #448. Admission refuses gracefully, so the shortfall costs voices, not overruns.

## Alternatives considered
- **The 09-27 draft's eleven one-per-voice models.** Kept as later macro-oscillator candidates; #130-#136 and #219 are re-scoped to that list.
- **`chimera-dsp` now.** ADR 0114 is not accepted; the code stays put and is written to move.
- **A per-interval RING product correction.** Simulated: fixes coincidence but is 5 dB worse detuned. The cheaper correction near coincidence is #449.
- **Flat event costs in the count** (2 x 1.8 x one event). Per-edge rates are exact and cut the over-bill.
- **Stopping to optimise toward 540 cycles.** The one idea left saves about 20 a sample, not the 70-90 needed; deferred to #444.

## Consequences
- One voice shape covers MIX, SYNC, RING, XOR and BITS; every wave plays at one level, so MODE and SHAPE changes do not jump in loudness.
- STAB 1 is a bit-exact, repeatable sound; below 1 each take is alive.
- VA costs more than the goal: 6-7 voices on rev V, 4-5 on rev Y.
- Follow-ups: a 4-point kernel if a gate tightens (private issue #451); BITS ADAA if UAT finds BITS harsh (private issue #452); a WAVE morph scope glyph (private issue #450); on-unit bench rows per MODE and the owner's listening pass at ship.

## Sources
- The spec and plan: `docs/superpowers/specs/2026-10-09-va-variable-wave-design.md`, `docs/superpowers/plans/2026-10-09-va-variable-wave.md`.
- Valimaki & Huovilainen 2007 (2-point polyBLEP); Esqueda, Valimaki & Bilbao, EUSIPCO 2015 (polyBLAMP); Hohnerlein, Rest & Parker, DAFx-17 (the 4-point upgrade path); La Pastina & D'Angelo, DAFx20in22 (CC-BY 4.0; sine hard sync); Kleimola, DAFx-08 (bitwise modulation); Werner & Azelborn, DAFx23 (CC-BY 4.0; ADAA).
- The Mutable Instruments Ambika manual (architecture only).
- Issues: #444, #448, #449, #450, #451, #452, #453; re-scoped #130-#136, #219.
