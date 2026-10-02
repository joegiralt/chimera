//! Focus glyphs: each param's hand-assigned gauge, the focus band's
//! dispatch, the UI clock and DEMO's glyph pages.

mod screen;

use chimera_core::addr::{BlockRead, BlockRef, Op, ParamAddr};
use chimera_core::block::{ParamSpec, ValFmt};
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::lfo::LfoParams;
use chimera_core::dsp::modal::{MODEL_NAMES, ModalParams};
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::modulation::ModSource;
use chimera_core::params::OutParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::block_def::{BlockDef, slot_addr};
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::glyph::{Braid, CompositeId, FocusGlyph, Gauge, Rings, anim_key};
use chimera_core::ui::page::{PageId, PageKey, PageLayout};
use chimera_core::ui::region::{RegionData, RegionKind};
use chimera_core::ui::view::{SlotCtx, View, view};
use chimera_core::ui::{draw, theme};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const NONE: FocusGlyph = FocusGlyph::None;
const SWITCH: FocusGlyph = FocusGlyph::Switch;
const LEVEL: FocusGlyph = FocusGlyph::LevelBar;
const XF: FocusGlyph = FocusGlyph::Crossfader;
const BRAID: FocusGlyph = FocusGlyph::Composite(CompositeId::ChorusBraid);
const RINGS: FocusGlyph = FocusGlyph::Composite(CompositeId::DelayRings);
const CUBE: FocusGlyph = FocusGlyph::Composite(CompositeId::ReverbCube);

/// Params assigned a glyph other than ARC, by `(block kind, ident)`: every
/// instance of the block (`AlgoOp`, `Env`, `Lfo`) alike. Assigning a glyph
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
    // Two states, one of them off: a toggle.
    ("Lfo", "SYNC", SWITCH),
    // Set-and-leave levels, 0 to max: a fader.
    ("Out", "LEVEL", LEVEL),
    ("AlgoOp", "LEVEL", LEVEL),
    ("AlgoOp", "FDBK", LEVEL),
    ("Part", "LEVEL", LEVEL),
    ("Part", "CHR", LEVEL),
    ("Part", "DLY", LEVEL),
    ("Part", "REV", LEVEL),
    ("Tape", "MIX", LEVEL),
    ("Comp", "MAKEUP", LEVEL),
    ("Comp", "MIX", LEVEL),
    // The chorus, all four params: the braid.
    ("Chorus", "MODE", BRAID),
    ("Chorus", "RATE", BRAID),
    ("Chorus", "DEPTH", BRAID),
    ("Chorus", "MIX", BRAID),
    // The delay, all seven params, CHAR's included: the rings.
    ("Delay", "TIME", RINGS),
    ("Delay", "FDBK", RINGS),
    ("Delay", "WOW", RINGS),
    ("Delay", "SAT", RINGS),
    ("Delay", "TONE", RINGS),
    ("Delay", "MIX", RINGS),
    ("Delay", "REV", RINGS),
    // The reverb, all five params: the cube.
    ("Reverb", "GRIT", CUBE),
    ("Reverb", "TIME", CUBE),
    ("Reverb", "DAMP", CUBE),
    ("Reverb", "SIZE", CUBE),
    ("Reverb", "MIX", CUBE),
    // Blends between two ends: a crossfader.
    ("Algo", "MORPH", XF),
    ("Drive", "MIX", XF),
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

/// Every LEVEL BAR param is a unipolar level, 0 to max.
#[test]
fn level_bars_are_unipolar() {
    for b in BlockRef::ALL {
        for s in b.specs().iter().filter(|s| s.glyph == FocusGlyph::LevelBar) {
            assert!(!s.fmt.is_bipolar(), "{b:?}.{}: bipolar keeps ARC", s.ident);
            assert!(s.min == 0.0 && s.max > 0.0, "{b:?}.{}: 0 to max", s.ident);
        }
    }
}

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
fn each_glyph_maps_to_its_gauge() {
    for g in FocusGlyph::ALL {
        let want = match g {
            FocusGlyph::None => Gauge::None,
            FocusGlyph::Switch => Gauge::Switch { on: 0.25 },
            FocusGlyph::LevelBar => Gauge::LevelBar {
                value: 0.25,
                ticks: 8,
            },
            FocusGlyph::Crossfader => Gauge::Crossfader { value: 0.25 },
            // A built composite: whatever its inputs make of it.
            FocusGlyph::Composite(CompositeId::ChorusBraid) => Gauge::Switch { on: 9.0 },
            FocusGlyph::Composite(CompositeId::DelayRings) => Gauge::Switch { on: 8.0 },
            FocusGlyph::Composite(CompositeId::ReverbCube) => Gauge::Switch { on: 7.0 },
            _ => Gauge::Arc {
                value: 0.25,
                bipolar: true,
            },
        };
        let composite = |id| match id {
            CompositeId::ChorusBraid => Gauge::Switch { on: 9.0 },
            CompositeId::DelayRings => Gauge::Switch { on: 8.0 },
            CompositeId::ReverbCube => Gauge::Switch { on: 7.0 },
        };
        assert_eq!(g.gauge(0.25, ValFmt::Bi, composite), want, "{g:?}");
    }
}

