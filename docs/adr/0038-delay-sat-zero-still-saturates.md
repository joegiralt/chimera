# 0038. The delay's feedback loop saturates at every SAT, SAT 0 included

- **Status:** Proposed
- **Deciders:** firmware (audit #52)

## Context
The tape delay applied `tanhf(g·x)/g` in its feedback loop only when SAT
> 0.01; at SAT 0 the loop was linear. Its one-pole TONE filter has unity DC
gain and FDBK reaches 1.0, so at FDBK 1 and SAT 0 a held note grew on every
repeat without bound, clipping the DACs and the reverb ring.

## Decision
`tanhf(g·x)/g` runs at every SAT, with g = 1 + 3·SAT as before, so SAT 0 is
g = 1: the gentlest curve, never none. Every write is then bounded by
|input| + FDBK. SAT > 0.01 is unchanged sample for sample, which the
`delay_feedback_100ms` FX golden, recorded before the change, locks. The
curve stays `tanhf`: ADR 0031 keeps the spec's Padé fallback parked.

## Alternatives considered
- Cap the loop gain below 1 (e.g. FDBK × 0.98): an arbitrary constant, and
  FDBK 1 would no longer sustain.
- A separate limiter only at SAT 0: a second curve, and a seam at 0.01.

## Consequences
SAT 0 is no longer clean: a 0.5-peak repeat loses about 0.6 dB per pass and
gains third harmonic around −34 dB; quiet repeats are unaffected. FDBK 1
sustains at a bounded level instead of running away. `tanhf` now runs per
sample at SAT 0 too, as it already did at the default SAT 0.2.

## Sources
FX diet spec § Delay; ADR 0031; audit issue #52; `delay_test.rs`
`full_feedback_with_no_saturation_stays_bounded`.
