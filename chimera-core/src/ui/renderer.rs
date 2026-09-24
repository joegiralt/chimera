use embedded_graphics::Drawable;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_6X10;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable};
use embedded_graphics::text::Text;

use crate::addr::Op;
use crate::ui::animation::AnimatedValue;
use crate::ui::components;
use crate::ui::block_def::{slot_addr, BlockDef, SlotBinding, VizType};
use crate::ui::chain::ChainNav;
use crate::ui::mod_grid::MatrixState;
use crate::ui::dungeon_map;
use crate::ui::fmt::{self, FmtBuf};
use crate::ui::page::PageLayout;
use crate::ui::perf::PerfStats;
use crate::ui::region::{self, RegionKind};
use crate::ui::viz;
use crate::ui::theme;

use core::fmt::Write;

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
    /// The live output is above silence (header dot).
    pub sounding: bool,
}

/// Full-screen renderer. Composites header, visualization, parameters, and dungeon map.
pub struct Renderer {
    /// Animated display values for the 6 encoders (normalized 0..1).
    pub anim: [AnimatedValue; 6],
    /// Animated scroll offset for dungeon map sub-page branches (in pixels).
    pub branch_scroll: AnimatedValue,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            anim: [AnimatedValue::new(0.5); 6],
            branch_scroll: AnimatedValue::new(0.0).with_speed(0.25),
        }
    }

    /// Jump the animated values (page change: nothing to lerp from).
    pub fn snap_to_current(&mut self, values: [f32; 6]) {
        for (a, &v) in self.anim.iter_mut().zip(values.iter()) {
            a.snap(v);
        }
    }

    /// Mod-bar amount for slot `i` of `def`, if that param is a destination.
    fn cell_mod_info(def: &BlockDef, i: usize, sel_op: Op, matrix_state: &MatrixState) -> Option<f32> {
        slot_addr(def, i, sel_op).and_then(|a| matrix_state.mod_info_for(a))
    }

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

    // ── BlockDef-based full render ─────────────────────────────────────

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
            PageLayout::BigViz => (region::quantize_values(&self.anim), f.focus as u32),
            PageLayout::Matrix => ([0; 6], 0),
        }
    }

    /// Draw a single region. The caller has already cleared it.
    pub fn draw_region_with_def<D>(&self, display: &mut D, kind: RegionKind, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (nav, def, matrix_state) = (f.nav, f.def, f.matrix);
        match kind {
            RegionKind::Header => self.draw_header(display, f),
            RegionKind::Focus => self.draw_focus(display, f),
            RegionKind::Viz => match def.layout {
                PageLayout::CellGrid => viz::live_output(display, f.scope),
                PageLayout::BigViz => self.draw_big_viz(display, f),
                PageLayout::Matrix => {}
            },
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

    /// Header band: context, page name, audio load, sounding dot.
    fn draw_header<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (context, name) = components::header_text(f.nav, f.def);
        components::header(display, context.as_str(), name.as_str(), f.sounding, f.perf.audio_load_pct);
    }

    // ── Sound Browser ────────────────────────────────────────────────

    /// Number of visible rows in the sound browser list.
    pub const BROWSER_VISIBLE_ROWS: usize = 10;
    /// Total entries: 32 pool slots + 3 init options (Pizza, Modal, FM).
    pub const BROWSER_TOTAL_ENTRIES: usize = crate::preset::POOL_SIZE + 3;

    /// Draw the full-screen sound browser overlay.
    pub fn draw_sound_browser<D>(
        display: &mut D,
        pool: &crate::preset::SoundPool,
        part: usize,
        cursor: usize,
        scroll: usize,
        part_chain_type: crate::preset::ChainType,
    )
    where
        D: DrawTarget<Color = Rgb565>,
    {
        // Clear screen
        let _ = Rectangle::new(Point::zero(), Size::new(240, 320))
            .draw_styled(&PrimitiveStyle::with_fill(theme::BG), display);

        // Title bar: "LOAD SOUND: P[n]" (the Part it loads into)
        let mut title_buf = FmtBuf::new();
        let _ = write!(title_buf, "LOAD SOUND: P{}", part + 1);
        let title_style = MonoTextStyle::new(&FONT_6X10, theme::ACCENT);
        let _ = Text::new(title_buf.as_str(), Point::new(8, theme::HEADER_Y + 10), title_style)
            .draw(display);

        // Separator below title
        let _ = Line::new(
            Point::new(0, 22),
            Point::new(theme::SCREEN_W - 1, 22),
        )
        .draw_styled(&PrimitiveStyle::with_stroke(theme::SEPARATOR, 1), display);

        // List rows
        let row_height: i32 = 24;
        let list_top: i32 = 28;
        let text_style = MonoTextStyle::new(&FONT_6X10, theme::TEXT);
        let dim_style = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);

        let visible = Self::BROWSER_VISIBLE_ROWS.min(Self::BROWSER_TOTAL_ENTRIES);
        for i in 0..visible {
            let entry_idx = scroll + i;
            if entry_idx >= Self::BROWSER_TOTAL_ENTRIES {
                break;
            }

            let y = list_top + i as i32 * row_height;
            let is_selected = entry_idx == cursor;

            // Highlight bar for selected row
            if is_selected {
                let _ = Rectangle::new(
                    Point::new(0, y),
                    Size::new(240, row_height as u32),
                )
                .draw_styled(&PrimitiveStyle::with_fill(theme::ACCENT_DIM), display);
            }

            let text_color = if is_selected { theme::TEXT } else { theme::TEXT_MID };
            let style = MonoTextStyle::new(&FONT_6X10, text_color);

            if entry_idx < crate::preset::POOL_SIZE {
                // Pool slot row: "[nn] Name  Type"
                let mut row_buf = FmtBuf::new();
                if let Some(sound) = pool.get(entry_idx) {
                    let _ = write!(row_buf, "{:2} {} {}", entry_idx + 1, sound.name_str(), sound.chain_type.label());
                } else {
                    let _ = write!(row_buf, "{:2} (empty)", entry_idx + 1);
                }
                let _ = Text::new(row_buf.as_str(), Point::new(8, y + 16), style).draw(display);
            } else {
                // Init entries: POOL_SIZE=Pizza, POOL_SIZE+1=Modal, POOL_SIZE+2=FM
                let init_types = [
                    crate::preset::ChainType::PizzaPoly,
                    crate::preset::ChainType::Modal,
                    crate::preset::ChainType::Fm,
                ];
                let init_idx = entry_idx - crate::preset::POOL_SIZE;
                let mut init_buf = FmtBuf::new();
                if init_idx < init_types.len() {
                    let _ = write!(init_buf, "** (init) {}", init_types[init_idx].label());
                }
                let _ = Text::new(init_buf.as_str(), Point::new(8, y + 16), style).draw(display);
            }
        }

        // Scroll indicator — show position in list
        if Self::BROWSER_TOTAL_ENTRIES > visible {
            let bar_top = list_top;
            let bar_height = visible as i32 * row_height;
            let thumb_height = (bar_height * visible as i32 / Self::BROWSER_TOTAL_ENTRIES as i32).max(8);
            let max_scroll = Self::BROWSER_TOTAL_ENTRIES - visible;
            let thumb_y = bar_top + if max_scroll > 0 {
                (bar_height - thumb_height) * scroll as i32 / max_scroll as i32
            } else {
                0
            };

            let _ = Rectangle::new(
                Point::new(234, thumb_y),
                Size::new(4, thumb_height as u32),
            )
            .draw_styled(&PrimitiveStyle::with_fill(theme::TEXT_DIM), display);
        }

        // Footer hint
        let hint_style = MonoTextStyle::new(&FONT_6X10, theme::TEXT_DIM);
        let _ = Text::new("Turn:scroll  Edit:load  B:cancel", Point::new(8, 306), hint_style)
            .draw(display);
    }

    // ── Dirty region helpers ─────────────────────────────────────────

    /// Clear a screen region by direct framebuffer fill. Much faster than draw_iter.
    pub fn clear_region_fb(fb: &mut [u16], y_start: u16, y_end: u16) {
        use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
        let bg = RawU16::from(theme::BG).into_inner();
        let start = y_start as usize * 240;
        let end = y_end as usize * 240;
        fb[start..end].fill(bg);
    }

}
