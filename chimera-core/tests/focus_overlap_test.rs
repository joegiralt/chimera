//! The focus band's gauge never touches its value: for every param's
//! widest formatted value (each unit, ratio and word), no pixel of the
//! value and of the gauge the band shows coincide, and every gauge keeps
//! to the columns `gauge_left` says it does.

mod screen;

use std::collections::HashSet;

use chimera_core::addr::BlockRef;
use chimera_core::block::{Block, ParamId, ParamSpec, ValFmt};
use chimera_core::dsp::rev::sync::{FoldState, Tempo};
use chimera_core::dsp::reverb::{ReverbParams, ReverbType};
use chimera_core::dsp::va::VaMode;
use chimera_core::dsp::va::osc::{Morph, Unit};
use chimera_core::ui::block_def::BlockDef;
use chimera_core::ui::block_registry::ALL_CHAINS;
use chimera_core::ui::components::{self, Layout, Look};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::glyph::{
    Accidental, Braid, CompositeId, FocusGlyph, Gauge, Reads, Rings, Tail, VaGlyph, VaMix,
    gauge as make_gauge,
};
use chimera_core::ui::reverb_panel;
use chimera_core::ui::theme;
use chimera_core::ui::view::{SlotCtx, view};
use screen::*;

/// Normalized steps swept per format: finer than any format's resolution.
const STEPS: usize = 1024;
const VALUES: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

/// The gauge `glyph` draws at `v`, with composites at one setting.
fn gauge_at(glyph: FocusGlyph, fmt: ValFmt, v: f32, rev: ReverbType) -> Gauge {
    make_gauge(glyph, v, fmt, |reads| match reads {
        Reads::Composite(CompositeId::ChorusBraid) => {
            Gauge::Braid(Braid::from_set([v; 4], None, 0))
        }
        Reads::Composite(CompositeId::DelayRings) => Gauge::Rings(Rings::from_set([v; 7], None, 0)),
        Reads::Composite(CompositeId::Reverb) if rev == ReverbType::Quad => {
            Gauge::Cube(chimera_core::ui::glyph::Cube::from_set([v; 6], None, 0))
        }
        Reads::Composite(CompositeId::Reverb) => Gauge::Tail(Tail::from_set(rev, [v; 11], None, 0)),
        Reads::Va(VaGlyph::Wave(_)) => Gauge::VaWave(Morph {
            wave: Unit::new(v),
            shape: Unit::new(v),
        }),
        Reads::Va(VaGlyph::Mode) => Gauge::VaMix(VaMix::from_set([v; 6])),
        Reads::Va(VaGlyph::Blend) => Gauge::Blend {
            value: v,
            mode: VaMode::ALL[(v * 4.0) as usize],
        },
        Reads::Lfo => {
            use chimera_core::lfo_glyph::{Glyph, LfoGauge, Q, Wave};
            Gauge::Lfo(LfoGauge {
                glyph: Glyph::Rate,
                wave: Wave::Classic {
                    rate: Q::of(v),
                    shape: chimera_core::dsp::lfo::LfoShape::Sine,
                    retrig: false,
                    phase: Q::of(0.0),
                    depth: Q::of(1.0),
                },
            })
        }
    })
}

fn render(f: impl FnOnce(&mut Fb)) -> Fb {
    let mut fb = Fb::new();
    f(&mut fb);
    fb
}

fn ink(fb: &Fb, i: usize) -> bool {
    fb.px[i] != 0
}

