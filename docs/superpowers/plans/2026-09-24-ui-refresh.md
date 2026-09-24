# UI Refresh (Direction A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redraw every Chimera page in Direction A (refined Elektron): u8g2 type hierarchy, one accent, no outline boxes, a focus band for the last-touched parameter, live output as a filled waveform, a node-line map. The page anatomy, navigation and parameter bindings stay as they are.

**Architecture:** `ui/theme.rs` becomes the only source of colours, fonts and layout constants. New modules hold the Direction A pieces: `ui/draw.rs` (primitives: u8g2 text, bars, arc gauge, pills), `ui/components.rs` (header, focus band, cells), `ui/viz.rs` (every visualization), `ui/browser.rs` (sound browser), `ui/focus.rs` (last-touched slot per page). The renderer draws from one `Frame` struct, and a full render is simply every dirty region drawn in turn, so full and dirty rendering cannot drift apart. Screen goldens (a hash per page type, rendered into an in-memory 240×320 framebuffer) land in Task 1 and lock each page type as it is converted.

**Tech Stack:** Rust 2024, `no_std` `chimera-core`, embedded-graphics 0.8, `u8g2-fonts` 0.8.0 (new, pinned `=0.8.0`), cargo integration tests, `just check`.

**Spec:** `docs/superpowers/specs/2026-09-24-ui-refresh-design.md` (ADR 0016 `docs/adr/0016-visual-direction-refined-elektron.md`; ADR 0013 hardware parity). Mockups: https://claude.ai/artifact/B1ydzBo2GpjTY1QjH5hdvS (drawing code `drawA`, `aHeader`, `aFocus`, `aCells`, `aMap`, `drawFilter`, `drawMixer`, `drawMatrix`, `drawBrowser`).

## Global Constraints

- No `unsafe` without a `// SAFETY:` comment; this plan adds no `unsafe`.
- No heap allocation in the audio callback; the audio thread never blocks or allocates (the UI only reads the scope buffer it already read).
- No libc. `chimera-core` stays `no_std` (`u8g2-fonts` is `no_std`, depends only on `embedded-graphics-core` 0.4).
- All parameter changes lerped in the UI, never snap: every drawn value comes from `Renderer::anim` (`AnimatedValue`).
- "one accent (cyan `#7fd4c8`-ish in RGB565) used only for the active element: focus arc, active cell, map pill, selected row/route" → `theme::ACCENT = Rgb565::new(15, 53, 25)`.
- "No outline boxes. Separation by spacing. Values as thin bars / arcs."
- "Numerals: Logisoso (large for the focus value; medium for readouts)." "No `FONT_6X10` left in page rendering."
- "Values that are choices display as text (POLY, P1, CH 1, pan L/C/R, mode names); numbers as numbers — reuse `ValFmt`/`fmt.rs`."
- "Theme tokens: `theme.rs` becomes the single source of colours, font handles and layout constants for Direction A; no colour literals in the renderer."
- "Fonts: add `u8g2-fonts` (MIT/Apache) to `chimera-core`. Only fonts used are linked. Record each font's licence" (ADR 0016 addendum, decision text unchanged).
- "Flash budget (ADR 0013 parity): firmware must still link; … Target: fonts ≤ 16 KB."
- "RAM: no new large statics; AXI headroom (~13 KB) must not shrink by more than a few hundred bytes (renderer state only)."
- "Redraw regions: keep the dirty-region system; add a `Focus` region; the scope strip region goes away … Changes only redraw their region (SPI bandwidth)."
- "Focus tracking: `UiState` tracks the last-touched slot per page key; default slot 0. The Mixer PART and matrix pages use the same mechanism."
- Bugs found go to GitHub issues; decisions go to ADRs (this plan only appends the font/flash addendum to ADR 0016).
- Every commit passes `just check` (in this environment: `PKG_CONFIG_PATH=/tmp/claude-1000/-home-carcosa-dev-chimera/05a415fc-f8d7-4033-943f-e96e5bd74fff/scratchpad/pkgconfig just check`). Baseline: core+hal 438 passed, 0 failed, 2 ignored; desktop 2 passed; firmware links.
- Commit messages: conventional, ending with a blank line and `Co-Authored-By: <model> <noreply@anthropic.com>`.

## Review Focus

1. **Text longer than `FmtBuf`'s 32 bytes is cut silently, and text near an edge runs off screen** (e.g. a browser footer or matrix stats line) — every screen must draw no pixel outside 240×320 (Task 1 `no_screen_draws_outside_240x320`, added in Task 6) and every page header must fit left of the dot (Task 5 `every_header_fits`).
2. **A page whose focused slot is empty** (System UPDATES, Tuning slots c–f, FM ALG slot b) — the focus band shows nothing rather than `--` or a panic (Task 6 `an_all_empty_page_has_an_empty_focus_band`).
3. **Garbage in the scope buffer** (NaN, ±inf from a blown-up filter) — the live output draws flat and stays in its band (Task 6 `non_finite_live_output_is_flat`).
4. **More matrix destinations than columns** (up to 16) — the grid scrolls, the cursor outline stays visible, nothing draws off screen (Task 11 `a_wide_matrix_scrolls_with_the_cursor`); an empty matrix says so (Task 11 `an_empty_matrix_says_so`).
5. **Maps with one node or many sub-pages** (a single-node chain divides by zero; FM MOD has five sub-pages) — `node_x(0, 1)` is centred and the branch list never leaves the map band on any chain (Task 5 `nodes_spread_over_the_line_and_a_single_node_is_centred`, `the_map_draws_only_in_its_band_on_every_chain`).

---

## Decisions this plan fixes (spec says "the plan resolves")

### Fonts (all `_tr` = ASCII 32–127; measured in a scratch build)

| Token | u8g2 font | Ascent | Data | Use (mockup size) |
|---|---|---|---|---|
| `FONT_FOCUS` | `u8g2_font_logisoso42_tr` | 42 px | 4 625 B | focus value (Oswald 56–64 px, cap ≈ 44) |
| `FONT_READOUT` | `u8g2_font_logisoso20_tr` | 21 px | 2 226 B | value riding on the filter viz (Oswald 26 px) |
| `FONT_VALUE` | `u8g2_font_helvB10_tr` | 11 px | 1 333 B | cell values, focus label, browser names (Oswald 12–15 px) |
| `FONT_LABEL` | `u8g2_font_helvR08_tr` | 8 px | 1 041 B | uppercase labels, tracking +1 px (Oswald 9–11 px) |
| `FONT_LABEL_BOLD` | `u8g2_font_helvB08_tr` | 8 px | 1 028 B | header name, map pill, selected numbers |

Font data 10 253 B ≤ 16 KB. `_tr` (not numeral-only `_tn`) is needed for the focus value because choices show as text (`POLY`, `P1`, `L32`). All renderers are built with `.with_ignore_unknown_chars(true)`: a glyph outside ASCII (`→`, `·`, `—`) is skipped, never an error; the arrow and dash are drawn as primitives instead.

**Measured flash** (release, `llvm-size -A target/thumbv7em-none-eabihf/release/chimera-stm32`): before `.text` 179 896 + `.rodata` 35 420 = 215 316 B; after Task 13 `.text` 171 536 + `.rodata` 47 096 = 218 632 B → **+3 316 B** (`.rodata` +11 676 incl. fonts, `.text` −8 360: the 6×10 mono font, cell icons and pre-refresh vizzes are gone). **RAM:** `.data` 40 696 and `.bss` 156 844 unchanged; `UiState` 33 264 → 33 312 B (+48 B, per-page focus table) on `main`'s stack, i.e. AXI headroom 12 848 B is unchanged by statics and the stack grows by 48 B. Transient render stack: one `[f32; 240]` scope copy (as before) plus a 217-byte column array.

### Layout constants (all in `theme.rs`)

| Band | y | Contents |
|---|---|---|
| Header | 0–28 | context label x 12 baseline 20 (`FONT_LABEL`, `MID`), name +7 px (`FONT_LABEL_BOLD`, `INK`), audio load right-aligned at 217, accent dot (225, 16) r 3 when sounding |
| Focus (CellGrid, Matrix) | 28–118 | label baseline 50 (`FONT_VALUE`, `MID`), value x 10 baseline 104 (`FONT_FOCUS`, `INK`), 270° arc gauge centre (188, 80) r 28 width 5 |
| Viz band (CellGrid) | 118–186 | centre line 152, amplitude ±24, x 12–228 |
| BigViz | 28–186 | plot 40–170; filter pass band y 72 |
| Cells | 186–266 | 3×2, x 12 + col·74, label baseline 196 + row·36, value +17, 2-px bar +22 (62 wide), mod line +26 |
| Map | 266–320 | line y 286 x 24–216, pill 34×20, rings r 4, labels baseline 306, sub-page rows from y 300 every 10 px |

### Screen goldens and the lock discipline

`chimera-core/tests/screen/mod.rs` holds `Fb`, an in-memory 240×320 RGB565 display implementing `DrawTarget` + `ChimeraDisplay` (so `render_dirty` can be tested) that also counts off-screen writes; the named cases (`CASES`, each a real input sequence on a fresh `UiState` followed by 120 `update()`s so lerps settle); and a fixed triangle `scope_fixture()`. `UiState::render_with_scope` / `render_dirty_with_scope` take the scope buffer as an argument, so goldens never read the global scope. `screen_golden_test.rs` hashes each case (FNV-1a 64 over the pixels).

Until Task 13 each golden is `Pending` (may change) or `Locked(hash)` (must match). Rules for every task:
- A task converts a page type, then locks that page type's cases (`SCREEN_RECORD=1 … -- --nocapture`, paste only those rows). The expected hash from the validation run is written in the task; if yours differs, dump the screen (`SCREEN_DUMP=dir`), compare with the mockup, and investigate before recording.
- A task must not change a `Locked` hash. `screen_goldens_match` fails if it does; the fix is in the code, not the table. Shared components (theme, primitives, header, map, cells, focus band) are all finished (Tasks 2–6) before the first lock, so later page conversions cannot move a locked page.
- Task 13 removes `Pending`; from then on the audio-golden rule applies: re-record only in the commit that deliberately changes that screen.
- `SCREEN_DUMP=/some/dir` writes `<case>.ppm` for every case (view with `magick x.ppm -scale 200% x.png`).

### Focus tracking

`ui::focus::FocusMemory` is a `[u8; 48]` indexed by `BlockDef::id` (ids are 0..=40, test-checked). `UiState::handle_input` calls `touch(def.id, i)` on the first non-zero delta of encoder `i` on any page, the matrix included; `UiState::focused_slot()` reads it for the current page. It replaces `UiState::last_encoder` and `Renderer::focused`, so MIX + Plus/Minus prime the slot the focus band shows. No timer. Keyed by def id (not `PageKey`) so FILTER keeps one focus across chains and an FM operator change does not reset it.

### Live scope in the viz band

