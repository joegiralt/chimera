# Dirty Region Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Only redraw and flush screen regions whose data actually changed, reducing encoder-turn response from ~1s to ~50-80ms.

**Architecture:** Each `PageLayout` defines tiled screen regions with data snapshots. Each frame, compare current vs previous data per region. Only clear+draw+flush regions that differ. Animations keep regions dirty until settled.

**Tech Stack:** Rust no_std, embedded-graphics, ILI9341 via bit-bang SPI, chimera-core/chimera-hal/chimera-stm32 workspace.

**Spec:** `docs/superpowers/specs/2026-04-19-dirty-region-tracking-design.md`

---

## File Map

| Action | File | Responsibility |
|---|---|---|
| Create | `chimera-core/src/ui/region.rs` | `RegionData`, `RegionKind`, `Region`, `RegionSet`, `quantize()`, layout-to-regions mapping, dirty comparison |
| Modify | `chimera-core/src/ui/mod.rs` | Add `pub mod region`, add `render_dirty()` method to `UiState` |
| Modify | `chimera-core/src/ui/renderer.rs` | Refactor draw methods to accept Y-bounded clipping, add per-region draw dispatch |
| Modify | `chimera-core/src/ui/page.rs` | Derive/add `Copy`+`PartialEq`+`Eq` to `PageLayout` if missing |
| Create | `chimera-core/tests/region_tests.rs` | All unit tests for dirty tracking logic |
| Modify | `chimera-hal/src/lib.rs` | Add `flush_region(y_start, y_end)` to `ChimeraDisplay` trait |
| Modify | `chimera-stm32/src/display.rs` | Implement `flush_region()` with SPI address windowing |
| Modify | `chimera-stm32/src/main.rs` | New main loop: snapshot → handle_input → update → render_dirty → flush dirty regions → wfi |

---

### Task 1: Add `flush_region` to HAL trait

**Files:**
- Modify: `chimera-hal/src/lib.rs:89-95`

- [ ] **Step 1: Add `flush_region` to `ChimeraDisplay` trait**

In `chimera-hal/src/lib.rs`, add to the `ChimeraDisplay` trait:

```rust
pub trait ChimeraDisplay: DrawTarget<Color = Rgb565> {
    /// Push framebuffer to hardware
    fn flush(&mut self);

    /// Push a horizontal band of the framebuffer (y_start inclusive, y_end exclusive)
    fn flush_region(&mut self, y_start: u16, y_end: u16);

    /// Raw pixel access for custom rendering
    fn pixel_buffer(&mut self) -> &mut [u16];
}
```

- [ ] **Step 2: Verify workspace compiles**

Run: `cargo check --workspace 2>&1 | head -20`
Expected: Compile errors in chimera-stm32 (missing `flush_region` impl) — that's correct, we'll fix it in Task 2.

- [ ] **Step 3: Commit**

```bash
git add chimera-hal/src/lib.rs
git commit -m "feat(hal): add flush_region to ChimeraDisplay trait"
```

---

### Task 2: Implement `flush_region` in STM32 display driver

**Files:**
- Modify: `chimera-stm32/src/display.rs:130-159`

- [ ] **Step 1: Add `flush_region` implementation**

Add to the `impl ChimeraDisplay for Stm32Display` block in `chimera-stm32/src/display.rs`:

```rust
fn flush_region(&mut self, y_start: u16, y_end: u16) {
    // Column address: 0-239
    self.cmd_data(0x2A, &[0x00, 0x00, 0x00, 0xEF]);
    // Row address: y_start to y_end-1
    let ys = y_start.to_be_bytes();
    let ye = (y_end - 1).to_be_bytes();
    self.cmd_data(0x2B, &[ys[0], ys[1], ye[0], ye[1]]);
    self.cmd(0x2C); // RAMWR

    let _ = self.dc.set_high();
    let _ = self.cs.set_low();

    let start = y_start as usize * SCREEN_WIDTH as usize;
    let end = y_end as usize * SCREEN_WIDTH as usize;
    let mut bytes = [0u8; 512];
    for chunk in self.fb[start..end].chunks(256) {
        for (i, &pixel) in chunk.iter().enumerate() {
            bytes[i * 2] = (pixel >> 8) as u8;
            bytes[i * 2 + 1] = pixel as u8;
        }
        let _ = self.spi.write(&bytes[..chunk.len() * 2]);
    }

    let _ = self.cs.set_high();
}
```

