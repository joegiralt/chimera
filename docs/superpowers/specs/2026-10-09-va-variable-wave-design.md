# VA Engine: variable-wave core: Design

- **Status:** Draft, awaiting owner review
- **Date:** 2026-10-09
- **Supersedes:** `2026-09-27-va-engine-design.md` (rev 2, never reviewed; its 11 one-per-voice models become candidates for later macro oscillators, see § Later)
- **Research:** (private notes) (codebase), (private notes) (what makes oscillators alive)
- **Ships in:** alpha (#414)

## Intent

Most digital synths sound sterile. The ones that don't build the movement into the oscillator: a few well-chosen controls that come alive under modulation, and an instability that lets the sound breathe (owner, 2026-10-09, on the Cobalt8 and the OP-XY). VA is Chimera's oscillator engine built on that idea.

Its architecture is a nod to the Mutable Instruments **Ambika**: two variable-waveform oscillators combined by a digital operator, plus a sub-oscillator locked to oscillator 1. We take the architecture from the Ambika manual and nothing else. The algorithms, names and tables are ours. The Ambika's firmware is GPL and is not read for code, and its wavetables are not used (ADR 0114).

**For the alpha** there is one engine with one sub-type, the **variable-wave core**, deliberately basic. It is the first of the macro oscillators (§ Later).

**Done when:**
- `EngineType::Va` plays the core on the desktop and the chip;
- every continuous knob modulates without clicks;
- INIT lands within ±1 dB of `REFERENCE_LUFS`;
- the worst VA voice leaves 6 voices beside the FX bus on rev V and rev Y (ADR 0040);
- the owner approves it by ear.

## Pages

The VA chain has three pages. EDIT moves down and wraps; MINUS/PLUS move along a page (D-pad, ADR on navigation).

**Top page, VA:**

| Cell | Param | Range | Does |
|---|---|---|---|
| WAVE1 | OSC 1 WAVE | 0–1 | Morphs OSC 1: sine → triangle → saw → square |
| WAVE2 | OSC 2 WAVE | 0–1 | The same for OSC 2 |
| MODE | MODE | MIX, SYNC, RING, XOR, BITS | How OSC 1 and OSC 2 combine |
| AMNT | AMOUNT | 0–1 | Depends on MODE (§ Combine) |
| SUB | SUB | 0–1 | Sub-oscillator level |
| STAB | STABILITY | 0–1 | 1 = steady; lower = the oscillators drift and breathe (§ Stability) |

**↓ OSC 1 page:**

| Cell | Param | Range | Does |
|---|---|---|---|
| CRSE | OSC 1 COARSE | −24…+24 semitones | Pitch offset |
| FINE | OSC 1 FINE | −100…+100 cents | Fine tune |
| SHAPE | OSC 1 SHAPE | 0–1 | The wave's own variation (§ Oscillator) |

**↓ OSC 2 page:** the same three controls for OSC 2.

FOLD isn't an operator: the voice already folds after the filter. Each cell gets a focus-band hint (ADR 0086) and a value bar from its `Motion` (ADR 0112 point 8).

## Oscillator

Each oscillator is a band-limited phase oscillator with two controls, WAVE and SHAPE.

**WAVE** morphs continuously through four anchors at 0, ⅓, ⅔ and 1: sine, triangle, saw, square. Between two anchors the output is a linear crossfade of the two neighbours, so at most two waves are evaluated per sample. It never steps (the owner's never-snap rule).

**SHAPE** varies whichever waves are sounding. At 0 every wave is its plain self.

| Wave | SHAPE 0 → 1 |
|---|---|
| Sine | Folds brighter: `sin(g·sin θ)` with g from 1 to 3 |
| Triangle | Skews toward a ramp: the peak moves from ½ to 0.95 of the cycle |
| Saw | Lifts the middle part of the ramp: a second step, whose position and height grow with SHAPE |
| Square | Pulse width 50% → 5% (PWM under modulation) |

**Antialiasing:** polyBLEP at every step (saw, square, saw lift, sync reset), and polyBLAMP at every corner (triangle and skew). These are 2-point kernels: polyBLEP (Välimäki et al., 2007) and polyBLAMP (Esqueda, Välimäki, Bilbao, EUSIPCO 2015). The sine fold is not band-limited; at g ≤ 3 its partials stay below the alias gate up to C6 (tested).

**Pitch:** the note, plus COARSE and FINE, plus STABILITY's drift. The voice's pitch modulation applies to both oscillators. Exponential conversion runs once per block, with a per-sample linear increment ramp.

## Combine

Let `a` be OSC 1 and `b` be OSC 2, both in −1…1, and let `x` be AMOUNT.

| MODE | Output | AMOUNT |
|---|---|---|
| MIX | `(1−x)·a + x·b` | Balance: 0 = OSC 1, 1 = OSC 2, ½ = equal |
| SYNC | as MIX, with OSC 2's phase reset at every OSC 1 wrap (BLEP-corrected) | Balance. Sweep OSC 2 COARSE/FINE for the sync sound |
| RING | `(1−x)·½(a+b) + x·(a·b)` | From an equal mix to pure ring |
| XOR | `(1−x)·½(a+b) + x·xor16(a, b)` | From an equal mix to pure XOR |
| BITS | `crush(½(a+b), x)` | Bit depth 16 → 2 bits as x goes 0 → 1, a log curve |

- `xor16` maps each input to 16-bit signed, XORs the words and maps back.
- XOR and BITS alias by nature; that is their character, and they are exempt from the alias gate (documented in the tests).
- **MODE changes** use the duck: the output ramps to 0 over one block, the mode swaps, and it ramps back over the next. SYNC's reset state is cleared on entry.
- **Sub:** a polyBLEP square one octave below OSC 1, toggled at OSC 1's wraps. It is added after the combine: `y = (c + SUB·s) / (1 + SUB)`, which stays peak-safe.

## Stability

STABILITY is 1 for steady and lower for alive. Its depth follows a curve `d = (1 − STABILITY)²`, so the top of the knob is fine-grained.

- **Drift:** each voice has three slow random walks: one shared, one for OSC 1 and one for OSC 2. Each is filtered noise in the 0.05–0.5 Hz range, updated once per block. OSC 1 takes shared + its own; OSC 2 takes shared + its own. So the pair drifts together and their interval beats slowly. Depth up to ±12 cents shared and ±6 cents each at `d = 1`.
- **Tolerance:** each voice gets a fixed small detune per oscillator, up to ±3 cents at `d = 1`, like a real VCO's trim.
- **High-frequency tracking (owner, 2026-10-09):** a real VCO goes progressively flat as it plays higher. Its core's reset time adds a fixed time τ to every cycle, so `f_eff = f / (1 + f·τ)`, a detune that grows roughly linearly with frequency. Each oscillator gets its own τ, from `d·τ_max` scaled by a seeded factor of 0.6–1.0, with `τ_max` = 4 µs (about −14 cents at C7 = 2093 Hz and −27 cents at 4 kHz at `d = 1`). The two oscillators then pull apart more the higher they play, and their beating speeds up with pitch, as on analog. Applied in the once-per-block pitch conversion, so it costs nothing per sample. The sub follows OSC 1's tracked pitch.
- **Jitter:** a per-cycle pitch wobble up to ±0.5 cent at `d = 1`, applied at each wrap.
- **Phase at note-on:** at STABILITY = 1, both oscillators start at phase 0, for a repeatable attack. Below 1, they free-run from where the voice last left them, like analog oscillators.
- **The sub follows OSC 1 exactly.** In SYNC, the slave's drift moves the sync ratio, so the timbre shimmers.
- **Recallable:** all randomness comes from xorshift seeded by (Part, voice slot), so a saved Sound sounds the same on reload, and STABILITY = 1 renders bit-identically every time.

## Voice and engine

VA plugs in on TAPE's pattern (ADR 0093; paths in `va-refresh.md` § 1):
- `EngineType::Va`, disk code 3, ident "VA";
- `BlockRef::Va`, block code 35, a frozen ident;
- a `SlotKind::Va` arm in `EngineSlot`, with a const-assert that `VaEngine` fits the slot;
- arms in `out_gain`, `cost`, `model_cost` and `playing_cost`, with `engines_test` guarding them;
- the nav map and `VA_BLOCKS` in `block_registry.rs`.

The voice's filter, drive, fold, envelopes, LFOs and modulation matrix are unchanged; VA is only a source. `Sound::init` for VA routes **ENV 1 → VCA** at 127, as TAPE does. With that route deleted, the note ends through the voice fade.

**INIT:**
- WAVE1 = WAVE2 = saw (⅔), SHAPE 0;
- OSC 2 FINE +6 cents;
- MODE MIX, AMOUNT ½;
- SUB 0;
- STABILITY 0.85, so it breathes a little out of the box;
- filter CLEAN LP4.

## Params and Motion (ADR 0112)

| Param | Type | Motion | Mod destination |
|---|---|---|---|
| WAVE1, WAVE2, SHAPE1, SHAPE2, AMOUNT, SUB, STABILITY | `f32` 0–1, step 1/128 | `Continuous(Knobs)` | yes |
| FINE1, FINE2 | `f32` cents | `Continuous(Knobs)` | yes |
| CRSE1, CRSE2 | `i8` semitones | `Stepped(Snap)` | yes (stepped) |
| MODE | `u8`, `VaMode` | `Stepped(Crossfade)` (the duck) | no |

- `VaMode` is `#[repr(u8)]` with fixed discriminants 0–4; an unknown byte reads as MIX (no legacy compat needed, it only has to load safely).
- The Sound record grows by 12 params: check `MAX_SOUND_COST` (ADR 0087) and the ≤ 640 B record limit (ADR 0102).

## Code layout

```
chimera-engine/src/dsp/va/
  mod.rs        VaEngine: render, MODE duck, dispatch
  osc.rs        variable-wave oscillator (WAVE morph, SHAPE per wave)
  blep.rs       polyBLEP / polyBLAMP kernels
  combine.rs    MIX, SYNC, RING, XOR, BITS
  stability.rs  drift walks, tolerance, jitter, seeding
```

- The product layer lives in `chimera-engine` (ADR 0108, opt 2).
- Technique files (`blep.rs`, `osc.rs`, `stability.rs`) are written for the library: rate-independent through a `prepare(sr)` that computes their coefficients, with no product types, and each carrying a `PROVENANCE` const (ADR 0114, all Textbook or Original). When ADR 0114 is accepted they move to `chimera-dsp` unchanged.
- The `chimera-dsp` crate is not created for alpha.
- polyBLEP (2007) and polyBLAMP (2015) are under 20 years old, so each carries a "patent: not checked" line for the plugin checklist.

## Budget and billing

- The voice budget is 7,000 cycles per sample on rev V and 5,833 on rev Y; with the bus, that is 970 / 775 per voice at 6 voices (`va-refresh.md` § 4).
- **Estimate for the worst core voice:**
  - two oscillators mid-morph: 2 × 2 waves, about 160;
  - SYNC: about 30;
  - sub: about 15;
  - stability: about 10;
  - shell: about 30;
  - LP24 carry: 95;
  - `CHAIN_COST` 36 and routing 47.
  - Total about 420, which leaves 8 voices on rev V.
- **Billing:** `VaEngine::cost` = one committed term per MODE, host-counted by the ADR 0056 method, as TAPE did. `playing_cost` covers a MODE change under a held note. Unit bench rows come at ship.
- **Loudness:** `VA_OUT_GAIN` is named like `TAPE_OUT_GAIN`, and set so INIT reaches −11.0 LUFS ±1 dB (ADR 0109) at LEVEL 45/128.

## Testing

- **Alias gate** at C6 for each WAVE anchor and each midpoint, with SHAPE at 0, ½ and 1, in MIX, SYNC and RING (XOR and BITS exempt).
- **Pitch:** each anchor within 1 cent at A4 (same DFT method as the 09-27 draft).
- **No clicks:**
  - every continuous knob swept 0 → 1 over 100 ms, plus every MODE change mid-note;
  - criterion: the largest second difference is at most 1.5× the held renders' (the FX-diet criterion; BITS and XOR exempt).
- **Every knob audible:** `rms(y₀ − y₁) ≥ 0.1·rms(y₀)` for each continuous knob.
- **Stability:**
  - STABILITY = 1 is bit-identical across runs and voices.
  - Below 1, the same Sound on the same Part and slot renders identically after a reload.
  - Drift depth stays inside its bounds.
  - **HF tracking:** at `d = 1`, a C7 note measures between −15 and −8 cents (the seeded 0.6–1.0 factor). At A2 it is within 1 cent of true. At STABILITY = 1 it is exactly 0. The OSC 1–OSC 2 beat rate at C7 exceeds the one at C4.
- **Peak-safe:** |y| ≤ 1 for every MODE, AMOUNT and SUB.
- **Plumbing and sanity:** `engines_test` arms; disk round-trip of all 12 params; the ADR 0011 sanity gate for INIT; loudness of INIT.
- **Library-ready:** each technique file is tested at 44.1, 48, 96 and 192 kHz (tuning and stability).

## Later (not alpha)

- OSC 1 and OSC 2 each become a choice of **macro oscillator**, each with its own pages of two controls.
- The variable-wave oscillator is the first macro oscillator. Candidates for the others come from `va-alive-survey.md` and the 09-27 draft: swarm, formant, sync families, PD, chaos.
- MODE stays as the glue between the two.
- Drift capture of the owner's Kobol, SE-02 and prologue may later tune STABILITY's curves (#425 rig).
- Issues #130–#136 and #219 get re-scoped to that list once this lands.

## ADRs

One new ADR (next free number, ≥ 0118) records:
- the architecture: two oscillators, an operator, a sub and STABILITY, as an open nod to the Ambika;
- the XOR/BITS alias exemption;
- STABILITY's seeding rule;
- the code-home rule (written for `chimera-dsp`, kept in `chimera-engine` until ADR 0114 is accepted).
