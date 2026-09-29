# 0049. Algo INIT is audibly routed; MORPH dims when A = B

- **Status:** Proposed
- **Deciders:** owner (#188, #189, 2026-09-29), firmware
- Supersedes part of 0024: the output scale is the carrier power, not the
  carrier count.

## Context
INIT was `AlgoParams::single(W1)`: operator 1 alone at LEVEL 99. Operator 1
is a carrier in all 32 algorithms, so every algorithm played the same sine
(spectral centroid 1.00 × f0 across all 32, #189). Only the volume moved.
ADR 0024's output scale, `1 / sqrt(max(1, Σ c))` with `c` each carrier's
blended weight, counted silent carriers too, so switching ALGO jumped the
level by up to 7.8 dB.

MORPH blends ALG A into ALG B. INIT and six of the eight factory Sounds have
ALG A = ALG B, where MORPH is the identity. Yet the knob drew as live, and
ENV 2 → MORPH could be routed to no effect (#188).

## Decision
- **INIT routes four operators.** Operator 1 is at LEVEL 99, operators 2–4
  at LEVEL 72 and operators 5–6 are silent. Every ratio is 1.00 on wave W1.
  ALG A is T1, ALG B is A1, MORPH is 0. `AlgoParams::single` stays for
  Sounds that want one operator.
- **Four, so INIT keeps eight voices.** `AlgoEngine::cost` charges an
  operator whose LEVEL is above 0. Four routed operators come to 691 per
  voice, and eight of them fit rev V's budget (ADR 0040), which
  `eight_init_voices_fit_rev_v` pins. Six operators would cost 819, and only
  six voices would fit.
- **LEVEL 72.**
  - At A3, T1's four-deep stack keeps a strong fundamental: 0.60, against
    0.40, 0.34 and 0.66 for harmonics 2–4.
  - LEVELs 66–70 nearly cancel it (0.03 at 68), so INIT would read an
    octave up.
  - Across the 32 algorithms the centroids spread 1.00–8.7 × f0, with T1 at
    8.7 and A1 at 1.0.
- **ALG B is A1.** With operators 5–6 silent, T1 is a single four-deep
  stack. A1 has no links and every operator is a carrier, so it is that
  stack's clean additive twin. MORPH then works as an FM-depth macro: each
  step towards B lowers every link's weight, and the centroid falls
  monotonically from bright to clean.
- **The output scale is the carrier power:**
  `1 / sqrt(max(1, Σ c·g²))`. Here `c` is each carrier's blended weight
  (`blend(carrier_a, carrier_b, m)`), and `g` its gain from LEVEL after
  modulation (`level_gain`), with velocity excluded.
  - It is computed once per block, from the LEVEL gains the block already
    has for its mip choice (`morph::carrier_power`, `carrier_norm`).
  - At unit gains it is ADR 0024's count.
  - `max(1, ·)` means it never boosts.
  - It is continuous in every gain, so a carrier fading through LEVEL 0
    moves the scale smoothly.
  - Velocity is excluded, so a soft note never makes another carrier louder.
  - INIT's loudness spread across the 32 algorithms falls from 9.0 dB (the
    count of sounding carriers, six-operator INIT) to 3.7 dB. It was 7.8 dB
    on the one-sine INIT.
- **Factory Sounds keep their sound through their data, not code.**
  - Each Sound's `out.volume` is 0.8 × old / new scale.
  - SQR BASS feeds DRIVE, so its op 1 LEVEL drops to 95 (−3.0 dB, T1's old
    1 / √2). DRIVE 0.3 is trimmed to 0.29950 to take out LEVEL 95's
    +0.01 dB, so the clipper's input is unchanged.
  - TX BASS, TX BRASS, SQR BASS and MORPH KEYS then render as before, to
    about 1e-7.
  - Four can't match exactly (#192). TX EPIANO, TX BELL and SAW LEAD reach
    the SVF louder, and its integrator saturation sits before `out.volume`
    (up to 0.13, 0.83 and 0.54 dB per block).
  - MORPH PAD's LFO sweeps MORPH, and the old scale followed that sweep
    (up to 1.06 dB).
  - `factory_level_test.rs` holds each Sound to its 937b89f render.
- **MORPH dims while ALG A = ALG B.**
  - `view::dimmed` rules it inapplicable. The knob and focus band draw
    dimmed, the encoder is ignored, and MIX+PLUS on it reports
    NOT MODULATABLE.
  - A route primed earlier stays. Its matrix column draws the way a dimmed
    cell does: the name and amounts in MID and the routes unlit, and the
    readout's effect in MID, tagged `INERT` (`mod_grid::inert_dests`).

## Alternatives considered
- **Keep INIT at one operator and teach algorithms in the manual.**
  Switching ALGO on a new Sound would still do nothing audible.
- **Six routed operators.** This was the first cut. It costs 819 per voice,
  which fits only six INIT voices on rev V, against ADR 0040's eight.
- **A count of sounding carriers (gain > 0).** This was the first cut too.
  It steps the scale 3 dB as a carrier's LEVEL crosses 0. When a velocity
  drives a carrier to 0, it makes the others louder on a soft note. At ratio
  1 it also leaves INIT's loudness spread across algorithms at 9 dB.
- **Gains that include velocity in the power.** A soft note would then
  raise the other carriers slightly whenever the power exceeds 1.
- **ALG B = A17.** In a four-operator INIT, T1 and A17 are the same stack,
  so MORPH would do nothing.
- **Hide MORPH when A = B.** The page layout would shift under the encoder.

## Consequences
- INIT peaks past full scale: about 1.27 at MORPH B, where four carriers
  sit in phase at one ratio, and 1.55 through REV 0.5 (#193). #190's gain
  staging, a voice-sum trim and a final limiter, is meant to cover it. The
  reverb goldens play a lone sine meanwhile.
- The four factory Sounds in #192 differ from their old render by up to
  about 1 dB per block until the owner picks an option there.
- INIT's loudness still varies 3.7 dB across algorithms. Its carriers are
  coherent at ratio 1, and the power norm assumes uncorrelated ones.
- Tests that meant "one sine" build it with `AlgoParams::single(W1)`. The A4
  pitch gate reads the period, not zero crossings.

## Sources
#188, #189, #190, #192, #193; ADR 0024, 0040.
`chimera-core/src/dsp/algo/{params,morph,engine}.rs`,
`chimera-core/src/factory.rs`, `chimera-core/src/ui/{view,mod_grid}.rs`.
Tests: `algo_engine_test.rs` (`init_algorithms_differ`,
`the_output_scale_is_continuous_as_a_carrier_fades_out`,
`a_soft_note_never_makes_another_carrier_louder`), `algo_morph_test.rs`,
`modulatable_test.rs` (`env2_into_morph_is_heard_on_init`),
`instrument_test.rs` (`eight_init_voices_fit_rev_v`),
`factory_level_test.rs`, `morph_dim_test.rs`.
