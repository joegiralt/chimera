//! Focus glyphs: each param's hand-assigned gauge, the focus band's
//! dispatch, the UI clock and the Demo chain's glyph pages.

mod screen;

use chimera_core::addr::{BlockRef, Blocks, Op, ParamAddr};
use chimera_core::block::{ParamSpec, ValFmt};
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::lfo::LfoParams;
use chimera_core::dsp::modal::{MODEL_NAMES, ModalParams};
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::modulation::{CUTOFF, ModSource};
use chimera_core::params::OutParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::{BlockDef, slot_addr};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::glyph::{CompositeId, FocusGlyph, Gauge, anim_key};
use chimera_core::ui::page::{PageId, PageKey, PageLayout};
use chimera_core::ui::region::{RegionData, RegionKind};
use chimera_core::ui::renderer::composite_set;
use chimera_core::ui::view::{SlotCtx, View, view};
use chimera_core::ui::{draw, theme};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const NONE: FocusGlyph = FocusGlyph::None;
const SWITCH: FocusGlyph = FocusGlyph::Switch;

/// Params assigned a glyph other than ARC, by `(block kind, ident)`: every
/// instance of the block (`AlgoOp`, `Env`, `Lfo`) alike. Each glyph story
/// adds its rows here.
const ASSIGNED: &[(&str, &str, FocusGlyph)] = &[
    // Word choices: the word is the value, there is no amount to gauge.
    ("Modal", "MODE", NONE),
    ("Algo", "ALG_A", NONE),
    ("Algo", "ALG_B", NONE),
    ("AlgoOp", "WAVE", NONE),
    ("Filter", "KIND", NONE),
    ("Filter", "MODE", NONE),
    ("Env", "TYPE", NONE),
    ("Env", "SPEED", NONE),
    ("Env", "HOLD", NONE),
    ("Env", "MODE", NONE),
    ("Env", "FORM", NONE),
    ("Env", "ENV_FORM", NONE),
    ("Env", "LFO_FORM", NONE),
    ("Env", "BRST_FORM", NONE),
    ("Lfo", "TYPE", NONE),
    ("Lfo", "FORM", NONE),
    ("Lfo", "SHAPE", NONE),
    ("Chorus", "MODE", NONE),
    // Two states, one of them off: a toggle.
    ("Lfo", "SYNC", SWITCH),
    ("Pitch", "STEAL", NONE),
    ("Part", "MODE", NONE),
    ("Part", "OUT", NONE),
    ("Theme", "GAMMA", NONE),
    ("Theme", "ACCENT", NONE),
];

/// Named choices that keep the ARC: their names are numbers on a scale,
/// so the arc still says where in the range the value sits.
const NAMED_ARC: &[(&str, &str)] = &[
    ("AlgoOp", "CRSE"),
    ("Modal", "MODES"),
    ("Comp", "RATIO"),
    ("Theme", "BRIGHT"),
    ("Theme", "BLACK"),
];

/// Two-value choices that are two peers, not on and off: a toggle would
/// read one of them as "off". They show the word (NONE).
const TWO_PEERS: &[(&str, &str)] = &[
    // A and B are two envelope designs.
    ("Env", "TYPE"),
    // CLASSIC and FUNC are two LFO designs.
    ("Lfo", "TYPE"),
    // MONO and POLY: neither is off.
    ("Part", "MODE"),
    // CUT and GLIDE: two ways to steal; GLIDE could read as "glide on".
    ("Pitch", "STEAL"),
];

/// Least gap between a SWITCH word and the pill.
const SWITCH_GAP: i32 = 6;

/// `Env` for `Env(Env1)`: the block's kind, every instance alike.
fn kind(b: BlockRef) -> String {
    format!("{b:?}").split('(').next().unwrap().to_string()
}