- [ ] **Step 2: Verify firmware compiles**

Run: `cargo build --release 2>&1 | grep "^error"`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add chimera-stm32/src/display.rs
git commit -m "feat(stm32): implement flush_region for partial ILI9341 updates"
```

---

### Task 3: Create `region.rs` with core types and quantization

**Files:**
- Create: `chimera-core/src/ui/region.rs`
- Modify: `chimera-core/src/ui/mod.rs:1` (add `pub mod region;`)
- Create: `chimera-core/tests/region_tests.rs`

- [ ] **Step 1: Write tests for quantization and RegionData equality**

Create `chimera-core/tests/region_tests.rs`:

```rust
use chimera_core::ui::region::{quantize, RegionData, RegionKind, Region, RegionSet, PageLayout};

#[test]
fn quantize_zero() {
    assert_eq!(quantize(0.0), 0);
}

#[test]
fn quantize_one() {
    assert_eq!(quantize(1.0), 1000);
}

#[test]
fn quantize_half() {
    assert_eq!(quantize(0.5), 500);
}

#[test]
fn quantize_clamps_negative() {
    assert_eq!(quantize(-1.0), 0);
}

#[test]
fn quantize_stability_tiny_jitter() {
    // Values within 0.001 should quantize the same
    let a = quantize(0.5);
    let b = quantize(0.5005);
    assert_eq!(a, b);
}

#[test]
fn region_data_same_is_equal() {
    let a = RegionData::header(0, 1, 2, 0);
    let b = RegionData::header(0, 1, 2, 0);
    assert_eq!(a, b);
}

#[test]
fn region_data_diff_is_not_equal() {
    let a = RegionData::header(0, 1, 2, 0);
    let b = RegionData::header(0, 1, 3, 0);
    assert_ne!(a, b);
}

#[test]
fn region_data_params_values_differ() {
    use chimera_core::ui::page::PageId;
    let a = RegionData::params(PageId::Filter, [500; 6]);
    let b = RegionData::params(PageId::Filter, [501, 500, 500, 500, 500, 500]);
    assert_ne!(a, b);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p chimera-core --test region_tests 2>&1 | tail -5`
Expected: Compile error — `region` module doesn't exist yet.

- [ ] **Step 3: Create `region.rs` with types**

Create `chimera-core/src/ui/region.rs`:

```rust
//! Dirty region tracking for partial display updates.
//!
//! Each PageLayout defines screen regions with data snapshots.
//! Only regions whose data changed get cleared, redrawn, and flushed.

use crate::ui::page::{PageId, PageLayout};

/// Quantize a float to u16 for cheap comparison. Range 0.0..65.0 → 0..65000.
pub fn quantize(f: f32) -> u16 {
    (f.clamp(0.0, 65.0) * 1000.0) as u16
}

/// Sentinel value that never matches real data — forces initial redraw.
const SENTINEL: u16 = u16::MAX;

/// Data snapshot for a screen region. If current != previous, region is dirty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionData {
    Header {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        render_us: u32,
    },
    Viz {
        page: PageId,
        values: [u16; 6],
    },
    Params {
        page: PageId,
        values: [u16; 6],
    },
    Cells {
        page: PageId,
        values: [u16; 6],
    },
    Nav {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
    },
}

impl RegionData {
    pub fn header(chain_idx: u8, node_idx: u8, sub_page: u8, render_us: u32) -> Self {
        Self::Header { chain_idx, node_idx, sub_page, render_us }
    }

    pub fn viz(page: PageId, values: [u16; 6]) -> Self {
        Self::Viz { page, values }
    }

