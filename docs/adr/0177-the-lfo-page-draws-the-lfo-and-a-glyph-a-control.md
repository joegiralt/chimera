# 0177. Draw the LFO itself on the LFO page, and give each control a glyph

- **Status:** Proposed
- **Deciders:** owner (the picks, 2026-10-19, "All the LFO glyphs are absolutely positively excellent"; the rulings below), firmware

## Context
The LFO pages (L1–L3) were `CellGrid` with `VizType::None`, so their band drew `viz::live_output`: the audio, not the LFO (#649). RATE, PHASE and DEPTH showed the arc, SYNC a switch, SHAPE, FORM and TYPE nothing. CLASSIC RATE read 0–127 though it is Hz; CLASSIC PHASE did nothing under FREE (ADR 0135: FREE plays the Part's clock) yet read live; FUNC RATE read up to 800 Hz but a per-block slot stops at the block rate ÷ 8 (`block_rate_max`, 93.75 Hz at 48 kHz), so the top 22 % of the knob was dead. The owner picked a scope and a glyph a control from mockups (private notes, outside the repo).

## Decision
- **The scope is one cycle of the LFO** (`VizType::Lfo(slot)`): the set SHAPE, DEPTH and (RETRIG) PHASE under CLASSIC, TILT and PHASE under FUNC; S&H and LFV a fixed eight-step picture. A dot rides it at the newest sounding voice's phase on the active Part: the clock under CLASSIC FREE, the cycle from PHASE under RETRIG, FUNC's φ, S&H and LFV walking the eight steps (wraps counted by the UI, frame to frame). The UI sees the phase at 20 fps, so above ~10 Hz it misses wraps (`Head::next` counts a fall of more than half a cycle) and the S&H and LFV dot aliases across the steps: still the picture, not the true position. With no voice it parks as a ring at the start.
- **The scope is cheap.** Its key is the curve's quantised steps (`Wave::curve_key`), RATE left out. The dot is its own key (`RegionData::Viz::dot`): when only it moves, its old and new 9 × 9 boxes are cleared and drawn whole (the curve's columns under them, then the dot), two short row flushes. Parked, nothing redraws.
- **The audio publishes three words.** The UI names the Part whose LFO page is up (`LfoView::watch`); once a block the audio stores that Part's newest voice's phase in each slot (`Instrument::publish_lfo_view`): a load, a scan of the slots and three relaxed stores, nothing when no LFO page is up. No allocation, no blocking.
- **The glyphs, in the 64 × 48 box at `WAVE_X`, `WAVE_Y`** (`FocusGlyph::Lfo` on every LFO spec but TYPE and OFST; `lfo_glyph::glyph_for` picks by TYPE, FORM and param):
  - RATE (both TYPEs): the wave over a fixed two seconds with a one-second ruler; a comb under four columns a cycle, solid under two. No arc.
  - SHAPE: the shape itself (S&H four of its steps).
  - SYNC and FORM: the key lane, two key presses under the wave: FREE runs through them, RETRIG and SYNC restart at each (RETRIG at PHASE), LFV walks.
  - PHASE (both): the wave slides under a fixed start key and dot, the unshifted cycle ghosted.
  - DEPTH: the scaled wave over its full-scale ghost.
  - TILT: one cycle, the rise lit to the peak's dot, a rise:fall split bar with a mark.
  - DELTA: the walk over five cycles between dotted ±1 rails, each target a dot.
  - SLEW: the slewed walk over its straight ghost (DELTA at least 0.6).
  - TYPE: no glyph (the optional still pose is not built).
