//! SETTINGS screens: the leaf's bands, the legends, the breadcrumb, the
//! marks and the list's scroll.

use chimera_core::project::PartId;
use chimera_core::ui::page::PageLayout;
use chimera_core::ui::region::{RegionKind, settings_regions};
use chimera_core::ui::settings::view::{Crumbs, LegendFor, VISIBLE_ROWS, first_visible, legend};
use chimera_core::ui::{draw, theme};

const ALL_LEGENDS: [LegendFor; 8] = [
    LegendFor::Opens,
    LegendFor::Action,
    LegendFor::Later,
    LegendFor::Leaf,
    LegendFor::Prompt,
    LegendFor::Naming,
    LegendFor::ManageList,
    LegendFor::ManageCommands,
];

#[test]
fn settings_leaf_never_draws_the_map() {
    let bands = settings_regions(Some(PageLayout::CellGrid));
    let has = |k| bands.iter().any(|b| b.0 == k);
    assert!(has(RegionKind::Crumbs) && has(RegionKind::Footer));
    assert!(!has(RegionKind::Header) && !has(RegionKind::Nav));
    let cells = bands.iter().find(|b| b.0 == RegionKind::Cells).unwrap();
    assert_eq!(cells.2, 266);
}

const P1: PartId = PartId::ALL[0];
const P2: PartId = PartId::ALL[1];

#[test]
fn legends_read_as_the_spec_says() {
    let copy = |l| [legend(l, false), legend(l, true)];
    let back_close = |a: &str| [format!("{a} · MENU BACK"), format!("{a} · MENU CLOSE")];
    assert_eq!(copy(LegendFor::Opens), back_close("EDIT OPEN"));
    assert_eq!(copy(LegendFor::Action), back_close("SEQ RUN"));
    assert_eq!(copy(LegendFor::Later), back_close("LATER"));
    for (l, s) in [
        (LegendFor::Leaf, "A-F EDIT · MENU BACK"),
        (LegendFor::Prompt, "A PICK · SEQ OK · MENU CANCEL"),
        (LegendFor::Naming, "SEQ SAVE · MENU CANCEL"),
        (LegendFor::ManageList, "EDIT COMMANDS · MENU BACK"),
        (LegendFor::ManageCommands, "SEQ RUN · MENU LIST"),
    ] {
        assert_eq!(copy(l), [s, s], "{l:?}");
    }
}

#[test]
fn part_crumb_names_the_active_part() {
    assert_eq!(format!("{}", Crumbs::of(&[1], P2)), "SETTINGS › PART 2");
    assert_eq!(
        format!("{}", Crumbs::of(&[1, 2], P2)),
        "SETTINGS › PART 2 › SAVE TO"
    );
}

#[test]
fn every_legend_fits() {
    for l in ALL_LEGENDS {
        for top in [false, true] {
            let s = legend(l, top);
            let w = draw::text_width(&theme::FONT_LABEL, s, 0);
            assert!(w <= 216, "{l:?} top={top}: {s} is {w} px");
        }
    }
}

#[test]
fn breadcrumb_drops_leading_parts_behind_dots() {
    // SETTINGS › AUDIO ROUTING › OUTPUTS
    let c = Crumbs::of(&[5, 0], P1).within(120);
    let line = format!("{c}");
    assert!(line.starts_with(".."), "{line}");
    assert!(line.ends_with("› OUTPUTS"), "{line}");
    assert_eq!(
        format!("{}", Crumbs::of(&[5, 0], P1)),
        "SETTINGS › AUDIO › OUTPUTS"
    );
}

#[test]
fn marks_have_width() {
    assert_eq!(draw::text_width(&theme::FONT_LABEL, "›", 0), 5);
    assert_eq!(draw::text_width(&theme::FONT_LABEL, "●", 0), 6);
    assert_eq!(draw::text_width(&theme::FONT_LABEL, "◦", 0), 6);
}

#[test]
fn first_visible_keeps_the_bar_on_screen() {
    for bar in 0..40 {
        for len in 0..40 {
            for prev in 0..40 {
                let first = first_visible(bar, len, prev);
                let at = format!("bar {bar} len {len} prev {prev}: first {first}");
                assert!((first..first + VISIBLE_ROWS).contains(&bar), "{at}");
                if bar < len {
                    let last_first = len.saturating_sub(VISIBLE_ROWS);
                    assert!(first <= last_first, "{at}");
                    if (prev..prev + VISIBLE_ROWS).contains(&bar) && prev <= last_first {
                        assert_eq!(first, prev, "{at}");
                    }
                }
            }
        }
    }
}