/// A level bar's ticks: one per step for a few steps, else 8.
#[test]
fn level_bar_ticks_follow_the_steps() {
    let ticks = |fmt| match FocusGlyph::LevelBar.gauge(0.5, fmt, |_| Gauge::None) {
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
    ui.update(UiTick::for_test());
    ui.update(UiTick::for_test());
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
        ui.update(UiTick::for_test());
    }
    render_dirty_ui(&mut ui);
    assert_eq!(ui.drawn_key(RegionKind::Focus), before);
}

fn to_demo(ui: &mut UiState, def: &'static BlockDef) {
    let node = reg::DEMO_BLOCKS
        .iter()
        .position(|b| core::ptr::eq(b.def, def))
        .expect("a DEMO page");
    screen::to_demo(ui, node);
    assert!(core::ptr::eq(ui.page_def(), def));
}

#[test]
fn only_the_matrix_demo_page_is_the_matrix() {
    for (n, b) in reg::DEMO_BLOCKS.iter().enumerate() {
        let mut ui = UiState::new();
        screen::to_demo(&mut ui, n);
        assert!(core::ptr::eq(ui.page_def(), b.def), "node {n}");
        let matrix = ui.page_def().layout == PageLayout::Matrix;
        assert_eq!(
            ui.page() == PageKey::Legacy(PageId::Demo(reg::DEMO_MATRIX.id)),
            matrix,
            "node {n}"
        );
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
    // Real ARC params, no override.
    let reso = ParamAddr::new(
        BlockRef::Filter,
        chimera_core::params::FilterParams::RESONANCE,
    );
    let pan_addr = ParamAddr::new(BlockRef::Out, OutParams::PAN);
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    for (slot, addr) in [(0, reso), (1, pan_addr)] {
        assert_eq!(view(ui.page_def(), slot, &ctx).addr(), Some(addr));
        assert_eq!(addr.spec().unwrap().glyph, FocusGlyph::Arc);
    }
    let (vol, pan) = (ui.params().filter.resonance, ui.params().out.pan);

    // Encoder a: the unipolar value.
    feed(&mut ui, Input::turn(EncoderId::A, 20));
    settle(&mut ui);
    assert_eq!(ui.focused_slot(), 0);
    assert_ne!(ui.params().filter.resonance, vol);
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
    let def = ui.page_def();
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
    let v = view(ui.page_def(), 0, &ctx);
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
    ui.update(UiTick::for_test());
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
    // Both specs carry LEVEL BAR.
    let vol = ParamAddr::new(BlockRef::Out, OutParams::VOLUME);
    let fdbk = ParamAddr::new(BlockRef::AlgoOp(Op::A), AlgoOpParams::FEEDBACK);
    assert_eq!(vol.spec().unwrap().glyph, FocusGlyph::LevelBar);
    assert_eq!(fdbk.spec().unwrap().glyph, FocusGlyph::LevelBar);
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    assert_eq!(view(ui.page_def(), 0, &ctx).addr(), Some(vol));
    assert_eq!(view(ui.page_def(), 1, &ctx).addr(), Some(fdbk));
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
    ui.update(UiTick::for_test());
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

/// The crossfader's cap centre at `v` (0 left, 1 right).
fn cap_x(v: f32) -> i32 {
    let x0 = theme::XF_CX - theme::XF_W / 2 + theme::XF_CAP_W / 2;
    x0 + (v * (theme::XF_W - theme::XF_CAP_W) as f32).round() as i32
}

fn to_xf(ui: &mut UiState) {
    to_demo(ui, &reg::DEMO_GLYPH_XF);
}

#[test]
fn glyph_crossfader_page_slides_left_centre_right() {
    use chimera_core::dsp::algo::params::AlgoParams;
    use chimera_core::params::DriveParams;
    let mut ui = UiState::new();
    to_xf(&mut ui);
    // Both specs carry CROSSFADER.
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let mix = ParamAddr::new(BlockRef::Drive, DriveParams::MIX);
    assert_eq!(morph.spec().unwrap().glyph, FocusGlyph::Crossfader);
    assert_eq!(mix.spec().unwrap().glyph, FocusGlyph::Crossfader);
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    assert_eq!(view(ui.page_def(), 0, &ctx).addr(), Some(morph));
    assert_eq!(view(ui.page_def(), 1, &ctx).addr(), Some(mix));
    assert!(!Gauge::Crossfader { value: 0.5 }.animates());

    let y = theme::ARC_CY - theme::XF_CAP_H / 2 + 2;
    for (ticks, v) in [(0i8, 0.0f32), (64, 64.0 / 127.0), (127, 1.0)] {
        feed(&mut ui, Input::turn(EncoderId::A, -127));
        feed(&mut ui, Input::turn(EncoderId::A, ticks));
        settle(&mut ui);
        let fb = render_ui(&ui);
        assert_eq!(fb.oob, 0);
        assert_eq!(arc_top(&fb), theme::BG, "no arc");
        assert_eq!(fb.at(cap_x(v), y), theme::ACCENT, "cap at {v}");
        // Only the cap rises that high: nothing at the far end.
        let other = if v < 0.5 { cap_x(1.0) } else { cap_x(0.0) };
        assert_eq!(fb.at(other, y), theme::BG, "{v}");
    }
}

/// The crossfader draws the set value: an LFO on MORPH moves its cell's
/// value, never the cap.
#[test]
fn crossfader_ignores_modulation() {
    let mut ui = UiState::new();
    to_xf(&mut ui);
    feed(&mut ui, Input::turn(EncoderId::A, -127));
    feed(&mut ui, Input::turn(EncoderId::A, 64));
    let morph = ParamAddr::new(
        BlockRef::Algo,
        chimera_core::dsp::algo::params::AlgoParams::MORPH,
    );
    let m = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.mod_state;
    let d = m.push(morph).unwrap();
    m.set_route(ModSource::Lfo1.index(), d, 127);
    settle(&mut ui);
    let cap = |fb: &Fb| -> Vec<_> {
        let x0 = theme::XF_CX - theme::XF_W / 2;
        let y0 = theme::ARC_CY - theme::XF_CAP_H / 2;
        (x0..x0 + theme::XF_W)
            .flat_map(|x| (y0..y0 + theme::XF_CAP_H).map(move |y| (x, y)))
            .map(|(x, y)| fb.at(x, y))
            .collect()
    };
    let first = cap(&render_ui(&ui));
    let mut moved = false;
    for _ in 0..40 {
        ui.update(UiTick::for_test());
        moved |= (ui.renderer.anim[0].current() - ui.renderer.set[0].current()).abs() > 0.01;
        assert_eq!(cap(&render_ui(&ui)), first);
    }
    assert!(moved, "the LFO moves the cell");
}

#[test]
fn braid_reads_its_set_values() {
    let b = |mode: f32| Braid::from_set([mode, 0.5, 0.5, 0.5], None, 0);
    // OFF a straight line; I two strands; II two, faster; I+II three.
    let strands: Vec<_> = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]
        .iter()
        .map(|&m| b(m).strands())
        .collect();
    assert_eq!(strands, [0, 2, 2, 3]);
    let at = |mode: f32, frame| Braid::from_set([mode, 0.5, 0.5, 0.5], None, frame);
    assert!(
        at(2.0 / 3.0, 100).twist() != at(1.0 / 3.0, 100).twist(),
        "II twists faster"
    );
    assert_eq!(b(1.0).mode, chimera_core::dsp::chorus::ChorusMode::JunoBoth);
    // RATE is how fast the twist travels: frame 0 is still, rate 0 slowest.
    let rate = |r: f32| Braid::from_set([1.0 / 3.0, r, 0.5, 0.5], None, 20).twist();
    assert!(rate(1.0) > rate(0.0));
    assert!(Gauge::Braid(b(1.0)).animates());
}

fn braid_box(x: i32, y: i32) -> bool {
    (theme::BRAID_X..theme::BRAID_X + theme::BRAID_W).contains(&x)
        && (theme::BRAID_Y..theme::BRAID_Y + theme::BRAID_H).contains(&y)
}

/// MODE I+II, DEPTH up, MIX up, `slot` focused.
fn braid_ui(slot: EncoderId) -> UiState {
    let mut ui = UiState::new();
    to_demo(&mut ui, &reg::DEMO_GLYPH_BRAID);
    feed(&mut ui, Input::turn(EncoderId::A, 3));
    feed(&mut ui, Input::turn(EncoderId::C, 30));
    feed(&mut ui, Input::turn(EncoderId::D, 60));
    // Focus `slot` without moving it.
    feed(&mut ui, Input::turn(slot, -1));
    feed(&mut ui, Input::turn(slot, 1));
    settle(&mut ui);
    ui
}

#[test]
fn glyph_braid_page_moves_only_inside_its_box() {
    use chimera_core::dsp::chorus::ChorusParams;
    // Every chorus param carries the braid.
    for id in [
        ChorusParams::MODE,
        ChorusParams::RATE,
        ChorusParams::DEPTH,
        ChorusParams::MIX,
    ] {
        let g = ParamAddr::new(BlockRef::Chorus, id).spec().unwrap().glyph;
        assert_eq!(g, FocusGlyph::Composite(CompositeId::ChorusBraid));
    }
    let mut ui = braid_ui(EncoderId::B);
    assert_eq!(ui.focused_slot(), 1);
    let a = render_ui(&ui);
    assert_eq!(a.oob, 0);
    assert_eq!(arc_top(&a), theme::BG, "no arc");
    ui.update(UiTick::for_test());
    let b = render_ui(&ui);
    let mut inside = 0;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if a.at(x, y) != b.at(x, y) {
                assert!(braid_box(x, y), "({x}, {y}) moved outside the box");
                inside += 1;
            }
        }
    }
    assert!(inside > 0, "the braid moves on the clock");
}

