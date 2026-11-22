# 0325. The sine table folds to a quarter turn; WAVE MORPH is frozen as 0.4 rendered it

- **Status:** Accepted (2026-11-22)
- **Deciders:** owner (2026-11-22: "freeze WAVE MORPH", then apply the fold); firmware (bug batch D3)

## Context
`SINE_LUT` took its Taylor series to x¹¹ over ±π unfolded, so `fast_sin` erred up to 4.6e-4 near half a turn (#352; #441 a duplicate). Folding fixes it, but the factory TAPE source WAVE MORPH is a render of MORPH PAD through the engine, and MORPH PAD's LFO reads the table. The fold moved WAVE MORPH's CRC from the pinned FD87A9B7 to CC0E4EA4, and with it the source reference inside the factory Sound TINZ, which must never change. No other factory source moved.

## Decision
- **The fold.** `SINE_LUT` folds each entry onto ±π/2 (`sin(π − x) = sin x`) before the series. That is exact to f32 there (the x¹³ term is 3.5e-7), so `fast_sin_turns` errs only by its interpolation, under 6e-6. Table data only: no code or size change.
- **WAVE MORPH is frozen.** Its recipe keeps `origin = "render"` (MORPH PAD 48, swept, as before) and adds `frozen = "frozen/WAVE-MORPH.f32"`. That file holds the 96,000 48 kHz samples origin/0.4 (6fb04d0e) rendered, as raw f32 LE. `factory_tapes::audio` reads it back bit for bit instead of rendering. The rest of the build is unchanged, so its CRC stays FD87A9B7 and TINZ stays the owner's file.
- **Pinned by tests.**
  - `wave_morph_is_frozen` holds the CRC.
  - `factory_tinz_is_the_owners` holds TINZ's record.
  - `tinz_plays_as_on_0_4` holds TINZ's two seconds through the Instrument, at 0.4's hash.
  - `manifest_crcs_are_pinned` now lists every source off its pin, not only the first.
- **Where it lives.** The file is an input of the host tool that builds the card's factory pack (`just factory-tapes`, then the card). It is neither firmware nor flash: it costs 384,000 B in the repo and none on the unit.

## Alternatives considered
- **Re-pinning WAVE MORPH and TINZ's source reference:** this changes TINZ.
- **Keeping the unfolded table for the tool's renders:** two tables in the engine, or a feature that leaks into every test build.
- **Freezing the built (16 kHz, packed) source:** smaller, but it skips the build path, and a pipeline change would no longer reach it.

## Consequences
Every other `fast_sin` user plays the more accurate sine: LFO and FUNC sines, DRUM's TONE and METAL kernels, the drawn waves.

Re-recorded goldens (each moved by the table alone; the freeze holds every factory source):
- `codec_compat_test`'s two fixture renders.
- `drum_render_null_test`'s ToneKick, ToneSnare, ToneTom, ToneCymbal, MetalJingle.
- `drum_slice1_vel_test`'s `drum_slice1_kit` and `_lr`.
- `fixed_columns_test`.
- `func_fill_hash_test`'s Envelope B fills.
- `golden_test`'s `modal_lfo_cutoff`, `algo_lfo_cutoff`, `algo_morph_sweep`, `factory_6`, `factory_8`, `factory_10`, `factory_11`, `drum_slice1_kit` and `_lr`.
- `orbit_door_test`'s four songs' audio, and dense's under the master tape.
- `screen_golden_test`'s `flt_fx_fold_post`.
- `algo_door_test`'s cases 9, 11 and 12, and the master-tape 9 and 12.
- `modal_door_test`'s LFO cases.

A future engine change that would move another rendered factory source can freeze it the same way.

## Sources
private issue #352, #441; ADR 0278 (TINZ's sends); `tools/factory-tapes/recipes.toml`; branch bugs-d3-352.