#[test]
fn every_param_is_arc_unless_assigned() {
    for b in BlockRef::ALL {
        for s in b.specs() {
            let want = ASSIGNED
                .iter()
                .find(|(k, id, _)| *k == kind(b) && *id == s.ident)
                .map_or(FocusGlyph::Arc, |a| a.2);
            assert_eq!(s.glyph, want, "{b:?}.{}", s.ident);
        }
    }
}

/// Every named choice is decided: NONE, or listed as keeping the arc.
#[test]
fn every_named_choice_is_decided() {
    for b in BlockRef::ALL {
        for s in b.specs() {
            if !matches!(s.fmt, ValFmt::Names(_)) {
                continue;
            }
            let id = (kind(b), s.ident);
            let none = ASSIGNED.iter().any(|a| (a.0, a.1) == (id.0.as_str(), id.1));
            let arc = NAMED_ARC
                .iter()
                .any(|a| (a.0, a.1) == (id.0.as_str(), id.1));
            assert!(none ^ arc, "{b:?}.{}: NONE or NAMED_ARC, once", s.ident);
        }
    }
}

/// Every two-value named choice is a toggle (SWITCH) or two peers (NONE).
#[test]
fn every_two_state_choice_is_decided() {
    for b in BlockRef::ALL {
        for s in b.specs() {
            let ValFmt::Names(names) = s.fmt else {
                continue;
            };
            if names.len() != 2 {
                continue;
            }
            let k = kind(b);
            let peers = TWO_PEERS
                .iter()
                .any(|p| (p.0, p.1) == (k.as_str(), s.ident));
            assert!(
                (s.glyph == FocusGlyph::Switch) ^ peers,
                "{b:?}.{}: SWITCH or TWO_PEERS, once",
                s.ident
            );
            if peers {
                assert_eq!(s.glyph, FocusGlyph::None, "{b:?}.{}", s.ident);
            }
        }
    }
}

/// Every word of a SWITCH param fits before the pill at the focus size.
#[test]
fn every_switch_word_fits_beside_the_pill() {
    let room = theme::SWITCH_CX - theme::SWITCH_W / 2 - SWITCH_GAP - theme::FOCUS_VALUE_X;
    for b in BlockRef::ALL {
        for s in b.specs().iter().filter(|s| s.glyph == FocusGlyph::Switch) {
            let ValFmt::Names(names) = s.fmt else {
                panic!("{b:?}.{}: SWITCH on a value that isn't named", s.ident);
            };
            assert_eq!(names.len(), 2, "{b:?}.{}: two states", s.ident);
            for w in names {
                let width = draw::text_width(&theme::FONT_FOCUS, w, 0);
                assert!(width <= room, "{b:?}.{} {w}: {width} > {room}", s.ident);
            }
        }
    }
}

/// Values the spec names read as words everywhere, as their panels show
/// them; names are display only (disk codes are pinned elsewhere).
#[test]
fn chorus_mode_and_lfo_shape_and_sync_are_named() {
    use chimera_core::dsp::chorus::ChorusParams;
    let names = |b: BlockRef, id| match ParamAddr::new(b, id).spec().unwrap().fmt {
        ValFmt::Names(n) => n.to_vec(),
        f => panic!("{b:?}: {f:?}"),
    };
    assert_eq!(
        names(BlockRef::Chorus, ChorusParams::MODE),
        ["OFF", "I", "II", "I+II"]
    );
    let lfo = BlockRef::Lfo(LfoSlot::Lfo1);
    assert_eq!(
        names(lfo, LfoParams::SHAPE),
        ["SINE", "TRI", "SAW", "SQR", "S&H"]
    );
    assert_eq!(names(lfo, LfoParams::SYNC), ["FREE", "RETRIG"]);
}