- **The glyph rule (ADR 0173's).** A glyph is drawn from the slot's set values, eased (`Renderer::eased_set`), quantised to 254 steps (`lfo_glyph::Q`); its key (`LfoGauge::key`, through `Gauge::outside_key`) holds those steps and no frame term. Nothing redraws at idle, playing or not, and a dirty redraw equals a full one (`tests/lfo_glyph_test.rs`).
- **Long values drop a size** beside a glyph (RETRIG, CLASSIC, SAW 40): Logisoso 20, with the glyph kept, 8 px clear of it, as TAPE's do.
- **Functional core, bank 2's raster.** `chimera_engine::lfo_glyph::draw` holds `render` (a glyph into a `Canvas` of `Ink`s) and `curve` (the scope's columns), pure functions of the quantised `Wave`. With `bank2` it is linked into bank 2 (`*14chimera_engine9lfo_glyph4draw*`) behind `Bank2Door::lfo` (`LfoDoor { render, curve }`); `check-door-placed` checks it. Closed, no glyph is drawn and the scope is a faint middle line; a firmware built without `bank2` (the bench sets: `bench-memory` would overflow FLASH by 8 KB with it) links no raster and draws the same; a host draws in place. Bank 1 keeps the inputs, the keys, the paint and the dot.
- **Rulings.**
  - CLASSIC RATE reads Hz (`Law::LfoHz`) on its own linear curve, 0.01–20.
  - CLASSIC PHASE is dimmed (inert) under FREE (`view::dimmed`), and keeps its glyph unchanged beside its dimmed value.
  - FUNC RATE's range stops at the cap: the LFO slot's RISE spec ends at `LFO_RISE_MAX` (`B_RATE` at 93.75 Hz) and reads `Law::LfoRate` (0.05–93.75 Hz over the knob). The stored value keeps its meaning, so every rate under the cap loads as it was; a save above it loads clamped to it (the codec's spec clamp). Envelope B's own LFO RATE is unchanged. The range is fixed at 48 kHz's cap: at 44.1 kHz a per-block slot stops at 86.1 Hz, so the knob's top ~1 % moves only the readout; the hardware runs at 48 kHz.
  - OFST stays off the page.

## Alternatives considered
- **Scope B (a fixed 2 s window)** and **C (the true output scrolling over 4 s)**: B loses the shape above ~6 Hz; C redraws the whole band every frame while a voice sounds. The owner picked A.
- **The playhead's strips redrawn through `DrawTargetExt::clipped`:** a second monomorph of every band primitive, ~1.3 KB of bank 1; drawing the boxes' columns instead leaves the same pixels.
- **The raster in bank 1:** 7.8 KB; behind the door per the ~1.5 KB rule.
- **A rect flush for the dot's boxes** (the mockup's ~2 KB a frame): the display flushes row bands only; two 9-row bands are ~8.6 KB a frame while a voice sounds. A windowed flush is a HAL change left for later (#684).
- **FUNC RATE rescaled under a new ParamId with a migration:** the same result for old saves (under the cap the same Hz, over it the cap) at the cost of a retired id.
- **CLASSIC PHASE offsetting the Part clock under FREE:** changes what every FREE voice plays; dimming says what it does today.

## Consequences
- **Flash, ORBIT build:** bank 1 +4,996 B (867,236 → 872,232 B; 50,268 → 45,272 B under the SD-flash limit), bank 2 +7,776 B (`.bk2` body 128,192 → 135,968 B). Bank 1's share is the plumbing: the page's wave from its eased params, the dot and its boxes, the audio's publish (its voice scan unrolled), the focus band's glyph handling.
- **Audio:** a load and three stores a block while an LFO page is up; nothing otherwise.
- **SPI:** zero at idle; while a voice sounds, two short row bands a frame as the dot moves.
- **Stack:** the paint holds the 3 KB canvas on the UI's stack for one draw.
- The GLYPH: SWITCH demo page flips THEME's HINTS, since LFO SYNC now draws the LFO's own glyph.
- Factory sources' comments read CLASSIC RATE in Hz; the About text names it in Hz.

## Sources
- #649; the mockup (private notes) and `notes.md`.
- ADR 0135 (the Part's LFO clock), 0146 (number and unit), 0173 (the TAPE glyphs' rule, on tape-streaming), 0137 and 0163 (the door).
- `chimera-engine/src/lfo_glyph/`, `chimera-core/src/ui/lfo_glyph.rs`, `chimera-core/tests/lfo_glyph_test.rs`, `chimera-core/tests/lfo_view_test.rs`, `chimera-engine/tests/lfo_proxy_test.rs`.