/// Each frame the braid redraws its box alone, and the screen stays what
/// a full render draws.
#[test]
fn braid_redraws_only_its_box_each_frame() {
    let mut ui = braid_ui(EncoderId::C);
    let mut fb = Fb::new();
    let perf = chimera_core::ui::perf::PerfStats::zero();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
    for _ in 0..3 {
        ui.update(UiTick::for_test());
        let flushed: Vec<_> = ui
            .render_dirty_with_scope(&mut fb, &perf, &scope_fixture())
            .into_iter()
            .filter(|&(a, b)| a != b)
            .collect();
        let rows = (
            theme::BRAID_Y as u16,
            (theme::BRAID_Y + theme::BRAID_H) as u16,
        );
        assert_eq!(flushed, [rows]);
        let mut full = Fb::new();
        ui.render_with_scope(&mut full, &perf, &scope_fixture());
        assert!(fb.px == full.px, "dirty frame matches a full render");
    }
}

/// The focused param is emphasised: same frame, a different braid.
#[test]
fn braid_emphasises_the_focused_param() {
    let shots: Vec<_> = [EncoderId::A, EncoderId::B, EncoderId::C, EncoderId::D]
        .into_iter()
        .map(|e| {
            let mut ui = braid_ui(e);
            // Same clock frame for every shot.
            while ui.clock().frame() < 200 {
                ui.update(UiTick::for_test());
            }
            render_ui(&ui)
        })
        .collect();
    let boxed = |fb: &Fb| -> Vec<_> {
        (theme::BRAID_Y..theme::BRAID_Y + theme::BRAID_H)
            .flat_map(|y| (theme::BRAID_X..theme::BRAID_X + theme::BRAID_W).map(move |x| (x, y)))
            .map(|(x, y)| fb.at(x, y))
            .collect()
    };
    for i in 0..4 {
        for j in i + 1..4 {
            assert_ne!(boxed(&shots[i]), boxed(&shots[j]), "focus {i} vs {j}");
        }
    }
}