`UiState::render`/`render_dirty` copy the scope front buffer once (`scope::read_samples`, as the old strip did) and pass it in `Frame::scope`. `viz::live_columns` auto-scales the first 217 samples to ±24 px (flat when `scope::peak` ≤ `SOUNDING_PEAK` = 1e-3; NaN/inf fall to 0 by Rust's saturating casts); `viz::live_key` fingerprints those columns, so the viz region redraws only when the drawn waveform changes (a silent or frozen scope sends nothing over SPI — the old strip redrew 26 rows every frame). The header's accent dot is `scope::peak > SOUNDING_PEAK`.

### What is deleted

`ui/cell.rs` (outline-box cells and every cell icon, `fold_wave`), `CellIcon` and `ParamSlot::icon`, `Renderer::draw_scope` (scope strip) and its per-frame redraw in `render_dirty`, `Renderer::draw_params_from_def`, `draw_cell_grid_from_def`, `draw_viz_from_type` and the pre-refresh vizzes (modal, VA, drive, folder, filter, envelopes, FX boxes, mixer bars, routing, compressor), `draw_header_with_def`, `draw_perf`, `draw_sound_browser`, `RegionKind::Params`/`RegionData::Params`, all `FONT_6X10`/`FONT_4X6` uses, and every theme constant except the Direction A tokens. **Survivors:** none — no mono font remains in `chimera-core`.

### Spec ambiguities resolved

1. **Header "accent dot when the part is sounding"**: the UI has no per-Part voice activity, only the mixed scope; the dot shows when the instrument's live output is non-silent.
2. **Perf overlay**: the `NNNus` render time is dropped (always `0us` on hardware); audio load shows as `CPU n%` in warn/alert colours only when measured.
3. **Header text**: Part chain `PART n` + page name; Mixer chain `MIXER` + page name, numbered when the page edits that Part (`PART 2`, `SENDS 2`; shared FX unnumbered). Names are upper-cased `BlockDef::name` (so `4OPFM`).
4. **Engine pages vs pages with their own viz type**: every CellGrid page shows live output except `MixerLevels` (PART: six-part overview), `EffectsFlow` (Chorus/Delay/Reverb and SENDS: the flow diagram restyled as a node line; SENDS gets `viz: EffectsFlow` and shows each send level) and `AlgorithmDiagram` (4opFM and Operator; Operator gets `viz: AlgorithmDiagram`). The old CellGrid engine vizzes (modal peaks, drive clip, wave fold) were never drawn on CellGrid pages and are deleted.
5. **"FM algorithm/operator pages" listed as BigViz**: they are CellGrid in the registry and stay CellGrid (layouts unchanged); the algorithm diagram (formerly a cell icon, now a table in `viz.rs`) sits in their viz band with the selected operator lit. The "FM Op" BigViz golden is Op1 Env.
6. **Touched value riding on the viz**: filter → marker at the cutoff plus the focused slot's label and value (flips left near the right edge); envelopes → the segment the focused slot edits is lit (ADSR slot a–d → segment 1–4; FM AR→1, D1R/D1L→2, D2R→3, RR→4; others none). Other BigViz pages (Master compressor, EQ, About) draw their viz or nothing.
7. **Values show `ValFmt` output**, not the mockup's physical units (`1.2k`, `0.80`): units are a later pass. New `ValFmt::Pan` shows `L64…C…R63` for Part and Out pan; the FM operator selector shows 1–4 (`OneBased(3)`) instead of 0–3.
8. **Matrix column headers**: two lines, block tag (`FLT`, `OP1`) over the spec label (`CUTOFF`); the mockup's one line cannot tell `OP1 LEVEL` from `OP2 LEVEL`. Amount lerps through display slot e.
9. **Browser order**: pool slots first, then the three INIT rows (as today; the mockup put INIT first). 8 visible rows instead of 10.
10. **Choices have no bar** (`ValFmt::is_discrete`: `Int`, `OneBased`, `Names`), matching the mockup's CH/MODE/OUT cells.

### Things in the spec that do not hold

- "BigViz (Filter, envelopes, FM algorithm/operator pages)" — FM algorithm/operator pages are CellGrid (see 5).
- "the existing visualizations keep their shapes, restyled" — on CellGrid pages the only existing visuals were the cell icons, which the spec's cells (label, value, bar) have no room for; they are removed. Filter, envelope, compressor, FX flow and the FM algorithm keep their shapes.
- Mockup values (`1.2k`, `0.80`, `+0.40`) are not what `ValFmt` produces; kept as `ValFmt` per the spec's own rule.
- "The tiny 4×6 may remain" — not needed; nothing remains.
- Logisoso is a light condensed face; the mockup's Oswald 700 is heavier. No bold Logisoso exists in u8g2; accepted.
- `just clippy` already fails on the base branch (5 pre-existing errors such as `approx_constant`); this plan adds no clippy findings in files it touches but does not fix the pre-existing ones.

---

## File Structure

| Action | File | Responsibility |
|---|---|---|
| Modify | `chimera-core/Cargo.toml` | add `u8g2-fonts = "=0.8.0"` |
| Rewrite | `chimera-core/src/ui/theme.rs` | palette, fonts, layout constants |
| Create | `chimera-core/src/ui/draw.rs` | primitives: text (tracked/right/centre), rect, line, dot, ring, pill, outline, bar, arc gauge, arrow |
| Create | `chimera-core/src/ui/components.rs` | header text + header, overlay title, focus band (param and route), cell |
| Create | `chimera-core/src/ui/viz.rs` | live output, filter, envelope, compressor, six-part overview, FX flow, FM algorithm |
| Create | `chimera-core/src/ui/focus.rs` | `FocusMemory` |
| Create | `chimera-core/src/ui/browser.rs` | sound browser overlay |
| Rewrite | `chimera-core/src/ui/dungeon_map.rs` | node-line map |
| Modify | `chimera-core/src/ui/renderer.rs` | `Frame`, region dispatch; legacy drawing removed |
| Modify | `chimera-core/src/ui/region.rs` | layout tables, `Focus`/`Route` data, `Params` removed |
| Modify | `chimera-core/src/ui/mod.rs` | focus memory, `render_with_scope`, `render_dirty_with_scope`, `region_data`, matrix display value |
| Modify | `chimera-core/src/ui/mod_grid.rs` | dot grid |
| Modify | `chimera-core/src/ui/block_def.rs`, `page.rs`, `block_registry.rs` | `CellIcon` removed; SENDS/OP viz; OP selector format |
| Modify | `chimera-core/src/block.rs`, `ui/fmt.rs`, `part.rs`, `params.rs` | `ValFmt::Pan`, `is_discrete` |
| Modify | `chimera-core/src/scope.rs` | `peak`, `SOUNDING_PEAK` |
| Delete | `chimera-core/src/ui/cell.rs` | |
| Create | `chimera-core/tests/screen/mod.rs` | test display, cases, scope fixture |
| Create | tests `screen_golden_test`, `draw_test`, `focus_test`, `header_map_test`, `cell_grid_test`, `big_viz_test`, `fm_viz_test`, `matrix_view_test`, `browser_test` | |
| Modify | tests `region_tests`, `mixer_page_test`, `ui_test`, `binding_test`, `memory_budget_test`; rename `cell_icon_test` → `valfmt_snap_test` | |
| Modify | `docs/adr/0016-visual-direction-refined-elektron.md` | addendum: font licences, measured cost |

`chimera-hal`, `chimera-desktop`, `chimera-stm32` need no changes: `render`, `prime_regions`, `render_dirty` keep their signatures.

---

### Task 1: Screen golden harness

**Files:**
- Create: `chimera-core/tests/screen/mod.rs`
- Create: `chimera-core/tests/screen_golden_test.rs`
- Modify: `chimera-core/src/ui/mod.rs` (`render`, `render_dirty`), `chimera-core/src/ui/renderer.rs` (`draw_with_def`, `draw_scope`)

**Interfaces:**
- Consumes: `UiState`, `PerfStats::zero()`, `scope::SCOPE_LEN`.
- Produces: `UiState::render_with_scope(&self, &mut D, &PerfStats, &[f32; SCOPE_LEN])`, `UiState::render_dirty_with_scope(&mut self, &mut D, &PerfStats, &[f32; SCOPE_LEN]) -> [(u16, u16); MAX_REGIONS]`; test module `screen` with `Fb { px: Vec<u16>, oob: usize }` (`new`, `at(x, y) -> Rgb565`, `hash() -> u64`, `dump(name)`), `W`, `H`, `Input` (`press`, `chord`, `turn`), `feed`, `settle`, `scope_fixture() -> [f32; SCOPE_LEN]`, `load_init(ui, ChainType)`, `CASES: &[(&str, fn(&mut UiState))]`, `ui_for(name) -> UiState`, `render(name) -> Fb`.

- [ ] **Step 1: Write the harness and the golden test (fails to compile: no `render_with_scope`)**

`chimera-core/tests/screen/mod.rs`:

```rust
//! Screen test harness (UI refresh spec § Testing): an in-memory 240×320
//! RGB565 display, the named screens the goldens lock, and a hash.
//!
//! `SCREEN_DUMP=<dir>` writes every rendered case to `<dir>/<case>.ppm`
//! for eyeballing against the mockups (`magick x.ppm x.png`).

#![allow(dead_code)]

use chimera_core::preset::{ChainType, Sound, POOL_SIZE};
use chimera_core::scope::SCOPE_LEN;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, ButtonState, ChimeraDisplay, Controls, EncoderId};
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;

pub const W: usize = 240;
pub const H: usize = 320;

/// A 240×320 framebuffer that counts writes outside the screen.
pub struct Fb {
    pub px: Vec<u16>,
    /// Pixels drawn outside 240×320 (must stay 0).
    pub oob: usize,
}

impl Fb {
    pub fn new() -> Self {
        Self { px: vec![0; W * H], oob: 0 }
    }

    pub fn at(&self, x: i32, y: i32) -> Rgb565 {
        RawU16::new(self.px[y as usize * W + x as usize]).into()
    }

    /// FNV-1a 64 over every pixel.
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &p in &self.px {
            for b in p.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }

    /// Write a binary PPM to `$SCREEN_DUMP/<name>.ppm` when the variable is set.
    pub fn dump(&self, name: &str) {
        let Some(dir) = std::env::var_os("SCREEN_DUMP") else { return };
        let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
        for &p in &self.px {
            let c: Rgb565 = RawU16::new(p).into();
            out.extend([(c.r() << 3) | (c.r() >> 2), (c.g() << 2) | (c.g() >> 4), (c.b() << 3) | (c.b() >> 2)]);
        }
        let path = std::path::Path::new(&dir).join(format!("{name}.ppm"));
        std::fs::write(&path, out).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

impl OriginDimensions for Fb {
    fn size(&self) -> Size {
        Size::new(W as u32, H as u32)
    }
}

impl DrawTarget for Fb {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if (0..W as i32).contains(&p.x) && (0..H as i32).contains(&p.y) {
                self.px[p.y as usize * W + p.x as usize] = RawU16::from(c).into_inner();
            } else {
                self.oob += 1;
            }
        }
        Ok(())
    }
}

impl ChimeraDisplay for Fb {
    fn flush(&mut self) {}
    fn flush_region(&mut self, _y_start: u16, _y_end: u16) {}
    fn pixel_buffer(&mut self) -> &mut [u16] {
        &mut self.px
    }
}

/// One frame of input.
#[derive(Default)]
pub struct Input {
    buttons: Vec<(ButtonId, ButtonState)>,
    encoders: Vec<(EncoderId, i8)>,
}

impl Input {
    pub fn press(b: ButtonId) -> Self {
        Self { buttons: vec![(b, ButtonState::Pressed)], ..Self::default() }
    }
    /// `held` down while `b` is pressed (MIX + B1, EDIT + B1, MIX + PLUS).
    pub fn chord(held: ButtonId, b: ButtonId) -> Self {
        Self { buttons: vec![(held, ButtonState::Held), (b, ButtonState::Pressed)], ..Self::default() }
    }
    pub fn turn(e: EncoderId, delta: i8) -> Self {
        Self { encoders: vec![(e, delta)], ..Self::default() }
    }
}

impl Controls for Input {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoders.iter().find(|e| e.0 == id).map_or(0, |e| e.1)
    }
    fn button_state(&self, id: ButtonId) -> ButtonState {
        self.buttons.iter().find(|b| b.0 == id).map_or(ButtonState::Up, |b| b.1)
    }
}

pub fn feed(ui: &mut UiState, input: Input) {
    ui.handle_input(&input);
}

/// Let every lerp settle (the goldens lock the resting screen).
pub fn settle(ui: &mut UiState) {
    for _ in 0..120 {
        ui.update();
    }
}

/// Live output the goldens draw: two periods of a lopsided triangle, peak 0.5.
pub fn scope_fixture() -> [f32; SCOPE_LEN] {
    core::array::from_fn(|i| {
        let p = (i as f32 / 120.0) % 1.0;
        let s = 0.7;
        0.5 * if p < s { -1.0 + 2.0 * p / s } else { 1.0 - 2.0 * (p - s) / (1.0 - s) }
    })
}

/// Load `ct`'s init Sound into Part 1 through the sound browser (EDIT + B1,
/// scroll to the init row, EDIT).
pub fn load_init(ui: &mut UiState, ct: ChainType) {
    let row = POOL_SIZE + ChainType::ALL.iter().position(|&c| c == ct).unwrap();
    feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    feed(ui, Input::turn(EncoderId::Main, row as i8));
    feed(ui, Input::press(ButtonId::Edit));
}

fn plus(ui: &mut UiState, n: usize) {
    for _ in 0..n {
        feed(ui, Input::press(ButtonId::Plus));
    }
}

/// Prime the focused slot for modulation (MIX + PLUS).
fn prime(ui: &mut UiState) {
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
}

/// Every screen the goldens lock, one or more per page type (spec § Testing).
pub const CASES: &[(&str, fn(&mut UiState))] = &[
    ("engine_pizza", |ui| feed(ui, Input::turn(EncoderId::A, 2))),
    ("engine_fm_alg", |ui| {
        load_init(ui, ChainType::Fm);
        feed(ui, Input::turn(EncoderId::A, 3));
    }),
    ("engine_fm_op", |ui| {
        load_init(ui, ChainType::Fm);
        feed(ui, Input::press(ButtonId::Edit)); // Operator sub-page
        feed(ui, Input::turn(EncoderId::A, 2)); // select operator 3
    }),
    ("bigviz_filter", |ui| {
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::B, 80)); // resonance
        feed(ui, Input::turn(EncoderId::A, -60)); // cutoff, focused
    }),
    ("bigviz_env", |ui| {
        plus(ui, 4);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::B, 6));
    }),
    ("bigviz_fm_op_env", |ui| {
        load_init(ui, ChainType::Fm);
        plus(ui, 4);
        feed(ui, Input::press(ButtonId::Edit));
        feed(ui, Input::turn(EncoderId::C, -4));
    }),
    ("mixer_part", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::D, -8));
    }),
    ("mixer_sends", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::C, 40));
    }),
    ("mixer_fx_delay", |ui| {
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::B1));
        plus(ui, 3);
    }),
    ("mod_matrix", |ui| {
        plus(ui, 2);
        feed(ui, Input::turn(EncoderId::A, 1)); // focus CUTOFF
        prime(ui);
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::A, 1)); // focus FOLD
        prime(ui);
        plus(ui, 1);
        feed(ui, Input::turn(EncoderId::E, 20)); // ENV → CUTOFF
        feed(ui, Input::turn(EncoderId::B, 1));
        feed(ui, Input::turn(EncoderId::E, -30)); // ENV → FOLD
        feed(ui, Input::turn(EncoderId::A, 1));
        feed(ui, Input::turn(EncoderId::B, -1));
        feed(ui, Input::turn(EncoderId::E, 42)); // LFO → CUTOFF, selected
    }),
    ("sound_browser", |ui| {
        let mut s = Sound::init(ChainType::Fm);
        s.name = [0; 16];
        s.name[..9].copy_from_slice(b"WARM BASS");
        ui.pool.store(0, s);
        let mut s = Sound::init(ChainType::Modal);
        s.name = [0; 16];
        s.name[..11].copy_from_slice(b"GLASS PLUCK");
        ui.pool.store(1, s);
        feed(ui, Input::chord(ButtonId::Edit, ButtonId::B1));
        feed(ui, Input::turn(EncoderId::Main, 1));
    }),
    ("system", |ui| feed(ui, Input::press(ButtonId::Menu))),
];

/// Build case `name`'s screen: a fresh UiState, the case's input, settled lerps.
pub fn ui_for(name: &str) -> UiState {
    let (_, setup) = CASES.iter().find(|c| c.0 == name).unwrap_or_else(|| panic!("no case {name}"));
    let mut ui = UiState::new();
    setup(&mut ui);
    settle(&mut ui);
    ui
}

/// Full render of case `name`.
pub fn render(name: &str) -> Fb {
    let ui = ui_for(name);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb.dump(name);
    fb
}

/// Render case `name` through `render_dirty` from a fresh region set.
pub fn render_dirty(name: &str) -> Fb {
    let mut ui = ui_for(name);
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb
}
```

`chimera-core/tests/screen_golden_test.rs`:

```rust
//! Screen goldens (UI refresh spec § Testing): one FNV-1a hash per screen.
//!
//! A case is `Pending` until the task that converts its page type to
//! Direction A records it; from then on it is `Locked` and must match
//! bit-for-bit. Re-record a Locked case ONLY in a task whose text names that
//! case as an intended change:
//!
//!     SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test -- --nocapture
//!
//! and paste the printed row over the case's entry. To look at the screens:
//!
//!     SCREEN_DUMP=/tmp/screens cargo test -p chimera-core --test screen_golden_test

mod screen;

use screen::*;

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // `Locked` arrives with the first converted page type
enum Golden {
    /// Not yet converted: may change freely.
    Pending,
    /// Converted: must match.
    Locked(u64),
}
use Golden::*;

const GOLDENS: &[(&str, Golden)] = &[
    ("engine_pizza", Pending),
    ("engine_fm_alg", Pending),
    ("engine_fm_op", Pending),
    ("bigviz_filter", Pending),
    ("bigviz_env", Pending),
    ("bigviz_fm_op_env", Pending),
    ("mixer_part", Pending),
    ("mixer_sends", Pending),
    ("mixer_fx_delay", Pending),
    ("mod_matrix", Pending),
    ("sound_browser", Pending),
    ("system", Pending),
];

#[test]
fn every_case_has_a_golden_entry() {
    let names: Vec<&str> = CASES.iter().map(|c| c.0).collect();
    let goldens: Vec<&str> = GOLDENS.iter().map(|g| g.0).collect();
    assert_eq!(names, goldens);
}

#[test]
fn screen_goldens_match() {
    let record = std::env::var_os("SCREEN_RECORD").is_some();
    let mut failures = Vec::new();
    for &(name, golden) in GOLDENS {
        let hash = render(name).hash();
        if record {
            println!("    (\"{name}\", Locked(0x{hash:016x})),");
            continue;
        }
        if let Locked(want) = golden
            && hash != want
        {
            failures.push(format!("{name}: 0x{hash:016x} (want 0x{want:016x})"));
        }
    }
    assert!(failures.is_empty(), "screen golden mismatch:\n{}", failures.join("\n"));
}

#[test]
fn rendering_is_deterministic() {
    for &(name, _) in GOLDENS {
        assert_eq!(render(name).hash(), render(name).hash(), "{name}");
    }
}
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p chimera-core --test screen_golden_test`
Expected: compile error `no method named render_with_scope found for struct UiState`.

- [ ] **Step 3: Thread the scope buffer through rendering**

In `chimera-core/src/ui/mod.rs` add `use crate::scope::SCOPE_LEN;` next to the `preset` import, and replace `render` with:

```rust
    /// Render full screen to a display, with live output from the scope buffer.
    pub fn render<D>(&self, display: &mut D, perf: &PerfStats)
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        let mut scope = [0.0f32; SCOPE_LEN];
        crate::scope::read_samples(&mut scope);
        self.render_with_scope(display, perf, &scope);
    }

    /// Render full screen with `scope` as the live output (tests pass a
    /// fixed buffer so screen goldens are deterministic).
    pub fn render_with_scope<D>(&self, display: &mut D, perf: &PerfStats, scope: &[f32; SCOPE_LEN])
    where
        D: embedded_graphics::draw_target::DrawTarget<
                Color = embedded_graphics::pixelcolor::Rgb565,
            >,
    {
        if let UiMode::SoundBrowser { part, cursor, scroll } = self.ui_mode {
            Renderer::draw_sound_browser(display, &self.pool, part, cursor, scroll, self.performance.parts[part].sound.chain_type);
            return;
        }
        let def = self.nav.active_block_def();
        self.renderer.draw_with_def(display, &self.nav, def, perf, &self.matrix_state, self.sel_op, scope);
    }
```

Split `render_dirty` the same way: keep its signature and doc comment, make its body

```rust
        let mut scope = [0.0f32; SCOPE_LEN];
        crate::scope::read_samples(&mut scope);
        self.render_dirty_with_scope(display, perf, &scope)
```

and add below it `pub fn render_dirty_with_scope<D>(&mut self, display: &mut D, perf: &PerfStats, scope: &[f32; SCOPE_LEN]) -> [(u16, u16); region::MAX_REGIONS]` (same `where` clause, doc `/// \`render_dirty\` with \`scope\` as the live output.`) holding the old body, with `renderer::Renderer::draw_scope(display);` changed to `renderer::Renderer::draw_scope(display, scope);`.

In `chimera-core/src/ui/renderer.rs`: `draw_with_def` gets `#[allow(clippy::too_many_arguments)]` and a last parameter `scope: &[f32; crate::scope::SCOPE_LEN]`, passing it on as `Self::draw_scope(display, scope);`; `draw_scope` becomes `pub fn draw_scope<D>(display: &mut D, buf: &[f32; crate::scope::SCOPE_LEN])` and loses its first three body lines (the `// Read scope buffer` comment, the `let mut buf` array and `read_samples` call).

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test screen_golden_test`
Expected: `3 passed` (everything `Pending`). Optional: `SCREEN_DUMP=/tmp/before cargo test -p chimera-core --test screen_golden_test` to keep the pre-refresh screens.

- [ ] **Step 5: `just check`, then commit**

Run: `PKG_CONFIG_PATH=… just check` — expected core+hal 441 passed, 2 ignored; desktop 2 passed; firmware links.

```bash
git add chimera-core/tests/screen chimera-core/tests/screen_golden_test.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/renderer.rs
git commit -m "test(core): screen golden harness (in-memory 240x320 display, per-screen hash)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Theme tokens, fonts and draw primitives

**Files:**
- Modify: `chimera-core/Cargo.toml`
- Rewrite: `chimera-core/src/ui/theme.rs`
- Create: `chimera-core/src/ui/draw.rs`
- Modify: `chimera-core/src/ui/mod.rs` (`pub mod draw;`), `chimera-core/src/ui/renderer.rs` (`clear_region_fb`)
- Modify: `docs/adr/0016-visual-direction-refined-elektron.md` (addendum)
- Test: `chimera-core/tests/draw_test.rs`

**Interfaces:**
- Consumes: embedded-graphics 0.8 primitives, `u8g2_fonts::{FontRenderer, fonts, types::*}`.
- Produces: `theme::{BG, INK, INK2, MID, BAR_REST, FAINT, ACCENT, ACCENT_SOFT, WARN, ALERT}`, `theme::{FONT_FOCUS, FONT_READOUT, FONT_VALUE, FONT_LABEL, FONT_LABEL_BOLD}: FontRenderer`, `theme::LABEL_TRACKING`, the layout constants below; `draw::{text, text_tracked, text_width, text_right, text_center, fill_rect, line, dot, ring, pill, round_outline, bar, arc_gauge, arrow}` with the signatures in the code. Legacy theme names stay as aliases until Task 13.

- [ ] **Step 1: Write the failing primitive tests**

`chimera-core/tests/draw_test.rs`:

```rust
//! Direction A primitives (`ui::draw`, `ui::theme`).

mod screen;

use chimera_core::ui::draw;
use chimera_core::ui::theme;
use embedded_graphics::pixelcolor::Rgb565;
use screen::Fb;

fn count(fb: &Fb, c: Rgb565) -> usize {
    (0..320).flat_map(|y| (0..240).map(move |x| (x, y))).filter(|&(x, y)| fb.at(x, y) == c).count()
}

#[test]
fn text_advance_matches_measured_width() {
    let mut fb = Fb::new();
    let adv = draw::text(&mut fb, &theme::FONT_VALUE, "CUTOFF", 10, 40, theme::INK);
    assert_eq!(adv, draw::text_width(&theme::FONT_VALUE, "CUTOFF", 0));
    assert_eq!(adv, 60, "helvB10 CUTOFF");
    assert!(count(&fb, theme::INK) > 0);
}

#[test]
fn tracking_adds_one_pixel_per_glyph() {
    let plain = draw::text_width(&theme::FONT_LABEL, "SHAPE", 0);
    assert_eq!(draw::text_width(&theme::FONT_LABEL, "SHAPE", 1), plain + 5);
    let mut fb = Fb::new();
    assert_eq!(draw::text_tracked(&mut fb, &theme::FONT_LABEL, "SHAPE", 0, 20, theme::MID, 1), plain + 5);
}

#[test]
fn unknown_glyphs_are_skipped_not_fatal() {
    let mut fb = Fb::new();
    draw::text(&mut fb, &theme::FONT_FOCUS, "→63", 10, 100, theme::INK);
    assert!(count(&fb, theme::INK) > 0, "the digits still draw");
}

#[test]
fn focus_font_is_large_and_label_font_small() {
    assert_eq!(theme::FONT_FOCUS.get_ascent(), 42);
    assert!(theme::FONT_LABEL.get_ascent() <= 8);
}

#[test]
fn unipolar_bar_fills_from_the_left() {
    let mut fb = Fb::new();
    draw::bar(&mut fb, 10, 10, 62, 2, 0.5, false, theme::FAINT, theme::ACCENT);
    let lit: Vec<i32> = (0..240).filter(|&x| fb.at(x, 10) == theme::ACCENT).collect();
    assert_eq!(lit.first(), Some(&10));
    assert_eq!(lit.len(), 31);
    assert_eq!(fb.at(71, 10), theme::FAINT, "track to the end");
}

#[test]
fn zero_bar_still_shows_two_pixels() {
    let mut fb = Fb::new();
    draw::bar(&mut fb, 10, 10, 62, 2, 0.0, false, theme::FAINT, theme::ACCENT);
    assert_eq!(count(&fb, theme::ACCENT), 4);
}

#[test]
fn bipolar_bar_grows_from_the_centre() {
    for (v, left, right) in [(0.75, 41, 56), (0.25, 26, 41)] {
        let mut fb = Fb::new();
        draw::bar(&mut fb, 10, 10, 62, 2, v, true, theme::FAINT, theme::ACCENT);
        let lit: Vec<i32> = (0..240).filter(|&x| fb.at(x, 10) == theme::ACCENT).collect();
        assert_eq!((lit[0], *lit.last().unwrap() + 1), (left, right), "value {v}");
    }
}

#[test]
fn arc_gauge_lights_the_start_for_low_values_and_the_top_for_bipolar_zero() {
    let (cx, cy, r) = (100, 100, 28);
    // Unipolar 0.1: lit near 7:30 (lower left), not at 4:30 (lower right).
    let mut fb = Fb::new();
    draw::arc_gauge(&mut fb, cx, cy, r, 5, 0.1, false, theme::FAINT, theme::ACCENT);
    assert_eq!(fb.at(cx - 20, cy + 20), theme::ACCENT, "start at lower left");
    assert_eq!(fb.at(cx + 20, cy + 20), theme::FAINT, "end at lower right is track");
    // Bipolar centre: only a cap at 12:00.
    let mut fb = Fb::new();
    draw::arc_gauge(&mut fb, cx, cy, r, 5, 0.5, true, theme::FAINT, theme::ACCENT);
    assert_eq!(fb.at(cx, cy - r), theme::ACCENT, "12 o'clock");
    assert_eq!(fb.at(cx - 20, cy + 20), theme::FAINT);
}

#[test]
fn pill_stays_inside_its_box() {
    let mut fb = Fb::new();
    draw::pill(&mut fb, 50, 276, 34, 20, theme::ACCENT);
    for y in 0..320 {
        for x in 0..240 {
            let inside = (50..84).contains(&x) && (276..296).contains(&y);
            if !inside {
                assert_ne!(fb.at(x, y), theme::ACCENT, "({x},{y})");
            }
        }
    }
    assert_eq!(fb.at(67, 286), theme::ACCENT);
    assert_ne!(fb.at(50, 276), theme::ACCENT, "rounded corner");
}

#[test]
fn primitives_clip_without_panicking() {
    let mut fb = Fb::new();
    draw::dot(&mut fb, 239, 319, 4, theme::INK);
    draw::text(&mut fb, &theme::FONT_FOCUS, "127", 220, 330, theme::INK);
    assert!(fb.oob > 0, "the harness sees off-screen writes");
}

/// The palette is the mockup's (ADR 0016) and the accent is unique.
#[test]
fn palette_tokens() {
    assert_eq!(theme::ACCENT, Rgb565::new(15, 53, 25)); // #7fd4c8
    assert_eq!(theme::BG, Rgb565::new(1, 2, 1)); // #0a0b0d
    for c in [theme::BG, theme::INK, theme::INK2, theme::MID, theme::BAR_REST, theme::FAINT, theme::ACCENT_SOFT] {
        assert_ne!(c, theme::ACCENT);
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test draw_test`
Expected: compile errors (`unresolved import chimera_core::ui::draw`, no `theme::INK`).

- [ ] **Step 3: Add the crate, the theme and the primitives**

`chimera-core/Cargo.toml`, after `embedded-graphics-core = "0.4"`:

```toml
# Bitmap fonts from U8g2 (crate MIT OR Apache-2.0; font licences in ADR 0016).
u8g2-fonts = "=0.8.0"
```

Replace `chimera-core/src/ui/theme.rs` entirely:

```rust
//! Direction A (ADR 0016): the single source of colours, fonts and layout.
//! Dark ground, warm greys, one accent for the active element.

use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::{fonts, FontRenderer};

// --- Palette (mockup hex → RGB565) ---

/// Ground `#0a0b0d`.
pub const BG: Rgb565 = Rgb565::new(1, 2, 1);
/// Primary text `#ecebe7`: names, the focus value.
pub const INK: Rgb565 = Rgb565::new(29, 58, 28);
/// Secondary text `#c9c7c1`: cell values, list names.
pub const INK2: Rgb565 = Rgb565::new(25, 49, 24);
/// Labels `#8b8a86`.
pub const MID: Rgb565 = Rgb565::new(17, 34, 16);
/// Resting bar fill `#6c6b67`.
pub const BAR_REST: Rgb565 = Rgb565::new(13, 26, 12);
/// Tracks, rules, empty slots `#2a2b2e`.
pub const FAINT: Rgb565 = Rgb565::new(5, 10, 5);
/// The one accent `#7fd4c8`: the active element only.
pub const ACCENT: Rgb565 = Rgb565::new(15, 53, 25);
/// Accent at 12 % over the ground: fill under a viz line.
pub const ACCENT_SOFT: Rgb565 = Rgb565::new(3, 8, 4);
/// Audio load above 60 % / 80 %.
pub const WARN: Rgb565 = Rgb565::new(31, 32, 0);
pub const ALERT: Rgb565 = Rgb565::new(31, 0, 0);

// --- Fonts (u8g2; only these are linked) ---

/// Focus value: Logisoso 42 px, ASCII (choices show as text: POLY, P1).
pub const FONT_FOCUS: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_logisoso42_tr>().with_ignore_unknown_chars(true);
/// Readout riding on a BigViz viz: Logisoso 20 px.
pub const FONT_READOUT: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_logisoso20_tr>().with_ignore_unknown_chars(true);
/// Values, header name, focus label, list names: Helvetica Bold 10.
pub const FONT_VALUE: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvB10_tr>().with_ignore_unknown_chars(true);
/// Small uppercase labels: Helvetica 8.
pub const FONT_LABEL: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvR08_tr>().with_ignore_unknown_chars(true);
/// Map pill, header name: Helvetica Bold 8.
pub const FONT_LABEL_BOLD: FontRenderer =
    FontRenderer::new::<fonts::u8g2_font_helvB08_tr>().with_ignore_unknown_chars(true);
/// Extra advance between uppercase label letters.
pub const LABEL_TRACKING: i32 = 1;

// --- Layout (240×320) ---

pub const SCREEN_W: i32 = 240;
pub const SCREEN_H: i32 = 320;
/// Left text margin.
pub const MARGIN_X: i32 = 12;

/// Header band 0..28.
pub const HEADER_BOTTOM: i32 = 28;
pub const HEADER_BASELINE: i32 = 20;
/// Sounding dot.
pub const HEADER_DOT_X: i32 = 225;
pub const HEADER_DOT_Y: i32 = 16;
pub const HEADER_DOT_R: i32 = 3;

/// Focus band 28..118: label, big value, arc gauge.
pub const FOCUS_BOTTOM: i32 = 118;
pub const FOCUS_LABEL_Y: i32 = 50;
pub const FOCUS_VALUE_X: i32 = 10;
pub const FOCUS_VALUE_Y: i32 = 104;
pub const ARC_CX: i32 = 188;
pub const ARC_CY: i32 = 80;
pub const ARC_R: i32 = 28;
pub const ARC_WIDTH: u32 = 5;

/// Viz band on CellGrid / Mixer pages: 118..186, centre line 152.
pub const VIZ_BAND_TOP: i32 = 118;
pub const VIZ_BAND_BOTTOM: i32 = 186;
pub const VIZ_BAND_MID: i32 = 152;
pub const VIZ_BAND_AMP: i32 = 24;
/// Large viz on BigViz pages: 28..186.
pub const BIGVIZ_BOTTOM: i32 = 186;
pub const VIZ_LEFT: i32 = 12;
pub const VIZ_RIGHT: i32 = 228;

/// Cells 186..266: 3×2, knob order a–f.
pub const CELLS_BOTTOM: i32 = 266;
/// Baseline of the first row's labels.
pub const CELL_LABEL_Y: i32 = 196;
pub const CELL_ROW_H: i32 = 36;
pub const CELL_COL_W: i32 = 74;
pub const CELL_VALUE_DY: i32 = 17;
pub const CELL_BAR_DY: i32 = 22;
pub const CELL_BAR_W: i32 = 62;
pub const CELL_BAR_H: i32 = 2;
/// Mod amount line under the bar (primed params only).
pub const CELL_MOD_DY: i32 = 26;

/// Map 266..320: nodes on a line, current block a pill.
pub const MAP_TOP: i32 = 266;
pub const MAP_LINE_Y: i32 = 286;
pub const MAP_X0: i32 = 24;
pub const MAP_X1: i32 = 216;
pub const PILL_W: i32 = 34;
pub const PILL_H: i32 = 20;
pub const PILL_LABEL_Y: i32 = 290;
pub const NODE_R: i32 = 4;
pub const NODE_LABEL_Y: i32 = 306;
/// Sub-page branch rows under the pill.
pub const BRANCH_START_Y: i32 = 300;
pub const BRANCH_LINE_HEIGHT: i32 = 10;

// --- Legacy (pre-Direction A) names, deleted once their last user is ---

pub const TEXT: Rgb565 = INK;
pub const TEXT_DIM: Rgb565 = FAINT;
pub const TEXT_MID: Rgb565 = MID;
pub const ACCENT_DIM: Rgb565 = ACCENT_SOFT;
pub const ACCENT_BRIGHT: Rgb565 = ACCENT;
pub const NODE_ACTIVE_BG: Rgb565 = ACCENT;
pub const NODE_ACTIVE_TEXT: Rgb565 = BG;
pub const NODE_INACTIVE_BORDER: Rgb565 = FAINT;
pub const NODE_INACTIVE_TEXT: Rgb565 = MID;
pub const NODE_CONNECTOR: Rgb565 = FAINT;
pub const BRANCH_MARKER: Rgb565 = ACCENT;
pub const BRANCH_TEXT: Rgb565 = MID;
pub const BRANCH_TEXT_ACTIVE: Rgb565 = INK;
pub const PARAM_LABEL: Rgb565 = MID;
pub const PARAM_VALUE: Rgb565 = INK;
pub const PARAM_BAR_BG: Rgb565 = FAINT;
pub const PARAM_BAR_FG: Rgb565 = ACCENT;
pub const VIZ_LINE: Rgb565 = ACCENT;
pub const VIZ_FILL: Rgb565 = ACCENT_SOFT;
pub const VIZ_GRID: Rgb565 = FAINT;
pub const SEPARATOR: Rgb565 = FAINT;
pub const HEADER_LABEL: Rgb565 = MID;
pub const ENCODER_ZONE_BOTTOM: i32 = 265;
pub const SCOPE_TOP: i32 = 240;
pub const SCOPE_HEIGHT: i32 = 26;
pub const SCOPE_BOTTOM: i32 = SCOPE_TOP + SCOPE_HEIGHT;
pub const HEADER_Y: i32 = 6;
pub const VIZ_TOP: i32 = 28;
pub const VIZ_BOTTOM: i32 = 170;
pub const PARAM_TOP: i32 = 178;
pub const PARAM_ROW_HEIGHT: i32 = 30;
pub const PARAM_COL_WIDTH: i32 = 74;
pub const PARAM_LEFT: i32 = 10;
pub const NODE_WIDTH: i32 = 30;
pub const NODE_HEIGHT: i32 = 14;
pub const NODE_GAP: i32 = 6;
pub const NODE_ROW_Y: i32 = 278;
pub const BAR_WIDTH: i32 = 52;
pub const BAR_HEIGHT: i32 = 3;
```

Create `chimera-core/src/ui/draw.rs`:

```rust
//! Direction A drawing primitives (ADR 0016): text in the u8g2 faces, thin
//! bars, arc gauges, pills, dots and rings. No outline boxes. Every function
//! ignores draw errors (the targets are infallible framebuffers).

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{AngleUnit, Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{
    Arc, Circle, CornerRadii, Line, PrimitiveStyle, Rectangle, RoundedRectangle, StyledDrawable,
};
use u8g2_fonts::types::{FontColor, VerticalPosition};
use u8g2_fonts::FontRenderer;

/// Draw `s` with its baseline at `y`; returns the advance in pixels.
pub fn text<D>(d: &mut D, font: &FontRenderer, s: &str, x: i32, y: i32, color: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    font.render(s, Point::new(x, y), VerticalPosition::Baseline, FontColor::Transparent(color), d)
        .map_or(0, |dims| dims.advance.x)
}

/// Draw `s` with `tracking` extra pixels after each glyph; returns the advance.
pub fn text_tracked<D>(d: &mut D, font: &FontRenderer, s: &str, x: i32, y: i32, color: Rgb565, tracking: i32) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut cx = x;
    for ch in s.chars() {
        let adv = font
            .render(ch, Point::new(cx, y), VerticalPosition::Baseline, FontColor::Transparent(color), d)
            .map_or(0, |dims| dims.advance.x);
        cx += adv + tracking;
    }
    cx - x
}

/// Advance of `s` in `font` (with `tracking` after each glyph).
pub fn text_width(font: &FontRenderer, s: &str, tracking: i32) -> i32 {
    let adv = font
        .get_rendered_dimensions(s, Point::zero(), VerticalPosition::Baseline)
        .map_or(0, |dims| dims.advance.x);
    adv + tracking * s.chars().count() as i32
}

/// Draw `s` ending at `right` (exclusive).
pub fn text_right<D>(d: &mut D, font: &FontRenderer, s: &str, right: i32, y: i32, color: Rgb565, tracking: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = text_width(font, s, tracking);
    text_tracked(d, font, s, right - w, y, color, tracking);
}

/// Draw `s` centred on `cx`.
pub fn text_center<D>(d: &mut D, font: &FontRenderer, s: &str, cx: i32, y: i32, color: Rgb565, tracking: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = text_width(font, s, tracking);
    text_tracked(d, font, s, cx - w / 2, y, color, tracking);
}

pub fn fill_rect<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    if w > 0 && h > 0 {
        let _ = Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
            .draw_styled(&PrimitiveStyle::with_fill(color), d);
    }
}

