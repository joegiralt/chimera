//! The prompt panel and NAMING (settings spec § Screens): A picks, SEQ
//! confirms and MENU cancels, both on release; B*n* drops either and acts.

mod screen;

use chimera_core::name::ProjectName;
use chimera_core::params::EngineType;
use chimera_core::project::{Line, PartFrom, PartId, PartSet, PartSource, SlotId};
use chimera_core::storage::ProjectId;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::hold::{HoldGates, Presses};
use chimera_core::ui::nav::Location;
use chimera_core::ui::page::PageLayout;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::region::{MAX_REGIONS, PROMPT, RegionKind, layout_regions, settings_regions};
use chimera_core::ui::settings::naming::{Naming, NamingOut, proposed_name};
use chimera_core::ui::settings::prompt::{Answer, Choice, Count, Pick, Wording, fits};
use chimera_core::ui::{UiState, theme};
use chimera_hal::{ButtonId, EncoderId};
use screen::*;

const NO_PRESS: Presses = Presses {
    menu: None,
    seq: None,
};

fn turn(e: EncoderId, d: i8) -> Input {
    Input::turn(e, d)
}

#[test]
fn prompt_picks_with_a_and_clamps() {
    let mut c = Choice::new(Count::Three);
    assert_eq!(c.pick(), Pick::First);
    assert_eq!(c.input(&turn(EncoderId::A, 5), &NO_PRESS), None);
    assert_eq!(c.pick(), Pick::Third);
    c.input(&turn(EncoderId::A, -9), &NO_PRESS);
    assert_eq!(c.pick(), Pick::First);
    let mut two = Choice::new(Count::Two);
    two.input(&turn(EncoderId::A, 5), &NO_PRESS);
    assert_eq!(two.pick(), Pick::Second);
}

#[test]
fn seq_confirms_and_menu_cancels_on_release() {
    let mut gates = HoldGates::new();
    let mut c = Choice::new(Count::Three);
    c.input(&turn(EncoderId::A, 1), &NO_PRESS);
    let mut frame = |c: &mut Choice, i: Input| {
        let p = gates.step(&i);
        c.input(&i, &p)
    };
    assert_eq!(frame(&mut c, Input::press(ButtonId::Seq).at(0)), None);
    assert_eq!(
        frame(&mut c, Input::release(ButtonId::Seq).at(100)),
        Some(Answer::Pick(Pick::Second))
    );
    assert_eq!(frame(&mut c, Input::press(ButtonId::Menu).at(200)), None);
    assert_eq!(
        frame(&mut c, Input::release(ButtonId::Menu).at(300)),
        Some(Answer::Cancel)
    );
}

/// Part 1 edited, then a replace from slot 5 asks.
fn asking() -> (UiState, chimera_core::preset::Sound) {
    asking_on(|_| {})
}

fn asking_on(go: impl FnOnce(&mut UiState)) -> (UiState, chimera_core::preset::Sound) {
    let mut ui = UiState::new();
    go(&mut ui);
    ui.params_mut().filter.cutoff *= 0.5;
    let before = ui.project().part(PartId::ALL[0]).sound.clone();
    let src = PartSource {
        part: PartId::ALL[0],
        from: PartFrom::Slot(SlotId::ALL[4]),
    };
    assert!(ui.ask_replace_for_test(src));
    assert!(ui.prompt_open());
    (ui, before)
}

#[test]
fn bn_drops_an_open_prompt() {
    let (mut ui, before) = asking();
    feed(&mut ui, Input::press(ButtonId::B3));
    assert_eq!(
        ui.location(),
        Location::part_home(PartId::ALL[2], EngineType::Algo)
    );
    assert!(!ui.prompt_open());
    assert!(ui.project().part(PartId::ALL[0]).sound.bits_eq(&before));
    // Nothing is left for a later SEQ to confirm.
    tap(&mut ui, ButtonId::Seq);
    assert!(ui.project().part(PartId::ALL[0]).sound.bits_eq(&before));
}

#[test]
fn mix_bn_drops_it_too_and_edit_bn_does_not() {
    let (mut ui, _) = asking();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    assert!(ui.prompt_open());
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B2));
    assert!(!ui.prompt_open());
    assert_eq!(ui.location().part(), Some(PartId::ALL[1]));
}

#[test]
fn a_prompt_takes_every_other_key() {
    let (mut ui, before) = asking();
    let at = ui.location();
    for b in [ButtonId::Plus, ButtonId::Minus, ButtonId::Edit] {
        tap(&mut ui, b);
    }
    hold(&mut ui, ButtonId::Menu);
    feed(&mut ui, turn(EncoderId::C, 9));
    assert_eq!(ui.location(), at);
    assert!(ui.prompt_open());
    assert!(ui.project().part(PartId::ALL[0]).sound.bits_eq(&before));
}