/// Every value a composite's param can show, as the focus band writes it.
fn value_texts(spec: &ParamSpec) -> Vec<String> {
    match spec.fmt {
        ValFmt::Names(names) => names.iter().map(|n| n.to_string()).collect(),
        fmt => (0..=128)
            .map(|i| {
                let mut buf = chimera_core::ui::fmt::FmtBuf::new();
                chimera_core::ui::fmt::fmt_val(&mut buf, i as f32 / 128.0, fmt);
                buf.as_str().to_string()
            })
            .collect(),
    }
}

/// A composite clears its box each frame: no value its params can show
/// may reach into it.
#[test]
fn composite_values_stay_clear_of_the_box() {
    const GAP: i32 = 4;
    for b in BlockRef::ALL {
        for s in b.specs() {
            let FocusGlyph::Composite(id) = s.glyph else {
                continue;
            };
            let box_x = match id {
                CompositeId::ChorusBraid => theme::BRAID_X,
                CompositeId::DelayRings => theme::RINGS_X,
                CompositeId::ReverbCube => theme::CUBE_X,
            };
            for w in value_texts(s) {
                let right = theme::FOCUS_VALUE_X + draw::text_width(&theme::FONT_FOCUS, &w, 0);
                assert!(
                    right + GAP <= box_x,
                    "{b:?}.{} {w}: ends at {right}",
                    s.ident
                );
            }
        }
    }
}