/// Every word of a NONE param fits the whole band at the focus size.
#[test]
fn every_none_word_fits_the_band() {
    let room = theme::SCREEN_W - theme::MARGIN_X - theme::FOCUS_VALUE_X;
    for b in BlockRef::ALL {
        for s in b.specs().iter().filter(|s| s.glyph == FocusGlyph::None) {
            let ValFmt::Names(names) = s.fmt else {
                panic!("{b:?}.{}: NONE on a value that isn't a word", s.ident);
            };
            for w in names {
                let width = draw::text_width(&theme::FONT_FOCUS, w, 0);
                assert!(width <= room, "{b:?}.{} {w}: {width} > {room}", s.ident);
            }
        }
    }
}

#[test]
fn glyph_is_hand_assigned_on_the_spec() {
    let s = ParamSpec::continuous(0, "X", ValFmt::Uni, 0.0, 1.0, 0.0, 0.1, false);
    assert_eq!(s.glyph, FocusGlyph::Arc);
    let s = s.glyph(FocusGlyph::Composite(CompositeId::ReverbCube));
    assert_eq!(s.glyph, FocusGlyph::Composite(CompositeId::ReverbCube));
    assert_eq!(FocusGlyph::default(), FocusGlyph::Arc);
}

#[test]
fn unbuilt_glyphs_draw_as_arc() {
    for g in FocusGlyph::ALL {
        let want = match g {
            FocusGlyph::None => Gauge::None,
            FocusGlyph::Switch => Gauge::Switch { on: 0.25 },
            FocusGlyph::LevelBar => Gauge::LevelBar {
                value: 0.25,
                ticks: 8,
            },
            _ => Gauge::Arc {
                value: 0.25,
                bipolar: true,
            },
        };
        assert_eq!(g.gauge(0.25, ValFmt::Bi), want, "{g:?}");
    }
}

/// A level bar's ticks: one per step for a few steps, else 8.
#[test]
fn level_bar_ticks_follow_the_steps() {
    let ticks = |fmt| match FocusGlyph::LevelBar.gauge(0.5, fmt) {
        Gauge::LevelBar { ticks, .. } => ticks,
        g => panic!("{g:?}"),
    };
    assert_eq!(ticks(ValFmt::Uni), 8);
    assert_eq!(ticks(ValFmt::Names(&["16", "24", "32", "48"])), 4);
    assert_eq!(ticks(ValFmt::Int(4)), 5);
    assert_eq!(ticks(ValFmt::Int(8)), 9);
    assert_eq!(ticks(ValFmt::Int(15)), 8);
    assert!(
        !Gauge::LevelBar {
            value: 1.0,
            ticks: 8
        }
        .animates()
    );
}

#[test]
fn a_view_reads_its_spec_glyph() {
    let addr = ParamAddr::new(BlockRef::Out, OutParams::PAN);
    let v = View::Param {
        addr,
        label: "PAN",
        fmt: ValFmt::Pan,
    };
    assert_eq!(v.glyph(), addr.spec().unwrap().glyph);
    let legacy = View::Legacy {
        label: "X",
        fmt: ValFmt::Uni,
    };
    assert_eq!(legacy.glyph(), FocusGlyph::Arc);
}

#[test]
fn anim_key_follows_the_clock_only_while_animating() {
    assert_eq!(anim_key(false, 7), 0);
    assert_eq!(anim_key(false, 8), 0);
    assert_ne!(anim_key(true, 7), anim_key(true, 8));
    let arc = Gauge::Arc {
        value: 0.5,
        bipolar: false,
    };
    assert!(!arc.animates());
    assert!(!Gauge::None.animates());
    assert!(!Gauge::Switch { on: 1.0 }.animates());
}

#[test]
fn the_clock_ticks_once_a_frame() {
    let mut ui = UiState::new();
    let t = ui.clock().frame();
    ui.update();
    ui.update();
    assert_eq!(ui.clock().frame(), t.wrapping_add(2));
}

fn render_dirty_ui(ui: &mut UiState) {
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(
        &mut fb,
        &chimera_core::ui::perf::PerfStats::zero(),
        &scope_fixture(),
    );
}

