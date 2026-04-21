# Mod Matrix PageLayout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the mod matrix its own `PageLayout::Matrix` variant with dedicated cursor state, region tracking, drawing path, and encoder handling — removing all the RoutingMatrix hacks from CellGrid/BigViz.

**Architecture:** Add `PageLayout::Matrix` to the layout enum. This layout has two regions: a full-content Grid region (Y: 28-266) and the Nav region (Y: 266-320). The grid state (cursor row/col, scroll x/y, selected amount) lives in a `MatrixState` struct on `UiState`, not in the animated values or ParamSnapshot. The mod grid renderer reads from `MatrixState`. Encoders on a Matrix page directly update `MatrixState` fields instead of going through the `PageId` param binding system.

**Tech Stack:** Rust, `no_std`, `chimera-core` crate

**Spec:** `docs/chimera-modulation-spec.md`

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `chimera-core/src/ui/page.rs` | **Modify** | Add `PageLayout::Matrix` variant |
| `chimera-core/src/ui/region.rs` | **Modify** | Add `Matrix` layout to `set_layout()` with Grid + Nav regions. Add `RegionKind::Grid` and `RegionData::Grid`. |
| `chimera-core/src/ui/mod_grid.rs` | **Modify** | Accept `MatrixState` instead of `&[f32; 6]`. Own cursor/scroll/selection logic. |
| `chimera-core/src/ui/mod.rs` | **Modify** | Add `MatrixState` to `UiState`. Handle Matrix-specific encoder input. Route Matrix layout through grid renderer. |
| `chimera-core/src/ui/renderer.rs` | **Modify** | Add `Matrix` layout dispatch in `draw_with_def` and `draw_region_with_def`. Remove RoutingMatrix hacks from BigViz/CellGrid paths. Pass `MatrixState` to grid draw. |
| `chimera-core/src/ui/block_registry.rs` | **Modify** | Update `DEMO_MATRIX` BlockDef to use `PageLayout::Matrix` |

---

### Task 1: Add MatrixState struct and PageLayout::Matrix

**Files:**
- Modify: `chimera-core/src/ui/page.rs`
- Create: Add `MatrixState` to `chimera-core/src/ui/mod_grid.rs`

- [ ] **Step 1: Add `PageLayout::Matrix` variant**

In `chimera-core/src/ui/page.rs`, add to the `PageLayout` enum:

```rust
pub enum PageLayout {
    BigViz,
    CellGrid,
    Matrix,  // NEW: full-content grid with own cursor state
}
```

- [ ] **Step 2: Add `MatrixState` struct to `mod_grid.rs`**

Add at the top of `chimera-core/src/ui/mod_grid.rs`:

```rust
/// State for the mod matrix grid — cursor, scroll, and grid data.
/// Lives on UiState, not in animated values or ParamSnapshot.
#[derive(Clone, Debug)]
pub struct MatrixState {
    /// Cursor position
    pub sel_row: usize,
    pub sel_col: usize,
    /// Scroll offset (for grids larger than visible area)
    pub scroll_x: usize,
    pub scroll_y: usize,
    /// Number of source rows
    pub num_rows: usize,
    /// Number of destination columns
    pub num_cols: usize,
}

impl Default for MatrixState {
    fn default() -> Self {
        Self {
            sel_row: 0,
            sel_col: 0,
            scroll_x: 0,
            scroll_y: 0,
            num_rows: SOURCES.len(),
            num_cols: DESTS.len(),
        }
    }
}

impl MatrixState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move cursor row, clamping to bounds
    pub fn move_row(&mut self, delta: i8) {
        if delta > 0 && self.sel_row + 1 < self.num_rows {
            self.sel_row += 1;
        } else if delta < 0 && self.sel_row > 0 {
            self.sel_row -= 1;
        }
        // Auto-scroll to keep cursor visible
        let visible_rows = self.visible_rows();
        if self.sel_row >= self.scroll_y + visible_rows {
            self.scroll_y = self.sel_row + 1 - visible_rows;
        }
        if self.sel_row < self.scroll_y {
            self.scroll_y = self.sel_row;
        }
    }

    /// Move cursor column, clamping to bounds
    pub fn move_col(&mut self, delta: i8) {
        if delta > 0 && self.sel_col + 1 < self.num_cols {
            self.sel_col += 1;
        } else if delta < 0 && self.sel_col > 0 {
            self.sel_col -= 1;
        }
        let visible_cols = self.visible_cols();
        if self.sel_col >= self.scroll_x + visible_cols {
            self.scroll_x = self.sel_col + 1 - visible_cols;
        }
        if self.sel_col < self.scroll_x {
            self.scroll_x = self.sel_col;
        }
    }

    /// Scroll vertically without moving cursor
    pub fn scroll_v(&mut self, delta: i8) {
        let max = self.num_rows.saturating_sub(self.visible_rows());
        if delta > 0 && self.scroll_y < max {
            self.scroll_y += 1;
        } else if delta < 0 && self.scroll_y > 0 {
            self.scroll_y -= 1;
        }
    }

    /// Scroll horizontally without moving cursor
    pub fn scroll_h(&mut self, delta: i8) {
        let max = self.num_cols.saturating_sub(self.visible_cols());
        if delta > 0 && self.scroll_x < max {
            self.scroll_x += 1;
        } else if delta < 0 && self.scroll_x > 0 {
            self.scroll_x -= 1;
        }
    }

    fn visible_rows(&self) -> usize {
        ((GRID_BOTTOM - GRID_TOP - COL_HEADER_H) / CELL_H) as usize
    }

    fn visible_cols(&self) -> usize {
        ((240 - ROW_LABEL_W) / CELL_W) as usize
    }
}
```

- [ ] **Step 3: Update `draw_grid` to accept `&MatrixState`**

Change the `draw_grid` function signature:

```rust
pub fn draw_grid<D>(display: &mut D, state: &MatrixState)
where D: DrawTarget<Color = Rgb565>,
{
    let sel_row = state.sel_row;
    let sel_col = state.sel_col;
    let scroll_x = state.scroll_x;
    let scroll_y = state.scroll_y;
    // ... rest of rendering unchanged
```

Remove the old `anim` parameter and the derivation of sel_row/sel_col/scroll from anim values.

- [ ] **Step 4: Build and test**

Run: `cargo build -p chimera-desktop`
Expected: May have compile errors from callers of `draw_grid` — those are fixed in later tasks.

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/page.rs chimera-core/src/ui/mod_grid.rs
git commit -m "feat(core): add MatrixState + PageLayout::Matrix variant"
```

---

### Task 2: Add Matrix layout to region tracking

**Files:**
- Modify: `chimera-core/src/ui/region.rs`

- [ ] **Step 1: Add `RegionKind::Grid` and `RegionData::Grid`**

```rust
pub enum RegionKind {
    Header,
    Viz,
    Params,
    Cells,
    Grid,   // NEW: mod matrix grid — full content zone
    Nav,
}

pub enum RegionData {
    // ... existing variants ...
    Grid {
        sel_row: u8,
        sel_col: u8,
        scroll_x: u8,
        scroll_y: u8,
        // Could add a hash of amounts later for value-change detection
    },
}
```

Add sentinel and constructor:

```rust
impl RegionData {
    pub fn grid(sel_row: u8, sel_col: u8, scroll_x: u8, scroll_y: u8) -> Self {
        Self::Grid { sel_row, sel_col, scroll_x, scroll_y }
    }

    pub fn sentinel_grid() -> Self {
        Self::Grid { sel_row: 255, sel_col: 255, scroll_x: 255, scroll_y: 255 }
    }
}
```

- [ ] **Step 2: Add `Matrix` layout to `set_layout()`**

```rust
PageLayout::Matrix => {
    self.count = 2;
    self.regions[0] = Region {
        kind: RegionKind::Grid, y_start: 0, y_end: 266,
        prev_data: RegionData::sentinel_grid(),
    };
    self.regions[1] = Region {
        kind: RegionKind::Nav, y_start: 266, y_end: 320,
        prev_data: RegionData::sentinel_nav(),
    };
}
```

Note: The Grid region covers Y: 0-266 (includes header area). The grid renderer draws the header itself as part of the grid. This simplifies the region model — one region for everything above the dungeon map.

- [ ] **Step 3: Build**

Run: `cargo build -p chimera-desktop`

- [ ] **Step 4: Commit**

```bash
git add chimera-core/src/ui/region.rs
git commit -m "feat(core): add Grid region kind + Matrix layout to dirty tracking"
```

---

### Task 3: Wire MatrixState into UiState and handle encoders

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`

