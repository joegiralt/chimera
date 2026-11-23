// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The LFO page's scope and glyphs (#649, ADR 0177): the scope draws one
//! cycle of the LFO with the newest voice's playhead; each control has a
//! glyph of what it does, keyed on quantised set values only, so nothing
//! redraws at idle and a dirty redraw equals a full one. CLASSIC RATE
//! reads Hz; CLASSIC PHASE dims under FREE and keeps its glyph; FUNC RATE
//! stops at the per-block cap, an old save above it clamped on load.
//!
//! `SCREEN_DUMP=<dir>` writes each page state (`lfo_*`) and each glyph
//! golden's box (`lfo_glyph_*`, at 3×).

#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::addr::BlockRef;
use chimera_core::dsp::lfo::{LFO_SPECS, LfoParams as L, LfoParams, LfoShape};
use chimera_core::dsp::modulator::law::{B_RATE, LFO_RATE, LFO_RISE_MAX, Law, block_rate_max};
use chimera_core::dsp::modulator::{LfoForm, LfoSlot, LfoType};
use chimera_core::instrument::{LfoAt, LfoView};
use chimera_core::lfo_glyph::draw::{curve, render};
use chimera_core::lfo_glyph::{COLS, Canvas, Glyph, H, Ink, LfoGauge, Q, W, Wave};
use chimera_core::storage::{RecordBuf, decode_block, encode_block};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::fmt::{FmtBuf, fmt_val};
use chimera_core::ui::glyph::FocusGlyph;
use chimera_core::ui::page::ValFmt;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::{UiState, theme};
use chimera_hal::EncoderId;
use screen::*;

// ── Fixtures ─────────────────────────────────────────────────────────

fn q(v: f32) -> Q {
    Q::of(v)
}

/// CLASSIC at `hz`.
fn classic(hz: f32, shape: LfoShape, retrig: bool, phase: f32, depth: f32) -> Wave {
    Wave::Classic {
        rate: q((hz - 0.01) / 19.99),
        shape,
        retrig,
        phase: q(phase),
        depth: q(depth),
    }
}

/// FUNC at `hz`.
fn func(hz: f32, form: LfoForm, fall: f32, tilt: f32) -> Wave {
    let knob = (hz / LFO_RATE.min).log2() / LFO_RATE.oct;
    Wave::Func {
        rate: q(knob),
        form,
        fall: q(fall),
        tilt: q(tilt),
    }
}