pub fn line<D>(d: &mut D, x0: i32, y0: i32, x1: i32, y1: i32, color: Rgb565, width: u32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = Line::new(Point::new(x0, y0), Point::new(x1, y1)).draw_styled(&PrimitiveStyle::with_stroke(color, width), d);
}

/// Filled circle of radius `r` centred on (cx, cy).
pub fn dot<D>(d: &mut D, cx: i32, cy: i32, r: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = Circle::with_center(Point::new(cx, cy), (2 * r + 1) as u32).draw_styled(&PrimitiveStyle::with_fill(color), d);
}

/// Circle outline of radius `r`.
pub fn ring<D>(d: &mut D, cx: i32, cy: i32, r: i32, color: Rgb565, width: u32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = Circle::with_center(Point::new(cx, cy), (2 * r + 1) as u32)
        .draw_styled(&PrimitiveStyle::with_stroke(color, width), d);
}

/// Filled rounded rectangle with fully round ends (radius h/2).
pub fn pill<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let r = Size::new((h / 2) as u32, (h / 2) as u32);
    let _ = RoundedRectangle::new(Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)), CornerRadii::new(r))
        .draw_styled(&PrimitiveStyle::with_fill(color), d);
}

/// Rounded-rectangle outline (the matrix cursor).
pub fn round_outline<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, radius: u32, color: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let _ = RoundedRectangle::new(
        Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)),
        CornerRadii::new(Size::new(radius, radius)),
    )
    .draw_styled(&PrimitiveStyle::with_stroke(color, 1), d);
}

/// Thin value bar: `track` over `w`, then the value in `fill`. Unipolar bars
/// grow from the left; bipolar bars grow from the centre. `value` is 0..1
/// (bipolar centre 0.5). The fill is at least 2 px so zero stays visible.
#[allow(clippy::too_many_arguments)]
pub fn bar<D>(d: &mut D, x: i32, y: i32, w: i32, h: i32, value: f32, bipolar: bool, track: Rgb565, fill: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    fill_rect(d, x, y, w, h, track);
    let v = value.clamp(0.0, 1.0);
    if bipolar {
        let c = x + w / 2;
        let len = ((v - 0.5) * w as f32) as i32;
        let (x0, x1) = if len >= 0 { (c, c + len) } else { (c + len, c) };
        let x1 = x1.max(x0 + 2);
        fill_rect(d, x0, y, x1 - x0, h, fill);
    } else {
        let len = ((v * w as f32 + 0.5) as i32).max(2);
        fill_rect(d, x, y, len, h, fill);
    }
}

/// 270° gauge from 7:30 clockwise to 4:30 (the mockup's 0.75π..2.25π).
/// Track in `track`; the value arc in `fill` from the start (unipolar) or
/// from 12:00 (bipolar). Round caps.
#[allow(clippy::too_many_arguments)]
pub fn arc_gauge<D>(d: &mut D, cx: i32, cy: i32, r: i32, width: u32, value: f32, bipolar: bool, track: Rgb565, fill: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    const START: f32 = 135.0;
    const SWEEP: f32 = 270.0;
    let v = value.clamp(0.0, 1.0);
    let dia = (2 * r + 1) as u32;
    let center = Point::new(cx, cy);
    let stroke = |c| PrimitiveStyle::with_stroke(c, width);
    let _ = Arc::with_center(center, dia, START.deg(), SWEEP.deg()).draw_styled(&stroke(track), d);
    let end = START + SWEEP * v;
    let (from, to) = if bipolar {
        let mid = START + SWEEP / 2.0;
        if end >= mid { (mid, end) } else { (end, mid) }
    } else {
        (START, end.max(START + 1.0))
    };
    let _ = Arc::with_center(center, dia, from.deg(), (to - from).deg()).draw_styled(&stroke(fill), d);
    let cap = (width as i32) / 2;
    for a in [from, to] {
        let rad = a.to_radians();
        let px = cx + libm::roundf(r as f32 * libm::cosf(rad)) as i32;
        let py = cy + libm::roundf(r as f32 * libm::sinf(rad)) as i32;
        dot(d, px, py, cap, fill);
    }
}

/// A right arrow ("→") at the text baseline in the label size; returns its width.
pub fn arrow<D>(d: &mut D, x: i32, y: i32, color: Rgb565) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    let my = y - 3;
    line(d, x, my, x + 7, my, color, 1);
    line(d, x + 5, my - 2, x + 7, my, color, 1);
    line(d, x + 5, my + 2, x + 7, my, color, 1);
    8
}
```

In `chimera-core/src/ui/mod.rs` add `pub mod draw;` before `pub mod dungeon_map;`. In `renderer.rs`, `clear_region_fb` must clear to the new ground (it wrote `0`):

```rust
    pub fn clear_region_fb(fb: &mut [u16], y_start: u16, y_end: u16) {
        use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
        let bg = RawU16::from(theme::BG).into_inner();
        let start = y_start as usize * 240;
        let end = y_end as usize * 240;
        fb[start..end].fill(bg);
    }
```

Append to `docs/adr/0016-visual-direction-refined-elektron.md`:

```markdown

## Addendum: Font licences (2026-09-24)
Recorded when `u8g2-fonts` was added (UI refresh plan); the decision above is unchanged.

| Font (u8g2 name) | Use | Author / licence |
|---|---|---|
| crate `u8g2-fonts` 0.8.0 | renderer | Finomnis; MIT OR Apache-2.0 |
| `logisoso42_tr`, `logisoso20_tr` | focus value, viz readout | Mathieu Gabiot (2009); GPL v2 with font exception per its copyright statement, OFL per openfontlibrary.org — either permits embedding in firmware |
| `helvB10_tr`, `helvR08_tr`, `helvB08_tr` | values, labels, map | Adobe / Digital Equipment Corp. X11 bitmap fonts; permission notice in the U8g2 LICENSE (use, copy, modify, distribute, sell; keep the notice) |

Sources: https://github.com/olikraus/u8g2/blob/master/LICENSE,
https://github.com/olikraus/u8g2/wiki/fntgrplogisoso
```

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test draw_test`
Expected: `11 passed`. (The arc test pins the e-g angle convention: 0° at 3 o'clock, clockwise positive.)

- [ ] **Step 5: `just check`, then commit**

Expected: core+hal 452 passed; firmware links (proves `u8g2-fonts` builds for `thumbv7em-none-eabihf`).

```bash
git add chimera-core/Cargo.toml Cargo.lock chimera-core/src/ui/theme.rs chimera-core/src/ui/draw.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/renderer.rs chimera-core/tests/draw_test.rs docs/adr/0016-visual-direction-refined-elektron.md
git commit -m "feat(core): Direction A theme tokens, u8g2 fonts and draw primitives

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Last-touched slot per page; one `Frame` for the renderer

**Files:**
- Create: `chimera-core/src/ui/focus.rs`
- Modify: `chimera-core/src/ui/mod.rs`, `chimera-core/src/ui/renderer.rs`
- Modify: `chimera-core/tests/mixer_page_test.rs` (one call site)
- Test: `chimera-core/tests/focus_test.rs`

**Interfaces:**
- Consumes: `BlockDef::id`, `screen::{feed, Input}`.
- Produces: `focus::{FocusMemory, MAX_PAGES}` (`new`, `get(def_id: u16) -> usize`, `touch(def_id: u16, slot: usize)`), `UiState::focused_slot() -> usize`, `renderer::Frame<'a> { nav, def, perf, matrix, sel_op, focus, scope }` (later tasks add `sounding`, `parts`, `active_part`), `Renderer::draw_with_def(&self, &mut D, &Frame)`, `Renderer::draw_region_with_def(&self, &mut D, RegionKind, &Frame)`. `Renderer::focused` and `UiState::last_encoder` are removed.

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/focus_test.rs`:

```rust
//! Focus band tracking (UI refresh spec § Focus tracking): the last-touched
//! slot per page, slot a by default, no timer.

mod screen;

use chimera_core::ui::block_def::ChainDef2;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::focus::{FocusMemory, MAX_PAGES};
use chimera_core::ui::UiState;
use chimera_hal::{ButtonId, EncoderId};
use screen::{feed, Input};

#[test]
fn every_page_starts_on_slot_a() {
    let ui = UiState::new();
    assert_eq!(ui.focused_slot(), 0);
    let m = FocusMemory::new();
    assert!((0..MAX_PAGES as u16).all(|id| m.get(id) == 0));
}

#[test]
fn the_first_tick_of_another_slot_moves_focus_and_it_stays() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    assert_eq!(ui.focused_slot(), 2);
    for _ in 0..50 {
        ui.update(); // no timer: focus does not fall back
    }
    assert_eq!(ui.focused_slot(), 2);
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(ui.focused_slot(), 1);
}

#[test]
fn focus_is_remembered_per_page() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // Pizza: C
    feed(&mut ui, Input::press(ButtonId::Plus)); // → Drive
    assert_eq!(ui.focused_slot(), 0, "a page not yet touched shows slot a");
    feed(&mut ui, Input::turn(EncoderId::B, 1)); // Drive: B
    feed(&mut ui, Input::press(ButtonId::Minus)); // ← Pizza
    assert_eq!(ui.focused_slot(), 2);
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert_eq!(ui.focused_slot(), 1);
}

#[test]
fn mixer_part_and_matrix_pages_use_the_same_mechanism() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    feed(&mut ui, Input::turn(EncoderId::D, -1)); // LEVEL
    assert_eq!(ui.focused_slot(), 3);
    feed(&mut ui, Input::press(ButtonId::B1)); // Part 1 chain, Pizza
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // → MOD
    }
    feed(&mut ui, Input::turn(EncoderId::E, 5)); // amount
    assert_eq!(ui.focused_slot(), 4);
}

#[test]
fn out_of_range_page_ids_are_ignored() {
    let mut m = FocusMemory::new();
    m.touch(MAX_PAGES as u16 + 3, 4);
    assert_eq!(m.get(MAX_PAGES as u16 + 3), 0);
    m.touch(1, 9);
    assert_eq!(m.get(1), 5, "slots clamp to f");
}

#[test]
fn every_page_id_fits_the_focus_table() {
    let chains: [&ChainDef2; 9] = [
        &reg::PIZZA_POLY_CHAIN, &reg::KICK_CHAIN, &reg::MODAL_PLUCK_CHAIN, &reg::FM_CHAIN, &reg::MIX_CHAIN,
        &reg::ENVELOPE_CHAIN, &reg::MIXER_CHANNEL_CHAIN, &reg::SYSTEM_CHAIN, &reg::DEMO_CHAIN,
    ];
    for chain in chains {
        for block in chain.blocks {
            for def in core::iter::once(block.def).chain(block.sub_pages.iter().copied()) {
                assert!((def.id as usize) < MAX_PAGES, "{} id {}", def.name, def.id);
            }
        }
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test focus_test`
Expected: compile error `unresolved import chimera_core::ui::focus`.

- [ ] **Step 3: Implement**

Create `chimera-core/src/ui/focus.rs`:

```rust
//! The last-touched encoder slot per page (UI refresh spec § Focus
//! tracking): the focus band shows it until another slot is touched. No
//! timer. Pages are keyed by `BlockDef::id`, so a page shared by several
//! chains (FILTER) keeps one focus, and the FM operator selection does not
//! reset it.

/// One entry per `BlockDef::id`; ids are 0..=40 today (test-checked).
pub const MAX_PAGES: usize = 48;

#[derive(Clone, Copy, Debug)]
pub struct FocusMemory {
    slots: [u8; MAX_PAGES],
}

impl Default for FocusMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusMemory {
    /// Every page starts focused on slot a.
    pub const fn new() -> Self {
        Self { slots: [0; MAX_PAGES] }
    }

    /// The focused slot (0..=5) of page `def_id`.
    pub fn get(&self, def_id: u16) -> usize {
        self.slots.get(def_id as usize).map_or(0, |&s| s as usize)
    }

    /// Slot `slot` of page `def_id` was just turned.
    pub fn touch(&mut self, def_id: u16, slot: usize) {
        if let Some(s) = self.slots.get_mut(def_id as usize) {
            *s = slot.min(5) as u8;
        }
    }
}
```

`chimera-core/src/ui/mod.rs`:
- `pub mod focus;` after `pub mod fmt;`.
- Field `last_encoder: usize` (and its doc comment) → `/// Last-touched slot per page: the focus band and MIX + Plus/Minus.\n    focus: focus::FocusMemory,`; initialiser `last_encoder: 0,` → `focus: focus::FocusMemory::new(),`.
- Add before `/// The selected FM operator.`:

```rust
    /// The slot the focus band shows on the current page: the last one
    /// turned there, slot a until then.
    pub fn focused_slot(&self) -> usize {
        self.focus.get(self.nav.active_block_def().id)
    }
```

- In `current_param_addr` and `mod_label`, `self.last_encoder` → `self.focused_slot()`.
- In `handle_input`, matrix branch: after `if delta != 0 {` insert `self.focus.touch(def.id, i);`; normal branch: replace the two lines `self.last_encoder = i;` / `self.renderer.focused = i;` with `self.focus.touch(def.id, i);`.
- In `render_with_scope` replace the last two lines with `self.renderer.draw_with_def(display, &self.frame(perf, scope));`, and add:

```rust
    /// What one frame draws from.
    fn frame<'a>(&'a self, perf: &'a PerfStats, scope: &'a [f32; SCOPE_LEN]) -> renderer::Frame<'a> {
        renderer::Frame {
            nav: &self.nav,
            def: self.nav.active_block_def(),
            perf,
            matrix: &self.matrix_state,
            sel_op: self.sel_op,
            focus: self.focused_slot(),
            scope,
        }
    }
```

- In `render_dirty_with_scope`, replace `let sel_op = self.sel_op;` with

```rust
        let frame = renderer::Frame {
            nav: &self.nav,
            def,
            perf,
            matrix: &self.matrix_state,
            sel_op: self.sel_op,
            focus: self.focus.get(def.id),
            scope,
        };
```

  (built from fields, not `self.frame`, because the loop borrows `self.region_set` mutably) and the draw call with `self.renderer.draw_region_with_def(display, r.kind, &frame);`.

`chimera-core/src/ui/renderer.rs`:
- Add above `pub struct Renderer`:

```rust
/// Everything one frame draws from, besides the renderer's own animation.
pub struct Frame<'a> {
    pub nav: &'a ChainNav,
    pub def: &'static BlockDef,
    pub perf: &'a PerfStats,
    pub matrix: &'a MatrixState,
    pub sel_op: Op,
    /// Slot the focus band shows: the last one touched on this page.
    pub focus: usize,
    /// Live output (the oscilloscope buffer).
    pub scope: &'a [f32; crate::scope::SCOPE_LEN],
}
```

- Delete the `focused` field (and `focused: 0,` in `new`).
- `draw_params_from_def` and `draw_cell_grid_from_def` get a `focus: usize` parameter after `def`; inside, `self.focused` → `focus`.
- `draw_with_def` becomes `pub fn draw_with_def<D>(&self, display: &mut D, f: &Frame)` (drop the `too_many_arguments` allow) starting with `let (nav, def, matrix_state, sel_op) = (f.nav, f.def, f.matrix, f.sel_op);`, passing `f.focus` to the two functions above, `f.scope` to `draw_scope`, `f.perf` to `draw_perf`. `draw_region_with_def` becomes `pub fn draw_region_with_def<D>(&self, display: &mut D, kind: RegionKind, f: &Frame)` starting with `let (nav, def, perf, matrix_state, sel_op) = (f.nav, f.def, f.perf, f.matrix, f.sel_op);` and passing `f.focus` likewise. Bodies otherwise unchanged.

`chimera-core/tests/mixer_page_test.rs`, in `part_viz_reads_the_level_and_pan_slots`, replace the `draw_region_with_def` call with:

```rust
    let scope = [0.0; chimera_core::scope::SCOPE_LEN];
    let frame = chimera_core::ui::renderer::Frame {
        nav: &nav, def: &reg::PART, perf: &PerfStats::zero(), matrix: &matrix, sel_op: Op::A, focus: 0, scope: &scope,
    };
    r.draw_region_with_def(&mut fb, RegionKind::Viz, &frame);
```

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test focus_test --test preset_test --test mixer_page_test`
Expected: all pass (`preset_test::priming_on_main_page_registers_focused_param` proves priming still follows the focus).

- [ ] **Step 5: `just check`, then commit**

Expected: core+hal 458 passed.

```bash
git add chimera-core/src/ui/focus.rs chimera-core/src/ui/mod.rs chimera-core/src/ui/renderer.rs chimera-core/tests/focus_test.rs chimera-core/tests/mixer_page_test.rs
git commit -m "feat(core): remember the last-touched slot per page; one Frame for the renderer

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Pan shows as L/C/R; `ValFmt::is_discrete`

**Files:**
- Modify: `chimera-core/src/block.rs` (`ValFmt`), `chimera-core/src/ui/fmt.rs`, `chimera-core/src/part.rs:99`, `chimera-core/src/params.rs:473`
- Test: `chimera-core/tests/ui_test.rs`

**Interfaces:**
- Produces: `ValFmt::Pan` (bipolar, `Bi` snap points, shown `L64`…`C`…`R63`), `ValFmt::is_discrete(self) -> bool` (`Int | OneBased | Names`). `PART_SPECS[4]` (PAN) and `OUT_SPECS[1]` (PAN) use `ValFmt::Pan`.

- [ ] **Step 1: Write the failing tests** — append to `chimera-core/tests/ui_test.rs`:

```rust

// -- Pan shows as L / C / R (UI refresh spec § Principles) --

#[test]
fn test_fmt_pan_left_centre_right() {
    for (v, want) in [(0.0, "L64"), (0.25, "L32"), (0.5, "C"), (0.75, "R31"), (1.0, "R63")] {
        let mut buf = FmtBuf::new();
        fmt_val(&mut buf, v, ValFmt::Pan);
        assert_eq!(buf.as_str(), want, "{v}");
    }
}

#[test]
fn test_pan_snaps_and_bipolar_like_bi() {
    assert!(ValFmt::Pan.is_bipolar());
    assert_eq!(ValFmt::Pan.snap_points(), ValFmt::Bi.snap_points());
    assert!(!ValFmt::Pan.is_discrete());
}

#[test]
fn test_discrete_formats_are_choices() {
    assert!(ValFmt::Int(7).is_discrete());
    assert!(ValFmt::OneBased(15).is_discrete());
    assert!(ValFmt::Names(&["A"]).is_discrete());
    assert!(!ValFmt::Uni.is_discrete() && !ValFmt::Bi.is_discrete());
}

#[test]
fn test_part_and_out_pan_use_the_pan_format() {
    use chimera_core::part::PART_SPECS;
    use chimera_core::params::OUT_SPECS;
    assert_eq!(PART_SPECS[4].fmt, ValFmt::Pan);
    assert_eq!(OUT_SPECS[1].fmt, ValFmt::Pan);
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test ui_test`
Expected: compile error `no variant named Pan`.

- [ ] **Step 3: Implement**

`chimera-core/src/block.rs`, in `enum ValFmt` after `Names(...)`:

```rust
    /// Stereo position: bipolar like `Bi`, shown as `L64`..`C`..`R63`.
    Pan,
```

In `snap_points`, `ValFmt::Bi =>` becomes `ValFmt::Bi | ValFmt::Pan =>`. Replace `is_bipolar` and add `is_discrete`:

```rust
    pub fn is_bipolar(self) -> bool {
        matches!(self, ValFmt::Bi | ValFmt::Pan)
    }

    /// A choice among a few values (channel, mode, output, type): shown as
    /// text with no value bar.
    pub fn is_discrete(self) -> bool {
        matches!(self, ValFmt::Int(_) | ValFmt::OneBased(_) | ValFmt::Names(_))
    }
```

`chimera-core/src/ui/fmt.rs`, in `fmt_val` before the `ValFmt::Int(max) =>` arm:

```rust
        ValFmt::Pan => {
            let v = (val * 127.0 + 0.5) as i32 - 64;
            let _ = match v {
                0 => buf.write_str("C"),
                v if v < 0 => write!(buf, "L{}", -v),
                v => write!(buf, "R{}", v),
            };
        }
```

`chimera-core/src/part.rs:99`: `ParamSpec::continuous(4, "PAN", ValFmt::Bi,` → `ValFmt::Pan,`. `chimera-core/src/params.rs:473` (`OUT_SPECS`): `ParamSpec::continuous(1, "PAN", ValFmt::Bi,` → `ValFmt::Pan,`. Display only; audio goldens are untouched.

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test ui_test --test golden_test --test block_spec_test`
Expected: all pass.

- [ ] **Step 5: `just check`, then commit** (core+hal 462 passed)

```bash
git add chimera-core/src/block.rs chimera-core/src/ui/fmt.rs chimera-core/src/part.rs chimera-core/src/params.rs chimera-core/tests/ui_test.rs
git commit -m "feat(core): pan shows as L/C/R; ValFmt::is_discrete

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Direction A header and map

**Files:**
- Create: `chimera-core/src/ui/components.rs`
- Rewrite: `chimera-core/src/ui/dungeon_map.rs`
- Modify: `chimera-core/src/scope.rs`, `chimera-core/src/ui/region.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/mod.rs`
- Modify: `chimera-core/tests/region_tests.rs`, `chimera-core/tests/mixer_page_test.rs`
- Test: `chimera-core/tests/header_map_test.rs`

**Interfaces:**
- Consumes: `draw::*`, `theme::*`, `Frame`.
- Produces: `components::{upper(&str) -> FmtBuf, header_text(&ChainNav, &BlockDef) -> (FmtBuf, FmtBuf), header(d, context, name, sounding: bool, load_pct: u8)}`; `dungeon_map::{node_x(i, n) -> i32, draw(d, &ChainNav, branch_scroll_px)}`; `scope::{peak(&[f32; SCOPE_LEN]) -> f32, SOUNDING_PEAK}`; `Frame::sounding: bool`; `RegionData::header(chain, node, sub, load_pct: u8, sounding: bool)`; private `UiState::region_data(&self, RegionKind, &Frame) -> RegionData` used by both `prime_regions` and `render_dirty_with_scope`. `Renderer::draw_header_with_def` and `draw_perf` are replaced by a private `draw_header(d, &Frame)`.

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/header_map_test.rs`:

```rust
//! Direction A header and map (UI refresh spec § Shared components 1, 5).

mod screen;

use chimera_core::ui::chain::{ChainId, ChainNav};
use chimera_core::ui::components::{header, header_text};
use chimera_core::ui::dungeon_map::{self, node_x};
use chimera_core::ui::theme;
use screen::Fb;

fn texts(nav: &ChainNav) -> (String, String) {
    let (c, n) = header_text(nav, nav.active_block_def());
    (c.as_str().to_string(), n.as_str().to_string())
}

#[test]
fn header_names_context_and_page() {
    let mut nav = ChainNav::new();
    assert_eq!(texts(&nav), ("PART 1".into(), "PIZZA".into()));
    nav.node = 2;
    assert_eq!(texts(&nav), ("PART 1".into(), "FILTER".into()));
    nav.chain_id = ChainId::Mixer(1);
    nav.node = 0;
    assert_eq!(texts(&nav), ("MIXER".into(), "PART 2".into()));
    nav.node = 1;
    assert_eq!(texts(&nav), ("MIXER".into(), "SENDS 2".into()));
    nav.node = 2;
    assert_eq!(texts(&nav), ("MIXER".into(), "CHORUS".into()), "shared FX are not numbered");
    nav.chain_id = ChainId::System;
    nav.node = 0;
    assert_eq!(texts(&nav), ("SYSTEM".into(), "MIDI SETUP".into()));
}

#[test]
fn header_dot_shows_only_while_sounding_and_stays_in_the_band() {
    for sounding in [false, true] {
        let mut fb = Fb::new();
        header(&mut fb, "PART 1", "PIZZA", sounding, 0);
        assert_eq!(fb.at(theme::HEADER_DOT_X, theme::HEADER_DOT_Y) == theme::ACCENT, sounding);
        assert!(fb.px[theme::HEADER_BOTTOM as usize * 240..].iter().all(|&p| p == 0), "nothing below y 28");
        assert_eq!(fb.oob, 0);
    }
}

#[test]
fn header_shows_audio_load_in_warning_colours() {
    let mut fb = Fb::new();
    header(&mut fb, "PART 1", "PIZZA", false, 85);
    let alert = (0..28).flat_map(|y| (120..225).map(move |x| (x, y))).any(|(x, y)| fb.at(x, y) == theme::ALERT);
    assert!(alert, "85 % is drawn in the alert colour");
}

/// Every page's header text fits left of the load readout and the dot (and
/// inside FmtBuf's 32 bytes).
#[test]
fn every_header_fits() {
    use chimera_core::ui::draw::text_width;
    for chain_id in [ChainId::Part(5), ChainId::Mixer(5), ChainId::System, ChainId::Demo] {
        let mut nav = ChainNav::new();
        nav.chain_id = chain_id;
        for node in 0..nav.active_chain().len() {
            nav.node = node;
            let subs = nav.active_chain_block().map_or(0, |b| b.sub_page_count()).max(1);
            for sub in 0..subs {
                nav.sub_page = sub;
                let def = nav.active_block_def();
                let (c, n) = header_text(&nav, def);
                assert_eq!(n.as_str().len(), def.name.len() + if n.as_str().ends_with(" 6") { 2 } else { 0 }, "{}", def.name);
                let w = theme::MARGIN_X + text_width(&theme::FONT_LABEL, c.as_str(), 1) + 7 + text_width(&theme::FONT_LABEL_BOLD, n.as_str(), 1);
                assert!(w < theme::HEADER_DOT_X - 40, "{} / {}: {w}", c.as_str(), n.as_str());
            }
        }
    }
}

#[test]
fn nodes_spread_over_the_line_and_a_single_node_is_centred() {
    assert_eq!(node_x(0, 5), theme::MAP_X0);
    assert_eq!(node_x(4, 5), theme::MAP_X1);
    assert_eq!(node_x(2, 5), 120);
    assert_eq!(node_x(0, 1), 120, "no division by zero");
}

#[test]
fn current_block_is_an_accent_pill_others_are_rings() {
    let mut nav = ChainNav::new();
    nav.node = 2; // FLT of PIZ DRV FLT FLD MOD
    let mut fb = Fb::new();
    dungeon_map::draw(&mut fb, &nav, 0);
    let (pill, other) = (node_x(2, 5), node_x(0, 5));
    assert_eq!(fb.at(pill - 14, theme::MAP_LINE_Y), theme::ACCENT, "pill body");
    assert_eq!(fb.at(other + theme::NODE_R, theme::MAP_LINE_Y), theme::MID, "ring edge");
    assert_eq!(fb.at(other, theme::MAP_LINE_Y), theme::BG, "ring is hollow");
    let label = (theme::NODE_LABEL_Y - 7..=theme::NODE_LABEL_Y).any(|y| (other - 8..other + 8).any(|x| fb.at(x, y) == theme::MID));
    assert!(label, "grey label under a ring");
}

#[test]
fn sub_pages_hang_under_the_pill_with_the_current_one_lit() {
    let mut nav = ChainNav::new();
    nav.node = 4; // MOD: MOD, ENV, LFO
    nav.sub_page = 1;
    let mut fb = Fb::new();
    dungeon_map::draw(&mut fb, &nav, 0);
    let x = node_x(4, 5) - 8;
    let lit_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x, lit_row), theme::ACCENT, "ENV lit");
    let first_row = theme::BRANCH_START_Y + theme::BRANCH_LINE_HEIGHT / 2;
    assert_eq!(fb.at(x - 2, first_row), theme::MID, "MOD ring");
}

