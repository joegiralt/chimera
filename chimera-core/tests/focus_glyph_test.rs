//! Focus glyphs: each param's hand-assigned gauge, the focus band's
//! dispatch, the UI clock and the Demo chain's glyph pages.

mod screen;

use chimera_core::addr::{BlockRef, Blocks, ParamAddr};
use chimera_core::block::{ParamSpec, ValFmt};
use chimera_core::dsp::modal::{MODEL_NAMES, ModalParams};
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

/// Params assigned a glyph other than ARC, by `(block, ident)`. Each glyph
/// story adds its rows here.
const ASSIGNED: &[(BlockRef, &str, FocusGlyph)] = &[];

#[test]
fn every_param_is_arc_unless_assigned() {
    for b in BlockRef::ALL {
        for s in b.specs() {
            let want = ASSIGNED
                .iter()
                .find(|(ab, id, _)| *ab == b && *id == s.ident)
                .map_or(FocusGlyph::Arc, |a| a.2);
            assert_eq!(s.glyph, want, "{b:?}.{}", s.ident);
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
            _ => Gauge::Arc {
                value: 0.25,
                bipolar: true,
            },
        };
        assert_eq!(g.gauge(0.25, true), want, "{g:?}");
    }
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
    // NONE is the page's, not MODEL's: MODEL's spec stays ARC.
    let model = ParamAddr::new(BlockRef::Modal, ModalParams::MODE);
    assert_eq!(model.spec().unwrap().glyph, FocusGlyph::Arc);
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
