use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, StyledDrawable};

use crate::addr::Op;
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::modulator::{EnvType, HoldPos};
use crate::perf::load::AudioStats;
use crate::ui::PrimeStatus;
use crate::ui::animation::AnimatedValue;
use crate::ui::audio_page;
use crate::ui::block_def::{BlockDef, FxFlow, SlotBinding, VizType, slot_addr};
use crate::ui::chain::ChainNav;
use crate::ui::components;
use crate::ui::draw;
use crate::ui::dungeon_map;
use crate::ui::fmt::{self, FmtBuf};
use crate::ui::mod_grid::MatrixState;
use crate::ui::page::PageLayout;
use crate::ui::perf::PerfStats;
use crate::ui::region::{self, RegionKind};
use crate::ui::theme;
use crate::ui::view::{self, EnvKind, SlotCtx, View};
use crate::ui::viz;

/// Everything one frame draws from, besides the renderer's own animation.
pub struct Frame<'a> {
    pub nav: &'a ChainNav,
    pub def: &'static BlockDef,
    pub perf: &'a PerfStats,
    pub matrix: &'a MatrixState,
    pub sel_op: Op,
    /// What the page's slots resolve against.
    pub ctx: SlotCtx,
    /// Slot the focus band shows: the last one touched on this page.
    pub focus: usize,
    /// Live output (the oscilloscope buffer).
    pub scope: &'a [f32; crate::scope::SCOPE_LEN],
    /// The live output is above silence (header dot).
    pub sounding: bool,
    /// Every Part (Mixer overview) and the one being edited.
    pub parts: &'a [crate::preset::Part; crate::hw::MAX_PARTS],
    pub active_part: usize,
    /// The last MIX+PLUS outcome, shown in the focus band in place of the
    /// value readout (issue #21).
    pub prime_status: Option<PrimeStatus>,
    /// The AUDIO sub-page's measured stats, `None` where it is not shown.
    pub audio: Option<&'a AudioStats>,
    /// The master compressor's gain reduction, dB (MST's GR meter).
    pub master_gr_db: f32,
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

    /// BigViz: the page's visualization with the touched value riding on it.
    fn draw_big_viz<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let a = |i: usize| self.anim[i].current();
        match f.def.viz {
            VizType::FilterResponse => {
                // By address, not slot (spec § UI "Vizzes read by address").
                let at = |id| {
                    let addr = crate::addr::ParamAddr::new(crate::addr::BlockRef::Filter, id);
                    (0..f.def.params.len())
                        .find(|&i| slot_addr(f.def, i, &f.ctx) == Some(addr))
                        .map_or(0.0, a)
                };
                use crate::params::FilterParams;
                let mode = f.parts[f.active_part].sound.params.filter.mode();
                let v = view::view(f.def, f.focus, &f.ctx);
                let buf = value_text(f, f.focus, a(f.focus));
                let readout = (v != View::Empty).then(|| (v.label(), buf.as_str()));
                viz::filter(
                    display,
                    at(FilterParams::CUTOFF),
                    at(FilterParams::RESONANCE),
                    viz::Response::of(mode),
                    readout,
                );
            }
            VizType::Adsr => {
                let Some(SlotBinding::EnvPanel(s, _)) = f.def.params.first().map(|p| p.binding)
                else {
                    return;
                };
                let addr = |id| crate::addr::ParamAddr::new(crate::addr::BlockRef::Env(s), id);
                let at = |id| {
                    (0..f.def.params.len())
                        .find(|&i| slot_addr(f.def, i, &f.ctx) == Some(addr(id)))
                        .map(a)
                };
                use crate::params::EnvParams as E;
                let p = &f.parts[f.active_part].sound.params.envelopes[s.index()];
                match f.ctx.envs[s.index()] {
                    EnvKind::A(_) => {
                        // H is a stage under AHDSR only; its floor, as A, D
                        // and R have, keeps it drawn at 0.
                        let hold = if p.hold_pos == HoldPos::Ahdsr {
                            at(E::HOLD).unwrap_or(0.0).max(0.02)
                        } else {
                            0.0
                        };
                        let (atk, dec, sus, rel) = (
                            at(E::ATTACK).unwrap_or(0.0).max(0.02),
                            at(E::DECAY).unwrap_or(0.0).max(0.02),
                            at(E::SUSTAIN).unwrap_or(0.0),
                            at(E::RELEASE).unwrap_or(0.0).max(0.02),
                        );
                        let total = atk + hold + dec + 0.3 + rel;
                        let focus = view::view(f.def, f.focus, &f.ctx).addr().map(|x| x.param);
                        let lit = match focus {
                            Some(E::ATTACK) => Some(0),
                            Some(E::HOLD) => Some(1),
                            Some(E::DECAY) => Some(2),
                            Some(E::SUSTAIN) => Some(3),
                            Some(E::RELEASE) => Some(4),
                            _ => None,
                        };
                        viz::envelope(
                            display,
                            &[
                                atk / total,
                                hold / total,
                                dec / total,
                                0.3 / total,
                                rel / total,
                            ],
                            &[0.0, 1.0, 1.0, sus, sus, 0.0],
                            &["A", "H", "D", "S", "R"],
                            lit,
                        );
                    }
                    EnvKind::B(func) => viz::func(
                        display,
                        func,
                        at(E::RISE).unwrap_or(p.func.rise),
                        at(E::FALL).unwrap_or(p.func.fall),
                        at(E::SHAPE).unwrap_or(p.func.shape),
                    ),
                }
            }
            VizType::CompressorCurve => {
                use crate::dsp::comp::RATIOS;
                let ratio = RATIOS[((a(1) * 7.0 + 0.5) as usize).min(RATIOS.len() - 1)];
                viz::compressor(display, -40.0 + 40.0 * a(0), ratio, f.master_gr_db);
            }
            VizType::EnvSpeed => {
                let e = &f.parts[f.active_part].sound.params.envelopes;
                viz::env_speed(
                    display,
                    core::array::from_fn(|i| {
                        (e[i].env_type == EnvType::A, e[i].speed, e[i].hold_pos)
                    }),
                );
            }
            _ => {}
        }
        if let Some(status) = f.prime_status {
            components::viz_status(display, status);
        }
    }

    /// The viz band of a CellGrid page.
    fn draw_band_viz<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match f.def.viz {
            VizType::AudioStats => audio_page::draw_viz(display, f.audio),
            VizType::MixerLevels => viz::parts_overview(display, &self.strips(f), f.active_part),
            VizType::EffectsFlow(FxFlow::Sends) => {
                let sends = [
                    self.anim[0].current(),
                    self.anim[1].current(),
                    self.anim[2].current(),
                ];
                viz::effects_flow(display, (f.focus < 3).then_some(f.focus), Some(sends));
            }
            VizType::EffectsFlow(FxFlow::Effect(node)) => {
                viz::effects_flow(display, Some(node as usize), None)
            }
            VizType::AlgoDiagram => {
                let algo = &f.parts[f.active_part].sound.params.algo;
                let morph = crate::addr::ParamAddr::new(
                    crate::addr::BlockRef::Algo,
                    crate::dsp::algo::params::AlgoParams::MORPH,
                );
                let slot = (0..f.def.params.len())
                    .find(|&i| slot_addr(f.def, i, &f.ctx) == Some(morph))
                    .expect("ALG page binds MORPH");
                viz::algo_diagram(
                    display,
                    AlgoId::clamped(algo.alg_a).algorithm(),
                    AlgoId::clamped(algo.alg_b).algorithm(),
                    self.anim[slot].current(),
                );
            }
            _ => viz::live_output(display, f.scope),
        }
    }

    /// Level and pan of every Part; the edited one from its animated LEVEL
    /// and PAN slots so it lerps like the cells.
    fn strips(&self, f: &Frame) -> [viz::Strip; crate::hw::MAX_PARTS] {
        let slot = |id| {
            let addr = crate::addr::ParamAddr::new(crate::addr::BlockRef::Part, id);
            (0..f.def.params.len()).find(|&i| slot_addr(f.def, i, &f.ctx) == Some(addr))
        };
        let (level, pan) = (
            slot(crate::part::PartParams::LEVEL),
            slot(crate::part::PartParams::PAN),
        );
        core::array::from_fn(|i| {
            let mix = &f.parts[i].mix;
            let mut s = viz::Strip {
                level: mix.level,
                pan: mix.pan,
            };
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
        let _ = Rectangle::new(
            Point::zero(),
            Size::new(theme::SCREEN_W as u32, theme::SCREEN_H as u32),
        )
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
                VizType::AudioStats => ([0; 6], audio_page::viz_key(f.audio)),
                VizType::MixerLevels => (
                    region::quantize_values(&self.anim),
                    strips_key(&self.strips(f), f.active_part),
                ),
                VizType::EffectsFlow(_) => (region::quantize_values(&self.anim), f.focus as u32),
                VizType::AlgoDiagram => {
                    let algo = &f.parts[f.active_part].sound.params.algo;
                    let q = region::quantize_values(&self.anim);
                    (
                        [0, 0, q[2], 0, 0, 0],
                        (algo.alg_a as u32) << 8 | algo.alg_b as u32,
                    )
                }
                _ => ([0; 6], viz::live_key(f.scope)),
            },
            PageLayout::BigViz => {
                // The GR meter redraws every 0.25 dB.
                let gr = match f.def.viz {
                    VizType::CompressorCurve => {
                        (f.master_gr_db.clamp(0.0, viz::GR_RANGE_DB) * 4.0) as u32
                    }
                    _ => 0,
                };
                (
                    region::quantize_values(&self.anim),
                    f.focus as u32 | gr << 8,
                )
            }
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
            RegionKind::Grid => crate::ui::mod_grid::draw_grid(
                display,
                matrix_state,
                amount_of(self.anim[MATRIX_AMOUNT_SLOT].current()),
            ),
            RegionKind::Nav => {
                dungeon_map::draw(
                    display,
                    nav,
                    (self.branch_scroll.current() * theme::BRANCH_LINE_HEIGHT as f32) as i32,
                );
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
        if f.def.viz == VizType::AudioStats {
            return audio_page::draw_focus(display, f.def, f.audio);
        }
        let view = view::view(f.def, f.focus, &f.ctx);
        if view == View::Empty {
            return;
        }
        let v = self.anim[f.focus].current();
        let buf = value_text(f, f.focus, v);
        components::focus_band(
            display,
            view.label(),
            buf.as_str(),
            v,
            view.fmt().is_bipolar(),
            look(f, f.focus),
            f.prime_status,
        );
    }

    /// Mod matrix focus band: the selected route (`SRC → TAG DEST`); the amount lerps through
    /// slot e's animated value (the amount encoder).
    fn draw_route<D>(&self, display: &mut D, m: &MatrixState)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let dest = m
            .dests
            .get(m.sel_col)
            .copied()
            .flatten()
            .filter(|_| m.sel_col < m.num_dests);
        let (Some(dest), Some(src)) = (dest, m.sources.get(m.sel_row).copied().flatten()) else {
            let label = "NO DESTINATIONS";
            draw::text_tracked(
                display,
                &theme::FONT_VALUE,
                label,
                theme::MARGIN_X,
                theme::FOCUS_LABEL_Y,
                theme::MID,
                theme::LABEL_TRACKING,
            );
            return;
        };
        let v = self.anim[MATRIX_AMOUNT_SLOT].current();
        let mut buf = FmtBuf::new();
        crate::ui::mod_grid::fmt_amount(&mut buf, amount_of(v));
        let mut name = FmtBuf::new();
        crate::ui::mod_grid::fmt_route_dest(&mut name, &dest);
        components::focus_route(display, src.name, name.as_str(), buf.as_str(), v);
    }

    /// The six cells, first row's labels at `top`.
    fn draw_cells<D>(&self, display: &mut D, f: &Frame, top: i32)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        if f.def.viz == VizType::AudioStats {
            return audio_page::draw_cells(display, f.def, f.audio, top);
        }
        for (i, anim) in self.anim.iter().enumerate() {
            let v = view::view(f.def, i, &f.ctx);
            if v == View::Empty {
                components::cell(display, i, top, None);
                continue;
            }
            let value = anim.current();
            let mut buf = FmtBuf::new();
            match v {
                View::Text { text, .. } => {
                    let _ = core::fmt::Write::write_str(&mut buf, text);
                }
                _ => fmt::fmt_val(&mut buf, value, v.fmt()),
            }
            let c = components::Cell {
                label: v.label(),
                text: buf.as_str(),
                value,
                fmt: v.fmt(),
                active: i == f.focus,
                mod_amount: mod_info(f, i),
                look: look(f, i),
            };
            components::cell(display, i, top, Some(&c));
        }
    }

    /// Header band: context, page name, audio load, sounding dot.
    fn draw_header<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (context, mut name) = components::header_text(f.nav, f.def);
        if let Some(ty) = ["", " / A", " / B"].get(title_type(f) as usize) {
            let _ = core::fmt::Write::write_str(&mut name, ty);
        }
        components::header(
            display,
            context.as_str(),
            name.as_str(),
            f.sounding,
            f.perf.audio_load_pct,
        );
    }

    // ── Dirty region helpers ─────────────────────────────────────────

    /// Clear a screen region by direct framebuffer fill. Much faster than draw_iter.
    pub fn clear_region_fb(fb: &mut [u16], y_start: u16, y_end: u16) {
        use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
        let bg = RawU16::from(theme::BG).into_inner();
        let start = y_start as usize * theme::SCREEN_W as usize;
        let end = y_end as usize * theme::SCREEN_W as usize;
        fb[start..end].fill(bg);
    }
}