/// While a MIX+PLUS status shows, the band carries no gauge: the clock
/// must not paint the braid over the message.
#[test]
fn a_status_stops_the_glyph_redrawing() {
    status_stops_redrawing(braid_ui(EncoderId::B));
    status_stops_redrawing(rings_ui(EncoderId::A));
    status_stops_redrawing(cube_ui(EncoderId::A));
}

fn status_stops_redrawing(mut ui: UiState) {
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    assert!(ui.prime_status().is_some(), "FX params aren't modulatable");
    let mut fb = Fb::new();
    let perf = chimera_core::ui::perf::PerfStats::zero();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
    ui.update(UiTick::for_test());
    let flushed: Vec<_> = ui
        .render_dirty_with_scope(&mut fb, &perf, &scope_fixture())
        .into_iter()
        .filter(|&(a, b)| a != b)
        .collect();
    assert!(flushed.is_empty(), "{flushed:?}");
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &perf, &scope_fixture());
    assert!(fb.px == full.px);
}

#[test]
fn rings_read_their_set_values() {
    // TIME, FDBK, TONE, MIX, MECH, SAT, REV.
    let r = |time: f32, fdbk: f32, tone: f32| {
        Rings::from_set([time, fdbk, tone, 0.5, 0.0, 0.0, 0.0], None, 0)
    };
    // TIME spaces the rings.
    assert!(r(1.0, 0.5, 0.5).spacing() > r(0.0, 0.5, 0.5).spacing());
    // FDBK: how many repeats survive; none feeding back, one ring.
    assert_eq!(r(0.5, 0.0, 0.5).survivors(), 1);
    assert!(r(0.5, 0.9, 0.5).survivors() > r(0.5, 0.4, 0.5).survivors());
    // TONE: how many stay crisp before they blur.
    assert!(r(0.5, 0.9, 1.0).crisp() > r(0.5, 0.9, 0.0).crisp());
    assert!(r(0.5, 0.9, 0.0).crisp() >= 1);
    assert!(Gauge::Rings(r(0.5, 0.5, 0.5)).animates());
}

fn rings_box(x: i32, y: i32) -> bool {
    (theme::RINGS_X..theme::RINGS_X + theme::RINGS_W).contains(&x)
        && (theme::RINGS_Y..theme::RINGS_Y + theme::RINGS_H).contains(&y)
}

const SIX: [EncoderId; 6] = [
    EncoderId::A,
    EncoderId::B,
    EncoderId::C,
    EncoderId::D,
    EncoderId::E,
    EncoderId::F,
];

/// FDBK and MIX up, MECH and SAT up a little, `slot` focused.
fn rings_ui(slot: EncoderId) -> UiState {
    let mut ui = UiState::new();
    to_demo(&mut ui, &reg::DEMO_GLYPH_RINGS);
    feed(&mut ui, Input::turn(EncoderId::B, 40));
    feed(&mut ui, Input::turn(EncoderId::D, 64));
    feed(&mut ui, Input::turn(EncoderId::E, 20));
    feed(&mut ui, Input::turn(EncoderId::F, 30));
    feed(&mut ui, Input::turn(slot, -1));
    feed(&mut ui, Input::turn(slot, 1));
    settle(&mut ui);
    ui
}

#[test]
fn glyph_rings_page_moves_only_inside_its_box() {
    use chimera_core::dsp::delay::DelayParams;
    // Every delay param carries the rings.
    for s in BlockRef::Delay.specs() {
        assert_eq!(s.glyph, RINGS, "{}", s.ident);
    }
    // Every value the page's params show stays clear of the box.
    for id in [
        DelayParams::TIME_MS,
        DelayParams::FEEDBACK,
        DelayParams::TONE,
        DelayParams::MIX,
        DelayParams::WOW_FLUTTER,
        DelayParams::SATURATION,
    ] {
        let spec = ParamAddr::new(BlockRef::Delay, id).spec().unwrap();
        for w in value_texts(spec) {
            let right = theme::FOCUS_VALUE_X + draw::text_width(&theme::FONT_FOCUS, &w, 0);
            assert!(right + 4 <= theme::RINGS_X, "{} {w}", spec.ident);
        }
    }
    let mut ui = rings_ui(EncoderId::A);
    let a = render_ui(&ui);
    assert_eq!(a.oob, 0);
    assert_eq!(arc_top(&a), theme::BG, "no arc");
    let mut inside = 0;
    for _ in 0..3 {
        ui.update(UiTick::for_test());
    }
    let b = render_ui(&ui);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if a.at(x, y) != b.at(x, y) {
                assert!(rings_box(x, y), "({x}, {y}) moved outside the box");
                inside += 1;
            }
        }
    }
    assert!(inside > 0, "the rings move on the clock");
}

