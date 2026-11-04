//! One D-pad on every chain (#316, ADR 0081): MINUS and PLUS along it,
//! EDIT down and wrapping from the bottom, SEQ up and inert at the top.

mod screen;

use chimera_core::params::EngineType;
use chimera_core::project::{PartId, PartStatus, part_status};
use chimera_core::ui::UiState;
use chimera_core::ui::block_def::ChainDef2;
use chimera_core::ui::block_def::Move;
use chimera_core::ui::block_registry::{MIXER_CHANNEL_CHAIN, MIXER_HOME, MIXER_PART};
use chimera_core::ui::nav::{
    Column, Location, MixPage, NavCtx, NavKey, OrbitAt, OrbitCx, OrbitKey, PageAt, PartChain,
    Recall, Step, chain_def_for,
};
use chimera_core::ui::orbit::ORBIT_CHAIN;
use chimera_core::ui::settings::{Kind, rows};
use chimera_hal::ButtonId;
use screen::{Input, feed, tap};

const P: [PartId; 6] = PartId::ALL;
const FX_FIRST: usize = if MIXER_HOME > MIXER_PART {
    MIXER_HOME + 1
} else {
    MIXER_PART + 1
};

fn cx(engine: EngineType) -> NavCtx {
    NavCtx {
        chains: [PartChain::base(engine); 6],
        dyn_rows: 3,
        orbit: OrbitCx::NONE,
        // HALL: every FX sub-page shows, REV › MORE 2 (DENSITY) too.
        rev: chimera_core::dsp::reverb::ReverbType::Hall,
        // ADR 0081's keys; ROW's are `row_*` below.
        pages: chimera_core::ui::theme_settings::Pages::Stack,
        opens: Default::default(),
    }
}

/// Under a TYPE with no cell on REV › MORE 2, EDIT from REV › MORE goes
/// round to REV.
#[test]
fn edit_skips_an_empty_more_2() {
    use chimera_core::dsp::reverb::ReverbType;
    use chimera_core::ui::block_registry::{EFX, EFX_MORE, EFX_MORE2};
    for (rev, want) in [(ReverbType::Quad, EFX.id), (ReverbType::Hall, EFX_MORE2.id)] {
        let cx = NavCtx {
            rev,
            ..cx(EngineType::Algo)
        };
        let mut r = Recall::new();
        let mut l = Location::mixer(P[1], MixPage::Sends);
        let def = |l: Location| {
            MIXER_CHANNEL_CHAIN
                .def_at(l.page(&cx).unwrap().1)
                .unwrap()
                .id
        };
        while def(l) != EFX.id {
            let Step::Go(to) = l.step(NavKey::Plus, &cx, &mut r) else {
                panic!("no REV")
            };
            l = to;
        }
        let Step::Go(more) = l.step(NavKey::Edit, &cx, &mut r) else {
            panic!()
        };
        assert_eq!(def(more), EFX_MORE.id);
        let Step::Go(next) = more.step(NavKey::Edit, &cx, &mut r) else {
            panic!()
        };
        assert_eq!(def(next), want, "{rev:?}");
    }
}

/// Where a key takes a chain page: PLUS and MINUS the node, clamped; EDIT
/// the next sub-page, wrapping; SEQ the one above, none at the top.
fn expect(c: &ChainDef2, at: PageAt, k: NavKey) -> Option<PageAt> {
    let (n, s) = (at.node() as usize, at.sub() as usize);
    let to = match k {
        NavKey::Plus => c.page(n + 1, 0),
        NavKey::Minus => n.checked_sub(1).and_then(|n| c.page(n, 0)),
        NavKey::Edit => c.page(n, s + 1).or(c.page(n, 0)),
        NavKey::SeqTap => s.checked_sub(1).and_then(|s| c.page(n, s)),
        _ => unreachable!(),
    };
    to.filter(|&t| t != at)
}

