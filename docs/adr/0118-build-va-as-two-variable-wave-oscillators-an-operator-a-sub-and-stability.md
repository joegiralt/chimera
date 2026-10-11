# 0118. Build VA as two variable-wave oscillators, an operator, a sub and STABILITY

- **Status:** Proposed
- **Deciders:** owner (the spec approval, 2026-10-09; WAVE's reach and SYNC's master ceiling, 2026-10-11), firmware
- **Amends:** [0056](0056-modal-resonators-share-four-macros.md) (its cost-counting method; VA gets its own, point 9)

## Context
The VA engine slot was a stub with a draft of eleven one-per-voice models (#130-#136, #219). The spec (`docs/superpowers/specs/2026-10-09-va-variable-wave-design.md`) replaced it with one voice shape that covers the classic analog territory: two oscillators that morph their wave, a combine MODE, a sub and a STABILITY knob for analog character. It had to sit beside the reverb bus at six voices (ADR 0040) and keep every wave clean enough to hear.

## Decision
1. **Architecture.** VA is two variable-wave oscillators, an operator (MODE: MIX, SYNC, RING, XOR, BITS), a sub locked to OSC 1 and STABILITY. It is an open nod to the Mutable Instruments Ambika's architecture, taken from its manual only: no GPL code and no wavetables (ADR 0114).
2. **One level.** Every wave, at every SHAPE, is DC-free at one RMS, R = 0.2, with the anchors' fundamentals in phase. The operators scale to stay within 0.9. `VA_CHAIN_GAIN = 1/(R*sqrt(3))` puts the saw at +-1 into the chain (the 2040's calibration); `VA_OUT_GAIN` sets the loudness.
3. **Alias gates and exemptions.** MIX and SYNC pass the -45 dB gate at C6, and RING detuned at C5. XOR and BITS are exempt: they are bitwise and quantising by design. RING's coincident edges (phase-locked, C6: -31...-38 dB) and RING above SHAPE 1/2 (C5: -37...-43 dB) are known exceptions, tracked in private issue #449 and private issue #453.
4. **STABILITY.** The per-voice character seeds from a splitmix32 hash of (Part, slot, salt). STAB 1 renders bit-identically. Below 1 the character (trim, HF tracking, scale error) recalls from the seed, while drift and free-run phase differ per take. VA's state rides the idle slot rebuild as TAPE's strip does (`VaCarry`). The phase reset switches at exactly STAB = 1, a threshold on a continuous knob.
5. **Code home.** The technique files (`blep.rs`, `osc.rs`, `stability.rs`) are written for `chimera-dsp` (no product types, `prepare(rate)`, `PROVENANCE`) and stay in `chimera-engine/src/dsp/va/` until ADR 0114 is accepted; `dsp/provenance.rs` moves with them.
6. **Rulings that constrain later work:**
   - the sine's SHAPE crossfade, the split saw, the one-sample pulse floor, signed threshold crossings, the three-part sync split, the raised-cosine MODE duck (two blocks each way), and no cell hints without a gesture;
   - `blep.rs`'s `blep_into`, `blamp_into` and `blamp6_into` are the one kernel home; the one-sample-late corrector is inline in `VarOsc::run` and `Sub::step` (the sub writes `blep_into` out), so a wider kernel changes both correctors and the latency the tests assume (#451). `Corrector` is the tests' reference only;
   - a MODE edit bills `max(new, playing)` until the duck swaps the MODE (`held_model_extra`; VA's `playing_cost` is the playing MODE's);
   - with no VCA route, VA's key ends the note through the fade (`key_gates`): VA never decays on its own;
   - entering SYNC clears nothing, the slave's corrector included (the spec's MODE changes: SYNC keeps no reset state);
   - a WAVE segment crossing inside a block is solved from WAVE's linear ramp, and the solve is the split (the next pair follows the ramp's direction); a sample rounding leaves on either side plays within a hair of the anchor, where both pairs are its pure wave.
7. **PIT under the VA node.** The pitch page hangs under the VA node, as ADR 0042 allows for any engine node.
8. **Defaults.** VA's filter default is the 2040 (LP24, VOICE REV2) per accepted ADR 0096. The neutral fixture maps its `kind:` line per engine (`default_for`) instead of appending a row per engine as ADR 0096 says: one text, no existing line changed.
9. **VA count method (amends ADR 0056).** The bill is ADR 0056's arithmetic (instructions x 1.46 x 1.1, plus 12 for each `vdiv`/`vsqrt`) on the release ELF disassembled by `llvm-objdump`; the scripts are `bills2.py` and `count.py` (round 3's copies in (private notes)):
   - each oscillator role (OSC 1, OSC 2 plain, the SYNC slave) bills its dearest wave pair: the cheapest round of the pair loop with no event, plus each branch's side at its event rate;
   - event rates are per edge, at the maximum increment 0.45 a sample (SYNC's master 0.25, point 12), each stretched by the half-cent jitter: the wrap at the top increment; an edge forward the top + 0.225/64; an edge backward 0.225/64; a sync reset at the master's top;
   - the operator bills its MODE's loop (the biggest: the MODE duck's variant, its gain a load and a multiply a sample) every block, though the duck runs only the 4 blocks after a MODE edit; the sub's flips at 0.45;
   - block work (/64) is the dearest straight path through `VaEngine::render` and its MODE's `render_*`; `VarOsc::render`'s run loop is jump-threaded per pair, so its steady block is the dearest way through exactly one run;
   - the worst case is one WAVE segment crossing per oscillator per block (point 11), billed as a whole extra run (its straight path, both segment-end lerps inlined), the round of `VarOsc::render` between two runs and `run_end`; a crossing block's peak bound splits at the anchors once per oscillator. The inlined lerps also sit on the steady run's straight path, an over-bill;
   - exclusions: `Stability::prepare` (runs on a sample-rate change only) and `end_shape` (tests only);
   - bills round up to 10, LP24's 95 carried in each engine term.
