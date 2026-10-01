//! The MOD node's pages (filter-routing spec § UI).

mod screen;

use chimera_core::addr::Op;
use chimera_core::dsp::modulator::law::Law;
use chimera_core::dsp::modulator::{EnvForm, EnvSpeed, EnvType, Func, LfoForm, LfoType};
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::{
    ALGO_CHAIN, ENV_2, ENV_3, ENV_SPEED, ENVELOPE, LFO, LFO_2, LFO_3, MOD_MATRIX,
};
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::view::{SlotCtx, View, is_dimmed, view};
use chimera_hal::EncoderId;
use screen::*;

fn labels(p: &ParamSnapshot, def: &chimera_core::ui::block_def::BlockDef) -> [&'static str; 6] {
    let ctx = SlotCtx::read(p, Op::A);
    core::array::from_fn(|i| view(def, i, &ctx).label())
}

#[test]
fn the_mod_node_is_mtx_with_its_sub_list() {
    let node = ALGO_CHAIN.blocks.last().unwrap();
    assert_eq!(node.def.id, MOD_MATRIX.id);
    assert_eq!(node.map, Some("MOD"));
    assert_eq!(node.sub_pages.first().unwrap().id, ENVELOPE.id);
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
    to_mod_sub(&mut ui, 1, &ENVELOPE);
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
    to_mod_sub(&mut ui, 1, &ENVELOPE);
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

#[test]
fn the_sub_list_is_mtx_then_e1_to_l3() {
    let node = ALGO_CHAIN.blocks.last().unwrap();
    let shorts: Vec<&str> = core::iter::once(node.def)
        .chain(node.sub_pages.iter().copied())
        .map(|d| d.short)
        .collect();
    assert_eq!(shorts, ["MTX", "E1", "E2", "E3", "SPD", "L1", "L2", "L3"]);
    assert_eq!((ENV_SPEED.id, LFO.id, LFO_2.id, LFO_3.id), (62, 12, 64, 65));
}

#[test]
fn lfo_pages_show_classic_or_func() {
    let mut p = ParamSnapshot::default();
    assert_eq!(
        labels(&p, &LFO),
        ["RATE", "SHAPE", "SYNC", "PHASE", "DEPTH", "TYPE"]
    );
    p.lfos[1].lfo_type = LfoType::Func;
    assert_eq!(
        labels(&p, &LFO_2),
        ["MODE", "RATE", "PHASE", "TILT", "FORM", "TYPE"]
    );
    p.lfos[1].func.lfo_form = LfoForm::Lfv;
    assert_eq!(
        labels(&p, &LFO_2),
        ["MODE", "RATE", "DELTA", "SLEW", "FORM", "TYPE"]
    );
    let ctx = SlotCtx::read(&p, Op::A);
    assert!(
        matches!(view(&LFO_2, 0, &ctx), View::Text { text: "LFO", .. }),
        "MODE is fixed"
    );
}

/// SPD: a type-B slot's column is dimmed and inert (ENV 3 is B).
#[test]
fn spd_dims_a_type_b_slot() {
    let s = Sound::init(EngineType::Algo);
    let ctx = SlotCtx::read(&s.params, Op::A);
    let dim: Vec<bool> = (0..6)
        .map(|i| is_dimmed(&view(&ENV_SPEED, i, &ctx), &s))
        .collect();
    assert_eq!(dim, [false, false, true, false, false, true]);
}

/// SPD: a type-B slot's cells read TYPE B (fixed, so dimmed); a type-A
/// slot's read its values.
#[test]
fn spd_reads_type_b_on_a_type_b_slot() {
    use chimera_core::addr::Blocks;
    let s = Sound::init(EngineType::Algo);
    let ctx = SlotCtx::read(&s.params, Op::A);
    for (i, label) in [(2, "E3 SPEED"), (5, "E3 HOLD")] {
        assert_eq!(
            view(&ENV_SPEED, i, &ctx),
            View::Text {
                label,
                text: "TYPE B"
            }
        );
    }
    let text = |i: usize| {
        let v = view(&ENV_SPEED, i, &ctx);
        let addr = v.addr().expect("E1 is a param");
        let mut b = FmtBuf::new();
        fmt_val(
            &mut b,
            s.params.block(addr.block).unwrap().get(addr.param) / 2.0,
            v.fmt(),
        );
        b.as_str().to_owned()
    };
    assert_eq!((text(0), text(3)), ("MED".to_owned(), "AHDSR".to_owned()));
}

/// FUNC's fixed MODE takes the focus but reads dimmed in the focus band as
/// in its cell: label and value in MID, no arc (#123).
#[test]
fn a_fixed_slot_is_dimmed_in_the_focus_band() {
    use chimera_core::ui::components::{self, Look};
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::theme;
    use embedded_graphics::pixelcolor::Rgb565;
    let mut ui = UiState::new();
    to_mod_sub(&mut ui, 5, &LFO);
    feed(&mut ui, Input::turn(EncoderId::F, 1)); // TYPE → FUNC
    feed(&mut ui, Input::turn(EncoderId::A, 1)); // MODE: focused, inert
    assert_eq!(ui.focused_slot(), 0);
    assert_eq!(ui.params().lfos[0].lfo_type, LfoType::Func);
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());

    let (y0, y1) = (theme::HEADER_BOTTOM, theme::FOCUS_BOTTOM);
    let band = |fb: &Fb| -> Vec<Rgb565> {
        (y0..y1)
            .flat_map(|y| (0..240).map(move |x| (x, y)))
            .map(|(x, y)| fb.at(x, y))
            .collect()
    };
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    let v = ui.renderer.anim[0].current();
    components::focus_band(
        &mut want,
        "MODE",
        "LFO",
        chimera_core::ui::glyph::Gauge::Arc {
            value: v,
            bipolar: false,
        },
        Look::Dimmed,
        None,
    );
    assert!(band(&fb) == band(&want), "the band is MODE LFO, dimmed");
    let inks = [theme::INK, theme::ACCENT];
    assert!(
        !band(&fb).iter().any(|c| inks.contains(c)),
        "no INK value, no arc"
    );
}