#[test]
fn the_map_draws_only_in_its_band_on_every_chain() {
    for chain_id in [ChainId::Part(0), ChainId::Mixer(0), ChainId::System, ChainId::Demo] {
        let n = { let mut nav = ChainNav::new(); nav.chain_id = chain_id; nav.active_chain().len() };
        for node in 0..n {
            let mut nav = ChainNav::new();
            nav.chain_id = chain_id;
            nav.node = node;
            let subs = nav.active_chain_block().map_or(0, |b| b.sub_page_count());
            for sub in 0..subs.max(1) {
                nav.sub_page = sub;
                let mut fb = Fb::new();
                dungeon_map::draw(&mut fb, &nav, 0);
                assert!(fb.px[..theme::MAP_TOP as usize * 240].iter().all(|&p| p == 0), "{chain_id:?} {node} {sub}");
                assert_eq!(fb.oob, 0, "{chain_id:?} {node} {sub}");
            }
        }
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test header_map_test`
Expected: compile error `unresolved import chimera_core::ui::components`.

- [ ] **Step 3: Implement**

Create `chimera-core/src/ui/components.rs`:

```rust
//! Direction A shared components (UI refresh spec § Shared components):
//! header, focus band, cells. The map lives in `dungeon_map`.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::addr::BlockRef;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::chain::{ChainId, ChainNav};
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::theme;

/// `s` in upper case (names are stored mixed case: "Filter", "4opFM").
pub fn upper(s: &str) -> FmtBuf {
    let mut buf = FmtBuf::new();
    for ch in s.chars() {
        let _ = buf.write_char(ch.to_ascii_uppercase());
    }
    buf
}

/// Whether any slot of `def` edits the Part's own mix settings (PART, SENDS).
fn edits_part(def: &BlockDef) -> bool {
    def.params.iter().any(|s| matches!(s.binding, SlotBinding::Param(a) if a.block == BlockRef::Part))
}

/// Header context label and page name: `PART 1` `FILTER`; on the Mixer
/// chain `MIXER` and the page, numbered when it edits that Part (`PART 2`,
/// `SENDS 2`; the FX are shared, so `CHORUS`).
pub fn header_text(nav: &ChainNav, def: &BlockDef) -> (FmtBuf, FmtBuf) {
    let mut context = FmtBuf::new();
    let mut name = upper(def.name);
    let _ = match nav.chain_id {
        ChainId::Part(n) => write!(context, "PART {}", n + 1),
        ChainId::Mixer(n) => {
            if edits_part(def) {
                let _ = write!(name, " {}", n + 1);
            }
            context.write_str("MIXER")
        }
        ChainId::System => context.write_str("SYSTEM"),
        ChainId::Demo => context.write_str("DEMO"),
    };
    (context, name)
}

/// Header band (y 0..28): grey context label, bold name, the audio load
/// when measured, and an accent dot while the instrument is sounding.
pub fn header<D>(d: &mut D, context: &str, name: &str, sounding: bool, load_pct: u8)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let x = theme::MARGIN_X
        + draw::text_tracked(d, &theme::FONT_LABEL, context, theme::MARGIN_X, y, theme::MID, theme::LABEL_TRACKING);
    draw::text_tracked(d, &theme::FONT_LABEL_BOLD, name, x + 7, y, theme::INK, theme::LABEL_TRACKING);
    if load_pct > 0 {
        let mut buf = FmtBuf::new();
        let _ = write!(buf, "CPU {}%", load_pct);
        let color = match load_pct {
            81.. => theme::ALERT,
            61..=80 => theme::WARN,
            _ => theme::MID,
        };
        draw::text_right(d, &theme::FONT_LABEL, buf.as_str(), theme::HEADER_DOT_X - 8, y, color, 0);
    }
    if sounding {
        draw::dot(d, theme::HEADER_DOT_X, theme::HEADER_DOT_Y, theme::HEADER_DOT_R, theme::ACCENT);
    }
}
```

Replace `chimera-core/src/ui/dungeon_map.rs` entirely:

```rust
//! The chain map (y 266..320, Direction A): nodes on a thin line, the
//! current block a filled accent pill with a dark label, the others a small
//! ring with a grey label below. A block's sub-pages hang under the pill as
//! indented nodes, the current one lit.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::ui::chain::ChainNav;
use crate::ui::draw;
use crate::ui::theme;

/// Centre x of node `i` of `n`, spread evenly over the map line.
pub fn node_x(i: usize, n: usize) -> i32 {
    if n <= 1 {
        theme::SCREEN_W / 2
    } else {
        theme::MAP_X0 + (theme::MAP_X1 - theme::MAP_X0) * i as i32 / (n as i32 - 1)
    }
}

/// Draw the map. `branch_scroll_px` scrolls the sub-page list (animated).
pub fn draw<D>(d: &mut D, nav: &ChainNav, branch_scroll_px: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let chain = nav.active_chain();
    let n = chain.blocks.len();
    if n > 1 {
        draw::fill_rect(d, theme::MAP_X0, theme::MAP_LINE_Y, theme::MAP_X1 - theme::MAP_X0, 1, theme::FAINT);
    }
    for (i, block) in chain.blocks.iter().enumerate() {
        let x = node_x(i, n);
        if i == nav.node {
            let top = theme::MAP_LINE_Y - theme::PILL_H / 2;
            draw::pill(d, x - theme::PILL_W / 2, top, theme::PILL_W, theme::PILL_H, theme::ACCENT);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, block.def.short, x, theme::PILL_LABEL_Y, theme::BG, 0);
        } else {
            draw::dot(d, x, theme::MAP_LINE_Y, theme::NODE_R, theme::BG);
            draw::ring(d, x, theme::MAP_LINE_Y, theme::NODE_R, theme::MID, 1);
            draw::text_center(d, &theme::FONT_LABEL, block.def.short, x, theme::NODE_LABEL_Y, theme::MID, 0);
        }
    }
    draw_branches(d, nav, node_x(nav.node, n), branch_scroll_px);
}

/// Sub-pages of the current block, under its pill.
fn draw_branches<D>(d: &mut D, nav: &ChainNav, pill_x: i32, branch_scroll_px: i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let Some(block) = nav.active_chain_block() else { return };
    let count = block.sub_page_count();
    if count == 0 {
        return;
    }
    let x = pill_x - 8;
    let pill_bottom = theme::MAP_LINE_Y + theme::PILL_H / 2;
    let mut last_y = pill_bottom;
    for i in 0..count {
        let y = theme::BRANCH_START_Y + i as i32 * theme::BRANCH_LINE_HEIGHT - branch_scroll_px;
        if y < theme::BRANCH_START_Y {
            continue;
        }
        if y + theme::BRANCH_LINE_HEIGHT > theme::SCREEN_H {
            break;
        }
        let label = if i == 0 { block.def.short } else { block.sub_pages[i - 1].short };
        let cy = y + theme::BRANCH_LINE_HEIGHT / 2;
        if i == nav.sub_page {
            draw::dot(d, x, cy, 2, theme::ACCENT);
            draw::text(d, &theme::FONT_LABEL, label, x + 6, y + 8, theme::ACCENT);
        } else {
            draw::ring(d, x, cy, 2, theme::MID, 1);
            draw::text(d, &theme::FONT_LABEL, label, x + 6, y + 8, theme::MID);
        }
        last_y = cy - 3;
    }
    draw::fill_rect(d, x, pill_bottom, 1, last_y - pill_bottom, theme::FAINT);
}
```

Append to `chimera-core/src/scope.rs`:

```rust

/// Largest |sample| in a scope buffer.
pub fn peak(buf: &[f32; SCOPE_LEN]) -> f32 {
    buf.iter().fold(0.0f32, |m, &s| m.max(if s < 0.0 { -s } else { s }))
}

/// Below this peak the output counts as silent (the header dot is off).
pub const SOUNDING_PEAK: f32 = 1.0e-3;
```

`chimera-core/src/ui/region.rs`: in `RegionData::Header` replace `render_us: u32,` with

```rust
        /// Audio load shown in the header (0 = not measured).
        load_pct: u8,
        sounding: bool,
```

and update its constructors:

```rust
    pub fn header(chain_idx: u8, node_idx: u8, sub_page: u8, load_pct: u8, sounding: bool) -> Self {
        Self::Header { chain_idx, node_idx, sub_page, load_pct, sounding }
    }
```

```rust
    pub fn sentinel_header() -> Self {
        Self::Header { chain_idx: 255, node_idx: 255, sub_page: 255, load_pct: u8::MAX, sounding: false }
    }
```

`chimera-core/src/ui/renderer.rs`:
- `use crate::ui::components;` after the `animation` import.
- `Frame` gets, after `scope`: `/// The live output is above silence (header dot).\n    pub sounding: bool,`.
- Delete `draw_header_with_def` and `draw_perf` (the whole `// ── Perf overlay` section) and add:

```rust
    /// Header band: context, page name, audio load, sounding dot.
    fn draw_header<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (context, name) = components::header_text(f.nav, f.def);
        components::header(display, context.as_str(), name.as_str(), f.sounding, f.perf.audio_load_pct);
    }
```

- In `draw_with_def`: `self.draw_header_with_def(display, nav, def);` → `self.draw_header(display, f);` and remove the trailing `self.draw_perf(display, f.perf);`. In `draw_region_with_def`: the `Header` arm becomes `RegionKind::Header => self.draw_header(display, f),`; in the `Grid` arm the two header lines become `self.draw_header(display, f);`; the destructuring drops `perf`: `let (nav, def, matrix_state, sel_op) = (f.nav, f.def, f.matrix, f.sel_op);`.

`chimera-core/src/ui/mod.rs`:
- `pub mod components;` after `pub mod chain;`.
- In `frame()` add `sounding: crate::scope::peak(scope) > crate::scope::SOUNDING_PEAK,` after `scope,`.
- Add after `frame()`:

```rust
    /// Snapshot of what region `kind` shows; a region redraws when it changes.
    fn region_data(&self, kind: region::RegionKind, f: &renderer::Frame) -> region::RegionData {
        use region::{RegionData, RegionKind};
        let qvalues = region::quantize_values(&self.renderer.anim);
        let (chain, node, sub) = nav_tag(&self.nav);
        match kind {
            RegionKind::Header => RegionData::header(chain, node, sub, f.perf.audio_load_pct, f.sounding),
            RegionKind::Viz => RegionData::viz(self.page, qvalues),
            RegionKind::Params => RegionData::params(self.page, qvalues),
            RegionKind::Cells => RegionData::cells(self.page, qvalues, self.matrix_state.num_dests as u16),
            RegionKind::Nav => RegionData::nav(chain, node, sub, region::quantize(self.renderer.branch_scroll.current())),
            RegionKind::Grid => RegionData::grid_with_amount(
                self.matrix_state.sel_row as u8,
                self.matrix_state.sel_col as u8,
                self.matrix_state.scroll_x as u8,
                self.matrix_state.scroll_y as u8,
                self.matrix_state.current_amount(),
            ),
        }
    }
```

- Replace `prime_regions` with:

```rust
    /// Prime the region set after an initial full render, so render_dirty
    /// won't redundantly redraw everything on the first call.
    pub fn prime_regions(&mut self, perf: &PerfStats) {
        let mut scope = [0.0f32; SCOPE_LEN];
        crate::scope::read_samples(&mut scope);
        self.region_set.set_layout(self.nav.active_block_def().layout);
        let mut data = [region::RegionData::sentinel_header(); region::MAX_REGIONS];
        {
            let f = self.frame(perf, &scope);
            for (d, r) in data.iter_mut().zip(self.region_set.active_regions()) {
                *d = self.region_data(r.kind, &f);
            }
        }
        for (r, d) in self.region_set.active_regions_mut().iter_mut().zip(data) {
            r.prev_data = d;
        }
    }
```

- In `render_dirty_with_scope` delete `use region::{RegionData, RegionKind};` and replace everything from `let def = self.nav.active_block_def();` up to (not including) `// Scope strip — always redraws` with:

```rust
        let layout = self.nav.active_block_def().layout;
        let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
        let mut flush_count = 0;

        // Rebuild regions if layout changed
        if self.region_set.prev_layout != Some(layout) {
            self.region_set.set_layout(layout);
        }

        let count = self.region_set.count as usize;
        let mut data = [region::RegionData::sentinel_header(); region::MAX_REGIONS];
        {
            let f = self.frame(perf, scope);
            for i in 0..count {
                let r = self.region_set.regions[i];
                data[i] = self.region_data(r.kind, &f);
                if data[i] != r.prev_data {
                    renderer::Renderer::clear_region_fb(display.pixel_buffer(), r.y_start, r.y_end);
                    self.renderer.draw_region_with_def(display, r.kind, &f);
                    flush_list[flush_count] = (r.y_start, r.y_end);
                    flush_count += 1;
                }
            }
        }
        for (r, d) in self.region_set.regions[..count].iter_mut().zip(data) {
            r.prev_data = d;
        }

```

  (Snapshots are computed and compared while `self` is only borrowed immutably; `prev_data` is written after the frame is dropped.)

Tests: in `chimera-core/tests/region_tests.rs` every `RegionData::header(a, b, c, 0)` becomes `RegionData::header(a, b, c, 0, false)` (`sed -i 's/RegionData::header(\([0-9]\), \([0-9]\), \([0-9]\), 0)/RegionData::header(\1, \2, \3, 0, false)/g'`). In `mixer_page_test.rs` the `Frame` literal gains `sounding: false,`.

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test header_map_test --test region_tests --test screen_golden_test`
Expected: all pass. Look: `SCREEN_DUMP=/tmp/t5 cargo test -p chimera-core --test screen_golden_test`, convert `engine_pizza.ppm` — header and map now match the mockup (`aHeader`, `aMap`); the middle is still legacy.

- [ ] **Step 5: `just check`, then commit** (core+hal 470 passed)

```bash
git add chimera-core/src/ui/components.rs chimera-core/src/ui/dungeon_map.rs chimera-core/src/scope.rs chimera-core/src/ui/region.rs chimera-core/src/ui/renderer.rs chimera-core/src/ui/mod.rs chimera-core/tests/header_map_test.rs chimera-core/tests/region_tests.rs chimera-core/tests/mixer_page_test.rs
git commit -m "feat(core): Direction A header and map

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: CellGrid pages — focus band, live output, cells; the scope strip goes

This converts every `PageLayout::CellGrid` page (engine pages, System, Demo, Mixer PART/SENDS/FX, FM ALG/OP). Their viz band shows live output for now; Tasks 8–10 give PART, the FX pages and the FM pages their own band, so only `engine_pizza` and `system` are locked here.

**Files:**
- Modify: `chimera-core/src/ui/components.rs` (focus band, cell), `chimera-core/src/ui/region.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/mod.rs`
- Create: `chimera-core/src/ui/viz.rs`
- Modify tests: `region_tests.rs`, `mixer_page_test.rs` (delete one test), `screen_golden_test.rs`
- Test: `chimera-core/tests/cell_grid_test.rs`

**Interfaces:**
- Consumes: `Frame`, `FocusMemory`, `ValFmt::is_discrete`, `scope::peak`.
- Produces: `components::{focus_band(d, label, value_text, value: f32, bipolar: bool), Cell<'a> { label, text, value, fmt, active, mod_amount }, cell(d, i, top, Option<&Cell>)}`; `viz::{LIVE_COLS, live_columns(&[f32; SCOPE_LEN]) -> [i8; LIVE_COLS], live_key(..) -> u32, live_output(d, ..)}`; `RegionKind::Focus`; `RegionData::{Focus { page, slot, value }, focus(..)}`; `RegionData::viz(page, values, live: u32)`; `RegionData::cells(page, values, focus: u8, dest_count)`; `region::layout_regions(PageLayout) -> &'static [(RegionKind, u16, u16)]`; `Renderer::viz_inputs(&self, &Frame) -> ([u16; 6], u32)`. `Renderer::draw_with_def` = clear + every region of the layout.

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/cell_grid_test.rs`:

```rust
//! CellGrid pages in Direction A (UI refresh spec § Page types, § Testing):
//! header · focus band · live output · cells · map.

mod screen;

use chimera_core::ui::components::{self, Cell};
use chimera_core::ui::fmt::{fmt_val, FmtBuf};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz;
use chimera_core::ui::UiState;
use chimera_hal::EncoderId;
use screen::*;

fn band(fb: &Fb, y0: i32, y1: i32) -> Vec<u16> {
    fb.px[y0 as usize * W..y1 as usize * W].to_vec()
}

#[test]
fn dirty_render_from_scratch_equals_full_render() {
    for name in ["engine_pizza", "system"] {
        assert!(render(name).px == render_dirty(name).px, "{name}");
    }
}

#[test]
fn a_settled_silent_or_frozen_screen_flushes_nothing() {
    let mut ui = ui_for("engine_pizza");
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    let second = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    assert!(second.iter().all(|&(a, b)| a == b), "{second:?}");
}

#[test]
fn a_turn_redraws_focus_and_cells_only() {
    let mut ui = ui_for("engine_pizza");
    let mut fb = Fb::new();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    feed(&mut ui, Input::turn(EncoderId::C, -3));
    ui.update();
    let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope);
    let bands: Vec<(u16, u16)> = flushed.into_iter().filter(|&(a, b)| a != b).collect();
    assert_eq!(bands, [(28, 118), (186, 266)]);
}

#[test]
fn new_live_output_redraws_only_the_viz_band() {
    let mut ui = ui_for("engine_pizza");
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let quieter = scope_fixture().map(|s| if s > 0.0 { s * 0.5 } else { s });
    let flushed = ui.render_dirty_with_scope(&mut fb, &PerfStats::zero(), &quieter);
    let bands: Vec<(u16, u16)> = flushed.into_iter().filter(|&(a, b)| a != b).collect();
    assert_eq!(bands, [(118, 186)]);
}

#[test]
fn focus_band_shows_the_last_touched_slot() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::turn(EncoderId::C, -5)); // LEVEL
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let v = ui.renderer.anim[2].current();
    let mut text = FmtBuf::new();
    fmt_val(&mut text, v, ValFmt::Uni);
    let mut want = Fb::new();
    want.px.fill(fb.px[0]); // ground
    components::focus_band(&mut want, "LEVEL", text.as_str(), v, false);
    assert!(band(&fb, 28, 118) == band(&want, 28, 118), "focus band is LEVEL {}", text.as_str());
}

/// The focus value lerps toward the new value (CLAUDE.md: never snap).
#[test]
fn the_focus_value_animates_toward_its_target() {
    let mut ui = ui_for("engine_pizza");
    let before = ui.renderer.anim[0].current();
    feed(&mut ui, Input::turn(EncoderId::A, 40));
    ui.update();
    let (now, target) = (ui.renderer.anim[0].current(), ui.renderer.anim[0].target());
    assert!(before < now && now < target, "{before} < {now} < {target}");
}

#[test]
fn only_the_focused_cell_label_uses_the_accent() {
    let fb = render("engine_pizza"); // focus SHAPE (slot a)
    let accent_in = |x0: i32| {
        (theme::CELL_LABEL_Y - 8..=theme::CELL_LABEL_Y).any(|y| (x0..x0 + 60).any(|x| fb.at(x, y) == theme::ACCENT))
    };
    assert!(accent_in(theme::MARGIN_X));
    assert!(!accent_in(theme::MARGIN_X + theme::CELL_COL_W));
    assert!(!accent_in(theme::MARGIN_X + 2 * theme::CELL_COL_W));
}

#[test]
fn empty_slots_are_a_dim_dash_and_choices_have_no_bar() {
    let mut fb = Fb::new();
    components::cell(&mut fb, 0, 200, None);
    assert_eq!(fb.at(theme::MARGIN_X + 3, 197), theme::FAINT);
    let c = Cell { label: "MODE", text: "POLY", value: 1.0, fmt: ValFmt::Names(&["MONO", "POLY"]), active: false, mod_amount: None };
    let mut fb = Fb::new();
    components::cell(&mut fb, 1, 200, Some(&c));
    let bar_y = 200 + theme::CELL_BAR_DY;
    let x = theme::MARGIN_X + theme::CELL_COL_W;
    assert!((x..x + theme::CELL_BAR_W).all(|x| fb.at(x, bar_y) != theme::FAINT), "no track under a choice");
}

