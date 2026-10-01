//! One `Location` and a pure step per key (ADR 0044, amended by 0066).

use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::{MIXER_CHANNEL_CHAIN, MODAL_1};
use chimera_core::ui::nav::{
    Location, MixPage, NavCtx, NavKey, PageAt, Recall, Step, chain_def_for, home,
};
use chimera_core::ui::settings::{Act, Kind, Screen, row_at, rows};

const P: [PartId; 6] = PartId::ALL;

fn cx() -> NavCtx {
    NavCtx {
        engines: [EngineType::Algo; 6],
        dyn_rows: 0,
    }
}

fn at(node: u8, sub: u8) -> PageAt {
    PageAt { node, sub }
}

/// Step and follow a `Go`; returns the step.
fn key(l: &mut Location, k: NavKey, cx: &NavCtx, r: &mut Recall) -> Step {
    let s = l.step(k, cx, r);
    if let Step::Go(to) = s {
        *l = to;
    }
    s
}

/// Every `List` and `Leaf` row's path, depth first.
fn paths() -> Vec<Vec<u8>> {
    fn walk(path: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        for (i, r) in rows(path).iter().enumerate() {
            path.push(i as u8);
            if matches!(r.kind, Kind::List(_) | Kind::Leaf(_)) {
                out.push(path.clone());
                walk(path, out);
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(&mut Vec::new(), &mut out);
    out
}

fn path_of(labels: &[&str]) -> Vec<u8> {
    let mut path = Vec::new();
    for l in labels {
        let i = rows(&path).iter().position(|r| r.label == *l).unwrap();
        path.push(i as u8);
    }
    path
}

fn fx_node(l: Location, cx: &NavCtx) -> Option<PageAt> {
    match l.page(cx) {
        Some((c, at)) if core::ptr::eq(c, &MIXER_CHANNEL_CHAIN) && l.part().is_none() => Some(at),
        _ => None,
    }
}

#[test]
fn every_settings_path_is_reached_by_edit_and_menu_backs_out() {
    let cx = cx();
    for path in paths() {
        let mut r = Recall::new();
        let mut l = Location::HOME;
        key(&mut l, NavKey::MenuTap, &cx, &mut r);
        assert_eq!(l, Location::settings_at(&[], 0));
        for &i in &path {
            key(&mut l, NavKey::Bar(i as i8), &cx, &mut r);
            assert_eq!(
                key(&mut l, NavKey::Edit, &cx, &mut r),
                Step::Go(l),
                "{path:?}"
            );
        }
        assert_eq!(l, Location::settings_at(&path, 0), "{path:?}");
        for _ in 0..=path.len() {
            key(&mut l, NavKey::MenuTap, &cx, &mut r);
        }
        assert_eq!(l, Location::HOME, "{path:?}");
    }
}

#[test]
fn bn_leaves_from_any_depth() {
    let cx = cx();
    let mut lists = vec![vec![]];
    lists.extend(paths());
    for path in lists {
        let n = rows(&path).len().max(1) as u8;
        for row in 0..n {
            let l = Location::settings_at(&path, row);
            let mut r = Recall::new();
            assert_eq!(
                l.step(NavKey::Part(P[2]), &cx, &mut r),
                Step::Go(Location::pages(P[2], home(EngineType::Algo))),
                "{path:?} row {row}"
            );
        }
    }
}

#[test]
fn settings_from_is_never_settings() {
    let mut seed: u32 = 0x9e37_79b9;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..10_000 {
        let mut cx = cx();
        cx.engines[(rand() % 6) as usize] = EngineType::Modal;
        cx.dyn_rows = (rand() % 4) as u8;
        let mut r = Recall::new();
        let mut l = Location::HOME;
        for _ in 0..40 {
            let p = P[(rand() % 6) as usize];
            let k = match rand() % 10 {
                0 => NavKey::Part(p),
                1 => NavKey::MixPart(p),
                2 => NavKey::EditPart(p),
                3 => NavKey::Plus,
                4 => NavKey::Minus,
                5 => NavKey::Edit,
                6 => NavKey::SeqTap,
                7 | 8 => NavKey::MenuTap,
                _ => NavKey::Bar((rand() % 21) as i8 - 10),
            };
            // UiState opens a Screen once its listing is in.
            if let (Step::Screen(_), Some(s)) = (key(&mut l, k, &cx, &mut r), l.settings()) {
                let mut path = s.path().to_vec();
                path.push(s.row());
                l = Location::settings_at(&path, 0);
            }
            assert!(r.settings_from().settings().is_none(), "{l:?} {k:?}");
        }
    }
}

#[test]
fn part_key_toggles_and_restores() {
    let mut cx = cx();
    let mut r = Recall::new();
    let mut l = Location::HOME;
    for _ in 0..3 {
        key(&mut l, NavKey::Plus, &cx, &mut r);
    }
    assert_eq!(l, Location::pages(P[0], at(3, 0)));
    key(&mut l, NavKey::Part(P[0]), &cx, &mut r);
    assert_eq!(l, Location::mixer(P[0], MixPage::Sends), "opens on SENDS");
    key(&mut l, NavKey::Part(P[0]), &cx, &mut r);
    assert_eq!(l, Location::pages(P[0], at(3, 0)), "the page left");

    key(&mut l, NavKey::Part(P[0]), &cx, &mut r);
    key(&mut l, NavKey::Minus, &cx, &mut r);
    assert_eq!(l, Location::mixer(P[0], MixPage::Part));
    key(&mut l, NavKey::MixPart(P[1]), &cx, &mut r);
    assert_eq!(
        l,
        Location::mixer(P[1], MixPage::Part),
        "mixer to mixer keeps PART"
    );
    key(&mut l, NavKey::Part(P[1]), &cx, &mut r);
    key(&mut l, NavKey::Part(P[1]), &cx, &mut r);
    assert_eq!(
        l,
        Location::mixer(P[1], MixPage::Sends),
        "PART never from outside"
    );

    key(&mut l, NavKey::Part(P[0]), &cx, &mut r);
    key(&mut l, NavKey::MixPart(P[0]), &cx, &mut r);
    cx.engines[0] = EngineType::Modal;
    key(&mut l, NavKey::Part(P[0]), &cx, &mut r);
    assert_eq!(
        l,
        Location::pages(P[0], home(EngineType::Modal)),
        "another engine: home"
    );
}

#[test]
fn part_key_restores_only_from_its_own_mixer() {
    let cx = cx();
    let mut r = Recall::new();
    let mut l = Location::pages(P[1], at(1, 0));
    key(&mut l, NavKey::Part(P[0]), &cx, &mut r);
    assert_eq!(l, Location::pages(P[0], at(0, 0)));
    key(&mut l, NavKey::MixPart(P[1]), &cx, &mut r);
    key(&mut l, NavKey::Part(P[0]), &cx, &mut r);
    assert_eq!(l, Location::pages(P[0], at(0, 0)), "Part 2's mixer");
    key(&mut l, NavKey::MenuTap, &cx, &mut r);
    key(&mut l, NavKey::Part(P[2]), &cx, &mut r);
    assert_eq!(l, Location::pages(P[2], at(0, 0)), "SETTINGS");
    key(&mut l, NavKey::MixPart(P[1]), &cx, &mut r);
    key(&mut l, NavKey::Part(P[1]), &cx, &mut r);
    assert_eq!(l, Location::pages(P[1], at(1, 0)), "its own mixer");
}

fn bn_lands_on(e: EngineType) -> PageAt {
    let mut cx = cx();
    cx.engines[2] = e;
    let mut r = Recall::new();
    let froms = [
        Location::HOME,
        Location::mixer(P[0], MixPage::Part),
        Location::sound(P[2]),
        Location::settings_at(&[1], 0),
    ];
    for from in froms {
        assert_eq!(
            from.step(NavKey::Part(P[2]), &cx, &mut r),
            Step::Go(Location::pages(P[2], home(e))),
            "{from:?}"
        );
    }
    home(e)
}

#[test]
fn bn_lands_on_the_engine_home_algo() {
    assert_eq!(bn_lands_on(EngineType::Algo), at(0, 0));
}

#[test]
fn bn_lands_on_the_engine_home_modal() {
    let h = bn_lands_on(EngineType::Modal);
    let c = chain_def_for(EngineType::Modal);
    assert_eq!(c.active_def(h.node as usize, 0).unwrap().id, MODAL_1.id);
    assert_eq!(c.blocks[h.node as usize].def.short, "RES");
    assert_eq!(h.sub, 0);
}

#[test]
fn mixer_walk_reaches_the_fx() {
    let cx = cx();
    let mut r = Recall::new();
    let mut l = Location::mixer(P[0], MixPage::Part);
    for _ in 0..11 {
        key(&mut l, NavKey::Plus, &cx, &mut r);
    }
    assert_eq!(l, Location::mixer(P[5], MixPage::Sends));
    key(&mut l, NavKey::Plus, &cx, &mut r);
    assert_eq!(fx_node(l, &cx), Some(at(2, 0)), "CHORUS");
    key(&mut l, NavKey::Minus, &cx, &mut r);
    assert_eq!(l, Location::mixer(P[5], MixPage::Sends));
    for _ in 0..11 {
        key(&mut l, NavKey::Minus, &cx, &mut r);
    }
    assert_eq!(l, Location::mixer(P[0], MixPage::Part));
    assert_eq!(l.step(NavKey::Minus, &cx, &mut r), Step::Stay);

    let last = MIXER_CHANNEL_CHAIN.len() as u8 - 1;
    l = Location::mixer(P[5], MixPage::Sends);
    for _ in 0..last - 1 {
        key(&mut l, NavKey::Plus, &cx, &mut r);
    }
    assert_eq!(fx_node(l, &cx), Some(at(last, 0)), "MASTER");
    assert_eq!(l.step(NavKey::Plus, &cx, &mut r), Step::Stay);
}

#[test]
fn leaf_keys() {
    let cx = cx();
    let mut r = Recall::new();

    #[cfg(debug_assertions)]
    {
        let mut l = Location::settings_at(&path_of(&["SYSTEM", "DEMO"]), 0);
        for n in 1..=4 {
            key(&mut l, NavKey::Plus, &cx, &mut r);
            assert_eq!(l.settings().unwrap().page(), at(n, 0));
        }
        assert_eq!(l.step(NavKey::Plus, &cx, &mut r), Step::Stay);
        key(&mut l, NavKey::Minus, &cx, &mut r);
        assert_eq!(l.settings().unwrap().page(), at(3, 0));
    }

    let mut l = Location::settings_at(&path_of(&["SYSTEM", "ABOUT"]), 0);
    key(&mut l, NavKey::Edit, &cx, &mut r);
    assert_eq!(l.settings().unwrap().page(), at(0, 1), "AUDIO");
    assert_eq!(l.step(NavKey::Edit, &cx, &mut r), Step::Stay);
    key(&mut l, NavKey::SeqTap, &cx, &mut r);
    assert_eq!(l.settings().unwrap().page(), at(0, 0), "ABOUT");
    assert!(l.page(&cx).is_some());

    let l = Location::settings_at(&path_of(&["PERSONALIZE", "THEME"]), 0);
    for k in [
        NavKey::Plus,
        NavKey::Minus,
        NavKey::Edit,
        NavKey::SeqTap,
        NavKey::Bar(3),
    ] {
        assert_eq!(l.step(k, &cx, &mut r), Step::Stay, "{k:?}");
    }
}

#[test]
fn edit_on_a_screen_row_does_not_move() {
    let cx = cx();
    let mut r = Recall::new();
    let l = Location::settings_at(&path_of(&["PROJECT"]), 0);
    assert_eq!(row_at(&[0, 0]).unwrap().label, "LOAD PROJECT");
    assert_eq!(
        l.step(NavKey::Edit, &cx, &mut r),
        Step::Screen(Screen::LoadProject)
    );
    assert_eq!(l, Location::settings_at(&[0], 0));

    // Inside a Screen, SEQ runs the row and EDIT stays.
    let s = Location::settings_at(&[0, 0], 0);
    let cx = NavCtx { dyn_rows: 3, ..cx };
    assert_eq!(s.step(NavKey::SeqTap, &cx, &mut r), Step::Run);
    assert_eq!(s.step(NavKey::Edit, &cx, &mut r), Step::Stay);
    assert!(s.page(&cx).is_none());
}

#[test]
fn seq_tap_runs_an_act() {
    let cx = cx();
    let mut r = Recall::new();
    let l = Location::settings_at(&path_of(&["PART"]), 1);
    assert_eq!(
        l.step(NavKey::SeqTap, &cx, &mut r),
        Step::Act(Act::PartClear)
    );
    assert_eq!(l.step(NavKey::Edit, &cx, &mut r), Step::Stay);
}

#[test]
fn seq_on_mixer_and_sound_opens_part_settings() {
    let cx = cx();
    assert_eq!(rows(&[])[1].label, "PART");
    for from in [
        Location::mixer(P[1], MixPage::Sends),
        Location::mixer(P[3], MixPage::Part),
        Location::sound(P[4]),
    ] {
        let mut r = Recall::new();
        let mut l = from;
        key(&mut l, NavKey::SeqTap, &cx, &mut r);
        assert_eq!(l, Location::settings_at(&[1], 0), "{from:?}");
        assert_eq!(r.settings_from(), from);
        key(&mut l, NavKey::MenuTap, &cx, &mut r);
        assert_eq!(l, Location::settings_at(&[], 1));
        key(&mut l, NavKey::MenuTap, &cx, &mut r);
        assert_eq!(l, from);
    }
}

#[test]
fn edit_and_the_sound_rung() {
    let cx = cx();
    let mut r = Recall::new();
    let mut l = Location::mixer(P[1], MixPage::Sends);
    key(&mut l, NavKey::Edit, &cx, &mut r);
    assert_eq!(l, Location::sound(P[1]));
    key(&mut l, NavKey::Minus, &cx, &mut r);
    key(&mut l, NavKey::Minus, &cx, &mut r);
    assert_eq!(l, Location::sound(P[5]), "wraps");
    key(&mut l, NavKey::Plus, &cx, &mut r);
    assert_eq!(l, Location::sound(P[0]));
    key(&mut l, NavKey::EditPart(P[3]), &cx, &mut r);
    assert_eq!(l, Location::sound(P[3]));
    assert_eq!(l.part(), Some(P[3]));
    assert!(l.page(&cx).is_none());
}

#[test]
fn later_rows_are_inert() {
    let cx = cx();
    let mut r = Recall::new();
    let l = Location::settings_at(&[], path_of(&["ORBIT"])[0]);
    assert!(matches!(row_at(&[2]).unwrap().kind, Kind::Later(_)));
    assert_eq!(l.step(NavKey::Edit, &cx, &mut r), Step::Stay);
    assert_eq!(l.step(NavKey::SeqTap, &cx, &mut r), Step::Stay);
}

#[test]
fn bar_wraps_any_delta_on_any_list() {
    let mut lists: Vec<(Vec<u8>, u8)> = vec![(vec![], 0)];
    lists.extend(paths().into_iter().map(|p| (p, 0)));
    for dyn_rows in [0, 1, 5] {
        lists.push((vec![0, 0], dyn_rows));
    }
    for (path, dyn_rows) in lists {
        let cx = NavCtx { dyn_rows, ..cx() };
        let len = match row_at(&path).unwrap().kind {
            Kind::List(r) => r.len(),
            Kind::Screen(_) => dyn_rows as usize,
            _ => continue,
        };
        for k in [
            NavKey::Bar(10),
            NavKey::Bar(-10),
            NavKey::Bar(i8::MAX),
            NavKey::Bar(i8::MIN),
            NavKey::Plus,
            NavKey::Minus,
        ] {
            let mut r = Recall::new();
            let mut l = Location::settings_at(&path, 0);
            for _ in 0..3 {
                key(&mut l, k, &cx, &mut r);
                let row = l.settings().unwrap().row() as usize;
                assert!(row < len.max(1), "{path:?} {k:?} {row}");
            }
        }
    }
}
