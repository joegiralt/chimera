use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, StyledDrawable};

use crate::addr::Op;
use crate::ui::animation::AnimatedValue;
use crate::ui::components;
use crate::ui::block_def::{slot_addr, BlockDef, SlotBinding, VizType};
use crate::ui::chain::ChainNav;
use crate::ui::mod_grid::MatrixState;
use crate::ui::draw;
use crate::ui::dungeon_map;
use crate::ui::fmt::{self, FmtBuf};
use crate::ui::page::PageLayout;
use crate::ui::perf::PerfStats;
use crate::ui::region::{self, RegionKind};
use crate::ui::viz;
use crate::ui::theme;

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
    /// Every Part (Mixer overview, FM algorithm) and the one being edited.
    pub parts: &'a [crate::preset::Part; crate::hw::MAX_PARTS],
    pub active_part: usize,
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

    /// The viz band of a CellGrid page.
    fn draw_band_viz<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match f.def.viz {
            VizType::AlgorithmDiagram => {
                use crate::ui::block_registry as reg;
                let alg = f.parts[f.active_part].sound.params.fm.algorithm;
                // Only the FM operator page edits a single operator; the FM
                // algorithm page lights none (accent is the active element only).
                let selected = (f.def.id == reg::FM_OP.id).then(|| f.sel_op.index());
                viz::fm_algorithm(display, alg, selected);
            }
            VizType::MixerLevels => viz::parts_overview(display, &self.strips(f), f.active_part),
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
            PageLayout::CellGrid => match f.def.viz {
                VizType::AlgorithmDiagram => {
                    use crate::ui::block_registry as reg;
                    let alg = f.parts[f.active_part].sound.params.fm.algorithm as u32;
                    let sel = if f.def.id == reg::FM_OP.id { f.sel_op.index() as u32 } else { 0 };
                    ([0; 6], alg << 2 | sel)
                }
                VizType::MixerLevels => (region::quantize_values(&self.anim), strips_key(&self.strips(f), f.active_part)),
                VizType::EffectsFlow => (region::quantize_values(&self.anim), f.focus as u32),
                _ => ([0; 6], viz::live_key(f.scope)),
            },
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
                PageLayout::CellGrid => self.draw_band_viz(display, f),
                PageLayout::BigViz => self.draw_big_viz(display, f),
                PageLayout::Matrix => {}
            },
            RegionKind::Cells => self.draw_cells(display, f, theme::CELL_LABEL_Y),
            RegionKind::Grid => {
                crate::ui::mod_grid::draw_grid(display, matrix_state, amount_of(self.anim[MATRIX_AMOUNT_SLOT].current()))
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
        if f.def.layout == PageLayout::Matrix {
            return self.draw_route(display, f.matrix);
        }
        let slot = &f.def.params[f.focus];
        if slot.binding == SlotBinding::Empty {
            return;
        }
        let v = self.anim[f.focus].current();
        let mut buf = FmtBuf::new();
        fmt::fmt_val(&mut buf, v, slot.format());
        components::focus_band(display, slot.label(), buf.as_str(), v, slot.format().is_bipolar());
    }

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

/// Fingerprint of the Mixer overview (quantized levels and pans, selection).
fn strips_key(strips: &[viz::Strip], selected: usize) -> u32 {
    strips.iter().fold(selected as u32, |h, s| {
        let q = (region::quantize(s.level) as u32) << 16 | region::quantize(s.pan + 1.0) as u32;
        (h ^ q).wrapping_mul(0x0100_0193)
    })
}

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