/// The value alone, and with `g`: the pixels `g` landed on, and if the
/// band drew it at all.
fn overlap(label: &str, text: &str, g: Gauge) -> (bool, Vec<(usize, usize)>) {
    let value = render(|d| {
        components::focus_band(
            d,
            label,
            text,
            Gauge::None,
            Look::Live,
            None,
            chimera_core::ui::theme::TextSize::Normal,
        )
    });
    let both = render(|d| {
        components::focus_band(
            d,
            label,
            text,
            g,
            Look::Live,
            None,
            chimera_core::ui::theme::TextSize::Normal,
        )
    });
    let shown = value.px != both.px;
    let alone = render(|d| components::draw_gauge(d, g));
    // Beside an LFO glyph a long value drops to the readout size (ADR
    // 0177): what it must keep clear of is the glyph's box and its gap.
    if let Gauge::Lfo(_) = g {
        use chimera_core::ui::theme;
        let (x0, y0) = (theme::WAVE_X, theme::WAVE_Y);
        let in_box = |x: i32, y: i32| {
            (x0..x0 + theme::WAVE_W).contains(&x) && (y0..y0 + theme::WAVE_H).contains(&y)
        };
        let label_bottom = theme::FOCUS_LABEL_Y + 4;
        let hits = (0..W * H)
            .map(|i| ((i % W) as i32, (i / W) as i32, i))
            .filter(|&(x, y, i)| {
                y > label_bottom
                    && y <= theme::FOCUS_VALUE_Y
                    && ink(&both, i)
                    && !ink(&alone, i)
                    && !in_box(x, y)
                    && x >= x0 - components::GLYPH_GAP
            })
            .map(|(x, y, _)| (x as usize, y as usize))
            .collect();
        return (shown, hits);
    }
    let hits = (0..W * H)
        .filter(|&i| ink(&value, i) && (value.px[i] != both.px[i] || (shown && ink(&alone, i))))
        .map(|i| (i % W, i / W))
        .collect();
    (shown, hits)
}

/// The text of `fmt` that sets widest at the focus size, over its range.
fn widest(fmt: ValFmt, spec: Option<&ParamSpec>) -> String {
    let mut best = (0, String::new());
    for k in 0..=STEPS {
        let v = k as f32 / STEPS as f32;
        // `LogHz` and `Vowel` read the stored value.
        let v = match (fmt, spec) {
            (ValFmt::LogHz { .. } | ValFmt::Vowel, Some(s)) => s.denormalize(v),
            _ => v,
        };
        let mut buf = FmtBuf::new();
        fmt_val(&mut buf, v, fmt);
        let w = components::big_width(buf.as_str());
        if w > best.0 {
            best = (w, buf.as_str().to_string());
        }
    }
    best.1
}

struct Cases {
    seen: HashSet<(String, String)>,
    shown: usize,
    checked: usize,
}

impl Cases {
    fn check(
        &mut self,
        what: &str,
        label: &str,
        text: &str,
        glyph: FocusGlyph,
        fmt: ValFmt,
        rev: ReverbType,
    ) {
        if !self
            .seen
            .insert((text.to_string(), format!("{glyph:?}{rev:?}")))
        {
            return;
        }
        for v in VALUES {
            let g = gauge_at(glyph, fmt, v, rev);
            // the value font beside VA's MODE scope is its own layout: the
            // band sets the text clear of the gauge (`va_glyph_test`)
            if components::layout(text, &g) == Layout::SmallBeside {
                continue;
            }
            let (shown, hits) = overlap(label, text, g);
            assert!(
                hits.is_empty(),
                "{what} {text:?} ({glyph:?}, v {v}): gauge on the value at {:?}",
                &hits[..hits.len().min(4)]
            );
            self.shown += usize::from(shown);
            self.checked += 1;
        }
    }
}

