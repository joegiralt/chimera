//! Mod matrix page (UI refresh spec § Page types, #161): an amount grid
//! with every source on screen, then a one-line readout of the route.

mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::draw;
use chimera_core::ui::mod_grid::{AMOUNT_Y, CELL_H, CELL_W, GRID_BOTTOM, READOUT_Y, cell_origin};
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::renderer::{MATRIX_AMOUNT_SLOT, amount_of, amount_value};
use chimera_core::ui::theme;
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

#[test]
fn amount_display_value_round_trips() {
    for a in -127..=127i8 {
        assert_eq!(amount_of(amount_value(a)), a);
    }
    assert_eq!(amount_value(0), 0.5);
}

/// A blank frame drawn by `f`, for comparing a band of the screen.
fn drawn(bg: u16, f: impl FnOnce(&mut Fb)) -> Fb {
    let mut fb = Fb::new();
    fb.px.fill(bg);
    f(&mut fb);
    fb
}

fn rows(fb: &Fb, y0: i32, y1: i32) -> &[u16] {
    &fb.px[y0 as usize * W..y1 as usize * W]
}

/// The fixture: ENV1→CUTOFF +20, ENV1→FOLD −30, LFO1→CUTOFF +42
/// (selected). Full names, then the amount and its effect in octaves.
#[test]
fn the_readout_names_the_selected_route() {
    let fb = render("mod_matrix");
    let want = drawn(fb.px[0], |d| {
        let t = |d: &mut Fb, s: &str, x: i32| {
            x + draw::text_tracked(d, &theme::FONT_LABEL, s, x, READOUT_Y, theme::ACCENT, 1)
        };
        let x = t(d, "LFO 1", theme::MARGIN_X) + 5;
        let x = x + draw::arrow(d, x, READOUT_Y, theme::ACCENT) + 5;
        t(d, "FILTER CUTOFF", x);
        draw::text(
            d,
            &theme::FONT_VALUE,
            "+42 = +3.3 oct",
            theme::MARGIN_X,
            AMOUNT_Y,
            theme::INK,
        );
    });
    assert!(rows(&fb, GRID_BOTTOM, AMOUNT_Y + 3) == rows(&want, GRID_BOTTOM, AMOUNT_Y + 3));
}

/// The effect follows the destination's offset law: octaves for CUTOFF,
/// a percentage of the span for VCA and the linear params.
#[test]
fn the_readout_states_the_effect_in_the_destinations_units() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::params::{FilterParams, FolderParams, OutParams};
    use chimera_core::ui::fmt::FmtBuf;
    use chimera_core::ui::mod_grid::fmt_route_effect;
    let effect = |b, id, a| {
        let mut buf = FmtBuf::new();
        fmt_route_effect(&mut buf, ParamAddr::new(b, id).spec().unwrap(), a);
        buf.as_str().to_string()
    };
    let cutoff = FilterParams::CUTOFF;
    assert_eq!(effect(BlockRef::Filter, cutoff, 42), "+42 = +3.3 oct");
    assert_eq!(effect(BlockRef::Filter, cutoff, -127), "-127 = -10.0 oct");
    assert_eq!(effect(BlockRef::Filter, cutoff, 0), "0 = 0 oct");
    assert_eq!(effect(BlockRef::Out, OutParams::VCA, 64), "+64 = +50%");
    assert_eq!(effect(BlockRef::Out, OutParams::VCA, 127), "+127 = +100%");
    assert_eq!(
        effect(BlockRef::Folder, FolderParams::FOLD, -30),
        "-30 = -24%"
    );
    assert_eq!(effect(BlockRef::Folder, FolderParams::FOLD, 0), "0 = 0%");
    use chimera_core::params::PitchParams;
    let pitch = PitchParams::PITCH;
    assert_eq!(effect(BlockRef::Pitch, pitch, 32), "+32 = +6.0 st");
    assert_eq!(effect(BlockRef::Pitch, pitch, -127), "-127 = -24.0 st");
    assert_eq!(effect(BlockRef::Pitch, pitch, 0), "0 = 0 st");
    assert_eq!(
        effect(BlockRef::Pitch, PitchParams::FINE, 32),
        "+32 = +25 ct"
    );
    assert_eq!(
        effect(BlockRef::Pitch, PitchParams::FINE, -127),
        "-127 = -100 ct"
    );
    assert_eq!(effect(BlockRef::Pitch, PitchParams::FINE, 0), "0 = 0 ct");
}

