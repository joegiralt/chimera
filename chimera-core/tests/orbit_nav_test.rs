//! ORBIT as a place (ADR 0044, amended 2026-10-02): SEQ hold goes in,
//! MENU leaves to where it came from, and its keys are their own type.

mod screen;

use chimera_core::orbit::RingIdx;
use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::{ALGO_CHAIN, FILTER};
use chimera_core::ui::nav::{
    Location, MixPage, NavCtx, NavKey, OrbitAct, OrbitAt, OrbitCx, OrbitKey, Outside, Recall, Step,
};
use chimera_core::ui::orbit::{CHRD_NODE, ORB_NODE, ORBIT_CHAIN, RING_NODE};
use chimera_core::ui::settings::rows;

const P: [PartId; 6] = PartId::ALL;
const R: [RingIdx; 5] = RingIdx::ALL;

fn cx() -> NavCtx {
    NavCtx {
        engines: [EngineType::Algo; 6],
        dyn_rows: 0,
        orbit: OrbitCx::NONE,
    }
}

/// Step and follow a `Go`; returns the step.
fn key(l: &mut Location, k: NavKey, cx: &NavCtx, r: &mut Recall) -> Step {
    let s = l.step(k, cx, r);
    if let Step::Go(to) = s {
        *l = to;
    }
    s
}

/// `key` for ORBIT's keys.
fn okey(l: &mut Location, k: OrbitKey, cx: &NavCtx, r: &mut Recall) -> Step {
    let s = l.step_orbit(k, cx, r);
    if let Step::Go(to) = s {
        *l = to;
    }
    s
}

/// Part `p`'s FILTER page.
fn filter(p: PartId) -> Location {
    let node = ALGO_CHAIN
        .blocks
        .iter()
        .position(|b| b.def.id == FILTER.id)
        .unwrap();
    Location::pages(p, ALGO_CHAIN.page(node, 0).unwrap())
}

fn orbit(o: OrbitAt) -> Location {
    Location::from(o)
}

fn all_keys() -> Vec<OrbitKey> {
    let mut ks = vec![
        OrbitKey::PlayStop,
        OrbitKey::Plus,
        OrbitKey::Minus,
        OrbitKey::Edit,
        OrbitKey::SeqTap,
        OrbitKey::MenuTap,
    ];
    for r in R {
        ks.extend([OrbitKey::Ring(r), OrbitKey::Cross(r), OrbitKey::Mute(r)]);
    }
    ks
}

/// Every ORBIT page on every ring, reached by its own keys.
fn every_orbit_place() -> Vec<OrbitAt> {
    let cx = cx();
    let mut seen = vec![OrbitAt::HOME];
    let mut i = 0;
    while i < seen.len() {
        for k in [OrbitKey::Plus, OrbitKey::Minus, OrbitKey::Edit] {
            if let Step::Go(to) = orbit(seen[i]).step_orbit(k, &cx, &mut Recall::new())
                && let Some(o) = to.orbit()
                && !seen.contains(&o)
            {
                seen.push(o);
            }
        }
        i += 1;
    }
    let pages = ORBIT_CHAIN
        .blocks
        .iter()
        .map(|b| b.sub_page_count().max(1))
        .sum::<usize>();
    assert_eq!(seen.len(), pages, "every ORBIT page is reachable");
    seen.iter()
        .flat_map(|o| R.map(|r| o.with_ring(r)))
        .collect()
}

