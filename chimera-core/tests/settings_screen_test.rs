//! SETTINGS screens: the leaf's bands, the legends, the breadcrumb, the
//! marks and the list's scroll.

mod screen;

use chimera_core::orbit::scene::SceneIdx;
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
const S1: SceneIdx = SceneIdx::ALL[0];

#[test]
fn legends_read_as_the_spec_says() {
    let copy = |l| [legend(l, false), legend(l, true)];
    let back_close = |a: &str| [format!("{a} · MENU BACK"), format!("{a} · MENU CLOSE")];
    assert_eq!(copy(LegendFor::Opens), back_close("EDIT OPEN"));
    assert_eq!(copy(LegendFor::Action), back_close("EDIT RUN"));
    assert_eq!(copy(LegendFor::Later), back_close("LATER"));
    for (l, s) in [
        (LegendFor::Leaf, "A-F EDIT · MENU BACK"),
        (LegendFor::Prompt, "A PICK · EDIT OK · MENU BACK"),
        (LegendFor::Naming, "EDIT SAVE · MENU BACK"),
        (LegendFor::ManageList, "EDIT COMMANDS · MENU BACK"),
        (LegendFor::ManageCommands, "EDIT RUN · MENU LIST"),
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
        format!("{}", Crumbs::of(&[PART_ROW], P2, S1)),
        "SETTINGS › PART 2"
    );
    assert_eq!(
        format!("{}", Crumbs::of(&[PART_ROW, 2], P2, S1)),
        "SETTINGS › PART 2 › SAVE TO"
    );
}

/// PART's sixth row: SAVE TO LIBRARY, its NAMING crumbed SAVE TO LIB
/// (MIDI MAP follows it).
#[test]
fn part_list_ends_with_save_to_library() {
    use chimera_core::ui::settings::view::Crumb;
    use chimera_core::ui::settings::{Act, Kind, SAVE_TO_LIB_ROW, rows};
    let part = rows(&[PART_ROW]);
    assert_eq!(part.len(), 6 + cfg!(feature = "midi-map") as usize);
    assert_eq!(SAVE_TO_LIB_ROW, 5);
    let r = part[5];
    assert_eq!((r.label, r.crumb), ("SAVE TO LIBRARY", "SAVE TO LIB"));
    assert!(matches!(r.kind, Kind::Act(Act::SaveToLibrary)));
    let mut c = Crumbs::of(&[PART_ROW], P2, S1);
    c.push(Crumb::Name("SAVE TO LIB"));
    // Too wide whole: SETTINGS gives way to the dots.
    assert_eq!(format!("{c}"), ".. › PART 2 › SAVE TO LIB");
}

