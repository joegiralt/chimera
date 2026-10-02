//! SETTINGS screens: the leaf's bands, the legends, the breadcrumb, the
//! marks and the list's scroll.

mod screen;

use chimera_core::project::{PartId, ProjectStatus};
use chimera_core::ui::UiState;
use chimera_core::ui::page::PageLayout;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::region::{RegionKind, settings_regions};
use chimera_core::ui::settings::PART_ROW;
use chimera_core::ui::settings::manage::Note;
use chimera_core::ui::settings::view::{
    Crumbs, LegendFor, VISIBLE_ROWS, first_visible, legend, status_text,
};
use chimera_core::ui::{draw, theme};
use chimera_hal::ButtonId;
use screen::{Fb, Input, audio_fixture, feed, scope_fixture, to_leaf};

const ALL_LEGENDS: [LegendFor; 11] = [
    LegendFor::Opens,
    LegendFor::Action,
    LegendFor::Later,
    LegendFor::Leaf,
    LegendFor::Prompt,
    LegendFor::Naming,
    LegendFor::ManageList,
    LegendFor::ManageCommands,
    LegendFor::ManageDimmed(None),
    LegendFor::ManageDimmed(Some(Note::LoadToRename)),
    LegendFor::ManageLater,
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
        (LegendFor::ManageDimmed(None), "MENU LIST"),
        (
            LegendFor::ManageDimmed(Some(Note::LoadToRename)),
            "LOAD TO RENAME · MENU LIST",
        ),
        (LegendFor::ManageLater, "LATER · MENU LIST"),
    ] {
        assert_eq!(copy(l), [s, s], "{l:?}");
    }
}

#[test]
fn part_crumb_names_the_active_part() {
    assert_eq!(
        format!("{}", Crumbs::of(&[PART_ROW], P2)),
        "SETTINGS › PART 2"
    );
    assert_eq!(
        format!("{}", Crumbs::of(&[PART_ROW, 2], P2)),
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

/// The crumbs as drawn, and their region's key after a dirty render.
fn crumbs(ui: &mut UiState) -> (String, Option<chimera_core::ui::region::RegionData>) {
    let mut fb = Fb::new();
    let _ = ui.render_dirty_with_audio(
        &mut fb,
        &PerfStats::zero(),
        Some(&audio_fixture()),
        &scope_fixture(),
    );
    (
        ui.crumbs().unwrap().to_string(),
        ui.drawn_key(RegionKind::Crumbs),
    )
}

#[test]
fn a_leaf_ends_the_breadcrumb() {
    let mut ui = UiState::new();
    to_leaf(&mut ui, &["SYSTEM", "ABOUT"]);
    let (about, k0) = crumbs(&mut ui);
    assert_eq!(about, "SETTINGS › SYSTEM › ABOUT");
    to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS", "AUDIO LOAD"]);
    let (audio, k1) = crumbs(&mut ui);
    assert_eq!(audio, ".. › SYSTEM › DIAG › AUD LOAD");
    assert_ne!(k0, k1);
    // EDIT on a leaf steps nothing: the crumbs stay.
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(crumbs(&mut ui), (audio, k1));

    to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    assert_eq!(crumbs(&mut ui).0, "SETTINGS › PERSONAL › THEME");
}

#[test]
fn a_new_project_shows_no_status_beside_its_name() {
    assert_eq!(status_text(ProjectStatus::Pristine), None);
    assert!(status_text(ProjectStatus::Saved).is_some());
    assert!(status_text(ProjectStatus::Modified).is_some());
}