#[test]
fn seq_hold_enters_orbit() {
    let cx = cx();
    let mut r = Recall::new();
    let mut l = filter(P[1]);
    key(&mut l, NavKey::SeqHold, &cx, &mut r);
    assert_eq!(l, orbit(OrbitAt::HOME));
    assert_eq!(l.orbit(), Some(OrbitAt::HOME));
    assert_eq!(l.part(), None);
    okey(&mut l, OrbitKey::MenuTap, &cx, &mut r);
    assert_eq!(l, filter(P[1]));

    // Back in, to the ORBIT page left.
    key(&mut l, NavKey::SeqHold, &cx, &mut r);
    okey(&mut l, OrbitKey::Plus, &cx, &mut r);
    let left = l;
    assert_ne!(left, orbit(OrbitAt::HOME));
    okey(&mut l, OrbitKey::MenuTap, &cx, &mut r);
    key(&mut l, NavKey::SeqHold, &cx, &mut r);
    assert_eq!(l, left);

    // From the mixer and the Sound rung, and back.
    for from in [Location::mixer(P[2], MixPage::Part), Location::sound(P[3])] {
        let mut l = from;
        key(&mut l, NavKey::SeqHold, &cx, &mut r);
        assert_eq!(l, left);
        okey(&mut l, OrbitKey::MenuTap, &cx, &mut r);
        assert_eq!(l, from);
    }

    // From SETTINGS › MIDI CONFIG, opened on Part 5's mixer: MENU goes
    // back to the mixer, where SETTINGS was opened.
    let from = Location::mixer(P[4], MixPage::Sends);
    let mut l = from;
    key(&mut l, NavKey::MenuTap, &cx, &mut r);
    let midi = rows(&[])
        .iter()
        .position(|row| row.label == "MIDI CONFIG")
        .unwrap();
    for _ in 0..midi {
        key(&mut l, NavKey::Plus, &cx, &mut r);
    }
    key(&mut l, NavKey::Edit, &cx, &mut r);
    assert_eq!(l, Location::settings_at(&[midi as u8], 0));
    key(&mut l, NavKey::SeqHold, &cx, &mut r);
    assert_eq!(l, left);
    okey(&mut l, OrbitKey::MenuTap, &cx, &mut r);
    assert_eq!(l, from);
    // A second tap, now outside ORBIT, opens SETTINGS.
    key(&mut l, NavKey::MenuTap, &cx, &mut r);
    assert_eq!(l, Location::settings_at(&[], 0));
}

#[test]
fn orbit_keys_never_leave_but_cross_and_menu() {
    let cx = cx();
    for o in every_orbit_place() {
        for k in all_keys() {
            if matches!(k, OrbitKey::Cross(_) | OrbitKey::MenuTap) {
                continue;
            }
            let mut r = Recall::new();
            match orbit(o).step_orbit(k, &cx, &mut r) {
                Step::Go(to) => assert!(to.orbit().is_some(), "{o:?} {k:?} → {to:?}"),
                Step::Stay | Step::Orbit(_) => {}
                s => panic!("{o:?} {k:?} → {s:?}"),
            }
        }
    }
}

#[test]
fn cross_lands_on_the_target_part() {
    let mut cx = cx();
    cx.orbit.targets = [P[0], P[3], P[2], P[3], P[4]];
    let mut r = Recall::new();
    // Part 4 left on FILTER, ORBIT entered from Part 2's mixer.
    let mut l = filter(P[3]);
    key(&mut l, NavKey::Part(P[1]), &cx, &mut r);
    let entered = Location::mixer(P[1], MixPage::Sends);
    key(&mut l, NavKey::MixPart(P[1]), &cx, &mut r);
    assert_eq!(l, entered);
    key(&mut l, NavKey::SeqHold, &cx, &mut r);
    assert_eq!(r.orbit_from(), entered);

    okey(&mut l, OrbitKey::Cross(R[1]), &cx, &mut r);
    assert_eq!(l, filter(P[3]));
    assert_eq!(l.part(), Some(P[3]));
    assert_eq!(r.orbit_from(), entered, "a cross keeps orbit_from");

    // After an engine change it lands on the engine's home.
    key(&mut l, NavKey::SeqHold, &cx, &mut r);
    cx.engines[3] = EngineType::Modal;
    okey(&mut l, OrbitKey::Cross(R[1]), &cx, &mut r);
    assert_eq!(l, Location::part_home(P[3], EngineType::Modal));

    // MENU opens SETTINGS from the Part; SEQ hold goes in from there and
    // MENU leaves to where SETTINGS was opened.
    key(&mut l, NavKey::MenuTap, &cx, &mut r);
    assert!(l.settings().is_some());
    key(&mut l, NavKey::SeqHold, &cx, &mut r);
    assert!(l.orbit().is_some());
    okey(&mut l, OrbitKey::MenuTap, &cx, &mut r);
    assert_eq!(l, Location::part_home(P[3], EngineType::Modal));
}