/// Every page of `c`.
fn pages(c: &ChainDef2) -> Vec<PageAt> {
    (0..c.len())
        .flat_map(|n| (0..).map_while(move |s| c.page(n, s)))
        .collect()
}

const DPAD: [NavKey; 4] = [NavKey::Minus, NavKey::Plus, NavKey::Edit, NavKey::SeqTap];

#[test]
fn part_pages_move_the_same_way_on_every_engine() {
    for engine in [EngineType::Algo, EngineType::Modal] {
        let cx = cx(engine);
        let c = chain_def_for(engine);
        for at in pages(c) {
            let l = Location::pages(P[2], at);
            for k in DPAD {
                let want =
                    expect(c, at, k).map_or(Step::Stay, |a| Step::Go(Location::pages(P[2], a)));
                assert_eq!(
                    l.step(k, &cx, &mut Recall::new()),
                    want,
                    "{engine:?} {at:?} {k:?}"
                );
            }
        }
    }
}

#[test]
fn edit_wraps_from_the_bottom_sub_page() {
    for engine in [EngineType::Algo, EngineType::Modal] {
        let cx = cx(engine);
        let c = chain_def_for(engine);
        let deep = (0..c.len())
            .find(|&n| c.page(n, 1).is_some())
            .expect("a node with sub-pages");
        let subs = (0..).take_while(|&s| c.page(deep, s).is_some()).count();
        let mut l = Location::pages(P[0], c.page(deep, 0).unwrap());
        let mut r = Recall::new();
        for _ in 0..subs {
            if let Step::Go(to) = l.step(NavKey::Edit, &cx, &mut r) {
                l = to;
            }
        }
        assert_eq!(
            l,
            Location::pages(P[0], c.page(deep, 0).unwrap()),
            "{engine:?}"
        );
    }
}

#[test]
fn the_mixer_and_the_sound_rung() {
    let cx = cx(EngineType::Algo);
    let p = P[3];
    let step = |l: Location, k| l.step(k, &cx, &mut Recall::new());
    let go = Step::Go;
    let part = Location::mixer(p, MixPage::Part);
    let sends = Location::mixer(p, MixPage::Sends);
    let sound = Location::sound(p);
    let table = [
        (part, NavKey::Minus, Step::Stay),
        (part, NavKey::Plus, go(sends)),
        (part, NavKey::Edit, go(sound)),
        (part, NavKey::SeqTap, Step::Stay),
        (sends, NavKey::Minus, go(part)),
        (sends, NavKey::Edit, go(sound)),
        (sends, NavKey::SeqTap, Step::Stay),
        (sound, NavKey::Minus, go(Location::sound(P[2]))),
        (sound, NavKey::Plus, go(Location::sound(P[4]))),
        // EDIT on a row loads it: `UiState`'s.
        (sound, NavKey::Edit, Step::Stay),
        (sound, NavKey::SeqTap, go(sends)),
    ];
    for (from, k, want) in table {
        assert_eq!(step(from, k), want, "{from:?} {k:?}");
    }
    let Step::Go(fx) = step(sends, NavKey::Plus) else {
        panic!("PLUS on SENDS")
    };
    assert_eq!(fx.page(&cx).unwrap().1.node() as usize, FX_FIRST);
}

#[test]
fn the_fx_pages_move_as_a_chain() {
    let cx = cx(EngineType::Algo);
    let c = &MIXER_CHANNEL_CHAIN;
    let p = P[1];
    // Every FX page, reached by PLUS from SENDS, then EDIT.
    let mut r = Recall::new();
    let mut l = Location::mixer(p, MixPage::Sends);
    let mut fx = Vec::new();
    while let Step::Go(to) = l.step(NavKey::Plus, &cx, &mut r) {
        l = to;
        let mut sub = l;
        loop {
            fx.push(sub);
            match sub.step(NavKey::Edit, &cx, &mut r) {
                Step::Go(to) if to != l => sub = to,
                _ => break,
            }
        }
    }
    assert_eq!(fx.len(), pages(c).len() - FX_FIRST);
    for l in fx {
        let at = l.page(&cx).unwrap().1;
        for k in DPAD {
            let got = match l.step(k, &cx, &mut Recall::new()) {
                Step::Go(to) if to.on_mixer() && to.part() == Some(p) => to.page(&cx).map(|x| x.1),
                Step::Stay => None,
                s => panic!("{at:?} {k:?}: {s:?}"),
            };
            assert_eq!(got, expect(c, at, k), "{at:?} {k:?}");
        }
    }
}