/// The voice's pitch reads `VOICE PITCH` in full: `PITCH PITCH` would repeat.
#[test]
fn the_readout_names_the_voice_pitch() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::params::PitchParams;
    use chimera_core::ui::fmt::FmtBuf;
    use chimera_core::ui::mod_grid::{ModDest, block_tag, fmt_readout_dest};
    let dest = ModDest {
        addr: ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH),
        label: [0; 8],
    };
    let mut buf = FmtBuf::new();
    fmt_readout_dest(&mut buf, "LFO 1", &dest);
    assert_eq!(buf.as_str(), "VOICE PITCH");
    assert_eq!(block_tag(BlockRef::Pitch), "PIT");
}

/// A cell with no route names it and shows `--` for the amount.
#[test]
fn an_absent_route_reads_dashes() {
    let mut ui = ui_for("mod_matrix");
    feed(&mut ui, Input::turn(EncoderId::B, 1)); // LFO1 → FOLD: none
    assert!(!ui.matrix_state.is_present(1, 1));
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    let want = drawn(fb.px[0], |d| {
        let t = |d: &mut Fb, s: &str, x: i32| {
            x + draw::text_tracked(d, &theme::FONT_LABEL, s, x, READOUT_Y, theme::ACCENT, 1)
        };
        let x = t(d, "LFO 1", theme::MARGIN_X) + 5;
        let x = x + draw::arrow(d, x, READOUT_Y, theme::ACCENT) + 5;
        t(d, "FOLDER FOLD", x);
        draw::text(
            d,
            &theme::FONT_VALUE,
            "--",
            theme::MARGIN_X,
            AMOUNT_Y,
            theme::MID,
        );
    });
    assert!(rows(&fb, GRID_BOTTOM, AMOUNT_Y + 3) == rows(&want, GRID_BOTTOM, AMOUNT_Y + 3));
}

/// Cell (`ci`, `vi`) as drawn: `edge` outline, and a route's fill and amount.
fn cell_as(
    bg: u16,
    ci: usize,
    vi: usize,
    edge: embedded_graphics::pixelcolor::Rgb565,
    route: Option<&str>,
) -> Fb {
    drawn(bg, |d| {
        let (x, y) = cell_origin(ci, vi);
        draw::round_outline(d, x, y, CELL_W, CELL_H, 0, edge);
        match route {
            Some("0") => draw::text_center(
                d,
                &theme::FONT_LABEL_BOLD,
                "0",
                x + CELL_W / 2,
                y + 10,
                theme::BAR_REST,
                0,
            ),
            Some(t) => {
                draw::fill_rect(d, x + 1, y + 1, CELL_W - 2, CELL_H - 2, theme::ACCENT_SOFT);
                draw::text_center(
                    d,
                    &theme::FONT_LABEL_BOLD,
                    t,
                    x + CELL_W / 2,
                    y + 10,
                    theme::ACCENT,
                    0,
                );
            }
            None => {}
        }
    })
}

fn same_cell(a: &Fb, b: &Fb, ci: usize, vi: usize) -> bool {
    let (x, y) = cell_origin(ci, vi);
    (y..y + CELL_H).all(|y| (x..x + CELL_W).all(|x| a.at(x, y) == b.at(x, y)))
}

/// Every route prints its amount in a lit cell (0 unlit, in the rest
/// grey); an absent route is an empty outline; the cursor's is the accent.
#[test]
fn cells_print_their_amounts_and_the_cursor_is_outlined() {
    let fb = render("mod_matrix");
    let bg = fb.px[0];
    for (ci, vi, edge, route) in [
        (0, 0, theme::FAINT, Some("+20")),
        (1, 0, theme::FAINT, Some("-30")),
        (0, 3, theme::ACCENT, Some("+42")), // LFO1, fourth row
        (1, 3, theme::FAINT, None),
        (0, 7, theme::FAINT, Some("0")), // NOTE → CUTOFF, the default pitch route
    ] {
        assert!(
            same_cell(&fb, &cell_as(bg, ci, vi, edge, route), ci, vi),
            "cell {ci},{vi}"
        );
    }
}