#[test]
fn no_gauge_touches_the_widest_value() {
    let mut c = Cases {
        seen: HashSet::new(),
        shown: 0,
        checked: 0,
    };
    // Every param's spec.
    for b in BlockRef::ALL {
        for s in b.specs() {
            let text = widest(s.fmt, Some(s));
            c.check(
                &format!("{b:?}.{}", s.ident),
                s.label,
                &text,
                s.glyph,
                s.fmt,
                ReverbType::Quad,
            );
        }
    }
    // Every page's cells, as each view formats them (matrix routes, the
    // page's own formats), under each reverb panel, filter and tape.
    let params = chimera_core::params::ParamSnapshot::default();
    let base = SlotCtx::read(&params, chimera_core::addr::Op::A);
    for rev in ReverbType::ALL {
        let ctx = SlotCtx { rev, ..base };
        for chain in ALL_CHAINS {
            for blk in chain.blocks {
                let defs: Vec<&BlockDef> = std::iter::once(blk.def)
                    .chain(blk.sub_pages.iter().copied())
                    .collect();
                for def in defs {
                    for i in 0..def.params.len() {
                        let v = view(def, i, &ctx);
                        let spec = v.addr().and_then(|a| a.spec());
                        let text = widest(v.fmt(), spec);
                        c.check(def.name, v.label(), &text, v.glyph(), v.fmt(), rev);
                    }
                }
            }
        }
    }
    // The reverb panel's own units: times, notes, beats, under each TYPE
    // and SYNC.
    for rev in ReverbType::ALL {
        for sync in 0..3 {
            for s in BlockRef::Reverb.specs() {
                let mut best = (0, String::new());
                for k in 0..=64 {
                    let mut p = ReverbParams::default();
                    p.set(ParamId(ReverbParams::TYPE.0), rev.index() as f32);
                    p.set(ReverbParams::SYNC, sync as f32);
                    p.set(s.id, k as f32 / 64.0);
                    for tempo in [Tempo(40.0), Tempo::DEFAULT, Tempo(300.0)] {
                        let t = reverb_panel::value_text(s.id, &p, tempo, FoldState::default());
                        let w =
                            chimera_core::ui::draw::text_width(&theme::FONT_FOCUS, t.as_str(), 0);
                        if w > best.0 {
                            best = (w, t.as_str().to_string());
                        }
                    }
                }
                c.check("reverb panel", s.label, &best.1, s.glyph, s.fmt, rev);
            }
        }
    }
    // The test must be able to fail: short values do get their gauge.
    assert!(
        c.shown > c.checked / 4,
        "{} of {} shown",
        c.shown,
        c.checked
    );
}

/// `gauge_left` is true: no gauge draws a pixel to its left.
#[test]
fn gauges_stay_right_of_gauge_left() {
    let gauges = |v: f32| {
        let mut all = vec![Gauge::Arc {
            value: v,
            bipolar: v > 0.5,
        }];
        all.push(Gauge::Switch { on: v });
        all.push(Gauge::LevelBar { value: v, ticks: 8 });
        all.push(Gauge::Crossfader { value: v });
        all.push(Gauge::Staff {
            step: v * 14.0 - 7.0,
            accidental: Accidental::Flat,
        });
        for g in [
            FocusGlyph::Wave,
            FocusGlyph::Composite(CompositeId::ChorusBraid),
            FocusGlyph::Composite(CompositeId::DelayRings),
            FocusGlyph::Composite(CompositeId::Reverb),
            FocusGlyph::Va(VaGlyph::Mode),
            FocusGlyph::Va(VaGlyph::Blend),
            FocusGlyph::Va(VaGlyph::Wave(chimera_core::ui::glyph::VaOsc::One)),
        ] {
            for rev in [ReverbType::Quad, ReverbType::Hall] {
                all.push(gauge_at(g, ValFmt::Int(7), v, rev));
            }
        }
        all
    };
    for k in 0..=20 {
        for g in gauges(k as f32 / 20.0) {
            let left = components::gauge_left(&g).unwrap();
            let fb = render(|d| components::draw_gauge(d, g));
            let min = (0..W * H)
                .filter(|&i| ink(&fb, i))
                .map(|i| (i % W) as i32)
                .min();
            assert!(
                min.is_none_or(|m| m >= left),
                "{g:?}: draws at {min:?}, left {left}"
            );
        }
    }
}

