# 0070. WAVE takes a scope traced from its own table (amends 0067)

- **Status:** Proposed
- **Deciders:** project owner (issue #310); firmware

## Context
ADR 0067 gave the operator WAVE choice NONE: the name alone. W1–W8, TRI,
SAW, SQR, P25, P12, TSAW, RSQR and SSAW say nothing to someone who
doesn't already know them.

## Decision
- **A new kind, `FocusGlyph::Wave`**, on the operator WAVE spec: a
  small scope (`theme::WAVE_*`, 64 by 48, flush with the right margin)
  with a FAINT frame, a dotted FAINT centre line and one period of the
  wave traced in ACCENT. Palette colours only.
- **The trace is the wave's own table**, never drawn by hand:
  `glyph::wave_trace` samples `WaveId::table(0)` (mip 0) at the scope's
  width, nearest sample, the last point the table's guard, so one whole
  period, scaled so full scale fills the scope less its padding. A new
  wave gets its glyph for free and it always matches the sound, Gibbs
  ripple included.
- **It is static**: drawn from the set value (`Renderer::set`), no
  animation and no per-frame box.
- **Its Demo page is "Glyph: Wave"** (id 79, short SCP: WAV is the
  WAVES demo), slot a operator A's WAVE.

## Alternatives considered
- **Hand-drawn icons per wave:** drift from the sound, and every new
  wave (the planned formant waves) needs one drawn.
- **Interpolating between samples:** at 55 points over 256 the nearest
  sample is already smooth, and keeps each point an exact table value.

## Consequences
- The `algo_wave` golden is re-recorded: the OSC page focuses WAVE.
- Every wave name must fit before the scope at the focus size (tested).

## Sources
- https://github.com/joegiralt/chimera/issues/310
- ADR 0067; `chimera-core/src/ui/glyph.rs` (`wave_trace`),
  `components::wave_scope`, `dsp/algo/waves.rs`.