#[test]
fn rings_redraw_only_their_box_each_frame() {
    let mut ui = rings_ui(EncoderId::C);
    let mut fb = Fb::new();
    let perf = chimera_core::ui::perf::PerfStats::zero();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
    for _ in 0..3 {
        ui.update(UiTick::for_test());
        let flushed: Vec<_> = ui
            .render_dirty_with_scope(&mut fb, &perf, &scope_fixture())
            .into_iter()
            .filter(|&(a, b)| a != b)
            .collect();
        let rows = (
            theme::RINGS_Y as u16,
            (theme::RINGS_Y + theme::RINGS_H) as u16,
        );
        assert_eq!(flushed, [rows]);
        let mut full = Fb::new();
        ui.render_with_scope(&mut full, &perf, &scope_fixture());
        assert!(fb.px == full.px, "dirty frame matches a full render");
    }
}

#[test]
fn rings_emphasise_the_focused_param() {
    let shots: Vec<_> = SIX
        .into_iter()
        .map(|e| {
            let mut ui = rings_ui(e);
            while ui.clock().frame() < 200 {
                ui.update(UiTick::for_test());
            }
            render_ui(&ui)
        })
        .collect();
    let boxed = |fb: &Fb| -> Vec<_> {
        (theme::RINGS_Y..theme::RINGS_Y + theme::RINGS_H)
            .flat_map(|y| (theme::RINGS_X..theme::RINGS_X + theme::RINGS_W).map(move |x| (x, y)))
            .map(|(x, y)| fb.at(x, y))
            .collect()
    };
    for i in 0..6 {
        for j in i + 1..6 {
            assert_ne!(boxed(&shots[i]), boxed(&shots[j]), "focus {i} vs {j}");
        }
    }
}

/// At any setting, focus and frame, a composite draws inside its box only:
/// the box is all it clears each frame.
#[test]
fn composites_draw_inside_their_boxes() {
    use chimera_core::ui::components::{draw_gauge, gauge_rect};
    use chimera_core::ui::glyph::{BraidPart, Cube, CubePart, RingsPart};
    let check = |g: Gauge| {
        let (x, y, w, h) = gauge_rect(&g).unwrap();
        let mut fb = Fb::new();
        draw_gauge(&mut fb, g);
        assert_eq!(fb.oob, 0);
        for py in 0..H as i32 {
            for px in 0..W as i32 {
                if fb.px[py as usize * W + px as usize] != 0 {
                    assert!(
                        (x..x + w).contains(&px) && (y..y + h).contains(&py),
                        "{g:?} at ({px}, {py})"
                    );
                }
            }
        }
    };
    for v in [0.0, 0.5, 1.0] {
        for frame in (0..200).step_by(7) {
            for focus in RingsPart::ALL.map(Some).into_iter().chain([None]) {
                check(Gauge::Rings(Rings::from_set(
                    [v, 1.0 - v, v, v, 1.0, 1.0, 1.0],
                    focus,
                    frame,
                )));
            }
            for focus in CubePart::ALL.map(Some).into_iter().chain([None]) {
                check(Gauge::Cube(Cube::from_set([v, v, v, 1.0, v], focus, frame)));
                check(Gauge::Cube(Cube::from_set(
                    [1.0, 1.0 - v, 1.0, 1.0, 1.0],
                    focus,
                    frame,
                )));
            }
            for focus in BraidPart::ALL.map(Some).into_iter().chain([None]) {
                check(Gauge::Braid(Braid::from_set([v, v, 1.0, v], focus, frame)));
            }
        }
    }
}

#[test]
fn cube_reads_its_set_values() {
    use chimera_core::ui::glyph::Cube;
    // SIZE, TIME, DAMP, MIX, GRIT.
    let c = |size: f32, time: f32| Cube::from_set([size, time, 0.5, 0.5, 0.0], None, 0);
    // SIZE grows the room.
    assert!(c(1.0, 0.5).half() > c(0.0, 0.5).half());
    // TIME: how long the afterimage trails; none at 0.
    assert_eq!(c(0.5, 0.0).trails(), 0);
    assert!(c(0.5, 1.0).trails() > c(0.5, 0.4).trails());
    // It turns on the clock.
    let at = |frame| Cube::from_set([0.5; 5], None, frame).turn();
    assert_ne!(at(0), at(20));
    assert!(Gauge::Cube(c(0.5, 0.5)).animates());
}

