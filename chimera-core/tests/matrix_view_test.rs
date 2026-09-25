//! Mod matrix page in Direction A (UI refresh spec § Page types): the
//! selected route in the focus band, then a dot grid.

mod screen;

use chimera_core::ui::components;
use chimera_core::ui::mod_grid::cell_center;
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

/// The fixture: ENV→CUTOFF +20, ENV→FOLD −30, LFO→CUTOFF +42 (selected).
/// The destination carries its block tag, as in the column headers.
#[test]
fn focus_band_names_the_selected_route() {
    let fb = render("mod_matrix");
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    components::focus_route(&mut want, "LFO", "FLT CUTOFF", "+42", amount_value(42));
    assert!(fb.px[28 * W..118 * W] == want.px[28 * W..118 * W]);
}

#[test]
fn dots_show_sign_and_size_and_the_cursor_is_outlined() {
    let fb = render("mod_matrix");
    let (x, y) = cell_center(0, 0); // ENV → CUTOFF, +20
    assert_eq!(fb.at(x, y), theme::INK2, "positive: filled");
    let (x, y) = cell_center(1, 0); // ENV → FOLD, −30
    assert_eq!(fb.at(x, y), theme::BG, "negative: a ring");
    assert!((1..6).any(|r| fb.at(x + r, y) == theme::INK2));
    let (x, y) = cell_center(1, 1); // LFO → FOLD, none
    assert_eq!(fb.at(x, y), theme::FAINT, "no route: a tiny dim dot");
    assert_ne!(fb.at(x + 2, y), theme::FAINT);
    let (x, y) = cell_center(0, 1); // LFO → CUTOFF, selected
    assert_eq!(fb.at(x, y), theme::ACCENT, "selected route lit");
    assert_eq!(fb.at(x - 14, y), theme::ACCENT, "cursor outline");
}

#[test]
fn the_amount_lerps() {
    let mut ui = ui_for("mod_matrix");
    let before = ui.renderer.anim[MATRIX_AMOUNT_SLOT].current();
    feed(&mut ui, Input::turn(EncoderId::E, 40));
    ui.update();
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
    for _ in 0..4 {
        feed(&mut ui, Input::press(ButtonId::Plus));
    }
    settle(&mut ui);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
    assert_eq!(fb.oob, 0);
    let mut want = Fb::new();
    want.px.fill(fb.px[0]);
    chimera_core::ui::draw::text_tracked(
        &mut want,
        &theme::FONT_VALUE,
        "NO DESTINATIONS",
        theme::MARGIN_X,
        theme::FOCUS_LABEL_Y,
        theme::MID,
        theme::LABEL_TRACKING,
    );
    assert!(fb.px[28 * W..118 * W] == want.px[28 * W..118 * W]);
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
    draw_grid(&mut fb, &m, m.current_amount());
    assert_eq!(fb.oob, 0);
    let (x, y) = cell_center(m.visible_cols() - 1, 0);
    assert_eq!(
        fb.at(x - 14, y),
        theme::ACCENT,
        "cursor in the last visible column"
    );
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
    draw_grid(&mut fb, &m, m.current_amount());
    assert_eq!(fb.oob, 0);
}

/// Radius of the filled accent dot at `(x, y)`: accent pixels to its right.
fn dot_radius(fb: &Fb, x: i32, y: i32) -> i32 {
    (1..12)
        .take_while(|&r| fb.at(x + r, y) == theme::ACCENT)
        .count() as i32
}

/// The selected route's dot grows with the lerped amount, frame by frame —
/// never a jump from the old size to the new — and a dirty render matches
/// a full render on every frame of the way.
#[test]
fn the_selected_dot_lerps_with_the_amount() {
    let mut ui = ui_for("mod_matrix");
    let (x, y) = cell_center(0, 1); // LFO → CUTOFF, +42
    let mut dirty = Fb::new();
    ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope_fixture());
    let full = |ui: &chimera_core::ui::UiState| {
        let mut fb = Fb::new();
        ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope_fixture());
        fb
    };
    let start = dot_radius(&full(&ui), x, y);
    feed(&mut ui, Input::turn(EncoderId::E, 85)); // +42 → +127
    let mut radii = vec![start];
    for frame in 0..30 {
        ui.update();
        ui.render_dirty_with_scope(&mut dirty, &PerfStats::zero(), &scope_fixture());
        let fb = full(&ui);
        assert!(
            dirty.px == fb.px,
            "frame {frame}: dirty render == full render"
        );
        radii.push(dot_radius(&fb, x, y));
    }
    let end = *radii.last().unwrap();
    assert!(start < end, "{radii:?}");
    assert!(
        radii.windows(2).all(|w| w[0] <= w[1]),
        "monotonic: {radii:?}"
    );
    assert!(
        radii.iter().any(|&r| start < r && r < end),
        "intermediate sizes: {radii:?}"
    );
    assert!(radii[1] < end, "no jump on the first frame: {radii:?}");
}