#[test]
fn live_output_is_flat_when_silent_and_scaled_to_the_band() {
    let silent = [0.0; chimera_core::scope::SCOPE_LEN];
    assert!(viz::live_columns(&silent).iter().all(|&c| c == 0));
    let cols = viz::live_columns(&scope_fixture());
    assert_eq!(cols.iter().max(), Some(&(theme::VIZ_BAND_AMP as i8)));
    let mut fb = Fb::new();
    viz::live_output(&mut fb, &scope_fixture());
    for (i, row) in fb.px.chunks(W).enumerate() {
        let y = i as i32;
        if !(theme::VIZ_BAND_TOP..theme::VIZ_BAND_BOTTOM).contains(&y) {
            assert!(row.iter().all(|&p| p == 0), "row {y} outside the band");
        }
    }
}

/// A page whose slots are all empty (System UPDATES) shows no focus band.
#[test]
fn an_all_empty_page_has_an_empty_focus_band() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(chimera_hal::ButtonId::Menu));
    for _ in 0..3 {
        feed(&mut ui, Input::press(chimera_hal::ButtonId::Plus)); // → UPDATES
    }
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let ground = fb.px[0];
    assert!(band(&fb, 28, 118).iter().all(|&p| p == ground));
    assert_eq!(fb.oob, 0);
}

/// Garbage in the scope buffer (NaN, ±inf) draws a flat line, in the band.
#[test]
fn non_finite_live_output_is_flat() {
    let mut buf = scope_fixture();
    buf[3] = f32::NAN;
    buf[9] = f32::INFINITY;
    buf[20] = f32::NEG_INFINITY;
    let cols = viz::live_columns(&buf);
    assert!(cols.iter().all(|&c| c.abs() <= theme::VIZ_BAND_AMP as i8));
    let mut fb = Fb::new();
    viz::live_output(&mut fb, &buf);
    assert_eq!(fb.oob, 0);
}
```

Append to `chimera-core/tests/screen_golden_test.rs`:

```rust

#[test]
fn no_screen_draws_outside_240x320() {
    for &(name, _) in GOLDENS {
        assert_eq!(render(name).oob, 0, "{name}");
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test cell_grid_test`
Expected: compile errors (`no function focus_band`, `unresolved import chimera_core::ui::viz`).

- [ ] **Step 3: Implement**

Append to `chimera-core/src/ui/components.rs`:

```rust

/// Focus band (y 28..118): the focused slot's label, its value large, and an
/// arc gauge (from 12:00 for bipolar params). `value` is the animated 0..1.
pub fn focus_band<D>(d: &mut D, label: &str, value_text: &str, value: f32, bipolar: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    focus_label(d, label, theme::MARGIN_X);
    focus_value(d, value_text, value, bipolar);
}

/// Focus label at `x`; returns where it ends.
fn focus_label<D>(d: &mut D, label: &str, x: i32) -> i32
where
    D: DrawTarget<Color = Rgb565>,
{
    x + draw::text_tracked(d, &theme::FONT_VALUE, label, x, theme::FOCUS_LABEL_Y, theme::MID, theme::LABEL_TRACKING)
}

fn focus_value<D>(d: &mut D, value_text: &str, value: f32, bipolar: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::text(d, &theme::FONT_FOCUS, value_text, theme::FOCUS_VALUE_X, theme::FOCUS_VALUE_Y, theme::INK);
    draw::arc_gauge(d, theme::ARC_CX, theme::ARC_CY, theme::ARC_R, theme::ARC_WIDTH, value, bipolar, theme::FAINT, theme::ACCENT);
}

/// One cell of the 3×2 grid.
pub struct Cell<'a> {
    pub label: &'a str,
    /// Formatted value.
    pub text: &'a str,
    /// Animated 0..1 value for the bar.
    pub value: f32,
    pub fmt: crate::block::ValFmt,
    /// The focused slot: accent label and bar.
    pub active: bool,
    /// Summed mod amount (−1..1) when the param is a mod destination.
    pub mod_amount: Option<f32>,
}

/// Draw cell `i` (knob order a–f, 3×2) with its label baseline `top + row·36`.
/// `None` is an empty slot: a dim dash.
pub fn cell<D>(d: &mut D, i: usize, top: i32, cell: Option<&Cell>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = theme::MARGIN_X + (i % 3) as i32 * theme::CELL_COL_W;
    let y = top + (i / 3) as i32 * theme::CELL_ROW_H;
    let Some(c) = cell else {
        draw::fill_rect(d, x, y - 3, 8, 1, theme::FAINT);
        return;
    };
    let label_color = if c.active { theme::ACCENT } else { theme::MID };
    draw::text_tracked(d, &theme::FONT_LABEL, c.label, x, y, label_color, theme::LABEL_TRACKING);
    let value_color = if c.active { theme::INK } else { theme::INK2 };
    draw::text(d, &theme::FONT_VALUE, c.text, x, y + theme::CELL_VALUE_DY, value_color);
    if !c.fmt.is_discrete() {
        let fill = if c.active { theme::ACCENT } else { theme::BAR_REST };
        draw::bar(d, x, y + theme::CELL_BAR_DY, theme::CELL_BAR_W, theme::CELL_BAR_H, c.value, c.fmt.is_bipolar(), theme::FAINT, fill);
    }
    if let Some(m) = c.mod_amount {
        let mid = x + theme::CELL_BAR_W / 2;
        let len = (m.clamp(-1.0, 1.0) * (theme::CELL_BAR_W / 2) as f32) as i32;
        let (x0, x1) = if len >= 0 { (mid, mid + len) } else { (mid + len, mid) };
        draw::fill_rect(d, mid, y + theme::CELL_MOD_DY - 1, 1, 3, theme::MID);
        draw::fill_rect(d, x0, y + theme::CELL_MOD_DY, (x1 - x0).max(1), 1, theme::INK2);
    }
}
```

Create `chimera-core/src/ui/viz.rs` (later tasks append to it):

```rust
//! Direction A visualizations (ADR 0016): drawn as the main element with a
//! soft accent fill under a 1-px accent line; no grid lines.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::scope::{self, SCOPE_LEN};
use crate::ui::draw;
use crate::ui::theme;

/// Columns of the viz band (x 12..=228).
pub const LIVE_COLS: usize = (theme::VIZ_RIGHT - theme::VIZ_LEFT + 1) as usize;

/// Live output as pixel offsets from the band's centre line, auto-scaled to
/// ±`VIZ_BAND_AMP` (flat while silent).
pub fn live_columns(buf: &[f32; SCOPE_LEN]) -> [i8; LIVE_COLS] {
    let peak = scope::peak(buf);
    let scale = if peak > scope::SOUNDING_PEAK { theme::VIZ_BAND_AMP as f32 / peak } else { 0.0 };
    core::array::from_fn(|i| libm::roundf(buf[i] * scale) as i8)
}

/// Cheap fingerprint of what `live_output` draws: the viz region redraws
/// only when it changes (a silent or frozen scope costs no SPI traffic).
pub fn live_key(buf: &[f32; SCOPE_LEN]) -> u32 {
    live_columns(buf).iter().fold(0x811c_9dc5u32, |h, &c| (h ^ c as u8 as u32).wrapping_mul(0x0100_0193))
}

/// The page's live output as a filled waveform in the viz band.
pub fn live_output<D>(d: &mut D, buf: &[f32; SCOPE_LEN])
where
    D: DrawTarget<Color = Rgb565>,
{
    let mid = theme::VIZ_BAND_MID;
    let cols = live_columns(buf);
    let y = |i: usize| mid - cols[i] as i32;
    for (i, _) in cols.iter().enumerate() {
        let x = theme::VIZ_LEFT + i as i32;
        let (a, b) = if y(i) < mid { (y(i) + 1, mid) } else { (mid, y(i)) };
        draw::fill_rect(d, x, a, 1, b - a, theme::ACCENT_SOFT);
    }
    for i in 1..cols.len() {
        let x = theme::VIZ_LEFT + i as i32;
        draw::line(d, x - 1, y(i - 1), x, y(i), theme::ACCENT, 1);
    }
}
```

`chimera-core/src/ui/mod.rs`: `pub mod viz;` after `pub mod theme;`.

`chimera-core/src/ui/region.rs`:
- `use crate::ui::theme;` after the `page` import.
- In `RegionData`, add before `Viz`:

```rust
    /// The focus band: which slot, and its animated value.
    Focus {
        page: PageKey,
        slot: u8,
        value: u16,
    },
```

  add to `Viz` a field `/// Fingerprint of outside data the viz shows (live output).\n        live: u32,` and to `Cells` a field `focus: u8,` after `values`.
- Constructors/sentinels:

```rust
    pub fn focus(page: PageKey, slot: u8, value: u16) -> Self {
        Self::Focus { page, slot, value }
    }

    pub fn viz(page: PageKey, values: [u16; 6], live: u32) -> Self {
        Self::Viz { page, values, live }
    }
```

```rust
    pub fn cells(page: PageKey, values: [u16; 6], focus: u8, dest_count: u16) -> Self {
        Self::Cells { page, values, focus, dest_count }
    }
```

```rust
    pub fn sentinel_focus() -> Self {
        Self::Focus { page: SENTINEL_PAGE, slot: u8::MAX, value: SENTINEL }
    }

    pub fn sentinel_viz() -> Self {
        Self::Viz { page: SENTINEL_PAGE, values: [SENTINEL; 6], live: u32::MAX }
    }
```

  and `sentinel_cells` becomes `Self::Cells { page: SENTINEL_PAGE, values: [SENTINEL; 6], focus: u8::MAX, dest_count: u16::MAX }`.
- `RegionKind` gains `Focus` after `Header`.
- Replace the body of `set_layout` with the table version and add the tables at the end of the file:

```rust
    /// Rebuild the region list for a new layout. All regions start dirty (sentinel data).
    pub fn set_layout(&mut self, layout: PageLayout) {
        let bands = layout_regions(layout);
        for (r, &(kind, y_start, y_end)) in self.regions.iter_mut().zip(bands) {
            *r = Region { kind, y_start, y_end, prev_data: sentinel(kind) };
        }
        self.count = bands.len() as u8;
        self.prev_layout = Some(layout);
    }
```

```rust

use RegionKind as K;

const HEADER: u16 = theme::HEADER_BOTTOM as u16;
const FOCUS: u16 = theme::FOCUS_BOTTOM as u16;
const BAND: u16 = theme::VIZ_BAND_BOTTOM as u16;
const CELLS: u16 = theme::CELLS_BOTTOM as u16;
const SCREEN: u16 = theme::SCREEN_H as u16;

/// CellGrid (UI refresh spec § Page types): header, focus band, viz band,
/// cells, map.
const CELL_GRID: [(RegionKind, u16, u16); 5] = [
    (K::Header, 0, HEADER),
    (K::Focus, HEADER, FOCUS),
    (K::Viz, FOCUS, BAND),
    (K::Cells, BAND, CELLS),
    (K::Nav, CELLS, SCREEN),
];
const BIG_VIZ: [(RegionKind, u16, u16); 4] = [(K::Header, 0, 28), (K::Viz, 28, 170), (K::Params, 170, 266), (K::Nav, 266, 320)];
const MATRIX: [(RegionKind, u16, u16); 2] = [(K::Grid, 0, CELLS), (K::Nav, CELLS, SCREEN)];

/// The bands of `layout`, top to bottom; they tile 0..320.
pub fn layout_regions(layout: PageLayout) -> &'static [(RegionKind, u16, u16)] {
    match layout {
        PageLayout::CellGrid => &CELL_GRID,
        PageLayout::BigViz => &BIG_VIZ,
        PageLayout::Matrix => &MATRIX,
    }
}

fn sentinel(kind: RegionKind) -> RegionData {
    match kind {
        K::Header => RegionData::sentinel_header(),
        K::Focus => RegionData::sentinel_focus(),
        K::Viz => RegionData::sentinel_viz(),
        K::Params => RegionData::sentinel_params(),
        K::Cells => RegionData::sentinel_cells(),
        K::Nav => RegionData::sentinel_nav(),
        K::Grid => RegionData::sentinel_grid(),
    }
}
```

`chimera-core/src/ui/renderer.rs`:
- Imports: `use crate::ui::block_def::{slot_addr, BlockDef, SlotBinding, VizType};`, `use crate::ui::region::{self, RegionKind};`, `use crate::ui::viz;`.
- Delete `draw_cell_grid_from_def` and the whole `// ── Oscilloscope` section (`draw_scope`).
- Replace `draw_with_def` and `draw_region_with_def` with the following, and add `viz_inputs`, `draw_focus`, `draw_cells`:

```rust
    /// Render the full screen: every region of the page's layout.
    pub fn draw_with_def<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let _ = Rectangle::new(Point::zero(), Size::new(240, 320))
            .draw_styled(&PrimitiveStyle::with_fill(theme::BG), display);
        for &(kind, _, _) in region::layout_regions(f.def.layout) {
            self.draw_region_with_def(display, kind, f);
        }
    }

    /// What the page's viz is drawn from, for dirty tracking: the slot
    /// values it reads (quantized) and a fingerprint of outside data.
    pub fn viz_inputs(&self, f: &Frame) -> ([u16; 6], u32) {
        match f.def.layout {
            PageLayout::CellGrid => ([0; 6], viz::live_key(f.scope)),
            PageLayout::BigViz | PageLayout::Matrix => (region::quantize_values(&self.anim), 0),
        }
    }

    /// Draw a single region. The caller has already cleared it.
    pub fn draw_region_with_def<D>(&self, display: &mut D, kind: RegionKind, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (nav, def, matrix_state, sel_op) = (f.nav, f.def, f.matrix, f.sel_op);
        match kind {
            RegionKind::Header => self.draw_header(display, f),
            RegionKind::Focus => self.draw_focus(display, f),
            RegionKind::Viz => match def.layout {
                PageLayout::CellGrid => viz::live_output(display, f.scope),
                PageLayout::BigViz | PageLayout::Matrix => self.draw_viz_from_type(display, def),
            },
            RegionKind::Params => {
                self.draw_params_from_def(display, def, f.focus, sel_op, matrix_state);
                let _ = Line::new(
                    Point::new(0, theme::ENCODER_ZONE_BOTTOM),
                    Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
                )
                .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);
            }
            RegionKind::Cells => self.draw_cells(display, f, theme::CELL_LABEL_Y),
            RegionKind::Grid => {
                self.draw_header(display, f);
                crate::ui::mod_grid::draw_grid(display, matrix_state);
                let _ = Line::new(
                    Point::new(0, theme::ENCODER_ZONE_BOTTOM),
                    Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
                )
                .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);
            }
            RegionKind::Nav => {
                dungeon_map::draw(display, nav, (self.branch_scroll.current() * theme::BRANCH_LINE_HEIGHT as f32) as i32);
            }
        }
    }

    /// Focus band: the focused slot large (nothing for an empty slot).
    fn draw_focus<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let slot = &f.def.params[f.focus];
        if slot.binding == SlotBinding::Empty {
            return;
        }
        let v = self.anim[f.focus].current();
        let mut buf = FmtBuf::new();
        fmt::fmt_val(&mut buf, v, slot.format());
        components::focus_band(display, slot.label(), buf.as_str(), v, slot.format().is_bipolar());
    }

    /// The six cells, first row's labels at `top`.
    fn draw_cells<D>(&self, display: &mut D, f: &Frame, top: i32)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        for (i, slot) in f.def.params.iter().enumerate() {
            if slot.binding == SlotBinding::Empty {
                components::cell(display, i, top, None);
                continue;
            }
            let v = self.anim[i].current();
            let mut buf = FmtBuf::new();
            fmt::fmt_val(&mut buf, v, slot.format());
            let c = components::Cell {
                label: slot.label(),
                text: buf.as_str(),
                value: v,
                fmt: slot.format(),
                active: i == f.focus,
                mod_amount: Self::cell_mod_info(f.def, i, f.sel_op, f.matrix),
            };
            components::cell(display, i, top, Some(&c));
        }
    }
```

  (A full render is now exactly "every region drawn in turn", so full and dirty rendering cannot drift. The legacy BigViz/Matrix arms stay until Tasks 7 and 11.)

`chimera-core/src/ui/mod.rs`, `region_data`:

```rust
            RegionKind::Focus => RegionData::focus(self.page, f.focus as u8, qvalues[f.focus]),
            RegionKind::Viz => {
                let (values, live) = self.renderer.viz_inputs(f);
                RegionData::viz(self.page, values, live)
            }
            RegionKind::Params => RegionData::params(self.page, qvalues),
            RegionKind::Cells => RegionData::cells(self.page, qvalues, f.focus as u8, self.matrix_state.num_dests as u16),
```

(replacing the `Viz` and `Cells` arms), and in `render_dirty_with_scope` delete the `// Scope strip — always redraws after regions …` block (the `if layout == PageLayout::CellGrid { … }` that cleared and redrew 240..266 every frame).

Tests:
- `region_tests.rs`: `RegionData::viz(page, v)` → `RegionData::viz(page, v, 0)` (`sed -E -i 's/RegionData::viz\((page), (values\w*)\)/RegionData::viz(\1, \2, 0)/'`); rename `cell_grid_has_3_regions` → `cell_grid_has_5_regions` asserting `5`; `cell_grid_region_kinds` expects `[Header, Focus, Viz, Cells, Nav]`; in `layout_change_resets_all_regions` add the arms `RegionData::Focus { slot: u8::MAX, .. } => {}` and `RegionData::Viz { values, .. } if values == [u16::MAX; 6] => {}`.
- `mixer_page_test.rs`: delete `part_viz_reads_the_level_and_pan_slots` (the PART viz band is live output until Task 8, which adds its replacement test). Keep the `Fb` struct for now (the browser test uses it).

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test cell_grid_test --test region_tests --test screen_golden_test --test mixer_page_test`
Expected: all pass. `SCREEN_DUMP=/tmp/t6 …` and compare `engine_pizza` with the mockup's Direction A screen: same header, focus (label, big value, arc), filled waveform, three cells with thin bars, map.

- [ ] **Step 5: Lock the converted cases**

Run: `SCREEN_RECORD=1 cargo test -p chimera-core --test screen_golden_test screen_goldens_match -- --nocapture`, then in `GOLDENS` set only:

```rust
    ("engine_pizza", Locked(0x4797d6f7d4edc427)),
    ("system", Locked(0x02f7456a841f9613)),
```

(values from the validation run) and delete the `#[allow(dead_code)]` line above `enum Golden`.

- [ ] **Step 6: `just check`, then commit** (core+hal 481 passed)

```bash
git add -A chimera-core
git commit -m "feat(core): CellGrid pages in Direction A (focus band, live output, cells); drop the scope strip

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: BigViz pages — filter with riding readout, lit envelope segment

**Files:**
- Modify: `chimera-core/src/ui/viz.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/region.rs`, `chimera-core/src/ui/mod.rs`
- Modify tests: `region_tests.rs`, `screen_golden_test.rs`
- Test: `chimera-core/tests/big_viz_test.rs`

**Interfaces:**
- Consumes: `draw::*`, `Frame`, `Renderer::anim`.
- Produces: `viz::{PLOT_TOP, PLOT_BASE, FILTER_PASS_Y, filter_y(t, cutoff, reso) -> i32, readout(d, x, y, label, value), filter(d, cutoff, reso, Option<(&str, &str)>), envelope(d, &[f32; 4], &[f32; 5], &[&str; 4], lit: Option<usize>), compressor(d)}`; private `Renderer::draw_big_viz`. Removed: `RegionKind::Params`, `RegionData::{Params, params, sentinel_params}`, `Renderer::{draw_params_from_def, draw_viz_from_type}` and every pre-refresh viz function.

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/big_viz_test.rs`:

```rust
//! BigViz pages in Direction A (UI refresh spec § Page types): header ·
//! large viz with the touched value riding on it · cells · map.

mod screen;

use chimera_core::ui::fmt::{fmt_val, FmtBuf};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz::{self, FILTER_PASS_Y, PLOT_BASE, PLOT_TOP};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn band(fb: &Fb, y0: i32, y1: i32) -> Vec<u16> {
    fb.px[y0 as usize * W..y1 as usize * W].to_vec()
}

#[test]
fn dirty_render_from_scratch_equals_full_render() {
    for name in ["bigviz_filter", "bigviz_env", "bigviz_fm_op_env"] {
        assert!(render(name).px == render_dirty(name).px, "{name}");
    }
}

#[test]
fn filter_curve_keeps_its_shape() {
    assert_eq!(viz::filter_y(0.0, 0.5, 0.0), FILTER_PASS_Y, "pass band");
    assert_eq!(viz::filter_y(0.5, 0.5, 1.0), PLOT_TOP, "full resonance peaks at the top");
    assert_eq!(viz::filter_y(1.0, 0.2, 0.5), PLOT_BASE, "rolled off");
    assert!(viz::filter_y(0.5, 0.5, 0.5) < FILTER_PASS_Y);
}

/// The readout on the filter is the focused slot, not always CUTOFF.
#[test]
fn filter_readout_rides_the_focused_value() {
    let mut ui = ui_for("bigviz_filter");
    feed(&mut ui, Input::turn(EncoderId::B, -1)); // RESO
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let (cutoff, reso) = (ui.renderer.anim[0].current(), ui.renderer.anim[1].current());
    let mut text = FmtBuf::new();
    fmt_val(&mut text, reso, ValFmt::Uni);
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    viz::filter(&mut want, cutoff, reso, Some(("RESO", text.as_str())));
    assert!(band(&fb, 28, 186) == band(&want, 28, 186));
}

#[test]
fn readout_flips_left_at_the_right_edge() {
    let mut fb = Fb::new();
    viz::readout(&mut fb, 220, 100, "CUTOFF", "127");
    for y in 28..186 {
        for x in theme::VIZ_RIGHT + 1..240 {
            assert_eq!(fb.px[y as usize * W + x as usize], 0, "({x},{y})");
        }
    }
}

fn accent_in_viz(name_setup: impl FnOnce(&mut chimera_core::ui::UiState)) -> usize {
    let mut ui = ui_for("bigviz_env");
    name_setup(&mut ui);
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    (28..186).flat_map(|y| (0..240).map(move |x| (x, y))).filter(|&(x, y)| fb.at(x, y) == theme::ACCENT).count()
}

/// Envelope: the segment the focused slot edits is lit; LEVEL/VEL light none.
#[test]
fn envelope_lights_the_edited_segment() {
    assert!(accent_in_viz(|_| {}) > 0, "DEC lit");
    assert_eq!(accent_in_viz(|ui| feed(ui, Input::turn(EncoderId::E, -1))), 0, "DEPTH lights no segment");
}

#[test]
fn fm_envelope_is_reachable_and_lit() {
    let mut ui = ui_for("bigviz_fm_op_env");
    feed(&mut ui, Input::press(ButtonId::Seq)); // back up to MOD
    feed(&mut ui, Input::press(ButtonId::Edit)); // E1 again
    assert_eq!(ui.focused_slot(), 2, "focus survives leaving the page");
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test big_viz_test`
Expected: compile error `cannot find function filter_y in module viz`.

- [ ] **Step 3: Implement**

Append to `chimera-core/src/ui/viz.rs`:

```rust

/// BigViz plot area: curves between `PLOT_TOP` and `PLOT_BASE`.
pub const PLOT_TOP: i32 = 40;
pub const PLOT_BASE: i32 = 170;
/// Filter pass band (the mockup's 0 dB line); resonance peaks above it.
pub const FILTER_PASS_Y: i32 = 72;

/// Filter response y at column `t` (0..1 across the plot): flat pass band,
/// a resonance bump at `cutoff`, then roll-off to the base (the pre-refresh
/// curve's shape).
pub fn filter_y(t: f32, cutoff: f32, reso: f32) -> i32 {
    let dist = (t - cutoff) * 6.0;
    let peak_h = (FILTER_PASS_Y - PLOT_TOP) as f32 * reso;
    let y = if dist < -0.5 {
        FILTER_PASS_Y as f32
    } else if dist < 0.5 {
        let peak = libm::cosf(dist * core::f32::consts::PI) * 0.5 + 0.5;
        FILTER_PASS_Y as f32 - peak_h * peak
    } else {
        let rolloff = (dist - 0.5).min(4.0) / 4.0;
        FILTER_PASS_Y as f32 + (PLOT_BASE - FILTER_PASS_Y) as f32 * rolloff
    };
    (y as i32).clamp(PLOT_TOP, PLOT_BASE)
}

/// Columns `x0..=x1`: soft fill from the curve point `y(x)` down to `base`,
/// then the curve as a 1-px accent line.
fn filled_curve<D>(d: &mut D, x0: i32, x1: i32, base: i32, y: impl Fn(i32) -> i32)
where
    D: DrawTarget<Color = Rgb565>,
{
    for x in x0..=x1 {
        draw::fill_rect(d, x, y(x) + 1, 1, base - y(x) - 1, theme::ACCENT_SOFT);
    }
    for x in x0 + 1..=x1 {
        draw::line(d, x - 1, y(x - 1), x, y(x), theme::ACCENT, 1);
    }
}

/// The touched value riding on a viz: label above, value in the readout
/// face, to the right of (x, y) or to its left when it would not fit.
pub fn readout<D>(d: &mut D, x: i32, y: i32, label: &str, value: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = draw::text_width(&theme::FONT_READOUT, value, 0).max(draw::text_width(&theme::FONT_LABEL, label, theme::LABEL_TRACKING));
    let left = if x + 8 + w <= theme::VIZ_RIGHT { x + 8 } else { x - 8 - w };
    let vy = (y + 2).clamp(PLOT_TOP + 20, PLOT_BASE - 2);
    draw::text_tracked(d, &theme::FONT_LABEL, label, left + 1, vy - 24, theme::MID, theme::LABEL_TRACKING);
    draw::text(d, &theme::FONT_READOUT, value, left, vy, theme::INK);
}

/// Filter: response with a soft fill, a faint pass-band line, and a marker
/// at the cutoff carrying `readout` (the focused slot's label and value).
pub fn filter<D>(d: &mut D, cutoff: f32, reso: f32, readout_text: Option<(&str, &str)>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32;
    let y = |x: i32| filter_y((x - theme::VIZ_LEFT) as f32 / w, cutoff, reso);
    filled_curve(d, theme::VIZ_LEFT, theme::VIZ_RIGHT, PLOT_BASE, y);
    draw::fill_rect(d, theme::VIZ_LEFT, FILTER_PASS_Y, theme::VIZ_RIGHT - theme::VIZ_LEFT, 1, theme::FAINT);
    let mx = theme::VIZ_LEFT + (w * cutoff.clamp(0.0, 1.0)) as i32;
    let my = y(mx);
    let mut dy = my + 6;
    while dy < PLOT_BASE {
        draw::fill_rect(d, mx, dy, 1, 2.min(PLOT_BASE - dy), theme::INK);
        dy += 5;
    }
    draw::dot(d, mx, my, 4, theme::INK);
    if let Some((label, value)) = readout_text {
        readout(d, mx, my, label, value);
    }
}

/// Envelope: four segments over proportional `widths`, breakpoints at
/// `heights` (0..1), stage labels below; segment `lit` (the one the focused
/// slot edits) in the accent.
pub fn envelope<D>(d: &mut D, widths: &[f32; 4], heights: &[f32; 5], labels: &[&str; 4], lit: Option<usize>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let base = PLOT_BASE - 8;
    let (x0, w, h) = (theme::VIZ_LEFT, (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32, (base - PLOT_TOP) as f32);
    let mut pts = [(0i32, 0i32); 5];
    let mut cx = x0 as f32;
    for i in 0..5 {
        pts[i] = (cx as i32, base - (h * heights[i].clamp(0.0, 1.0)) as i32);
        if i < 4 {
            cx += w * widths[i];
        }
    }
    let y_at = |x: i32| {
        let s = (0..4).find(|&s| x <= pts[s + 1].0).unwrap_or(3);
        let ((xa, ya), (xb, yb)) = (pts[s], pts[s + 1]);
        if xb == xa { yb } else { ya + (yb - ya) * (x - xa) / (xb - xa) }
    };
    for x in x0..=pts[4].0 {
        draw::fill_rect(d, x, y_at(x) + 1, 1, base - y_at(x) - 1, theme::ACCENT_SOFT);
    }
    draw::fill_rect(d, x0, base, theme::VIZ_RIGHT - x0, 1, theme::FAINT);
    for s in 0..4 {
        let ((xa, ya), (xb, yb)) = (pts[s], pts[s + 1]);
        let (color, width) = if lit == Some(s) { (theme::ACCENT, 2) } else { (theme::INK2, 1) };
        draw::line(d, xa, ya, xb, yb, color, width);
        // A label wider than its segment is left out, unless it is the lit one.
        let fits = draw::text_width(&theme::FONT_LABEL, labels[s], theme::LABEL_TRACKING) + 2 <= xb - xa;
        if lit == Some(s) {
            draw::text_center(d, &theme::FONT_LABEL, labels[s], (xa + xb) / 2, base + 14, theme::ACCENT, theme::LABEL_TRACKING);
        } else if fits {
            draw::text_center(d, &theme::FONT_LABEL, labels[s], (xa + xb) / 2, base + 14, theme::MID, theme::LABEL_TRACKING);
        }
    }
    for (i, &(x, y)) in pts.iter().enumerate() {
        let on_lit = lit.is_some_and(|s| i == s || i == s + 1);
        draw::dot(d, x, y, 2, if on_lit { theme::ACCENT } else { theme::INK2 });
    }
}

/// Compressor transfer curve (knee at 60 %, 0.3 above it) over a faint 1:1 line.
pub fn compressor<D>(d: &mut D)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (x0, x1) = (theme::VIZ_LEFT + 30, theme::VIZ_RIGHT - 30);
    let (w, h) = ((x1 - x0) as f32, (PLOT_BASE - PLOT_TOP) as f32);
    draw::line(d, x0, PLOT_BASE, x1, PLOT_TOP, theme::FAINT, 1);
    let out = |t: f32| if t < 0.6 { t } else { 0.6 + (t - 0.6) * 0.3 };
    filled_curve(d, x0, x1, PLOT_BASE, |x| PLOT_BASE - (h * out((x - x0) as f32 / w)) as i32);
    draw::text(d, &theme::FONT_LABEL, "IN", x1 + 4, PLOT_BASE, theme::MID);
    draw::text(d, &theme::FONT_LABEL, "OUT", x0 - 20, PLOT_TOP + 8, theme::MID);
}
```

`chimera-core/src/ui/renderer.rs`:
- Delete everything from `// ── Modal / Physical Modeling` up to (not including) `// ── BlockDef-based rendering` (the modal, VA, drive, filter, folder, envelope, FX, mixer, routing and compressor vizzes and `draw_envelope_shape`), and delete `draw_params_from_def` and `draw_viz_from_type`.
- Remove the now-unused imports `use crate::ui::cell;`, `use crate::part::PartParams;`, and `BlockRef, ParamAddr` from `use crate::addr::{…}` (leaving `use crate::addr::Op;`).
- Add:

```rust
    /// BigViz: the page's visualization with the touched value riding on it.
    fn draw_big_viz<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let a = |i: usize| self.anim[i].current();
        match f.def.viz {
            VizType::FilterResponse => {
                let slot = &f.def.params[f.focus];
                let mut buf = FmtBuf::new();
                fmt::fmt_val(&mut buf, a(f.focus), slot.format());
                let readout = (slot.binding != SlotBinding::Empty).then(|| (slot.label(), buf.as_str()));
                viz::filter(display, a(0), a(1), readout);
            }
            VizType::Adsr => {
                let (atk, dec, sus, rel) = (a(0).max(0.02), a(1).max(0.02), a(2), a(3).max(0.02));
                let total = atk + dec + 0.3 + rel;
                viz::envelope(
                    display,
                    &[atk / total, dec / total, 0.3 / total, rel / total],
                    &[0.0, 1.0, sus, sus, 0.0],
                    &["ATK", "DEC", "SUS", "REL"],
                    (f.focus < 4).then_some(f.focus),
                );
            }
            VizType::FmEnvelope => {
                // Rates: higher = faster = narrower. D1L is the level after D1R.
                let (ar, d1r, d1l, rr) = (a(0).max(0.02), a(1).max(0.02), a(2), a(4).max(0.02));
                let (atk_t, d1_t, d2_t, rel_t) = ((1.0 - ar).max(0.03), (1.0 - d1r).max(0.03), 0.25, (1.0 - rr).max(0.03));
                let total = atk_t + d1_t + d2_t + rel_t;
                let lit = match f.focus {
                    0 => Some(0),
                    1 | 2 => Some(1),
                    3 => Some(2),
                    4 => Some(3),
                    _ => None,
                };
                viz::envelope(
                    display,
                    &[atk_t / total, d1_t / total, d2_t / total, rel_t / total],
                    &[0.0, 1.0, d1l, d1l * 0.3, 0.0],
                    &["AR", "D1R", "D2R", "RR"],
                    lit,
                );
            }
            VizType::CompressorCurve => viz::compressor(display),
            _ => {}
        }
    }
```

- In `draw_region_with_def`: the `Viz` arm becomes

```rust
            RegionKind::Viz => match def.layout {
                PageLayout::CellGrid => viz::live_output(display, f.scope),
                PageLayout::BigViz => self.draw_big_viz(display, f),
                PageLayout::Matrix => {}
            },
```

  delete the `Params` arm, and change the destructuring to `let (nav, def, matrix_state) = (f.nav, f.def, f.matrix);`.
- In `viz_inputs` the non-CellGrid arm becomes two arms (the focus decides the readout and lit segment):

```rust
            PageLayout::BigViz => (region::quantize_values(&self.anim), f.focus as u32),
            PageLayout::Matrix => ([0; 6], 0),
```

`chimera-core/src/ui/region.rs`: delete the `Params` variant, `params()` and `sentinel_params()`, `RegionKind::Params` and its `sentinel` arm; add `const BIG_VIZ_END: u16 = theme::BIGVIZ_BOTTOM as u16;` beside the other band constants and replace the `BIG_VIZ` table:

```rust
/// BigViz: header, large viz, cells, map.
const BIG_VIZ: [(RegionKind, u16, u16); 4] =
    [(K::Header, 0, HEADER), (K::Viz, HEADER, BIG_VIZ_END), (K::Cells, BIG_VIZ_END, CELLS), (K::Nav, CELLS, SCREEN)];
```

`chimera-core/src/ui/mod.rs`: delete the `RegionKind::Params =>` arm in `region_data`.

`region_tests.rs`: `RegionData::params(p, v)` → `RegionData::cells(p, v, 0, 0)` (`sed -E -i 's/RegionData::params\(([^,]+), (\[[^]]*\]|\w+)\)/RegionData::cells(\1, \2, 0, 0)/'`), rename `region_data_params_values_differ` → `region_data_cell_values_differ`, `RegionKind::Params` → `RegionKind::Cells`.

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test big_viz_test --test region_tests --test screen_golden_test`
Expected: all pass (the locked `engine_pizza`/`system` still match). Dump and compare `bigviz_filter` with `drawFilter`: filled curve, faint pass-band line, dotted marker, dot, `CUTOFF` + value next to it.

- [ ] **Step 5: Lock**

```rust
    ("bigviz_filter", Locked(0x3bf9b75c5812d82f)),
    ("bigviz_env", Locked(0x16e11d64abffc08c)),
    ("bigviz_fm_op_env", Locked(0x928f9fb2b436c6d0)),
```

- [ ] **Step 6: `just check`, then commit** (core+hal 487 passed)

```bash
git add -A chimera-core
git commit -m "feat(core): BigViz pages in Direction A (filter readout, lit envelope segment)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Mixer PART viz band shows all six Parts

**Files:**
- Modify: `chimera-core/src/ui/viz.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/mod.rs`
- Modify/Test: `chimera-core/tests/mixer_page_test.rs`, `screen_golden_test.rs`

**Interfaces:**
- Produces: `viz::{Strip { level, pan }, strip_x(i) -> i32, STRIP_NUM_Y, STRIP_TOP, STRIP_H, STRIP_PAN_Y, parts_overview(d, &[Strip], selected)}`; `Frame::{parts: &[Part; MAX_PARTS], active_part: usize}`; private `Renderer::{draw_band_viz, strips}`, free `strips_key`.

- [ ] **Step 1: Write the failing tests** — in `chimera-core/tests/mixer_page_test.rs` delete the local `struct Fb` and its two impls, and insert before `/// The sound browser's title names what it loads`:

```rust
mod screen;

fn overview(setup: impl FnOnce(&mut UiState), part_button: ButtonId) -> screen::Fb {
    let mut ui = UiState::new();
    setup(&mut ui);
    open_mixer(&mut ui, part_button);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    ui.render_with_scope(&mut fb, &chimera_core::ui::perf::PerfStats::zero(), &screen::scope_fixture());
    fb
}

/// Mixer PART viz band: every Part's level bar and pan dot, the edited Part
/// lit and drawn from its LEVEL and PAN slots (CH, MODE, OUT, LEVEL, PAN).
#[test]
fn part_overview_shows_every_part_with_the_edited_one_lit() {
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::{strip_x, STRIP_H, STRIP_PAN_Y, STRIP_TOP};
    let fb = overview(
        |ui| {
            ui.performance.parts[1].mix.level = 1.0;
            ui.performance.parts[1].mix.pan = 1.0;
            ui.performance.parts[3].mix.level = 0.0;
        },
        ButtonId::B2,
    );
    let column = |i: usize, c| (STRIP_TOP..STRIP_TOP + STRIP_H).filter(|&y| fb.at(strip_x(i) + 4, y) == c).count();
    assert_eq!(column(1, theme::ACCENT), STRIP_H as usize, "Part 2: full, lit");
    assert_eq!(column(0, theme::ACCENT), 0, "Part 1 not lit");
    assert!(column(0, theme::BAR_REST) > 0, "Part 1 at its stored level");
    assert_eq!(column(3, theme::BAR_REST), 0, "Part 4 at level 0");
    let dot: Vec<i32> = (strip_x(1) - 6..strip_x(1) + 16).filter(|&x| fb.at(x, STRIP_PAN_Y) == theme::INK).collect();
    assert!(dot.iter().all(|&x| x > strip_x(1) + 10), "Part 2 panned right: {dot:?}");
}

#[test]
fn part_overview_redraws_as_the_level_lerps() {
    let mut ui = UiState::new();
    open_mixer(&mut ui, ButtonId::B1);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    let (perf, scope) = (chimera_core::ui::perf::PerfStats::zero(), screen::scope_fixture());
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    turn(&mut ui, EncoderId::D, -20);
    ui.update();
    let flushed = ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert!(flushed.contains(&(118, 186)), "viz band follows the lerped level: {flushed:?}");
}

#[test]
fn mixer_part_dirty_render_equals_full_render() {
    assert!(screen::render("mixer_part").px == screen::render_dirty("mixer_part").px);
}

```

In `sound_browser_title_names_the_part` switch to the harness display (the browser now clears to the Direction A ground): `let title_band = |fb: &screen::Fb| fb.px[..22 * 240].to_vec();`, `let mut got = screen::Fb::new();`, and `let mut want = screen::Fb::new();` followed by `want.px.fill(got.px[0]); // the ground`.

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p chimera-core --test mixer_page_test`
Expected: compile error `cannot find function strip_x in module viz`.

- [ ] **Step 3: Implement**

Append to `chimera-core/src/ui/viz.rs`:

```rust

/// One Part in the Mixer overview.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    /// 0..1
    pub level: f32,
    /// −1 (left) .. 1 (right)
    pub pan: f32,
}