- [ ] **Step 1: Add `MatrixState` to `UiState`**

```rust
use crate::ui::mod_grid::MatrixState;

pub struct UiState {
    pub nav: ChainNav,
    pub params: ParamSnapshot,
    pub renderer: Renderer,
    pub matrix_state: MatrixState,  // NEW
    page: PageId,
    region_set: region::RegionSet,
}
```

Initialize in `new()`:

```rust
matrix_state: MatrixState::new(),
```

- [ ] **Step 2: Handle Matrix encoder input separately**

In `handle_input()`, before the normal encoder handling, check if the current page is a Matrix layout and handle encoders differently:

```rust
let def = self.nav.active_block_def();

if def.layout == PageLayout::Matrix {
    // Matrix page: encoders control grid state, not params
    for (i, &enc) in encoder_ids.iter().enumerate() {
        let delta = controls.encoder_delta(enc);
        if delta != 0 {
            match i {
                0 => self.matrix_state.move_row(delta),   // A: cursor row
                1 => self.matrix_state.move_col(delta),   // B: cursor col
                2 => self.matrix_state.scroll_v(delta),   // C: scroll V
                3 => self.matrix_state.scroll_h(delta),   // D: scroll H
                4 => { /* E: amount — TODO wire to mod matrix data */ }
                _ => {}
            }
        }
    }
} else {
    // Normal page: encoders update params via PageId
    // ... existing encoder handling code ...
}
```

- [ ] **Step 3: Update `render_dirty` to handle Grid region**

In the `render_dirty` match on `RegionKind`, add `Grid`:

```rust
RegionKind::Grid => RegionData::grid(
    self.matrix_state.sel_row as u8,
    self.matrix_state.sel_col as u8,
    self.matrix_state.scroll_x as u8,
    self.matrix_state.scroll_y as u8,
),
```

And in `prime_regions`, same.

- [ ] **Step 4: Build**

Run: `cargo build -p chimera-desktop`

- [ ] **Step 5: Commit**

```bash
git add chimera-core/src/ui/mod.rs
git commit -m "feat(core): MatrixState in UiState, encoders drive grid cursor"
```

---

### Task 4: Update renderer for Matrix layout

**Files:**
- Modify: `chimera-core/src/ui/renderer.rs`

- [ ] **Step 1: Add Matrix layout dispatch in `draw_with_def`**

```rust
match def.layout {
    PageLayout::BigViz => {
        self.draw_viz_from_type(display, def.viz);
        self.draw_params_from_def(display, def);
    }
    PageLayout::CellGrid => {
        self.draw_cell_grid_from_def(display, def);
    }
    PageLayout::Matrix => {
        // Matrix draws header + grid + separator — no params, no cell grid
        // Grid is drawn by the region handler, not here
        // (full render calls draw_grid directly)
    }
}
```

Actually, for the full render path (`draw_with_def`), draw the grid directly:

```rust
PageLayout::Matrix => {
    // mod_grid::draw_grid handles everything in the content zone
    // The MatrixState is passed from UiState
    // We need access to it — add matrix_state param or store on Renderer
}
```

The challenge: `draw_with_def` doesn't have access to `MatrixState`. Two options:
- Pass `&MatrixState` as an extra parameter
- Store a reference on `Renderer`

Simplest: add an optional `&MatrixState` parameter to `draw_with_def` and `draw_region_with_def`. For non-Matrix pages it's ignored.

```rust
pub fn draw_with_def<D>(
    &self, display: &mut D, nav: &ChainNav, def: &BlockDef,
    perf: &PerfStats, matrix: &MatrixState,
)
```

