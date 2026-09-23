# Engine Pivot: Monomachine-style Engines + MI Physical Modeling

**Date:** 2026-09-23
**Status:** Draft, awaiting review
**Amends:** `docs/chimera-synth-design.md` (Synthesis Engines, CPU Budget, Phases 3 and 7)

## Intent

Replace Chimera's planned engine list with cheap, characterful Monomachine-style
engines plus a port of Mutable Instruments Rings/Elements. Only the sound
engines change. Parts, polyphony, the shared drive/filter/wavefolder chain,
LSDJ-style navigation and the dungeon map stay as designed.

The engines are our own implementations of each machine *type*, not faithful
clones of Elektron's firmware. gearmulator-md-mm (which runs Elektron ROMs)
is used only as a listening reference. No Elektron code or ROM data (e.g.
DigiPRO wavetables) is used.

## Engine list

| Engine | Inspired by | Encoders a–f |
|---|---|---|
| SWAVE | MnM SWAVE | shape (saw/pulse/ensemble), detune, voice count, pulse width, sub level, drift |
| FM | MnM FM+ | ratio, index, feedback, mod decay, op2 ratio, op2 index |
| GND | MnM GND | wave (sine/noise), mix, noise color, pitch-env amount, pitch-env decay, fine tune |
| Wavetable | MnM DPRO | table, position, position mod, detune, sub, bit crush |
| SID-style | MnM SID-6581 | wave, pulse width, ring mod, sync, osc2 detune, osc2 level |
| VO | MnM VO-6 | vowel, formant shift, voiced/unvoiced mix, breath, glide, resonance |
| Resonator | MI Rings / Elements | model, structure, brightness, damping, position, exciter |

Parameter lists are first drafts, tuned by ear during implementation.

Dropped from the original design: 4-op TX81Z FM (replaced by the FM engine
above) and Polymod VA (SWAVE + shared filter cover most of it).

Wavetables are generated in code (additive/formula), not loaded from files.

## Architecture

- Engines live in `chimera-core/src/engine/`, one module per engine.
- `const BLOCK: usize = 128;` Engines render one block at a time into
  `&mut [f32; BLOCK]`, mono. Resonator's stereo output is summed to mono for V1.
- Dispatch is an enum, not a trait object:
  `enum Engine { Swave(Swave), Fm(Fm), Gnd(Gnd), Wavetable(Wavetable), Sid(Sid), Vo(Vo), Resonator(Resonator) }`.
  No allocation, fixed voice size, inlined calls, and exhaustive `match`
  flags every site when an engine is added.
- A shared `EngineVoice` trait defines the interface each variant implements
  (`note_on`, `note_off`, `set_params`, `render`, `PARAMS`, `COST`); `Engine`
  forwards to it.
- Engine output feeds the existing shared chain (drive → filter → wavefolder → VCA).

### Heavy engine memory

An enum is as large as its largest variant. Resonator needs large delay
lines, so it borrows buffers from a static pool sized for the maximum number
of Resonator voices instead of owning them inline. Pool slots are handed out
by the voice allocator alongside the voice. The exact pool type is decided
when the port starts; the constraint is: no heap, and `size_of::<Engine>()`
stays small (target ≤ 1 KB).

## Types

- `Hz(f32)`, `MidiNote(u8)` (0..=127, `TryFrom<u8>`), `Velocity(Unit)`.
- `Unit(f32)`: a value in 0..=1, clamped at construction.
- `Curve { Linear, Exponential, Stepped(u8) }`.
- `ParamSpec { name: &'static str, min: f32, max: f32, curve: Curve }`.
- Each engine declares `const PARAMS: [ParamSpec; 6]`. Arity 6 is enforced
  by the array type.
- Encoders deliver `[Unit; 6]`; the engine maps them through its specs.
- `Cost(u32)`: estimated cycles per sample per voice. Each engine declares
  `const COST: Cost`.
- `Budget`: the voice allocator admits a voice only if its `Cost` fits the
  remaining budget.
- `FilterParams::mode: u8` becomes `enum FilterMode { Lp1, Lp2, Lp4, Bp2, Bp4, Hp4, Nt2, Phazor }`.

## Build order

1. Types + `EngineVoice` trait + `Engine` enum, with SWAVE as the first engine.
2. FM
3. GND
4. Wavetable
5. SID-style
6. VO
7. Voice allocator with `Budget`
8. Resonator (Rings/Elements port)

Cheap engines first so the interfaces are proven before the large port.

## Testing

TDD for behavior; types for wiring.

Every engine (shared test suite, run against each `Engine` variant):
- No NaN/Inf output across extreme parameter settings.
- Output stays within ±1.0.
- Silent within a declared time after `note_off`.
- Deterministic: same inputs give identical output.

Per engine:
- SWAVE: FFT peak within ±5 cents of the target pitch; detune widens the energy around the fundamental.
- FM: index 0 gives a pure sine; sidebands at the expected carrier ± n·modulator frequencies.
- GND: sine is pure; noise spectral tilt follows color.
- Wavetable: position sweep is continuous (no step discontinuities between frames).
- SID-style: sync resets osc2 phase; ring mod produces sum/difference tones.
- VO: formant peaks within tolerance of target frequencies per vowel.
- Resonator: golden tests. The original MI C++ is built on desktop as test
  tooling only, renders reference WAVs, and the Rust port must match within
  tolerance.

Performance: an on-hardware cycle-counter benchmark verifies each engine's
render time per block is at or below its declared `COST`.

## Licensing

- MI Rings/Elements firmware code: verify the license in the
  pichenettes/eurorack repo before porting and carry its notice.
- Any reSID-derived code is GPL; the SID-style engine is written from
  scratch to avoid it.
- No Elektron firmware, ROM data or wavetables.

## Design doc updates

When this spec is approved, update `docs/chimera-synth-design.md`:
Synthesis Engines section, CPU Budget table, and Phases 3 and 7 to match the
engine list and build order above.

## Out of scope

- Monomachine sequencer, track model, parameter locks.
- Faithful sound matching against Elektron hardware.
- Resonator stereo output (V1 sums to mono).