#[test]
fn edit_cycles_only_inside() {
    let plain = cx();
    let mut r = Recall::new();

    // ORB always opens CUSTOM, SCALE named or not (ADR 0084); CUSTOM
    // returns to ORB.
    let home = orbit(OrbitAt::HOME);
    let mut l = home;
    okey(&mut l, OrbitKey::Edit, &plain, &mut r);
    assert_eq!(l, orbit(OrbitAt::HOME.custom()));
    assert_eq!(l.orbit().unwrap().page().node() as usize, ORB_NODE);
    okey(&mut l, OrbitKey::Edit, &plain, &mut r);
    assert_eq!(l, home);

    // RING: wraps over its sub-pages.
    let mut l = home;
    okey(&mut l, OrbitKey::Ring(R[2]), &plain, &mut r);
    let ring = l.orbit().unwrap();
    assert_eq!(ring.page().node() as usize, RING_NODE);
    assert_eq!(ring.ring(), R[2]);
    let subs = ORBIT_CHAIN.blocks[RING_NODE].sub_page_count().max(1);
    assert!(subs > 1, "RING has sub-pages to cycle");
    let mut shown = vec![ring.page()];
    for _ in 0..subs {
        okey(&mut l, OrbitKey::Edit, &plain, &mut r);
        let o = l.orbit().unwrap();
        assert_eq!(o.page().node() as usize, RING_NODE);
        assert_eq!(o.ring(), R[2]);
        shown.push(o.page());
    }
    assert_eq!(l, orbit(ring), "{subs} EDITs come back round");
    shown.sort_by_key(|p| p.sub());
    shown.dedup();
    assert_eq!(shown.len(), subs, "each sub-page once");

    // CHRD: nothing.
    let mut l = home;
    while l.orbit().unwrap().page().node() as usize != CHRD_NODE {
        okey(&mut l, OrbitKey::Plus, &plain, &mut r);
    }
    assert_eq!(l.step_orbit(OrbitKey::Edit, &plain, &mut r), Step::Stay);
}

#[test]
fn ring_tap_reopens_the_ring_sub_page_last_shown() {
    let cx = cx();
    let subs = ORBIT_CHAIN.blocks[RING_NODE].sub_page_count();
    assert!(subs > 1, "RING has sub-pages to remember");
    let mut r = Recall::new();
    let mut l = orbit(OrbitAt::HOME);
    okey(&mut l, OrbitKey::Ring(R[0]), &cx, &mut r);
    let first = l.orbit().unwrap().page();
    assert_eq!(first.sub(), 0, "a first visit opens RING's first sub-page");
    okey(&mut l, OrbitKey::Edit, &cx, &mut r);
    let sub = l.orbit().unwrap().page();
    assert_ne!(sub, first);
    // Another ring keeps the sub-page.
    okey(&mut l, OrbitKey::Ring(R[3]), &cx, &mut r);
    assert_eq!(l.orbit().unwrap().page(), sub);
    assert_eq!(l.orbit().unwrap().ring(), R[3]);
    // Off RING and back by a tap: the same sub-page.
    okey(&mut l, OrbitKey::Minus, &cx, &mut r);
    okey(&mut l, OrbitKey::Ring(R[4]), &cx, &mut r);
    assert_eq!(l.orbit().unwrap().page(), sub);
    assert_eq!(l.orbit().unwrap().ring(), R[4]);
}

#[test]
fn seq_tap_in_orbit_never_saves() {
    let cx = cx();
    for o in every_orbit_place() {
        let mut r = Recall::new();
        let s = orbit(o).step_orbit(OrbitKey::SeqTap, &cx, &mut r);
        assert!(matches!(s, Step::Go(_) | Step::Stay), "{o:?}: {s:?}");
    }
}

#[test]
fn mute_and_play_are_acts() {
    let cx = cx();
    let mut r = Recall::new();
    let l = orbit(OrbitAt::HOME);
    assert_eq!(
        l.step_orbit(OrbitKey::PlayStop, &cx, &mut r),
        Step::Orbit(OrbitAct::PlayStop)
    );
    for ring in R {
        assert_eq!(
            l.step_orbit(OrbitKey::Mute(ring), &cx, &mut r),
            Step::Orbit(OrbitAct::Mute(ring))
        );
    }
}

/// Sound-side: neither SETTINGS nor ORBIT.
fn sound_side(l: Location) -> bool {
    l.settings().is_none() && l.orbit().is_none()
}