/// Bar x of Part `i` in the overview.
pub fn strip_x(i: usize) -> i32 {
    20 + i as i32 * 36
}
pub const STRIP_NUM_Y: i32 = 128;
pub const STRIP_TOP: i32 = 132;
pub const STRIP_H: i32 = 38;
pub const STRIP_PAN_Y: i32 = 177;

/// Mixer PART viz band: each Part's level bar and pan dot, `selected` lit.
pub fn parts_overview<D>(d: &mut D, strips: &[Strip], selected: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut num = crate::ui::fmt::FmtBuf::new();
    for (i, s) in strips.iter().enumerate() {
        let (x, sel) = (strip_x(i), i == selected);
        num.clear();
        let _ = core::fmt::Write::write_fmt(&mut num, format_args!("{}", i + 1));
        let (font, color) = if sel { (&theme::FONT_LABEL_BOLD, theme::INK) } else { (&theme::FONT_LABEL, theme::MID) };
        draw::text_center(d, font, num.as_str(), x + 4, STRIP_NUM_Y, color, 0);
        draw::fill_rect(d, x, STRIP_TOP, 8, STRIP_H, theme::FAINT);
        let h = (STRIP_H as f32 * s.level.clamp(0.0, 1.0) + 0.5) as i32;
        let fill = if sel { theme::ACCENT } else if s.level > 0.0 { theme::BAR_REST } else { theme::FAINT };
        draw::fill_rect(d, x, STRIP_TOP + STRIP_H - h, 8, h, fill);
        draw::fill_rect(d, x - 6, STRIP_PAN_Y, 20, 1, theme::FAINT);
        let px = x + 4 + libm::roundf(s.pan.clamp(-1.0, 1.0) * 10.0) as i32;
        draw::dot(d, px, STRIP_PAN_Y, 2, if sel { theme::INK } else { theme::MID });
    }
}
```

`chimera-core/src/ui/renderer.rs`:
- `Frame` gets, after `sounding`:

```rust
    /// Every Part (Mixer overview, FM algorithm) and the one being edited.
    pub parts: &'a [crate::preset::Part; crate::hw::MAX_PARTS],
    pub active_part: usize,
```

- `Viz` arm: `PageLayout::CellGrid => self.draw_band_viz(display, f),`; `viz_inputs` CellGrid arm:

```rust
            PageLayout::CellGrid => match f.def.viz {
                VizType::MixerLevels => (region::quantize_values(&self.anim), strips_key(&self.strips(f), f.active_part)),
                _ => ([0; 6], viz::live_key(f.scope)),
            },
```

- Add:

```rust
    /// The viz band of a CellGrid page.
    fn draw_band_viz<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match f.def.viz {
            VizType::MixerLevels => viz::parts_overview(display, &self.strips(f), f.active_part),
            _ => viz::live_output(display, f.scope),
        }
    }

    /// Level and pan of every Part; the edited one from its animated LEVEL
    /// and PAN slots so it lerps like the cells.
    fn strips(&self, f: &Frame) -> [viz::Strip; crate::hw::MAX_PARTS] {
        let slot = |id| {
            let addr = crate::addr::ParamAddr::new(crate::addr::BlockRef::Part, id);
            (0..f.def.params.len()).find(|&i| slot_addr(f.def, i, Op::A) == Some(addr))
        };
        let (level, pan) = (slot(crate::part::PartParams::LEVEL), slot(crate::part::PartParams::PAN));
        core::array::from_fn(|i| {
            let mix = &f.parts[i].mix;
            let mut s = viz::Strip { level: mix.level, pan: mix.pan };
            if i == f.active_part {
                if let Some(l) = level {
                    s.level = self.anim[l].current();
                }
                if let Some(p) = pan {
                    s.pan = self.anim[p].current() * 2.0 - 1.0;
                }
            }
            s
        })
    }
```

  and at the end of the file:

```rust

/// Fingerprint of the Mixer overview (quantized levels and pans, selection).
fn strips_key(strips: &[viz::Strip], selected: usize) -> u32 {
    strips.iter().fold(selected as u32, |h, s| {
        let q = (region::quantize(s.level) as u32) << 16 | region::quantize(s.pan + 1.0) as u32;
        (h ^ q).wrapping_mul(0x0100_0193)
    })
}
```

`chimera-core/src/ui/mod.rs`, `frame()`: add `parts: &self.performance.parts,` and `active_part: self.active_part,`.

- [ ] **Step 4: Run to see it pass**

Run: `cargo test -p chimera-core --test mixer_page_test --test screen_golden_test`
Expected: all pass. Compare `mixer_part` with `drawMixer` (numbers 1–6, bars, pan dots, Part 1 lit).

- [ ] **Step 5: Lock** — `("mixer_part", Locked(0x3e3480eee3041370)),`

- [ ] **Step 6: `just check`, then commit** (core+hal 490 passed)

```bash
git add -A chimera-core
git commit -m "feat(core): Mixer PART viz band shows all six Parts

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 9: FX flow in the viz band of the Mixer SENDS and FX pages

**Files:**
- Modify: `chimera-core/src/ui/viz.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/block_registry.rs` (SENDS `viz`)
- Modify/Test: `chimera-core/tests/mixer_page_test.rs`, `screen_golden_test.rs`

**Interfaces:**
- Produces: `viz::{FX_NODES, FLOW_Y, FLOW_SEND_Y, effects_flow(d, lit: Option<usize>, sends: Option<[f32; 3]>)}` (`lit` 0 = CHR, 1 = DLY, 2 = REV). `reg::SENDS.viz == VizType::EffectsFlow`.

- [ ] **Step 1: Write the failing tests** — in `mixer_page_test.rs` insert before `mixer_part_dirty_render_equals_full_render`:

```rust
/// FX flow node `k` (0 = CHR) is the lit pill in `fb`.
fn flow_lit(fb: &screen::Fb) -> Vec<usize> {
    use chimera_core::ui::dungeon_map::node_x;
    use chimera_core::ui::theme;
    use chimera_core::ui::viz::FLOW_Y;
    (0..3).filter(|&k| fb.at(node_x(k + 1, 5) - 14, FLOW_Y) == theme::ACCENT).collect()
}

#[test]
fn fx_pages_light_their_effect_in_the_flow() {
    let fb = screen::render("mixer_fx_delay");
    assert_eq!(flow_lit(&fb), [1], "Delay page lights DLY");
    assert!(matches!(reg::SENDS.viz, chimera_core::ui::block_def::VizType::EffectsFlow));
}

#[test]
fn sends_page_lights_the_focused_send() {
    assert_eq!(flow_lit(&screen::render("mixer_sends")), [2], "REV send focused");
    let mut ui = screen::ui_for("mixer_sends");
    turn(&mut ui, EncoderId::A, 1);
    screen::settle(&mut ui);
    let mut fb = screen::Fb::new();
    ui.render_with_scope(&mut fb, &chimera_core::ui::perf::PerfStats::zero(), &screen::scope_fixture());
    assert_eq!(flow_lit(&fb), [0], "CHR send focused");
}
```

and make `mixer_part_dirty_render_equals_full_render` cover all three Mixer cases:

```rust
    for name in ["mixer_part", "mixer_sends", "mixer_fx_delay"] {
        assert!(screen::render(name).px == screen::render_dirty(name).px, "{name}");
    }
```

- [ ] **Step 2: Run to see it fail** — `cargo test -p chimera-core --test mixer_page_test` → compile error `cannot find value FLOW_Y`.

- [ ] **Step 3: Implement**

Append to `viz.rs`:

```rust

/// The FX flow's node labels, in the Mixer chain's order.
pub const FX_NODES: [&str; 5] = ["IN", "CHR", "DLY", "REV", "OUT"];
pub const FLOW_Y: i32 = 146;
pub const FLOW_SEND_Y: i32 = 176;

/// FX pages and SENDS: IN → CHR → DLY → REV → OUT on a line (the
/// pre-refresh flow diagram, restyled like the map). `lit` (0 = CHR) is the
/// page's effect, or on SENDS the focused send; `sends` shows each send level
/// under its effect.
pub fn effects_flow<D>(d: &mut D, lit: Option<usize>, sends: Option<[f32; 3]>)
where
    D: DrawTarget<Color = Rgb565>,
{
    let n = FX_NODES.len();
    draw::fill_rect(d, theme::MAP_X0, FLOW_Y, theme::MAP_X1 - theme::MAP_X0, 1, theme::FAINT);
    for (i, label) in FX_NODES.iter().enumerate() {
        let x = crate::ui::dungeon_map::node_x(i, n);
        let fx = i.checked_sub(1).filter(|&k| k < 3);
        if fx.is_some() && fx == lit {
            draw::pill(d, x - theme::PILL_W / 2, FLOW_Y - theme::PILL_H / 2, theme::PILL_W, theme::PILL_H, theme::ACCENT);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, label, x, FLOW_Y + 4, theme::BG, 0);
        } else {
            draw::dot(d, x, FLOW_Y, theme::NODE_R, theme::BG);
            draw::ring(d, x, FLOW_Y, theme::NODE_R, theme::MID, 1);
            draw::text_center(d, &theme::FONT_LABEL, label, x, FLOW_Y + 18, theme::MID, 0);
        }
        if let (Some(k), Some(levels)) = (fx, sends) {
            let fill = if lit == Some(k) { theme::ACCENT } else { theme::BAR_REST };
            draw::bar(d, x - 14, FLOW_SEND_Y, 28, 2, levels[k], false, theme::FAINT, fill);
        }
    }
}
```

`renderer.rs`, `draw_band_viz`, before the `_ =>` arm:

```rust
            VizType::EffectsFlow => {
                use crate::ui::block_registry as reg;
                if f.def.id == reg::SENDS.id {
                    let sends = [self.anim[0].current(), self.anim[1].current(), self.anim[2].current()];
                    viz::effects_flow(display, (f.focus < 3).then_some(f.focus), Some(sends));
                } else {
                    let lit = [reg::CHORUS.id, reg::DELAY.id, reg::EFX.id].iter().position(|&id| id == f.def.id);
                    viz::effects_flow(display, lit, None);
                }
            }
```

and in `viz_inputs` after the `MixerLevels` arm: `VizType::EffectsFlow => (region::quantize_values(&self.anim), f.focus as u32),`.