    pub fn params(page: PageId, values: [u16; 6]) -> Self {
        Self::Params { page, values }
    }

    pub fn cells(page: PageId, values: [u16; 6]) -> Self {
        Self::Cells { page, values }
    }

    pub fn nav(chain_idx: u8, node_idx: u8, sub_page: u8) -> Self {
        Self::Nav { chain_idx, node_idx, sub_page }
    }

    /// Sentinel that never matches — used to force initial redraw.
    pub fn sentinel_header() -> Self {
        Self::Header { chain_idx: 255, node_idx: 255, sub_page: 255, render_us: u32::MAX }
    }

    pub fn sentinel_viz() -> Self {
        Self::Viz { page: PageId::Filter, values: [SENTINEL; 6] }
    }

    pub fn sentinel_params() -> Self {
        Self::Params { page: PageId::Filter, values: [SENTINEL; 6] }
    }

    pub fn sentinel_cells() -> Self {
        Self::Cells { page: PageId::Filter, values: [SENTINEL; 6] }
    }

    pub fn sentinel_nav() -> Self {
        Self::Nav { chain_idx: 255, node_idx: 255, sub_page: 255 }
    }
}

/// Which draw method to dispatch for a region.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    Header,
    Viz,
    Params,
    Cells,
    Nav,
}

/// A screen region with Y bounds and cached data.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub kind: RegionKind,
    pub y_start: u16,
    pub y_end: u16,
    pub prev_data: RegionData,
}

pub const MAX_REGIONS: usize = 5;

/// Tracks the region list for the current page layout.
pub struct RegionSet {
    pub regions: [Region; MAX_REGIONS],
    pub count: u8,
    pub prev_layout: Option<PageLayout>,
}

impl RegionSet {
    pub fn new() -> Self {
        Self {
            regions: [Region {
                kind: RegionKind::Header,
                y_start: 0,
                y_end: 0,
                prev_data: RegionData::sentinel_header(),
            }; MAX_REGIONS],
            count: 0,
            prev_layout: None,
        }
    }

    /// Rebuild the region list for a new layout. All regions start dirty (sentinel data).
    pub fn set_layout(&mut self, layout: PageLayout) {
        match layout {
            PageLayout::BigViz => {
                self.count = 4;
                self.regions[0] = Region {
                    kind: RegionKind::Header, y_start: 0, y_end: 28,
                    prev_data: RegionData::sentinel_header(),
                };
                self.regions[1] = Region {
                    kind: RegionKind::Viz, y_start: 28, y_end: 144,
                    prev_data: RegionData::sentinel_viz(),
                };
                self.regions[2] = Region {
                    kind: RegionKind::Params, y_start: 144, y_end: 214,
                    prev_data: RegionData::sentinel_params(),
                };
                self.regions[3] = Region {
                    kind: RegionKind::Nav, y_start: 214, y_end: 320,
                    prev_data: RegionData::sentinel_nav(),
                };
            }
            PageLayout::CellGrid => {
                self.count = 3;
                self.regions[0] = Region {
                    kind: RegionKind::Header, y_start: 0, y_end: 28,
                    prev_data: RegionData::sentinel_header(),
                };
                self.regions[1] = Region {
                    kind: RegionKind::Cells, y_start: 28, y_end: 214,
                    prev_data: RegionData::sentinel_cells(),
                };
                self.regions[2] = Region {
                    kind: RegionKind::Nav, y_start: 214, y_end: 320,
                    prev_data: RegionData::sentinel_nav(),
                };
            }
        }
        self.prev_layout = Some(layout);
    }

    /// Iterate active regions.
    pub fn active_regions(&self) -> &[Region] {
        &self.regions[..self.count as usize]
    }