/// All eight sources are rows at once: no vertical scroll.
#[test]
fn every_source_is_a_row_on_screen() {
    let mut ui = ui_for("mod_matrix");
    let m = &ui.matrix_state;
    assert_eq!(m.num_sources, 8);
    let fb = render("mod_matrix");
    for vi in 0..8 {
        let (_, y) = cell_origin(0, vi);
        let label =
            (y..y + CELL_H).any(|y| (theme::MARGIN_X..40).any(|x| fb.at(x, y) != theme::BG));
        assert!(label, "row {vi} labelled");
        assert_ne!(
            fb.at(cell_origin(1, vi).0, y),
            theme::BG,
            "row {vi} has cells"
        );
    }
    assert!(cell_origin(0, 7).1 + CELL_H <= GRID_BOTTOM);
    feed(&mut ui, Input::turn(EncoderId::A, 7));
    assert_eq!(ui.matrix_state.sel_row, 7);
    let before = ui.matrix_state.clone();
    feed(&mut ui, Input::turn(EncoderId::C, 3));
    let after = &ui.matrix_state;
    assert_eq!(
        (after.sel_row, after.sel_col, after.scroll_x, after.rev),
        (before.sel_row, before.sel_col, before.scroll_x, before.rev),
        "C does nothing on MTX"
    );
}

#[test]
fn the_amount_lerps() {
    let mut ui = ui_for("mod_matrix");
    let before = ui.renderer.anim[MATRIX_AMOUNT_SLOT].current();
    feed(&mut ui, Input::turn(EncoderId::E, 40));
    ui.update(UiTick::for_test());
    let (now, target) = (
        ui.renderer.anim[MATRIX_AMOUNT_SLOT].current(),
        ui.renderer.anim[MATRIX_AMOUNT_SLOT].target(),
    );
    assert!(before < now && now < target, "{before} < {now} < {target}");
    assert_eq!(amount_of(target), 82);
}

#[test]
fn an_empty_matrix_says_so() {
    let mut ui = chimera_core::ui::UiState::new();
    ui.project_mut()
        .edit_part(PartId::ALL[0])
        .sound
        .dest_registry
        .remove(chimera_core::modulation::CUTOFF);
    feed(&mut ui, Input::press(ButtonId::B2));
    feed(&mut ui, Input::press(ButtonId::B1)); // via Part 2: Part 1's home
    for _ in 0..5 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // → MOD: MTX
    }
    assert_eq!(
        ui.page(),
        chimera_core::ui::page::PageKey::Part {
            def: chimera_core::ui::block_registry::MOD_MATRIX.id,
            op: chimera_core::addr::Op::A
        }
    );
    assert_eq!(ui.matrix_state.num_dests, 0);
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    assert_eq!(fb.oob, 0);
    let want = drawn(fb.px[0], |d| {
        draw::text_tracked(
            d,
            &theme::FONT_LABEL,
            "NO DESTINATIONS",
            theme::MARGIN_X,
            READOUT_Y,
            theme::MID,
            1,
        );
    });
    assert!(rows(&fb, GRID_BOTTOM, READOUT_Y + 3) == rows(&want, GRID_BOTTOM, READOUT_Y + 3));
    let (x, y) = cell_origin(0, 0);
    assert_eq!(fb.at(x, y), theme::BG, "no cells");
    let mut dirty = Fb::new();
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope_fixture());
    assert!(dirty.px == fb.px);
}

#[test]
fn matrix_dirty_render_equals_full_render() {
    assert!(render("mod_matrix").px == render_dirty("mod_matrix").px);
}

/// More destinations than columns: the grid scrolls, shows `<`, and the
/// cursor stays on screen.
#[test]
fn a_wide_matrix_scrolls_with_the_cursor() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::ui::mod_grid::{MAX_DESTS, MatrixState, ModDest, draw_grid};
    let mut m = MatrixState::new();
    m.rebuild_sources(&["ENV", "LFO"]);
    for (i, dest) in m.dests.iter_mut().enumerate().take(MAX_DESTS) {
        *dest = Some(ModDest {
            addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)),
            label: [b'X'; 8],
        });
    }
    m.num_dests = MAX_DESTS;
    m.move_col(MAX_DESTS as i8 - 1);
    assert_eq!(m.scroll_x, MAX_DESTS - m.visible_cols());
    let mut fb = Fb::new();
    draw_grid(&mut fb, &m, m.current_amount(), 0);
    assert_eq!(fb.oob, 0);
    let (x, y) = cell_origin(m.visible_cols() - 1, 0);
    assert_eq!(
        fb.at(x, y),
        theme::ACCENT,
        "cursor in the last visible column"
    );
}

/// The grid stays put until the cursor passes the last visible column, then
/// follows it one column at a time, and back.
#[test]
fn sideways_scroll_starts_past_the_last_visible_column() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::ui::mod_grid::{MatrixState, ModDest};
    let mut m = MatrixState::new();
    m.rebuild_sources(&["ENV"]);
    for (i, dest) in m.dests.iter_mut().enumerate().take(8) {
        *dest = Some(ModDest {
            addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)),
            label: [0; 8],
        });
    }
    m.num_dests = 8;
    let vis = m.visible_cols();
    let mut seen = vec![];
    for _ in 0..8 {
        seen.push(m.scroll_x);
        m.move_col(1);
    }
    assert_eq!(seen, [0, 0, 0, 0, 0, 1, 2, 3]);
    assert_eq!(m.scroll_x, 8 - vis);
    m.move_col(-(vis as i8));
    assert_eq!((m.sel_col, m.scroll_x), (2, 2));
}

