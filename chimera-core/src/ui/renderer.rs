use crate::dsp::clampf;
use crate::dsp::reverb::ReverbType;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::{Point, Size};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, StyledDrawable};

use crate::addr::{BlockRead, BlockRef, Op, ParamAddr};
use crate::block::find_spec;
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::env::EnvRates;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::modulator::{EnvType, HoldPos, LfoSlot};
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeMachine};
use crate::dsp::va::osc::{Morph, Unit};
use crate::part::DacPair;
use crate::perf::load::AudioStats;
use crate::project::PartId;
use crate::ui::PrimeStatus;
use crate::ui::animation::{AnimatedValue, KNOB_SPEED};
use crate::ui::audio_page;
use crate::ui::block_def::{BlockDef, ChainDef2, FxFlow, SlotBinding, VizType, slot_addr};
use crate::ui::cell_text::{self, CellCx};
use crate::ui::components::{self, Head};
use crate::ui::draw;
use crate::ui::dungeon_map;
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::{
    BRAID_PARAMS, Braid, BraidPart, CompositeId, Cube, CubePart, FocusGlyph, Gauge, REVERB_PARAMS,
    RINGS_PARAMS, Reads, Rings, RingsPart, TAPE_GLYPH_PARAMS, Tail, VA_MIX_PARAMS, VaGlyph, VaMix,
    va_mode,
};
use crate::ui::hint;
use crate::ui::lfo_glyph::{self, LfoGauge};
use crate::ui::mod_grid::MatrixState;
use crate::ui::nav::PageAt;
use crate::ui::orbit::door as orbit;
use crate::ui::orbit::draw::{Focus as OrbitFocus, OrbitFrame};
use crate::ui::page::PageLayout;
use crate::ui::perf::PerfStats;
use crate::ui::region::{self, Layout, RegionKind};
use crate::ui::settings::Ask;
use crate::ui::settings::prompt::{Beneath, blank_beneath, draw_prompt};
use crate::ui::settings::view::Bands;
use crate::ui::tape_glyph::{Pose, TapeGauge};
use crate::ui::theme;
use crate::ui::view::{self, EnvKind, SlotCtx, View};
use crate::ui::viz;

/// Every Part, as the project holds them.
pub type Parts = [crate::preset::Part; crate::hw::MAX_PARTS];

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
    /// ORBIT's beat dot: its fill while ORBIT runs (`OrbitAnim::dot`).
    pub orbit_dot: Option<u8>,
    /// The Parts with a voice sounding (the header's Part dots).
    pub live: crate::project::PartSet,
    /// Every Part (Mixer overview) and the one being edited.
    pub parts: &'a Parts,
    pub active_part: PartId,
    /// The last MIX+PLUS outcome or MIDI learn's word, shown in the focus
    /// band in place of the value readout (issue #21).
    pub prime_status: Option<PrimeStatus>,
    /// MIDI MAP's pointing hint (ADR 0288): over every hint the page
    /// would show, and in a BigViz status line's place.
    #[cfg(feature = "midi-map")]
    pub point: Option<hint::Hint>,
    /// The focused cell's pickup waiting: the hint that says which way the
    /// knob must go, in the focus band or on a BigViz page's status line
    /// (none with HINTS off).
    pub pickup: Option<hint::Hint>,
    /// The project's pool, for PROJECT › POOL.
    pub pool: &'a crate::project::Pool,
    /// The measured stats AUDIO LOAD and ABOUT show; `None` where not had.
    pub audio: Option<&'a AudioStats>,
    /// The card as last seen, for ABOUT.
    pub card: crate::storage::Card,
    /// Bank 2 as the boot judged it, for ABOUT.
    pub bank2: Option<crate::bank2::Bank2Status>,
    /// The master compressor's gain reduction, dB (MST's GR meter).
    pub master_gr_db: f32,
    /// The active Part's LEVEL LED fill (`ClipHold`); 0 is dark.
    pub clip: f32,
    /// MIXER › PRT's meters and key marks (ADR 0293).
    pub meters: &'a crate::ui::part_meter::PartMeters,
    /// The tempo the reverb follows and its synced time's fold.
    pub rev_clock: (
        crate::dsp::rev::sync::Tempo,
        crate::dsp::rev::sync::FoldState,
    ),
    /// Animation phase: what an animated glyph draws from.
    pub clock: crate::ui::animation::UiClock,
    /// The FX's stored params (a composite reads the chorus's).
    pub fx: &'a crate::dsp::fx_bus::FxParams,
    /// What ORBIT's pages draw: the live scene and the UI's view of it.
    pub orbit: OrbitFrame<'a>,
    /// ORBIT's reading of the focused slot, if it has its own: built once
    /// a frame, for the band's key, its gauge and its drawing.
    pub orbit_focus: Option<OrbitFocus>,
    /// THEME's HINTS: gesture hints drawn (ADR 0086).
    pub hints: bool,
    /// THEME's PAGES: how the map shows sub-pages (ADR 0145).
    pub pages: crate::ui::theme_settings::Pages,
    /// TEXT SIZE: the reading text's faces (ADR 0154).
    pub text: theme::TextSize,
    /// SYSTEM's PUSH ENCODERS: a value cell with no hint of its own hints
    /// the pushes (ADR 0140).
    pub push: bool,
    /// MIX is down: a mixer cell's band hints its PART jump in place of
    /// the fine turn (ADR 0279).
    pub mix: bool,
    /// TAPE's strip: the active Part's source, and its newest note's heads.
    pub strip: &'a crate::ui::tape_strip::Overview,
    pub heads: crate::dsp::tape_engine::Heads,
    /// The edited Part's newest voice's formant view (OP pages).
    pub formant: &'a crate::dsp::algo::engine::FormantView,
    /// The focused cell's refused choice, shown dimmed (ADR 0151).
    pub held: Option<crate::ui::held::Shown>,
    /// An LFO page's playhead: its slot's newest voice on the active Part,
    /// `None` with no voice (ADR 0177).
    pub lfo: Option<crate::ui::lfo_glyph::Head>,
    /// PIT's corner (ADR 0201).
    pub tuner: &'a crate::ui::pit_tuner::Shown,
    /// What SETTINGS › TUNING's band says of a Part's file (ADR 0259).
    pub tuned: &'a crate::ui::tuning::Tuned,
    /// The active Part's kit as KIT's band draws it.
    pub pads: crate::ui::kit_band::Pads<'a>,
}