/// ORBIT's list crumb is `SCENE n`, the playing scene, as PART's is
/// `PART n`: short enough that SAVE TO keeps it at the breadcrumb's width,
/// left of the beat dot.
#[cfg(feature = "orbit")]
#[test]
fn orbit_crumb_names_the_playing_scene() {
    use chimera_core::ui::settings::rows;
    let at = rows(&[]).iter().position(|r| r.label == "ORBIT").unwrap() as u8;
    let s3 = SceneIdx::ALL[2];
    assert_eq!(
        format!("{}", Crumbs::of(&[at], P2, s3)),
        "SETTINGS › SCENE 3"
    );
    assert_eq!(
        format!("{}", Crumbs::of(&[at, 2], P2, s3)),
        ".. › SCENE 3 › SAVE TO"
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
    let c = Crumbs::of(&[5, 0], P1, S1).within(120);
    let line = format!("{c}");
    assert!(line.starts_with(".."), "{line}");
    assert!(line.ends_with("› OUTPUTS"), "{line}");
    assert_eq!(
        format!("{}", Crumbs::of(&[5, 0], P1, S1)),
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
    // The crumbs always clear ORBIT's beat dot; without ORBIT there is
    // room for SYSTEM.
    let (want, theme) = if cfg!(feature = "orbit") {
        (".. › DIAG › AUD LOAD", ".. › PERSONAL › THEME")
    } else {
        (
            ".. › SYSTEM › DIAG › AUD LOAD",
            "SETTINGS › PERSONAL › THEME",
        )
    };
    assert_eq!(audio, want);
    assert_ne!(k0, k1);
    // EDIT on a leaf steps nothing: the crumbs stay.
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(crumbs(&mut ui), (audio, k1));

    to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    assert_eq!(crumbs(&mut ui).0, theme);
}

#[test]
fn a_new_project_shows_no_status_beside_its_name() {
    assert_eq!(status_text(ProjectStatus::Pristine), None);
    assert!(status_text(ProjectStatus::Saved).is_some());
    assert!(status_text(ProjectStatus::Modified).is_some());
}

/// A leaf with nothing to edit names only MENU; one with cells keeps A-F.
#[test]
fn a_read_only_leaf_says_only_menu_back() {
    for (labels, want) in [
        (&["SYSTEM", "ABOUT"][..], "MENU BACK"),
        (&["SYSTEM", "DIAGNOSTICS", "AUDIO LOAD"][..], "MENU BACK"),
        (&["PERSONALIZE", "THEME"][..], "A-F EDIT · MENU BACK"),
        (&["MIDI CONFIG", "CHANNELS"][..], "A-F EDIT · MENU BACK"),
    ] {
        let mut ui = UiState::new();
        to_leaf(&mut ui, labels);
        assert_eq!(ui.legend_for_test(), Some(want), "{labels:?}");
    }
    #[cfg(debug_assertions)]
    {
        // DEMO's WAV edits through its page table.
        let mut ui = UiState::new();
        to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS", "DEMO", "WAV"]);
        assert_eq!(ui.legend_for_test(), Some("A-F EDIT · MENU BACK"));
        // MTX's encoders edit its grid.
        let mut ui = UiState::new();
        to_leaf(&mut ui, &["SYSTEM", "DIAGNOSTICS", "DEMO", "MTX"]);
        assert_eq!(ui.legend_for_test(), Some("A-F EDIT · MENU BACK"));
    }
}

/// Every SETTINGS breadcrumb, NAMING's last part on it too, ends left of
/// ORBIT's beat dot (`theme::CRUMBS_RIGHT`): `dropped` keeps it there
/// however deep the path, so the crumbs never move with ORBIT's transport.
#[test]
fn every_breadcrumb_clears_the_dots() {
    use chimera_core::ui::settings::tree::{Kind, rows};
    use chimera_core::ui::settings::view::{Crumb, draw_crumbs};
    use screen::W;
    fn walk(path: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        out.push(path.clone());
        for (i, r) in rows(path).iter().enumerate() {
            path.push(i as u8);
            match r.kind {
                Kind::List(_) => walk(path, out),
                _ => out.push(path.clone()),
            }
            path.pop();
        }
    }
    let mut paths = Vec::new();
    walk(&mut Vec::new(), &mut paths);
    assert!(paths.len() > 20, "{} paths", paths.len());
    // Every scene: ORBIT's crumb names the playing one.
    for (path, scene) in paths.iter().flat_map(|p| SceneIdx::ALL.map(|s| (p, s))) {
        let base = Crumbs::of(path, PartId::ALL[5], scene);
        // A crumb for each row from the top, and one for NAMING's.
        let naming: &[&str] = if path.len() + 2 <= 6 {
            &["RENAME", "SAVE AS", "SAVE TO LIB"]
        } else {
            &[]
        };
        let ends = core::iter::once(None).chain(naming.iter().copied().map(Some));
        for last in ends {
            let mut c = base;
            if let Some(n) = last {
                c.push(Crumb::Name(n));
            }
            let mut fb = Fb::new();
            draw_crumbs(&mut fb, &c);
            let right = (0..W)
                .rev()
                .find(|&x| (0..theme::HEADER_BOTTOM as usize).any(|y| fb.px[y * W + x] != 0));
            assert!(
                right.is_some_and(|r| r as i32 <= theme::CRUMBS_RIGHT),
                "{c}: ends at {right:?}, past {}",
                theme::CRUMBS_RIGHT
            );
        }
    }
}

#[cfg(feature = "midi-map")]
#[test]
fn the_map_legend_fits() {
    let s = legend(LegendFor::Map, false);
    let w = draw::text_width(&theme::FONT_LABEL, s, 0);
    assert!(w <= 216, "{s} is {w} px");
}