/// Scrolling sideways (encoder D) drags the cursor along when its column
/// would leave the screen, so the readout never names a hidden cell; a
/// cursor still on screen stays put.
#[test]
fn scrolling_sideways_keeps_the_cursor_on_screen() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::ui::mod_grid::{MatrixState, ModDest};
    let mut m = MatrixState::new();
    m.rebuild_sources(&["ENV"]);
    for (i, dest) in m.dests.iter_mut().enumerate().take(8) {
        *dest = Some(ModDest {
            addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)),
            label: [0; 8],
        });
    }
    m.num_dests = 8;
    let vis = m.visible_cols();
    m.move_col(2);
    m.scroll_h(1);
    assert_eq!((m.sel_col, m.scroll_x), (2, 1), "still on screen: stays");
    m.scroll_h(10);
    assert_eq!((m.sel_col, m.scroll_x), (3, 3), "left edge pulls it right");
    m.move_col(4);
    assert_eq!(m.sel_col, 7);
    m.scroll_h(-10);
    assert_eq!(
        (m.sel_col, m.scroll_x),
        (vis - 1, 0),
        "right edge pulls it left"
    );
    let mut empty = MatrixState::new();
    empty.rebuild_sources(&["ENV"]);
    empty.scroll_h(3);
    assert_eq!((empty.sel_col, empty.scroll_x), (0, 0), "no destinations");
}

/// Every modulatable destination's column header fits `HEADER_MAX_W`
/// without clipping (a `short` where the label is too wide) and reads apart
/// from the others in its block.
#[test]
fn every_destination_header_fits_and_is_unique_in_its_block() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::ui::mod_grid::{HEADER_MAX_W, fit_header};
    for b in BlockRef::ALL {
        let mut seen = Vec::new();
        for spec in b.specs() {
            if !ParamAddr::new(b, spec.id).modulatable() {
                continue;
            }
            let full = spec.short.unwrap_or(spec.label);
            let w = draw::text_width(&theme::FONT_LABEL, full, 0);
            assert!(w <= HEADER_MAX_W, "{b:?} {full} is {w}px");
            assert_eq!(fit_header(spec), full, "{b:?} {full}: not clipped");
            assert!(!seen.contains(&full), "{b:?}: two {full} columns");
            seen.push(full);
        }
    }
}

/// Issue #11: `adjust_amount` must not write past `num_dests`, independent
/// of how the cursor got there. `load_matrix`'s `clamp_cursor` is the fix a
/// user actually hits (see `priming_after_a_stale_cursor_does_not_inherit_a_phantom_amount`
/// in ui_routing_test.rs); this is the belt-and-suspenders check on
/// `adjust_amount` itself, direct on `MatrixState` since no button sequence
/// can leave the cursor stale here to exercise it through `UiState`.
#[test]
fn adjust_amount_is_a_no_op_past_num_dests() {
    use chimera_core::ui::mod_grid::MatrixState;
    let mut m = MatrixState::new();
    m.rebuild_sources(&["ENV", "LFO"]);
    m.num_dests = 0;
    m.sel_col = 1; // stale: no destination at this column
    m.adjust_amount(50);
    assert_eq!(
        m.amounts[0][1], 0,
        "no destination at this column -- must not write"
    );
}

/// Issue #11 fix round 1: `clamp_cursor`'s `scroll_x` must follow the same
/// rule as `scroll_h` (`num_dests.saturating_sub(visible_cols())`), not
/// `num_dests - 1`, so destinations that fit on screen are not left
/// scrolled out of view.
#[test]
fn clamp_cursor_does_not_hide_columns_that_fit_on_screen() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::ui::mod_grid::{MatrixState, ModDest};
    let mut m = MatrixState::new();
    m.rebuild_sources(&["ENV", "LFO"]);
    for (i, dest) in m.dests.iter_mut().enumerate().take(8) {
        *dest = Some(ModDest {
            addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)),
            label: [b'X'; 8],
        });
    }
    m.num_dests = 8;
    m.move_col(7); // scroll right: scroll_x lands at 8 - visible_cols()
    assert_eq!(m.scroll_x, 8 - m.visible_cols());

    // Switch to a Part with only 2 destinations -- both fit on screen.
    m.num_dests = 2;
    m.clamp_cursor();
    assert_eq!(
        m.scroll_x, 0,
        "both destinations fit on screen -- must not stay scrolled"
    );
    assert_eq!(m.sel_col, 1);
}