#[test]
fn outside_is_never_orbit_or_settings() {
    assert_eq!(Outside::new(orbit(OrbitAt::HOME)), None);
    assert_eq!(Outside::new(Location::settings_at(&[], 0)), None);
    assert!(Outside::new(filter(P[0])).is_some());

    let mut seed: u32 = 0x2545_f491;
    let mut rand = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..10_000 {
        let mut cx = cx();
        cx.engines[(rand() % 6) as usize] = EngineType::Modal;
        cx.orbit.targets = R.map(|_| P[(rand() % 6) as usize]);
        let mut r = Recall::new();
        let mut l = Location::home(&cx);
        for _ in 0..40 {
            let p = P[(rand() % 6) as usize];
            let ring = R[(rand() % 5) as usize];
            let was = l;
            if l.orbit().is_some() {
                let k = match rand() % 10 {
                    0 => OrbitKey::Ring(ring),
                    1 => OrbitKey::Cross(ring),
                    2 => OrbitKey::Mute(ring),
                    3 => OrbitKey::PlayStop,
                    4 => OrbitKey::Plus,
                    5 => OrbitKey::Minus,
                    6 => OrbitKey::Edit,
                    7 => OrbitKey::SeqTap,
                    _ => OrbitKey::MenuTap,
                };
                okey(&mut l, k, &cx, &mut r);
                if k == OrbitKey::MenuTap {
                    assert!(sound_side(l), "ORBIT's MENU from {was:?} → {l:?}");
                }
            } else {
                let k = match rand() % 11 {
                    0 => NavKey::Part(p),
                    1 => NavKey::MixPart(p),
                    2 => NavKey::EditPart(p),
                    3 => NavKey::Plus,
                    4 => NavKey::Minus,
                    5 => NavKey::Edit,
                    6 => NavKey::SeqTap,
                    7 | 8 => NavKey::MenuTap,
                    9 => NavKey::SeqHold,
                    _ => NavKey::Bar((rand() % 21) as i8 - 10),
                };
                if let (Step::Screen(_), Some(s)) = (key(&mut l, k, &cx, &mut r), l.settings()) {
                    let mut path = s.path().to_vec();
                    path.push(s.row());
                    l = Location::settings_at(&path, 0);
                }
                if was.settings().is_some() && l.settings().is_none() && k == NavKey::MenuTap {
                    assert!(sound_side(l), "SETTINGS closed from {was:?} → {l:?}");
                }
            }
            assert!(sound_side(r.settings_from()), "{was:?} → {l:?}");
            assert!(sound_side(r.orbit_from()), "{was:?} → {l:?}");
        }
    }
}

#[cfg(not(feature = "orbit"))]
#[test]
fn orbit_disabled_ignores_seq_hold() {
    use chimera_core::ui::UiState;
    use chimera_hal::ButtonId;
    let mut ui = Box::new(UiState::new());
    let at = ui.location();
    screen::hold(&mut ui, ButtonId::Seq);
    assert_eq!(ui.location(), at);
}

#[cfg(feature = "orbit")]
#[test]
fn seq_hold_on_the_unit_enters_orbit() {
    use chimera_core::ui::UiState;
    use chimera_hal::ButtonId;
    let mut ui = Box::new(UiState::new());
    let home = ui.location();
    screen::hold(&mut ui, ButtonId::Seq);
    assert_eq!(ui.location().orbit(), Some(OrbitAt::HOME));
    // A held SEQ inside ORBIT is no key there.
    screen::hold(&mut ui, ButtonId::Seq);
    assert_eq!(ui.location().orbit(), Some(OrbitAt::HOME));
    screen::tap(&mut ui, ButtonId::Menu);
    assert_eq!(ui.location(), home);
}

#[cfg(feature = "orbit")]
mod quick_save {
    use super::screen::{Input, hold, tap};
    use chimera_core::name::ProjectName;
    use chimera_core::project::{
        LoadLink, Project, ProjectFile, ProjectNote, SaveTo, list_projects, new_project_id,
        save_project,
    };
    use chimera_core::storage::{Card, Generation, SystemSettings, SystemSync};
    use chimera_core::ui::UiState;
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::busy::ToastStep;
    use chimera_core::ui::settings::CardCx;
    use chimera_hal::ButtonId;
    use chimera_hal::testkit::MemStore;