For the Matrix layout:
```rust
PageLayout::Matrix => {
    crate::ui::mod_grid::draw_grid(display, matrix);
}
```

- [ ] **Step 2: Add Grid region handling in `draw_region_with_def`**

```rust
RegionKind::Grid => {
    self.draw_header_with_def(display, nav, def);
    self.draw_perf(display, perf);
    crate::ui::mod_grid::draw_grid(display, matrix);
    let _ = Line::new(
        Point::new(0, theme::ENCODER_ZONE_BOTTOM),
        Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
    )
    .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);
}
```

- [ ] **Step 3: Remove all RoutingMatrix hacks from BigViz/CellGrid paths**

Remove:
- `if def.viz != VizType::RoutingMatrix` check in BigViz Params
- `if def.viz == VizType::RoutingMatrix` check in CellGrid
- `if def.viz == VizType::RoutingMatrix` check in RegionKind::Cells
- The `RoutingMatrix` arm in `draw_viz_from_type` can stay (it's now only called from the Matrix path)

Clean up so BigViz and CellGrid are pure again — no special-casing for RoutingMatrix.

- [ ] **Step 4: Update all callers of draw_with_def and draw_region_with_def**

In `chimera-core/src/ui/mod.rs`:
- `render()` passes `&self.matrix_state`
- `render_dirty()` passes `&self.matrix_state`

- [ ] **Step 5: Build and test**

Run: `cargo build -p chimera-desktop && cargo test -p chimera-core`

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/ui/renderer.rs chimera-core/src/ui/mod.rs
git commit -m "feat(core): Matrix layout in renderer — clean separation from BigViz/CellGrid"
```

---

### Task 5: Update DEMO_MATRIX BlockDef and clean up

**Files:**
- Modify: `chimera-core/src/ui/block_registry.rs`
- Modify: `chimera-core/src/ui/page.rs` (remove DemoMatrix param binding hacks)

- [ ] **Step 1: Update DEMO_MATRIX to use `PageLayout::Matrix`**

```rust
pub static DEMO_MATRIX: BlockDef = BlockDef {
    name: "Matrix",
    short: "MTX",
    layout: PageLayout::Matrix,
    viz: VizType::RoutingMatrix,
    params: [EMPTY; 6],  // Matrix doesn't use the 6-param system
};
```

- [ ] **Step 2: Clean up PageId::DemoMatrix**

Remove the `DemoMatrix` read_values and resolve_param_mut entries from page.rs — the Matrix page doesn't go through the PageId param binding system.

In `read_values`, DemoMatrix returns `[0.0; 6]`.
In `resolve_param_mut`, DemoMatrix returns `None`.

- [ ] **Step 3: Build everything**

Run: `cargo test -p chimera-core && cargo build -p chimera-desktop && cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf`

- [ ] **Step 4: Commit**

```bash
git add chimera-core/src/ui/block_registry.rs chimera-core/src/ui/page.rs
git commit -m "refactor(core): DEMO_MATRIX uses PageLayout::Matrix, remove param binding hacks"
```

---

### Task 6: Flash and verify on hardware

- [ ] **Step 1: Build and flash**

```bash
cargo build --release -p chimera-stm32 --target thumbv7em-none-eabihf
rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin
dfu-util -a0 -d 0x0483:0xdf11 -D chimera.bin -s 0x8020000:leave
```

- [ ] **Step 2: Verify mod matrix**

MIX+B6 → Plus×3 → Matrix page:
- Encoder A moves cursor row (highlighted row changes)
- Encoder B moves cursor column
- Cursor auto-scrolls to stay visible
- Encoder C scrolls vertically without moving cursor
- Encoder D scrolls horizontally
- Full grid visible (no dirty region clipping)
- Dungeon map renders correctly below the grid

- [ ] **Step 3: Verify normal pages unaffected**

B1 → Pizza page — params display and edit normally, no RoutingMatrix hacks interfering.

- [ ] **Step 4: Commit**

```bash
git add chimera.bin
git commit -m "feat(core): mod matrix PageLayout — proper cursor/scroll on hardware"
```