/// FNV-1a over a canvas's inks.
fn canvas_hash(c: &Canvas) -> u64 {
    c.0.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &i| {
        (h ^ i as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// A canvas at 3× as a PPM, to `$SCREEN_DUMP/<name>.ppm`.
fn dump_canvas(c: &Canvas, name: &str) {
    let Some(dir) = std::env::var_os("SCREEN_DUMP") else {
        return;
    };
    let mut out = format!("P6\n{} {}\n255\n", W * 3, H * 3).into_bytes();
    for y in 0..H * 3 {
        for x in 0..W * 3 {
            let col = chimera_core::ui::lfo_glyph::ink(c.at(x / 3, y / 3));
            use embedded_graphics::pixelcolor::RgbColor;
            out.extend([
                (col.r() << 3) | (col.r() >> 2),
                (col.g() << 2) | (col.g() >> 4),
                (col.b() << 3) | (col.b() >> 2),
            ]);
        }
    }
    std::fs::write(std::path::Path::new(&dir).join(format!("{name}.ppm")), out).unwrap();
}

fn glyph(glyph: Glyph, wave: Wave) -> Canvas {
    let mut c = Canvas::new();
    render(&LfoGauge { glyph, wave }, &mut c);
    c
}

/// The UI on LFO 1, its playhead read from its own view (never the
/// audio's: the tests run side by side), `set` applied to the slot.
fn lfo_ui(set: impl FnOnce(&mut LfoParams)) -> (UiState, &'static LfoView) {
    let mut ui = UiState::new();
    stack_map(&mut ui);
    let view: &'static LfoView = Box::leak(Box::new(LfoView::new()));
    ui.hear_lfo(view);
    to_mod_sub(&mut ui, 5, &reg::LFO);
    set(&mut ui.params_mut().lfos[0]);
    settle(&mut ui);
    (ui, view)
}

/// Focus cell `e` without moving it: a step there and back.
fn focus(ui: &mut UiState, e: EncoderId) {
    feed(ui, Input::turn(e, 1));
    feed(ui, Input::turn(e, -1));
    settle(ui);
}

/// The voice the audio would publish: part 1, slot 1, at `phase`.
fn voice_at(view: &LfoView, phase: Option<f32>) {
    view.publish(0, LfoAt { part: 0, phase });
}

fn full(ui: &UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    fb
}

/// A dirty render onto `fb`: the rows it flushed.
fn dirty(ui: &mut UiState, fb: &mut Fb) -> Vec<(u16, u16)> {
    ui.render_dirty_with_scope(fb, &PerfStats::zero(), &scope_fixture())
        .into_iter()
        .filter(|r| *r != (0, 0))
        .collect()
}

/// A full render then a primed dirty one, as the shells start.
fn primed(ui: &mut UiState) -> Fb {
    let mut fb = full(ui);
    ui.prime_regions(&PerfStats::zero(), None, &scope_fixture());
    assert!(dirty(ui, &mut fb).is_empty(), "primed");
    fb
}

// ── The specs and the laws ───────────────────────────────────────────

/// Every control on the page carries the LFO glyph; TYPE and OFST none.
#[test]
fn the_specs_carry_the_lfo_glyph() {
    for id in [
        L::RATE,
        L::SHAPE,
        L::SYNC,
        L::PHASE,
        L::DEPTH,
        L::FORM,
        L::RISE,
        L::FALL,
        L::SHAPE_B,
    ] {
        assert_eq!(
            LFO_SPECS[usize::from(id.0)].glyph,
            FocusGlyph::Lfo,
            "{id:?}"
        );
    }
    assert_ne!(LFO_SPECS[usize::from(L::TYPE.0)].glyph, FocusGlyph::Lfo);
    assert_ne!(LFO_SPECS[usize::from(L::OFFSET.0)].glyph, FocusGlyph::Lfo);
}

/// CLASSIC RATE reads Hz on its own linear curve, 0.01–20; under 1 Hz
/// with the places a semitone's turn needs (ADR 0279).
#[test]
fn classic_rate_reads_hz() {
    let at = |v: f32| {
        let mut b = FmtBuf::new();
        fmt_val(&mut b, v, ValFmt::Law(Law::LfoHz));
        b.as_str().to_owned()
    };
    assert_eq!(at(0.0), "0.0100 Hz");
    assert_eq!(at((0.5 - 0.01) / 19.99), "0.500 Hz");
    assert_eq!(at((1.0 - 0.01) / 19.99), "1.00 Hz");
    assert_eq!(at(1.0), "20.0 Hz");
}

/// FUNC RATE's knob ends where a per-block slot stops, 93.75 Hz at
/// 48 kHz, and reads it: no dead top.
#[test]
fn func_rate_stops_at_the_block_cap() {
    let cap = block_rate_max(48_000);
    assert!(
        (B_RATE.at(LFO_RISE_MAX) - cap).abs() < 0.05,
        "{}",
        B_RATE.at(LFO_RISE_MAX)
    );
    assert!((LFO_RATE.at(1.0) - cap).abs() < 0.05);
    assert_eq!(LFO_SPECS[usize::from(L::RISE.0)].max, LFO_RISE_MAX);
    // The same Hz below the cap, knob or stored position.
    for p in [0.0, 0.2, 0.309, 0.5, 0.7] {
        let knob = p / LFO_RISE_MAX;
        assert!((LFO_RATE.at(knob) - B_RATE.at(p)).abs() < 1e-3 * B_RATE.at(p).max(1.0));
    }
    let mut b = FmtBuf::new();
    fmt_val(&mut b, 1.0, ValFmt::Law(Law::LfoRate));
    assert_eq!(b.as_str(), "93.8 Hz");
}

/// A save from before the cap with FUNC RATE above it loads at the cap;
/// one below loads as saved.
#[test]
fn func_rate_above_the_cap_clamps_on_load() {
    let load = |rise: f32| {
        let mut l = LfoParams::default();
        l.func.rise = rise;
        let mut r = RecordBuf::new();
        encode_block(BlockRef::Lfo(LfoSlot::Lfo1), &l, None, &mut r);
        let mut snap = chimera_core::params::ParamSnapshot::default();
        decode_block(r.as_slice(), &[], &[], Some(&mut snap)).unwrap();
        snap.lfos[0].func.rise
    };
    assert_eq!(load(0.95), LFO_RISE_MAX);
    assert_eq!(load(1.0), LFO_RISE_MAX);
    assert_eq!(load(0.5), 0.5);
}

// ── The glyphs ───────────────────────────────────────────────────────

/// Each glyph at representative values, pinned. Re-record only for a
/// change meant to alter the pictures.
const GOLDENS: &[(&str, u64)] = &[
    ("rate_classic_1hz", 0x569c31c02f392d87),
    ("rate_classic_3hz_tri", 0xb2ef5304e86cd7d8),
    ("rate_classic_comb", 0x786923bb07493842),
    ("rate_classic_solid", 0xa916424a60aecab0),
    ("rate_func_slow", 0xd00a1016508fa0ac),
    ("rate_func_lfv", 0x18327c2f717cc4fc),
    ("rate_func_cap", 0xa916424a60aecab0),
    ("shape_sine", 0x240c46be05fe1f0c),
    ("shape_tri", 0x50c5a58ed9aaa338),
    ("shape_saw", 0x13f48bd31ce37ab1),
    ("shape_sqr", 0x965a51640e0bc4c1),
    ("shape_sh", 0xe0a73ebbc2c643aa),
    ("sync_free", 0x5e8e3ea8a05df946),
    ("sync_retrig", 0x7b5c292397478fd2),
    ("form_free", 0x9ea3fbdfab626bb3),
    ("form_sync", 0xa852fa9a5ccef25e),
    ("form_lfv", 0x66aae851fb2c679a),
    ("phase_classic_0", 0xb8ffeb07cfee5ed1),
    ("phase_classic_quarter", 0x0472c460e0ce5c18),
    ("phase_func_90", 0x33aef1b462a5b2cd),
    ("depth_full", 0x1ed3841175a1fa33),
    ("depth_half_tri", 0x05a9fa0a757b395a),
    ("tilt_saw", 0x6c65ee6f9475db62),
    ("tilt_tri", 0x276e5542f7ae6f08),
    ("tilt_ramp", 0x2a7e03bc3d924184),
    ("tilt_30", 0x759a53afd6df16be),
    ("delta_small", 0xfbeaf0418d01ac1f),
    ("delta_big", 0x6b17c44aec7289d6),
    ("slew_0", 0x76ecda1a85111b45),
    ("slew_half", 0xe2e91dec67652597),
];

fn golden_cases() -> Vec<(&'static str, Glyph, Wave)> {
    use Glyph::*;
    use LfoForm::*;
    use LfoShape::*;
    vec![
        (
            "rate_classic_1hz",
            Rate,
            classic(1.0, Sine, false, 0.0, 1.0),
        ),
        (
            "rate_classic_3hz_tri",
            Rate,
            classic(3.0, Triangle, false, 0.0, 1.0),
        ),
        (
            "rate_classic_comb",
            Rate,
            classic(9.0, Saw, false, 0.0, 1.0),
        ),
        (
            "rate_classic_solid",
            Rate,
            classic(20.0, Sine, false, 0.0, 1.0),
        ),
        ("rate_func_slow", Rate, func(0.3, Free, 0.0, 0.5)),
        ("rate_func_lfv", Rate, func(1.5, Lfv, 0.7, 0.3)),
        ("rate_func_cap", Rate, func(93.75, Free, 0.0, 0.5)),
        ("shape_sine", Shape, classic(1.0, Sine, false, 0.0, 1.0)),
        ("shape_tri", Shape, classic(1.0, Triangle, false, 0.0, 1.0)),
        ("shape_saw", Shape, classic(1.0, Saw, false, 0.0, 1.0)),
        ("shape_sqr", Shape, classic(1.0, Square, false, 0.0, 1.0)),
        ("shape_sh", Shape, classic(1.0, Random, false, 0.0, 1.0)),
        ("sync_free", Lane, classic(1.0, Sine, false, 0.25, 1.0)),
        ("sync_retrig", Lane, classic(1.0, Sine, true, 0.25, 1.0)),
        ("form_free", Lane, func(1.0, Free, 0.0, 0.5)),
        ("form_sync", Lane, func(1.0, Sync, 0.0, 0.5)),
        ("form_lfv", Lane, func(1.0, Lfv, 0.6, 0.3)),
        ("phase_classic_0", Phase, classic(1.0, Sine, true, 0.0, 1.0)),
        (
            "phase_classic_quarter",
            Phase,
            classic(1.0, Sine, true, 0.25, 1.0),
        ),
        ("phase_func_90", Phase, func(1.0, Sync, 0.25, 0.5)),
        ("depth_full", Depth, classic(1.0, Sine, false, 0.0, 1.0)),
        (
            "depth_half_tri",
            Depth,
            classic(1.0, Triangle, false, 0.0, 0.5),
        ),
        ("tilt_saw", Tilt, func(1.0, Free, 0.0, 0.0)),
        ("tilt_tri", Tilt, func(1.0, Free, 0.0, 0.5)),
        ("tilt_ramp", Tilt, func(1.0, Free, 0.0, 1.0)),
        ("tilt_30", Tilt, func(1.0, Free, 0.0, 0.3)),
        ("delta_small", Delta, func(1.0, Lfv, 0.25, 0.0)),
        ("delta_big", Delta, func(1.0, Lfv, 0.9, 0.2)),
        ("slew_0", Slew, func(1.0, Lfv, 0.6, 0.0)),
        ("slew_half", Slew, func(1.0, Lfv, 0.6, 0.5)),
    ]
}

#[test]
fn the_glyphs_match_their_goldens() {
    let mut got = Vec::new();
    for (name, g, w) in golden_cases() {
        let c = glyph(g, w);
        dump_canvas(&c, &format!("lfo_glyph_{name}"));
        got.push((name, canvas_hash(&c)));
    }
    let table: String = got
        .iter()
        .map(|(n, h)| format!("    (\"{n}\", {h:#018x}),\n"))
        .collect();
    assert_eq!(got.len(), GOLDENS.len(), "record:\n{table}");
    for ((n, h), (gn, gh)) in got.iter().zip(GOLDENS) {
        assert_eq!((n, h), (gn, gh), "record:\n{table}");
    }
}

/// Every glyph draws something, and a different picture per input: each
/// control's turn shows.
#[test]
fn each_glyph_moves_with_its_control() {
    let c = |g, w| canvas_hash(&glyph(g, w));
    let base = classic(1.0, LfoShape::Sine, true, 0.0, 1.0);
    assert_ne!(
        c(Glyph::Rate, base),
        c(Glyph::Rate, classic(2.0, LfoShape::Sine, true, 0.0, 1.0))
    );
    assert_ne!(
        c(Glyph::Phase, base),
        c(Glyph::Phase, classic(1.0, LfoShape::Sine, true, 0.3, 1.0))
    );
    assert_ne!(
        c(Glyph::Depth, base),
        c(Glyph::Depth, classic(1.0, LfoShape::Sine, true, 0.0, 0.4))
    );
    assert_ne!(
        c(Glyph::Lane, base),
        c(Glyph::Lane, classic(1.0, LfoShape::Sine, false, 0.0, 1.0))
    );
    let f = func(1.0, LfoForm::Free, 0.0, 0.5);
    assert_ne!(
        c(Glyph::Tilt, f),
        c(Glyph::Tilt, func(1.0, LfoForm::Free, 0.0, 0.2))
    );
    let l = func(1.0, LfoForm::Lfv, 0.6, 0.2);
    assert_ne!(
        c(Glyph::Delta, l),
        c(Glyph::Delta, func(1.0, LfoForm::Lfv, 0.3, 0.2))
    );
    assert_ne!(
        c(Glyph::Slew, l),
        c(Glyph::Slew, func(1.0, LfoForm::Lfv, 0.6, 0.6))
    );
    // RATE's turn never moves the others: their key leaves it out.
    for g in [Glyph::Shape, Glyph::Lane, Glyph::Phase, Glyph::Depth] {
        let a = LfoGauge {
            glyph: g,
            wave: base,
        };
        let b = LfoGauge {
            glyph: g,
            wave: classic(7.0, LfoShape::Sine, true, 0.0, 1.0),
        };
        assert_eq!(a.key(), b.key(), "{g:?}");
        assert_eq!(c(g, a.wave), c(g, b.wave), "{g:?}");
    }
    for g in Glyph::ALL {
        let w = if matches!(g, Glyph::Tilt | Glyph::Delta | Glyph::Slew) {
            l
        } else {
            base
        };
        assert!(glyph(g, w).0.iter().any(|&i| i != Ink::Bg), "{g:?} draws");
    }
}

/// The scope's curve: one cycle at the set values (S&H and LFV their
/// eight steps), up positive, full scale 24 px.
#[test]
fn the_scope_draws_one_cycle_of_the_lfo() {
    let mut ys = [0i8; COLS];
    curve(&classic(1.0, LfoShape::Sine, false, 0.0, 1.0), &mut ys);
    assert_eq!(ys[0], 0);
    assert_eq!(ys[(COLS - 1) / 4], 24, "the crest a quarter in");
    assert_eq!(ys[3 * (COLS - 1) / 4], -24);
    curve(&classic(1.0, LfoShape::Sine, false, 0.0, 0.5), &mut ys);
    assert_eq!(ys[(COLS - 1) / 4], 12, "DEPTH scales it");
    // RETRIG's PHASE shifts it; FREE's doesn't.
    let mut free = [0i8; COLS];
    curve(&classic(1.0, LfoShape::Saw, false, 0.5, 1.0), &mut free);
    curve(&classic(1.0, LfoShape::Saw, true, 0.5, 1.0), &mut ys);
    assert_eq!(free[0], -24);
    assert_eq!(ys[0], 0);
    // S&H: eight held steps.
    curve(&classic(1.0, LfoShape::Random, false, 0.0, 1.0), &mut ys);
    assert!(ys.chunk_by(|a, b| a == b).count() <= 8);
    // FUNC: TILT's rise share, PHASE added.
    curve(&func(1.0, LfoForm::Free, 0.0, 1.0), &mut ys);
    assert!(ys[0] < 0 && ys[COLS - 2] > 20, "RAMP rises the cycle");
}

// ── The page ─────────────────────────────────────────────────────────

/// The page's states shown, dumped (`lfo_*`).
#[test]
fn page_states_render() {
    // Each focuses its cell first (a step there and back), then sets.
    type Setup = Box<dyn Fn(&mut UiState, &LfoView)>;
    let shots: Vec<(&str, EncoderId, Setup)> = vec![
        ("lfo_classic_rate", EncoderId::A, Box::new(|_, _| {})),
        (
            "lfo_classic_shape_tri",
            EncoderId::B,
            Box::new(|ui, _| lfo(ui).shape = 1),
        ),
        (
            "lfo_classic_sync_retrig",
            EncoderId::C,
            Box::new(|ui, v| {
                (lfo(ui).sync, lfo(ui).phase_offset) = (1, 0.25);
                voice_at(v, Some(0.4));
            }),
        ),
        (
            "lfo_classic_phase_free_dimmed",
            EncoderId::D,
            Box::new(|ui, _| lfo(ui).phase_offset = 0.25),
        ),
        (
            "lfo_classic_depth",
            EncoderId::E,
            Box::new(|ui, v| {
                lfo(ui).depth = 0.6;
                voice_at(v, Some(0.3));
            }),
        ),
        (
            "lfo_func_rate",
            EncoderId::B,
            Box::new(|ui, v| {
                func_page(ui, LfoForm::Free);
                voice_at(v, Some(0.6));
            }),
        ),
        (
            "lfo_func_tilt",
            EncoderId::D,
            Box::new(|ui, _| {
                func_page(ui, LfoForm::Free);
                lfo(ui).func.shape = 0.3;
            }),
        ),
        (
            "lfo_func_form_sync",
            EncoderId::E,
            Box::new(|ui, _| func_page(ui, LfoForm::Sync)),
        ),
        (
            "lfo_func_lfv_delta",
            EncoderId::C,
            Box::new(|ui, v| {
                func_page(ui, LfoForm::Lfv);
                lfo(ui).func.fall = 0.6;
                voice_at(v, Some(0.5));
            }),
        ),
        (
            "lfo_func_lfv_slew",
            EncoderId::D,
            Box::new(|ui, _| {
                func_page(ui, LfoForm::Lfv);
                (lfo(ui).func.fall, lfo(ui).func.shape) = (0.7, 0.5);
            }),
        ),
    ];
    for (name, cell, setup) in shots {
        let (mut ui, view) = lfo_ui(|_| {});
        focus(&mut ui, cell);
        setup(&mut ui, view);
        settle(&mut ui);
        let fb = full(&ui);
        assert_eq!(fb.oob, 0, "{name}");
        fb.dump(name);
    }
}

fn lfo(ui: &mut UiState) -> &mut LfoParams {
    &mut ui.params_mut().lfos[0]
}

fn func_page(ui: &mut UiState, form: LfoForm) {
    let l = &mut ui.params_mut().lfos[0];
    l.lfo_type = LfoType::Func;
    l.func.lfo_form = form;
    settle(ui);
}

/// A way onto a page state.
type To = fn(&mut UiState);

fn tick(ui: &mut UiState) {
    ui.update(chimera_core::ui::animation::UiTick::for_test());
}

/// Every cell of both pages, voice or none: once drawn, the screen
/// redraws nothing while nothing moves (no frame term in any key).
#[test]
fn nothing_redraws_at_idle() {
    let pages: [(&str, To); 3] = [
        ("classic", |_| {}),
        ("func", |ui| func_page(ui, LfoForm::Free)),
        ("lfv", |ui| func_page(ui, LfoForm::Lfv)),
    ];
    for (page, to) in pages {
        for cell in [
            EncoderId::A,
            EncoderId::B,
            EncoderId::C,
            EncoderId::D,
            EncoderId::E,
            EncoderId::F,
        ] {
            for phase in [None, Some(0.3)] {
                let (mut ui, view) = lfo_ui(|_| {});
                to(&mut ui);
                focus(&mut ui, cell);
                voice_at(view, phase);
                settle(&mut ui);
                let mut fb = primed(&mut ui);
                for _ in 0..40 {
                    tick(&mut ui);
                    assert!(
                        dirty(&mut ui, &mut fb).is_empty(),
                        "{page} {cell:?} {phase:?}"
                    );
                }
            }
        }
    }
}

/// A knob turned and easing, frame by frame: the dirty screen is the
/// full one at every frame.
#[test]
fn a_dirty_redraw_equals_a_full_one_while_a_knob_eases() {
    let turns: [(To, EncoderId, i8); 6] = [
        (|_| {}, EncoderId::A, 20),
        (|ui| lfo(ui).sync = 1, EncoderId::D, 30),
        (|_| {}, EncoderId::E, -40),
        (|ui| func_page(ui, LfoForm::Free), EncoderId::D, -25),
        (|ui| func_page(ui, LfoForm::Sync), EncoderId::C, 30),
        (|ui| func_page(ui, LfoForm::Lfv), EncoderId::C, 40),
    ];
    for (i, (to, cell, by)) in turns.into_iter().enumerate() {
        let (mut ui, view) = lfo_ui(|_| {});
        to(&mut ui);
        voice_at(view, Some(0.2));
        settle(&mut ui);
        let mut fb = primed(&mut ui);
        feed(&mut ui, Input::turn(cell, by));
        for frame in 0..30 {
            tick(&mut ui);
            dirty(&mut ui, &mut fb);
            assert!(fb.px == full(&ui).px, "turn {i}, frame {frame}");
        }
    }
}

/// The playhead moving redraws its two boxes alone, a few rows, and the
/// screen is the full render's; with no voice it parks and stops.
#[test]
fn the_playhead_redraws_its_boxes_alone() {
    for (to, shape) in [
        (None, LfoShape::Sine),
        (None, LfoShape::Random),
        (Some(LfoForm::Free), LfoShape::Sine),
        (Some(LfoForm::Lfv), LfoShape::Sine),
    ] {
        let (mut ui, view) = lfo_ui(|l| l.shape = shape as u8);
        if let Some(form) = to {
            func_page(&mut ui, form);
        }
        let mut fb = primed(&mut ui);
        for k in 0..60 {
            voice_at(view, Some((k as f32 * 0.07) % 1.0));
            tick(&mut ui);
            let rows = dirty(&mut ui, &mut fb);
            assert!(fb.px == full(&ui).px, "{to:?} {shape:?} step {k}");
            let n: u16 = rows.iter().map(|r| r.1 - r.0).sum();
            assert!(n <= 18, "{n} rows: only the dot's boxes");
        }
        voice_at(view, None);
        tick(&mut ui);
        dirty(&mut ui, &mut fb);
        assert!(fb.px == full(&ui).px, "parked");
        tick(&mut ui);
        assert!(dirty(&mut ui, &mut fb).is_empty(), "parked: nothing moves");
    }
}

/// CLASSIC PHASE under FREE reads dimmed and keeps its glyph: the box is
/// RETRIG's pixel for pixel; only the value's ink differs.
#[test]
fn dimmed_phase_keeps_its_glyph() {
    let shot = |sync: u8| {
        let (mut ui, _) = lfo_ui(|_| {});
        lfo(&mut ui).sync = 1;
        settle(&mut ui);
        focus(&mut ui, EncoderId::D);
        (lfo(&mut ui).sync, lfo(&mut ui).phase_offset) = (sync, 0.3);
        settle(&mut ui);
        full(&ui)
    };
    let (free, retrig) = (shot(0), shot(1));
    let boxed = |fb: &Fb| {
        let mut px = Vec::new();
        for y in theme::WAVE_Y..theme::WAVE_Y + H {
            for x in theme::WAVE_X..theme::WAVE_X + W {
                px.push(fb.at(x, y));
            }
        }
        px
    };
    assert_eq!(boxed(&free), boxed(&retrig), "the glyph as it was");
    assert!(boxed(&free).contains(&theme::ACCENT), "a glyph is drawn");
    let value = |fb: &Fb| {
        (theme::FOCUS_VALUE_X..theme::WAVE_X - 8)
            .flat_map(|x| (60..theme::FOCUS_VALUE_Y).map(move |y| (x, y)))
            .any(|(x, y)| fb.at(x, y) == theme::INK)
    };
    assert!(!value(&free), "FREE's value reads in MID");
    assert!(value(&retrig), "RETRIG's in INK");
}
