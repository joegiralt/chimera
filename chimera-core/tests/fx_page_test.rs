//! FLT's pages in layout A (#614, ADR 0155): KIND · MODE or VOICE · FX ·
//! PLACE · FREQ or CUTOFF · Q or RES up front, the routes and jacks on
//! FLT › MOD, and FLT › FX's INPUT, the type's pots and MIX.

use chimera_core::addr::{BlockRef, Op};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::voice_fx::params::{FxType, VoiceFxParams};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::ui::block_def::{ChainBlock, SlotBinding};
use chimera_core::ui::block_registry::{AMP, FILTER, FILTER_FX, FILTER_MOD};
use chimera_core::ui::fx_panel;
use chimera_core::ui::nav::chain_def_for;
use chimera_core::ui::view::{self, SlotCtx, View};
use chimera_core::ui::{draw, mod_grid, theme};

fn sound(kind: FilterKind, fx: FxType) -> Sound {
    let mut s = Sound::plain_init(EngineType::Algo);
    s.params.filter.set_kind(kind);
    s.params.fx.set_type(fx);
    s
}

fn labels(def: &chimera_core::ui::block_def::BlockDef, s: &Sound) -> Vec<&'static str> {
    let ctx = SlotCtx::read(&s.params, Op::A);
    (0..6).map(|i| view::view(def, i, &ctx).label()).collect()
}

#[test]
fn flt_front_reads_kind_mode_fx_place_freq_q_on_clean() {
    for k in [FilterKind::Svf, FilterKind::Dirty, FilterKind::Ladder] {
        let s = sound(k, FxType::Off);
        assert_eq!(
            labels(&FILTER, &s),
            ["TYPE", "MODE", "FX", "PLACE", "FREQ", "Q"],
            "{k:?}"
        );
    }
}

#[test]
fn flt_front_reads_kind_voice_fx_place_cutoff_res_on_2040() {
    let s = sound(FilterKind::K2040, FxType::Off);
    assert_eq!(
        labels(&FILTER, &s),
        ["TYPE", "VOICE", "FX", "PLACE", "CUTOFF", "RES"]
    );
    let ctx = SlotCtx::read(&s.params, Op::A);
    let addr = |i| view::view(&FILTER, i, &ctx).addr().unwrap();
    assert_eq!(addr(1).param, FilterParams::VOICE);
    assert_eq!(addr(2).block, BlockRef::VoiceFx);
    assert_eq!(addr(2).param, VoiceFxParams::TYPE);
    assert_eq!(addr(3).param, VoiceFxParams::PLACE);
}

#[test]
fn flt_mod_holds_the_routes_and_jacks() {
    let clean = sound(FilterKind::Svf, FxType::Off);
    assert_eq!(
        labels(&FILTER_MOD, &clean),
        ["FM1", "FM2", "FM3", "FM3 IN", "QM", "QM IN"]
    );
    let k2040 = sound(FilterKind::K2040, FxType::Off);
    assert_eq!(
        labels(&FILTER_MOD, &k2040),
        ["ENV AMT", "KEYBD", "FM3", "FM3 IN", "QM", "QM IN"]
    );
    let ctx = SlotCtx::read(&k2040.params, Op::A);
    for i in [0, 1] {
        assert!(
            matches!(view::view(&FILTER_MOD, i, &ctx), View::Route { .. }),
            "{i}"
        );
    }
}

