# 0011. Golden recordings are a refactor lock, not a quality claim

- **Status:** Accepted (2026-09-23)
- **Deciders:** project owner

## Context
The engines had never been listened to or validated. A behavior-preserving
refactor still needs proof that sound didn't change.

## Decision
Before refactoring, a sanity gate checks each engine's init patch (not
silent, finite, within ±1.0, silent after note-off, pitch within a
semitone). Then goldens (FNV-1a hash of all samples + spot samples) freeze
today's output bit-for-bit. A failing engine gets an issue and its golden
still locks the broken output; fixing sound is separate work that re-records
goldens deliberately. Goldens are never re-recorded to make a refactor pass.

## Alternatives considered
- **Fix engines during the refactor** — mixes structure and sound changes,
  so neither can be verified.

## Consequences
Modal's goldens lock known-broken output (octave high, rings after
note-off; `docs/issues/003-modal-sanity-gate.md`). Listening remains the real
quality test, on the desktop simulator.

## Sources
`chimera-core/tests/golden_test.rs`, `sanity_test.rs` (commit 05fdfa9).
