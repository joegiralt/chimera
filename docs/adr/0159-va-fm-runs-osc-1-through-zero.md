# 0159. Run VA MODE FM as OSC 2 driving OSC 1's read through zero

- **Status:** Proposed
- **Deciders:** owner (the VA MODEs plan, 2026-10-19, issue #601), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) points 1 (FM joins the operators), 3 (FM's alias gate and its exceptions) and 10 (FM's bill); builds on [0158](0158-va-oscillators-read-a-modulated-phase-beside-a-clean-clock.md)

## Context
VA's MODEs combined two finished waves (MIX, SYNC, RING, XOR, BITS). Issue #601 asks for a through-zero FM MODE in the manner of an analogue through-zero VCO pair (the Intellijel DPO, an instrument reference only): OSC 2 moves OSC 1's rate, the rate goes negative when the modulator is deep, and the note stays in tune at every depth. ADR 0158 gave `VarOsc` a read phase beside its clock, so the read can be modulated without the clock, the sub or the jitter following it. What was left was the index curve, how high a read can run, what is heard, the alias gate (Carson's rule does not hold for a saw carrier) and the bill.

## Decision
1. **The MODE.** FM is disk code 5, ident `FM` (`VaMode::Fm`), appended; nothing renumbered. OSC 2 renders plain; its samples, scaled to its block peak bound (`b/pk.b`, in −1..1) and by the index, drive OSC 1's `MOD` render with `k = (1, 0)`: OSC 1's read rate is `inc·(1 + I·m)` (ADR 0158 point 2). This is linear FM of the rate, so the read's mean rate is OSC 1's note and the pitch stays centred at every index.
2. **The index.** `I = 5·BLEND²` (`fm_index`), ramped across each block (`fm: Ramp`), held at the MODE duck's swap so FM's first block starts on its value. Past `I = 1` the rate goes negative on the modulator's troughs: the read runs backward (through zero).
3. **The read-rate cap.** `I` is capped at `max(0, 0.45/inc − 1)`, so OSC 1's fastest read `inc·(1 + I)` stays under 0.45 cycles a sample (`MAX_INC`). The cap is taken at the faster end of the block's increment ramp, and the ramp's start is clamped to this block's cap, so a pitch jump past the old cap lands on it. The cap is pitch-dependent: top notes lose index, they never ask the edges to cross faster than their tracking holds. At C6 (1047 Hz) the full `I = 5` fits; it starts to bite near 3.6 kHz.
4. **What is heard.** OSC 1 alone, through the sub and the chain gain as MIX (OSC 2 is the modulator, inaudible, as the DPO's main out). BLEND's ends read `CLEAN` (0, OSC 1 plain) and `DEEP` (1, `I = 5`).
5. **The sub stays in tune.** The sub flips on OSC 1's clock, not on its read, so it holds the note's octave below whatever the index does.
6. **The scope.** MODE's glyph integrates the read across its points, `φ += Δt·(1 + I·b/pk.b)` (`I` uncapped), and draws OSC 1 there; the other MODEs keep `combine`. `va_glyph_test` locks every MODE's scope at BLEND 0.5 and 1 (`MODE_GOLDENS`).
7. **The alias gate** (amends 0118 point 3). `alias_db_lattice(f_c, f_m)` on the lattice of both pitches, notes 72 and 84, CRSE2 0, 7 and 12, WAVE1 and WAVE2 over sine, triangle, saw and pulse, BLEND ¼, ½ and 1:
   - Sine and triangle carriers: −45 dB where Carson's bandwidth `(I + 1)·f_m + f_c` fits under Nyquist. Worst measured −51.8 dB (84 +12, triangle carrier, sine modulator, BLEND 1).
   - **Saw and pulse carriers** (ruling R4): the correction must win 10 dB over the uncorrected render, not −45 dB. Carson is a sine-carrier rule: harmonic n is deviated n times as far, so a saw's band reaches Nyquist at any index. Measured −23.3 to −48.2 dB at BLEND 1 (uncorrected −1.5 to −19.6), −42.2 to −60.2 dB at BLEND ¼ and ½; the least correction is 12.9 dB. Most of it is the 2-point kernel at up to 6× the note's read rate (BLEND 1, `I = 5`): a saw carrier under a sine modulator reads −34 to −38 dB at C6, what a 2-point BLEP gives a plain saw that high. The rate's corners alone are not the cause: a sine carrier under a saw modulator, the same corners, reads −57.7 dB or lower. A saw modulator adds about 10 dB more (84 +0: −23.3 against −34.4 under a sine), likely its edge stepping and flipping the rate inside the BLEP's 2-sample support; tracked in private issue #650.
   - **CRSE 7 at the just fifth** (ruling R5): measured at FINE2 +1.955 cents (3:2), not the equal-tempered fifth. The ET fifth's lattice, cut at `|i|, |j| ≤ ⌈24 kHz/f⌉ + 1`, leaves in-band sidebands of harmonics above the cut unmasked (C5 pulse carrier, sine modulator, BLEND ½: −30.9 dB ET against −58.6 dB at 3:2; uncorrected the two read the same). A limit of the measuring tool, not of the sound.
8. **The bill** (amends 0118 point 10). By ADR 0118 point 9's count (`bills2.py` with a `mod_master` role, (private notes)): OSC 1's MOD instance 222.8 (SINE/TRI: its edges follow the read, crossed either way at the 0.45 cap plus their own motion, each at its dearest side with the crossing's divide, +12 a `vdiv`; the wrap and the sub's flip on the clock as MIX's master), OSC 2 plain 134.4, operator and sub 29.4, block 74.2 (5.1 of it the modulator's scaling loop), crossings 29.9, LP24 95: **585.7, bill 590** (`COST_FM`). With INIT's +98 a voice is 688: 7 voices on rev V (7 beside ORBIT, 7 with the master tape), **6 on rev Y**, so ADR 0040's six voices holds in FM. The other MODEs' bills are unchanged.

## Alternatives considered
- **Phase modulation (DX-style) instead.** PM is a separate MODE (PHASE, #602); FM of the rate is what a through-zero VCO pair does, and only it runs the read backward.
- **Exponential FM.** It bends the pitch with depth (the mean rate rises); linear FM keeps the note centred.
- **No cap, or clamping the rate per sample.** Uncapped, top notes' reads pass ½ a cycle a sample and the edges' crossings stop tracking; clamping per sample bends the waveform and the pitch. Capping the index a block keeps the shape and lets the top notes lose only depth.
- **OSC 2 heard beside OSC 1.** The modulator's pitch would sit in every patch, and BLEND would have two jobs.
- **Gating saw carriers at −45 dB.** Not reachable with 2-point kernels at 6× the note's rate; a 4-point kernel (#451) or a gentler index curve is post-alpha work (#650).
- **The equal-tempered fifth in the alias gate.** It measures the lattice mask, not the alias.

## Consequences
- FM loses rev V's eighth voice (7 at 590 + 98); rev Y keeps six.
- A saw or pulse carrier at full BLEND on top notes aliases audibly more than the gate implies for a sine carrier (down to −23 dB at C6); BLEND ½ stays under −42 dB.
- At a locked 1:1 ratio (CRSE 0, FINE 0, STAB 1, both from phase 0) the level moves with BLEND: +2.00 dB at ½, −1.33 dB at 1 against MIX. The read's path repeats every cycle and plays a fixed wave with its own RMS; any detune or a fifth averages it out (≤ 0.21 dB). Physics, measured and not gated.
- The modulator's depth `I/pk.b` takes OSC 2's peak bound a block, so it steps a hair at block edges while WAVE2 or SHAPE2 move; the bound is continuous block to block, so the steps are tiny, and it keeps the cap's guarantee (`|b/pk.b| ≤ 1`, so `|m| ≤ I`).
- The bench gains an `FM` row (BLEND 1, OSC 2 CRSE 0); its unit reading is pending.
- PHASE, AM, SOFT SYNC, MAX and LOOPFM reuse the scope's per-point dispatch.

## Sources
- The plan: `docs/superpowers/plans/2026-10-19-va-modes.md` (Tasks 3 and 4); rulings R4 and R5 (private notes); Task 3's report (the alias table).
- Chowning, "The Synthesis of Complex Audio Spectra by Means of Frequency Modulation", JAES 21(7), 1973 (FM). Through-zero linear FM of the read rate is from first principles; the Intellijel DPO is an instrument reference only (its manual's description, no code).
- Carson's bandwidth rule (textbook).
- ADR 0118 point 9 (the count), ADR 0158 (the read phase), ADR 0040 (six voices on rev Y).
