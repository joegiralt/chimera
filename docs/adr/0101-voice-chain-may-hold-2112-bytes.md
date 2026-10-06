# 0101. The voice chain may hold 2,112 B (supersedes in part 0051)

- **Status:** Proposed
- **Deciders:** owner (the 2040 and its VOICE, 2026-10-06), firmware (Task 8 review)

## Context
ADR 0051 asserts a `Voice` is at most `VOICE_CHAIN_BYTES` (2,048 B) plus its `EngineSlot`, so the chain can't grow unnoticed. Before the 2040 the chain was 2,032 B. ADR 0096's filter types keep every type's state apart, so the crossfade can run two at once: the 2040 adds `K2040` (44 B: four states, three ramp ends, its last VOICE) and `Filter` a last-type tag. The chain is now 2,080 B. ADR 0096's "under ADR 0051's assert" didn't hold.

## Decision
- `VOICE_CHAIN_BYTES` is **2,112 B**: 2,080 measured, rounded up to the next 64. The assert stays, and the bound stays tight: the next type (DIRTY) or the tap state raises it again, with its own measured figure, through an ADR.
- Only the number changes. The rest of 0051 (one engine per voice, rebuilt in place, the slot's own assert) stands.

## Alternatives considered
- **Share the ramp ends between CLEAN and the 2040** (FREQ's `g`, LEVEL's gain): saves about 16 B, not the 32 over, and couples the types' ramps, which Task 10's crossfade wants apart.
- **Shrink `Voice` elsewhere** (`played`, a whole `ParamSnapshot`): churn far outside the filter for 32 B.
- **Drop the assert:** loses 0051's reason for it.

## Consequences
- D2 holds 8 × 6,328 = 50,624 B of its 286,720 B; the change is 384 B in all.
- The chain grows only by an ADR, as before.

## Sources
- ADR 0051, ADR 0096; spec `docs/superpowers/specs/2026-10-05-filter-types-design.md` § Memory.
- Plan `docs/superpowers/plans/2026-10-05-filter-types.md`, Task 8 § Measured; commit ebcc54bf.