#[test]
fn flt_fx_reads_input_then_the_types_pots_then_mix() {
    let soft = sound(FilterKind::Svf, FxType::Soft);
    assert_eq!(
        labels(&FILTER_FX, &soft),
        ["INPUT", "DRIVE", "TONE", "LEVEL", "MIX", "--"]
    );
    let fold = sound(FilterKind::Svf, FxType::Fold);
    assert_eq!(
        labels(&FILTER_FX, &fold),
        ["INPUT", "FOLD", "SYMMETRY", "LEVEL", "MIX", "--"]
    );
    // The pots are P1–P3 whatever the type.
    let ctx = SlotCtx::read(&fold.params, Op::A);
    for (i, id) in [VoiceFxParams::P1, VoiceFxParams::P2, VoiceFxParams::P3]
        .into_iter()
        .enumerate()
    {
        let a = view::view(&FILTER_FX, i + 1, &ctx).addr().unwrap();
        assert_eq!((a.block, a.param), (BlockRef::VoiceFx, id));
    }
    // INPUT is the filter's LEVEL.
    let a = view::view(&FILTER_FX, 0, &ctx).addr().unwrap();
    assert_eq!((a.block, a.param), (BlockRef::Filter, FilterParams::LEVEL));
    // OFF: dashes for the pots, MIX dimmed.
    let off = sound(FilterKind::Svf, FxType::Off);
    let ctx = SlotCtx::read(&off.params, Op::A);
    for i in 1..=3 {
        assert_eq!(view::view(&FILTER_FX, i, &ctx), View::Empty, "{i}");
    }
    let mix = view::view(&FILTER_FX, 4, &ctx);
    assert_eq!(mix.label(), "MIX");
    assert!(view::is_dimmed(&mix, &off, &ctx));
    let ctx = SlotCtx::read(&soft.params, Op::A);
    assert!(!view::is_dimmed(
        &view::view(&FILTER_FX, 4, &ctx),
        &soft,
        &ctx
    ));
}

/// Full words where they fit (the full-word-labels rule): only SYMMETRY
/// outruns its bar, and its cell reads SYM. Every short fits a column.
#[test]
fn the_fx_labels_fit() {
    assert!(fx_panel::cells(FxType::Off).is_none());
    for t in FxType::LIST {
        let Some(cells) = fx_panel::cells(t) else {
            continue;
        };
        for c in cells {
            // The cell shows the word where it fits its bar, else the short
            // (ADR 0053, `DENSITY`'s `DENS`): SYMMETRY reads SYM on the
            // cell and in full in the focus band.
            let bar = |s| draw::text_width(&theme::FONT_LABEL, s, theme::LABEL_TRACKING);
            let shown = if bar(c.label) <= theme::CELL_BAR_W {
                c.label
            } else {
                c.short
            };
            assert!(bar(shown) <= theme::CELL_BAR_W, "{t:?} {}", c.label);
            let w = draw::text_width(&theme::FONT_LABEL, c.short, 0);
            assert!(w <= mod_grid::HEADER_MAX_W, "{t:?} {}: {w}px", c.short);
            assert_eq!(fx_panel::cell_of(t, c.id), Some(c));
        }
    }
    assert!(fx_panel::cell_of(FxType::Soft, VoiceFxParams::MIX).is_none());
}

/// AMP is the sends and VCA VEL: no fold, nothing on a retired block.
#[test]
fn amp_has_no_fold() {
    assert_eq!(AMP.id, 9);
    for s in AMP.params {
        if let SlotBinding::Param(a) = s.binding {
            assert!(a.block != BlockRef::VoiceFx, "{a:?}");
        }
    }
}

#[test]
fn chains_have_no_drv() {
    let map = |ct| -> Vec<&str> {
        chain_def_for(ct)
            .blocks
            .iter()
            .map(|b: &ChainBlock| b.map.unwrap_or(b.def.short))
            .collect()
    };
    for ct in EngineType::ALL {
        let c = chain_def_for(ct);
        for b in c.blocks {
            assert_ne!(b.def.short, "DRV", "{ct:?}");
            for d in b.sub_pages {
                assert_ne!(d.short, "DRV", "{ct:?}");
            }
        }
    }
    assert_eq!(map(EngineType::Va), ["VA", "FLT", "AMP", "MOD"]);
    assert_eq!(map(EngineType::Algo), ["ALG", "OSC", "FLT", "AMP", "MOD"]);
    assert_eq!(map(EngineType::Tape), ["SRC", "DECK", "FLT", "AMP", "MOD"]);
    assert_eq!(map(EngineType::Modal), ["EXC", "RES", "FLT", "AMP", "MOD"]);
    // FLT's pages: the front, MOD and FX; FX in DRV's freed id.
    let flt = chain_def_for(EngineType::Va)
        .blocks
        .iter()
        .find(|b| b.def.id == FILTER.id)
        .unwrap();
    let subs: Vec<u16> = flt.sub_pages.iter().map(|d| d.id).collect();
    assert_eq!(subs, [FILTER_MOD.id, FILTER_FX.id]);
    assert_eq!(
        (FILTER_FX.id, FILTER_FX.name, FILTER_FX.short),
        (8, "FX", "FX")
    );
}

