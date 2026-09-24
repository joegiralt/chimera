# 0015. Steal released voices first; FX sends return wet only

- **Status:** Accepted (2026-09-24)
- **Deciders:** project owner

## Context
The instrument core (Performance of Parts sharing a 6-voice pool and one FX
send bus) shipped two behaviors the final review flagged as surprising:
1. When the pool was full, the allocator stole the oldest voice by note-on,
   which could cut a held drone while newer notes were only ringing out.
2. Each effect on the send bus returned its own dry/wet mix, so turning up a
   Part's send leaked dry signal into DAC pair 1 — ignoring the Part's pan
   and output assignment.

## Decision
1. **Voice stealing:** when the pool is full (or the CPU budget needs room),
   steal the oldest *released* (tail-ringing) non-mono voice first; only if
   none exist, steal the oldest *held* non-mono voice. Mono voices are never
   stolen.
2. **FX returns:** the send bus is a true send/return. Each effect receives the
   sum of the Parts' sends and returns only its wet signal; an effect's MIX
   acts as its return level. The return is mono into both sides of DAC pair 1
   (stereo effects are later work).

## Alternatives considered
- **Steal oldest by note-on** (original spec) — can cut held notes before
  fading tails.
- **Keep per-effect dry/wet on the bus** — makes sends change a Part's level,
  width and output routing.

## Consequences
The `reverb_send_on` instrument golden was re-recorded deliberately (pair 1 no
longer carries dry send). Standalone effect `process()` keeps dry/wet for
non-bus callers; the bus uses `process_wet()`.

## Sources
Final review of the instrument-core branch;
`chimera-core/src/voice_alloc.rs`, `chimera-core/src/dsp/fx_bus.rs`;
supersedes the steal rule in
`docs/superpowers/specs/2026-09-24-instrument-core-design.md` § Voice allocation.