fn cube_box(x: i32, y: i32) -> bool {
    (theme::CUBE_X..theme::CUBE_X + theme::CUBE_W).contains(&x)
        && (theme::CUBE_Y..theme::CUBE_Y + theme::CUBE_H).contains(&y)
}

/// MIX and GRIT up a little, DAMP up, `slot` focused.
fn cube_ui(slot: EncoderId) -> UiState {
    let mut ui = UiState::new();
    to_demo(&mut ui, &reg::DEMO_GLYPH_CUBE);
    feed(&mut ui, Input::turn(EncoderId::C, 30));
    feed(&mut ui, Input::turn(EncoderId::D, 64));
    feed(&mut ui, Input::turn(EncoderId::E, 20));
    feed(&mut ui, Input::turn(slot, -1));
    feed(&mut ui, Input::turn(slot, 1));
    settle(&mut ui);
    ui
}

#[test]
fn glyph_cube_page_moves_only_inside_its_box() {
    // Every reverb param carries the cube.
    for s in BlockRef::Reverb.specs() {
        assert_eq!(s.glyph, CUBE, "{}", s.ident);
        for w in value_texts(s) {
            let right = theme::FOCUS_VALUE_X + draw::text_width(&theme::FONT_FOCUS, &w, 0);
            assert!(right + 4 <= theme::CUBE_X, "{} {w}", s.ident);
        }
    }
    let mut ui = cube_ui(EncoderId::A);
    let a = render_ui(&ui);
    assert_eq!(a.oob, 0);
    assert_eq!(arc_top(&a), theme::BG, "no arc");
    for _ in 0..3 {
        ui.update(UiTick::for_test());
    }
    let b = render_ui(&ui);
    let mut inside = 0;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if a.at(x, y) != b.at(x, y) {
                assert!(cube_box(x, y), "({x}, {y}) moved outside the box");
                inside += 1;
            }
        }
    }
    assert!(inside > 0, "the cube turns on the clock");
}

#[test]
fn cube_redraws_only_its_box_each_frame() {
    let mut ui = cube_ui(EncoderId::B);
    let mut fb = Fb::new();
    let perf = chimera_core::ui::perf::PerfStats::zero();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
    for _ in 0..3 {
        ui.update(UiTick::for_test());
        let flushed: Vec<_> = ui
            .render_dirty_with_scope(&mut fb, &perf, &scope_fixture())
            .into_iter()
            .filter(|&(a, b)| a != b)
            .collect();
        let rows = (theme::CUBE_Y as u16, (theme::CUBE_Y + theme::CUBE_H) as u16);
        assert_eq!(flushed, [rows]);
        let mut full = Fb::new();
        ui.render_with_scope(&mut full, &perf, &scope_fixture());
        assert!(fb.px == full.px, "dirty frame matches a full render");
    }
}

#[test]
fn cube_emphasises_the_focused_param() {
    let shots: Vec<_> = SIX[..5]
        .iter()
        .map(|&e| {
            let mut ui = cube_ui(e);
            while ui.clock().frame() < 200 {
                ui.update(UiTick::for_test());
            }
            render_ui(&ui)
        })
        .collect();
    let boxed = |fb: &Fb| -> Vec<_> {
        (theme::CUBE_Y..theme::CUBE_Y + theme::CUBE_H)
            .flat_map(|y| (theme::CUBE_X..theme::CUBE_X + theme::CUBE_W).map(move |x| (x, y)))
            .map(|(x, y)| fb.at(x, y))
            .collect()
    };
    for i in 0..5 {
        for j in i + 1..5 {
            assert_ne!(boxed(&shots[i]), boxed(&shots[j]), "focus {i} vs {j}");
        }
    }
}

/// The box of `rect` in `fb`.
fn boxed_rect(
    fb: &Fb,
    (x, y, w, h): (i32, i32, i32, i32),
) -> Vec<embedded_graphics::pixelcolor::Rgb565> {
    (y..y + h)
        .flat_map(|py| (x..x + w).map(move |px| (px, py)))
        .map(|(px, py)| fb.at(px, py))
        .collect()
}

