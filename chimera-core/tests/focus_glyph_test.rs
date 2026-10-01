//! Focus glyphs: each param's hand-assigned gauge, the focus band's
//! dispatch, the UI clock and the Demo chain's glyph pages.

mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{ParamSpec, ValFmt};
use chimera_core::params::OutParams;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::glyph::{CompositeId, Drawn, FocusGlyph, anim_key};
use chimera_core::ui::region::RegionKind;
use chimera_core::ui::view::View;
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
        assert_eq!(g.drawn(), Drawn::Arc, "{g:?}");
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
    assert!(!Drawn::Arc.animates());
}

#[test]
fn the_clock_ticks_once_a_frame() {
    let mut ui = chimera_core::ui::UiState::new();
    let t = ui.clock().frame();
    ui.update();
    ui.update();
    assert_eq!(ui.clock().frame(), t.wrapping_add(2));
}

#[test]
fn an_arc_focus_band_does_not_redraw_on_the_clock() {
    let mut ui = chimera_core::ui::UiState::new();
    settle(&mut ui);
    render_dirty_ui(&mut ui);
    let before = ui.drawn_key(RegionKind::Focus);
    for _ in 0..5 {
        ui.update();
    }
    render_dirty_ui(&mut ui);
    assert_eq!(ui.drawn_key(RegionKind::Focus), before);
}

fn render_dirty_ui(ui: &mut chimera_core::ui::UiState) {
    let mut fb = Fb::new();
    ui.render_dirty_with_scope(
        &mut fb,
        &chimera_core::ui::perf::PerfStats::zero(),
        &scope_fixture(),
    );
}

fn to_glyph_arc(ui: &mut chimera_core::ui::UiState) {
    feed(ui, Input::chord(ButtonId::Mix, ButtonId::B6));
    let chain = ui.nav.active_chain();
    let node = chain
        .blocks
        .iter()
        .position(|b| core::ptr::eq(b.def, &reg::DEMO_GLYPH_ARC))
        .expect("GLYPH: ARC on the Demo chain");
    for _ in 0..node {
        feed(ui, Input::press(ButtonId::Plus));
    }
    assert!(core::ptr::eq(
        ui.nav.active_block_def(),
        &reg::DEMO_GLYPH_ARC
    ));
}

#[test]
fn glyph_arc_page_drives_the_arc() {
    let mut ui = chimera_core::ui::UiState::new();
    to_glyph_arc(&mut ui);
    settle(&mut ui);
    let before = render_ui(&ui);
    assert_eq!(before.oob, 0);

    // Encoder a: the unipolar value.
    feed(&mut ui, Input::turn(EncoderId::A, 20));
    settle(&mut ui);
    assert_eq!(ui.focused_slot(), 0);
    let uni = render_ui(&ui);
    assert_ne!(uni.hash(), before.hash());

    // Encoder b: the bipolar arc.
    feed(&mut ui, Input::turn(EncoderId::B, -10));
    settle(&mut ui);
    assert_eq!(ui.focused_slot(), 1);
    let def = ui.nav.active_block_def();
    let ctx = chimera_core::ui::view::SlotCtx::read(&ui.params().clone(), ui.selected_op());
    assert!(
        chimera_core::ui::view::view(def, 1, &ctx)
            .fmt()
            .is_bipolar()
    );
    assert!(
        !chimera_core::ui::view::view(def, 0, &ctx)
            .fmt()
            .is_bipolar()
    );
    let bi = render_ui(&ui);
    assert_eq!(bi.oob, 0);
    assert_ne!(bi.hash(), uni.hash());
}