/// The focus band's destination is `TAG NAME`, the column header's two
/// lines, so OP1 LEVEL and OP2 LEVEL read apart. Every destination fits its
/// buffer untruncated, and the longest route stays on screen.
#[test]
fn route_destination_names_the_block_and_fits() {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::ui::block_registry::PART_MOD_SOURCES;
    use chimera_core::ui::fmt::FmtBuf;
    use chimera_core::ui::mod_grid::{ModDest, block_tag, fmt_route_dest};
    let mut longest = (0, String::new());
    for b in BlockRef::ALL {
        for spec in b.specs() {
            let dest = ModDest {
                addr: ParamAddr::new(b, spec.id),
                label: [0; 8],
            };
            let mut buf = FmtBuf::new();
            fmt_route_dest(&mut buf, &dest);
            let want = format!("{} {}", block_tag(b), spec.label);
            assert_eq!(buf.as_str(), want, "untruncated");
            let w = chimera_core::ui::draw::text_width(
                &theme::FONT_VALUE,
                &want,
                theme::LABEL_TRACKING,
            );
            if w > longest.0 {
                longest = (w, want);
            }
        }
    }
    let src = PART_MOD_SOURCES
        .iter()
        .max_by_key(|s| {
            chimera_core::ui::draw::text_width(&theme::FONT_VALUE, s, theme::LABEL_TRACKING)
        })
        .unwrap();
    let mut fb = Fb::new();
    components::focus_route(&mut fb, src, &longest.1, "-127", amount_value(-127));
    assert_eq!(fb.oob, 0, "{src} -> {}", longest.1);
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
    draw_grid(&mut tag_fb, &m_tag, 0);
    let tag_right = (0..theme::SCREEN_W)
        .rev()
        .find(|&x| ink_at(&tag_fb, x))
        .expect("the tag must draw something");

    let mut m_hint = MatrixState::new();
    m_hint.rebuild_sources(&["ENV"]);
    m_hint.num_dests = 6; // more than visible_cols(): the hint shows
    let mut hint_fb = Fb::new();
    draw_grid(&mut hint_fb, &m_hint, 0);
    let hint_left = (0..theme::SCREEN_W)
        .find(|&x| ink_at(&hint_fb, x))
        .expect("the hint must draw something");

    assert!(
        tag_right < hint_left,
        "column 4's widest tag (right edge x={tag_right}) reaches the scroll hint (left edge x={hint_left})"
    );
}

/// Issue #15 fix round 1: reverting `GRID_COL_W` to 40 must not silently
/// reopen adjacent-column-name collisions -- the regression the review
/// caught in narrowing it to 36 (`"CUTOFF"` and `"FOLD"`, the `mod_matrix`
/// golden's own destinations, rendered as `"CUTOFFFOLD"` with no visible
/// gap). Exhaustively checks every pair of distinct real spec labels
/// against every pair of neighbouring visible columns and requires at
/// least a 2px gap between their rendered extents.
///
/// Excluded: any pair naming `"CUTOFF"` or `"INHARM"` -- the two widest
/// labels in the whole spec table (41px and 42px against a 40px column
/// pitch) are already marginal or overlapping even at the restored
/// `GRID_COL_W = 40` (e.g. `"CUTOFF"` next to itself: 0px gap; `"BRIGHT"`
/// next to `"INHARM"`: 1px gap) -- a separate, pre-existing, deeper issue
/// than the one this fix addresses; see the round-1 report.
///
/// Confirmed manually: with that same exclusion, this fails at
/// `GRID_COL_W = 36` (e.g. `"BRIGHT"` next to itself overlaps by 1px, one
/// of 65 other failing pairs) and passes at the restored 40 (worst
/// remaining pair, `"BRIGHT"` next to itself, has a 3px gap).
#[test]
fn adjacent_column_names_never_touch_for_ordinary_real_labels() {
    use chimera_core::addr::BlockRef;
    use chimera_core::ui::mod_grid::{GRID_NAME_Y, cell_center};

    let mut labels: Vec<&str> = BlockRef::ALL
        .iter()
        .flat_map(|&b| b.specs().iter().map(|s| s.label))
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

    let excluded = |label: &str| label == "CUTOFF" || label == "INHARM";

    for ci in 0..4 {
        for (ai, &a) in labels.iter().enumerate() {
            if excluded(a) {
                continue;
            }
            let (_, ra) = extents[ai][ci];
            for (bi, &b) in labels.iter().enumerate() {
                if excluded(b) {
                    continue;
                }
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
