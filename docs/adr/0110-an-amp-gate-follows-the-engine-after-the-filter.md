# 0110. An amp gate after the filter follows the engine's tail

- **Status:** Proposed
- **Deciders:** owner (ruling 2026-10-08, #374); firmware

## Context
ALGO and MODAL shape their note inside the engine: ALGO's operator envelopes, MODAL's resonator decay. Nothing after the filter gates the voice unless a source routes into VCA (ADR 0096's fixed VCA column; spec § 4). A filter that self-oscillates (CLEAN from RES 0.95; the 2040, DIRTY and LADDER too) is driven by the engine only to start. It keeps ringing after note-off until the engine ends: MODAL INIT held a 0.45 peak for 10.5 s, and `sweep_thorough` reported `Stuck Filter.RESO`. The owner ruled that the voice's amp gates the whole voice after the filter, as on a classic subtractive synth and the Cascadia, so a ringing filter follows the release. MODAL must still ring like Rings: note-off never damps the resonator.

## Decision
- **Order.** ENGINE, DRIVE, FILTER, FOLDER, tap-VCA, DC blocker, **amp gate**, VCA (routes) / OUT LEVEL. The gate sits after the blocker, beside the VCA, and is a different stage from the VCA column: it needs no route and no ENV.
- **No second envelope.** The envelopes are inside the engines, so there is no amp ENV to move: the gate derives the tail from the engine's raw output (`dsp/gate.rs`, `AmpGate`).
- **Held, open.** While the key is down the gate is open and costs nothing (a held self-oscillation is a played tone, as on any synth).
- **Released, capped.** From note-off the voice may be no louder than `REACH` (4, 12 dB) times the engine's block peak, held up 2 % a block, so the ring dies as the tail feeding it does. `REACH` sits over the largest resonant peak of a filter that isn't ringing by itself, so a normal release is untouched. The gate closes no faster than 1/16 per block (21 ms), so a ring is faded, not cut, and the existing voice-end fade is left only the remainder.
- **Where it runs.** ALGO and MODAL with no VCA route. A VCA route gates the voice itself (spec § 4); TAPE already ends by its key through the voice fade, after the filter, and ONESHOT ignores note-off, so it needs nothing.
- **MODAL.** The gate takes MODAL's resonator as it is: its ring is the engine's `DAMP` T60, so the filter's ring follows that long tail (about 10 s at INIT) down and frees with it. Nothing damps the resonator, and the gate adds no release time of its own.
- **Existing patches.** A release that was not self-oscillating is bit-identical: all goldens hold. A patch with RES 0.95 or more now dies with its tail instead of ringing on until the engine ends; no patch needs converting.
- **Cost.** Nothing while held. Released, two block scans (3 instructions a sample each) and a ramp (4): about 16 a sample, host-counted, against `CHAIN_COST`'s unspent headroom (30 billed for a measured 19), 5 over it for a voice in release. The voice chain gains 8 B (`AmpGate`, two `f32`s), inside ADR 0101's 2,112 B. Flash grows 2,520 B.

## Alternatives considered
- **Move an amp envelope after the filter.** There is none to move; ALGO's carriers each have their own, and MODAL has no envelope. Pulling them out of the engines would change FM and the resonator.
- **Gate by the engine's level alone** (open until -30 dB, then shut). Left a ring at full level for most of MODAL's tail, and squared every normal release below -30 dB, which re-blessed the ALGO goldens. The cap by ratio leaves normal sounds alone.
- **End the voice by filter output level.** The ring would be cut at a threshold, and MODAL's own tail is the Rings behaviour to keep.
- **Keep ringing like Rings.** Right for MODAL's resonator, wrong for a filter with no input.

## Consequences
- A self-oscillating filter plays as a held tone and releases with its engine.
- A filter made to ring after the chord's release (RES jumped up in a SYMP tail) builds to 4× the halo it follows; `sweep_thorough` allows that case with its reason.
- A user who wants a sustained drone after note-off routes ENV or a held source into VCA; the gate is then out of the way.
- Raising `REACH` lets a resonant release keep more of its peak; lowering it begins to touch a resonant normal release.

## Sources
- #374; ADR 0096 (fixed VCA column, filter TYPEs), 0101 (voice chain), 0060 (DC blocker, chain cost), 0062 (MODAL rings free); spec § 4.
- `chimera-engine/src/dsp/gate.rs`; `a_self_oscillating_filter_dies_with_its_note` in `chimera-core/tests/param_sweep_test.rs`.
