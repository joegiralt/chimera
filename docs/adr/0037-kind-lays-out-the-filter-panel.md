# 0037. KIND lays out the filter panel; MODE follows KIND

- **Status:** Proposed
- **Deciders:** project owner (2026-09-27; 2026-09-28, AMP VEL's priming and the saturator's hot path)

## Context
The filter models (#123–#127) each bring a classic synth's panel and their own modes. MODE was a raw `u8` on no page (#111).

## Decision
- KIND (Filter ParamId 6) is FLT's first knob; it sets knobs 2–6 to that synth's panel, from `const` data (`ui/filter_panel.rs`). A label is the original panel's word; the target is the same everywhere. A kind's row lands with its model; today the SVF is the only kind: CUTOFF · RES · MODE · ENV · KEY, with DRIVE and LFO on FLT › MODE.
- Route knobs are views of matrix cells (ADR 0035); shortcuts to elsewhere (TB-303's DECAY) arrive with their kind.
- MODE (ParamId 7) is a typed `FilterMode` whose discriminants 0–7 are the old byte. `mode ∈ kind.modes()` holds on every write of KIND or MODE. A KIND change keeps MODE if the new kind has it, else takes its default. A single-mode kind shows MODE fixed and dimmed.
- The matrix always applies; "not shown, not applied" covers only the filter's own parameters.
- Filter ParamIds 3 (FM), 4 (ENV) and 5 (KEY) are retired and never reused (ADR 0009).
- A fixed or inapplicable slot draws dimmed (label and value in MID, no bar); its encoder is ignored. An absent route draws a dash, with no arc. `renderer::look` decides a cell's look (Live, Dimmed or Absent) in one place: the cell, the focus band and the region's Focus key all read it (#123).
- One exception to the dimming rule, the owner's decision: a dimmed cell whose MIX+PLUS primes a different, hidden destination still primes it. AMP's VEL, dimmed under the pass-through, primes the VCA (ADR 0035), since there is no other way in.
- `FilterKind::cost(kind, mode)` bills each kind like an engine (ADR 0026), as its mode's delta over LP24, which the engine terms already carry. The SVF bills PHASER 17 (the bench's SVF row, 500, less 1 OP, 483), BP24 and HP24 2 (provisional), and LP24, LP6, LP12, BP12 and NOTCH 0. `saturate`'s hot divide (1 < |x| < 1.5) is not billed, by the owner's decision (#157): the eight-voice stress run peaked at 87 % with no overruns.

## Alternatives considered
- **KIND rewriting routes on a change** (the SH-101 kind setting ENV 1 → VCA): it would destroy the user's routing.
- **One MODE list for every kind:** would show modes a model doesn't have.
- **A flat SVF bill:** the engine terms already carry LP24, so every voice would pay the SVF twice.
- **A reciprocal multiply in `saturate`:** it changed audio goldens, factory Sounds among them.

## Consequences
Each model issue adds its `FilterKind` variant, its panel row, its modes and its cost. The model choice, topologies and oversampling policy are #128's own ADR. A patch that drives the SVF hot can run past its bill (#157).

## Sources
`docs/superpowers/specs/2026-09-27-filter-routing-design.md` § 6–7; plan `docs/superpowers/plans/2026-09-28-filter-routing.md` (`## Measured`, Task 15); #111, #122, #123, #128, #157.
