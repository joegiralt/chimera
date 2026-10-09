# VA Engine: variable-wave core: Design

- **Status:** Draft, awaiting owner review
- **Date:** 2026-10-09
- **Supersedes:** `2026-09-27-va-engine-design.md` (rev 2, never reviewed; its 11 one-per-voice models become candidates for later macro oscillators, see § Later)
- **Research:** (private notes) (codebase), (private notes) (what makes oscillators alive), (private notes) (literature check; references in § References)
- **Ships in:** alpha (#414)

## Intent

Most digital synths sound sterile. The ones that don't build the movement into the oscillator: a few well-chosen controls that come alive under modulation, and an instability that lets the sound breathe (owner, 2026-10-09, on the Cobalt8 and the OP-XY). VA is Chimera's oscillator engine built on that idea.

Its architecture is a nod to the Mutable Instruments **Ambika**: two variable-waveform oscillators combined by a digital operator, plus a sub-oscillator locked to oscillator 1. We take the architecture from the Ambika manual and nothing else. The algorithms, names and tables are ours. The Ambika's firmware is GPL and is not read for code, and its wavetables are not used (ADR 0114).

**For the alpha** there is one engine with one sub-type, the **variable-wave core**, deliberately basic. It is the first of the macro oscillators (§ Later).

**Done when:**
- `EngineType::Va` plays the core on the desktop and the chip;
- every continuous knob modulates without clicks;
- INIT lands within ±1 dB of `REFERENCE_LUFS`;
- the worst VA voice leaves 6 voices beside the FX bus on rev V and rev Y (ADR 0040);
- the owner approves it by ear.

## Pages

The VA chain has three pages. EDIT moves down and wraps; MINUS/PLUS move along a page (D-pad, ADR on navigation).

**Top page, VA:**

| Cell | Param | Range | Does |
|---|---|---|---|
| WAVE1 | OSC 1 WAVE | 0–1 | Morphs OSC 1: sine → triangle → saw → square |
| WAVE2 | OSC 2 WAVE | 0–1 | The same for OSC 2 |
| MODE | MODE | MIX, SYNC, RING, XOR, BITS | How OSC 1 and OSC 2 combine |
| AMNT | AMOUNT | 0–1 | Depends on MODE (§ Combine) |
| SUB | SUB | 0–1 | Sub-oscillator level |
| STAB | STABILITY | 0–1 | 1 = steady; lower = the oscillators drift and breathe (§ Stability) |

**↓ OSC 1 page:**

| Cell | Param | Range | Does |
|---|---|---|---|
| CRSE | OSC 1 COARSE | −24…+24 semitones | Pitch offset |
| FINE | OSC 1 FINE | −100…+100 cents | Fine tune |
| SHAPE | OSC 1 SHAPE | 0–1 | The wave's own variation (§ Oscillator) |

**↓ OSC 2 page:** the same three controls for OSC 2.

FOLD isn't an operator: the voice already folds after the filter. Each cell gets a focus-band hint (ADR 0086) and a value bar from its `Motion` (ADR 0112 point 8).

## Oscillator

Each oscillator is a band-limited phase oscillator with two controls, WAVE and SHAPE.

**WAVE** morphs continuously through four anchors at 0, ⅓, ⅔ and 1: sine, triangle, saw, square. Between two anchors the output is a linear crossfade of the two neighbours, so at most two waves are evaluated per sample. It never steps (the owner's never-snap rule).

**Anchor phase.** All four anchors share the phase of their fundamental: sine `sin 2πφ`; triangle peaking at φ = ¼; a **falling** saw `1 − 2φ`; square high for the first half-cycle. A crossfade then never cancels the fundamental. A rising saw (fundamental −(2/π)·sin) would cancel it by 19.4 dB at the tri→saw midpoint, a hole in the knob; the falling saw loses 1.0 dB there. No paper covers this.

**Anchor level.** Each anchor is scaled to the saw's RMS (−4.8 dB): sine ×0.816 (√(2/3)), triangle ×1, saw ×1, square ×0.577 (1/√3). The factors fold into the crossfade weights at block rate: 0 cycles, peak-safe, and the square end no longer jumps +4.8 dB.

**SHAPE** varies whichever waves are sounding. At 0 every wave is its plain self.

| Wave | SHAPE 0 → 1 |
|---|---|
| Sine | Folds brighter: `sin(g·sin θ)` with g from 1 to 3 |
| Triangle | Skews toward a ramp: the peak moves from ½ to 0.95 of the cycle |
| Saw | Lifts the middle part of the ramp: a second step, whose position and height grow with SHAPE |
| Square | Pulse width 50% → 5% (PWM under modulation) |

**Antialiasing:** polyBLEP at every step (saw, square, saw lift, sync reset), and polyBLAMP at every corner (triangle, skew, sync reset). These are 2-point kernels: polyBLEP (Välimäki & Huovilainen 2007 [3]) and polyBLAMP (Esqueda, Välimäki, Bilbao 2015 [4]). Välimäki et al. 2012 [2] measure the 2-point polyBLEP perceptually alias-free to about 2.3 kHz at 48 kHz (about C7), so the C6 gate has margin. The 4-point B-spline [2] and 4-point polyBLAMP [5][6] (about +20 dB) are the upgrade if the gate moves to C7 or higher (about +2–4 cycles per sample per oscillator).

- **Superposition:** every step or corner whose kernel window covers a sample adds its own correction to it. Several per sample are normal: a 5% pulse at C8 (11.5-sample period) puts both edges in one window, and the saw lift's second step can sit beside the wrap. No per-sample if/else. Free.
- **Sine fold, no ADAA:** the fold's input is the phase ramp, not an arbitrary signal, so ADAA [9–13] buys nothing. By Jacobi–Anger, harmonic n has weight J_n(g). At g = 3 and 48 kHz the worst alias is below −140 dB up to C7, and −42 dB at C8. If C8 matters, cap `g ≤ 3 − 1.5·clamp((f0 − 2.5 kHz)/2 kHz, 0, 1)` (one multiply-add per block).

**Pitch:** the note, plus COARSE and FINE, plus STABILITY's drift. The voice's pitch modulation applies to both oscillators. Exponential conversion runs once per block, with a per-sample linear increment ramp.

## Combine

Let `a` be OSC 1 and `b` be OSC 2, both in −1…1, and let `x` be AMOUNT.

| MODE | Output | AMOUNT |
|---|---|---|
| MIX | `(1−x)·a + x·b` | Balance: 0 = OSC 1, 1 = OSC 2, ½ = equal |
| SYNC | as MIX, with OSC 2's phase reset at every OSC 1 wrap (corrected, below) | Balance. Sweep OSC 2 COARSE/FINE for the sync sound |
| RING | `(1−x)·½(a+b) + x·(a·b)` | From an equal mix to pure ring |
| XOR | `(1−x)·½(a+b) + x·xor16(a, b)` | From an equal mix to pure XOR |
| BITS | `crush(½(a+b), x)` | Bit depth 16 → 2 bits as x goes 0 → 1, a log curve |

- `xor16` maps each input to 16-bit signed, XORs the words and maps back.
- XOR and BITS alias by nature; that is their character, and they are exempt from the alias gate (documented in the tests).
- **SYNC reset:** at each OSC 1 wrap, at fractional position d, OSC 2 resets to phase 0. The combined slave output (WAVE crossfade and SHAPE included) gets one polyBLEP scaled by `y(0) − y(φ⁻)` and one polyBLAMP scaled by `(y′(0) − y′(φ⁻))·inc`, both evaluated at the exact reset instant. BLEP alone is right only where the slope is equal either side (saw, square); a triangle, a sine (slope jump 2π·inc·(1 − cos φ⁻)), the fold and the skew need the BLAMP. The leftover curvature jump scales as inc² (about 0.02 at C6), under the gate. For a pure-sine slave, La Pastina & D'Angelo's exact residual [1] is the upgrade if the C7 alias gate fails. About 50 cycles an event, inside SYNC's 30-per-sample line at typical masters.
- **XOR of identical oscillators is silent** (same wave, pitch and phase: `a XOR a = 0`), and passing through identical states pops [8]. By design. At STABILITY = 1 both start at phase 0, so with equal WAVE and tuning, XOR at AMOUNT 1 is silent. INIT's +6-cent FINE avoids it; detune under 20 cents is the fat, moving texture [8].
- **MODE changes** use the duck: the output ramps to 0 over one block, the mode swaps, and it ramps back over the next. SYNC's reset state is cleared on entry.
- **Sub:** a polyBLEP square one octave below OSC 1, toggled at OSC 1's wraps. It is added after the combine: `y = (c + SUB·s) / (1 + SUB)`, which stays peak-safe.

## Stability

STABILITY is 1 for steady and lower for alive. Its depth follows a curve `d = (1 − STABILITY)²`, so the top of the knob is fine-grained.

- **Drift:** each voice has three slow random walks: one shared, one for OSC 1 and one for OSC 2. Each is filtered noise in the 0.05–0.5 Hz range, updated once per block. OSC 1 takes shared + its own; OSC 2 takes shared + its own. So the pair drifts together and their interval beats slowly. Depth up to ±12 cents shared and ±6 cents each at `d = 1`.
- **Tolerance:** each voice gets a fixed small detune per oscillator, up to ±3 cents at `d = 1`, like a real VCO's trim.
- **High-frequency tracking (owner, 2026-10-09):** a real VCO's core adds a fixed reset or switching delay τ to every cycle (its exponential converter's bulk resistance has the same first-order effect, cents ∝ f) [17][18], so `f_eff = f / (1 + f·τ)`, a detune that grows roughly linearly with frequency. Each oscillator gets a seeded signed `τ = τ_max·d·u`, with u in [−0.25, 1]: mostly flat, sometimes slightly sharp, like an over-compensated VCO. `τ_max` = 4 µs, derived from the CEM3340 datasheet [17] (I/f ≈ 10 nC, r_e 5–20 Ω gives 2–8 µs): an untrimmed-VCO **estimate, not a measurement**. At `d = 1`, C7 (2093 Hz) lands between −14.4 and +3.6 cents. `f_eff` clamps so that 1 + f·τ ≥ 0.5. The two oscillators pull apart more the higher they play, and their beating speeds up with pitch, as on analog. Applied in the once-per-block pitch conversion, so it costs nothing per sample. The sub follows OSC 1's tracked pitch.
- **Scale error:** each oscillator also gets a seeded V/oct error `ε = ±0.2 %·d` (CEM3340 untrimmed typical [17]), applied as `pitch_oct·(1 + ε)` about C4: octave stretch, ±2.4 cents per octave at `d = 1` (±7.2 cents at C7, ±3 at A2). About 20 cycles a block per oscillator.
- **Jitter:** a per-cycle pitch wobble up to ±0.5 cent at `d = 1`, applied at each wrap.
- **Phase at note-on:** at STABILITY = 1, both oscillators start at phase 0, for a repeatable attack. Below 1, they free-run from where the voice last left them, like analog oscillators.
- **The sub follows OSC 1 exactly.** In SYNC, the slave's drift moves the sync ratio, so the timbre shimmers.
- **Recallable:** all randomness comes from xorshift seeded by (Part, voice slot), so a saved Sound sounds the same on reload, and STABILITY = 1 renders bit-identically every time.

## Voice and engine

VA plugs in on TAPE's pattern (ADR 0093; paths in `va-refresh.md` § 1):
- `EngineType::Va`, disk code 3, ident "VA";
- `BlockRef::Va`, block code 35, a frozen ident;
- a `SlotKind::Va` arm in `EngineSlot`, with a const-assert that `VaEngine` fits the slot;
- arms in `out_gain`, `cost`, `model_cost` and `playing_cost`, with `engines_test` guarding them;
- the nav map and `VA_BLOCKS` in `block_registry.rs`.

The voice's filter, drive, fold, envelopes, LFOs and modulation matrix are unchanged; VA is only a source. `Sound::init` for VA routes **ENV 1 → VCA** at 127, as TAPE does. With that route deleted, the note ends through the voice fade.

**INIT:**
- WAVE1 = WAVE2 = saw (⅔), SHAPE 0;
- OSC 2 FINE +6 cents;
- MODE MIX, AMOUNT ½;
- SUB 0;
- STABILITY 0.7 (`d` = 0.09, about ±1 cent shared drift), so it breathes a little out of the box; 0.85 gives `d` = 0.0225, ±0.27 cents, probably inaudible. Confirm by ear;
- filter CLEAN LP4.

## Params and Motion (ADR 0112)

| Param | Type | Motion | Mod destination |
|---|---|---|---|
| WAVE1, WAVE2, SHAPE1, SHAPE2, AMOUNT, SUB, STABILITY | `f32` 0–1, step 1/128 | `Continuous(Knobs)` | yes |
| FINE1, FINE2 | `f32` cents | `Continuous(Knobs)` | yes |
| CRSE1, CRSE2 | `i8` semitones | `Stepped(Snap)` | yes (stepped) |
| MODE | `u8`, `VaMode` | `Stepped(Crossfade)` (the duck) | no |

- `VaMode` is `#[repr(u8)]` with fixed discriminants 0–4; an unknown byte reads as MIX (no legacy compat needed, it only has to load safely).
- The Sound record grows by 12 params: check `MAX_SOUND_COST` (ADR 0087) and the ≤ 640 B record limit (ADR 0102).

## Code layout

```
chimera-engine/src/dsp/va/
  mod.rs        VaEngine: render, MODE duck, dispatch
  osc.rs        variable-wave oscillator (WAVE morph, SHAPE per wave)
  blep.rs       polyBLEP / polyBLAMP kernels
  combine.rs    MIX, SYNC, RING, XOR, BITS
  stability.rs  drift walks, tolerance, jitter, seeding
```

- The product layer lives in `chimera-engine` (ADR 0108, opt 2).
- Technique files (`blep.rs`, `osc.rs`, `stability.rs`) are written for the library: rate-independent through a `prepare(sr)` that computes their coefficients, with no product types, and each carrying a `PROVENANCE` const (ADR 0114, all Textbook or Original). When ADR 0114 is accepted they move to `chimera-dsp` unchanged.
- The `chimera-dsp` crate is not created for alpha.
- polyBLEP (2007) and polyBLAMP (2015) are under 20 years old, so each carries a "patent: not checked" line for the plugin checklist.

## Budget and billing

- The voice budget is 7,000 cycles per sample on rev V and 5,833 on rev Y; with the bus, that is 970 / 775 per voice at 6 voices (`va-refresh.md` § 4).
- **Estimate for the worst core voice:**
  - two oscillators mid-morph: 2 × 2 waves, about 160;
  - SYNC: about 30;
  - sub: about 15;
  - stability: about 10;
  - shell: about 30;
  - LP24 carry: 95;
  - `CHAIN_COST` 36 and routing 47.
  - Total about 420, which leaves 8 voices on rev V.
- **Billing:** `VaEngine::cost` = one committed term per MODE, host-counted by the ADR 0056 method, as TAPE did. `playing_cost` covers a MODE change under a held note. Unit bench rows come at ship.
- **Loudness:** `VA_OUT_GAIN` is named like `TAPE_OUT_GAIN`, and set so INIT reaches −11.0 LUFS ±1 dB (ADR 0109) at LEVEL 45/128.

## Testing

- **Alias gate** at C6 for each WAVE anchor and each midpoint, with SHAPE at 0, ½ and 1, in MIX, SYNC and RING (XOR and BITS exempt).
- **Superposition:** a 5% pulse at C8 and the saw lift (SHAPE 1) at C7 and C8 also pass the alias gate.
- **Anchor phase:** across the WAVE sweep at SHAPE 0, the fundamental never dips more than 3 dB below its strongest value.
- **Anchor level:** the four anchors' RMS are equal within 0.1 dB.
- **Pitch:** each anchor within 1 cent at A4 (same DFT method as the 09-27 draft).
- **No clicks:**
  - every continuous knob swept 0 → 1 over 100 ms, plus every MODE change mid-note;
  - criterion: the largest second difference is at most 1.5× the held renders' (the FX-diet criterion; BITS and XOR exempt).
- **Every knob audible:** `rms(y₀ − y₁) ≥ 0.1·rms(y₀)` for each continuous knob. RING and XOR AMOUNT are tested with detuned inputs (INIT's +6 cents); XOR of identical oscillators at AMOUNT 1 is silent.
- **Stability:**
  - STABILITY = 1 is bit-identical across runs and voices.
  - Below 1, the same Sound on the same Part and slot renders identically after a reload.
  - Drift depth stays inside its bounds.
  - **HF tracking:** at `d = 1`, τ alone puts a C7 note between −14.5 and +3.7 cents (the seeded u in [−0.25, 1]). At A2 it is within 1 cent of true. At STABILITY = 1 it is exactly 0. The OSC 1–OSC 2 beat rate at C7 exceeds the one at C4 where their τ differ.
  - **Scale error:** C4 is exact; at `d = 1` C7 is within ±7.2 cents and A2 within ±3 cents of true; at STABILITY = 1 exactly 0.
- **Peak-safe:** |y| ≤ 1 for every MODE, AMOUNT and SUB.
- **Plumbing and sanity:** `engines_test` arms; disk round-trip of all 12 params; the ADR 0011 sanity gate for INIT; loudness of INIT.
- **Library-ready:** each technique file is tested at 44.1, 48, 96 and 192 kHz (tuning and stability).

## Later (not alpha)

- OSC 1 and OSC 2 each become a choice of **macro oscillator**, each with its own pages of two controls.
- The variable-wave oscillator is the first macro oscillator. Candidates for the others come from `va-alive-survey.md` and the 09-27 draft: swarm, formant, sync families, PD, chaos.
- MODE stays as the glue between the two.
- Drift capture of the owner's Kobol, SE-02 and prologue may later tune STABILITY's curves (#425 rig).
- BITS ADAA (Werner & Azelborn 2023 [9]), about 25 cycles, if UAT finds it harsh.
- Issues #130–#136 and #219 get re-scoped to that list once this lands.

## ADRs

One new ADR (next free number, ≥ 0118) records:
- the architecture: two oscillators, an operator, a sub and STABILITY, as an open nod to the Ambika;
- the XOR/BITS alias exemption;
- STABILITY's seeding rule;
- the code-home rule (written for `chimera-dsp`, kept in `chimera-engine` until ADR 0114 is accepted).

## References

"Textbook" below means: licence not stated, so the provenance is Textbook. Every patent note for a technique under 20 years old reads "not checked". A web search found no patent for any of them, but that is not a freedom-to-operate check.

1. **P. P. La Pastina, S. D'Angelo**, "A General Antialiasing Method for Sine Hard Sync", *DAFx20in22*, 2022. https://www.dafx.de/paper-archive/2022/papers/DAFx20in22_paper_3.pdf
   - Licence: CC-BY 4.0. Patent: not checked.
2. **V. Välimäki, J. Pekonen, J. Nam**, "Perceptually informed synthesis of bandlimited classical waveforms using integrated polynomial interpolation", *J. Acoust. Soc. Am.* 131(1):974–986, 2012. DOI 10.1121/1.3651227. https://mac.kaist.ac.kr/pubs/ValimakiPeknenNam-jasa2012.pdf
   - Licence: ASA copyright → Textbook. Patent: not checked.
3. **V. Välimäki, A. Huovilainen**, "Antialiasing oscillators in subtractive synthesis", *IEEE Signal Processing Magazine* 24(2):116–125, 2007. This is the 2-point polyBLEP.
   - Licence: IEEE → Textbook. Patent: not checked. I did not re-read it; it is the canonical reference.
4. **F. Esqueda, V. Välimäki, S. Bilbao**, "Aliasing reduction in soft-clipping algorithms", *EUSIPCO*, 2015. This is the 2-point polyBLAMP.
   - Licence: Textbook. Patent: not checked. I did not re-read it.
5. **F. Esqueda, V. Välimäki, S. Bilbao**, "Rounding Corners with BLAMP", *DAFx-16*, 2016. https://www.dafx.de/paper-archive/2016/dafxpapers/18-DAFx-16_paper_33-PN.pdf
   - Licence: Textbook. Patent: not checked.
6. **C. Hohnerlein, M. Rest, J. D. Parker**, "Efficient Anti-aliasing of a Complex Polygonal Oscillator", *DAFx-17*, 2017. https://www.dafx.de/paper-archive/2017/papers/DAFx17_paper_100.pdf
   - Licence: Textbook. Patent: not checked.
7. **A. Argentieri, F. Scagliola**, "Arbitrary Polygon Oscillator: Generalizing Polygonal Synthesis to Arbitrary Shapes, Morphing, and Three-Dimensional Polyhedra", *DAFx26*, 2026. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_32.pdf
   - Licence: CC-BY 4.0. Patent: not checked.
8. **J. Kleimola**, "Audio Synthesis by Bitwise Logical Modulation", *DAFx-08*, 2008. https://www.dafx.de/paper-archive/2008/papers/dafx08_13.pdf
   - Licence: Textbook. Patent: not checked.
9. **K. J. Werner, E. Azelborn**, "Antialiasing Piecewise Polynomial Waveshapers", *DAFx23*, 2023. https://www.dafx.de/paper-archive/2023/DAFx23_paper_61.pdf
   - Licence: CC-BY 4.0. Patent: not checked.
10. **L. Gabrielli, S. Squartini**, "Simplifying Antiderivative Antialiasing with Lookup Table Integration", *DAFx25*, 2025. https://www.dafx.de/paper-archive/2025/DAFx25_paper_30.pdf
    - Licence: CC-BY 4.0. Patent: not checked.
11. **L. Gabrielli, S. Squartini**, "PolyADAA: Improving Aliasing Reduction in Memoryless Nonlinearities Using Lagrange Interpolation and Polynomial Approximation", *DAFx26*, 2026. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_47.pdf
    - Licence: CC-BY 4.0. Patent: not checked.
12. **M. Holters**, "Combined Derivative/Antiderivative Antialiasing", *DAFx20in22*, 2022. https://www.dafx.de/paper-archive/2022/papers/DAFx20in22_paper_7.pdf
    - Licence: CC-BY 4.0. Patent: not checked.
13. **S. Bilbao, F. Esqueda, J. D. Parker, V. Välimäki**, "Antiderivative Antialiasing for Memoryless Nonlinearities", *IEEE Signal Processing Letters* 24(7):1049–1053, 2017.
    - Licence: IEEE → Textbook. Patent: not checked. Canonical; I did not re-read it.
14. **L. Gabrielli, S. D'Angelo, L. Turchet**, "Analysis and Emulation of Early Digitally-Controlled Oscillators Based on the Walsh-Hadamard Transform", *DAFx-19*, 2019. https://www.dafx.de/paper-archive/2019/DAFx2019_paper_13.pdf
    - Licence: Textbook. Patent: not checked.
15. **R. Simionato, S. Fasciani**, "Towards Neural Emulation of Voltage-Controlled Oscillators", *DAFx25*, 2025. https://www.dafx.de/paper-archive/2025/DAFx25_paper_33.pdf
    - Licence: CC-BY 4.0. It reports no drift statistics.
16. **J. Roth, D. Keller, O. Castañeda, C. Studer**, "Alias-Free Oscillator Synchronization via Additive Synthesis", *DAFx26*, 2026. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_49.pdf
    - Licence: CC-BY 4.0. Useful only as an offline ground truth (`va-refresh.md`).
17. **Curtis Electromusic Specialties**, *CEM3340/3345 Voltage Controlled Oscillator* datasheet, c. 1980. https://static.miraheze.org/sdiywikiwiki/1/1c/CEM3340-3345_long.pdf
    - A manufacturer's measurement data, used here for numbers only.
18. **Sound Semiconductor**, *SSI2130 FATKEYS VCO* datasheet. https://www.soundsemiconductor.com/downloads/ssi2130datasheet.pdf
    - Used for numbers only.
