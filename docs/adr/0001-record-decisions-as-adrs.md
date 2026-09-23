# 0001. Record architectural decisions as ADRs

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
Design intent was spread across long specs, 7,800-line implementation plans,
review rounds and chat. Specs get superseded (the engine-pivot spec was
partly overturned the same day it was written), and several large design
docs (`chimera-synth-design.md`, product architecture, UI/UX spec) are
partly stale. "Ask an agent to dig through the specs" breaks down once specs
contradict each other.

## Decision
Every decision that constrains future work (architecture, algorithm choice
and provenance, UX behavior, licensing) gets a short ADR in `docs/adr/`,
numbered, using `0000-template.md`, and listed in `README.md`.
ADRs are never edited after acceptance except to mark them superseded; a
changed decision is a new ADR. Specs cite ADRs instead of re-arguing them;
plans stay disposable.

## Alternatives considered
- **Superpowers specs/plans as the record.** They say *what/how* at a point
  in time; the *why* is buried in revisions, and they go stale.
- **One big living design doc.** Already exists in several copies; drifts.

## Consequences
One place to answer "why is it like this". Small ongoing cost per decision.
CLAUDE.md tells every agent session to write them.

## Sources
Session of 2026-09-23 (engine pivot brainstorm).