`block_registry.rs`, `SENDS`: `viz: VizType::None,` → `viz: VizType::EffectsFlow,`.

- [ ] **Step 4: Run to see it pass** — `cargo test -p chimera-core --test mixer_page_test --test screen_golden_test` → all pass.

- [ ] **Step 5: Lock**

```rust
    ("mixer_sends", Locked(0xfc90ed4f83130581)),
    ("mixer_fx_delay", Locked(0x4b5f420a1ec41f11)),
```

- [ ] **Step 6: `just check`, then commit** (core+hal 492 passed)

```bash
git add -A chimera-core
git commit -m "feat(core): FX flow viz band on the Mixer SENDS and FX pages

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 10: FM algorithm diagram, selected operator lit

**Files:**
- Modify: `chimera-core/src/ui/viz.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/block_registry.rs` (FM_OP `viz`), `chimera-core/src/ui/block_def.rs` (`SelectOp` format)
- Modify: `chimera-core/tests/binding_test.rs`, `screen_golden_test.rs`
- Test: `chimera-core/tests/fm_viz_test.rs`

**Interfaces:**
- Produces: `viz::{alg_op_center(alg: u8, op: usize) -> (i32, i32), fm_algorithm(d, alg: u8, selected: usize)}`. `reg::FM_OP.viz == AlgorithmDiagram`. The `SelectOp` slot formats as `OneBased(3)` (shows 1–4).

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/fm_viz_test.rs`:

```rust
//! FM algorithm diagram in the viz band of the FM algorithm and operator
//! pages (UI refresh spec § Page types: edited operator lit).

mod screen;

use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme;
use chimera_core::ui::viz::alg_op_center;
use chimera_hal::EncoderId;
use screen::*;

#[test]
fn every_algorithm_fits_the_band_without_overlap() {
    for alg in 0..8 {
        let c: Vec<(i32, i32)> = (0..4).map(|op| alg_op_center(alg, op)).collect();
        for &(x, y) in &c {
            assert!((theme::VIZ_BAND_TOP + 8..=theme::VIZ_BAND_BOTTOM - 8).contains(&y), "alg {alg} y {y}");
            assert!((8..232).contains(&x), "alg {alg} x {x}");
        }
        for i in 0..4 {
            for j in i + 1..4 {
                let (dx, dy) = (c[i].0 - c[j].0, c[i].1 - c[j].1);
                assert!(dx * dx + dy * dy >= 16 * 16, "alg {alg}: ops {} and {} overlap", i + 1, j + 1);
            }
        }
    }
}

#[test]
fn the_selected_operator_is_lit() {
    let mut ui = ui_for("engine_fm_op"); // operator 3 selected
    let alg = ui.performance.parts[0].sound.params.fm.algorithm;
    let lit = |ui: &chimera_core::ui::UiState| {
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        (0..4).filter(|&op| {
            let (x, y) = alg_op_center(alg, op);
            fb.at(x - 4, y) == theme::ACCENT
        }).collect::<Vec<_>>()
    };
    assert_eq!(lit(&ui), [2]);
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    settle(&mut ui);
    assert_eq!(lit(&ui), [1]);
}

#[test]
fn algorithm_seven_is_one_row_of_carriers() {
    let ys: Vec<i32> = (0..4).map(|op| alg_op_center(7, op).1).collect();
    assert!(ys.iter().all(|&y| y == theme::VIZ_BAND_MID));
}

#[test]
fn fm_pages_dirty_render_equals_full_render() {
    for name in ["engine_fm_alg", "engine_fm_op"] {
        assert!(render(name).px == render_dirty(name).px, "{name}");
    }
}
```

In `binding_test.rs::part_pages_display_like_before`: `use ValFmt::{Bi, Int, Uni};` → `use ValFmt::{Bi, Int, OneBased, Uni};`, the FM_OP row starts `[("OP", OneBased(3)),`, and the doc comment gains `/// …, except\n/// the operator selector, shown 1–4 since the UI refresh.`

- [ ] **Step 2: Run to see it fail** — `cargo test -p chimera-core --test fm_viz_test --test binding_test` → compile error `cannot find function alg_op_center`; `binding_test` fails on `OP`.

- [ ] **Step 3: Implement**

Append to `viz.rs`:

```rust

/// FM algorithm topologies 0..=7 (the pre-refresh diagrams, as data):
/// operator 1..4 positions as (x in half steps from centre, row), the
/// modulation edges (from, to), and the carriers (bit n-1 = operator n).
const ALG_POS: [[(i8, i8); 4]; 8] = [
    [(0, 3), (0, 2), (0, 1), (0, 0)],
    [(0, 2), (0, 1), (-1, 0), (1, 0)],
    [(0, 2), (0, 1), (-1, 0), (1, 0)],
    [(0, 2), (1, 1), (-1, 1), (0, 0)],
    [(-1, 1), (-1, 0), (1, 1), (1, 0)],
    [(-2, 1), (0, 1), (2, 1), (0, 0)],
    [(-2, 1), (0, 1), (2, 1), (2, 0)],
    [(-3, 0), (-1, 0), (1, 0), (3, 0)],
];
const ALG_EDGES: [&[(u8, u8)]; 8] = [
    &[(4, 3), (3, 2), (2, 1)],
    &[(3, 2), (4, 2), (2, 1)],
    &[(3, 2), (2, 1), (4, 1)],
    &[(4, 3), (4, 2), (3, 1), (2, 1)],
    &[(2, 1), (4, 3)],
    &[(4, 1), (4, 2), (4, 3)],
    &[(4, 3)],
    &[],
];
const ALG_CARRIERS: [u8; 8] = [0b0001, 0b0001, 0b0001, 0b0001, 0b0101, 0b0111, 0b0111, 0b1111];
const ALG_STEP_X: i32 = 20;
const ALG_STEP_Y: i32 = 17;
const ALG_OP_R: i32 = 7;

/// Centre of operator `op` (0-based) in algorithm `alg`, in the viz band.
pub fn alg_op_center(alg: u8, op: usize) -> (i32, i32) {
    let a = (alg as usize).min(7);
    let rows = ALG_POS[a].iter().map(|p| p.1).max().unwrap_or(0) as i32 + 1;
    let top = theme::VIZ_BAND_MID - (rows - 1) * ALG_STEP_Y / 2;
    let (hx, row) = ALG_POS[a][op];
    (theme::SCREEN_W / 2 + hx as i32 * ALG_STEP_X, top + row as i32 * ALG_STEP_Y)
}

/// FM algorithm page and operator page: the algorithm's operators and
/// edges; carriers filled, modulators as rings, the selected operator lit.
pub fn fm_algorithm<D>(d: &mut D, alg: u8, selected: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let a = (alg as usize).min(7);
    for &(from, to) in ALG_EDGES[a] {
        let (x0, y0) = alg_op_center(alg, from as usize - 1);
        let (x1, y1) = alg_op_center(alg, to as usize - 1);
        draw::line(d, x0, y0, x1, y1, theme::MID, 1);
    }
    for (op, label) in ["1", "2", "3", "4"].into_iter().enumerate() {
        let (x, y) = alg_op_center(alg, op);
        let carrier = ALG_CARRIERS[a] & (1 << op) != 0;
        if op == selected {
            draw::dot(d, x, y, ALG_OP_R, theme::ACCENT);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, label, x + 1, y + 4, theme::BG, 0);
        } else if carrier {
            draw::dot(d, x, y, ALG_OP_R, theme::INK2);
            draw::text_center(d, &theme::FONT_LABEL_BOLD, label, x + 1, y + 4, theme::BG, 0);
        } else {
            draw::dot(d, x, y, ALG_OP_R, theme::BG);
            draw::ring(d, x, y, ALG_OP_R, theme::MID, 1);
            draw::text_center(d, &theme::FONT_LABEL, label, x + 1, y + 4, theme::MID, 0);
        }
    }
}
```

`renderer.rs`, `draw_band_viz` before `_ =>`:

```rust
            VizType::AlgorithmDiagram => {
                let alg = f.parts[f.active_part].sound.params.fm.algorithm;
                viz::fm_algorithm(display, alg, f.sel_op.index());
            }
```

`viz_inputs`, before `_ =>`:

```rust
                VizType::AlgorithmDiagram => {
                    let alg = f.parts[f.active_part].sound.params.fm.algorithm as u32;
                    ([0; 6], alg << 2 | f.sel_op.index() as u32)
                }
```

(The algorithm is a stored integer choice; it is drawn from the param, not lerped.)

`block_registry.rs`, `FM_OP`: `viz: VizType::None,` → `viz: VizType::AlgorithmDiagram,`. `block_def.rs`, `ParamSlot::format`: `SlotBinding::SelectOp => ValFmt::Int(3),` → `SlotBinding::SelectOp => ValFmt::OneBased(3),`.

- [ ] **Step 4: Run to see it pass** — `cargo test -p chimera-core --test fm_viz_test --test binding_test --test screen_golden_test` → all pass.

- [ ] **Step 5: Lock**

```rust
    ("engine_fm_alg", Locked(0x48b0b28b69670bc6)),
    ("engine_fm_op", Locked(0x2353d264169904b3)),
```

- [ ] **Step 6: `just check`, then commit** (core+hal 496 passed)

```bash
git add -A chimera-core
git commit -m "feat(core): FM algorithm diagram in the FM viz band, selected operator lit

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 11: Mod matrix — selected route in the focus band, dot grid

**Files:**
- Modify: `chimera-core/src/ui/mod_grid.rs`, `chimera-core/src/ui/components.rs`, `chimera-core/src/ui/region.rs`, `chimera-core/src/ui/renderer.rs`, `chimera-core/src/ui/mod.rs`
- Modify: `screen_golden_test.rs`
- Test: `chimera-core/tests/matrix_view_test.rs`

**Interfaces:**
- Consumes: `MatrixState` (unchanged API: `sel_row`, `sel_col`, `scroll_x/y`, `amounts`, `dests`, `sources`, `move_col`, `visible_cols/rows`, `current_amount`).
- Produces: `mod_grid::{GRID_X, GRID_COL_W, GRID_TAG_Y, GRID_NAME_Y, GRID_ROW0_Y, GRID_ROW_H, HINT_Y, STATS_Y, block_tag(BlockRef) -> &str, dest_name(&ModDest) -> &str, fmt_amount(&mut FmtBuf, i8), cell_center(ci, vi) -> (i32, i32), draw_grid(d, &MatrixState)}` (5 visible columns, 3 visible rows); `components::focus_route(d, source, dest, value_text, value)`; `RegionData::Route { row, col, dests, value }`; `renderer::{MATRIX_AMOUNT_SLOT = 4, amount_value(i8) -> f32, amount_of(f32) -> i8}`; the Matrix layout becomes Header · Focus · Grid · Nav.

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/matrix_view_test.rs`:

```rust
//! Mod matrix page in Direction A (UI refresh spec § Page types): the
//! selected route in the focus band, then a dot grid.

mod screen;

use chimera_core::ui::components;
use chimera_core::ui::mod_grid::cell_center;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::renderer::{amount_of, amount_value, MATRIX_AMOUNT_SLOT};
use chimera_core::ui::theme;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

#[test]
fn amount_display_value_round_trips() {
    for a in -127..=127i8 {
        assert_eq!(amount_of(amount_value(a)), a);
    }
    assert_eq!(amount_value(0), 0.5);
}

/// The fixture: ENV→CUTOFF +20, ENV→FOLD −30, LFO→CUTOFF +42 (selected).
#[test]
fn focus_band_names_the_selected_route() {
    let fb = render("mod_matrix");
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    components::focus_route(&mut want, "LFO", "CUTOFF", "+42", amount_value(42));
    assert!(fb.px[28 * W..118 * W] == want.px[28 * W..118 * W]);
}

#[test]
fn dots_show_sign_and_size_and_the_cursor_is_outlined() {
    let fb = render("mod_matrix");
    let (x, y) = cell_center(0, 0); // ENV → CUTOFF, +20
    assert_eq!(fb.at(x, y), theme::INK2, "positive: filled");
    let (x, y) = cell_center(1, 0); // ENV → FOLD, −30
    assert_eq!(fb.at(x, y), theme::BG, "negative: a ring");
    assert!((1..6).any(|r| fb.at(x + r, y) == theme::INK2));
    let (x, y) = cell_center(1, 1); // LFO → FOLD, none
    assert_eq!(fb.at(x, y), theme::FAINT, "no route: a tiny dim dot");
    assert_ne!(fb.at(x + 2, y), theme::FAINT);
    let (x, y) = cell_center(0, 1); // LFO → CUTOFF, selected
    assert_eq!(fb.at(x, y), theme::ACCENT, "selected route lit");
    assert_eq!(fb.at(x - 14, y), theme::ACCENT, "cursor outline");
}

#[test]
fn the_amount_lerps() {
    let mut ui = ui_for("mod_matrix");
    let before = ui.renderer.anim[MATRIX_AMOUNT_SLOT].current();
    feed(&mut ui, Input::turn(EncoderId::E, 40));
    ui.update();
    let (now, target) = (ui.renderer.anim[MATRIX_AMOUNT_SLOT].current(), ui.renderer.anim[MATRIX_AMOUNT_SLOT].target());
    assert!(before < now && now < target, "{before} < {now} < {target}");
    assert_eq!(amount_of(target), 82);
}

#[test]
fn an_empty_matrix_says_so() {
    let mut ui = chimera_core::ui::UiState::new();
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    assert_eq!(fb.oob, 0);
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    chimera_core::ui::draw::text_tracked(&mut want, &theme::FONT_VALUE, "NO DESTINATIONS", theme::MARGIN_X, theme::FOCUS_LABEL_Y, theme::MID, theme::LABEL_TRACKING);
    assert!(fb.px[28 * W..118 * W] == want.px[28 * W..118 * W]);
}

#[test]
fn matrix_dirty_render_equals_full_render() {
    assert!(render("mod_matrix").px == render_dirty("mod_matrix").px);
}

/// More destinations than columns: the grid scrolls, shows `<`, and the
/// cursor stays on screen.
#[test]
fn a_wide_matrix_scrolls_with_the_cursor() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::ui::mod_grid::{draw_grid, ModDest, MatrixState, MAX_DESTS};
    let mut m = MatrixState::new();
    m.rebuild_sources(&["ENV", "LFO"]);
    for i in 0..MAX_DESTS {
        m.dests[i] = Some(ModDest { addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)), label: [b'X'; 8] });
    }
    m.num_dests = MAX_DESTS;
    m.move_col(MAX_DESTS as i8 - 1);
    assert_eq!(m.scroll_x, MAX_DESTS - m.visible_cols());
    let mut fb = Fb::new();
    draw_grid(&mut fb, &m);
    assert_eq!(fb.oob, 0);
    let (x, y) = cell_center(m.visible_cols() - 1, 0);
    assert_eq!(fb.at(x - 14, y), theme::ACCENT, "cursor in the last visible column");
}
```

- [ ] **Step 2: Run to see it fail** — `cargo test -p chimera-core --test matrix_view_test` → compile error `cannot find function cell_center`.

- [ ] **Step 3: Implement**

`chimera-core/src/ui/mod_grid.rs`: replace the module doc with `//! Mod matrix: routing state (cursor, amounts, sources, destinations) and\n//! its dot grid (UI refresh spec § Page types).`, and replace everything from the first `use` up to (not including) `/// Max sources and destinations for the amounts grid.` with:

```rust
use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::addr::{BlockRef, ParamAddr};
use crate::mod_path::LABEL_LEN;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::theme;

/// Dot grid geometry (grid region y 118..266): destination labels across,
/// sources down, one dot per route.
pub const GRID_X: i32 = 58;
pub const GRID_COL_W: i32 = 40;
pub const GRID_TAG_Y: i32 = 130;
pub const GRID_NAME_Y: i32 = 140;
pub const GRID_ROW0_Y: i32 = 162;
pub const GRID_ROW_H: i32 = 24;
pub const HINT_Y: i32 = 236;
pub const STATS_Y: i32 = 254;
const VISIBLE_COLS: usize = 5;
const VISIBLE_ROWS: usize = 3;

```

Then replace everything from `/// Draw the mod matrix grid in the content zone.` to the end of the file (old `draw_grid`, `format_amount`, `format_stats`, `write_num`) with:

```rust
/// Short tag for the block a destination lives in (column header, top line).
pub fn block_tag(b: BlockRef) -> &'static str {
    use crate::addr::Op;
    match b {
        BlockRef::Pizza => "PIZ",
        BlockRef::Modal => "MDL",
        BlockRef::Fm => "FM",
        BlockRef::FmOp(Op::A) => "OP1",
        BlockRef::FmOp(Op::B) => "OP2",
        BlockRef::FmOp(Op::C) => "OP3",
        BlockRef::FmOp(Op::D) => "OP4",
        BlockRef::Drive => "DRV",
        BlockRef::Filter => "FLT",
        BlockRef::Folder => "FLD",
        BlockRef::AmpEnv => "ENV",
        BlockRef::FilterEnv => "FEN",
        BlockRef::AuxEnv => "AEN",
        BlockRef::Lfo => "LFO",
        BlockRef::Out => "OUT",
        BlockRef::Chorus => "CHR",
        BlockRef::Delay => "DLY",
        BlockRef::Reverb => "REV",
        BlockRef::Part => "PRT",
    }
}

/// A destination's parameter name (its spec label).
pub fn dest_name(d: &ModDest) -> &'static str {
    d.addr.spec().map_or("?", |s| s.label)
}

/// Amount as shown: `+42`, `-30`, `0`.
pub fn fmt_amount(buf: &mut FmtBuf, amount: i8) {
    let _ = if amount > 0 { write!(buf, "+{}", amount) } else { write!(buf, "{}", amount) };
}

/// Centre of grid cell (visible column `ci`, visible row `vi`).
pub fn cell_center(ci: usize, vi: usize) -> (i32, i32) {
    (GRID_X + ci as i32 * GRID_COL_W, GRID_ROW0_Y + vi as i32 * GRID_ROW_H)
}

/// Dot grid: sources down, primed destinations across; a filled dot is a
/// positive amount, a ring negative, size = |amount|, a tiny dim dot none;
/// the selected cell outlined in the accent. Then the hint and route count.
pub fn draw_grid<D>(d: &mut D, state: &MatrixState)
where
    D: DrawTarget<Color = Rgb565>,
{
    let (cols, rows) = (state.visible_cols(), state.visible_rows());
    for ci in 0..cols {
        let di = ci + state.scroll_x;
        let Some(Some(dest)) = state.dests.get(di).filter(|_| di < state.num_dests) else { break };
        let x = GRID_X + ci as i32 * GRID_COL_W;
        let name_color = if di == state.sel_col { theme::INK } else { theme::MID };
        draw::text_center(d, &theme::FONT_LABEL, block_tag(dest.addr.block), x, GRID_TAG_Y, theme::MID, 0);
        draw::text_center(d, &theme::FONT_LABEL, dest_name(dest), x, GRID_NAME_Y, name_color, 0);
    }
    if state.scroll_x > 0 {
        draw::text(d, &theme::FONT_LABEL, "<", GRID_X - 26, GRID_NAME_Y, theme::MID);
    }
    if state.num_dests > state.scroll_x + cols {
        draw::text(d, &theme::FONT_LABEL, ">", theme::SCREEN_W - 8, GRID_NAME_Y, theme::MID);
    }
    for vi in 0..rows {
        let ri = vi + state.scroll_y;
        if ri >= state.num_sources {
            break;
        }
        let (_, y) = cell_center(0, vi);
        let name = state.sources[ri].map_or("?", |s| s.name);
        let color = if ri == state.sel_row { theme::INK } else { theme::MID };
        draw::text(d, &theme::FONT_LABEL_BOLD, name, theme::MARGIN_X, y + 4, color);
        for ci in 0..cols {
            let di = ci + state.scroll_x;
            if di >= state.num_dests {
                break;
            }
            let (x, y) = cell_center(ci, vi);
            let amount = state.amounts[ri][di];
            let selected = ri == state.sel_row && di == state.sel_col;
            if selected {
                draw::round_outline(d, x - 14, y - 11, 28, 22, 6, theme::ACCENT);
            }
            let r = 2 + (amount as i32).abs() * 8 / 127;
            let color = if selected { theme::ACCENT } else { theme::INK2 };
            match amount {
                0 => draw::dot(d, x, y, 1, theme::FAINT),
                a if a > 0 => draw::dot(d, x, y, r, color),
                _ => draw::ring(d, x, y, r, color, 1),
            }
        }
    }
    draw::text(d, &theme::FONT_LABEL, "MIX+PLUS ADD   MIX+MINUS REMOVE", theme::MARGIN_X, HINT_Y, theme::MID);
    let routes = (0..state.num_sources)
        .flat_map(|r| (0..state.num_dests).map(move |c| (r, c)))
        .filter(|&(r, c)| state.amounts[r][c] != 0)
        .count();
    let mut buf = FmtBuf::new();
    let _ = write!(buf, "{} ROUTES   {} OF {} DESTINATIONS", routes, state.num_dests, MAX_DESTS);
    draw::text(d, &theme::FONT_LABEL, buf.as_str(), theme::MARGIN_X, STATS_Y, theme::MID);
}
```

`components.rs`, after `focus_band`:

```rust
/// Mod matrix focus band: the selected route `SOURCE → DEST` and its
/// bipolar amount.
pub fn focus_route<D>(d: &mut D, source: &str, dest: &str, value_text: &str, value: f32)
where
    D: DrawTarget<Color = Rgb565>,
{
    let x = focus_label(d, source, theme::MARGIN_X) + 6;
    let x = x + draw::arrow(d, x, theme::FOCUS_LABEL_Y, theme::MID) + 6;
    focus_label(d, dest, x);
    focus_value(d, value_text, value, true);
}
```

`region.rs`: add after `Focus` in `RegionData`:

```rust
    /// The mod matrix focus band: the selected route and its animated amount.
    Route {
        row: u8,
        col: u8,
        dests: u8,
        value: u16,
    },
```

and replace the `MATRIX` table:

```rust
/// Mod matrix: header, the selected route, dot grid, map.
const MATRIX: [(RegionKind, u16, u16); 4] =
    [(K::Header, 0, HEADER), (K::Focus, HEADER, FOCUS), (K::Grid, FOCUS, CELLS), (K::Nav, CELLS, SCREEN)];
```

`renderer.rs`:
- `use crate::ui::draw;` (keep the `Line` import: the legacy browser still uses it until Task 12).
- Grid arm: `RegionKind::Grid => crate::ui::mod_grid::draw_grid(display, matrix_state),`.
- At the top of `draw_focus` (after `{`):

```rust
        if f.def.layout == PageLayout::Matrix {
            return self.draw_route(display, f.matrix);
        }
```

- Add:

```rust
    /// Mod matrix focus band: the selected route; the amount lerps through
    /// slot e's animated value (the amount encoder).
    fn draw_route<D>(&self, display: &mut D, m: &MatrixState)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let dest = m.dests.get(m.sel_col).copied().flatten().filter(|_| m.sel_col < m.num_dests);
        let (Some(dest), Some(src)) = (dest, m.sources.get(m.sel_row).copied().flatten()) else {
            let label = "NO DESTINATIONS";
            draw::text_tracked(display, &theme::FONT_VALUE, label, theme::MARGIN_X, theme::FOCUS_LABEL_Y, theme::MID, theme::LABEL_TRACKING);
            return;
        };
        let v = self.anim[MATRIX_AMOUNT_SLOT].current();
        let mut buf = FmtBuf::new();
        crate::ui::mod_grid::fmt_amount(&mut buf, amount_of(v));
        components::focus_route(display, src.name, crate::ui::mod_grid::dest_name(&dest), buf.as_str(), v);
    }
```

  and at the end of the file:

```rust

/// The matrix page shows the selected amount through this display slot.
pub const MATRIX_AMOUNT_SLOT: usize = 4;

/// Amount −127..127 as a 0..1 display value (0 at the centre).
pub fn amount_value(amount: i8) -> f32 {
    0.5 + amount as f32 / 254.0
}

/// Inverse of `amount_value`, rounded.
pub fn amount_of(v: f32) -> i8 {
    libm::roundf((v - 0.5) * 254.0).clamp(-127.0, 127.0) as i8
}
```

`mod.rs`:
- In `region_data`, before the `RegionKind::Focus =>` arm:

```rust
            RegionKind::Focus if f.def.layout == PageLayout::Matrix => RegionData::Route {
                row: self.matrix_state.sel_row as u8,
                col: self.matrix_state.sel_col as u8,
                dests: self.matrix_state.num_dests as u8,
                value: qvalues[renderer::MATRIX_AMOUNT_SLOT],
            },
```

- Replace `enter_page` and add `display_values`, so the matrix amount is a lerped display value like any slot:

