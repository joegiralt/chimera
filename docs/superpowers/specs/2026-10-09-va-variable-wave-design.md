# VA Engine: variable-wave core: Design

- **Status:** Approved by the owner 2026-10-09; round-2 review fixes applied after (private notes)
- **Date:** 2026-10-09
- **Supersedes:** `2026-09-27-va-engine-design.md` (rev 2, never reviewed; its 11 one-per-voice models become candidates for later macro oscillators, see § Later)
- **Research:** (private notes) (codebase), (private notes) (what makes oscillators alive), (private notes) (literature check; references in § References)
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

FOLD isn't an operator: the voice already folds after the filter.
- **Value bars** come from each cell's `Motion` (ADR 0112 point 8).
- **No hints:** VA adds no MIX or hold gesture, so under ADR 0086 no cell has a focus-band hint (an owner call, approved with the spec; descriptive cell help would be a new ADR).
- **WAVE's focus glyph** is the plain arc. `FocusGlyph::Wave` draws an ALGO wavetable by index, which would show an unrelated wave. A morph scope drawn from the oscillator is § Later.

## Oscillator

Each oscillator is a band-limited phase oscillator with two controls, WAVE and SHAPE.

**WAVE** morphs continuously through four anchors at 0, ⅓, ⅔ and 1: sine, triangle, saw, square. Between two anchors the output is a linear crossfade of the two neighbours, so at most two waves are evaluated per sample. It never steps (the owner's never-snap rule).

**SHAPE** varies whichever waves are sounding. At 0 every wave is its plain self.

| Wave | SHAPE 0 → 1 |
|---|---|
| Sine | Folds brighter: crossfades to `sin(g·sin θ)`, g from 1 to 3 |
| Triangle | Skews: its rising part grows from ½ to 0.95 of the cycle |
| Saw | Splits its reset into two half-height steps that move a quarter cycle apart (at 1, two saws ¼ cycle apart): the second harmonic thins and the fundamental holds (+0.6 dB). Never an octave-up saw |
| Square | Pulse width 50% → 5% (PWM under modulation), never narrower than one sample |

**Anchor phase.** Every wave's fundamental is `sin 2πφ` at every SHAPE:
- the sine;
- a triangle whose rising part is centred on φ = 0 (trough at −w/2, peak at +w/2);
- a **falling** saw `1 − 2φ`, whose two steps sit about 0 weighted by their heights;
- a pulse high on a window of width w centred on φ = ¼.

A crossfade then never cancels the fundamental. In simulation it holds within 0.01 dB of the lerp of the two anchors' fundamentals, at SHAPE 0, ½ and 1. A rising saw would cancel it by 19.4 dB at the tri→saw midpoint. No paper covers this.

**Level.** Every wave, at every SHAPE, is DC-free and scaled to one RMS, **R = 0.2** (−14 dBFS). The gains are closed forms (the sine fold's is a 17-point table).
- The narrowest pulse then peaks at 0.87, so an oscillator stays within 0.9.
- **Into the chain at full scale:** R is the morph's working level, not the chain's. `render`'s last multiply is `VA_CHAIN_GAIN = 1/(R√3)` (≈ 2.887), so the plain saw reaches the drive at ±1, the "maximum saw" the 2040 is tuned on (ADR 0096; TAPE's sources peak at −0.5 dBFS). Every wave keeps the saw's RMS, so a 5% pulse peaks at 2.5 and the chain saturates it, as a hot pulse would.
- `VA_OUT_GAIN`, at the VCA, sets the loudness (§ Budget).
- Across SHAPE the level holds within 0.01 dB (simulation). Across WAVE, a crossfade of two different waves dips up to 0.6 dB at SHAPE 0 and 1.9 dB at SHAPE 1.
- The pulse loses its DC, so no DC reaches the drive, the filter, the fold or the operators (ADR 0060's DC blocker comes after them).

**Antialiasing:** polyBLEP at every step (saw, square, sync reset), and polyBLAMP at every corner (triangle, sync reset). These are 2-point kernels: polyBLEP (Välimäki & Huovilainen 2007 [3]) and polyBLAMP (Esqueda, Välimäki, Bilbao 2015 [4]). Välimäki et al. 2012 [2] measure the 2-point polyBLEP perceptually alias-free to about 2.3 kHz at 48 kHz (about C7), so the C6 gate has margin. The 4-point B-spline [2] and 4-point polyBLAMP [5][6] (about +20 dB) are the upgrade if the gate moves to C7 or higher (about +2–4 cycles per sample per oscillator).

- **Superposition:** every step or corner whose kernel window covers a sample adds its own correction to it. Several per sample are normal: two pulse edges one sample apart at C8, the saw's two steps. Finding the events is per-sample conditional work, billed per event (§ Budget).
- **Moving thresholds:** SHAPE moves the pulse edges, the saw's steps and the triangle's corners. A threshold that crosses the phase is an edge, whichever way it crosses.
  - Each sample compares the sign of (phase − threshold) with the last sample's.
  - The crossing time comes from both motions: `t = d₁/(d₁ − d₀)` samples ago. The step takes the crossing's sign.
  - A corner's slope jump is the difference of its two branches' time derivatives (phase and threshold motion both).
  - Step and slope are taken at the crossing instant (a lerp from the last sample's).
  - A 1 ms ENV into SHAPE at C1 then stays within 0.01 of the ideal, against 0.18 with forward-only crossings (simulation).
- **The pulse is never narrower than one sample** (`w ≥ inc`, a continuous `max`). At C8 a 5% pulse would be 0.6 samples wide and fold at −41.7 dB. With the floor it folds at −53.6 dB.
- **Sine fold, no ADAA:** the fold's input is the phase ramp, not an arbitrary signal, so ADAA [9–13] buys nothing. By Jacobi–Anger, harmonic n has weight J_n(g). At g = 3 and 48 kHz the worst alias is below −140 dB up to C7, and −42 dB at C8. If C8 matters, cap `g ≤ 3 − 1.5·clamp((f0 − 2.5 kHz)/2 kHz, 0, 1)` (one multiply-add per block).

**Pitch:** the note, plus COARSE and FINE, plus STABILITY's drift. The voice's pitch modulation applies to both oscillators. Exponential conversion runs once per block, with a per-sample linear increment ramp.

## Combine

Let `a` be OSC 1 and `b` be OSC 2, each at RMS R and within its block peak bound `pk_a`, `pk_b` (closed forms). Let `x` be AMOUNT.

| MODE | Output | AMOUNT |
|---|---|---|
| MIX | `(1−x)·a + x·b` | Balance: 0 = OSC 1, 1 = OSC 2, ½ = equal |
| SYNC | as MIX, with OSC 2's phase reset at every OSC 1 wrap (corrected, below) | Balance. Sweep OSC 2 COARSE/FINE for the sync sound |
| RING | `(1−x)·½(a+b) + x·k·a·b`, `k = min(1/R, 0.9/(pk_a·pk_b))` | From an equal mix to pure ring |
| XOR | `(1−x)·½(a+b) + x·p·xor16(a/p, b/p)`, `p = max(pk_a, pk_b)` | From an equal mix to pure XOR |
| BITS | `q·crush(½(a+b)/q, x)`, `q = ½(pk_a + pk_b)`; the crusher clamps its output to ±1 (between AMOUNT anchors the level count L isn't whole, so round(v·L)/L can pass 1; at the anchors the clamp is a no-op) | Bit depth 16 → 2 bits as x goes 0 → 1, a log curve |

- **Operator scaling:**
  - RING's `1/R` keeps the ring term at the oscillators' level, and its cap keeps the engine within 0.9.
  - XOR and BITS see their inputs at unit peak, so their bit patterns use the whole word.
- `xor16` maps each input to 16-bit signed, XORs the words and maps back.
- **XOR and BITS** alias by nature; that is their character, and they are exempt from the alias gate.
  - Near-unison XOR is biased positive (the inputs share sign bits). The DC goes through the drive and the filter to the voice's DC blocker, in ADR 0060's order. By design.
  - BITS sets its depth once a block, so a modulated AMOUNT steps at 750 Hz in BITS. This is exempt from never-snap, like the crush itself.
- **SYNC reset:** at each OSC 1 wrap, `t` samples ago, OSC 2 restarts at phase 0. A sample that holds a reset is split in three parts, in time order:
  1. The slave's own events between the last sample and the reset instant (its natural wrap included) are corrected at `t + (φ⁻ − e)/inc`.
  2. The reset gets one polyBLEP scaled by `y(0) − y(φ⁻)` and one polyBLAMP scaled by `(y′(0) − y′(φ⁻))·inc`. Both are taken on the combined slave output (WAVE crossfade and SHAPE included) at the reset instant. BLEP alone is right only where the slope is equal either side (saw, square); a triangle, a sine (slope jump 2π·inc·(1 − cos φ⁻)), the fold and the skew need the BLAMP.
  3. Events in `(0, t·inc]` after the reset are corrected at `(t·inc − e)/inc`. Phase 0 itself is not an event: the reset is that edge.
  - The sample's SHAPE values hold through a reset sample, so threshold motion is ignored there. The error is O(Δw), once per master cycle.
  - The leftover curvature jump scales as inc² (about 0.02 at C6), under the gate. For a pure-sine slave, La Pastina & D'Angelo's exact residual [1] is the upgrade if the gate fails. About 50 cycles an event.
- **RING's coincident edges:** RING multiplies two corrected oscillators.
  - Where their steps coincide (STAB 1, equal or octave tuning, steep waves), the residuals multiply. They fold at −31…−38 dB at C6, and that falls only 3 dB an octave.
  - Under detune the edges pass through coincidence and back.
  - A per-interval product correction was simulated. It is exact at coincidence but 5 dB worse elsewhere, so it is § Later. The gate tests RING detuned (§ Testing). RING above SHAPE ½ is ungated: detuned at C5 the steep waves at SHAPE 1 fold at −37…−43 dB (pulse, and WAVE ⅚).
- **RING's gain `k` ramps per sample** from the last block's value to this block's, so a SHAPE LFO that moves the cap never steps it. XOR's `p` and BITS' `q` are set once a block, exempt like the crush.
- **XOR of identical oscillators is silent** (same wave, pitch and phase: `a XOR a = 0`), and passing through identical states pops [8]. By design. At STABILITY = 1 both start at phase 0, so with equal WAVE and tuning, XOR at AMOUNT 1 is silent. INIT's +6-cent FINE avoids it; detune under 20 cents is the fat, moving texture [8].
- **MODE changes** use the duck, a raised cosine two blocks each way: the output falls as `g_down(i) = ½(1 + cos(π(i+1)/128))` over two blocks (i = 0…127), the mode swaps, and it rises as `g_up = 1 − g_down` over the next two. A change while it falls retargets it; one while it rises waits for the rise to end. A linear duck clicks at its turnaround (Δ² 6× the click rule's limit at A4). One block each way passes at A4 (91%) but not at A3 (163%); two pass both (73% and 91% in simulation, f32, 8 start phases). SYNC keeps no reset state, so entering it clears nothing.
- **Sub:** a polyBLEP square of ±R (the oscillators' RMS), one octave below OSC 1, toggled at OSC 1's wraps. It is added after the combine: `y = (c + SUB·s)/√(1 + SUB²)`. That is level-stable, and it stays within √(0.9² + R²) = 0.92.

## Stability

STABILITY is 1 for steady and lower for alive. Its depth follows a curve `d = (1 − STABILITY)²`, so the top of the knob is fine-grained.

- **Drift:** each voice has three walks: one shared, one for OSC 1 and one for OSC 2. OSC 1 takes shared + its own, and OSC 2 takes shared + its own, so the pair drifts together and their interval beats slowly.
  - **A walk** is white noise through two cascaded one-pole low-passes at 0.2 Hz, stepped once a block.
  - `prepare(rate)` sets its gain from the filter's closed-form variance, so its RMS is 0.30 at every rate. It is clamped to ±1, which engages under 1% of the time.
  - Rate and level are therefore independent. In simulation, at 689–3,000 Hz update rates, the RMS is 0.28–0.33 and the walk makes 0.18–0.27 zero crossings a second (hysteresis ±0.1).
  - **Depth:** ×12 cents shared and ×6 cents own at `d = 1` (shared RMS 3.6 cents, ±12 at the clamp). The pair's drift correlates at 0.82.
- **Tolerance:** each voice gets a fixed small detune per oscillator, up to ±3 cents at `d = 1`, like a real VCO's trim.
- **High-frequency tracking (owner, 2026-10-09):** a real VCO's core adds a fixed reset or switching delay τ to every cycle [17][18], so `f_eff = f / (1 + f·τ)`. Its exponential converter's bulk resistance has the same first-order effect, cents ∝ f. The result is a detune that grows roughly linearly with frequency.
  - Each oscillator gets a seeded signed `τ = τ_max·d·u`, with u in [−0.25, 1]: mostly flat, sometimes slightly sharp, like an over-compensated VCO.
  - `τ_max` = 4 µs, derived from the CEM3340 datasheet [17] (I/f ≈ 10 nC, r_e 5–20 Ω gives 2–8 µs). It is an untrimmed-VCO **estimate, not a measurement**.
  - At `d = 1`, C7 (2093 Hz) lands between −14.4 and +3.6 cents. `f_eff` clamps so that 1 + f·τ ≥ 0.5.
  - The two oscillators pull apart more the higher they play, and their beating speeds up with pitch, as on analog.
  - It is applied in the once-per-block pitch conversion, so it costs nothing per sample. The sub follows OSC 1's tracked pitch.
- **Scale error:** each oscillator also gets a seeded V/oct error `ε = ±0.2 %·d` (CEM3340 untrimmed typical [17]), applied as `pitch_oct·(1 + ε)` about C4. That is octave stretch: ±2.4 cents per octave at `d = 1` (±7.2 cents at C7, ±3 at A2). About 20 cycles a block per oscillator.
- **Jitter:** a per-cycle pitch wobble up to ±0.5 cent at `d = 1`, applied at each wrap.
- **Phase at note-on:**
  - At STABILITY = 1, a note-on on an **idle** voice starts both oscillators and the sub at phase 0, for a repeatable attack.
  - A re-strike or same-Part steal of a **sounding** voice keeps its phase, so it doesn't click.
  - At STAB 1 the saw and the pulse start at a non-zero value. The VCA's attack covers that; at a 0 ms attack it clicks like a hard-reset VCO. By design.
  - The switch at exactly STAB = 1 is a threshold on a continuous knob while `d` goes to 0 smoothly. The ADR records it.
- **Free-run:** below 1, the oscillators run on from note to note.
  - A voice's VA state survives the idle voice's slot rebuild (`Voice::reset` → `EngineSlot::build`), carried as TAPE carries its strip. That state is both phases, the sub's flip-flop, the walks and the seeded character.
  - So a note starts where the last one left off, and the walks run on and never replay.
  - Nothing advances while the voice is idle. The slot has no clock, and the last note's length already decorrelates the start phase.
  - A slot that played another engine in between starts afresh from its seed, as TAPE's strip does.
- **The sub follows OSC 1 exactly.** In SYNC, the slave's drift moves the sync ratio, so the timbre shimmers.
- **Seeding:** each random draw comes from a splitmix32 hash of (Part, voice slot, purpose salt). A walk's xorshift state starts from that hash, so the first draws are not biased.
- **What recalls:**
  - STABILITY = 1 renders bit-identically.
  - Below 1, each (Part, voice slot)'s character (its trims, τ factors and scale errors) is recalled from its seed.
  - The drift and the free-running phase differ from take to take, by design.

## Voice and engine

VA plugs in on TAPE's pattern (ADR 0093; paths in `va-refresh.md` § 1):
- `EngineType::Va`, disk code 3, ident "VA";
- `BlockRef::Va`, block code 35, a frozen ident;
- a `SlotKind::Va` arm in `EngineSlot`, with a const-assert that `VaEngine` fits the slot, and the carry across `build` (§ Stability);
- arms in `out_gain`, `cost`, `model_cost` and `playing_cost`, with `engines_test` guarding them;
- the nav map and `VA_BLOCKS` in `block_registry.rs`.

The voice's filter, drive, fold, envelopes, LFOs and modulation matrix are unchanged; VA is only a source. `Sound::init` for VA routes **ENV 1 → VCA** at 127, as TAPE does. With that route deleted, the note ends through the voice fade.

**Neutral** (the frozen disk base, ADR 0087):
- WAVE1 = WAVE2 = saw (⅔), SHAPE 0;
- CRSE 0, FINE 0;
- MIX, AMOUNT ½;
- SUB 0;
- STABILITY 1;
- filter **2040** LP4 (VOICE REV2), from `FilterKind::default_for(Va)`, as accepted ADR 0096 rules for VA.

**INIT** (`Sound::init`) is neutral plus three values that are written to disk, so the owner can retune them by ear later without a migration:
- OSC 2 FINE +6 cents;
- STABILITY 0.7 (`d` = 0.09, about ±1 cent of shared drift), so it breathes a little out of the box. 0.85 gives `d` = 0.0225 (±0.27 cents), which is probably inaudible. Confirm by ear at UAT;
- ENV 1 → VCA 127.

## Params and Motion (ADR 0112)

| Param | Type | Motion | Mod destination |
|---|---|---|---|
| WAVE1, WAVE2, SHAPE1, SHAPE2, AMOUNT, SUB, STABILITY | `f32` 0–1, step 1/128 | `Continuous(Knobs)` | yes |
| FINE1, FINE2 | `f32` cents, stepped by 1 (PITCH FINE's precedent) | `Continuous(Knobs)` | yes |
| CRSE1, CRSE2 | `i8` semitones | `Stepped(Snap)` | yes (stepped) |
| MODE | `u8`, `VaMode` | `Stepped(Crossfade)` (the duck) | no |

- `VaMode` is `#[repr(u8)]` with fixed discriminants 0–4; an unknown byte reads as MIX (no legacy compat needed, it only has to load safely).
- **Matrix headers** fit the column: WV1, WV2, AMNT, SUB, STAB, CR1, FN1, SH1, CR2, FN2, SH2.
- The Sound record grows by 12 params: check `MAX_SOUND_COST` (ADR 0087) and the ≤ 640 B record limit (ADR 0102).

## Code layout

```
chimera-engine/src/dsp/va/
  mod.rs        VaEngine: render, MODE duck, dispatch, carry
  osc.rs        variable-wave oscillator (WAVE morph, SHAPE per wave), sync, sub
  blep.rs       polyBLEP / polyBLAMP kernels
  combine.rs    MIX, SYNC, RING, XOR, BITS
  stability.rs  drift walks, tolerance, jitter, seeding
```

- The product layer lives in `chimera-engine` (ADR 0108, opt 2).
- The technique files (`blep.rs`, `osc.rs`, `stability.rs`) are written for the library:
  - each is rate-independent through a `prepare(sr)` that computes its coefficients, and takes no product types;
  - each declares a `PROVENANCE` in ADR 0114's variants (`Textbook { cite }`, `CircuitModel { circuit, refs, check }` for the CEM3340-derived HF tracking and scale error, and `Original { author }`), plus a `PATENT` line per technique under 20 years old;
  - the GPL check is noted as a clean room: the Ambika manual only, with Mutable Instruments' firmware not read.
- When ADR 0114 is accepted, the technique files move to `chimera-dsp` unchanged. The `chimera-dsp` crate is not created for alpha.

## Budget and billing

- **Rooms:** the voices share what ADR 0091's reserve leaves (the bus's 1,180 and a note-on burst's 836; `cost_test`):

| Revision | Room | Per voice at 6 |
|---|---|---|
| rev V | 4,984 | 830 |
| rev Y | 3,817 | 636 |
| rev V, ORBIT playing | 4,813 | 802 |
| rev V, master tape on | 4,674 | 779 |

- **Estimate for the worst voice:** SYNC, both oscillators mid-morph with SHAPE moving, at the highest event rate. It comes to about **500–650 cycles**:
  - two oscillators' fast path, about 120;
  - the exact per-sample reciprocal and square root while SHAPE moves (2 `vdiv` + 1 `vsqrt` per oscillator), about 85;
  - events: up to 4 a cycle per oscillator, which is 1.8 a sample at the 0.45 increment ceiling, at about 25 each: about 90;
  - SYNC resets, about 50;
  - sub, jitter, block work and shell, about 55;
  - LP24's carry, 95;
  - `CHAIN_COST` 36 and routing 47;
  - INIT's 2040 REV2, +15 over LP24.
- **What that leaves:** 6 voices on rev V in every row. On rev Y, 6 voices only if the count lands at or under 636. The count decides; if it doesn't fit, the owner chooses (5 VA voices on rev Y, or a cheaper SYNC).
- **Billing:** `VaEngine::cost` is one committed term per MODE, host-counted by the ADR 0056 method:
  - bill = the per-sample fast path + Σ (each event's cost × its maximum rate at the 0.45 ceiling) + per-block work ÷ 64 + 95;
  - each oscillator's `tick` is `#[inline(always)]` into each mode's loop, so the count sees it;
  - `playing_cost` covers a MODE change under a held note;
  - the unit's bench rows come at ship.
- **Loudness:** `VA_OUT_GAIN` is named like `TAPE_OUT_GAIN`, measured after `VA_CHAIN_GAIN`, and set so INIT reaches −11.0 LUFS ±1 dB (ADR 0109) at LEVEL 45/128.

## Testing

- **Alias gate** (BH4, 2¹⁵ samples, ±4 bins of each series masked, 20 Hz–10 kHz ≤ −45 dB):
  - MIX and SYNC at C6, over each WAVE anchor and midpoint × SHAPE 0, ½ and 1. In simulation the worst is −53.0 dB for MIX and −46.8 dB for SYNC (pulse slave at 2.37×, a thin margin).
  - RING at C5, detuned: FINE2 +6 cents at CRSE2 0 and +12, WAVE2 = WAVE1 or triangle, SHAPE 0 and ½, with the intermodulation lattice masked. In simulation the worst is −49.6 dB.
  - XOR and BITS are exempt. RING's coincident edges and RING above SHAPE ½ are documented exceptions (§ Combine).
  - The band stops at 10 kHz. Above it the 2-point kernel is known weak [2]; the 4-point kernel is the named upgrade.
- **Superposition:** the pulse (at its one-sample floor) and the split saw (SHAPE 1) pass the gate at C7 and C8 (simulation: −52 dB or better).
- **Anchor phase:** across the WAVE sweep at SHAPE 0, ½ and 1, the fundamental stays within 1 dB of the lerp of its two neighbouring anchors' fundamentals, and in phase.
- **Level and DC:**
  - at each anchor, the RMS stays within ±1 dB of R across SHAPE;
  - across WAVE it stays at least −2.5 dB of R;
  - |DC| ≤ 10⁻³·R everywhere;
  - the split saw's fundamental at SHAPE 1 is within ±1 dB of the plain saw's.
- **Pitch:** each anchor within 1 cent at A4 (same DFT method as the 09-27 draft).
- **No clicks:**
  - WAVE and SHAPE swept 0 → 1 over 100 ms on the oscillator track an ideal reference within 2·10⁻³. The reference is the continuous wave through the 2-point kernel's own triangle filter (simulation ≤ 6.3·10⁻⁴), and one edge left uncorrected inside the oscillator fails it.
  - A 1 ms ENV into SHAPE at C1 stays within 0.02.
  - The other knobs and every MODE change are tested on sine-only renders: the largest second difference, from block 2 on, is at most 1.5× the held renders' (+10⁻⁶), and a step of 0.002 fails it. OSC 2 is detuned +6 cents (CRSE2 +7 in SYNC), so AMOUNT mixes two different signals.
  - SUB is tested by reference: the sweep equals `with_sub(c, s, SUB[n])` within 10⁻⁶, with c and s rendered apart and SUB[n] the per-sample ramp, the only click source.
  - BITS and XOR are exempt.
- **Every knob audible:** `rms(y₀ − y₁) ≥ 0.1·rms(y₀)` for each continuous knob, from its minimum to its maximum (FINE −100 vs +100 cents), with WAVE2 0.7 and SYNC at CRSE2 +7, so the two oscillators differ. RING and XOR AMOUNT are also tested at INIT's +6 cents.
- **Stability:**
  - STABILITY = 1 is bit-identical across takes, Parts and slots. A re-strike of a sounding voice keeps its phase.
  - Below 1, a (Part, slot)'s character is equal after a fresh boot. Successive takes on one voice differ, and the second starts off phase 0.
  - The seeds spread: over 8 Parts, the 8 voices' draws have std ≥ 0.25 and are pairwise apart.
  - The walk has RMS 0.25–0.35 and makes 0.12–0.40 crossings a second at every rate, its mean within 10% (RMS) and 20% (crossings) of 48 kHz's, and the clamp engages ≤ 1.5% of the time.
  - **HF tracking:** at `d = 1`, τ alone puts a C7 note between −14.5 and +3.7 cents (the seeded u in [−0.25, 1]). At A2 it is within 1 cent of true. At STABILITY = 1 it is exactly 0. The OSC 1–OSC 2 beat rate at C7 exceeds the one at C4 where their τ differ by more than 0.05.
  - **Scale error:** C4 is exact. At `d = 1`, C7 is within ±7.2 cents and A2 within ±3 cents of true. At STABILITY = 1 it is exactly 0.
- **Peak-safe:** |y| ≤ 0.95·`VA_CHAIN_GAIN` for every MODE, AMOUNT and SUB.
- **Plumbing and sanity:** `engines_test` arms; disk round-trip of all 12 params; the ADR 0011 sanity gate for INIT; the loudness of INIT over whole beats of its +6-cent pair.
- **Library-ready:** each technique file is tested at 44.1, 48, 96 and 192 kHz (tuning and stability).

## Later (not alpha)

- OSC 1 and OSC 2 each become a choice of **macro oscillator**, each with its own pages of two controls.
- The variable-wave oscillator is the first macro oscillator. Candidates for the others come from `va-alive-survey.md` and the 09-27 draft: swarm, formant, sync families, PD, chaos.
- MODE stays as the glue between the two.
- Drift capture of the owner's Kobol, SE-02 and prologue may later tune STABILITY's curves (#425 rig).
- BITS ADAA (Werner & Azelborn 2023 [9]), about 25 cycles, if UAT finds it harsh.
- RING product correction for coincident edges (a GitHub issue).
- A VA morph scope for WAVE's focus glyph, drawn from the oscillator (a GitHub issue).
- Issues #130–#136 and #219 get re-scoped to that list once this lands.

## ADRs

One new ADR (next free number, ≥ 0118) records:
- the architecture: two oscillators, an operator, a sub and STABILITY, as an open nod to the Ambika;
- the XOR/BITS alias exemption, and RING's documented exceptions;
- one RMS for every wave (R = 0.2), the operator scaling, and `VA_CHAIN_GAIN` (the saw at full scale into the chain);
- the neutral fixture maps `kind:` per engine instead of ADR 0096's appended row (same text, no existing line changes);
- STABILITY's seeding and what recalls, the free-run carry, and the STAB = 1 threshold;
- the code-home rule (written for `chimera-dsp`, kept in `chimera-engine` until ADR 0114 is accepted).

VA's 2040 default follows accepted ADR 0096 and needs no new ADR.

## References

"Textbook" below means: licence not stated, so the provenance is Textbook. Every patent note for a technique under 20 years old reads "not checked". A web search found no patent for any of them, but that is not a freedom-to-operate check.

1. **P. P. La Pastina, S. D'Angelo**, "A General Antialiasing Method for Sine Hard Sync", *DAFx20in22*, 2022. https://www.dafx.de/paper-archive/2022/papers/DAFx20in22_paper_3.pdf
   - Licence: CC-BY 4.0. Patent: not checked.
2. **V. Välimäki, J. Pekonen, J. Nam**, "Perceptually informed synthesis of bandlimited classical waveforms using integrated polynomial interpolation", *J. Acoust. Soc. Am.* 131(1):974–986, 2012. DOI 10.1121/1.3651227. https://mac.kaist.ac.kr/pubs/ValimakiPeknenNam-jasa2012.pdf
   - Licence: ASA copyright → Textbook. Patent: not checked.
3. **V. Välimäki, A. Huovilainen**, "Antialiasing oscillators in subtractive synthesis", *IEEE Signal Processing Magazine* 24(2):116–125, 2007. This is the 2-point polyBLEP.
   - Licence: IEEE → Textbook. Patent: not checked. I did not re-read it; it is the canonical reference.
4. **F. Esqueda, V. Välimäki, S. Bilbao**, "Aliasing reduction in soft-clipping algorithms", *EUSIPCO*, 2015. This is the 2-point polyBLAMP.
   - Licence: Textbook. Patent: not checked. I did not re-read it.
5. **F. Esqueda, V. Välimäki, S. Bilbao**, "Rounding Corners with BLAMP", *DAFx-16*, 2016. https://www.dafx.de/paper-archive/2016/dafxpapers/18-DAFx-16_paper_33-PN.pdf
   - Licence: Textbook. Patent: not checked.
6. **C. Hohnerlein, M. Rest, J. D. Parker**, "Efficient Anti-aliasing of a Complex Polygonal Oscillator", *DAFx-17*, 2017. https://www.dafx.de/paper-archive/2017/papers/DAFx17_paper_100.pdf
   - Licence: Textbook. Patent: not checked.
7. **A. Argentieri, F. Scagliola**, "Arbitrary Polygon Oscillator: Generalizing Polygonal Synthesis to Arbitrary Shapes, Morphing, and Three-Dimensional Polyhedra", *DAFx26*, 2026. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_32.pdf
   - Licence: CC-BY 4.0. Patent: not checked.
8. **J. Kleimola**, "Audio Synthesis by Bitwise Logical Modulation", *DAFx-08*, 2008. https://www.dafx.de/paper-archive/2008/papers/dafx08_13.pdf
   - Licence: Textbook. Patent: not checked.
9. **K. J. Werner, E. Azelborn**, "Antialiasing Piecewise Polynomial Waveshapers", *DAFx23*, 2023. https://www.dafx.de/paper-archive/2023/DAFx23_paper_61.pdf
   - Licence: CC-BY 4.0. Patent: not checked.
10. **L. Gabrielli, S. Squartini**, "Simplifying Antiderivative Antialiasing with Lookup Table Integration", *DAFx25*, 2025. https://www.dafx.de/paper-archive/2025/DAFx25_paper_30.pdf
    - Licence: CC-BY 4.0. Patent: not checked.
11. **L. Gabrielli, S. Squartini**, "PolyADAA: Improving Aliasing Reduction in Memoryless Nonlinearities Using Lagrange Interpolation and Polynomial Approximation", *DAFx26*, 2026. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_47.pdf
    - Licence: CC-BY 4.0. Patent: not checked.
12. **M. Holters**, "Combined Derivative/Antiderivative Antialiasing", *DAFx20in22*, 2022. https://www.dafx.de/paper-archive/2022/papers/DAFx20in22_paper_7.pdf
    - Licence: CC-BY 4.0. Patent: not checked.
13. **S. Bilbao, F. Esqueda, J. D. Parker, V. Välimäki**, "Antiderivative Antialiasing for Memoryless Nonlinearities", *IEEE Signal Processing Letters* 24(7):1049–1053, 2017.
    - Licence: IEEE → Textbook. Patent: not checked. Canonical; I did not re-read it.
14. **L. Gabrielli, S. D'Angelo, L. Turchet**, "Analysis and Emulation of Early Digitally-Controlled Oscillators Based on the Walsh-Hadamard Transform", *DAFx-19*, 2019. https://www.dafx.de/paper-archive/2019/DAFx2019_paper_13.pdf
    - Licence: Textbook. Patent: not checked.
15. **R. Simionato, S. Fasciani**, "Towards Neural Emulation of Voltage-Controlled Oscillators", *DAFx25*, 2025. https://www.dafx.de/paper-archive/2025/DAFx25_paper_33.pdf
    - Licence: CC-BY 4.0. It reports no drift statistics.
16. **J. Roth, D. Keller, O. Castañeda, C. Studer**, "Alias-Free Oscillator Synchronization via Additive Synthesis", *DAFx26*, 2026. https://www.dafx.de/paper-archive/2026/papers/DAFx26_paper_49.pdf
    - Licence: CC-BY 4.0. Useful only as an offline ground truth (`va-refresh.md`).
17. **Curtis Electromusic Specialties**, *CEM3340/3345 Voltage Controlled Oscillator* datasheet, c. 1980. https://static.miraheze.org/sdiywikiwiki/1/1c/CEM3340-3345_long.pdf
    - A manufacturer's measurement data, used here for numbers only.
18. **Sound Semiconductor**, *SSI2130 FATKEYS VCO* datasheet. https://www.soundsemiconductor.com/downloads/ssi2130datasheet.pdf
    - Used for numbers only.