/// Every SETTINGS list and leaf path, depth first.
fn settings_paths() -> Vec<Vec<u8>> {
    fn walk(path: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        for (i, r) in rows(path).iter().enumerate() {
            path.push(i as u8);
            if matches!(r.kind, Kind::List(_) | Kind::Leaf(_) | Kind::Screen(_)) {
                out.push(path.clone());
                walk(path, out);
            }
            path.pop();
        }
    }
    let mut out = vec![vec![]];
    walk(&mut Vec::new(), &mut out);
    out
}

#[test]
fn seq_backs_out_of_settings_as_menu_does_and_never_runs_a_row() {
    let cx = cx(EngineType::Algo);
    for path in settings_paths() {
        let n = rows(&path).len().max(3) as u8;
        for row in 0..n {
            let l = Location::settings_at(&path, row);
            let seq = l.step(NavKey::SeqTap, &cx, &mut Recall::new());
            match path.split_last() {
                None => assert_eq!(seq, Step::Stay, "the top: nothing"),
                Some((&last, up)) => {
                    assert_eq!(seq, Step::Go(Location::settings_at(up, last)), "{path:?}");
                    assert_eq!(
                        seq,
                        l.step(NavKey::MenuTap, &cx, &mut Recall::new()),
                        "{path:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn edit_runs_inside_a_screen() {
    let cx = cx(EngineType::Algo);
    let mut r = Recall::new();
    // PROJECT › LOAD PROJECT.
    let load = Location::settings_at(&[0, 0], 1);
    assert_eq!(load.step(NavKey::Edit, &cx, &mut r), Step::Run);
    assert_eq!(
        load.step(NavKey::Plus, &cx, &mut r),
        Step::Go(Location::settings_at(&[0, 0], 2))
    );
    // MANAGE PROJECTS: EDIT into the commands, EDIT runs one, SEQ back.
    let manage = rows(&[0])
        .iter()
        .position(|r| r.label == "MANAGE PROJECTS")
        .unwrap() as u8;
    let mut l = Location::settings_at(&[0, manage], 1);
    let list = l;
    let Step::Go(cmds) = l.step(NavKey::Edit, &cx, &mut r) else {
        panic!("into the commands")
    };
    assert_eq!(cmds.settings().unwrap().column(), Some(Column::Command(0)));
    l = cmds;
    assert_eq!(l.step(NavKey::Edit, &cx, &mut r), Step::Run);
    assert_eq!(l.step(NavKey::SeqTap, &cx, &mut r), Step::Go(list));
    assert_eq!(
        list.step(NavKey::SeqTap, &cx, &mut r),
        Step::Go(Location::settings_at(&[0], manage))
    );
}

/// Every ORBIT page, reached by its own keys.
fn orbit_places(cx: &NavCtx) -> Vec<OrbitAt> {
    let mut seen = vec![OrbitAt::HOME];
    let mut i = 0;
    while i < seen.len() {
        for k in [OrbitKey::Plus, OrbitKey::Minus, OrbitKey::Edit] {
            if let Step::Go(to) = Location::from(seen[i]).step_orbit(k, cx, &mut Recall::new())
                && let Some(o) = to.orbit()
                && !seen.contains(&o)
            {
                seen.push(o);
            }
        }
        i += 1;
    }
    seen
}

#[test]
fn orbit_moves_as_a_chain_and_seq_never_saves() {
    let cx = cx(EngineType::Algo);
    let c = &ORBIT_CHAIN;
    let places = orbit_places(&cx);
    assert_eq!(places.len(), pages(c).len());
    for o in places {
        let at = o.page();
        let l = Location::from(o);
        let step = |k| l.step_orbit(k, &cx, &mut Recall::new());
        let page = |s: Step| match s {
            Step::Go(to) => Some(to.orbit().unwrap().page()),
            Step::Stay => None,
            s => panic!("{at:?}: {s:?}"),
        };
        assert_eq!(page(step(OrbitKey::Plus)), c.step(at, Move::Next), "{at:?}");
        assert_eq!(
            page(step(OrbitKey::Minus)),
            c.step(at, Move::Prev),
            "{at:?}"
        );
        assert_eq!(page(step(OrbitKey::SeqTap)), c.step(at, Move::Up), "{at:?}");
        // EDIT on a page with sub-pages wraps them.
        if c.page(at.node() as usize, 1).is_some() {
            let n = at.node() as usize;
            assert_eq!(
                page(step(OrbitKey::Edit)),
                c.page(n, at.sub() as usize + 1).or(c.page(n, 0)),
                "{at:?}"
            );
        }
    }
}

/// MIX + EDIT on the Sound rung saves the Part's Sound, over its slot.
#[test]
fn mix_edit_in_the_browser_saves_the_sound() {
    let mut ui = UiState::new();
    let p = P[1];
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    assert!(ui.location().browse().is_some());
    ui.project_mut().edit_part(p).sound.params.filter.cutoff *= 0.5;
    let status = |ui: &UiState| part_status(ui.project().part(p), ui.project().pool());
    assert_eq!(status(&ui), PartStatus::Edited);
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Edit));
    assert_eq!(status(&ui), PartStatus::Clean, "saved");
    assert_eq!(
        ui.location().browse().map(|b| b.0),
        Some(p),
        "still browsing"
    );
}

/// Encoder B turns the Sound rung PROJECT → LIBRARY → FACTORY, clamped at
/// each end; MINUS and PLUS keep it and move between Parts, and EDIT+Part
/// opens PROJECT again.
#[test]
fn b_turns_three_tabs_and_the_browser_opens_on_project() {
    use chimera_core::ui::library::BrowserTab as Tab;
    use chimera_hal::EncoderId;
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    assert_eq!(ui.lib_tab(), Tab::Project, "PROJECT at boot");
    feed(&mut ui, Input::turn(EncoderId::B, -1));
    assert_eq!(ui.lib_tab(), Tab::Project, "clamped");
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(ui.lib_tab(), Tab::Library);
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(ui.lib_tab(), Tab::Factory);
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    assert_eq!(ui.lib_tab(), Tab::Factory, "clamped");
    assert_eq!(ui.location().browse().map(|b| b.0), Some(P[1]));
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert_eq!(ui.location().browse().map(|b| b.0), Some(P[2]), "PLUS");
    assert_eq!(ui.lib_tab(), Tab::Factory, "the tab is kept");
    tap(&mut ui, ButtonId::Menu);
    assert!(ui.location().browse().is_none(), "MENU leaves");
    while ui.in_settings() {
        tap(&mut ui, ButtonId::Menu);
    }
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    assert!(ui.location().browse().is_some());
    assert_eq!(ui.lib_tab(), Tab::Project, "opened anew on PROJECT");
}

/// MIX + EDIT in LIBRARY opens NAMING and saves nothing yet.
#[test]
fn mix_edit_in_library_opens_naming() {
    use chimera_hal::EncoderId;
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    let used = ui.project().pool().used();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Edit));
    assert_eq!(ui.naming_title_for_test(), Some("SOUND NAME"));
    assert_eq!(ui.project().pool().used(), used);
}

/// MIX + MINUS in LIBRARY has no delete: nothing asked, the pool as it was.
#[test]
fn mix_minus_in_library_clears_nothing() {
    use chimera_core::project::SlotId;
    use chimera_hal::EncoderId;
    let mut ui = UiState::new();
    let slot = SlotId::ALL
        .into_iter()
        .find(|&s| {
            let p = ui.project();
            p.pool().get(s).is_some() && p.users(s).is_empty()
        })
        .expect("a filled slot no Part plays");
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B1));
    feed(&mut ui, Input::turn(EncoderId::A, slot.index() as i8));
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    let used = ui.project().pool().used();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::Minus));
    assert_eq!(ui.prompt_kind_for_test(), None);
    assert!(ui.project().pool().get(slot).is_some(), "slot kept");
    assert_eq!(ui.project().pool().used(), used);
}