#[test]
fn an_arc_focus_band_does_not_redraw_on_the_clock() {
    let mut ui = UiState::new();
    settle(&mut ui);
    render_dirty_ui(&mut ui);
    let before = ui.drawn_key(RegionKind::Focus);
    for _ in 0..5 {
        ui.update();
    }
    render_dirty_ui(&mut ui);
    assert_eq!(ui.drawn_key(RegionKind::Focus), before);
}

/// A composite's set values are the stored ones: an LFO on the param moves
/// its cell, never what `composite_set` returns.
#[test]
fn composite_set_ignores_modulation() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(ButtonId::B2));
    feed(&mut ui, Input::press(ButtonId::B1));
    let chain = ui.nav.active_chain();
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    let (node, slot) = (0..chain.len())
        .find_map(|n| {
            let def = chain.active_def(n, 0)?;
            (0..6)
                .find(|&i| slot_addr(def, i, &ctx) == Some(CUTOFF))
                .map(|i| (n, i))
        })
        .expect("a page with CUTOFF");
    for _ in 0..node {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    // Off the top of its range, so the LFO swings both ways.
    let enc = [
        EncoderId::A,
        EncoderId::B,
        EncoderId::C,
        EncoderId::D,
        EncoderId::E,
        EncoderId::F,
    ];
    feed(&mut ui, Input::turn(enc[slot], -40));
    let m = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.mod_state;
    let d = m.push(CUTOFF).unwrap();
    m.set_route(ModSource::Lfo1.index(), d, 127);

    let stored = ui
        .params()
        .block(BlockRef::Filter)
        .unwrap()
        .normalized(CUTOFF.param);
    let mut moved = false;
    for _ in 0..60 {
        ui.update();
        moved |= (ui.renderer.anim[slot].current() - stored).abs() > 0.01;
        assert_eq!(composite_set(ui.params(), [CUTOFF; 3]), [stored; 3]);
    }
    assert!(moved, "the LFO moves the cell");
}

fn to_demo(ui: &mut UiState, def: &'static BlockDef) {
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::B6));
    let node = ui
        .nav
        .active_chain()
        .blocks
        .iter()
        .position(|b| core::ptr::eq(b.def, def))
        .expect("on the Demo chain");
    for _ in 0..node {
        feed(ui, Input::press(ButtonId::Plus));
    }
    assert!(core::ptr::eq(ui.nav.active_block_def(), def));
}

#[test]
fn only_the_matrix_demo_node_is_the_matrix() {
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B6));
    for n in 0..ui.nav.active_chain().len() {
        assert_eq!(ui.nav.node, n);
        let matrix = ui.nav.active_block_def().layout == PageLayout::Matrix;
        assert_eq!(
            ui.page() == PageKey::Legacy(PageId::DemoMatrix),
            matrix,
            "node {n}"
        );
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
}

fn focus_key(ui: &mut UiState) -> Option<RegionData> {
    render_dirty_ui(ui);
    ui.drawn_key(RegionKind::Focus)
}

#[test]
fn glyph_arc_page_drives_the_arc() {
    let mut ui = UiState::new();
    to_demo(&mut ui, &reg::DEMO_GLYPH_ARC);
    settle(&mut ui);
    let before = render_ui(&ui);
    assert_eq!(before.oob, 0);
    let key = focus_key(&mut ui);
    let (vol, pan) = (ui.params().out.volume, ui.params().out.pan);

    // Encoder a: the unipolar value.
    feed(&mut ui, Input::turn(EncoderId::A, 20));
    settle(&mut ui);
    assert_eq!(ui.focused_slot(), 0);
    assert_ne!(ui.params().out.volume, vol);
    let uni_key = focus_key(&mut ui);
    assert_ne!(uni_key, key);
    let uni = render_ui(&ui);
    assert_ne!(uni.hash(), before.hash());

    // Encoder b: the bipolar arc.
    feed(&mut ui, Input::turn(EncoderId::B, -10));
    settle(&mut ui);
    assert_eq!(ui.focused_slot(), 1);
    assert_ne!(ui.params().out.pan, pan);
    assert_ne!(focus_key(&mut ui), uni_key);
    let def = ui.nav.active_block_def();
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    assert!(view(def, 1, &ctx).fmt().is_bipolar());
    assert!(!view(def, 0, &ctx).fmt().is_bipolar());
    let bi = render_ui(&ui);
    assert_eq!(bi.oob, 0);
    assert_ne!(bi.hash(), uni.hash());
}
/// The arc's track at 12:00: lit on an ARC band, ground on a NONE band.
fn arc_top(fb: &Fb) -> embedded_graphics::pixelcolor::Rgb565 {
    fb.at(theme::ARC_CX, theme::ARC_CY - theme::ARC_R)
}