    /// Iterate active regions mutably.
    pub fn active_regions_mut(&mut self) -> &mut [Region] {
        &mut self.regions[..self.count as usize]
    }
}
```

- [ ] **Step 4: Add `pub mod region;` to `chimera-core/src/ui/mod.rs`**

Add after the existing module declarations (line 1):

```rust
pub mod region;
```

- [ ] **Step 5: Ensure `PageId` and `PageLayout` derive required traits**

Check `chimera-core/src/ui/page.rs`. `PageLayout` needs `Clone, Copy, PartialEq, Eq`. `PageId` needs `Clone, Copy, PartialEq, Eq`. Add if missing.

- [ ] **Step 6: Run tests**

Run: `cargo test -p chimera-core --test region_tests 2>&1 | tail -10`
Expected: All 8 tests pass.

- [ ] **Step 7: Commit**

```bash
git add chimera-core/src/ui/region.rs chimera-core/src/ui/mod.rs chimera-core/tests/region_tests.rs
git commit -m "feat(core): add dirty region tracking types with tests"
```

---

### Task 4: Add tests for layout region definitions

**Files:**
- Modify: `chimera-core/tests/region_tests.rs`

- [ ] **Step 1: Write layout tests**

Append to `chimera-core/tests/region_tests.rs`:

```rust
use chimera_core::ui::region::RegionKind;

#[test]
fn big_viz_has_4_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    assert_eq!(rs.count, 4);
}

#[test]
fn cell_grid_has_3_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    assert_eq!(rs.count, 3);
}

#[test]
fn regions_tile_full_screen_big_viz() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    let regions = rs.active_regions();
    // First region starts at 0
    assert_eq!(regions[0].y_start, 0);
    // Last region ends at 320
    assert_eq!(regions[regions.len() - 1].y_end, 320);
    // No gaps between regions
    for i in 1..regions.len() {
        assert_eq!(regions[i].y_start, regions[i - 1].y_end,
            "gap between region {} and {}", i - 1, i);
    }
}

#[test]
fn regions_tile_full_screen_cell_grid() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    let regions = rs.active_regions();
    assert_eq!(regions[0].y_start, 0);
    assert_eq!(regions[regions.len() - 1].y_end, 320);
    for i in 1..regions.len() {
        assert_eq!(regions[i].y_start, regions[i - 1].y_end);
    }
}

#[test]
fn layout_change_resets_all_regions() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    // Manually set prev_data to non-sentinel for one region
    rs.regions[2].prev_data = RegionData::params(PageId::Filter, [500; 6]);

    // Switch layout — should reset all to sentinel
    rs.set_layout(PageLayout::CellGrid);
    for r in rs.active_regions() {
        // All prev_data should be sentinels (will never match real data)
        match r.prev_data {
            RegionData::Header { chain_idx: 255, .. } => {}
            RegionData::Cells { values, .. } if values == [u16::MAX; 6] => {}
            RegionData::Nav { chain_idx: 255, .. } => {}
            other => panic!("expected sentinel, got {:?}", other),
        }
    }
}

#[test]
fn big_viz_region_kinds() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);
    let kinds: Vec<RegionKind> = rs.active_regions().iter().map(|r| r.kind).collect();
    assert_eq!(kinds, vec![RegionKind::Header, RegionKind::Viz, RegionKind::Params, RegionKind::Nav]);
}

#[test]
fn cell_grid_region_kinds() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::CellGrid);
    let kinds: Vec<RegionKind> = rs.active_regions().iter().map(|r| r.kind).collect();
    assert_eq!(kinds, vec![RegionKind::Header, RegionKind::Cells, RegionKind::Nav]);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p chimera-core --test region_tests 2>&1 | tail -15`
Expected: All 15 tests pass.

- [ ] **Step 3: Commit**

```bash
git add chimera-core/tests/region_tests.rs
git commit -m "test(core): add layout region definition tests"
```

---

### Task 5: Add `render_dirty` to `UiState`

This is the main integration point. `UiState` gains a `render_dirty` method that computes current `RegionData` per region, compares, and only draws dirty regions via existing renderer methods.

**Files:**
- Modify: `chimera-core/src/ui/mod.rs`
- Modify: `chimera-core/src/ui/region.rs` (add data computation helpers)
- Modify: `chimera-core/src/ui/renderer.rs` (expose per-region draw methods, add `clear_region`)

- [ ] **Step 1: Add `quantize_values` helper to `region.rs`**

Append to `chimera-core/src/ui/region.rs`:

```rust
use crate::ui::animation::AnimatedValue;