/// The stats line must not overflow the 32-byte `FmtBuf`
/// at the worst case — `MAX_MOD_SOURCES` × `MAX_DESTS` routes, `MAX_DESTS`
/// of `MAX_DESTS` destinations. The original `"{} OF {} DESTINATIONS"`
/// wording produced `"128 ROUTES   16 OF 16 DESTINATIONS"` (34 bytes),
/// silently truncated by `FmtBuf`. The shortened wording must fit and must
/// not be truncated.
#[test]
fn stats_line_fits_untruncated_at_max_counts() {
    use chimera_core::modulation::MAX_MOD_SOURCES;
    use chimera_core::ui::fmt::FmtBuf;
    use chimera_core::ui::mod_grid::{MAX_DESTS, fmt_stats};

    let max_routes = MAX_MOD_SOURCES * MAX_DESTS;
    let mut buf = FmtBuf::new();
    fmt_stats(&mut buf, max_routes, MAX_DESTS);

    let want = format!(
        "{} ROUTES   {} OF {} DEST",
        max_routes, MAX_DESTS, MAX_DESTS
    );
    assert!(
        want.len() <= 32,
        "test's own expectation must fit FmtBuf: {} bytes",
        want.len()
    );
    assert_eq!(
        buf.as_str(),
        want,
        "stats line must be produced in full, not truncated"
    );
}

/// The same worst case, rendered: `draw_grid` must not panic or draw outside
/// the screen when every cell of a full matrix is routed.
#[test]
fn stats_line_renders_at_max_counts_without_overflow() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::modulation::MAX_MOD_SOURCES;
    use chimera_core::ui::mod_grid::{MAX_DESTS, MatrixState, ModDest, draw_grid};

    let mut m = MatrixState::new();
    let names: [&'static str; MAX_MOD_SOURCES] = ["S0", "S1", "S2", "S3", "S4", "S5", "S6", "S7"];
    m.rebuild_sources(&names);
    for (i, dest) in m.dests.iter_mut().enumerate().take(MAX_DESTS) {
        *dest = Some(ModDest {
            addr: ParamAddr::new(BlockRef::Filter, ParamId(i as u8 % 6)),
            label: [b'X'; 8],
        });
    }
    m.num_dests = MAX_DESTS;
    for r in 0..m.num_sources {
        for c in 0..m.num_dests {
            m.amounts[r][c] = 1;
        }
    }
    let mut fb = Fb::new();
    draw_grid(&mut fb, &m, m.current_amount(), 0);
    assert_eq!(fb.oob, 0);
}

/// The selected cell's amount counts up with the lerp, frame by frame —
/// never a jump to the new value — and a dirty render matches a full
/// render on every frame of the way.
#[test]
fn the_selected_amount_lerps() {
    let mut ui = ui_for("mod_matrix");
    let mut dirty = Fb::new();
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope_fixture());
    let full = |ui: &chimera_core::ui::UiState| {
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        fb
    };
    let shows =
        |fb: &Fb, t: &str| same_cell(fb, &cell_as(fb.px[0], 0, 3, theme::ACCENT, Some(t)), 0, 3);
    assert!(shows(&full(&ui), "+42"));
    feed(&mut ui, Input::turn(EncoderId::E, 85)); // +42 → +127
    let mut shown = vec![];
    for frame in 0..60 {
        ui.update(UiTick::for_test());
        ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope_fixture());
        let fb = full(&ui);
        assert!(
            dirty.px == fb.px,
            "frame {frame}: dirty render == full render"
        );
        let a = amount_of(ui.renderer.anim[MATRIX_AMOUNT_SLOT].current());
        assert!(
            shows(&fb, &format!("{a:+}")),
            "frame {frame}: cell prints {a:+}"
        );
        shown.push(a);
    }
    assert!(
        shown.windows(2).all(|w| w[0] <= w[1]),
        "monotonic: {shown:?}"
    );
    assert!(shown[0] < 127 && shown.contains(&127), "{shown:?}");
    assert!(
        shown.iter().any(|&a| 42 < a && a < 127),
        "intermediate: {shown:?}"
    );
}