/// The ENV page title's TYPE suffix: 0 none, 1 A, 2 B.
pub fn title_type(f: &Frame) -> u8 {
    match f.def.params.first().map(|p| p.binding) {
        Some(SlotBinding::EnvPanel(s, _)) => match f.ctx.envs[s.index()] {
            EnvKind::A(_) => 1,
            EnvKind::B(_) => 2,
        },
        _ => 0,
    }
}

/// How cell `i` of the page reads now: the one place a cell's look is
/// decided, for drawing and for the dirty-region key. Both looks read the
/// Sound's `mod_state`, which every matrix edit syncs, not the UI's mirror.
pub fn look(f: &Frame, i: usize) -> components::Look {
    let sound = &f.parts[f.active_part].sound;
    match view::view(f.def, i, &f.ctx) {
        View::Route { source, .. }
            if sound.mod_state.routes_into(crate::modulation::CUTOFF) & (1 << source.index())
                == 0 =>
        {
            components::Look::Absent
        }
        v if view::is_dimmed(&v, sound) => components::Look::Dimmed,
        _ => components::Look::Live,
    }
}

/// Mod-bar amount for cell `i`, if its param is a destination.
pub(crate) fn mod_info(f: &Frame, i: usize) -> Option<f32> {
    slot_addr(f.def, i, &f.ctx).and_then(|a| f.matrix.mod_info_for(a))
}

/// Slot `i`'s value as a cell, the focus band and a viz readout show it:
/// `--` for an absent route, a fixed readout's text.
fn value_text(f: &Frame, i: usize, v: f32) -> FmtBuf {
    let mut buf = FmtBuf::new();
    match view::view(f.def, i, &f.ctx) {
        View::Text { text, .. } => {
            let _ = core::fmt::Write::write_str(&mut buf, text);
        }
        _ if look(f, i) == components::Look::Absent => {
            let _ = core::fmt::Write::write_str(&mut buf, "--");
        }
        view => fmt::fmt_val(&mut buf, v, view.fmt()),
    }
    buf
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
