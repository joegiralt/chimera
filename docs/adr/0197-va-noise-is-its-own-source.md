# 0197. VA NOISE is its own source

- **Status:** Proposed
- **Deciders:** owner (listening to the per-oscillator prototype, 2026-10-21, and the design), firmware
- **Amends:** [0118](0118-build-va-as-two-variable-wave-oscillators-an-operator-a-sub-and-stability.md) (the voice's sources and its bill)

## Context
VA had no noise. A prototype (branch `va-noise-lab`, kept on origin) put NOISE and COLOR on each oscillator, blended in before MODE, so SYNC, FM, RING and XOR worked on the noise. It was rendered through the full engine and the owner listened to it.

The owner rejected it: "it doesn't sound good in the context of mixes, it doesn't add anything musically ... you don't need noise on both oscillators, that's a waste." It also cost +50 to +130 cycles a voice with both oscillators noisy, needed the MODEs' peak bounds reworked, and its interesting results (SYNC's looped noise) came out of the operators, not out of the noise.

The owner's design: noise is its own source, like SUB, with its own page.

## Decision
1. **One source a voice, beside SUB.** `va::noise::Noise` adds to the operator's block after MODE, before the filter, at a weight `z` along a per-block ramp (the `Ramp` SUB's level uses), times the chain's norm. The MODE duck does not touch it: noise is not MODE's.
2. **Mixed like SUB, level-stable.** The chain's norm becomes `VA_CHAIN_GAIN / √(1 + SUB² + z²)` (`Lanes::norm`), so as LEVEL comes up the oscillators give way and the filter sees about the same level (within 1.7 LU of LEVEL 0 at full, measured).
3. **The page.** A new sub-page under VA, after OSC 1 and OSC 2 and before PIT: **OSC 1 · OSC 2 · NOISE · PIT** (block id 110). A is **KIND**, B is **LEVEL**, C–F empty. LEVEL, not AMOUNT: SUB is presented as a level (its LevelBar glyph), and VA already stores AMOUNT as BLEND's ident.
4. **KIND** (VA param 12, `NOISEKIND`, codes appended: WHITE 0, PINK 1, RED 2, DIGITAL 3; never modulatable):
   - **WHITE**: a uniform draw a sample from an LCG (Numerical Recipes' constants), seeded per voice slot.
   - **PINK**: white through Paul Kellet's three-pole "economy" filter, −3 dB an octave from about 16 Hz.
   - **RED**: white through a one-pole low-pass at 150 Hz, less its own one-pole at 25 Hz: −6 dB an octave above 150 Hz, and no sub-20 Hz wander (the prototype's RED put 2–5 % of its power there, past the DC blocker).
   - **DIGITAL**: a sample-and-hold drawing 16 times each OSC 1 cycle (the prototype's CRUNCH hold clock at a fixed rate). It brightens up the keyboard; the steps are naive, like XOR's and BITS'. Kept on the measure that it tracks the key (centroid 294 Hz on the owner's bass phrase against 823 Hz on INIT's C4 stab, both at full); the owner judges it by ear, and it is the one KIND to cut if it does not earn its place.

   Each KIND is made up to the waves' RMS `R` before LEVEL (`every_kind_at_r`, within 0.5 dB). A new KIND starts its filter from rest.
5. **LEVEL** (VA param 13, `NOISE`, modulatable, `Smoothing::KNOB`, LevelBar glyph): `z = LEVEL² · FULL[KIND]`. Squared, so the lower half is the breath (LEVEL ½ is −12 dB on full). `FULL` = WHITE 0.64, PINK 0.80, RED 0.83, DIGITAL 0.69, set by BS.1770 loudness on a held note, filter open: at full the noise is as loud as INIT VA's two saws (within 0.3 LU) and 4–5 LU under the owner's brighter single oscillator. It never sits over the oscillators. LEVEL is a matrix destination and MIDI-mappable as SUB is (its knob read plan in `knobs.rs`).
6. **Off is off.** At LEVEL 0 at both ends of a block the pass does not run, and `1 + s² + 0·0` is `1 + s²` bit for bit. `va::tests::noise_off_is_bit_identical` pins every MODE's FNV hash recorded on main before NOISE (`0xefdb13e5776a536c`), and `vanoise hash` gives the same SHA-256 for all 44 full-engine renders (11 MODEs × bass and stab × the owner's Sound and INIT VA) before and after. Old Sounds carry no NOISE ids and load at LEVEL 0.
7. **The bill, per Sound.** `tools/loop_count.py` on `va::noise::kern::add` in the release ELF (bank 2), a sample's round plus the entry's setup ÷ 64 (1.7): WHITE 10.6 → **20**, PINK 20.0 → **35**, RED 16.0 → **30**, DIGITAL 18.25 (a draw every sample; 12.0 without) → **35**. The door's call from `render` is a few instructions a block, under 0.1 a sample. `EngineSlot::cost` adds the KIND's bill when LEVEL > 0 or a route of nonzero amount moves LEVEL, as the 2040 is billed by Sound. At LEVEL 0 every Sound keeps its voices. With NOISE on, a MODE loses at most one voice in one room: MIX on rev Y 6 → 5 (any KIND); PHASE on rev Y 5 → 4 with PINK, RED or DIGITAL; MAX and SOFT SYNC on rev V 7 → 6 with PINK, RED or DIGITAL; FM, MAX and SOFT SYNC beside ORBIT 7 → 6; LOOPFM on rev Y 8 → 7.
8. **Memory.** `VaParams` grows 8 B (a `Voice` 6,576 → 6,584 B); `VaEngine` grows by the noise's 28 B and a `Ramp`, still within the MODAL engine's size.
9. **The pass plays from bank 2.** Every KIND's loop is `va::noise::kern` (`bank2.x`, `*2va5noise4kern*`, door-placed), reached through `Bank2Door::noise` (`NoiseDoor`); bank 1 keeps `Noise::add`'s call, the level's lane and norm, the params, the page and the codec. Without `bank2` (the desktop, the bench sets) it is called directly. Closed, NOISE adds nothing (the oscillators play on under its norm) and KIND reads NEEDS UPDATE (`bank2::closed_text`); `va_proxy_test` pins the open door's print to the direct call's. Bank 1 grows 616 B (the params, the page, the call), bank 2 1,600 B.

## Alternatives considered
- **Per-oscillator NOISE and COLOR before MODE (the prototype).** Rejected by the owner by ear in mixes; twice the cost for one musical source; the MODEs' bounds (XOR's words, RING's cap, FM's index) had to carry the noise. The branch stays as kept work.
- **AMOUNT as B's label.** VA's AMOUNT is already BLEND's stored ident, and SUB reads as a level.
- **A linear LEVEL like SUB's.** Half the knob would sit above −6 dB, where noise already dominates; the squared law spends the travel where noise is used.
- **Equal RMS for every KIND at full.** WHITE then read 1–4 LU over the oscillators and RED 1.4 LU under WHITE; the loudness-set `FULL` keeps the KINDs comparable.
- **Additive noise without the norm.** Simpler, but the filter's input (the 2040 and SEM are level-sensitive) would grow with LEVEL; SUB already set the rule.
- **A pitched, looped DIGITAL (restarting at OSC 1's wraps).** A fixed random wave a cycle: a tone, not noise, and a second oscillator in disguise.

## Consequences
- A new page and two params: Sounds grow 10 B at most in the pool (`MAX_SOUND_COST` stays 2,656).
- The noise is deterministic per voice slot: renders and tests repeat.
- Through a low filter (the owner's Sound sits at 255 Hz) the noise is mostly hidden, as any source would be: it shows with the filter open or under an envelope.
- DIGITAL's place depends on the owner's ear.

## Sources
- Prototype and notes: branch `va-noise-lab`; (private notes).
- This change's renders, page and hashes: (private notes) (the A/B method of (private notes): raw levels, pipeline check REV2 against EXPNDR +5.4 / +6.3 dB against the unit's +5.6 / +6.4).
- Paul Kellet, "Filter to make pink noise from white" (music-dsp archive, 1999), economy version: public coefficients.
- ITU-R BS.1770-4 for the loudness (`chimera-utils` `lufs_gated`).
