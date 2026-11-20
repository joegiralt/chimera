# 0299. FDBK runs the TX81Z's 0–7 in tenths

- **Status:** Accepted (2026-11-20)
- **Deciders:** owner (#990, on 0.4.0 since 2026-11-15), firmware

## Context
#990: FDBK ran 0–7 in the TX81Z's eight steps (`tx::FEEDBACK_CYCLES`: 0, then 1/32 of a cycle doubling to 2). The step from 0 to 1 is already a clear buzz, so "a little grit" could not be dialled. The ticket left the law to design: a finer table, or FDBK continuous between today's steps, today's steps landing where they were.

What the code had (origin/0.4 a1dd3a9c):
- `AlgoOpParams::feedback: u8`, 0–7; the spec `Int(7)`, stepped by 1, `Motion::Continuous(KNOB)`: the Part's knobs already glide it (ADR 0112) and `tx::feedback_cycles` already gives the swing between two steps, linear from one to the next.
- A Sound stores a param as an `f32` in the spec's units (ADR 0045), so a file holds FDBK as 0.0–7.0.
- `ParamSpec::step` existed, 1.0 on every stepped param; `quantize` rounded a stepped value to a whole number.

## Decision
1. **The unit stays, the resolution grows.** FDBK is still the TX81Z's 0–7; it now moves in tenths: 71 values, shown `0.0` to `7.0` (`ValFmt::Tenths(70)`). A whole number is the TX81Z's step; between two the swing runs straight from one to the next, the kernel's own glide law (`tx::feedback_cycles`, unchanged). Ten new settings sit under what step 1 was, and nine between each pair above.
2. **Typed.** `AlgoOpParams::feedback` is `Fdbk`, a byte of tenths 0..=70 (`Fdbk::tx(n)` for a whole TX81Z step, `Fdbk::tenths(t)`, `Fdbk::steps()` in the spec's units). Factory code, the randomizer and the bill name the unit they mean.
3. **Nothing moves on the card or the wire.** Same `ParamId` (11), same ident (`FDBK`), same range in the file (0.0–7.0), same NRPN line (`N 3–8 11 … Index7`). No migration: a Sound saved before holds a whole number and loads as that step. `Fdbk::steps()` is the `f32` nearest `tenths / 10`, so a text Sound's `"0.9"` stores as written.
4. **A stepped spec may step by less than 1.** `ParamSpec::stepping(0.1)`; `quantize` rounds a stepped value to its step (`round(v / step) × step`: for step 1 the same bits as before); `nrpn::steps` and `nrpn::law` count in steps, so FDBK is `Index7` over 0..=70. Every other stepped param still has step 1 (a test holds it).
5. **The knobs read the byte.** `dsp::knobs` reads FDBK's byte as stored and places it by its tenths (`Kind::Tenths`, the spec's `lo` and `k` scaled at compile time): the audio thread's read is the load and multiply it was, no divide. A landed knob is the stored value (`AlgoLive::from_params`, `Fdbk::steps()`), exact at a whole step.
6. **A tick is a tenth; MIX + a turn steps the whole numbers** (`snap_target`), so the TX81Z's eight settings stay one gesture apart.
7. **NRPN's values change meaning**: FDBK's MSB was 0–7 for the eight steps and is now 0–70 for the tenths (10 × the old value for the same sound). The chart's line is unchanged, its range in `docs/guide/NRPN.md` and `NRPN.json` is regenerated. A mapped CC is unaffected (it sweeps the range by place).

## Null: every old value plays bit for bit
- `Fdbk::tx(n).steps() == n` exactly for 0..=7 (a compile-time assert), and `feedback_cycles(n)` returns `FEEDBACK_CYCLES[n]` itself at a whole step: the kernel gets the same `f32` it got (`every_tenth_of_fdbk_adds_harmonics`, `an_old_sounds_fdbk_loads_and_plays_as_it_did`).
- A 0.3 block (`OP_B`, id 11, 5.0) decodes to `Fdbk::tx(5)` with nothing lost and encodes back to the same bytes.
- Factory: `factory.bin` is unchanged (its test rebuilds it byte for byte), the TOML Sources hold whole steps and build as before, the randomizer still rolls whole steps (a seed rolls the Sound it rolled).
- The audio goldens and the factory render hashes are unchanged (the full check).
- While a FDBK knob is on its way (30 ms after a turn) the glide's rounding may differ in the last bit from 0.3's: it is a glide on the same law, to the same value.

## Sweep
`every_tenth_of_fdbk_adds_harmonics`: one sine operator at 440 Hz, harmonics 2–8 summed, for each tenth from 0 through step 3. Each tenth adds to the one before; the first tenth is heard over none and is well under what step 1 gave. Past 3.8 that sum flattens (it dips by 0.2 % from 3.8 to 3.9): the energy moves above the eighth harmonic, as it did between the TX81Z's own steps.

## Alternatives considered
- **A new param in tenths (0–70) with a migration** (`storage::MIGRATIONS`, the first one): a new `ParamId` renumbers NRPN, drops MIDI-map bindings to the old id, shows 0–70 in the browser editor (it prints the stored range), and needs every factory TOML rewritten. The unit didn't change, so neither should the file.
- **A finer table under step 1 only** (three or four halvings): fewer, uneven steps, and nothing between the steps above, where 4→5 and 5→6 are as coarse.
- **A continuous `f32` knob**: `AlgoOpParams` loses `Eq` and grows (ADR 0295 measured what a grown params struct costs bank 1's `synth`), and NRPN's law would change (`Place14`), a renumber.
- **An exponential law between steps** (constant ratio a tenth): it can't reach 0 from step 1; linear is what the glide already plays.

## Consequences
- The focus band and cells show one decimal; FDBK's level bar is as before.
- The browser editor reads the spec (`cont lin 0 7 0.1 0`) and steps by tenths; its schema fixture and one wheel test follow.
- `quantize` gains a divide and a multiply for stepped params (UI and load side only).
- Bank 1: the format's arm, `quantize`, `Fdbk`'s conversions (measured in ADR 0300's table).

## Sources
- #990 (private issue #990); Elektronauts PreenFM3 thread, post 251.
- Yamaha TX81Z owner's manual (feedback level 0–7), ADR 0023; ADR 0045 (frozen codes), 0112 (glides), 0216 (NRPN).