/// Grid and readout keys (`MatrixState.rev`, cursor, scroll): every matrix
/// edit — cursor, amount, MIX+MINUS delete, MIX+PLUS prime, a sideways
/// scroll — redraws what changed, so a dirty render equals a full one.
#[test]
fn matrix_edits_redraw_through_the_region_keys() {
    let mut ui = ui_for("mod_matrix");
    let mut dirty = Fb::new();
    let check = |ui: &mut chimera_core::ui::UiState, dirty: &mut Fb, what: &str| {
        settle(ui);
        ui.render_dirty_with_scope(dirty, &PerfStats::zero(), &scope_fixture());
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        assert!(dirty.px == fb.px, "{what}: dirty render == full render");
        fb
    };
    let before = check(&mut ui, &mut dirty, "start");
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    let moved = check(&mut ui, &mut dirty, "cursor");
    assert!(
        rows(&before, GRID_BOTTOM, READOUT_Y + 3) != rows(&moved, GRID_BOTTOM, READOUT_Y + 3),
        "readout follows the cursor"
    );
    feed(&mut ui, Input::turn(EncoderId::E, 12));
    let set = check(&mut ui, &mut dirty, "amount");
    feed(&mut ui, Input::turn(EncoderId::E, 30));
    ui.update(UiTick::for_test()); // mid-lerp: the readout's amount alone moves
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope_fixture());
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    assert!(
        dirty.px == fb.px,
        "amount lerp: dirty render == full render"
    );
    assert!(
        rows(&set, READOUT_Y + 3, AMOUNT_Y + 3) != rows(&fb, READOUT_Y + 3, AMOUNT_Y + 3),
        "the readout's amount follows the lerp"
    );
    check(&mut ui, &mut dirty, "amount settled");
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert!(!ui.matrix_state.is_present(1, 1), "MIX+MINUS deletes");
    let fb = check(&mut ui, &mut dirty, "delete");
    assert!(same_cell(
        &fb,
        &cell_as(fb.px[0], 1, 3, theme::ACCENT, None),
        1,
        3
    ));
    // MIX+PLUS on DRV's three knobs and FLT's C: six columns, one past
    // the visible five, so the grid scrolls sideways.
    let prime = |ui: &mut chimera_core::ui::UiState, enc| {
        feed(ui, Input::turn(enc, 1));
        feed(ui, Input::chord(ButtonId::Mix, ButtonId::Plus));
    };
    let dests = ui.matrix_state.num_dests;
    for _ in 0..3 {
        feed(&mut ui, Input::press(ButtonId::Minus)); // → DRV
    }
    for enc in [EncoderId::A, EncoderId::B, EncoderId::C] {
        prime(&mut ui, enc);
    }
    feed(&mut ui, Input::press(ButtonId::Plus)); // → FLT
    prime(&mut ui, EncoderId::C);
    for _ in 0..2 {
        feed(&mut ui, Input::press(ButtonId::Plus)); // → MTX
    }
    assert_eq!(ui.matrix_state.num_dests, dests + 4, "MIX+PLUS primes");
    check(&mut ui, &mut dirty, "primed");
    let col = ui.matrix_state.sel_col;
    feed(&mut ui, Input::turn(EncoderId::D, 1));
    assert_eq!(
        (ui.matrix_state.sel_col, ui.matrix_state.scroll_x),
        (col, 1)
    );
    check(&mut ui, &mut dirty, "scrolled, cursor still");
    feed(&mut ui, Input::turn(EncoderId::B, 8));
    assert!(ui.matrix_state.scroll_x > 0);
    let fb = check(&mut ui, &mut dirty, "scrolled");
    let (x, y) = cell_origin(ui.matrix_state.visible_cols() - 1, ui.matrix_state.sel_row);
    assert_eq!(fb.at(x, y), theme::ACCENT, "cursor on screen");
}