#[test]
fn replace_replaces_and_cancel_keeps() {
    let (mut ui, before) = asking();
    tap(&mut ui, ButtonId::Menu);
    assert!(!ui.prompt_open());
    assert!(ui.project().part(PartId::ALL[0]).sound.bits_eq(&before));

    let (mut ui, before) = asking();
    feed(&mut ui, turn(EncoderId::A, 1));
    tap(&mut ui, ButtonId::Seq);
    assert!(!ui.prompt_open());
    assert!(!ui.project().part(PartId::ALL[0]).sound.bits_eq(&before));
}

#[test]
fn the_panel_redraws_over_the_screen_beneath_and_closing_redraws_it() {
    // OSC's viz draws the live output.
    let (mut ui, _) = asking_on(to_osc);
    let mut fb = Fb::new();
    let perf = PerfStats::zero();
    let mut scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let inside = (24, 150);
    let panel = fb.at(inside.0, inside.1);
    assert_eq!(panel, theme::PANEL);
    // The viz beneath moves: it redraws, and the panel over it.
    scope.iter_mut().for_each(|s| *s = -*s * 0.5);
    let flushed = ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert!(flushed.contains(&(PROMPT.1, PROMPT.2)));
    assert_eq!(fb.at(inside.0, inside.1), panel);

    tap(&mut ui, ButtonId::Menu);
    let flushed = ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let rows: u16 = flushed.iter().map(|&(a, b)| b - a).sum();
    assert_eq!(rows, theme::SCREEN_H as u16, "the whole screen redraws");
    assert_ne!(fb.at(inside.0, inside.1), panel);
}

fn naming(s: &str) -> Naming {
    Naming::new(s)
}

fn edit(n: &mut Naming, i: Input) -> Option<NamingOut> {
    n.input(&i, &NO_PRESS)
}

fn save(n: &mut Naming) -> Option<NamingOut> {
    let p = Presses {
        menu: None,
        seq: Some(chimera_core::ui::hold::Press::Tap),
    };
    n.input(&Input::default(), &p)
}

#[test]
fn naming_edits_per_encoder() {
    let mut n = naming("DUB-042");
    edit(&mut n, turn(EncoderId::A, -10));
    assert_eq!(n.cursor(), 0);
    edit(&mut n, turn(EncoderId::B, 1));
    assert_eq!(n.text(), "EUB-042");
    edit(&mut n, turn(EncoderId::D, 1));
    assert_eq!(n.text(), "eUB-042");
    edit(&mut n, turn(EncoderId::E, 1));
    assert_eq!(n.text(), "UB-042");
    edit(&mut n, turn(EncoderId::A, 10));
    edit(&mut n, turn(EncoderId::C, 1));
    assert_eq!(n.text(), "UB-0420");
    // E left deletes before the cursor.
    edit(&mut n, turn(EncoderId::A, -1));
    edit(&mut n, turn(EncoderId::E, -1));
    assert_eq!(n.text(), "UB-020");
    // C runs 0-9, space, '-'; B keeps a letter's case.
    edit(&mut n, turn(EncoderId::C, -3));
    assert_eq!(n.text(), "UB-0-0");
    edit(&mut n, turn(EncoderId::A, -10));
    edit(&mut n, turn(EncoderId::D, 1));
    edit(&mut n, turn(EncoderId::B, -1));
    assert_eq!(n.text(), "tB-0-0");
    edit(&mut n, turn(EncoderId::F, 3));
    assert_eq!(n.text(), "tB-0-0");
}

#[test]
fn naming_trims_and_refuses_empty() {
    let mut n = naming("  AB ");
    assert_eq!(
        save(&mut n),
        Some(NamingOut::Save(ProjectName::new("AB").unwrap()))
    );
    assert_eq!(save(&mut naming("   ")), Some(NamingOut::Empty));
    let mut n = naming("A");
    edit(&mut n, turn(EncoderId::E, -1));
    assert_eq!(n.text(), "");
    assert_eq!(save(&mut n), Some(NamingOut::Empty));
    let menu = Presses {
        menu: Some(chimera_core::ui::hold::Press::Tap),
        seq: None,
    };
    assert_eq!(n.input(&Input::default(), &menu), Some(NamingOut::Cancel));
}

#[test]
fn naming_caps_at_16() {
    let mut n = naming("ABCDEFGHIJKLMNOPQRST");
    assert_eq!(n.text(), "ABCDEFGHIJKLMNOP");
    edit(&mut n, turn(EncoderId::A, 20));
    assert_eq!(n.cursor(), 15);
    edit(&mut n, turn(EncoderId::C, 1));
    assert_eq!(n.text(), "ABCDEFGHIJKLMNO0");
    let mut n = naming("AB");
    for _ in 0..20 {
        edit(&mut n, turn(EncoderId::A, 1));
        edit(&mut n, turn(EncoderId::B, 1));
    }
    assert_eq!(n.text().len(), 16);
    assert_eq!(n.cursor(), 15);
    // What isn't a name's character is dropped.
    assert_eq!(naming("A_B!C").text(), "ABC");
}