    struct Rig {
        ui: Box<UiState>,
        s: MemStore,
        card: Card,
        sync: SystemSync,
        set: SystemSettings,
        link: LoadLink,
    }

    impl Rig {
        fn work(&mut self) {
            let link = &self.link;
            let cx = CardCx {
                card: &mut self.card,
                store: &mut self.s,
                sync: &mut self.sync,
                settings: &mut self.set,
            };
            let _ = self.ui.card_work(cx, link, |swap, _| {
                let _ = swap.settle(link, || false);
            });
        }

        fn feed(&mut self, i: Input) {
            super::screen::feed(&mut self.ui, i);
            self.work();
        }

        fn tap(&mut self, b: ButtonId) {
            tap(&mut self.ui, b);
            self.work();
        }

        fn toast(&mut self) -> String {
            match self.ui.step_toast(0) {
                ToastStep::Show(t) => t.as_str().to_string(),
                _ => String::new(),
            }
        }
    }

    fn generation(s: &mut MemStore, f: ProjectFile) -> Option<Generation> {
        let mut g = None;
        let _ = list_projects(&mut Card::new(), s, &mut |e| {
            if e.id == f.id() {
                g = e.generation;
            }
        });
        g
    }

    /// A rig with a project saved as "ALPHA" loaded through LOAD, back
    /// outside SETTINGS.
    fn loaded() -> (Rig, ProjectFile) {
        let mut s = MemStore::new(1);
        let mut card = Card::new();
        let (mut p, _) = Project::boxed();
        p.set_name(ProjectName::new("ALPHA").unwrap());
        let fresh = new_project_id(&mut card, &mut s).out.unwrap();
        let f = fresh.file();
        assert!(matches!(
            save_project(&mut card, &mut s, &mut p, SaveTo::Fresh(fresh)).out,
            ProjectNote::Saved(_)
        ));
        let (sync, set, _) = SystemSync::boot(&mut card, &mut s);
        let mut r = Rig {
            ui: Box::new(UiState::new()),
            s,
            card,
            sync,
            set,
            link: LoadLink::new(),
        };
        // SETTINGS › PROJECT › LOAD PROJECT; its first row is ALPHA.
        r.tap(ButtonId::Menu);
        r.feed(Input::press(ButtonId::Edit));
        r.feed(Input::press(ButtonId::Edit));
        r.tap(ButtonId::Edit);
        assert_eq!(r.ui.project().meta().file(), Some(f));
        while r.ui.in_settings() {
            r.tap(ButtonId::Menu);
        }
        (r, f)
    }

    #[test]
    fn menu_hold_in_orbit_quick_saves() {
        let (mut r, f) = loaded();
        hold(&mut r.ui, ButtonId::Seq);
        r.work();
        let at = r.ui.location();
        assert!(at.orbit().is_some());
        r.ui.project_mut()
            .set_name(ProjectName::new("ALPHA2").unwrap());
        r.ui.update(UiTick::for_test());
        let was = generation(&mut r.s, f);

        super::screen::feed(&mut r.ui, Input::press(ButtonId::Menu).at(0));
        super::screen::feed(
            &mut r.ui,
            Input::held(ButtonId::Menu).at(chimera_core::ui::hold::HOLD_MS),
        );
        assert_eq!(r.ui.location(), at, "the hold moves nothing");
        r.work();
        assert_eq!(r.toast(), "SAVED");
        assert_eq!(r.ui.location(), at);
        let saved = generation(&mut r.s, f);
        assert_ne!(saved, was);

        // The release does nothing.
        r.feed(Input::release(ButtonId::Menu).at(chimera_core::ui::hold::HOLD_MS + 33));
        assert_eq!(r.ui.location(), at);
        assert_eq!(generation(&mut r.s, f), saved);
    }
}