/// `fmt_route_dest` (`TAG NAME`) never truncates. Every source with every
/// modulatable destination fits the readout: the full block name where it
/// fits (`ENV 3 → FILTER CUTOFF`), else the tag, the param name whole.
#[test]
fn every_route_readout_fits_its_line() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::modulation::ModSource;
    use chimera_core::ui::fmt::FmtBuf;
    use chimera_core::ui::mod_grid::{
        MatrixState, ModDest, block_tag, draw_readout, fmt_readout_dest, fmt_route_dest,
    };
    let mut full = FmtBuf::new();
    let cutoff = ModDest {
        addr: ParamAddr::new(BlockRef::Filter, chimera_core::params::FilterParams::CUTOFF),
        label: [0; 8],
    };
    fmt_readout_dest(&mut full, "ENV 3", &cutoff);
    assert_eq!(full.as_str(), "FILTER CUTOFF");
    let mut widest = 0;
    for b in BlockRef::ALL {
        for spec in b.specs() {
            let dest = ModDest {
                addr: ParamAddr::new(b, spec.id),
                label: [0; 8],
            };
            let mut buf = FmtBuf::new();
            fmt_route_dest(&mut buf, &dest);
            assert_eq!(buf.as_str(), format!("{} {}", block_tag(b), spec.label));
            if !dest.addr.modulatable() {
                continue;
            }
            for src in ModSource::ALL.iter() {
                let mut name = FmtBuf::new();
                fmt_readout_dest(&mut name, src.name(), &dest);
                assert!(name.as_str().ends_with(spec.label), "{}", name.as_str());
                let mut m = MatrixState::new();
                m.rebuild_sources(&chimera_core::ui::block_registry::PART_MOD_SOURCES);
                m.dests[0] = Some(dest);
                m.num_dests = 1;
                m.set(src.index(), 0, -127);
                m.sel_row = (0..8).find(|&r| m.row_source(r) == src.index()).unwrap();
                let mut fb = Fb::new();
                draw_readout(&mut fb, &m, -127, 0);
                assert_eq!(fb.oob, 0);
                let right = (0..theme::SCREEN_W)
                    .rev()
                    .find(|&x| (READOUT_Y - 9..=AMOUNT_Y).any(|y| fb.at(x, y) != fb.at(0, 0)))
                    .unwrap();
                assert!(
                    right < theme::SCREEN_W - theme::MARGIN_X,
                    "{} -> {} ends at x={right}",
                    src.name(),
                    name.as_str()
                );
                widest = widest.max(right);
            }
        }
    }
    assert!(widest > 0);
}

/// Issue #15 fix round 1: the `>` scroll-more hint moved from the name row
/// (`GRID_NAME_Y`) to the block-tag row (`GRID_TAG_Y`), since a destination
/// name can be as wide as `INHARM` (Modal's inharmonicity, the widest label
/// in the whole spec table) but every `block_tag()` is <= 3 characters for
/// every `BlockRef` -- so the hint can never be reached by a tag, unlike a
/// name. Renders the two halves separately so each bound comes from real
/// pixels, not an estimate:
/// - the widest tag alone: five destinations (the hint stays hidden --
///   `num_dests` fits exactly in `visible_cols()`), so only the tag's own
///   pixels land on screen;
/// - the hint alone: six destinations (one more than fits, so the hint
///   shows) with every destination empty, so the header loop draws no
///   column at all and only the hint's own pixels land on screen.
#[test]
fn scroll_hint_does_not_overlap_the_fifth_columns_tag() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::block::ParamId;
    use chimera_core::ui::mod_grid::{GRID_TAG_Y, MatrixState, ModDest, block_tag, draw_grid};

    // Widest block tag anywhere in the spec table -- the worst case for how
    // far a column-4 tag can reach towards the hint, now that both live on
    // the same (tag) row.
    let widest_tag_block = BlockRef::ALL
        .iter()
        .copied()
        .max_by_key(|&b| chimera_core::ui::draw::text_width(&theme::FONT_LABEL, block_tag(b), 0))
        .unwrap();
    let widest_tag_addr = ParamAddr::new(widest_tag_block, widest_tag_block.specs()[0].id);
    let short = ParamAddr::new(BlockRef::Part, ParamId(0)); // "LEVEL", block tag "PRT"

    let ink_at = |fb: &Fb, x: i32| {
        (GRID_TAG_Y - 9..=GRID_TAG_Y).any(|y| fb.px[y as usize * W + x as usize] != 0)
    };

    let mut m_tag = MatrixState::new();
    m_tag.rebuild_sources(&["ENV"]);
    for dest in m_tag.dests.iter_mut().take(4) {
        *dest = Some(ModDest {
            addr: short,
            label: [0; 8],
        });
    }
    m_tag.dests[4] = Some(ModDest {
        addr: widest_tag_addr,
        label: [0; 8],
    });
    m_tag.num_dests = 5;
    let mut tag_fb = Fb::new();
    draw_grid(&mut tag_fb, &m_tag, 0, 0);
    let tag_right = (0..theme::SCREEN_W)
        .rev()
        .find(|&x| ink_at(&tag_fb, x))
        .expect("the tag must draw something");

    let mut m_hint = MatrixState::new();
    m_hint.rebuild_sources(&["ENV"]);
    m_hint.num_dests = 6; // more than visible_cols(): the hint shows
    let mut hint_fb = Fb::new();
    draw_grid(&mut hint_fb, &m_hint, 0, 0);
    let hint_left = (0..theme::SCREEN_W)
        .find(|&x| ink_at(&hint_fb, x))
        .expect("the hint must draw something");

    assert!(
        tag_right < hint_left,
        "column 4's widest tag (right edge x={tag_right}) reaches the scroll hint (left edge x={hint_left})"
    );
}