10. **The honest bills** (cycles a sample, LP24 included; round 3 with points 11 and 12, 2026-10-11): MIX 510, SYNC 570, RING 510, XOR 530, BITS 510 (they were 610, 720, 620, 630, 620). MIX, RING and BITS fit 8 voices on rev V and 6 on rev Y (7 beside ORBIT or the master tape); XOR 7 and 6; SYNC 7 and 5 (7 beside ORBIT, 6 beside the tape). ADR 0040's six voices on rev Y is **met in every MODE but SYNC**; the rest of the cost work is tracked in private issue #444 and the per-Sound billing in private issue #448. Admission refuses gracefully, so the shortfall costs voices, not overruns.
11. **WAVE's reach (owner, 2026-10-11).** A block's WAVE ramp crosses one segment boundary (⅓ or ⅔) at most: asked past a second it stops just short of it, the rest carried into the next block (`osc::wave_reach`, applied to the ramp's target, so the ramp itself never snaps). `VarOsc::render` makes it structural: a block starts two runs at most, the second playing to the block's end (rounding past the edge plays that pair a hair beyond its end; the next block takes the new segment). *Audibly:* a jump end to end lands in two blocks (2.7 ms) instead of one, passing through the middle segment; WAVE modulation faster than one segment a block (an audio-rate LFO at full depth) lags and swings narrower. *Why:* a segment crossing costs a whole extra run, and two a block were the bill's worst case; one a block takes 21 cycles off every MODE (crossings 51.5 → 30.2) and puts XOR inside rev Y's 636.
12. **SYNC's master ceiling (owner, 2026-10-11).** While SYNC plays or is coming (the duck down), OSC 1's pitch stops at 12 kHz (`SYNC_MASTER_HZ`, 0.25 a sample at 48 kHz), ramped like any pitch, so it eases in and out under the MODE duck. *Audibly:* a SYNC master asked above 12 kHz plays 12 kHz (only past note 126 or with PITCH/CRSE pushing it there); the slave and the other MODEs are untouched. *Why:* the slave's resets and the master's events bill at the master's top increment; 0.45 → 0.25 takes about 57 cycles off SYNC (master 18, slave resets 39; 720 → 570 with point 11 and round 3), which still leaves SYNC at 5 voices on rev Y.

## Alternatives considered
- **The 09-27 draft's eleven one-per-voice models.** Kept as later macro-oscillator candidates; #130-#136 and #219 are re-scoped to that list.
- **`chimera-dsp` now.** ADR 0114 is not accepted; the code stays put and is written to move.
- **A per-interval RING product correction.** Simulated: fixes coincidence but is 5 dB worse detuned. The cheaper correction near coincidence is #449.
- **Flat event costs in the count** (2 x 1.8 x one event). Per-edge rates are exact and cut the over-bill.
- **Stopping to optimise toward 540 cycles** (rounds 1-2). Round 3 went on with no change to the sound: the run's setup inlined into its loop, edges tracked as the phase since them, a WAVE crossing solved instead of walked, the sine's polynomial in turns, the operators' constant work folded, the duck in the operator loop. Kept out, each measured: a crossing that lerps its three waves once (no cheaper), a separate steady run (its loop compiled worse), cold crossing paths (the events cost more than the fast path saved), the sine's angle accumulated instead of its phase (it drifts against the phase the tests replay).

## Consequences
- One voice shape covers MIX, SYNC, RING, XOR and BITS; every wave plays at one level, so MODE and SHAPE changes do not jump in loudness.
- STAB 1 is a bit-exact, repeatable sound; below 1 each take is alive.
- VA meets the goal in every MODE but SYNC (6 on rev Y; SYNC 5), with two audible limits the owner approved: WAVE's reach (point 11) and SYNC's 12 kHz master (point 12).
- The desktop simulator builds with `test-support`, so it runs the logging `apply` and carries the edge log in each `VarOsc`: the same sound (`Correction::On`, no skip), not the firmware's code byte for byte.
- Follow-ups: a 4-point kernel if a gate tightens (private issue #451); BITS ADAA if UAT finds BITS harsh (private issue #452); a WAVE morph scope glyph (private issue #450); on-unit bench rows per MODE and the owner's listening pass at ship.

## Sources
- The spec and plan: `docs/superpowers/specs/2026-10-09-va-variable-wave-design.md`, `docs/superpowers/plans/2026-10-09-va-variable-wave.md`.
- Valimaki & Huovilainen 2007 (2-point polyBLEP); Esqueda, Valimaki & Bilbao, EUSIPCO 2015 (polyBLAMP); Hohnerlein, Rest & Parker, DAFx-17 (the 4-point upgrade path); La Pastina & D'Angelo, DAFx20in22 (CC-BY 4.0; sine hard sync); Kleimola, DAFx-08 (bitwise modulation); Werner & Azelborn, DAFx23 (CC-BY 4.0; ADAA).
- The Mutable Instruments Ambika manual (architecture only).
- Issues: #444, #448, #449, #450, #451, #452, #453; re-scoped #130-#136, #219.