```rust
    /// Recompute the page identity and jump the display to its values.
    fn enter_page(&mut self) {
        self.page = PageKey::from_nav(&self.nav, self.sel_op);
        let values = self.display_values();
        self.renderer.snap_to_current(values);
    }

    /// The six values the display animates toward: the page's slots, and on
    /// the mod matrix the selected route's amount in slot e.
    fn display_values(&mut self) -> [f32; 6] {
        let def = self.nav.active_block_def();
        let mut values = page_values(self.page, def, &self.performance.edit(self.active_part), self.sel_op);
        if def.layout == PageLayout::Matrix {
            values[renderer::MATRIX_AMOUNT_SLOT] = renderer::amount_value(self.matrix_state.current_amount());
        }
        values
    }
```

- In `update`, `let mut values = page_values(self.page, def, &self.performance.edit(at), self.sel_op);` → `let mut values = self.display_values();`.

- [ ] **Step 4: Run to see it pass** — `cargo test -p chimera-core --test matrix_view_test --test region_tests --test screen_golden_test --test ui_routing_test --test modulation_integration_test` → all pass. Compare `mod_matrix` with `drawMatrix`.

- [ ] **Step 5: Lock** — `("mod_matrix", Locked(0xb66137bb078dc484)),`

- [ ] **Step 6: `just check`, then commit** (core+hal 503 passed)

```bash
git add -A chimera-core
git commit -m "feat(core): mod matrix as a dot grid with the selected route in the focus band

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 12: Sound browser

**Files:**
- Create: `chimera-core/src/ui/browser.rs`
- Modify: `chimera-core/src/ui/components.rs` (`title_to`), `chimera-core/src/ui/renderer.rs` (delete the browser), `chimera-core/src/ui/mod.rs`
- Modify: `chimera-core/tests/mixer_page_test.rs` (title test), `screen_golden_test.rs`
- Test: `chimera-core/tests/browser_test.rs`

**Interfaces:**
- Produces: `browser::{VISIBLE_ROWS = 8, INIT_TYPES: [ChainType; 3], TOTAL_ENTRIES, LIST_TOP, ROW_H, SCROLL_X, SCROLL_TOP, SCROLL_H, row_y(i) -> i32, draw(d, &SoundPool, part, cursor, scroll)}`; `components::title_to(d, context, name)`. Removed: `Renderer::{draw_sound_browser, BROWSER_VISIBLE_ROWS, BROWSER_TOTAL_ENTRIES}`.

- [ ] **Step 1: Write the failing tests**

`chimera-core/tests/browser_test.rs`:

```rust
//! Sound browser in Direction A (UI refresh spec § Page types).

mod screen;

use chimera_core::preset::{ChainType, Sound, SoundPool};
use chimera_core::ui::browser::{self, row_y, SCROLL_TOP, SCROLL_X, TOTAL_ENTRIES, VISIBLE_ROWS};
use chimera_core::ui::theme;
use chimera_core::ui::UiMode;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn drawn(pool: &SoundPool, cursor: usize, scroll: usize) -> Fb {
    let mut fb = Fb::new();
    browser::draw(&mut fb, pool, 0, cursor, scroll);
    assert_eq!(fb.oob, 0);
    fb
}

fn row_has(fb: &Fb, i: usize, c: embedded_graphics::pixelcolor::Rgb565) -> bool {
    let y = row_y(i);
    (y - 12..y + 2).any(|yy| (40..230).any(|x| fb.at(x, yy) == c))
}

#[test]
fn the_selected_row_is_an_accent_pill() {
    let fb = drawn(&SoundPool::new(), 2, 0);
    assert_eq!(fb.at(120, row_y(2) - 5), theme::ACCENT);
    assert_ne!(fb.at(120, row_y(1) - 5), theme::ACCENT);
}

#[test]
fn empty_slots_are_dimmed_and_saved_ones_bright() {
    let mut pool = SoundPool::new();
    pool.store(0, Sound::init(ChainType::Fm));
    let fb = drawn(&pool, 5, 0);
    assert!(row_has(&fb, 0, theme::INK), "saved slot name in ink");
    assert!(!row_has(&fb, 1, theme::INK) && row_has(&fb, 1, theme::FAINT), "empty slot: a dim dash");
}

#[test]
fn the_scroll_thumb_follows_the_list() {
    let thumb_top = |fb: &Fb| (SCROLL_TOP..SCROLL_TOP + 208).find(|&y| fb.at(SCROLL_X, y) == theme::MID).unwrap();
    let top = thumb_top(&drawn(&SoundPool::new(), 0, 0));
    let bottom = thumb_top(&drawn(&SoundPool::new(), TOTAL_ENTRIES - 1, TOTAL_ENTRIES - VISIBLE_ROWS));
    assert_eq!(top, SCROLL_TOP);
    assert!(bottom > top + 150, "{bottom}");
}

#[test]
fn init_rows_end_the_list_and_load() {
    let mut ui = chimera_core::ui::UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    feed(&mut ui, Input::turn(EncoderId::Main, 100)); // clamps to the last row
    assert_eq!(ui.ui_mode, UiMode::SoundBrowser { part: 1, cursor: TOTAL_ENTRIES - 1, scroll: TOTAL_ENTRIES - VISIBLE_ROWS });
    let fb = render_ui(&ui);
    assert_eq!(fb.at(120, row_y(VISIBLE_ROWS - 1) - 5), theme::ACCENT, "last visible row selected");
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.performance.parts[1].sound.chain_type, ChainType::Fm);
}

fn render_ui(ui: &chimera_core::ui::UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &chimera_core::ui::perf::PerfStats::zero(), &scope_fixture());
    fb
}

#[test]
fn browser_dirty_render_equals_full_render() {
    assert!(render("sound_browser").px == render_dirty("sound_browser").px);
}
```

Replace `sound_browser_title_names_the_part` in `mixer_page_test.rs`:

```rust
/// The sound browser's title names what it loads and the Part it loads
/// into: "LOAD SOUND → PART 2" for Part 2.
#[test]
fn sound_browser_title_names_the_part() {
    use chimera_core::preset::SoundPool;
    use chimera_core::ui::{browser, components};

    let mut got = screen::Fb::new();
    browser::draw(&mut got, &SoundPool::new(), 1, 0, 0);
    let mut want = screen::Fb::new();
    want.px.fill(got.px[0]); // the ground
    components::title_to(&mut want, "LOAD SOUND", "PART 2");
    assert!(got.px[..28 * 240] == want.px[..28 * 240], "title is LOAD SOUND → PART 2");
}
```

- [ ] **Step 2: Run to see it fail** — `cargo test -p chimera-core --test browser_test` → compile error `unresolved import chimera_core::ui::browser`.

- [ ] **Step 3: Implement**

Create `chimera-core/src/ui/browser.rs`:

```rust
//! Sound browser overlay in Direction A (UI refresh spec § Page types):
//! title, list rows (slot / name / engine tag) with the selected row an
//! accent pill and empty slots dimmed, a thin scroll indicator, key hints.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::preset::{ChainType, SoundPool, POOL_SIZE};
use crate::ui::chain::chain_def_for;
use crate::ui::components;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::theme;

/// Rows on screen.
pub const VISIBLE_ROWS: usize = 8;
/// The pool's slots, then one init Sound per chain type.
pub const INIT_TYPES: [ChainType; 3] = [ChainType::PizzaPoly, ChainType::Modal, ChainType::Fm];
pub const TOTAL_ENTRIES: usize = POOL_SIZE + INIT_TYPES.len();

pub const LIST_TOP: i32 = 44;
pub const ROW_H: i32 = 26;
pub const SCROLL_X: i32 = 236;
pub const SCROLL_TOP: i32 = 40;
pub const SCROLL_H: i32 = 208;
const HINT_Y: i32 = 284;
const INFO_Y: i32 = 304;

/// Baseline of visible row `i`.
pub fn row_y(i: usize) -> i32 {
    LIST_TOP + i as i32 * ROW_H
}

/// The full-screen browser for Part `part` (0-based).
pub fn draw<D>(d: &mut D, pool: &SoundPool, part: usize, cursor: usize, scroll: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    draw::fill_rect(d, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    let mut name = FmtBuf::new();
    let _ = write!(name, "PART {}", part + 1);
    components::title_to(d, "LOAD SOUND", name.as_str());

    for i in 0..VISIBLE_ROWS {
        let entry = scroll + i;
        if entry >= TOTAL_ENTRIES {
            break;
        }
        row(d, pool, entry, row_y(i), entry == cursor);
    }

    // Scroll position.
    draw::fill_rect(d, SCROLL_X, SCROLL_TOP, 2, SCROLL_H, theme::FAINT);
    let thumb_h = (SCROLL_H * VISIBLE_ROWS as i32 / TOTAL_ENTRIES as i32).max(8);
    let max_scroll = (TOTAL_ENTRIES - VISIBLE_ROWS) as i32;
    let thumb_y = SCROLL_TOP + (SCROLL_H - thumb_h) * scroll.min(max_scroll as usize) as i32 / max_scroll;
    draw::fill_rect(d, SCROLL_X, thumb_y, 2, thumb_h, theme::MID);

    for (i, (key, what)) in [("EDIT", "LOAD"), ("SEQ", "SAVE"), ("B", "CANCEL")].iter().enumerate() {
        let x = theme::MARGIN_X + i as i32 * 76;
        let w = draw::text_tracked(d, &theme::FONT_LABEL_BOLD, key, x, HINT_Y, theme::INK, theme::LABEL_TRACKING);
        draw::text_tracked(d, &theme::FONT_LABEL, what, x + w + 4, HINT_Y, theme::MID, theme::LABEL_TRACKING);
    }
    // Two strings: FmtBuf holds 32 bytes and one line would not fit.
    draw::text(d, &theme::FONT_LABEL, "MAIN SCROLLS", theme::MARGIN_X, INFO_Y, theme::MID);
    let mut info = FmtBuf::new();
    let _ = write!(info, "{} INIT + {} SLOTS", INIT_TYPES.len(), POOL_SIZE);
    draw::text_right(d, &theme::FONT_LABEL, info.as_str(), theme::VIZ_RIGHT, INFO_Y, theme::MID, 0);
}

fn row<D>(d: &mut D, pool: &SoundPool, entry: usize, y: i32, selected: bool)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut slot = FmtBuf::new();
    let (name, chain, saved) = if entry < POOL_SIZE {
        let _ = write!(slot, "{:02}", entry + 1);
        match pool.get(entry) {
            Some(s) => (components::upper(s.name_str()), Some(s.chain_type), true),
            None => (FmtBuf::new(), None, false),
        }
    } else {
        let _ = slot.write_str("INIT");
        let ct = INIT_TYPES[entry - POOL_SIZE];
        (components::upper(ct.label()), Some(ct), false)
    };
    let empty = chain.is_none();
    if selected {
        draw::pill(d, 8, y - 16, 224, 22, theme::ACCENT);
    }
    let dim = if selected { theme::BG } else { theme::MID };
    draw::text_tracked(d, &theme::FONT_LABEL, slot.as_str(), 18, y, dim, theme::LABEL_TRACKING);
    if empty {
        draw::fill_rect(d, 52, y - 4, 10, 1, if selected { theme::BG } else { theme::FAINT });
    } else {
        let color = if selected { theme::BG } else if saved { theme::INK } else { theme::INK2 };
        draw::text(d, &theme::FONT_VALUE, name.as_str(), 52, y, color);
    }
    if let Some(ct) = chain {
        draw::text_right(d, &theme::FONT_LABEL, chain_def_for(ct).blocks[0].def.short, 223, y, dim, 0);
    }
}
```

`components.rs`, before `focus_band`:

```rust
/// Overlay title in the header band: grey context, an arrow, bold name
/// (`LOAD SOUND → PART 1`).
pub fn title_to<D>(d: &mut D, context: &str, name: &str)
where
    D: DrawTarget<Color = Rgb565>,
{
    let y = theme::HEADER_BASELINE;
    let x = theme::MARGIN_X
        + draw::text_tracked(d, &theme::FONT_LABEL, context, theme::MARGIN_X, y, theme::MID, theme::LABEL_TRACKING)
        + 6;
    let x = x + draw::arrow(d, x, y, theme::MID) + 5;
    draw::text_tracked(d, &theme::FONT_LABEL_BOLD, name, x, y, theme::INK, theme::LABEL_TRACKING);
}
```

`renderer.rs`: delete the `// ── Sound Browser` section (the two `BROWSER_*` constants and `draw_sound_browser`) and the now-unused imports `embedded_graphics::Drawable`, `mono_font::MonoTextStyle`, `mono_font::ascii::FONT_6X10`, `text::Text`, `Line` (primitives import becomes `{PrimitiveStyle, Rectangle, StyledDrawable}`) and `core::fmt::Write`.

`mod.rs`:
- `pub mod browser;` after `pub mod block_registry;`.
- Both `Renderer::draw_sound_browser(display, &self.pool, part, cursor, scroll, self.performance.parts[part].sound.chain_type);` → `browser::draw(display, &self.pool, part, cursor, scroll);`.
- In the browser input: `Renderer::BROWSER_TOTAL_ENTRIES` → `browser::TOTAL_ENTRIES`, `Renderer::BROWSER_VISIBLE_ROWS` → `browser::VISIBLE_ROWS`, and the local `init_types` array block becomes:

```rust
                    // Init entries follow the pool slots.
                    if let Some(&ct) = browser::INIT_TYPES.get(sel_cursor - POOL_SIZE) {
                        self.performance.parts[sel_part].load_init(ct);
                    }
```

- [ ] **Step 4: Run to see it pass** — `cargo test -p chimera-core --test browser_test --test mixer_page_test --test screen_golden_test` → all pass (including `no_screen_draws_outside_240x320`: a single footer line `MAIN ENCODER SCROLLS · 3 INIT + 32 SLOTS` overflowed 240 px in validation, hence two strings). Compare `sound_browser` with `drawBrowser`.

- [ ] **Step 5: Lock** — `("sound_browser", Locked(0x91b39fd39c530bcd)),`. Every case is now `Locked`.

- [ ] **Step 6: `just check`, then commit** (core+hal 508 passed)

```bash
git add -A chimera-core
git commit -m "feat(core): sound browser in Direction A

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 13: Remove the legacy drawing; goldens become plain hashes; measure

**Files:**
- Delete: `chimera-core/src/ui/cell.rs`
- Modify: `chimera-core/src/ui/mod.rs`, `block_def.rs`, `page.rs`, `block_registry.rs`, `theme.rs`
- Rename: `chimera-core/tests/cell_icon_test.rs` → `chimera-core/tests/valfmt_snap_test.rs`
- Modify: `chimera-core/tests/screen_golden_test.rs`, `chimera-core/tests/memory_budget_test.rs`
- Modify: `docs/adr/0016-visual-direction-refined-elektron.md` (measured cost)

**Interfaces:**
- Produces: `ParamSlot::{param(block, param), selected_op(param), select_op(), legacy(label, fmt)}` without the icon argument; no `CellIcon`; `GOLDENS: &[(&str, u64)]`.

- [ ] **Step 1: Write the failing budget test** — append to `memory_budget_test.rs`:

```rust

/// UiState lives on `main`'s stack in AXI. Its Performance and SoundPool
/// are counted on their own in `axi_residents_fit`; the rest (navigation,
/// renderer, regions, focus: 1 120 B after the UI refresh, +48 B for the
/// per-page focus) comes out of the UI reserve.
#[test]
fn ui_state_fits_the_ui_reserve() {
    use chimera_core::preset::{Performance, SoundPool};
    let rest = size_of::<chimera_core::ui::UiState>() - size_of::<Performance>() - size_of::<SoundPool>();
    eprintln!("UiState without Performance and SoundPool = {rest} B, reserve {} B", hw::UI_RESERVE);
    assert!(rest <= 2 * 1024, "UiState grew to {rest} B besides its Performance and SoundPool");
}
```

Run: `cargo test -p chimera-core --test memory_budget_test -- --nocapture` → passes and prints `1120 B` (a guard, not a red step: it pins the measured size).

- [ ] **Step 2: Delete the legacy code**

- `git rm chimera-core/src/ui/cell.rs`; remove `pub mod cell;` from `ui/mod.rs`.
- `page.rs`: delete `enum CellIcon` with its doc comment.
- `block_def.rs`: `use crate::ui::page::{PageLayout, ValFmt};`; delete the `pub icon: CellIcon,` field; the constructors become

```rust
    pub const EMPTY: ParamSlot = ParamSlot { binding: SlotBinding::Empty, label_override: None };

    pub const fn param(block: BlockRef, param: ParamId) -> Self {
        Self { binding: SlotBinding::Param(ParamAddr::new(block, param)), label_override: None }
    }

    pub const fn selected_op(param: ParamId) -> Self {
        Self { binding: SlotBinding::SelectedOp(param), label_override: None }
    }

    pub const fn select_op() -> Self {
        Self { binding: SlotBinding::SelectOp, label_override: None }
    }

    pub const fn legacy(label: &'static str, fmt: ValFmt) -> Self {
        Self { binding: SlotBinding::Legacy { label, fmt }, label_override: None }
    }
```

- `block_registry.rs`: `use crate::ui::page::{PageLayout, ValFmt};` and drop every icon argument: `perl -0pi -e 's/,\s*CellIcon::[A-Za-z]+\)/)/g; s/\(CellIcon::[A-Za-z]+\)/()/g' chimera-core/src/ui/block_registry.rs` (181 occurrences; `grep -c CellIcon` must then print 0).
- `theme.rs`: delete everything from `// --- Legacy (pre-Direction A) names` to the end.
- `cell_icon_test.rs` → `git mv` to `valfmt_snap_test.rs`; delete its `// ── Icon frame quantization` test (it tested a local closure) and the three `fold_wave` tests (`fold_wave` is gone; the DSP folder has its own); add the module doc `//! Snap points of the value formats (shift + encoder).`.
- `screen_golden_test.rs`: remove `enum Golden` and `use Golden::*;`; `GOLDENS: &[(&str, u64)]` with the twelve hashes below; the doc paragraph becomes "Every case must match bit-for-bit. Re-record a case ONLY for a change that is meant to alter that screen, in the commit that makes it:"; the record line prints `println!("    (\"{name}\", 0x{hash:016x}),");`; the comparison is `for &(name, want) in GOLDENS { … if hash != want { … } }`.

```rust
const GOLDENS: &[(&str, u64)] = &[
    ("engine_pizza", 0x4797d6f7d4edc427),
    ("engine_fm_alg", 0x48b0b28b69670bc6),
    ("engine_fm_op", 0x2353d264169904b3),
    ("bigviz_filter", 0x3bf9b75c5812d82f),
    ("bigviz_env", 0x16e11d64abffc08c),
    ("bigviz_fm_op_env", 0x928f9fb2b436c6d0),
    ("mixer_part", 0x3e3480eee3041370),
    ("mixer_sends", 0xfc90ed4f83130581),
    ("mixer_fx_delay", 0x4b5f420a1ec41f11),
    ("mod_matrix", 0xb66137bb078dc484),
    ("sound_browser", 0x91b39fd39c530bcd),
    ("system", 0x02f7456a841f9613),
];
```

- [ ] **Step 3: Prove the deletion is pure** — `cargo test -p chimera-core --test screen_golden_test` → every hash unchanged. `grep -rn "FONT_6X10\|FONT_4X6\|mono_font\|CellIcon" chimera-core/src` → no output.

- [ ] **Step 4: Measure and record**

```bash
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
/home/carcosa/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-size -A target/thumbv7em-none-eabihf/release/chimera-stm32 | grep -E 'text|rodata|\.data|bss'
```

Expected (validation run): `.text 171536`, `.rodata 47096`, `.data 40696`, `.bss 156844`. Append to ADR 0016 (under the Font licences addendum):

```markdown

Measured cost (release firmware, `llvm-size -A`, UI refresh final task):
`.text + .rodata` 215 316 → 218 632 B (+3 316 B). `.rodata` +11 676 B, of
which font data is 10 253 B (logisoso42 4 625, logisoso20 2 226, helvB10
1 333, helvR08 1 041, helvB08 1 028); `.text` −8 360 B (the 6×10 mono font,
cell icons and pre-refresh vizzes are gone). `.data`/`.bss` unchanged;
`UiState` +48 B (per-page focus).
```

(If your numbers differ, record yours; the budget is fonts ≤ 16 KB and `.data`/`.bss` unchanged.)

- [ ] **Step 5: `just check`, manual look, commit**

`just check` → core+hal 505 passed, 2 ignored; desktop 2 passed; firmware links. Manual: `just desktop`, visit every chain (B1–B6, MIX+B1, MENU, MIX+B6, EDIT+B1) and compare with the mockups; `SCREEN_DUMP=/tmp/final cargo test -p chimera-core --test screen_golden_test` gives the same screens as files.

```bash
git add -A chimera-core docs/adr/0016-visual-direction-refined-elektron.md
git commit -m "refactor(core): remove cell icons, CellIcon and legacy theme names; goldens all locked

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 14: Screens in the README (regenerated from the real renderer)

User request (2026-09-24): the README shows the rendered screens, and they must stay in sync with the firmware renderer.

**Files:**
- Modify: `Justfile` (new `screens` recipe)
- Create: `docs/screens/*.png` (one per screen-golden case, 2× nearest-neighbour)
- Modify: `README.md` (new "Screens" section)

**Interfaces:**
- Consumes: `SCREEN_DUMP=<dir>` from Task 1 (writes `<dir>/<case>.ppm` for every screen-golden case).
- Produces: `just screens` — regenerates `docs/screens/*.png`.

- [ ] **Step 1: Add the recipe** to `Justfile`:

```make
# Render every screen-golden case with the real renderer and write
# docs/screens/<case>.png at 2x (nearest neighbour). Needs ImageMagick (`magick`).
screens:
    rm -rf target/screens && mkdir -p target/screens docs/screens
    SCREEN_DUMP=target/screens cargo test -p chimera-core --test screen_golden_test -q
    for f in target/screens/*.ppm; do magick "$f" -filter point -resize 200% "docs/screens/$(basename "$f" .ppm).png"; done
```

(If the screen-golden test file has a different name, use the one Task 1 created; list it in the report.)

- [ ] **Step 2: Run it** — `just screens`. Expected: one PNG per case in `docs/screens/` (12 files: engine_pizza, engine_fm_alg, engine_fm_op, bigviz_filter, bigviz_env, bigviz_fm_op_env, mixer_part, mixer_sends, mixer_fx_delay, mod_matrix, sound_browser, system), each 480×640.

- [ ] **Step 3: README section.** Add after the project description in `README.md`:

```markdown
## Screens

Rendered by the firmware's own renderer (`chimera-core`) at the display's native 240×320, shown at 2×. Regenerate with `just screens` after any UI change.

| Engine (Pizza) | Filter | Envelope |
|---|---|---|
| ![Pizza engine page](docs/screens/engine_pizza.png) | ![Filter page](docs/screens/bigviz_filter.png) | ![Amp envelope page](docs/screens/bigviz_env.png) |

| FM algorithm | FM operator | Mod matrix |
|---|---|---|
| ![FM algorithm page](docs/screens/engine_fm_alg.png) | ![FM operator page](docs/screens/engine_fm_op.png) | ![Mod matrix page](docs/screens/mod_matrix.png) |

| Mixer · Part | Mixer · Sends | Sound browser |
|---|---|---|
| ![Mixer part page](docs/screens/mixer_part.png) | ![Mixer sends page](docs/screens/mixer_sends.png) | ![Sound browser](docs/screens/sound_browser.png) |
```

- [ ] **Step 4: Verify** the README renders (images exist at the referenced paths: `for f in $(grep -o 'docs/screens/[a-z_]*.png' README.md); do test -f "$f" || echo MISSING $f; done` prints nothing), `just check` still green.

- [ ] **Step 5: Commit**

```bash
git add Justfile README.md docs/screens/
git commit -m "docs: README shows the rendered screens; just screens regenerates them

Co-Authored-By: <model that writes it> <noreply@anthropic.com>"
```

## Self-review notes

- Spec coverage: principles (Tasks 2, 4, 6), header (5), focus band (3, 6, 11), viz band (6, 8, 9, 10), cells (6), map (5), every page type (6–12), theme as single source (2, 13), fonts + licences + flash (2, 13), RAM (13), dirty regions with a `Focus` region and no scope strip (6), focus tracking (3), legacy removal (6, 7, 12, 13), goldens (1, locked in 6–12), behaviour tests (focus follows/survives: 3, 6; POLY/P1/CH/L-C-R: 4 + existing `mixer_page_test`; nothing off screen: 6; accent on selection: 6, 9, 10, 11, 12; lerp: 6, 8, 11), builds (every task), manual (13).
- Validation: every task was executed in a scratch worktree of `ui-refresh`; all goldens, test counts and sizes above come from that run.