impl<'a> Frame<'a> {
    /// What a cell's text reads from this frame.
    #[inline(always)]
    pub(crate) fn cell(&self) -> CellCx<'_> {
        CellCx {
            def: self.def,
            ctx: &self.ctx,
            held: &self.held,
            part: &self.parts[self.active_part.index()],
            focus: self.focus,
            clock: &self.clock,
            bank2: &self.bank2,
            fx: self.fx,
            rev_clock: &self.rev_clock,
        }
    }

    /// `h`, or none with HINTS off.
    /// MIDI MAP's pointing hint, if pointing (ADR 0288); none without
    /// the map.
    #[inline(always)]
    pub fn point(&self) -> Option<hint::Hint> {
        #[cfg(feature = "midi-map")]
        return self.point;
        #[cfg(not(feature = "midi-map"))]
        None
    }

    pub fn hint(&self, h: hint::Hint) -> hint::Hint {
        // Pointing's keys, whatever HINTS says: no other way out shows.
        match (self.point(), self.hints) {
            (Some(p), _) => p,
            (None, true) => h,
            (None, false) => hint::NONE,
        }
    }

    /// What the header shows beside its words: the load and the dots.
    pub fn header_status(&self) -> components::HeaderStatus {
        components::HeaderStatus {
            parts: matches!(self.head, Head::Sound(_) | Head::Mix(_)).then_some(
                components::PartDots {
                    current: self.active_part,
                    live: self.live,
                },
            ),
            sounding: self.sounding,
            load_pct: self.perf.audio_load_pct,
            orbit_dot: self.orbit_dot,
        }
    }
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
    /// CUSTOM's cursor in twelfths round the circle, unwrapped, so it eases
    /// the short way across 11 and 0 (`animation::nearest`).
    pub orbit_cursor: AnimatedValue,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            anim: [AnimatedValue::new(0.5).with_speed(KNOB_SPEED); 6],
            set: [AnimatedValue::new(0.5).with_speed(KNOB_SPEED); 6],
            branch_scroll: AnimatedValue::new(0.0).with_speed(0.25),
            orbit_cursor: AnimatedValue::new(0.0),
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

    /// FLT's CUTOFF and RES slots, by address (spec § UI "Vizzes read by
    /// address").
    fn filter_slots(&self, f: &Frame) -> (Option<usize>, Option<usize>) {
        use crate::params::FilterParams;
        (
            filter_slot(f, FilterParams::CUTOFF),
            filter_slot(f, FilterParams::RESONANCE),
        )
    }

    /// The curve the viz draws: SEM's at MODE's set value, eased as the
    /// knob's cell is (its stored value off the page).
    fn filter_response(&self, f: &Frame, flt: &crate::params::FilterParams) -> viz::Response {
        use crate::dsp::filter::FilterKind;
        use crate::params::FilterParams;
        if flt.kind() != FilterKind::Sem {
            return viz::Response::of_voice(flt.kind(), flt.mode(), flt.voice);
        }
        let m = filter_slot(f, FilterParams::MORPH).map_or(flt.morph, |s| self.set[s].current());
        viz::Response::sem(flt.mode(), m)
    }

    /// Where modulation has CUTOFF and RES now, against their set values:
    /// the set eased, plus the modulation's own offset (the animators'
    /// targets differ by it alone), so a knob turn alone leaves no mark.
    pub fn filter_mark(&self, f: &Frame) -> viz::ModMark {
        let (c, r) = self.filter_slots(f);
        let moved = |s: Option<usize>| {
            s.map_or((0.0, 0.0), |s| {
                let set = self.set[s].current();
                (set, set + self.anim[s].target() - self.set[s].target())
            })
        };
        let ((c, mc), (r, mr)) = (moved(c), moved(r));
        viz::ModMark::of(c, mc, r, mr)
    }

    /// The values a page's bars and viz draw: the set ones on FLT, whose
    /// modulation is `filter_mark`'s; the modulated ones elsewhere.
    pub fn eased(&self, viz: VizType) -> &[AnimatedValue; 6] {
        if viz == VizType::FilterResponse {
            &self.set
        } else {
            &self.anim
        }
    }

    /// The FLT viz's mark key; 0 on any other page.
    pub fn mark_key(&self, f: &Frame) -> u32 {
        match (f.def.layout, f.def.viz) {
            (PageLayout::BigViz, VizType::FilterResponse) => self.filter_mark(f).key(),
            _ => 0,
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
                let (c, r) = self.filter_slots(f);
                // A DRUM Part's curve is its selected pad's.
                let mut flt = f.parts[f.active_part.index()].sound.params.filter;
                if f.ctx.drum {
                    f.pads
                        .kit
                        .pad(f.ctx.sel_pad)
                        .sound()
                        .filter()
                        .apply(&mut flt);
                }
                let flt = &flt;
                let v = view::view(f.def, f.focus, &f.ctx);
                let buf = value_text(f, f.focus, self.set[f.focus].target());
                // KIND names a mode that has no cell: `TYPE · 4-POLE LP`.
                use core::fmt::Write;
                let mut label = FmtBuf::new();
                let kind = filter_slot(f, crate::params::FilterParams::KIND) == Some(f.focus);
                let _ = match crate::ui::filter_panel::kind_note(flt.kind()) {
                    Some(n) if kind => write!(label, "{} · {n}", v.label()),
                    _ => label.write_str(v.label()),
                };
                let readout = (v != View::Empty).then(|| (label.as_str(), buf.as_str()));
                // The solid curve is the set value; modulation is the mark.
                let at = |s: Option<usize>| s.map_or(0.0, |s| self.set[s].current());
                viz::filter(display, at(c), at(r), self.filter_response(f, flt), readout);
                self.filter_mark(f).draw(display);
                if f.clip > 0.0 {
                    draw::clip_led(display, viz::LED_X, viz::LED_Y, f.clip);
                }
            }
            VizType::Adsr => {
                let first = f.def.params.first().map(|p| p.binding);
                // A pad's AHD: ATTACK, HOLD, DECAY in A-C, DECAY's top INF.
                if let Some(SlotBinding::Param(_)) = first {
                    let (atk, dec) = (a(0).max(0.02), a(2).max(0.02));
                    let hold = a(1).max(viz::label_floor("H", atk + dec));
                    let total = atk + hold + dec;
                    let end = if a(2) >= 1.0 { 1.0 } else { 0.0 };
                    // Its top clear of the status line's PAD hint.
                    viz::envelope_from(
                        display,
                        viz::PLOT_TOP + 8,
                        &[atk / total, hold / total, dec / total],
                        &[0.0, 1.0, 1.0, end],
                        &["A", "H", "D"],
                        (f.focus < 3).then_some(f.focus),
                    );
                } else if let Some(SlotBinding::EnvPanel(s, _)) = first {
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
            }
            VizType::CompressorCurve => {
                use crate::dsp::comp::RATIOS;
                let ratio = RATIOS[((a(1) * 7.0 + 0.5) as usize).min(RATIOS.len() - 1)];
                viz::compressor(display, -40.0 + 40.0 * a(0), ratio, f.master_gr_db);
            }
            VizType::About => crate::ui::about_page::draw_viz(display, f.bank2.as_ref()),
            VizType::Orbit => orbit::orb(display, &f.orbit, f.ctx.ring),
            VizType::OrbitCircle => orbit::circle(display, &f.orbit, self.orbit_cursor.current()),
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
        // Pointing (ADR 0288): its hint in the status line's place.
        if f.point().is_none()
            && let Some(status) = f.prime_status
        {
            let label = view::view(f.def, f.focus, &f.ctx).label();
            let w = components::viz_status(display, status, label);
            // Sensing sure: MIX + SEQ  DONE after the guess, where it fits.
            #[cfg(feature = "midi-map")]
            if let PrimeStatus::Learn(Some(crate::ui::learn::LearnNote::Sensing(s))) = status
                && s.sure()
                && w + 8 + crate::ui::learn::DONE_W <= theme::CONTENT_W
            {
                let x = theme::MARGIN_X + w + 8;
                hint::draw(display, crate::ui::learn::DONE, x, theme::BIGVIZ_STATUS_Y);
            }
            #[cfg(not(feature = "midi-map"))]
            let _ = w;
        } else if f.point().is_none()
            && let Some(h) = f.pickup
        {
            hint::draw(display, h, theme::MARGIN_X, theme::BIGVIZ_STATUS_Y);
        } else if let Some(h) = f.point().or_else(|| match Self::band(f) {
            (pad, true) => Some(pad),
            (_, false) => None,
        }) {
            // Pointing's, or a pad cell's (MIX + -/+ steps the pad, and SEQ
            // + turn is fine), said where the band would, on the status
            // line's own backing (`viz_status`'s).
            draw::fill_rect(
                display,
                theme::MARGIN_X - 2,
                theme::HEADER_BOTTOM,
                hint::width_at(h, theme::TextSize::Normal) + 4,
                viz::PLOT_TOP - theme::HEADER_BOTTOM,
                theme::BG,
            );
            hint::draw(display, h, theme::MARGIN_X, theme::BIGVIZ_STATUS_Y);
        }
    }

    /// The viz band of a CellGrid page.
    fn draw_band_viz<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match f.def.viz {
            VizType::AudioStats => audio_page::draw_viz(display, f.audio),
            VizType::Pool => crate::ui::pool_page::draw_viz(display, f.pool),
            VizType::MixerLevels => {
                viz::parts_overview(display, &self.strips(f), f.active_part.index());
                // Each Part's live meter and key mark beside it (ADR 0293).
                let on = &mut crate::ui::tape_strip::On(display);
                crate::bank2::ui_door(|u| {
                    u.meters(crate::ui::part_meter::Ask::Paint(on, f.meters))
                });
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
            VizType::OrbitRhythm => orbit::rhythm(display, &f.orbit, f.ctx.ring),
            VizType::OrbitVoice => orbit::voice(display, &f.orbit, f.ctx.ring),
            VizType::OrbitNotes => orbit::notes(display, &f.orbit, f.ctx.ring),
            VizType::OrbitChords => orbit::chords(display, &f.orbit),
            VizType::OrbitScenes => orbit::scenes(display, &f.orbit),
            VizType::AlgoDiagram => {
                let algo = &f.parts[f.active_part.index()].sound.params.algo;
                let morph = crate::addr::ParamAddr::new(
                    crate::addr::BlockRef::Algo,
                    crate::dsp::algo::params::AlgoParams::MORPH,
                );
                let slot = (0..f.def.params.len())
                    .find(|&i| slot_addr(f.def, i, &f.ctx) == Some(morph))
                    .expect("ALG page binds MORPH");
                let (a, b) = (algo.algs().0.algorithm(), algo.algs().1.algorithm());
                // Picking ALG A or B shows that algorithm itself (ADR 0153),
                // even a refused, held one; else A moving to B with MORPH.
                if let Some((is_b, held)) = alg_pick(f) {
                    let own = if is_b { b } else { a };
                    let shown = held.map_or(own, |c| AlgoId::clamped(c).algorithm());
                    viz::algo_diagram(display, shown, shown, 0.0);
                } else {
                    viz::algo_diagram(display, a, b, self.anim[slot].current());
                }
            }
            VizType::Strip => {
                let (model, p) = self.strip_params(f);
                crate::ui::tape_strip::draw_strip(
                    display,
                    f.strip,
                    model,
                    &p,
                    f.heads,
                    theme::VIZ_LEFT,
                    crate::ui::tape_strip::Y,
                );
            }
            VizType::OpEnv => {
                if let Some((rates, lit)) = self.op_env(f) {
                    let (widths, heights) = viz::op_env(rates);
                    let top = viz::BAND_PLOT_TOP;
                    viz::envelope_from(display, top, &widths, &heights, &viz::OP_ENV_LABELS, lit);
                }
            }
            VizType::OpPage(op) => {
                let (rates, inset) = Self::op_page(f, op);
                crate::ui::op_page::draw_band(display, rates, inset);
            }
            VizType::Lfo(s) => {
                let w = self.lfo_wave(f, s);
                lfo_glyph::draw_scope(display, &w, lfo_glyph::Dot::of(&w, f.lfo));
            }
            VizType::Pads => crate::ui::kit_band::draw(display, &f.pads),
            VizType::FxCurve => {
                use crate::dsp::voice_fx::{look, params::Place};
                let fx = &f.parts[f.active_part.index()].sound.params.fx;
                // The set pots (ADR 0152). Bank 2's: closed, no band.
                let mut rows = [[0; look::COLS]; 2];
                if !look::trace(fx.ty(), fx.pots, &mut rows) {
                    return;
                }
                // The node before FLT on this chain: VA, OSC, DECK, EXC...
                let source = f.map.map_or("SRC", |(chain, at)| {
                    chain
                        .block_at((at.node() as usize).saturating_sub(1))
                        .map_or("SRC", |b| b.map.unwrap_or(b.def.short))
                });
                viz::fx_strip(display, source, fx.place == Place::Post, 128);
                viz::fx_curve(display, 168, &rows);
            }
            // PIT's corner rides the scope (ADR 0201): empty off PIT.
            _ => {
                viz::live_output(display, f.scope);
                crate::ui::pit_tuner::paint(display, f.tuner);
            }
        }
    }

    /// An OP page's operator: its stored envelope and its kind's inset.
    fn op_page(f: &Frame, op: Op) -> (EnvRates, crate::ui::op_page::Inset) {
        let algo = &f.parts[f.active_part.index()].sound.params.algo;
        (
            algo.ops[op.index()].rates(),
            crate::ui::op_page::Inset::of(algo, f.formant, op),
        )
    }

    /// The active Part's machine and its TAPE params, this page's as its
    /// slots glide: the strip never snaps. The machine is the Part's.
    fn strip_params(&self, f: &Frame) -> (TapeMachine, TapeEngineParams) {
        let t = f.parts[f.active_part.index()].sound.params.tape;
        let mut p = t;
        for (i, anim) in self.anim.iter().enumerate().take(f.def.params.len()) {
            let Some(a) = slot_addr(f.def, i, &f.ctx).filter(|a| a.block == BlockRef::TapeEngine)
            else {
                continue;
            };
            if let Some(s) = find_spec(a.block.specs(), a.param) {
                let v = s.min + anim.current() * (s.max - s.min);
                crate::block::Block::set(&mut p, a.param, v);
            }
        }
        p.machine = t.machine;
        (t.machine, p)
    }

    /// The focused operator's envelope rates, this page's from its animated
    /// slot (to the nearest step), and the stage that rate edits.
    fn op_env(&self, f: &Frame) -> Option<(EnvRates, Option<usize>)> {
        use AlgoOpParams as P;
        let addr = slot_addr(f.def, f.focus, &f.ctx)?;
        let BlockRef::AlgoOp(op) = addr.block else {
            return None;
        };
        let spec = find_spec(addr.block.specs(), addr.param)?;
        let v =
            spec.quantize(spec.min + self.anim[f.focus].current() * (spec.max - spec.min)) as u8;
        let mut r = f.parts[f.active_part.index()].sound.params.algo.ops[op.index()].rates();
        let (field, lit) = match addr.param {
            P::AR => (&mut r.ar, 0),
            P::D1R => (&mut r.d1r, 1),
            P::D1L => (&mut r.d1l, 1),
            P::D2R => (&mut r.d2r, 2),
            P::RR => (&mut r.rr, 3),
            _ => return Some((r, None)),
        };
        *field = v;
        Some((r, Some(lit)))
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
        Self::blank_beneath_prompt(display, f);
        self.draw_region_with_def(display, RegionKind::Prompt, f);
    }

    /// Under an opening prompt: the panel's band over a SETTINGS list,
    /// all but the header and map over a page.
    pub fn blank_beneath_prompt<D>(display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        if f.prompt.is_some() {
            let on = match f.settings {
                Some(_) => Beneath::List,
                None => Beneath::Page,
            };
            blank_beneath(display, on);
        }
    }

    /// What the page's viz is drawn from, for dirty tracking: the slot
    /// values it reads (quantized) and a fingerprint of outside data.
    pub fn viz_inputs(&self, f: &Frame) -> ([u16; 6], u32) {
        match f.def.layout {
            PageLayout::CellGrid => match f.def.viz {
                VizType::AudioStats => ([0; 6], audio_page::viz_key(f.audio)),
                VizType::Pool => ([0; 6], crate::ui::pool_page::viz_key(f.pool)),
                VizType::MixerLevels => (
                    region::quantize_values(&self.anim),
                    strips_key(&self.strips(f), f.active_part.index()) ^ f.meters.key(),
                ),
                VizType::EffectsFlow(_) => (region::quantize_values(&self.anim), f.focus as u32),
                VizType::AlgoDiagram => {
                    let algo = &f.parts[f.active_part.index()].sound.params.algo;
                    let q = region::quantize_values(&self.anim);
                    // The picked cell and its held code move the band too.
                    let pick = alg_pick(f).map_or(0, |(is_b, held)| {
                        let side = u32::from(is_b) + 1;
                        side | u32::from(held.map_or(0, |c| c + 1)) << 2
                    });
                    (
                        [0, 0, q[2], 0, 0, 0],
                        pick << 16 | (algo.algs().0.get() as u32) << 8 | algo.algs().1.get() as u32,
                    )
                }
                // ORBIT's own key: `orbit_viz_key`.
                VizType::OrbitRhythm
                | VizType::OrbitVoice
                | VizType::OrbitNotes
                | VizType::OrbitChords
                | VizType::OrbitScenes => ([0; 6], 0),
                VizType::Strip => {
                    let (model, p) = self.strip_params(f);
                    let key = crate::ui::tape_strip::key(f.strip, model, &p, f.heads);
                    (region::quantize_values(&self.anim), key)
                }
                VizType::OpEnv => {
                    // AR D1R D2R 5 bits, D1L RR 4, the lit stage 3.
                    let key = self.op_env(f).map_or(0, |(r, lit)| {
                        (r.ar as u32) << 23
                            | (r.d1r as u32) << 18
                            | (r.d2r as u32) << 13
                            | (r.d1l as u32) << 9
                            | (r.rr as u32) << 5
                            | lit.map_or(7, |l| l as u32)
                    });
                    ([0; 6], key)
                }
                VizType::OpPage(op) => {
                    let (r, inset) = Self::op_page(f, op);
                    ([0; 6], crate::ui::op_page::key(r, inset))
                }
                // The curve's own steps; the playhead is `lfo_dot`'s.
                VizType::Lfo(s) => ([0; 6], self.lfo_wave(f, s).curve_key()),
                VizType::FxCurve => {
                    let fx = &f.parts[f.active_part.index()].sound.params.fx;
                    let bits = fx.pots.map(f32::to_bits);
                    let tp = (fx.ty() as u32) << 1 | fx.place as u32;
                    ([0; 6], region::fold_keys(tp.into(), &bits))
                }
                VizType::Pads => ([0; 6], crate::ui::kit_band::key(&f.pads)),
                // PIT's corner's key is 0 off PIT.
                _ => ([0; 6], viz::live_key(f.scope) ^ f.tuner.key),
            },
            PageLayout::BigViz => {
                // The GR meter redraws every 0.25 dB; ABOUT on bank 2's word.
                let gr = match f.def.viz {
                    VizType::CompressorCurve => {
                        (clampf(f.master_gr_db, 0.0, viz::GR_RANGE_DB) * 4.0) as u32
                    }
                    VizType::About => f.bank2.map_or(0, |b| 1 + b.is_open() as u32),
                    _ => 0,
                };
                let live = f.focus as u32 | gr << 8 | led_key(f) << 16;
                // The readout's text follows the target, not the eased value.
                // FLT's curve is the set value: its modulation is `filter_mark`.
                // The curve and the readout riding it move by less than a
                // quantum as CUTOFF and RES ease in: key their exact values.
                let (c, r) = match f.def.viz {
                    VizType::FilterResponse => self.filter_slots(f),
                    _ => (None, None),
                };
                let exact = |s: Option<usize>| s.map_or(0, |s| self.set[s].current().to_bits());
                (
                    region::quantize_values(self.eased(f.def.viz)),
                    region::fold_keys(
                        live.into(),
                        &[self.text_key(f, f.focus), exact(c), exact(r)],
                    ),
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
                amount_of(self.anim[MATRIX_AMOUNT_SLOT].target()),
                inert(f),
            ),
            RegionKind::Nav => {
                if let Some((chain, at)) = f.map {
                    dungeon_map::draw(
                        display,
                        chain,
                        at,
                        f.ctx.models(),
                        f.pages,
                        (self.branch_scroll.current() * theme::BRANCH_LINE_HEIGHT as f32) as i32,
                    );
                }
            }
            RegionKind::Crumbs => {
                if let Some(b) = &f.settings {
                    b.draw_crumbs(display, f.header_status())
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
                    a.with_view(|v| draw_prompt(display, v))
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
                amount_of(self.anim[MATRIX_AMOUNT_SLOT].target()),
                inert(f),
                f.hint(view::matrix_hint(f.matrix)),
            );
        }
        if f.def.viz == VizType::AudioStats {
            return audio_page::draw_focus(display, f.def, f.audio, f.text);
        }
        if f.def.viz == VizType::Pool {
            return crate::ui::pool_page::draw_focus(display, f.pool, f.text);
        }
        let view = view::view(f.def, f.focus, &f.ctx);
        if view == View::Empty {
            return;
        }
        if let Some(o) = &f.orbit_focus {
            let (label, text) = (o.label.as_str(), o.text.as_str());
            components::focus_band(
                display,
                label,
                text,
                o.gauge,
                look(f, f.focus),
                f.prime_status,
                f.text,
            );
            let right = hint_right(&o.gauge);
            return hint::focus(display, f.hint(o.hint), f.text, right);
        }
        let v = self.set[f.focus].target();
        let held = held_on(f, &view).map(|h| h.code());
        // The cell's own text where bank 2 can't say (NEEDS UPDATE).
        let word = view
            .addr()
            .filter(|&a| crate::bank2::closed_text(a, v, f.bank2).is_none())
            .and_then(|a| view::readout_word(a, v, held));
        let buf = match word {
            Some(w) => {
                let mut b = FmtBuf::new();
                let _ = core::fmt::Write::write_str(&mut b, w);
                b
            }
            None => value_text(f, f.focus, v),
        };
        let gauge = self.gauge(f);
        let label = band_label(f, &view);
        components::focus_band(
            display,
            label.as_str(),
            buf.as_str(),
            gauge,
            look(f, f.focus),
            f.prime_status,
            f.text,
        );
        // Sensing: learn's hint line, whatever the cell's own (spec § 8.1).
        #[cfg(feature = "midi-map")]
        if let Some(PrimeStatus::Learn(Some(crate::ui::learn::LearnNote::Sensing(s)))) =
            f.prime_status
        {
            if s.sure() {
                hint::draw(
                    display,
                    crate::ui::learn::DONE,
                    theme::MARGIN_X,
                    hint::FOCUS_Y,
                );
            }
            return;
        }
        // Pointing: its keys, whatever the cell's own (ADR 0288).
        // Pointing: its keys, over the cell's own (ADR 0288).
        if f.point().is_some() {
            return hint::focus(display, Self::focus_hint(f), f.text, hint_right(&gauge));
        }
        // A pickup waiting: which way to turn, over the cell's own hint.
        // Never LARGE: it is near the line's whole width at NORMAL.
        if let Some(h) = f.pickup {
            return hint::draw(display, h, theme::MARGIN_X, hint::FOCUS_Y);
        }
        let part = &f.parts[f.active_part.index()];
        let sound = &part.sound;
        let tuning = view
            .addr()
            .and_then(|a| view::tuning_note(a, f.tuned, f.active_part, part));
        let tonal = view.addr().and_then(|a| {
            view::tonal_note(a, &f.ctx).or_else(|| view::kit_or_voices_note(a, f.pads.kit, f.parts))
        });
        let note = view
            .addr()
            .and_then(|a| view::band_note(a, sound, f.ctx.src).or(view::delay_note(a, &f.ctx)))
            .or(tuning.as_ref().map(|l| l.as_str()))
            .or(tonal.as_ref().map(|l| l.as_str()))
            .or_else(|| view::clash_note(&view, &f.ctx));
        match (f.held.filter(|s| held_on(f, &view) == Some(s.held)), note) {
            (Some(s), _) => {
                let why = crate::ui::held::reason(s.why);
                Self::draw_reason(display, why.as_str(), theme::ALERT)
            }
            (None, Some(n)) => Self::draw_reason(display, n, theme::MID),
            (None, None) => hint::focus(display, Self::focus_hint(f), f.text, hint_right(&gauge)),
        }
        if let View::Source(view::SourceState::Card { cost, free, .. }) = view {
            crate::ui::source_cell::draw_card_row(display, f.pool, cost, free, f.hints);
        }
    }

    /// The hint the focus band draws now (none with HINTS off). PUSH
    /// ENCODERS on: the pushes' hint over none or the generic LEARN (the
    /// two never fit together); a cell's own words stay.
    pub fn focus_hint(f: &Frame) -> hint::Hint {
        Self::band(f).0
    }

    /// The focus band's hint, and whether its cell is a pad's (its words
    /// go on a big viz's status line too). Once per cell: the band and
    /// the status line share it.
    fn band(f: &Frame) -> (hint::Hint, bool) {
        if let Some(o) = &f.orbit_focus {
            return (f.hint(o.hint), false);
        }
        let view = view::view(f.def, f.focus, &f.ctx);
        let words = view::cell_words(f.def, &view, &f.ctx);
        let band = match f.head {
            Head::Mix(_) => crate::ui::feel::Band::Mixer { mix: f.mix },
            Head::Sound(_) => crate::ui::feel::Band::Sound,
            _ => crate::ui::feel::Band::Other,
        };
        let push = if f.push {
            crate::ui::feel::Pushes::On
        } else {
            crate::ui::feel::Pushes::Off
        };
        // With the fine turn in it, bank 2's (ADR 0279); closed, the
        // pushes', the cell's own and the mixer's PART jump.
        let h = crate::bank2::feel::hint(&view, words, band, push).unwrap_or_else(|| {
            let generic = matches!(words, view::Words::Plain | view::Words::Learn);
            if cfg!(feature = "push-encoders") && generic && f.push && view.pushes() {
                crate::ui::push::HINT
            } else if matches!(f.head, Head::Mix(_)) {
                view::mixer_hint(words)
            } else {
                words.hint()
            }
        });
        (f.hint(h), f.hints && matches!(words, view::Words::Pad))
    }

    /// Words in the hint's place: why the cell's choice is dimmed (ALERT),
    /// or a TAPE cell's note (`view::band_note`, MID).
    fn draw_reason<D>(display: &mut D, text: &str, color: Rgb565)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let t = crate::ui::source_cell::tracking(text);
        crate::ui::draw::text_tracked(
            display,
            &theme::FONT_LABEL,
            text,
            theme::MARGIN_X,
            hint::FOCUS_Y,
            color,
            t,
        );
    }

    /// What an ORBIT viz draws, for its region's key; 0 on any other.
    pub fn orbit_viz_key(&self, f: &Frame) -> u32 {
        orbit::viz_key(f.def, &f.orbit, f.ctx.ring, self.orbit_cursor.current())
    }

    /// A hash of slot `i`'s printed text at its target, and of the focused
    /// cell's pickup hint: a move below the display resolution leaves it, a
    /// change in the number changes it.
    pub fn text_key(&self, f: &Frame, i: usize) -> u32 {
        if view::view(f.def, i, &f.ctx) == View::Empty {
            return 0;
        }
        let text = value_text(f, i, self.set[i].target());
        let label = band_label(f, &view::view(f.def, i, &f.ctx));
        // The focused cell's pickup hint rides with its text (§ 8.3).
        let pickup = f.pickup.map_or(0, |h| h.as_ptr() as usize as u32);
        region::settings_key(&[text.as_str().as_bytes(), label.as_str().as_bytes()]) ^ pickup
    }

    /// The matrix amount as printed (the target's `amount_of`).
    pub fn amount_key(&self) -> u32 {
        amount_of(self.anim[MATRIX_AMOUNT_SLOT].target()) as u8 as u32
    }

    /// The focused slot's gauge, by its glyph; ORBIT's own where it reads
    /// the slot itself.
    pub fn gauge(&self, f: &Frame) -> Gauge {
        if let Some(o) = &f.orbit_focus {
            return components::fit_gauge(o.text.as_str(), o.gauge);
        }
        let view = view::view(f.def, f.focus, &f.ctx);
        let (fmt, glyph, focused) = (view.fmt(), view.glyph(), view.addr());
        let value = match glyph {
            FocusGlyph::Crossfader
            | FocusGlyph::Staff
            | FocusGlyph::Wave
            | FocusGlyph::Stability
            | FocusGlyph::Feedback => self.set[f.focus].current(),
            _ => self.anim[f.focus].current(),
        };
        let frame = f.clock.frame();
        let text = value_text(f, f.focus, self.set[f.focus].target());
        let g = super::glyph::gauge(glyph, value, fmt, |reads| match reads {
            Reads::Composite(id) => {
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
                    // The cube is QUAD's picture; the others draw their TAIL.
                    // The cube's params lead `REVERB_PARAMS`.
                    CompositeId::Reverb => {
                        let set = self.eased_set(f, REVERB_PARAMS);
                        if f.ctx.rev == ReverbType::Quad {
                            Gauge::Cube(Cube::from_set(
                                core::array::from_fn(|i| set[i]),
                                focus.and_then(|i| CubePart::ALL.get(i).copied()),
                                frame,
                            ))
                        } else {
                            Gauge::Tail(Tail::from_set(f.ctx.rev, set, focus, frame))
                        }
                    }
                }
            }
            Reads::Va(VaGlyph::Wave(osc)) => {
                let [wave, shape] = self.eased_set(f, osc.params());
                Gauge::VaWave(Morph {
                    wave: Unit::new(wave),
                    shape: Unit::new(shape),
                })
            }
            Reads::Va(VaGlyph::Mode) => {
                Gauge::VaMix(VaMix::from_set(self.eased_set(f, VA_MIX_PARAMS)))
            }
            // Without TAPE's DSP (the bench) there is no glyph to draw.
            Reads::Tape if !cfg!(feature = "tape") => Gauge::None,
            Reads::Tape => {
                use crate::dsp::tape_engine::TapeEngineParams as T;
                // MODEL names the machine: its still identity.
                let pose = if focused.is_some_and(|a| a.param == T::MODEL) {
                    Pose::Still
                } else {
                    Pose::Live
                };
                let set = self.eased_set(f, TAPE_GLYPH_PARAMS);
                Gauge::Tape(TapeGauge::of_set(set, pose))
            }
            Reads::Lfo => match focused.map(|a| a.block) {
                Some(BlockRef::Lfo(s)) => {
                    let kind = f.ctx.lfos[s.index()];
                    let p = focused.map_or(crate::block::ParamId(0), |a| a.param);
                    lfo_glyph::glyph_for(kind, p).map_or(Gauge::None, |glyph| {
                        Gauge::Lfo(LfoGauge {
                            glyph,
                            wave: self.lfo_wave(f, s),
                        })
                    })
                }
                _ => Gauge::None,
            },
            Reads::Drum => f.ctx.pad.map_or(Gauge::None, Gauge::Drum),
            Reads::Va(VaGlyph::Blend) => {
                let [mode, amount] = self.eased_set(f, [VA_MIX_PARAMS[0], VA_MIX_PARAMS[1]]);
                Gauge::Blend {
                    value: amount,
                    mode: va_mode(mode),
                }
            }
        });
        components::fit_gauge(text.as_str(), g)
    }

    /// LFO slot `s`'s wave on its page, from its set values eased.
    fn lfo_wave(&self, f: &Frame, s: LfoSlot) -> lfo_glyph::Wave {
        lfo_glyph::wave(
            f.ctx.lfos[s.index()],
            self.eased_set(f, lfo_glyph::params(s)),
        )
    }

    /// The LFO scope's playhead on its page, if this is one.
    pub fn lfo_dot(&self, f: &Frame) -> Option<lfo_glyph::Dot> {
        match f.def.viz {
            VizType::Lfo(s) => Some(lfo_glyph::Dot::of(&self.lfo_wave(f, s), f.lfo)),
            _ => None,
        }
    }

    /// Redraw only the LFO scope's playhead, moved from `was` (its old and
    /// new boxes, each cleared and drawn whole within it); the rows to
    /// flush.
    pub fn redraw_lfo_dot<D>(
        &self,
        display: &mut D,
        fb: impl Fn(&mut D) -> &mut [u16],
        f: &Frame,
        was: lfo_glyph::Dot,
    ) -> [(u16, u16); 2]
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let VizType::Lfo(s) = f.def.viz else {
            return [(0, 0); 2];
        };
        let w = self.lfo_wave(f, s);
        let now = lfo_glyph::Dot::of(&w, f.lfo);
        let (ys, _) = lfo_glyph::columns(&w);
        let boxes = [was, now].map(|dot| dot.rect(&ys));
        for (x, y, bw, bh) in boxes {
            Self::clear_rect_fb(fb(display), x, y, bw, bh);
        }
        // The curve under each box's columns, then the dot on top.
        for (x, _, bw, _) in boxes {
            let c0 = (x - theme::VIZ_LEFT).max(0) as usize;
            let c1 = (x + bw - theme::VIZ_LEFT).max(0) as usize;
            lfo_glyph::draw_scope_cols(display, &w, now, c0..c1);
        }
        boxes.map(|(_, y, _, bh)| (y.max(0) as u16, (y + bh) as u16))
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
    /// the band, or for an absent or empty slot, or a dimmed one but for
    /// TAPE's machine glyph, which a cell its machine ignores leaves as it
    /// was (ADR 0173).
    pub fn shown_gauge(&self, f: &Frame) -> Option<Gauge> {
        let plain = f.def.layout != PageLayout::Matrix
            && !matches!(f.def.viz, VizType::AudioStats | VizType::Pool);
        if !plain || f.prime_status.is_some() || view::view(f.def, f.focus, &f.ctx) == View::Empty {
            return None;
        }
        match look(f, f.focus) {
            components::Look::Live => Some(self.gauge(f)),
            // A TAPE cell its machine ignores, an LFO cell its mode
            // ignores, keep their glyph (ADR 0173, 0177).
            components::Look::Dimmed => {
                Some(self.gauge(f)).filter(|g| matches!(g, Gauge::Tape(_) | Gauge::Lfo(_)))
            }
            _ => None,
        }
    }

    /// Redraw only `gauge` (the shown one) in its box, if it has one; the
    /// rows to flush.
    pub fn redraw_gauge<D>(
        display: &mut D,
        fb: impl FnOnce(&mut D) -> &mut [u16],
        gauge: Gauge,
        hint: hint::Hint,
        text: theme::TextSize,
    ) -> Option<(u16, u16)>
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let (x, y, w, h) = components::gauge_rect(&gauge)?;
        Self::clear_rect_fb(fb(display), x, y, w, h);
        components::draw_gauge(display, gauge);
        // The box clears the hint's top row where the two meet.
        hint::focus(display, hint, text, hint_right(&gauge));
        Some((y as u16, (y + h) as u16))
    }

    /// The six cells, first row's labels at `top`.
    fn draw_cells<D>(&self, display: &mut D, f: &Frame, top: i32)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        match f.def.viz {
            VizType::AudioStats => {
                return audio_page::draw_cells(display, f.def, f.audio, top, f.text);
            }
            VizType::Pool => {
                return crate::ui::pool_page::draw_cells(display, f.def, f.pool, top, f.text);
            }
            VizType::About => {
                return crate::ui::about_page::draw_cells(
                    display, f.def, f.audio, f.card, top, f.text,
                );
            }
            _ => {}
        }
        for i in 0..self.anim.len() {
            let v = view::view(f.def, i, &f.ctx);
            if v == View::Empty {
                components::cell(display, i, top, None, f.text);
                continue;
            }
            let value = self.anim[i].current();
            let mut buf = FmtBuf::new();
            match v {
                // The cell keeps a readout's first words: `POLY`, not
                // `POLY (UNPITCHED)`; the focus band reads it whole.
                View::Text { text, .. } => {
                    let end = text.bytes().position(|b| b == b'(').unwrap_or(text.len());
                    let text = text[..end].trim_end();
                    let _ = core::fmt::Write::write_str(&mut buf, text);
                }
                View::Orbit(cell) => buf = orbit::cell_text(cell, &f.orbit, f.ctx.ring),
                View::Source(s) => s.cell(&mut buf),
                // A column wide: NEEDS UPDATE's last word (the band reads it whole).
                _ if v
                    .addr()
                    .and_then(|a| crate::bank2::closed_text(a, self.set[i].target(), f.bank2))
                    .is_some() =>
                {
                    let _ = core::fmt::Write::write_str(&mut buf, crate::bank2::NEEDS_UPDATE_CELL);
                }
                _ => buf = param_text(f, i, &v, self.set[i].target()),
            }
            // A name wider than the column keeps its first word: `SOFT`,
            // not `SOFT SYNC` (the focus band reads it whole); a pad's
            // MACHINE as `pad_machine_cell` says.
            let one = match v.addr() {
                Some(a) if a == PAD_MACHINE => f.ctx.pad.map(pad_machine_cell),
                _ => buf.as_str().split(' ').next(),
            }
            .unwrap_or("");
            if one.len() < buf.as_str().len()
                && draw::text_width(&theme::FONT_VALUE, buf.as_str(), 0) > CELL_VALUE_W
            {
                let mut first = FmtBuf::new();
                let _ = core::fmt::Write::write_str(&mut first, one);
                buf = first;
            }
            let c = components::Cell {
                label: cell_label(&v),
                text: buf.as_str(),
                value,
                fmt: v.fmt(),
                active: i == f.focus,
                mod_amount: mod_info(f, i),
                look: look(f, i),
                motion: v.addr().and_then(|a| a.spec()).map(|s| s.motion()),
                mini: (v.addr() == Some(PLACE)).then(|| components::MiniGlyph::Place {
                    post: self.set[i].current() >= 0.5,
                }),
            };
            let w = components::cell(display, i, top, Some(&c), f.text);
            if f.clip > 0.0 && v.addr() == Some(LEVEL) {
                draw::clip_led(
                    display,
                    theme::MARGIN_X + (i % 3) as i32 * theme::CELL_COL_W + w + LED_DX,
                    top + (i / 3) as i32 * theme::CELL_ROW_H + theme::cell_lines(f.text).value
                        - LED_RISE,
                    f.clip,
                );
            }
        }
    }

    /// Header band: context, page name, audio load, sounding dot.
    fn draw_header<D>(&self, display: &mut D, f: &Frame)
    where
        D: DrawTarget<Color = Rgb565>,
    {
        let h = page_header(f.head, f.def, f.focus, &f.ctx, f.parts, f.bank2);
        components::header(
            display,
            h.context.as_str(),
            h.name.as_str(),
            h.warn,
            f.header_status(),
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

/// Where a LARGE focus hint must end: clear of `gauge`, which its taller
/// line would reach (ADR 0154).
fn hint_right(gauge: &Gauge) -> i32 {
    components::focus_label_slot(gauge).right
}

/// `addrs`' stored values, normalized: `eased_set`'s fallback for a param
/// the page has no slot for.
fn stored_set<const N: usize>(stored: &impl BlockRead, addrs: [ParamAddr; N]) -> [f32; N] {
    addrs.map(|a| stored.block(a.block).map_or(0.0, |b| b.normalized(a.param)))
}

/// The edited Part's stored blocks and the FX's, read-only.
struct Stored<'f, 'a>(&'f Frame<'a>);

impl BlockRead for Stored<'_, '_> {
    fn block(&self, b: crate::addr::BlockRef) -> Option<crate::addr::BlockView<'_>> {
        let p = &self.0.parts[self.0.active_part.index()];
        crate::project::part_block(&p.sound, &p.mix, self.0.fx, b)
    }
}

/// The header a page draws: the one place its name is decided, for the
/// screen and for the console's `status`.
pub fn page_header(
    head: Head,
    def: &BlockDef,
    focus: usize,
    ctx: &SlotCtx,
    parts: &Parts,
    bank2: Option<crate::bank2::Bank2Status>,
) -> components::HeaderText {
    let suffix = ["", " / A", " / B"][title_type(def, &ctx.envs) as usize % 3];
    let closed = match head {
        Head::Sound(p) | Head::Mix(p) => {
            crate::bank2::closed_part(parts[p.index()].sound.engine(), bank2)
        }
        Head::Ring(_) | Head::Orbit(_) => crate::bank2::closed_orbit(bank2),
        Head::Settings => None,
    };
    components::header_text(
        head,
        def,
        // The pad's name only where the page edits it.
        view::Models {
            pad: ctx
                .models()
                .pad
                .filter(|_| view::edits_pad(def, focus, ctx)),
            ..ctx.models()
        },
        ctx.kind,
        suffix,
        out_of(head, parts),
        closed,
    )
}

/// The OUT of the Part whose pages these are; P1 in SETTINGS.
pub fn header_out(f: &Frame) -> DacPair {
    out_of(f.head, f.parts)
}

fn out_of(head: Head, parts: &Parts) -> DacPair {
    match head {
        Head::Sound(p) | Head::Mix(p) => parts[p.index()].mix.output,
        Head::Settings | Head::Ring(_) | Head::Orbit(_) => DacPair::P1,
    }
}

/// The ENV page title's TYPE suffix: 0 none, 1 A, 2 B.
pub fn title_type(def: &BlockDef, envs: &[EnvKind; 3]) -> u8 {
    match def.params.first().map(|p| p.binding) {
        Some(SlotBinding::EnvPanel(s, _)) => match envs[s.index()] {
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
        View::Route { route, .. } => match cell_text::route_absent(sound, route) {
            false => components::Look::Live,
            true => components::Look::Absent,
        },
        View::Source(view::SourceState::Card { .. }) => components::Look::Dimmed,
        v if held_on(f, &v).is_some() => components::Look::Dimmed,
        v if view::is_dimmed(&v, sound, &f.ctx) || view::clash_note(&v, &f.ctx).is_some() => {
            components::Look::Dimmed
        }
        _ => components::Look::Live,
    }
}

/// The focus band's label: FREQ MOD and RES MOD name the source their jack
/// acts as now (`FREQ MOD LFO1`), as the JACK page does.
pub fn band_label(f: &Frame, view: &View) -> FmtBuf {
    use core::fmt::Write;
    let mut label = FmtBuf::new();
    // Sensing, the control heard: `CC 21 → CUTOFF` (spec § 8.1).
    #[cfg(feature = "midi-map")]
    if let Some(PrimeStatus::Learn(Some(crate::ui::learn::LearnNote::Sensing(s)))) = f.prime_status
    {
        crate::ui::learn::label_line(s, view.label(), &mut label);
        return label;
    }
    let _ = label.write_str(view.label());
    if let View::Route {
        route: crate::ui::filter_panel::RouteView::Jack(j),
        ..
    } = view
    {
        let sound = &f.parts[f.active_part.index()].sound;
        let _ = write!(label, " {}", j.effective(sound).tag());
    }
    label
}

/// The matrix's inert columns on the edited Part (`mod_grid::inert_dests`).
pub fn inert(f: &Frame) -> u32 {
    crate::ui::mod_grid::inert_dests(f.matrix, &f.parts[f.active_part.index()].sound)
}

/// Mod-bar amount for cell `i`, if its param is a destination.
pub(crate) fn mod_info(f: &Frame, i: usize) -> Option<f32> {
    slot_addr(f.def, i, &f.ctx).and_then(|a| f.matrix.mod_info_for(a))
}

/// Slot `i`'s value as a cell, the focus band and a viz readout show it
/// (`cell_text::value_text`).
#[inline(never)]
pub(crate) fn value_text(f: &Frame, i: usize, v: f32) -> FmtBuf {
    cell_text::value_text(&f.cell(), i, v)
}

/// The ALGO page's picked cell (ALG A or B is the focus) and the refused
/// code held on it, if any.
fn alg_pick(f: &Frame) -> Option<(bool, Option<u8>)> {
    let at = slot_addr(f.def, f.focus, &f.ctx)?;
    [AlgoParams::ALG_A, AlgoParams::ALG_B]
        .into_iter()
        .find(|&id| at == ParamAddr::new(BlockRef::Algo, id))
        .map(|id| {
            let held = f.held.filter(|s| s.held.addr().param == id);
            (id == AlgoParams::ALG_B, held.map(|s| s.held.code()))
        })
}

/// A param's text at display value `v` (`cell_text::param_text`), out of
/// line as it was: a caller builds no `CellCx`.
#[inline(never)]
fn param_text(f: &Frame, i: usize, view: &View, v: f32) -> FmtBuf {
    cell_text::param_text(&f.cell(), i, view, v)
}

/// The refused choice `view` shows in place of its value, if it is the cell.
#[inline(always)]
fn held_on(f: &Frame, view: &View) -> Option<crate::ui::held::Held> {
    cell_text::held_on(f.held, view)
}

/// Fingerprint of the Mixer overview (quantized levels and pans, selection).
fn strips_key(strips: &[viz::Strip], selected: usize) -> u32 {
    use core::hash::Hasher;
    let mut h = region::Fnv::new();
    h.write_usize(selected);
    for s in strips {
        h.write_u16(region::quantize(s.level));
        h.write_u16(region::quantize(s.pan + 1.0));
    }
    h.get()
}

/// The matrix page shows the selected amount through this display slot.
pub const MATRIX_AMOUNT_SLOT: usize = 4;

/// Amount −127..127 as a 0..1 display value (0 at the centre).
/// The filter LEVEL, whose cell (FLT › FX's INPUT) carries the clip LED.
const LEVEL: ParamAddr = ParamAddr::new(BlockRef::Filter, crate::params::FilterParams::LEVEL);
const PLACE: ParamAddr = ParamAddr::new(
    BlockRef::VoiceFx,
    crate::dsp::voice_fx::params::VoiceFxParams::PLACE,
);

/// The page's slot on filter param `id`: the Sound's, or on DRUM the pad's.
fn filter_slot(f: &Frame, id: crate::block::ParamId) -> Option<usize> {
    (0..f.def.params.len()).find(|&i| {
        slot_addr(f.def, i, &f.ctx).is_some_and(|a| {
            a.param == id && matches!(a.block, BlockRef::Filter | BlockRef::PadFilter)
        })
    })
}
/// A DRUM pad's MACHINE: its cell keeps the name's last word.
const PAD_MACHINE: ParamAddr =
    ParamAddr::new(BlockRef::Pad, crate::dsp::drum::params::PadParams::MACHINE);
/// The widest value a cell draws before it runs into the next column's.
pub const CELL_VALUE_W: i32 = theme::CELL_COL_W - 4;

/// A pad's MACHINE in its cell (owner rule: full words when they fit):
/// the name, else its last word, the instrument (`SNARE`, not `TONE
/// SNARE`), else the machine's short name (`BELL`: `COWBELL` ran into
/// LEVEL). The focus band reads it whole.
pub fn pad_machine_cell(m: crate::dsp::drum::types::Machine) -> &'static str {
    let s = crate::dsp::drum::laws::spec(m);
    let fits = |t: &str| draw::text_width(&theme::FONT_VALUE, t, 0) <= CELL_VALUE_W;
    let last = s.name.rsplit(' ').next().unwrap_or(s.name);
    if fits(s.name) {
        s.name
    } else if fits(last) {
        last
    } else {
        s.short
    }
}

/// The LED's centre from the end of LEVEL's value, and above its baseline.
const LED_DX: i32 = 7;
const LED_RISE: i32 = 7;

/// The LED's redraw key: its fill on a page that draws it (FLT's viz, a
/// LEVEL cell), else 0.
pub fn led_key(f: &Frame) -> u32 {
    let shows = f.def.viz == VizType::FilterResponse
        || (0..f.def.params.len()).any(|i| view::view(f.def, i, &f.ctx).addr() == Some(LEVEL));
    if shows {
        region::quantize(f.clip) as u32
    } else {
        0
    }
}

pub fn amount_value(amount: i8) -> f32 {
    0.5 + amount as f32 / 254.0
}

/// Inverse of `amount_value`, rounded.
pub fn amount_of(v: f32) -> i8 {
    clampf(libm::roundf((v - 0.5) * 254.0), -127.0, 127.0) as i8
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