/// On the real path, a composite draws the set values: a different
/// display (modulated) value in the focused slot leaves its box as it was.
#[test]
fn composites_draw_set_values_not_the_animated_ones() {
    let shots = [
        (
            braid_ui(EncoderId::B),
            1,
            (
                theme::BRAID_X,
                theme::BRAID_Y,
                theme::BRAID_W,
                theme::BRAID_H,
            ),
        ),
        (
            rings_ui(EncoderId::A),
            0,
            (
                theme::RINGS_X,
                theme::RINGS_Y,
                theme::RINGS_W,
                theme::RINGS_H,
            ),
        ),
        (
            cube_ui(EncoderId::A),
            0,
            (theme::CUBE_X, theme::CUBE_Y, theme::CUBE_W, theme::CUBE_H),
        ),
    ];
    for (mut ui, slot, rect) in shots {
        assert_eq!(ui.focused_slot(), slot);
        let before = boxed_rect(&render_ui(&ui), rect);
        let v = ui.renderer.anim[slot].current();
        ui.renderer.anim[slot].snap(if v > 0.5 { v - 0.4 } else { v + 0.4 });
        assert_eq!(boxed_rect(&render_ui(&ui), rect), before, "slot {slot}");
    }
}

/// The focus key carries the set value: under a modulation pinned at the
/// top, a CROSSFADER's set value moving still redraws the cap.
#[test]
fn a_crossfader_redraws_when_only_its_set_value_moves() {
    use chimera_core::dsp::algo::params::AlgoParams;
    let morph = ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH);
    let mut ui = UiState::new();
    feed(&mut ui, Input::press(ButtonId::B2));
    feed(&mut ui, Input::press(ButtonId::B1));
    let ctx = SlotCtx::read(ui.params(), ui.selected_op());
    let slot = (0..6)
        .find(|&i| slot_addr(ui.page_def(), i, &ctx) == Some(morph))
        .expect("ALG holds MORPH");
    feed(&mut ui, Input::turn(SIX[slot], -127));
    feed(&mut ui, Input::turn(SIX[slot], 76));
    // VEL stands in as 1 on the display: a full route pins MORPH at the top.
    let m = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.mod_state;
    let d = m.push(morph).unwrap();
    m.set_route(ModSource::Vel.index(), d, 127);
    let perf = chimera_core::ui::perf::PerfStats::zero();
    let mut fb = Fb::new();
    for _ in 0..120 {
        ui.update(UiTick::for_test());
        ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
    }
    assert_eq!(ui.renderer.anim[slot].current(), 1.0, "modulated, pinned");
    assert!((ui.renderer.set[slot].current() - 76.0 / 127.0).abs() < 0.01);
    let xf = (
        theme::XF_CX - theme::XF_W / 2,
        theme::ARC_CY - theme::XF_CAP_H / 2,
        theme::XF_W,
        theme::XF_CAP_H,
    );
    let before = boxed_rect(&fb, xf);
    feed(&mut ui, Input::turn(SIX[slot], 26));
    for _ in 0..120 {
        ui.update(UiTick::for_test());
        ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
    }
    assert_eq!(ui.renderer.anim[slot].current(), 1.0, "still pinned");
    assert_ne!(boxed_rect(&fb, xf), before, "the cap moved");
    let mut full = Fb::new();
    ui.render_with_scope(&mut full, &perf, &scope_fixture());
    assert!(fb.px == full.px, "dirty frame matches a full render");
}

/// The wobble's phase stays in 0..τ, smooth, however long the clock runs.
#[test]
fn rings_wobble_phase_wraps_in_f64() {
    let tau = core::f64::consts::TAU;
    for frame in [0, 1, 1000, u32::MAX - 1] {
        let r = Rings::from_set([0.5; 7], None, frame);
        let want = ((frame as f64 * 0.37) % tau) as f32;
        assert_eq!(r.wobble_phase(), want, "{frame}");
        assert!((0.0..core::f32::consts::TAU + 1e-3).contains(&r.wobble_phase()));
    }
}

/// Each composite's params are the specs that carry it, both ways; a
/// CROSSFADER blends a range, never a list of names.
#[test]
fn composite_params_match_their_specs() {
    let ids = [
        CompositeId::ChorusBraid,
        CompositeId::DelayRings,
        CompositeId::ReverbCube,
    ];
    for b in BlockRef::ALL {
        for s in b.specs() {
            let addr = ParamAddr::new(b, s.id);
            if let FocusGlyph::Composite(id) = s.glyph {
                assert!(
                    id.params().contains(&addr),
                    "{b:?}.{} not in {id:?}",
                    s.ident
                );
            }
            if s.glyph == FocusGlyph::Crossfader {
                assert!(!matches!(s.fmt, ValFmt::Names(_)), "{b:?}.{}", s.ident);
            }
        }
    }
    for id in ids {
        for a in id.params() {
            assert_eq!(a.spec().unwrap().glyph, FocusGlyph::Composite(id), "{a:?}");
        }
    }
}