#[test]
fn glyph_none_page_steps_words_across_the_whole_band() {
    let mut ui = UiState::new();
    to_demo(&mut ui, &reg::DEMO_GLYPH_NONE);
    // MODEL's own spec carries NONE.
    let model = ParamAddr::new(BlockRef::Modal, ModalParams::MODE);
    assert_eq!(model.spec().unwrap().glyph, FocusGlyph::None);
    feed(&mut ui, Input::turn(EncoderId::A, -127));
    for (i, word) in MODEL_NAMES.iter().enumerate() {
        if i > 0 {
            feed(&mut ui, Input::turn(EncoderId::A, 1));
        }
        settle(&mut ui);
        assert_eq!(ui.params().modal.mode as usize, i, "{word}");
        let fb = render_ui(&ui);
        assert_eq!(fb.oob, 0, "{word}");
        assert_eq!(arc_top(&fb), theme::BG, "{word}: no gauge");
        let right = theme::FOCUS_VALUE_X + draw::text_width(&theme::FONT_FOCUS, word, 0);
        assert!(right <= theme::SCREEN_W - theme::MARGIN_X, "{word} fits");
    }
    // For contrast, an ARC page draws its track there.
    let mut arc = UiState::new();
    to_demo(&mut arc, &reg::DEMO_GLYPH_ARC);
    settle(&mut arc);
    assert_ne!(arc_top(&render_ui(&arc)), theme::BG);
}

/// The switch's knob centre at `on` (0 off, 1 on).
fn knob_x(on: i32) -> i32 {
    let x0 = theme::SWITCH_CX - theme::SWITCH_W / 2 + theme::SWITCH_H / 2;
    x0 + on * (theme::SWITCH_W - theme::SWITCH_H)
}

#[test]
fn glyph_switch_page_flips_a_real_two_state_param() {
    let mut ui = UiState::new();
    to_demo(&mut ui, &reg::DEMO_GLYPH_SWITCH);
    // LFO1 SYNC's own spec carries SWITCH.
    let sync = ParamAddr::new(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::SYNC);
    assert_eq!(sync.spec().unwrap().glyph, FocusGlyph::Switch);
    let lfo_sync = |ui: &UiState| {
        ui.params()
            .block(BlockRef::Lfo(LfoSlot::Lfo1))
            .unwrap()
            .get(LfoParams::SYNC)
    };

    // It reads as the LFO page shows it: FREE / RETRIG.
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    let v = view(ui.nav.active_block_def(), 0, &ctx);
    assert_eq!(v.addr(), Some(sync));
    assert_eq!(v.fmt(), ValFmt::Names(&["FREE", "RETRIG"]));

    feed(&mut ui, Input::turn(EncoderId::A, -1));
    settle(&mut ui);
    assert_eq!(lfo_sync(&ui), 0.0);
    let off = render_ui(&ui);
    assert_eq!(off.oob, 0);
    assert_eq!(arc_top(&off), theme::BG, "no arc");
    assert_eq!(
        off.at(knob_x(0), theme::ARC_CY),
        theme::MID,
        "knob left, off"
    );
    assert_eq!(off.at(knob_x(1), theme::ARC_CY), theme::FAINT, "track");

    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(lfo_sync(&ui), 1.0);
    // Eased: one frame in, the knob is on its way.
    ui.update();
    let t = ui.renderer.anim[0].current();
    assert!(t > 0.0 && t < 1.0, "eased: {t}");
    settle(&mut ui);
    let on = render_ui(&ui);
    assert_eq!(
        on.at(knob_x(1), theme::ARC_CY),
        theme::INK,
        "knob right, on"
    );
    assert_eq!(on.at(knob_x(0), theme::ARC_CY), theme::ACCENT, "lit track");
}

