# Dirty Region Tracking for Display Rendering

## Problem

The current renderer redraws the entire 240x320 RGB565 framebuffer every frame via embedded-graphics primitives, then flushes all 76,800 pixels over bit-bang SPI. This takes ~1 second per frame, making the UI unresponsive.

Most user interactions only change a small portion of the screen. An encoder turn changes one parameter value. A button press changes navigation state. The renderer should only redraw and flush the regions that actually changed.

## Design

### Core concept

Each `PageLayout` variant defines an ordered list of **regions** that tile the full screen (0..320) with no gaps. Each region has a Y range, a data snapshot, and a draw method. Every frame, the system computes current data for each region, compares it to the stored snapshot, and only clears + redraws + flushes regions whose data changed.

### RegionData

Each region type has a corresponding data variant that captures everything needed to determine if a redraw is required. Float values are quantized to `u16` to avoid float comparison issues and keep comparisons cheap.

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionData {
    Header {
        chain_idx: u8,
        node_idx: u8,
        sub_page: u8,
        render_us: u32,    // perf overlay lives in header region
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
```

Quantization helper:

```rust
fn quantize(f: f32) -> u16 {
    (f.clamp(0.0, 65.0) * 1000.0) as u16
}
```

### RegionKind enum (draw dispatch)

no_std Rust can't use `dyn Fn`. Draw dispatch is an enum match that gates which existing `draw_*` methods get called. No new abstraction needed — the renderer already has separate methods.

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionKind {
    Header,
    Viz,
    Params,
    Cells,
    Nav,
}
```

### Region struct and storage

```rust
const MAX_REGIONS: usize = 5;

struct Region {
    kind: RegionKind,
    y_start: u16,
    y_end: u16,              // exclusive
    prev_data: RegionData,
}

struct RegionSet {
    regions: [Region; MAX_REGIONS],
    count: u8,
    prev_layout: PageLayout,
}
```

`RegionSet` lives in the `Renderer`. When `PageLayout` changes, the region list is rebuilt and all regions are marked dirty (prev_data zeroed).

### Layout-to-regions mapping

Each `PageLayout` variant declares its region list. Regions tile the full screen 0..320 with no gaps:

```
BigViz:   [Header(0..28),  Viz(28..144),  Params(144..214), Nav(214..320)]
CellGrid: [Header(0..28),  Cells(28..214),                  Nav(214..320)]
```

The separator line (y=213) is included in the Params/Cells region and drawn as part of its render. Header extends to y=0 to cover the top gap. Nav extends to y=320 to cover the bottom.

New page layouts define their own region lists following the same tiling rule.

### Region clearing via direct framebuffer access

Region clears do NOT go through embedded-graphics `draw_iter` (which is pixel-by-pixel and very slow). Instead, clear directly via `pixel_buffer()`:

```rust
fn clear_region(fb: &mut [u16], y_start: u16, y_end: u16, bg_color: u16) {
    let start = y_start as usize * 240;
    let end = y_end as usize * 240;
    for px in &mut fb[start..end] {
        *px = bg_color;
    }
}
```

This is orders of magnitude faster than a filled Rectangle through DrawTarget.

### Animation integration

The `AnimatedValue` system lerps parameter values over multiple frames. This is essential for smooth knob feel. Dirty tracking must not break animation.

How it works: when an encoder fires, the `AnimatedValue` target changes. On subsequent frames, the lerp produces new intermediate values. Each frame, the quantized snapshot of the Params/Cells region differs from `prev_data`, so the region stays dirty and keeps redrawing until the animation settles.

The main loop must keep running frames while any animation is unsettled, not just when `has_activity()` is true:

```rust
loop {
    controls.snapshot();
    let has_input = controls.has_activity();

    if has_input {
        ui.handle_input(&controls);
    }

    ui.update();  // always tick animations

    let any_dirty = ui.render_dirty(&mut display);
    // render_dirty returns true if any region was redrawn

    if !has_input && !any_dirty {
        // truly idle — nothing to do
        cortex_m::asm::wfi();  // sleep until next interrupt
    }
}
```

The `AnimatedValue::settled()` check (already exists — snaps when within `SNAP_THRESHOLD`) ensures the quantized values eventually stop changing, and the region stops redrawing.

### Frame loop (pseudo-code)

```
1. controls.snapshot()
2. if has_input: ui.handle_input(&controls)
3. ui.update()                          // tick animations
4. if layout changed: rebuild region list, mark all dirty
5. for each region:
     compute current RegionData (using quantized anim values)
     if current_data != prev_data:
       clear region via direct fb memset
       call region's draw method
       prev_data = current_data
       mark region for flush
6. for each marked region:
     display.flush_region(y_start, y_end)
7. if no input and no dirty regions: wfi (sleep until SysTick)
```

### Partial flush

The ILI9341 supports setting an address window to any rectangle. `flush_region` sets the column range to 0-239 and the row range to the dirty region's Y span, then pushes only those rows via the same byte-swap + SPI write loop used by the current `flush()`.

```rust
// Added to ChimeraDisplay trait
fn flush_region(&mut self, y_start: u16, y_end: u16);
```

Implementation in `Stm32Display`:

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
    let start = y_start as usize * 240;
    let end = y_end as usize * 240;
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

Each dirty region gets its own `set_window` + SPI transfer. If Header and Params are both dirty, that's 2 small transfers instead of 1 large one that includes clean rows between them.

### Page navigation (layout change)

When `PageId` changes and the `PageLayout` differs from the previous frame, the `RegionSet` is rebuilt with the new layout's regions. All `prev_data` is set to a sentinel value that never matches, forcing a full redraw.

When `PageId` changes but the `PageLayout` stays the same (e.g., switching between EngineFmA and EngineFmB), every region's data will naturally differ due to the `page` field in `RegionData`, so all regions redraw without special handling.

### What lives where

| Crate | Responsibility |
|---|---|
| `chimera-hal` | `flush_region(y_start, y_end)` added to `ChimeraDisplay` trait |
| `chimera-core` | `RegionData`, `RegionKind`, `Region`, `RegionSet`, region lists per `PageLayout`, dirty comparison, per-region draw dispatch, direct fb clear |
| `chimera-stm32` | `flush_region()` implementation (SPI address window + partial transfer), main loop wiring |

### Testing strategy

All dirty tracking logic lives in `chimera-core` and is testable on the host with `cargo test`.

**Unit tests:**

1. `RegionData` equality — same data compares equal, different data compares not-equal
2. Quantization — `quantize(0.0) == 0`, `quantize(1.0) == 1000`, `quantize(0.5) == 500`, values clamped
3. Quantization stability — small float jitter below threshold doesn't trigger dirty
4. Layout region definitions — BigViz has 4 regions, CellGrid has 3
5. Region tiling — Y ranges don't overlap, no gaps, cover full 0..320
6. Page change detection — changing PageId makes Viz/Params/Cells regions dirty
7. Navigation change detection — changing chain/node/sub_page makes Header and Nav dirty
8. Encoder-only change — only Params/Cells region is dirty, Header and Nav are clean
9. Full layout change — switching BigViz to CellGrid rebuilds region list and marks all dirty
10. Animation settling — region stays dirty while AnimatedValue is lerping, goes clean once settled

**Integration tests (with mock display):**

11. Mock DrawTarget that records which pixel ranges were written — verify only dirty region pixels are touched
12. Consecutive frames with no input change and settled animations — zero regions dirty, zero pixels flushed
13. Encoder turn followed by animation settle — region dirty for N frames, then clean

### What this does NOT do (intentionally)

- **No sub-region granularity** — entire Params region redraws even if only one encoder changed. The Params region is small enough (~16K pixels) that this is fast.
- **No double buffering** — same single framebuffer, just partial writes and partial flushes.
- **No DMA SPI** — the bit-bang SPI stays as-is. Partial flush reduces the data volume instead of speeding up the transfer.

### Performance estimate

| Scenario | Current | With dirty regions |
|---|---|---|
| Encoder turn (params only) | ~1000ms | ~50-80ms (params: 240x70 = 16,800 px) |
| Animation settling frame | ~1000ms | ~50-80ms (same region, new values) |
| Page navigation | ~1000ms | ~1000ms (full redraw, acceptable) |
| Idle (no input, settled) | ~1000ms | 0ms (wfi sleep, zero CPU) |

The encoder-turn case improves by ~15x. The idle case drops from 100% CPU to sleeping.