/// ORBIT's buttons through `UiState`: scripted frames, stamped.
#[cfg(feature = "orbit")]
#[test]
fn orbit_buttons() {
    use chimera_core::addr::Op;
    use chimera_core::orbit::channel::{Cmd, CmdLog, Seq as LogSeq, receive_each};
    use chimera_core::orbit::runner::OrbitShared;
    use chimera_core::ui::UiState;
    use chimera_core::ui::hold::HOLD_MS;
    use chimera_core::ui::orbit::{ORBIT_NOTES, ORBIT_RHYTHM, ORBIT_VOICE};
    use chimera_core::ui::part_page;
    use chimera_hal::ButtonId::{B1, B2, B3, B4, B5, B6, Edit, Menu, Mix, Seq};
    use screen::{Input, feed, hold, tap};

    fn muted(ui: &UiState) -> [bool; 5] {
        let mut out = OrbitShared::default();
        ui.publish_orbit(&mut out);
        std::array::from_fn(|i| out.settings.rings[i].ring.mute)
    }
    fn first_cell(ui: &UiState) -> f32 {
        part_page::read_values(ui.page_def(), ui.params(), Op::A)[0]
    }

    let mut ui = Box::new(UiState::new());
    // Enter from Part 2's FILTER, to see MENU come back to it.
    ui.go_for_test(filter(P[1]));
    let entry = ui.location();
    hold(&mut ui, Seq);
    let orb = ui.location();
    assert_eq!(orb.orbit(), Some(OrbitAt::HOME));

    // B3 tapped opens RING 3, and never leaves ORBIT.
    tap(&mut ui, B3);
    let at = ui.location().orbit().expect("still ORBIT");
    assert_eq!(at.ring(), R[2]);
    let ring3 = ui.location();

    // MIX + B4: a mute on the press, no select, no cross after 600 ms.
    // The init scene mutes ring 5 only.
    let init = [false, false, false, false, true];
    assert_eq!(muted(&ui), init);
    let rev = ui.project().rev();
    feed(&mut ui, Input::chord(Mix, B4).at(0));
    let ring4_off = [false, false, false, true, true];
    assert_eq!(muted(&ui), ring4_off);
    assert_ne!(ui.project().rev(), rev, "the project is marked");
    feed(&mut ui, Input::held(B4).and_held(Mix).at(600));
    feed(&mut ui, Input::release(B4).and_held(Mix).at(633));
    feed(&mut ui, Input::release(Mix).at(666));
    assert_eq!(ui.location(), ring3, "no select, no cross");
    assert_eq!(muted(&ui), ring4_off, "once");
    // Again: back on.
    feed(&mut ui, Input::chord(Mix, B4).at(1000));
    feed(&mut ui, Input::release(B4).and_held(Mix).at(1100));
    feed(&mut ui, Input::release(Mix).at(1133));
    assert_eq!(muted(&ui), init);

    // EDIT + B2 changes nothing.
    feed(&mut ui, Input::chord(Edit, B2).at(2010));
    feed(&mut ui, Input::release(B2).and_held(Edit).at(2110));
    feed(&mut ui, Input::release(Edit).at(2143));
    assert_eq!(ui.location(), ring3);
    // Held 600 ms: no cross either, and the mutes stay as they were.
    feed(&mut ui, Input::chord(Edit, B2).at(3000));
    feed(&mut ui, Input::held(B2).and_held(Edit).at(3600));
    feed(&mut ui, Input::release(B2).and_held(Edit).at(3633));
    feed(&mut ui, Input::release(Edit).at(3666));
    assert_eq!(ui.location(), ring3);
    assert_eq!(ui.active_part, P[1]);
    assert_eq!(muted(&ui), init);

    // B6 logs one TogglePlay, and the publish carries it.
    assert_eq!(ui.orbit().log().head(), LogSeq::ZERO);
    tap(&mut ui, B6);
    let toggles = |log: &CmdLog| {
        let mut n = 0;
        receive_each(log, &mut LogSeq::default(), |c| {
            n += (c == Cmd::TogglePlay) as usize
        });
        n
    };
    assert_eq!(toggles(ui.orbit().log()), 1);
    let mut out = OrbitShared::default();
    ui.publish_orbit(&mut out);
    assert_eq!(toggles(&out.log), 1);

    // Hold B1 reads Part 1's first cell; hold B3 is Part 3's own.
    hold(&mut ui, B1);
    assert_eq!(ui.active_part, P[0]);
    let one = first_cell(&ui);
    hold(&mut ui, Seq);
    assert_eq!(ui.location(), ring3, "SEQ hold returns to RING");
    // Make Part 3 differ from Part 1, then cross to it.
    hold(&mut ui, B3);
    assert_eq!(ui.active_part, P[2]);
    assert_eq!(ui.location().part(), Some(P[2]));
    feed(&mut ui, Input::turn(chimera_hal::EncoderId::A, 5));
    let three = first_cell(&ui);
    assert_ne!(three, one);
    hold(&mut ui, Seq);
    hold(&mut ui, B3);
    assert_eq!(ui.active_part, P[2]);
    assert_eq!(first_cell(&ui), three, "Part 3's own value");
    hold(&mut ui, Seq);
    assert_eq!(ui.location(), ring3);

    // EDIT on ORB opens CUSTOM (ADR 0084); on RING it cycles RING's
    // sub-pages (ADR 0044) and stays on the ring.
    ui.go_for_test(orb);
    tap(&mut ui, Edit);
    assert_eq!(ui.location(), orb.orbit().unwrap().custom().into());
    ui.go_for_test(ring3);
    tap(&mut ui, Edit);
    let o = ui.location().orbit().unwrap();
    assert_eq!((o.page().node() as usize, o.ring()), (RING_NODE, R[2]));
    assert_ne!(ui.location(), ring3, "the sub-page moved");

    // Three EDITs on RING: VOICE, NOTES, and back to RHYTHM.
    ui.go_for_test(ring3);
    let mut shown = vec![ui.page_def().id];
    for _ in 0..3 {
        tap(&mut ui, Edit);
        shown.push(ui.page_def().id);
    }
    assert_eq!(
        shown,
        [
            ORBIT_RHYTHM.id,
            ORBIT_VOICE.id,
            ORBIT_NOTES.id,
            ORBIT_RHYTHM.id
        ]
    );
    assert_eq!(ui.location(), ring3);
    // B1–B5 open each ring on the sub-page last shown: NOTES, then VOICE.
    tap(&mut ui, Edit);
    tap(&mut ui, Edit);
    for (b, r) in [B1, B2, B3, B4, B5].into_iter().zip(R) {
        tap(&mut ui, b);
        assert_eq!(ui.page_def().id, ORBIT_NOTES.id, "ring {}", r.get() + 1);
        assert_eq!(ui.location().orbit().unwrap().ring(), r);
    }
    tap(&mut ui, Edit);
    tap(&mut ui, Edit);
    tap(&mut ui, B1);
    assert_eq!(ui.page_def().id, ORBIT_VOICE.id);
    ui.go_for_test(ring3);

    // A SEQ tap saves no scene (every page's:
    // `orbit_scenes_test::seq_saves_nothing_on_any_orbit_page`).
    ui.project_mut()
        .orbit_mut()
        .live_mut_for_test()
        .chords
        .tonic = chimera_theory::PitchClass::E;
    tap(&mut ui, Seq);
    let song = ui.project().orbit();
    assert_ne!(*song.scene(song.current()), *song.live(), "not saved");

    // MENU tap goes back to where ORBIT was entered, Part and all.
    ui.go_for_test(entry);
    hold(&mut ui, Seq);
    assert!(ui.location().orbit().is_some());
    tap(&mut ui, Menu);
    assert_eq!(ui.location(), entry);
    assert_eq!(ui.active_part, P[1]);
    assert_eq!(toggles(ui.orbit().log()), 1);

    // MENU hold queues the quick save and stays.
    hold(&mut ui, Seq);
    let at = ui.location();
    assert!(at.orbit().is_some());
    feed(&mut ui, Input::press(Menu).at(0));
    feed(&mut ui, Input::held(Menu).at(HOLD_MS));
    feed(&mut ui, Input::release(Menu).at(HOLD_MS + 33));
    assert_eq!(ui.location(), at);
    assert_eq!(toggles(ui.orbit().log()), 1);
}

/// Every ORBIT page draws on screen, inside 240×320.
#[test]
fn every_orbit_page_renders() {
    use chimera_core::ui::UiState;
    use screen::{Fb, settle};
    let mut ui = Box::new(UiState::new());
    for o in every_orbit_place().into_iter().filter(|o| o.ring() == R[0]) {
        ui.go_for_test(orbit(o));
        settle(&mut ui);
        let mut fb = Fb::new();
        ui.render_with_scope(
            &mut fb,
            &chimera_core::ui::perf::PerfStats::zero(),
            &screen::scope_fixture(),
        );
        assert_eq!(fb.oob, 0, "{o:?} drew off screen");
        assert!(fb.drawn > 0, "{o:?} drew nothing");
    }
}
