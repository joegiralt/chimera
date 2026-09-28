# 0036. ENV slots follow the Cascadia's Envelopes A and B; LFO slots are CLASSIC or FUNC

- **Status:** Accepted (2026-09-28)
- **Deciders:** project owner (2026-09-27); the "Defaults chosen" of the spec await the owner's word

## Context
Chimera had one linear amp envelope with a per-sample divide, two envelopes nothing read, and one LFO. The owner chose the Intellijel Cascadia's two envelopes as the model for all three ENV slots.

## Decision
- Provenance: the behaviour and ranges of the Cascadia manual v1.2 (2023-10-15), pp. 28–39 and 82–97; none of its code.
- Every time or rate slider is a position 0..1 on `q = q_min · (q_max/q_min)^p` (one `fast_exp2` per block); a matrix route adds to the position, so it acts in octaves.
- **Envelope A:** AHDSR with HOLD POSITION (OFF, AHDSR, GATE EXT) and SPEED (FAST, MED, SLOW) at the manual's ranges. One fixed RC shape: attack aims at 1.3 and stops at 1; decay and release aim 0.01 past their end, so a full swing takes the slider's time. Per sample it is one multiply-add, with no divide; per block it is closed form. ENV n LEVEL (the peak, `clamp(Σ, 0, 1)` when routed) and ENV n TIME (× `2^(−5·Σ)`) are its destinations, the Cascadia's CTRL SOURCE done in the matrix.
- **Envelope B:** MODE ENV, LFO or BURST. FORM (the Cascadia's TYPE SELECT) is AD, AHR or CYCLE in ENV and BURST, and FREE, SYNC or LFV in LFO, default first. RISE, FALL and SHAPE follow MODE and FORM. SHAPE's curve is `x / (x + (1 − x)·2^(4·(2·SHAPE − 1)))`, linear at the centre. LFV is a clamped random walk with a slew. BURST is pulses under a TILT-shaped burst. SYNC resets at each note-on until #44 gives a clock. ENV n RISE, FALL and SHAPE are its destinations.
- The FORM chosen is kept per MODE, in types: `EnvForm` (ENV and BURST) and `LfoForm`, stored as `env_form`, `lfo_form` and `burst_form`. What runs is `Func { Env(EnvForm), Lfo(LfoForm), Burst(EnvForm) }`, so a MODE/FORM mismatch can't be represented. A value a TYPE, MODE or FORM doesn't use is kept.
- **Rates:** a B slot run per block clamps its rates to block rate ÷ 8 (`block_rate_max(sr)`: 93.75 Hz at 48 kHz, 86.1 Hz at the desktop's 44.1 kHz). The clamp covers RATE, BURST's pulse RATE and the repeat rates of ENV CYCLE (1/(RISE + FALL)) and BURST CYCLE (1/LENGTH); one-shot AD and AHR times are not clamped. Only a slot on the VCA runs per sample, at the full ranges.
- **LFO slots:** CLASSIC is today's LFO, bit for bit, with OFFSET stored but no longer applied. FUNC is Envelope B locked to LFO mode, with DEPTH not applied.
- A TYPE, MODE or FORM change never jumps the level: into A or B ENV the new shape enters at the current level; otherwise the difference glides to 0 over 256 samples (one `Glide` type, shared by ENV and LFO slots). The glided value is clamped to the union of the old and new kinds' ranges, so a negative LFO gliding into a unipolar kind doesn't jump.
- Outputs carry no velocity; VEL reaches the sound as a source, through AMP's VEL and through ENV n LEVEL.
- **Accuracy:** both paths (the per-sample tick and the per-block closed form) are tested against an f64 reference, to 1e-4 absolute, with stage changes within ±1 sample. This supersedes the spec's 1e-6, which f32 cannot meet over a long stage. B's per-sample path is anchored, not accumulated: a position is `x₀ ± k·step`, re-anchored at each turn, wrap and block start, and the phase is a `u32` turn.
- **Pages:** E1–E3 and SPD are BigViz pages on the MOD node, after its home, the matrix: MTX · E1 · E2 · E3 · SPD · L1 · L2 · L3 (the owner's order, 2026-09-28). SPD holds each slot's SPEED and HOLD POSITION, a column per slot, dimmed on a type-B slot. The B page shows the FORM names of the current MODE. LFO mode's PHASE and ENV mode's FALL are one param id, so a MODE switch carries the value across (as the spec has it).

## Alternatives considered
- **A curve control on Envelope A:** the Cascadia has none; A stays divide-free.
- **All slots per sample:** six per-sample modulators per voice would cost voices; only VCA-routed ENV slots tick per sample.
- **Resetting FREE, LFV and BURST phases on note-on** (the Cascadia's default gate behaviour): it would make FREE and SYNC the same until #44.
- **One FORM value for every MODE:** switching MODE would reinterpret the stored FORM; a raw `u8` could hold a mismatch.
- **An accumulated f32 step:** a constant step added every sample rounds the same way each time, and the error passes 1e-4 within a few cycles.
- **Clamping a glide to the new kind's range:** a negative LFO into a unipolar kind would jump by about 1.

## Consequences
- In f32, the two paths can differ by up to 1e-4 and a sample at a stage change; the f64 reference tests hold them there.
- Per-block B rates stop far below the Cascadia's 800 Hz LFO and 1 kHz bursts; audio-rate modulation of other destinations needs per-sample routing, not built.
- `played` grows about 210 B per voice; the voice fits ADR 0040's D2 budget.

## Sources
Intellijel Cascadia manual v1.2 (2023-10-15); `docs/superpowers/specs/2026-09-27-filter-routing-design.md` § 1; #140.