/// FLT's EDIT goes down to MOD, then FX, then JACK, and round again (ADR
/// 0155, 0190).
#[test]
fn edit_on_flt_goes_to_mod_then_fx_and_wraps() {
    use chimera_core::ui::block_registry::{FILTER, FILTER_FX, FILTER_JACKS, FILTER_MOD};
    let cx = cx(EngineType::Algo);
    let c = chain_def_for(EngineType::Algo);
    let n = (0..c.len())
        .find(|&n| c.active_def(n, 0).is_some_and(|d| d.id == FILTER.id))
        .unwrap();
    assert_eq!(c.active_def(n, 1).unwrap().id, FILTER_MOD.id);
    assert_eq!(c.active_def(n, 2).unwrap().id, FILTER_FX.id);
    assert_eq!(c.active_def(n, 3).unwrap().id, FILTER_JACKS.id);
    let pages = [0, 1, 2, 3, 0].map(|i| c.page(n, i).unwrap());
    let mut r = Recall::new();
    for w in pages.windows(2) {
        assert_eq!(
            Location::pages(P[0], w[0]).step(NavKey::Edit, &cx, &mut r),
            Step::Go(Location::pages(P[0], w[1]))
        );
    }
}

/// THEME's PAGES ROW (ADR 0145): the panel has EDIT left of SEQ, so EDIT
/// is the previous sub-page, none before the first, and SEQ the next,
/// wrapping from the last; MINUS and PLUS as under STACK.
#[test]
fn row_edit_steps_left_and_seq_right_wrapping() {
    use chimera_core::ui::theme_settings::Pages;
    for engine in [EngineType::Algo, EngineType::Modal, EngineType::Va] {
        let cx = NavCtx {
            pages: Pages::Row,
            opens: Default::default(),
            ..cx(engine)
        };
        let c = chain_def_for(engine);
        for at in pages(c) {
            let l = Location::pages(P[2], at);
            for k in DPAD {
                let stack_key = match k {
                    NavKey::Edit => NavKey::SeqTap,
                    NavKey::SeqTap => NavKey::Edit,
                    k => k,
                };
                let want = expect(c, at, stack_key)
                    .map_or(Step::Stay, |a| Step::Go(Location::pages(P[2], a)));
                assert_eq!(
                    l.step(k, &cx, &mut Recall::new()),
                    want,
                    "{engine:?} {at:?} {k:?}"
                );
            }
        }
    }
}

