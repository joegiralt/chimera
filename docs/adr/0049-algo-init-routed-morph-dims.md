# 0049. Algo INIT is audibly routed; MORPH dims when A = B

- **Status:** Proposed
- **Deciders:** owner (#188, #189, 2026-09-29), firmware
- Supersedes part of 0024: the output scale counts sounding carriers only.

## Context
INIT was `AlgoParams::single(W1)`: operator 1 alone at LEVEL 99. Operator 1
is a carrier in all 32 algorithms, so every algorithm played the same sine
(spectral centroid 1.00 × f0 across all 32, #189). Only the volume moved:
ADR 0024's output scale, `1 / sqrt(max(1, Σ carrier gain))`, counted silent
carriers too, so switching ALGO jumped the level by up to 7.8 dB.

MORPH blends ALG A into ALG B. INIT and six of the eight factory Sounds have
ALG A = ALG B, where MORPH is the identity, yet the knob drew live and
ENV 2 → MORPH could be routed to no effect (#188).

## Decision
- **INIT is routed FM.** Operator 1 at LEVEL 99, operators 2–6 at LEVEL 72,
  every ratio 1.00, wave W1. ALG A is T1, ALG B is A1, MORPH 0.
  `AlgoParams::single` stays for Sounds that want one operator.
- **LEVEL 72.** At A3 the 32 algorithms' centroids spread 1.00–26.8 × f0
  (T1 8.7, A1 1.0). T1 keeps a strong fundamental (0.39, beside 0.33, 0.21,
  0.48 for harmonics 2–4). LEVELs 66–70 all but cancel it (0.015 at 68), so
  INIT reads an octave up. 64 keeps it but leaves T1 − A1 at 2.1 × f0,
  barely apart.
- **ALG B is A1.** Every operator is a carrier and there are no links, so B
  is T1's stacks' clean additive twin. MORPH works as an FM-depth macro: each
  step towards B lowers every link's weight, and the centroid falls
  monotonically (8.7, 4.1, 1.0 × f0 at MORPH 0, 48, 127), never harsher
  than A. ENV 2 → MORPH at +127 changes INIT's output by more than 10 % RMS.
- **Sounding carriers only.** The scale is `1 / sqrt(max(1, Σ gain))`, summed
  over carriers whose target gain is above 0 (`morph::Sounding`). A wave-swap
  duck does not count, so a swap never moves the other carriers' level. A
  lone operator is equally loud under every algorithm.
- **MORPH dims while ALG A = ALG B.** `view::dimmed` rules it inapplicable.
  The knob and focus band draw dimmed, the encoder is ignored, and MIX+PLUS
  on it reports NOT MODULATABLE. A route primed earlier stays. Its matrix
  column draws as a dimmed cell does: the name and amounts in MID, routes
  unlit, and the readout's effect in MID, tagged `INERT`
  (`mod_grid::inert_dests`).

## Alternatives considered
- **Keep INIT one operator; teach algorithms in the manual:** switching ALGO
  on a new Sound would still do nothing audible.
- **Gain-weighted scale (`Σ gain²`):** at ratio 1 the carriers are coherent,
  and a count leaves INIT's RMS spread over 9 dB across algorithms. The owner
  chose the count of sounding carriers. Weighting would also move every
  factory Sound's level.
- **ALG B = A17 (one six-deep chain):** a brighter B, but harsh (21 × f0 at
  A3) and less useful as INIT's default sweep.
- **Hide MORPH when A = B:** the page layout would shift under the encoder.

## Consequences
- Factory Sounds 0–5 have silent carriers, so they play louder by a pure
  gain: TX BASS, TX EPIANO and TX BELL +1.76 dB, TX BRASS and SQR BASS
  +3.01 dB, SAW LEAD +3.98 dB. SQR BASS feeds DRIVE 0.3, so its timbre
  moves too. The two MORPH Sounds are unchanged.
- Six operators cost more than one: at rev V's budget a pool of routed INITs
  fits six voices, not eight.
- INIT's RMS still varies about 9 dB across algorithms: coherent carriers of
  unequal level against a count-based scale.
- INIT through REV 0.5 peaks at 1.11 (a sine there peaked at 0.91). The
  reverb goldens now play a lone sine, so they lock the FX, not INIT.
- Tests that meant "one sine" build it with `AlgoParams::single(W1)`. The A4
  pitch gate reads the period, not zero crossings.

## Sources
#188, #189; `chimera-core/src/dsp/algo/{params,morph,engine}.rs`,
`chimera-core/src/ui/{view,mod_grid}.rs`; `algo_engine_test.rs`
(`init_algorithms_differ`, `a_lone_carrier_is_equally_loud_under_every_algorithm`),
`modulatable_test.rs` (`env2_into_morph_is_heard_on_init`),
`morph_dim_test.rs`.