/// Quantize 6 animated values into a [u16; 6] for snapshot comparison.
pub fn quantize_values(anim: &[AnimatedValue; 6]) -> [u16; 6] {
    [
        quantize(anim[0].current()),
        quantize(anim[1].current()),
        quantize(anim[2].current()),
        quantize(anim[3].current()),
        quantize(anim[4].current()),
        quantize(anim[5].current()),
    ]
}
```

- [ ] **Step 2: Add `clear_region` to renderer**

Add to `chimera-core/src/ui/renderer.rs`, inside the `impl Renderer` block:

```rust
/// Clear a screen region by direct framebuffer fill. Much faster than draw_iter.
pub fn clear_region_fb(fb: &mut [u16], y_start: u16, y_end: u16) {
    let bg = 0u16; // theme::BG is black = 0x0000
    let start = y_start as usize * 240;
    let end = y_end as usize * 240;
    for px in &mut fb[start..end] {
        *px = bg;
    }
}
```

- [ ] **Step 3: Add `draw_region` dispatch to renderer**

Add to `chimera-core/src/ui/renderer.rs`, inside the `impl Renderer` block:

```rust
/// Draw a single region by kind. The caller has already cleared the region.
pub fn draw_region<D>(
    &self,
    display: &mut D,
    kind: RegionKind,
    nav: &ChainNav,
    page: PageId,
    perf: &PerfStats,
)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::ui::region::RegionKind;
    match kind {
        RegionKind::Header => {
            self.draw_header(display, nav);
            self.draw_perf(display, perf);
        }
        RegionKind::Viz => {
            self.draw_visualization(display, page);
        }
        RegionKind::Params => {
            self.draw_params(display, page);
            // Separator line at bottom of params region
            let _ = Line::new(
                Point::new(0, theme::ENCODER_ZONE_BOTTOM),
                Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
            )
            .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);
        }
        RegionKind::Cells => {
            self.draw_cell_grid(display, page);
            let _ = Line::new(
                Point::new(0, theme::ENCODER_ZONE_BOTTOM),
                Point::new(theme::SCREEN_W - 1, theme::ENCODER_ZONE_BOTTOM),
            )
            .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);
        }
        RegionKind::Nav => {
            dungeon_map::draw(display, nav);
        }
    }
}
```

Add the required import at the top of `renderer.rs`:

```rust
use crate::ui::region::RegionKind;
```

- [ ] **Step 4: Add `render_dirty` to `UiState`**

Add to `chimera-core/src/ui/mod.rs`, inside `impl UiState`:

```rust
use crate::ui::region::{RegionData, RegionSet, quantize_values};
use chimera_hal::SCREEN_WIDTH;
```

Add `region_set: RegionSet` field to `UiState`, initialized in `new()`:

```rust
pub struct UiState {
    pub nav: ChainNav,
    pub params: ParamSnapshot,
    pub renderer: Renderer,
    page: PageId,
    region_set: region::RegionSet,
}
```

Update `new()`:

```rust
pub fn new() -> Self {
    // ... existing code ...
    Self {
        nav,
        params,
        renderer,
        page,
        region_set: region::RegionSet::new(),
    }
}
```

Add the `render_dirty` method:

```rust
/// Render only dirty regions. Returns true if any region was redrawn.
/// Caller must call display.flush_region() for each dirty region.
/// Returns a list of (y_start, y_end) pairs to flush.
pub fn render_dirty<D>(
    &mut self,
    display: &mut D,
    perf: &perf::PerfStats,
) -> [(u16, u16); region::MAX_REGIONS]
where
    D: embedded_graphics::draw_target::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>
        + chimera_hal::ChimeraDisplay,
{
    let layout = self.page.layout();
    let mut flush_list = [(0u16, 0u16); region::MAX_REGIONS];
    let mut flush_count = 0;

    // Rebuild regions if layout changed
    if self.region_set.prev_layout != Some(layout) {
        self.region_set.set_layout(layout);
    }

    let qvalues = region::quantize_values(&self.renderer.anim);

    for r in self.region_set.active_regions_mut() {
        let current_data = match r.kind {
            region::RegionKind::Header => RegionData::header(
                self.nav.chain as u8,
                self.nav.node as u8,
                self.nav.sub_page as u8,
                perf.render_us,
            ),
            region::RegionKind::Viz => RegionData::viz(self.page, qvalues),
            region::RegionKind::Params => RegionData::params(self.page, qvalues),
            region::RegionKind::Cells => RegionData::cells(self.page, qvalues),
            region::RegionKind::Nav => RegionData::nav(
                self.nav.chain as u8,
                self.nav.node as u8,
                self.nav.sub_page as u8,
            ),
        };

        if current_data != r.prev_data {
            // Clear region via direct fb access
            let fb = display.pixel_buffer();
            Renderer::clear_region_fb(fb, r.y_start, r.y_end);

            // Draw region
            self.renderer.draw_region(display, r.kind, &self.nav, self.page, perf);

            r.prev_data = current_data;
            flush_list[flush_count] = (r.y_start, r.y_end);
            flush_count += 1;
        }
    }

    // Zero out unused slots
    for i in flush_count..region::MAX_REGIONS {
        flush_list[i] = (0, 0);
    }

    flush_list
}
```

- [ ] **Step 5: Verify chimera-core compiles**

Run: `cargo check -p chimera-core 2>&1 | grep "^error"`
Expected: No errors. (May need to adjust field visibility on `ChainNav` — `chain`, `node`, `sub_page` must be `pub`.)

- [ ] **Step 6: Commit**

```bash
git add chimera-core/src/ui/mod.rs chimera-core/src/ui/region.rs chimera-core/src/ui/renderer.rs
git commit -m "feat(core): add render_dirty with per-region draw dispatch"
```

---

### Task 6: Wire up STM32 main loop

**Files:**
- Modify: `chimera-stm32/src/main.rs`

- [ ] **Step 1: Replace main loop with dirty-region loop**

Replace the entire `loop { ... }` block in `chimera-stm32/src/main.rs`:

```rust
    // Initial full render
    ui.update();
    ui.render(&mut display, &perf.stats);
    display.flush();
    led.set_low();

    loop {
        controls.snapshot();
        let has_input = controls.has_activity();

        if has_input {
            ui.handle_input(&controls);
        }

        ui.update(); // always tick animations

        let flush_list = ui.render_dirty(&mut display, &perf.stats);

        let mut any_flushed = false;
        for &(ys, ye) in &flush_list {
            if ys != ye {
                display.flush_region(ys, ye);
                any_flushed = true;
            }
        }

        if !has_input && !any_flushed {
            cortex_m::asm::wfi(); // sleep until next SysTick
        }
    }