#[test]
fn proposed_names() {
    let id = |n| ProjectId::new(n).unwrap();
    assert_eq!(proposed_name(id(42)).as_str(), "DRIFT-042");
    assert_eq!(proposed_name(id(1)).as_str(), "ACID-001");
    assert_eq!(proposed_name(id(1007)).as_str(), "STATIC-007");
}

#[test]
fn every_prompt_fits() {
    let name = ProjectName::new("WWWWWWWWWWWWWWWW").unwrap();
    let (p2, p4) = (PartId::ALL[1], PartId::ALL[3]);
    let all_but = |p| {
        PartId::ALL
            .into_iter()
            .filter(|&q| q != p)
            .fold(PartSet::EMPTY, PartSet::with)
    };
    let slot = SlotId::ALL[2];
    let every = [
        Wording::Load {
            to: Some(name),
            current: name,
        },
        Wording::Load {
            to: None,
            current: name,
        },
        Wording::Replace {
            part: p2,
            to_init: false,
        },
        Wording::Replace {
            part: p2,
            to_init: true,
        },
        Wording::AlsoUses {
            first: p4,
            more: PartSet::EMPTY,
            slot,
        },
        Wording::AlsoUses {
            first: p4,
            more: all_but(p2),
            slot,
        },
        Wording::NameExists { slot, name },
        Wording::Delete { name },
        Wording::Clear { name },
        Wording::SaveOver { name },
        Wording::CardChanged,
        Wording::ClearSlot { slot },
    ];
    for w in every {
        w.with_view(|v| {
            assert!(fits(v), "{} / {} doesn't fit", v.question, v.reason);
            assert_eq!(v.options.count(), w.count());
        });
    }
    Wording::AlsoUses {
        first: p4,
        more: PartSet::EMPTY.with(PartId::ALL[4]),
        slot,
    }
    .with_view(|v| {
        assert_eq!(v.question, "P4 P5 ALSO USE SLOT 03");
        assert_eq!(v.options.as_slice(), ["UPDATE ALL", "LEAVE"]);
    });
}

#[test]
fn prompt_overlay_fits_every_region_set() {
    for l in [PageLayout::CellGrid, PageLayout::BigViz, PageLayout::Matrix] {
        assert!(layout_regions(l).len() < MAX_REGIONS, "{l:?}");
        assert!(settings_regions(Some(l)).len() < MAX_REGIONS, "{l:?}");
    }
    assert!(settings_regions(None).len() < MAX_REGIONS);
    assert_eq!(MAX_REGIONS, 6);
}

#[test]
fn naming_saves_into_what_it_renames() {
    let mut ui = UiState::new();
    tap(&mut ui, ButtonId::Menu);
    let at = ui.location();
    ui.rename_for_test(None, "  ");
    tap(&mut ui, ButtonId::Seq);
    assert!(ui.naming().is_some(), "an empty name is refused");
    assert_eq!(
        ui.step_toast(0),
        ToastStep::Show(Line::new("NAME IS EMPTY"))
    );
    feed(&mut ui, turn(EncoderId::B, 2));
    tap(&mut ui, ButtonId::Seq);
    assert!(ui.naming().is_none());
    assert_eq!(ui.project().meta().name().as_str(), "B");
    assert_eq!(ui.location(), at);

    let p2 = PartId::ALL[1];
    ui.rename_for_test(Some(p2), "PAD");
    feed(&mut ui, turn(EncoderId::C, 1));
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(ui.project().part(p2).sound.name.as_str(), "PAD0");
    ui.rename_for_test(Some(p2), "GONE");
    tap(&mut ui, ButtonId::Menu);
    assert!(ui.naming().is_none());
    assert_eq!(ui.project().part(p2).sound.name.as_str(), "PAD0");
    assert_eq!(ui.location(), at, "MENU cancels NAMING, not SETTINGS");
}

#[test]
fn naming_takes_the_list_band_and_the_legend() {
    let mut ui = UiState::new();
    tap(&mut ui, ButtonId::Menu);
    let mut fb = Fb::new();
    let perf = PerfStats::zero();
    let scope = scope_fixture();
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    let (list, footer) = (
        ui.drawn_key(RegionKind::List),
        ui.drawn_key(RegionKind::Footer),
    );
    ui.rename_for_test(None, "DUB-042");
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert_ne!(ui.drawn_key(RegionKind::List), list);
    assert_ne!(ui.drawn_key(RegionKind::Footer), footer);
    let named = ui.drawn_key(RegionKind::List);
    feed(&mut ui, turn(EncoderId::A, -1));
    ui.render_dirty_with_scope(&mut fb, &perf, &scope);
    assert_ne!(ui.drawn_key(RegionKind::List), named, "the cursor redraws");
}
