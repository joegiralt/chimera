# 0166. Set VA MODE LOOPFM's b0 on an even curve to a sharp pulse

- **Status:** Proposed
- **Deciders:** owner (the A/B pick, variant B, issue #683), firmware
- **Supersedes in part:** [0162](0162-va-loopfm-loops-each-oscillator-in-closed-form.md) points 4 (BLEND's `B = 0.95·BLEND²`), 5 (WAVE's scale of `B`) and 7 (where the cap binds); amends its point 2 (the output scale above `|b0|` 5/7)

## Context
On the unit LOOPFM did little across most of BLEND (#683). The `b0` cap was never the limit: below C6 it allows `|b0|` 0.94–0.99, and the curve never asked for more than 0.724. Two things squeezed it. `B ≤ 0.95` through `b0 = −B/(1 + √(1 − B²))` caps `|b0|` at 0.724 (harmonic `k` is `|b0|^(k−1)`: centroid 3.6 harmonics, 15 within −40 dB, against INIT's MIX saws at 26 at C3). And `B = 0.95·BLEND²` with `|b0| ≈ B/2` near 0 made three quarters of the knob a sine with a trace of h2 (−30 dB at BLEND 32, −18 at 64, −11 at 96); it all happened in the last ~25 steps.

Three variants were rendered for the owner (scratch `9a768a1c`, (private notes)): A (`|b0|` to 0.846), B (to 0.95) and C (B with OSC 2 odd-only, +110 on the bill). The owner picked B. Its catch was headroom: held at RMS `R`, B's top peaks +14 dB over MIX's saws (crest 21 dB).

The full-chain check (INIT VA: 2040 LP24 wide open and at 1 kHz, the clean SVF, FX OFF and FUZZ, AMP, the output stage; C2–C5, BLEND 127). MIX's saws peak at 0.98–0.99 into the filter's LEVEL clipper (`sat_c1`, transparent to ±1, flat from 2). Unleaned, B's top drove it flat: about +1 dBFS out of the limiter's gain, the limiter taking 3.6 dB for one note against MIX's 0.4, and the pulse's tips clipped (no oversampling there). A-weighted, the unleaned top was +3 to +7 dB louder than MIX.

## Decision
1. **B's curve** (supersedes 0162 point 4). `b0` is set directly (`lbfm::target_b0`): the harmonic at −45 dB rises in octaves, `h − 1 = 100^e`, `e = min(1, 1.5·WAVE·BLEND)`, `|b0| = 0.0056^(1/(h − 1))`; 0 at `e = 0`. At WAVE ⅔: `|b0|` 0.197, 0.601, 0.853, 0.9495 at BLEND 32, 64, 96, 127 (the top's −45 dB harmonic is the 101st). Measured at C3, unison: centroid 1.25 / 2.51 / 6.78 / 19.8 harmonics, 3 / 10 / 29 / 89 within −40 dB. `B` and its root are gone (two `exp2` a block in place of a root and a divide); the paper's eq. 26 stays only as a test's reference.
2. **WAVE scales `e`** (supersedes 0162 point 5): ×1.5·WAVE, so ⅔ (the default) is ×1, 1 reaches the top at BLEND ⅔ and holds, 0 never loops. BLEND 0 is a clean sine for every WAVE (level against MIX: 0.00 dB).
3. **The sign** is 0162 point 6's: `target_b0` is negative, capped in magnitude only.
4. **The cap** (supersedes 0162 point 7). `b0_cap` is unchanged. At the top it now binds from about C4 (0.944 against 0.9495): C5 0.891, C6 0.789, C7 0.609. Alias gate (lattice, BLEND 1, WAVE ⅔ and 1, C5–G8): worst −72.4 dB at C5 (was −75.4 overall); the gate is −45.
5. **The lean** (amends 0162 point 2's scale). Up to `|b0| = 5/7` the scale is RMS `R`, `R·√2/√(1 − b0²)`; past it the peak `scale·(1 + |b0|)` is held at `PEAK = 2√3·R`, twice a saw's peak at RMS `R` (`lbfm::scale_of`; the halves meet at 5/7). So the engine's peak stays within +6.2 dB of MIX's saws at every BLEND, C2–C5: inside the LEVEL clipper's knee, never its flat. The RMS gives way instead: −2.8 to −3.9 dB at BLEND 96, −4.4 to −9.4 dB at 127 (−4.4 at C5, where the cap already trims). A-weighted the top sits −1.0 to −4.5 dB from MIX (C2–C5), from +3 to +7 unleaned: a sharp pulse's energy lies where the ear is keen. Through the chain at BLEND 127: pre-limiter peaks −6.0 to −7.5 dBFS at C2–C4 (MIX −6.2 to −7.0), the limiter taking 0.8–1.0 dB (MIX 0.3–0.4); C5 −2.6, 2.4 dB; BLEND 96 1.3–2.6 dB. With FUZZ or the 2040 at 1 kHz the limiter takes nothing. It is one compare a block in `scale_of`, no divide added.
6. **The bill** stays 350 (0162 point 10). `render_loop`'s per-sample loop is untouched; the per-block work trades a root and a divide for two `exp2` and adds a compare (host: B ran +5 % over the old curve, the noise of a block's setup).
7. **The scope** draws `target_b0` uncapped, as before; goldens `loopfm_050`, `loopfm_100` and the door prints (`va_proxy_test`) are re-recorded for the new sound.

## Alternatives considered
- **A** (`|b0|` to 0.846, peak +9 dB): an even sweep with half B's top; the owner preferred B.
- **C** (B with OSC 2 odd-only): a character option, but about +110 on the bill and away from "each loops on its own frequency".
- **B unleaned:** clips the filter's LEVEL stage (flat tips, aliasing there) and leans on the limiter (3.6 dB for one note), and is the loudest MODE A-weighted.
- **A lean at RMS (`k/(1 + |b0|)` everywhere), or a lower peak (+3.5 dB, `PEAK` 2.6·R):** quieter through more of the knob (−10.5 dB RMS at the top) for no clipping gained; the knee's end is the natural bound.
- **Keeping `B` and raising its ceiling:** the root compresses near 1 and keeps the double taper; setting `b0` directly is cheaper and even.

## Consequences
- LOOPFM sweeps evenly from a sine to a sharp pulse; BLEND 64 is already as bright as the old 127, the top as bright as MIX's saws at C3–C4.
- Patches saved with LOOPFM sound brighter at the same BLEND (no legacy compat: they load, not sound the same).
- Past `|b0|` 5/7 (about BLEND 76 at WAVE ⅔) LOOPFM gets quieter in RMS as it sharpens; peak and A-weighted level stay near MIX.
- Above about C4 the top is trimmed by the cap, never aliasing.
- Bill 350, voices unchanged.

## Sources
- Issue #683; the diagnosis and renders: (private notes), `measure.md` (scratch `9a768a1c`, branch `loopfm-voice`, not merged).
- Tamara Smyth, "Loopback Frequency Modulation Using a Time-Varying Delay Line", DAFx-26, paper 36 (CC BY 4.0): the closed form (eq. 25), as ADR 0162; credited in `THIRD_PARTY.md` and `lbfm.rs`'s `PROVENANCE`.
- ADR 0162 (LOOPFM), 0109 (the output stage's knee), the filter LEVEL stage (`filter::level`).
- Tests: `va_modes_test` (`loopfm_curve_is_even`, `loopfm_alias_gate`, `loopfm_peak_stays_near_mix`, `loopfm_blend_sweeps_do_not_click`, `loopfm_is_dc_free_at_level`), `cost_test` (bill 350).