```

- [ ] **Step 2: Build firmware**

Run: `cargo build --release 2>&1 | grep "^error"`
Expected: No errors.

- [ ] **Step 3: Generate binary**

Run: `rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin`

- [ ] **Step 4: Commit**

```bash
git add chimera-stm32/src/main.rs
git commit -m "feat(stm32): wire up dirty region main loop with wfi sleep"
```

---

### Task 7: Add dirty detection integration tests

**Files:**
- Modify: `chimera-core/tests/region_tests.rs`

- [ ] **Step 1: Add dirty detection tests**

Append to `chimera-core/tests/region_tests.rs`:

```rust
use chimera_core::ui::region::quantize_values;
use chimera_core::ui::animation::AnimatedValue;

#[test]
fn encoder_only_dirties_params_not_header() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageId::Filter;
    let values_a = [500u16; 6];
    let values_b = [501, 500, 500, 500, 500, 500]; // one encoder changed

    // Set initial data (simulating first render)
    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0);
    rs.regions[1].prev_data = RegionData::viz(page, values_a);
    rs.regions[2].prev_data = RegionData::params(page, values_a);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0);

    // Check which regions are dirty with new values
    let current = [
        RegionData::header(0, 0, 0, 0),  // unchanged
        RegionData::viz(page, values_b),   // changed (viz shows animated values)
        RegionData::params(page, values_b), // changed
        RegionData::nav(0, 0, 0),          // unchanged
    ];

    let dirty: Vec<bool> = rs.active_regions().iter().zip(current.iter())
        .map(|(r, c)| r.prev_data != *c)
        .collect();

    assert_eq!(dirty, vec![false, true, true, false]);
}