/// Neighbouring column headers never touch (#22): every real spec label, as
/// the header draws it (`fit_header`), keeps at least a 2px gap to any other
/// in the next column. `CUTOFF` (41px) and `INHARM` (42px) are wider than the
/// 40px column pitch, so without clipping they touched.
#[test]
fn adjacent_column_headers_never_touch() {
    use chimera_core::addr::BlockRef;
    use chimera_core::ui::mod_grid::{GRID_NAME_Y, cell_center, fit_header};

    let mut labels: Vec<&str> = BlockRef::ALL
        .iter()
        .flat_map(|&b| b.specs().iter().map(fit_header))
        .collect();
    labels.sort_unstable();
    labels.dedup();

    // Rendered (min_x, max_x) of `label` centred at `cx`, from real pixels.
    let extent = |label: &str, cx: i32| -> (i32, i32) {
        let mut fb = Fb::new();
        chimera_core::ui::draw::text_center(
            &mut fb,
            &theme::FONT_LABEL,
            label,
            cx,
            GRID_NAME_Y,
            theme::MID,
            0,
        );
        let mut span: Option<(i32, i32)> = None;
        for y in (GRID_NAME_Y - 9)..=GRID_NAME_Y {
            for x in 0..theme::SCREEN_W {
                if fb.px[y as usize * W + x as usize] != 0 {
                    span = Some(span.map_or((x, x), |(l, r)| (l.min(x), r.max(x))));
                }
            }
        }
        span.expect("every real label must draw something")
    };

    // Extents at every visible column's x, computed once per label.
    let xs: Vec<i32> = (0..5).map(|ci| cell_center(ci, 0).0).collect();
    let extents: Vec<Vec<(i32, i32)>> = labels
        .iter()
        .map(|&label| xs.iter().map(|&cx| extent(label, cx)).collect())
        .collect();

    for ci in 0..4 {
        for (ai, &a) in labels.iter().enumerate() {
            let (_, ra) = extents[ai][ci];
            for (bi, &b) in labels.iter().enumerate() {
                let (lb, _) = extents[bi][ci + 1];
                assert!(
                    lb - ra > 2,
                    "columns {ci}/{}: {a:?} (right={ra}) touches {b:?} (left={lb})",
                    ci + 1
                );
            }
        }
    }
}

/// The widest amount keeps a pixel clear of the cell's fill on each side.
#[test]
fn the_widest_amount_fits_its_cell() {
    for a in ["-127", "+127"] {
        let w = draw::text_width(&theme::FONT_LABEL_BOLD, a, 0);
        assert!(w <= CELL_W - 4, "{a} is {w}px in a {CELL_W}px cell");
    }
}

/// Rows read envelopes, then LFOs, VELO, NOTE (the mockup's grouping);
/// the cursor walks them in that order; amounts stay by `ModSource` index.
#[test]
fn rows_read_envs_then_lfos_then_velo_and_note() {
    use chimera_core::modulation::ModSource;
    let mut ui = ui_for("mod_matrix");
    let m = &ui.matrix_state;
    let rows: Vec<_> = (0..m.num_sources)
        .map(|r| m.sources[m.row_source(r)].unwrap().name)
        .collect();
    assert_eq!(
        rows,
        [
            "ENV1", "ENV2", "ENV3", "LFO1", "LFO2", "LFO3", "VELO", "NOTE"
        ]
    );
    feed(&mut ui, Input::turn(EncoderId::A, -3)); // LFO1 → ENV1
    assert_eq!(ui.matrix_state.sel_source(), ModSource::Env1.index());
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.matrix_state.sel_source(), ModSource::Env2.index());
    feed(&mut ui, Input::turn(EncoderId::E, 10));
    let sound = &ui.project().part(PartId::ALL[0]).sound;
    let d = sound
        .mod_state
        .find(chimera_core::modulation::CUTOFF)
        .unwrap();
    assert_eq!(
        sound.mod_state.amount(ModSource::Env2.index(), d),
        10,
        "saved by source"
    );
}

/// Every row label ends at least 3 px left of the first cell.
#[test]
fn row_labels_fit_left_of_the_grid() {
    use chimera_core::ui::block_registry::PART_MOD_SOURCES;
    for s in PART_MOD_SOURCES {
        let right = theme::MARGIN_X + draw::text_width(&theme::FONT_LABEL_BOLD, s, 0);
        assert!(right + 3 <= cell_origin(0, 0).0, "{s} ends at {right}");
    }
}