/// `sub_move` is the one map from PAGES and a key to a step: STACK keeps
/// ADR 0081's, ROW swaps EDIT and SEQ, and nothing else moves a page.
#[test]
fn sub_move_maps_pages_and_keys() {
    use chimera_core::ui::nav::sub_move;
    use chimera_core::ui::theme_settings::Pages::{Row, Stack};
    let cases = [
        (Stack, NavKey::Edit, Some(Move::Down)),
        (Stack, NavKey::SeqTap, Some(Move::Up)),
        (Row, NavKey::Edit, Some(Move::Up)),
        (Row, NavKey::SeqTap, Some(Move::Down)),
        (Row, NavKey::Plus, Some(Move::Next)),
        (Stack, NavKey::Minus, Some(Move::Prev)),
        (Row, NavKey::MenuTap, None),
    ];
    for (p, k, want) in cases {
        assert_eq!(sub_move(p, k), want, "{p:?} {k:?}");
    }
}

/// ORBIT under ROW: SEQ cycles RING's sub-pages and opens CUSTOM, EDIT
/// goes back; EDIT on SCN still saves.
#[test]
fn row_flips_orbit_sub_pages_but_scn_still_saves() {
    use chimera_core::ui::orbit::{ORB_NODE, RING_NODE, SCN_NODE};
    use chimera_core::ui::theme_settings::Pages;
    let cx = NavCtx {
        pages: Pages::Row,
        opens: Default::default(),
        ..cx(EngineType::Algo)
    };
    let mut r = Recall::new();
    let go = |l: Location, k, r: &mut Recall| match l.step_orbit(k, &cx, r) {
        Step::Go(to) => Some(to),
        _ => None,
    };
    let at = |l: Location| l.page(&cx).unwrap().1;
    let home = Location::from(OrbitAt::HOME);
    assert_eq!(at(home).node() as usize, ORB_NODE);
    assert_eq!(
        go(home, OrbitKey::Edit, &mut r),
        None,
        "none before the first"
    );
    let custom = go(home, OrbitKey::SeqTap, &mut r).unwrap();
    assert_eq!(at(custom).sub(), 1, "SEQ opens CUSTOM");
    assert_eq!(go(custom, OrbitKey::Edit, &mut r), Some(home));
    assert_eq!(go(custom, OrbitKey::SeqTap, &mut r), Some(home), "wraps");
    let walk = |node: usize, r: &mut Recall| {
        let mut l = home;
        while at(l).node() as usize != node {
            let k = if (at(l).node() as usize) < node {
                OrbitKey::Plus
            } else {
                OrbitKey::Minus
            };
            l = go(l, k, r).unwrap();
        }
        l
    };
    let ring = walk(RING_NODE, &mut r);
    let next = go(ring, OrbitKey::SeqTap, &mut r).unwrap();
    assert_eq!(at(next).sub(), at(ring).sub() + 1);
    assert_eq!(go(next, OrbitKey::Edit, &mut r), Some(ring));
    let scn = walk(SCN_NODE, &mut r);
    assert!(matches!(
        scn.step_orbit(OrbitKey::Edit, &cx, &mut r),
        Step::Orbit(_)
    ));
}