#[test]
fn nav_change_dirties_header_and_nav() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageId::Filter;
    let values = [500u16; 6];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0);
    rs.regions[1].prev_data = RegionData::viz(page, values);
    rs.regions[2].prev_data = RegionData::params(page, values);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0);

    let current = [
        RegionData::header(0, 1, 0, 0),   // node changed
        RegionData::viz(page, values),      // unchanged
        RegionData::params(page, values),   // unchanged
        RegionData::nav(0, 1, 0),           // node changed
    ];

    let dirty: Vec<bool> = rs.active_regions().iter().zip(current.iter())
        .map(|(r, c)| r.prev_data != *c)
        .collect();

    assert_eq!(dirty, vec![true, false, false, true]);
}

#[test]
fn no_change_means_no_dirty() {
    let mut rs = RegionSet::new();
    rs.set_layout(PageLayout::BigViz);

    let page = PageId::Filter;
    let values = [500u16; 6];

    rs.regions[0].prev_data = RegionData::header(0, 0, 0, 0);
    rs.regions[1].prev_data = RegionData::viz(page, values);
    rs.regions[2].prev_data = RegionData::params(page, values);
    rs.regions[3].prev_data = RegionData::nav(0, 0, 0);

    let current = [
        RegionData::header(0, 0, 0, 0),
        RegionData::viz(page, values),
        RegionData::params(page, values),
        RegionData::nav(0, 0, 0),
    ];

    let any_dirty = rs.active_regions().iter().zip(current.iter())
        .any(|(r, c)| r.prev_data != *c);

    assert!(!any_dirty);
}

#[test]
fn animation_settling_produces_dirty_then_clean() {
    let mut anim = [AnimatedValue::new(0.5); 6];
    anim[0].set_target(0.8);

    // Frame 1: animating — values differ
    anim[0].update();
    let v1 = quantize_values(&anim);

    // Frame 2: still animating
    anim[0].update();
    let v2 = quantize_values(&anim);

    assert_ne!(v1, v2, "animation should produce different quantized values");

    // Run until settled
    for _ in 0..100 {
        anim[0].update();
    }
    let settled_a = quantize_values(&anim);

    anim[0].update();
    let settled_b = quantize_values(&anim);

    assert_eq!(settled_a, settled_b, "settled animation should produce stable values");
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -p chimera-core --test region_tests 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add chimera-core/tests/region_tests.rs
git commit -m "test(core): add dirty detection and animation settling tests"
```

---

### Task 8: Flash and verify on hardware

**Files:** None (testing only)

- [ ] **Step 1: Build release binary**

Run: `cargo build --release && rust-objcopy -O binary target/thumbv7em-none-eabihf/release/chimera-stm32 chimera.bin`

- [ ] **Step 2: Flash via DFU**

Run: `dfu-util -a 0 -s 0x08020000:leave -D chimera.bin`

- [ ] **Step 3: Verify behavior**

Test checklist:
- [ ] Screen renders correctly on boot (full initial draw)
- [ ] Encoder turn updates parameter values quickly (~50-80ms vs old ~1s)
- [ ] Parameter animation is smooth (no snapping)
- [ ] Page navigation (button press) redraws full screen correctly
- [ ] Idle state: LED/CPU calm (wfi sleeping)
- [ ] No visual artifacts at region boundaries
- [ ] No ghost pixels from previous page after navigation

- [ ] **Step 4: Final commit with working binary**

```bash
git add chimera.bin
git commit -m "feat: dirty region tracking — 15x faster encoder response"
```
