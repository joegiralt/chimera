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
use chimera_core::ui::components::{self, Look};
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
    let value = render(|d| components::focus_band(d, label, text, Gauge::None, Look::Live, None));
    let both = render(|d| components::focus_band(d, label, text, g, Look::Live, None));
    let shown = value.px != both.px;
    let alone = render(|d| components::draw_gauge(d, g));
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
        let w = chimera_core::ui::draw::text_width(&theme::FONT_FOCUS, buf.as_str(), 0);
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
            let (shown, hits) = overlap(label, text, gauge_at(glyph, fmt, v, rev));
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
