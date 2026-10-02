use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, StyledDrawable};

use crate::addr::{BlockRead, Op, ParamAddr};
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::modulator::{EnvType, HoldPos};
use crate::part::DacPair;
use crate::perf::load::AudioStats;
use crate::project::PartId;
use crate::ui::PrimeStatus;
use crate::ui::animation::AnimatedValue;
use crate::ui::audio_page;
use crate::ui::block_def::{BlockDef, ChainDef2, FxFlow, SlotBinding, VizType, slot_addr};
use crate::ui::components::{self, Head};
use crate::ui::dungeon_map;
use crate::ui::fmt::{self, FmtBuf};
use crate::ui::glyph::{
    BRAID_PARAMS, Braid, BraidPart, CUBE_PARAMS, CompositeId, Cube, CubePart, FocusGlyph, Gauge,
    RINGS_PARAMS, Rings, RingsPart,
};
use crate::ui::mod_grid::MatrixState;
use crate::ui::nav::PageAt;
use crate::ui::page::PageLayout;
use crate::ui::perf::PerfStats;
use crate::ui::region::{self, Layout, RegionKind};
use crate::ui::settings::Ask;
use crate::ui::settings::prompt::{Beneath, draw_prompt};
use crate::ui::settings::view::Bands;
use crate::ui::theme;
use crate::ui::view::{self, EnvKind, SlotCtx, View};
use crate::ui::viz;

/// Everything one frame draws from, besides the renderer's own animation.
pub struct Frame<'a> {
    /// Whose page this is, for the header and its OUT warning.
    pub head: Head,
    /// The chain the map shows, and where on it; `None` in SETTINGS.
    pub map: Option<(&'static ChainDef2, PageAt)>,
    pub layout: Layout,
    /// SETTINGS' breadcrumb, list and footer.
    pub settings: Option<Bands<'a>>,
    /// A prompt over the screen.
    pub(crate) prompt: Option<&'a Ask>,
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
    pub active_part: PartId,
    /// The last MIX+PLUS outcome, shown in the focus band in place of the
    /// value readout (issue #21).
    pub prime_status: Option<PrimeStatus>,
    /// The AUDIO sub-page's measured stats, `None` where it is not shown.
    pub audio: Option<&'a AudioStats>,
    /// The master compressor's gain reduction, dB (MST's GR meter).
    pub master_gr_db: f32,
    /// Animation phase: what an animated glyph draws from.
    pub clock: crate::ui::animation::UiClock,
    /// The FX's stored params (a composite reads the chorus's).
    pub fx: &'a crate::dsp::fx_bus::FxParams,
}