/// Every format's text at each swept step, in order of the value.
fn texts(fmt: ValFmt, spec: &ParamSpec) -> Vec<String> {
    (0..=STEPS)
        .map(|k| {
            let v = k as f32 / STEPS as f32;
            let v = match fmt {
                ValFmt::LogHz { .. } | ValFmt::Vowel => spec.denormalize(v),
                _ => v,
            };
            let mut buf = FmtBuf::new();
            fmt_val(&mut buf, v, fmt);
            buf.as_str().to_string()
        })
        .collect()
}

const ARC: Gauge = Gauge::Arc {
    value: 0.5,
    bipolar: false,
};

/// A value in Hz, kHz, ms or s keeps its arc at every step.
#[test]
fn timed_and_pitched_values_keep_their_arc() {
    let mut n = 0;
    for b in BlockRef::ALL {
        for s in b.specs().iter().filter(|s| s.glyph == FocusGlyph::Arc) {
            for t in texts(s.fmt, s) {
                if matches!(components::split_unit(&t).1, "Hz" | "kHz" | "ms" | "s") {
                    n += 1;
                    assert_eq!(
                        components::layout(&t, &ARC),
                        Layout::WithGauge,
                        "{b:?}.{}: {t:?} loses its arc",
                        s.ident
                    );
                }
            }
        }
    }
    assert!(n > 1000, "{n} unit values");
}

/// While a value turns the band changes layout at most down its fallback
/// once (large and gauge, large alone, small), never back and forth.
#[test]
fn the_layout_never_flickers_as_a_value_turns() {
    for b in BlockRef::ALL {
        for s in b.specs().iter().filter(|s| s.glyph == FocusGlyph::Arc) {
            let seq: Vec<Layout> = texts(s.fmt, s)
                .iter()
                .map(|t| components::layout(t, &ARC))
                .collect();
            let changes = seq.windows(2).filter(|w| w[0] != w[1]).count();
            assert!(changes <= 2, "{b:?}.{}: {changes} layout changes", s.ident);
        }
    }
}

/// Through `Renderer::gauge` and the real band: turn each encoder of a
/// formant's page detent by detent; the arc stays, and nothing but the
/// gauge's own colours stands right of `gauge_left` beside the value.
#[test]
fn the_rendered_page_keeps_its_arc_clear_of_the_value() {
    use chimera_core::dsp::algo::kinds::OpKind;
    use chimera_core::project::PartId;
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::block_registry::ALGO_FORMANT;
    use chimera_hal::{ALL_ENCODERS, EncoderId};
    let mut ui = stack_ui();
    load_init(&mut ui, chimera_core::params::EngineType::Algo);
    let sound = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound;
    sound.params.algo.try_set_kind(0, OpKind::Formant).unwrap();
    to_osc(&mut ui);
    to_osc_sub(&mut ui, &ALGO_FORMANT);
    let left = components::gauge_left(&ARC).unwrap() - components::GAUGE_GAP;
    let mut arcs = 0;
    for e in ALL_ENCODERS {
        let e: EncoderId = e;
        for step in 0..140 {
            feed(&mut ui, Input::turn(e, if step < 70 { 1 } else { -1 }));
            for _ in 0..6 {
                ui.update(UiTick::for_test());
            }
            let fb = render_ui(&ui);
            let arc = |x, y| matches!(fb.at(x, y), c if c == theme::ACCENT || c == theme::FAINT);
            let rows = theme::ARC_CY - theme::ARC_R..theme::ARC_CY + theme::ARC_R;
            let drawn = rows
                .clone()
                .any(|y| (left..theme::SCREEN_W).any(|x| arc(x, y)));
            arcs += usize::from(drawn);
            // Rows the value spans, clear of the label above and the hint below.
            for y in 62..theme::FOCUS_VALUE_Y {
                for x in left..theme::SCREEN_W {
                    let c = fb.at(x, y);
                    assert!(
                        c == theme::BG || arc(x, y),
                        "{e:?} step {step}: value ink at ({x}, {y}) right of the gauge's edge"
                    );
                }
            }
        }
    }
    assert!(arcs > 0, "no arc was ever drawn");
}
