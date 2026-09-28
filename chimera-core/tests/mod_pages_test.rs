//! The MOD node's pages (filter-routing spec § UI).

mod screen;

use chimera_core::addr::Op;
use chimera_core::dsp::modulator::law::Law;
use chimera_core::dsp::modulator::{EnvForm, EnvSpeed, EnvType, Func, LfoForm};
use chimera_core::params::ParamSnapshot;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{ALGO_CHAIN, ENV_2, ENV_3, ENVELOPE, MOD_MATRIX};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::view::{SlotCtx, view};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

fn labels(p: &ParamSnapshot, def: &chimera_core::ui::block_def::BlockDef) -> [&'static str; 6] {
    let ctx = SlotCtx::read(p, Op::A);
    core::array::from_fn(|i| view(def, i, &ctx).label())
}

#[test]
fn the_mod_node_is_e1_with_its_sub_list() {
    let node = ALGO_CHAIN.blocks.last().unwrap();
    assert_eq!(node.def.id, ENVELOPE.id);
    assert_eq!(node.map, Some("MOD"));
    let subs: Vec<&str> = core::iter::once(node.def)
        .chain(node.sub_pages.iter().copied())
        .map(|d| d.short)
        .collect();
    assert_eq!(subs.first(), Some(&"E1"));
    assert_eq!(subs.last(), Some(&"MTX"));
    assert_eq!(node.sub_pages.last().unwrap().id, MOD_MATRIX.id);
    assert_eq!((ENV_2.id, ENV_3.id), (60, 61));
}

#[test]
fn an_env_page_shows_its_types_panel() {
    let mut p = ParamSnapshot::default();
    assert_eq!(
        labels(&p, &ENVELOPE),
        ["ATTACK", "DECAY", "SUSTAIN", "RELEASE", "HOLD", "TYPE"]
    );
    assert_eq!(
        labels(&p, &ENV_3),
        ["MODE", "RISE", "FALL", "SHAPE", "FORM", "TYPE"]
    );
    for (f, want) in [
        (
            Func::Lfo(LfoForm::Free),
            ["MODE", "RATE", "PHASE", "TILT", "FORM", "TYPE"],
        ),
        (
            Func::Lfo(LfoForm::Lfv),
            ["MODE", "RATE", "DELTA", "SLEW", "FORM", "TYPE"],
        ),
        (
            Func::Burst(EnvForm::Ad),
            ["MODE", "RATE", "LENGTH", "TILT", "FORM", "TYPE"],
        ),
    ] {
        p.envelopes[2].func.set_func(f);
        assert_eq!(labels(&p, &ENV_3), want, "{f:?}");
    }
    p.envelopes[0].env_type = EnvType::B;
    assert_eq!(labels(&p, &ENVELOPE)[0], "MODE", "any ENV slot can be B");
}

/// Times and rates read in their units (the approved mockups).
#[test]
fn slider_cells_read_in_units() {
    let p = ParamSnapshot::default();
    let ctx = SlotCtx::read(&p, Op::A);
    let text = |def, i, v| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, v, view(def, i, &ctx).fmt());
        b.as_str().to_owned()
    };
    assert_eq!(
        text(&ENVELOPE, 0, 0.189),
        "10 ms",
        "MED attack at its default"
    );
    assert_eq!(text(&ENVELOPE, 1, 1.0), "10.0 s");
    assert_eq!(text(&ENVELOPE, 2, 0.55), "55%");
    assert_eq!(
        text(&ENVELOPE, 4, 0.0),
        "0.0 ms",
        "the integer formatter pads"
    );
    assert_eq!(text(&ENV_3, 3, 0.5), "LIN");
    assert_eq!(text(&ENV_3, 3, 0.8), "EXP 60");

    // The unit follows the value as printed: just under a boundary rounds
    // up into the next unit, never "1000 ms" or "10.00 Hz".
    let hold = Law::Hold(EnvSpeed::Med);
    for (law, lo, hi, want) in [
        (hold, 0.9995, 1.0, "1.0 s"),
        (hold, 0.00995, 0.01, "10 ms"),
        (Law::BRate, 9.995, 10.0, "10.0 Hz"),
        (Law::BRate, 99.95, 100.0, "100 Hz"),
    ] {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, position_in(law, lo, hi), ValFmt::Law(law));
        assert_eq!(b.as_str(), want, "{law:?} in [{lo}, {hi})");
    }
}

/// A slider position whose quantity lies in `[lo, hi)`, by bisection.
fn position_in(law: Law, lo: f32, hi: f32) -> f32 {
    let r = law.range().unwrap();
    let (mut a, mut b) = (0.0f32, 1.0f32);
    for _ in 0..60 {
        let m = 0.5 * (a + b);
        if r.at(m) < 0.5 * (lo + hi) {
            a = m
        } else {
            b = m
        }
    }
    let q = r.at(b);
    assert!((lo..hi).contains(&q), "{law:?}: {q} not in [{lo}, {hi})");
    b
}

/// A TYPE flip redraws the header, whose title reads `ENV 1 / B` now.
#[test]
fn a_type_flip_redraws_the_title() {
    use chimera_core::ui::page::PageLayout;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::{RegionKind, layout_regions};
    let mut ui = UiState::new();
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    let (mut fb, perf, scope) = (Fb::new(), PerfStats::zero(), scope_fixture());
    ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    feed(&mut ui, Input::turn(EncoderId::F, 1)); // TYPE → B
    let flushed = ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
    let &(_, y0, y1) = layout_regions(PageLayout::BigViz)
        .iter()
        .find(|r| r.0 == RegionKind::Header)
        .unwrap();
    assert!(flushed.contains(&(y0, y1)), "{flushed:?}");
}

/// A TYPE change re-seeds the animators: the cells jump to the new
/// panel's values instead of sweeping from the old ones.
#[test]
fn a_type_change_reseeds_the_page() {
    let mut ui = UiState::new();
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    feed(&mut ui, Input::turn(EncoderId::F, 1)); // TYPE → B
    ui.update();
    let rise = ui.params().envelopes[0].func.rise;
    assert_eq!(ui.renderer.anim[1].current(), rise, "RISE shown at once");
}

/// FORM reads its MODE's list: LFO's forms are not ENV's (spec § 1).
#[test]
fn form_reads_its_modes_names() {
    let mut p = ParamSnapshot::default();
    for (f, want) in [
        (Func::Env(EnvForm::Ahr), "AHR"),
        (Func::Lfo(LfoForm::Sync), "SYNC"),
        (Func::Lfo(LfoForm::Lfv), "LFV"),
        (Func::Burst(EnvForm::Cycle), "CYCLE"),
    ] {
        p.envelopes[2].func.set_func(f);
        let ctx = SlotCtx::read(&p, Op::A);
        let mut b = FmtBuf::new();
        let v = p.envelopes[2].func.form_index() / 2.0;
        fmt_val(&mut b, v, view(&ENV_3, 4, &ctx).fmt());
        assert_eq!(b.as_str(), want, "{f:?}");
    }
}