mod common;
mod screen;

use chimera_core::ui::components::{self, Cell, Look, MiniGlyph};
use chimera_core::ui::region::RegionKind;
use screen::{Fb, settle, stack_ui};

/// The ACCENT pixels' mean column in `x0..x1`.
fn accent_centroid(fb: &Fb, x0: i32, x1: i32, y0: i32, y1: i32) -> Option<f32> {
    let xs: Vec<i32> = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) == theme::ACCENT)
        .map(|(x, _)| x)
        .collect();
    (!xs.is_empty()).then(|| xs.iter().sum::<i32>() as f32 / xs.len() as f32)
}

#[test]
fn the_place_cell_draws_its_mini_glyph() {
    let x = theme::MARGIN_X;
    let draw = |mini| {
        let mut fb = Fb::new();
        let c = Cell {
            label: "PLACE",
            text: "POST",
            value: 1.0,
            fmt: chimera_core::ui::page::ValFmt::Uni,
            active: false,
            mod_amount: None,
            look: Look::Live,
            motion: None,
            mini,
        };
        components::cell(
            &mut fb,
            0,
            theme::CELL_LABEL_Y,
            Some(&c),
            theme::TextSize::Normal,
        );
        let (y0, y1) = (
            theme::CELL_LABEL_Y + 4,
            theme::CELL_LABEL_Y + theme::CELL_ROW_H,
        );
        accent_centroid(&fb, x, x + theme::CELL_COL_W, y0, y1)
    };
    let (pre, post) = (
        draw(Some(MiniGlyph::Place { post: false })).expect("PRE lit"),
        draw(Some(MiniGlyph::Place { post: true })).expect("POST lit"),
    );
    assert!(pre < post);
    assert_eq!(draw(None), None, "the text cell has no accent");
}

#[test]
fn fx_band_redraws_when_type_or_place_changes() {
    let mut ui = stack_ui();
    screen::on_flt_fx(&mut ui, FilterKind::Svf, FxType::Soft);
    let mut fb = Fb::new();
    let (perf, scope) = (
        chimera_core::ui::perf::PerfStats::zero(),
        screen::scope_fixture(),
    );
    let mut key = |ui: &mut chimera_core::ui::UiState| {
        settle(ui);
        ui.render_dirty_with_audio(&mut fb, &perf, None, &scope);
        ui.drawn_key(RegionKind::Viz).expect("the viz is drawn")
    };
    let base = key(&mut ui);
    ui.params_mut().fx.place = chimera_core::dsp::voice_fx::params::Place::Post;
    let post = key(&mut ui);
    assert_ne!(base, post, "PLACE");
    ui.params_mut().fx.set_type(FxType::Fold);
    let fold = key(&mut ui);
    assert_ne!(post, fold, "TYPE");
    // The curve follows the pots: P1 is in the region key.
    ui.params_mut().fx.pots[0] = 0.9;
    assert_ne!(fold, key(&mut ui), "P1");
}

/// The door's rows: OFF traces the sine itself, FUZZ's curve follows SUSTAIN.
#[test]
fn the_fx_rows_are_the_set_curve_and_off_is_the_line() {
    use chimera_core::dsp::voice_fx::look::{COLS, trace};
    let rows = |t, p| {
        let mut r = [[0i8; COLS]; 2];
        assert!(trace(t, p, &mut r));
        r
    };
    let off = rows(FxType::Off, [0.5; 3]);
    assert_eq!(off[0], off[1]);
    let (lo, hi) = (
        rows(FxType::Fuzz, [0.2, 0.5, 0.5]),
        rows(FxType::Fuzz, [1.0, 0.5, 0.5]),
    );
    assert_eq!(lo[0], hi[0]);
    assert_ne!(lo[1], hi[1]);
}
