# 0042. Voice pitch is a matrix destination on each engine's PITCH page

- **Status:** Accepted (2026-09-28)
- **Deciders:** owner (#162, 2026-09-28), firmware

## Context
No matrix route could reach a voice's pitch, so vibrato and pitch-envelope
patches were impossible. Algo had a TRANSPOSE of its own; Modal had none. A
pitch that belongs to one engine would lose its routes on an engine switch.

## Decision
- **One block for the voice.** `BlockRef::Pitch` (`PitchParams`) lives in
  the Sound's `ParamSnapshot`, beside the engine blocks, not in any of them.
  The same route reaches it from every engine and survives an engine switch.
- **Two cells.** PITCH is a transpose, −24..=+24 semitones in steps of one.
  FINE is −100..=+100 cents. Both are Stepped: turning the encoder moves them
  a whole step, while a modulated copy stays fractional. Both are
  modulatable, and MIX+PLUS on the cell primes it.
- **The offset law.** An amount of ±127 moves PITCH ±24 semitones and FINE
  ±100 cents, linear in the unit (`OffsetLaw::Semitones(24)`,
  `OffsetLaw::Cents(100)`), clamped to the stored range. The MTX readout
  states the effect in the unit (`+32 = +6.0 st`, `+32 = +25 ct`). The block's
  full name there is `VOICE`, so the line reads `VOICE PITCH`, not
  `PITCH PITCH`; its tag is `PIT`.
- **A departure from ADR 0010's linear law.** ADR 0010's offset is
  `v + off · (max − min)`, which would give ±127 the whole span: ±48
  semitones on PITCH and ±200 cents on FINE. `Semitones` and `Cents` give it
  half the span, the stored range's reach either side of 0, so a full amount
  lands on the range's end from the centre.
- **The DSP.** After the matrix pass, each block, the voice's offset is
  `pitch + fine / 100` semitones. Algo adds it to the note's exponent where
  the ratio-1 phase increment is computed, so every operator scales by
  2^(st/12), stepped per block as the note already is. Modal scales its
  resonator bank's frequency every block, and retunes its strings (main and
  sympathetic) when the ratio changes. At exactly 0 no extra maths runs, so
  every audio golden stays bit-identical.
- **The page.** PIT (`PITCH`) is a sub-page of each engine's home node: ALG
  on Algo, MDL on Modal (after MDL2), and VA's when that engine lands. A is
  PITCH, B is FINE. C–F stay empty for GLIDE (portamento rate) and pitch
  SLEW, which are out of scope here.
- **Cost.** The route's sum and offset are billed as any destination's,
  through the DEST terms. The rest is per block. Algo's increments are
  already computed per block. Modal's ratio is one `fast_exp2` a block at a
  nonzero offset, and its SVF bank already recomputes per block. Its strings
  retune only when the ratio changes, but under a moving route that is every
  block: a divide for the main string and one for each of the seven
  sympathetic ones, whose ratios to it are computed once at note-on. A
  Modal Sound with any route into PITCH or FINE bills `ModalEngine::PITCH`
  (12 cycles/sample) on top. That figure is provisional, from the emulator
  (3 instructions a sample and 9 I-cache misses a block for an LFO on PITCH
  on Sympathetic), pending a bench row (#182).

## Alternatives considered
- Route Algo's TRANSPOSE: it is Algo's alone, so a route to it would not
  reach Modal or survive an engine switch. Its range is also 48 semitones,
  which the linear law would give to ±127.
- One cent-resolution PITCH cell: coarse transposes would take 2400 ticks.
- A per-sample pitch ramp: smoother vibrato, but it costs a term per sample,
  and the note itself steps per block today.

## Consequences
Vibrato and pitch envelopes are one route away on every engine. Modal's
strings have integer delay lengths, so on them a routed pitch moves in
one-sample steps of the period. Vibrato there is coarse at high notes, and
fixing that needs fractional delay reads (#163). Algo's pitch steps per
block, as the note does (#164). The PIT page has four free cells for GLIDE
and SLEW.

The sympathetic ratios are fixed at note-on, so an INHARM edit mid-note
does not retune a sounding Sympathetic voice. That matches main, where
Modal's settings are read at note-on and none is modulatable; only this
branch's first cut retuned them, and only on a pitch change.

Measuring the retune showed Modal's flat `ModalEngine::COST` (390, benched
on String) undercounted the other models several times over (#49). Modal
is now billed per model (`ModalEngine::cost`): String 390 as benched;
Bowed 620, Sympathetic 1,400 and the resonator bank 460 + 45 a mode (1,900
at its 32) provisional, from the emulator scaled to String's bench reading
and rounded up about 10 %, until the bench's MDL rows (ROUTING 3/3) read
them. Beside the whole FX bus that gives, on rev V and rev Y: String 8
and 8 voices, Bowed 8 and 6, Sympathetic 3 and 3, the bank 2 and 2. No
factory Sound is Modal, so their counts are unchanged. A sounding voice
keeps its note-on model until its next note, so after a MODE edit it is
billed at the costlier of that model and the stored one (#183).

RAM, measured with `memory_budget_test` after this change: `PitchParams`
adds 8 B to `ParamSnapshot`; the engines' pitch state and Modal's seven
sympathetic ratios take `Voice` to 35,248 B. `[Voice; 8]` is 281,984 B of
D2's 286,720, and `Instrument` 284,640 B on the host, so about 1.7 KB is
left on the firmware (+376 B there, per ADR 0040). AXI is 433,348 of
524,288 B. These replace ADR 0040's figures of 35,200 B and 284,256 B.

## Sources
- Issue #162 and the owner's comment on it; #163 (Modal's whole-sample
  string steps) and #164 (Algo's per-block steps).
- `chimera-core/src/params.rs` (`PitchParams`, `PITCH_SPECS`),
  `chimera-core/src/block.rs` (`OffsetLaw`),
  `chimera-core/src/dsp/algo/engine.rs` (`cycles`),
  `chimera-core/src/dsp/modal/mod.rs` (`set_pitch`, `retune`).
- Tests: `chimera-core/tests/pitch_test.rs`, `pitch_page_test.rs`.