/// SPD's viz draws a column per ENV slot, so the knobs under a column
/// drive that slot: SPEED on the top row, HOLD on the bottom (hardware:
/// the knobs under E2 moved E1 HOLD and the inert E3 SPEED).
#[test]
fn spd_knobs_drive_the_column_above() {
    use chimera_core::dsp::modulator::HoldPos;
    use chimera_core::ui::perf::PerfStats;
    let column = |ui: &mut UiState, slot: i32| -> Vec<u16> {
        settle(ui);
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        let x0 = 16 + slot * 74;
        (44..150)
            .flat_map(|y| (x0..x0 + 60).map(move |x| (x, y)))
            .map(|(x, y)| fb.px[y as usize * W + x as usize])
            .collect()
    };
    let mut ui = UiState::new();
    to_mod_sub(&mut ui, 4, &ENV_SPEED);
    let e2 = column(&mut ui, 1);
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(
        ui.params().envelopes[1].speed,
        EnvSpeed::Slow,
        "B: E2 SPEED"
    );
    assert_ne!(column(&mut ui, 1), e2, "E2's pills move");
    let e2 = column(&mut ui, 1);
    feed(&mut ui, Input::turn(EncoderId::E, 1));
    assert_eq!(
        ui.params().envelopes[1].hold_pos,
        HoldPos::GateExt,
        "E: E2 HOLD"
    );
    assert_ne!(column(&mut ui, 1), e2, "E2's HOLD name changes");

    // E3 is type B: C and F are dimmed and inert, and its column faint.
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    feed(&mut ui, Input::turn(EncoderId::F, 1));
    let e3 = ui.params().envelopes[2];
    assert_eq!(
        (e3.speed, e3.hold_pos),
        (EnvSpeed::Med, HoldPos::Ahdsr),
        "inert"
    );
    ui.params_mut().envelopes[2].env_type = EnvType::A;
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    feed(&mut ui, Input::turn(EncoderId::F, -1));
    let e3 = ui.params().envelopes[2];
    assert_eq!(
        (e3.speed, e3.hold_pos),
        (EnvSpeed::Slow, HoldPos::Off),
        "C, F: E3 at A"
    );
}
