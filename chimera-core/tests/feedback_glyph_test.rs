//! FEEDBACK's glyph (ADR 0190): a low-pass curve with a loop from its
//! output back to the cutoff corner. Still, keyed on the set value in
//! eighths: nothing redraws at idle and a dirty redraw is the full one.
//!
//! `SCREEN_DUMP=<dir>` writes the five values (`fb_m100` .. `fb_p100`).

mod common;
mod screen;

use chimera_core::block::ValFmt;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::params::{FILTER_SPECS, FilterParams};
use chimera_core::ui::glyph::{FEEDBACK_STEPS, FocusGlyph, Gauge, gauge};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::{UiState, theme};
use chimera_hal::EncoderId;
use screen::*;

fn q(v: f32) -> i8 {
    match gauge(FocusGlyph::Feedback, v, ValFmt::Bi, |_| Gauge::None) {
        Gauge::Feedback { q } => q,
        g => panic!("{g:?}"),
    }
}

#[test]
fn feedback_carries_its_glyph_and_name() {
    let s = FILTER_SPECS
        .iter()
        .find(|s| s.id == FilterParams::FB)
        .unwrap();
    assert_eq!((s.glyph, s.label), (FocusGlyph::Feedback, "FEEDBACK"));
}

#[test]
fn the_glyph_is_still_and_quantised() {
    assert_eq!(
        [0.0, 0.25, 0.5, 0.75, 1.0].map(q),
        [-8, -4, 0, 4, 8].map(|n| n * (FEEDBACK_STEPS as i8) / 8)
    );
    assert_eq!((q(-1.0), q(2.0)), (-8, 8));
    let g = Gauge::Feedback { q: 3 };
    assert!(!g.animates());
    assert_eq!(g.outside_key(), 0);
    assert_eq!(chimera_core::ui::components::gauge_rect(&g), None);
}

fn on_fb(v: i8) -> UiState {
    let mut ui = UiState::new();
    stack_map(&mut ui);
    on_flt(&mut ui, FilterKind::K2040, true);
    // A step first, so the cell has focus, then the rest.
    feed(&mut ui, Input::turn(EncoderId::D, 1));
    feed(&mut ui, Input::turn(EncoderId::D, v - 1));
    settle(&mut ui);
    ui
}

fn box_px(fb: &Fb) -> Vec<u16> {
    (theme::WAVE_Y..theme::WAVE_Y + theme::WAVE_H)
        .flat_map(|y| {
            (theme::WAVE_X..theme::WAVE_X + theme::WAVE_W)
                .map(move |x| fb.px[y as usize * W + x as usize])
        })
        .collect()
}

const VALUES: [(&str, i8); 5] = [
    ("fb_m100", -64),
    ("fb_m050", -32),
    ("fb_000", 0),
    ("fb_p050", 32),
    ("fb_p100", 64),
];

const GOLDENS: &[(&str, u64)] = &[
    ("fb_m100", 0xf0182884699bda84),
    ("fb_m050", 0xdeb714860c4c07a1),
    ("fb_000", 0x67b0160ec5162c13),
    ("fb_p050", 0x19b02f7277c38d1b),
    ("fb_p100", 0x00b6868725ec909f),
];

#[test]
fn the_five_values_draw_their_pictures() {
    let mut got = Vec::new();
    for (name, v) in VALUES {
        let fb = render_ui(&on_fb(v));
        assert_eq!(fb.oob, 0, "{name}");
        fb.dump(name);
        let mut h = 0xcbf29ce484222325u64;
        for px in box_px(&fb) {
            h = (h ^ px as u64).wrapping_mul(0x100000001b3);
        }
        got.push((name, h));
    }
    let hs: Vec<_> = got.iter().map(|g| g.1).collect();
    assert!(hs.iter().enumerate().all(|(i, h)| !hs[..i].contains(h)));
    common::golden::check(GOLDENS, &got);
}

fn full(ui: &UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb
}

fn dirty(ui: &mut UiState, fb: &mut Fb) -> Vec<(u16, u16)> {
    ui.render_dirty_with_scope(fb, &PerfStats::zero(), &scope_fixture())
        .into_iter()
        .filter(|r| *r != (0, 0))
        .collect()
}

fn tick(ui: &mut UiState) {
    ui.update(chimera_core::ui::animation::UiTick::for_test());
}

#[test]
fn nothing_redraws_at_idle() {
    for (name, v) in VALUES {
        let mut ui = on_fb(v);
        let mut fb = full(&ui);
        ui.prime_regions(&PerfStats::zero(), None, &scope_fixture());
        assert!(dirty(&mut ui, &mut fb).is_empty(), "{name} primed");
        for _ in 0..40 {
            tick(&mut ui);
            assert!(dirty(&mut ui, &mut fb).is_empty(), "{name}");
        }
    }
}

#[test]
fn a_dirty_redraw_equals_a_full_one_while_it_eases() {
    for by in [64i8, -40, -64] {
        let mut ui = on_fb(0);
        let mut fb = full(&ui);
        ui.prime_regions(&PerfStats::zero(), None, &scope_fixture());
        feed(&mut ui, Input::turn(EncoderId::D, by));
        for frame in 0..30 {
            tick(&mut ui);
            dirty(&mut ui, &mut fb);
            assert!(fb.px == full(&ui).px, "{by} frame {frame}");
        }
    }
}