/// Full-screen renderer. Composites header, visualization, parameters, and dungeon map.
pub struct Renderer {
    /// Animated display values for the 6 encoders (normalized 0..1).
    pub anim: [AnimatedValue; 6],
    /// The same slots' set values, eased, with no modulation: what the
    /// glyphs that ignore modulation draw (CROSSFADER, and the composites
    /// through `eased_set`).
    pub set: [AnimatedValue; 6],
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
            set: [AnimatedValue::new(0.5); 6],
            branch_scroll: AnimatedValue::new(0.0).with_speed(0.25),
        }
    }

    /// Jump the animated values (page change: nothing to lerp from).
    pub fn snap_to_current(&mut self, values: [f32; 6]) {
        for (a, &v) in self.anim.iter_mut().zip(values.iter()) {
            a.snap(v);
        }
        for (a, &v) in self.set.iter_mut().zip(values.iter()) {
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
                let mode = f.parts[f.active_part.index()].sound.params.filter.mode();
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
                let p = &f.parts[f.active_part.index()].sound.params.envelopes[s.index()];
                match f.ctx.envs[s.index()] {
                    EnvKind::A(_) => {
                        let (atk, dec, sus, rel) = (
                            at(E::ATTACK).unwrap_or(0.0).max(0.02),
                            at(E::DECAY).unwrap_or(0.0).max(0.02),
                            at(E::SUSTAIN).unwrap_or(0.0),
                            at(E::RELEASE).unwrap_or(0.0).max(0.02),
                        );
                        // H is a stage under AHDSR only, wide enough for its
                        // label even at 0.
                        let hold = if p.hold_pos == HoldPos::Ahdsr {
                            let floor = viz::label_floor("H", atk + dec + 0.3 + rel);
                            at(E::HOLD).unwrap_or(0.0).max(floor)
                        } else {
                            0.0
                        };
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
                let e = &f.parts[f.active_part.index()].sound.params.envelopes;
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
            VizType::MixerLevels => {
                viz::parts_overview(display, &self.strips(f), f.active_part.index())
            }
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
                let algo = &f.parts[f.active_part.index()].sound.params.algo;
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
            if i == f.active_part.index() {
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
        for &(kind, _, _) in f.layout.regions() {
            self.draw_region_with_def(display, kind, f);
        }
        self.draw_region_with_def(display, RegionKind::Prompt, f);
    }

    /// What the page's viz is drawn from, for dirty tracking: the slot
    /// values it reads (quantized) and a fingerprint of outside data.
    pub fn viz_inputs(&self, f: &Frame) -> ([u16; 6], u32) {
        match f.def.layout {
            PageLayout::CellGrid => match f.def.viz {
                VizType::AudioStats => ([0; 6], audio_page::viz_key(f.audio)),
                VizType::MixerLevels => (
                    region::quantize_values(&self.anim),
                    strips_key(&self.strips(f), f.active_part.index()),
                ),
                VizType::EffectsFlow(_) => (region::quantize_values(&self.anim), f.focus as u32),
                VizType::AlgoDiagram => {
                    let algo = &f.parts[f.active_part.index()].sound.params.algo;
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
        let (def, matrix_state) = (f.def, f.matrix);
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
                inert(f),
            ),
            RegionKind::Nav => {
                if let Some((chain, at)) = f.map {
                    dungeon_map::draw(
                        display,
                        chain,
                        at,
                        f.ctx.model,
                        (self.branch_scroll.current() * theme::BRANCH_LINE_HEIGHT as f32) as i32,
                    );
                }
            }
            RegionKind::Crumbs => {
                if let Some(b) = &f.settings {
                    b.draw_crumbs(display, f.sounding)
                }
            }
            RegionKind::List => {
                if let Some(b) = &f.settings {
                    b.draw_list(display)
                }
            }
            RegionKind::Footer => {
                if let Some(b) = &f.settings {
                    b.draw_footer(display)
                }
            }
            RegionKind::Prompt => {
                if let Some(a) = f.prompt {
                    let on = match f.settings {
                        Some(_) => Beneath::List,
                        None => Beneath::Page,
                    };
                    a.with_view(|v| draw_prompt(display, v, on))
                }
            }
        }
    }

    /// Focus band: the focused slot large (nothing for an empty slot).
    fn draw_focus<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        if f.def.layout == PageLayout::Matrix {
            return crate::ui::mod_grid::draw_readout(
                display,
                f.matrix,
                amount_of(self.anim[MATRIX_AMOUNT_SLOT].current()),
                inert(f),
            );
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
            self.gauge(f),
            look(f, f.focus),
            f.prime_status,
        );
    }

    /// The focused slot's gauge, by its glyph.
    pub fn gauge(&self, f: &Frame) -> Gauge {
        let view = view::view(f.def, f.focus, &f.ctx);
        let (fmt, glyph, focused) = (view.fmt(), view.glyph(), view.addr());
        let value = match glyph {
            FocusGlyph::Crossfader => self.set[f.focus].current(),
            _ => self.anim[f.focus].current(),
        };
        let frame = f.clock.frame();
        glyph.gauge(value, fmt, |id| {
            // Which of the composite's params has focus.
            let focus = id.params().iter().position(|&a| Some(a) == focused);
            match id {
                CompositeId::ChorusBraid => Gauge::Braid(Braid::from_set(
                    self.eased_set(f, BRAID_PARAMS),
                    focus.map(|i| BraidPart::ALL[i]),
                    frame,
                )),
                CompositeId::DelayRings => Gauge::Rings(Rings::from_set(
                    self.eased_set(f, RINGS_PARAMS),
                    focus.map(|i| RingsPart::ALL[i]),
                    frame,
                )),
                CompositeId::ReverbCube => Gauge::Cube(Cube::from_set(
                    self.eased_set(f, CUBE_PARAMS),
                    focus.map(|i| CubePart::ALL[i]),
                    frame,
                )),
            }
        })
    }

    /// A composite's inputs, `addrs`' set values normalized: eased (`set`)
    /// where the page has a slot for one, else stored; never `anim` or
    /// anything modulated, so the glyph only ever moves by its own
    /// animation.
    fn eased_set<const N: usize>(&self, f: &Frame, addrs: [ParamAddr; N]) -> [f32; N] {
        let stored = stored_set(&Stored(f), addrs);
        core::array::from_fn(|i| {
            (0..f.def.params.len())
                .find(|&s| slot_addr(f.def, s, &f.ctx) == Some(addrs[i]))
                .map_or(stored[i], |s| self.set[s].current())
        })
    }

    /// The gauge the focus band shows now: none while a prime status holds
    /// the band, or for a dimmed, absent or empty slot.
    pub fn shown_gauge(&self, f: &Frame) -> Option<Gauge> {
        let plain = f.def.layout != PageLayout::Matrix && f.def.viz != VizType::AudioStats;
        (plain
            && f.prime_status.is_none()
            && view::view(f.def, f.focus, &f.ctx) != View::Empty
            && look(f, f.focus) == components::Look::Live)
            .then(|| self.gauge(f))
    }

    /// Redraw only `gauge` (the shown one) in its box, if it has one; the
    /// rows to flush.
    pub fn redraw_gauge<D>(
        display: &mut D,
        fb: impl FnOnce(&mut D) -> &mut [u16],
        gauge: Gauge,
    ) -> Option<(u16, u16)>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (x, y, w, h) = components::gauge_rect(&gauge)?;
        Self::clear_rect_fb(fb(display), x, y, w, h);
        components::draw_gauge(display, gauge);
        Some((y as u16, (y + h) as u16))
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
                label: cell_label(&v),
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
        let suffix = ["", " / A", " / B"][title_type(f) as usize % 3];
        let h = components::header_text(f.head, f.def, f.ctx.model, suffix, header_out(f));
        components::header(
            display,
            h.context.as_str(),
            h.name.as_str(),
            h.warn,
            f.sounding,
            f.perf.audio_load_pct,
        );
    }

    // ── Dirty region helpers ─────────────────────────────────────────

    /// Clear a box by direct framebuffer fill.
    pub fn clear_rect_fb(fb: &mut [u16], x: i32, y: i32, w: i32, h: i32) {
        use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
        let bg = RawU16::from(theme::BG).into_inner();
        let sw = theme::SCREEN_W as usize;
        for row in y.max(0) as usize..(y + h).min(theme::SCREEN_H) as usize {
            fb[row * sw + x.max(0) as usize..row * sw + (x + w).min(theme::SCREEN_W) as usize]
                .fill(bg);
        }
    }

    /// Clear a screen region by direct framebuffer fill. Much faster than draw_iter.
    pub fn clear_region_fb(fb: &mut [u16], y_start: u16, y_end: u16) {
        use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
        let bg = RawU16::from(theme::BG).into_inner();
        let start = y_start as usize * theme::SCREEN_W as usize;
        let end = y_end as usize * theme::SCREEN_W as usize;
        fb[start..end].fill(bg);
    }
}

/// `addrs`' stored values, normalized: `eased_set`'s fallback for a param
/// the page has no slot for.
fn stored_set<const N: usize>(stored: &impl BlockRead, addrs: [ParamAddr; N]) -> [f32; N] {
    addrs.map(|a| stored.block(a.block).map_or(0.0, |b| b.normalized(a.param)))
}

/// The edited Part's stored blocks and the FX's, read-only.
struct Stored<'f, 'a>(&'f Frame<'a>);

impl BlockRead for Stored<'_, '_> {
    fn block(&self, b: crate::addr::BlockRef) -> Option<&dyn crate::block::Block> {
        let p = &self.0.parts[self.0.active_part.index()];
        crate::project::part_block(&p.sound, &p.mix, self.0.fx, b)
    }
}

/// The OUT of the Part whose pages these are; P1 in SETTINGS.
pub fn header_out(f: &Frame) -> DacPair {
    match f.head {
        Head::Sound(p) | Head::Mix(p) => f.parts[p.index()].mix.output,
        Head::Settings => DacPair::P1,
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
    let sound = &f.parts[f.active_part.index()].sound;
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

/// The matrix's inert columns on the edited Part (`mod_grid::inert_dests`).
pub fn inert(f: &Frame) -> u16 {
    crate::ui::mod_grid::inert_dests(f.matrix, &f.parts[f.active_part.index()].sound)
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

/// A cell's label: the spec's short form when the full one would outrun
/// the cell's bar into the next column.
fn cell_label(v: &View) -> &'static str {
    let label = v.label();
    match v.short() {
        Some(short)
            if crate::ui::draw::text_width(&theme::FONT_LABEL, label, theme::LABEL_TRACKING)
                > theme::CELL_BAR_W =>
        {
            short
        }
        _ => label,
    }
}