/// Tick `i` of `n` on the level bar, bottom (0) to top: where the
/// handle's centre sits at that step.
fn tick_y(i: i32, n: i32) -> i32 {
    let travel = theme::LEVEL_BOTTOM - theme::LEVEL_TOP - theme::LEVEL_W;
    theme::LEVEL_BOTTOM - theme::LEVEL_W / 2 - i * travel / (n - 1)
}

#[test]
fn glyph_level_page_drives_a_level_and_a_stepped_value() {
    let mut ui = UiState::new();
    to_demo(&mut ui, &reg::DEMO_GLYPH_LEVEL);
    // LEVEL BAR is the page's: the specs stay ARC until approved.
    let vol = ParamAddr::new(BlockRef::Out, OutParams::VOLUME);
    let fdbk = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::FEEDBACK);
    assert_eq!(vol.spec().unwrap().glyph, FocusGlyph::Arc);
    assert_eq!(fdbk.spec().unwrap().glyph, FocusGlyph::Arc);
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    assert_eq!(view(ui.nav.active_block_def(), 0, &ctx).addr(), Some(vol));
    assert_eq!(view(ui.nav.active_block_def(), 1, &ctx).addr(), Some(fdbk));
    let (x, tick_x) = (theme::LEVEL_X, theme::LEVEL_TICK_X);

    // a, low: lit at the bottom only.
    feed(&mut ui, Input::turn(EncoderId::A, -127));
    settle(&mut ui);
    let low = render_ui(&ui);
    assert_eq!(low.oob, 0);
    assert_eq!(arc_top(&low), theme::BG, "no arc");
    assert_eq!(low.at(x, theme::LEVEL_BOTTOM - 2), theme::ACCENT);
    assert_eq!(low.at(x, theme::LEVEL_TOP + 2), theme::FAINT);

    // a, high: lit to the top; eased on the way.
    feed(&mut ui, Input::turn(EncoderId::A, 127));
    ui.update();
    let t = ui.renderer.anim[0].current();
    assert!(t > 0.0 && t < 1.0, "eased: {t}");
    settle(&mut ui);
    let high = render_ui(&ui);
    assert_eq!(high.at(x, theme::LEVEL_TOP + 2), theme::ACCENT);
    // A continuous level: eight ticks.
    for i in 0..8 {
        assert_ne!(high.at(tick_x, tick_y(i, 8)), theme::BG, "tick {i}");
    }

    // b: FDBK, 0..7, a tick per step; at 3 the handle sits on tick 3.
    feed(&mut ui, Input::turn(EncoderId::B, -127));
    feed(&mut ui, Input::turn(EncoderId::B, 3));
    settle(&mut ui);
    assert_eq!(ui.focused_slot(), 1);
    let b = render_ui(&ui);
    for i in 0..8 {
        assert_ne!(b.at(tick_x, tick_y(i, 8)), theme::BG, "tick {i}");
    }
    assert_eq!(b.at(tick_x, (tick_y(0, 8) + tick_y(1, 8)) / 2), theme::BG);
    assert_eq!(
        b.at(x + theme::LEVEL_W - 1, tick_y(3, 8)),
        theme::ACCENT,
        "handle"
    );
    assert_eq!(b.at(x, tick_y(5, 8)), theme::FAINT, "track above");
}
