# 0029. Return the reverb and chorus in stereo

- **Status:** Accepted (2026-09-27); supersedes in part [0015](0015-voice-steal-and-fx-returns.md) (decision 2's clause "the return is mono into both sides of DAC pair 1")
- **Deciders:** project owner

## Context
ADR 0015 made the FX bus a true send/return with a mono return, leaving
stereo effects for later. The new ring reverb (ADR 0028) has distinct left
and right taps, and a mono chorus is narrow.

## Decision
- `FxBus::process` returns a `Stereo` block; `mix_parts` adds its L to
  DAC pair 1's left and its R to the right.
- The reverb returns its left and right taps (ring stages S1/S2/S3 for
  left, S3/S4/S1 for right).
- The chorus reads each BBD line twice: the normal tap on the left, a tap
  on the same triangle LFO inverted on the right; in Juno I+II each side
  averages its two lines' taps. The mono sum does not cancel.
- The delay stays mono and is added at unity to both sides.
- Everything else in ADR 0015 stands: each Part has three mono sends,
  each effect returns wet only, and its MIX is the return level.

## Alternatives considered
- **L = wet, R = −wet (the Juno trick):** cancels in the mono sum and
  leaves a lone −wet on the far side of a panned Part's dry.
- **A ping-pong delay:** needs a second 500 ms line; there is no RAM.
- **Per-Part FX buses:** several times the cost.

## Consequences
One extra interpolated read per chorus line (two in Juno I+II), no extra
memory. The mono sum of the chorus keeps at least −6 dB of one side.

## Sources
- `docs/superpowers/specs/2026-09-27-fx-diet-design.md` § Chorus, § Bus.
- `chimera-core/src/dsp/fx_bus.rs`, `chorus.rs`, `ring.rs`, `instrument.rs`; `tests/chorus_test.rs`.