/// On the unit, ROW (the default): from OSC's first page EDIT does
/// nothing, SEQ steps right, EDIT back left, and SEQ from the last page
/// wraps to the first.
#[test]
fn row_buttons_on_the_panel() {
    let mut ui = UiState::new();
    screen::to_osc(&mut ui);
    let sub = |ui: &UiState| ui.location().page(&cx(EngineType::Algo)).unwrap().1.sub();
    tap(&mut ui, ButtonId::Edit);
    assert_eq!(sub(&ui), 0, "EDIT at the first does nothing");
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(sub(&ui), 1, "SEQ right");
    tap(&mut ui, ButtonId::Edit);
    assert_eq!(sub(&ui), 0, "EDIT left");
    let n = chain_def_for(EngineType::Algo).blocks[screen::osc_node()].sub_page_count();
    for _ in 0..n - 1 {
        tap(&mut ui, ButtonId::Seq);
    }
    assert_eq!(sub(&ui) as usize, n - 1);
    tap(&mut ui, ButtonId::Seq);
    assert_eq!(sub(&ui), 0, "SEQ wraps from the last");
}

/// MENU and back returns to the browser as it was left.
#[test]
fn menu_and_back_keeps_the_tab() {
    use chimera_core::ui::library::BrowserTab as Tab;
    use chimera_hal::EncoderId;
    let mut ui = UiState::new();
    feed(&mut ui, Input::chord(ButtonId::Edit, ButtonId::B2));
    feed(&mut ui, Input::turn(EncoderId::B, 1));
    tap(&mut ui, ButtonId::Menu);
    while ui.in_settings() {
        tap(&mut ui, ButtonId::Menu);
    }
    assert!(ui.location().browse().is_some(), "back on the browser");
    assert_eq!(ui.lib_tab(), Tab::Library);
}
