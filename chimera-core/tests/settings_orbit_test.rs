//! SETTINGS › ORBIT (#262): CLEAR, SAVE TO PROJ and RELOAD FROM PROJ act
//! on the playing scene, each behind its prompt; RENAME is later (#298).
#![cfg(feature = "orbit")]

mod screen;

use chimera_core::block::Block;

use chimera_core::orbit::engine::OrbitKind;
use chimera_core::orbit::scene::{Scene, SceneIdx, SceneRing};
use chimera_core::project::{Confirmed, PartId, SceneSlot};
use chimera_core::ui::UiState;
use chimera_core::ui::busy::ToastStep;
use chimera_core::ui::settings::prompt::{
    Answers, ClearScene, ClearSceneAnswer, Opts, ReloadScene, ReloadSceneAnswer, with_view,
};
use chimera_core::ui::settings::replace::Commits;
use chimera_core::ui::settings::view::{OrbitBand, RowLook};
use chimera_core::ui::settings::{Kind, Screen, rows, screen_path};
use chimera_hal::ButtonId::{B2, B6, Edit, Menu, Plus, Seq};
use chimera_hal::EncoderId::A;
use screen::{Input, ShellRig, feed, hold, plus, tap, to_leaf};

const S: [SceneIdx; 8] = SceneIdx::ALL;
/// ORBIT's rows, top to bottom.
const RENAME: usize = 0;
const CLEAR: usize = 1;
const SAVE_TO: usize = 2;
const RELOAD: usize = 3;

/// SETTINGS › ORBIT, the bar on `row`; from ORBIT's pages, MENU leaves
/// them first.
fn orbit_row(ui: &mut UiState, row: usize) {
    while ui.location().orbit().is_some() {
        tap(ui, Menu);
    }
    to_leaf(ui, &["ORBIT"]);
    plus(ui, row);
}

fn question(ui: &UiState) -> String {
    ui.prompt_words_for_test()
        .map(|(q, _)| q.as_str().into())
        .unwrap_or_default()
}

fn toast(ui: &mut UiState) -> String {
    match ui.step_toast(0) {
        ToastStep::Show(l) => l.as_str().into(),
        s => panic!("no toast: {s:?}"),
    }
}

fn slot(ui: &UiState, n: usize) -> Scene {
    *ui.project().orbit().scene(S[n])
}

fn band(ui: &UiState) -> OrbitBand {
    OrbitBand::of(ui.project().orbit())
}

/// The look of ORBIT's row `i`, as its list draws it.
fn look(ui: &UiState, i: usize) -> RowLook {
    band(ui).look(rows(&to_orbit())[i].kind)
}

/// ORBIT's path from the top.
fn to_orbit() -> Vec<u8> {
    let i = rows(&[]).iter().position(|r| r.label == "ORBIT").unwrap();
    vec![i as u8]
}

/// Inside ORBIT and playing, then SETTINGS › ORBIT: the transport runs on.
fn playing() -> ShellRig {
    let mut ui = Box::new(UiState::new());
    hold(&mut ui, Seq);
    assert!(ui.location().orbit().is_some());
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running && v.loops >= 1);
    rig
}

/// Frames of one block until a loop ends, then one more, which reads the
/// boundary's view.
fn past_the_loop(rig: &mut ShellRig) {
    let loops = rig.view().loops;
    rig.until(|v| v.loops != loops);
    rig.idle(1);
}

#[test]
fn orbit_list_reaches_every_row() {
    let mut ui = UiState::new();
    to_leaf(&mut ui, &["ORBIT"]);
    let labels: Vec<_> = rows(&to_orbit()).iter().map(|r| r.label).collect();
    assert_eq!(
        labels,
        ["RENAME", "CLEAR", "SAVE TO PROJ", "RELOAD FROM PROJ"]
    );
    assert!(matches!(
        rows(&to_orbit())[RENAME].kind,
        Kind::Later(i) if i.get() == 298
    ));
    assert_eq!(look(&ui, RENAME), RowLook::Later);
    assert_eq!(look(&ui, CLEAR), RowLook::Normal);
    assert_eq!(look(&ui, SAVE_TO), RowLook::Normal);
    assert_eq!(ui.crumbs().unwrap().to_string(), "SETTINGS › SCENE 1");
    // RENAME is inert.
    for k in [Edit, Seq] {
        tap(&mut ui, k);
        assert!(!ui.prompt_open());
        assert!(ui.naming().is_none());
        assert_eq!(ui.location().settings().unwrap().path(), to_orbit());
    }
    for i in 1..4 {
        feed(&mut ui, Input::press(Plus));
        assert_eq!(ui.location().settings().unwrap().row() as usize, i);
    }
    feed(&mut ui, Input::press(Plus));
    assert_eq!(ui.location().settings().unwrap().row(), 0, "wraps");

    plus(&mut ui, SAVE_TO);
    feed(&mut ui, Input::press(Edit));
    assert_eq!(
        ui.location().settings().unwrap().path(),
        screen_path(Screen::SceneSaveTo)
    );
    // SETTINGS behind `..`: the crumbs keep left of the beat dot.
    let crumbs = ui.crumbs().unwrap().to_string();
    assert_eq!(crumbs, ".. › SCENE 1 › SAVE TO");
}

/// CLEAR on the playing scene: CANCEL and MENU leave the slot; CLEAR
/// makes it neutral, and `live` lands neutral on the loop, silent.
#[test]
fn clear_asks_and_clears() {
    let mut rig = playing();
    let one = slot(&rig.ui, 0);
    assert_ne!(one, Scene::neutral());
    orbit_row(&mut rig.ui, CLEAR);
    for cancel in [Menu, Seq] {
        rig.tap(Seq);
        assert_eq!(question(&rig.ui), "CLEAR SCENE 1?");
        if cancel == Seq {
            rig.frame(Input::turn(A, 1), 1); // CANCEL
        }
        rig.tap(cancel);
        assert!(!rig.ui.prompt_open());
        assert!(rig.ui.in_settings(), "{cancel:?} closes the prompt only");
        assert_eq!(slot(&rig.ui, 0), one, "{cancel:?} leaves it");
        assert_eq!(rig.ui.project().orbit().queued(), None);
    }
    let live = rig.ui.project().orbit().live;
    rig.tap(Seq);
    rig.tap(Seq); // CLEAR
    assert!(!rig.ui.prompt_open());
    let song = rig.ui.project().orbit();
    assert_eq!(slot(&rig.ui, 0), Scene::neutral());
    assert_eq!(song.saved() & 1, 0);
    assert_eq!(song.live, live, "live waits for the loop");
    assert_eq!(song.queued(), Some(S[0]));
    assert_eq!(rig.view().queued, Some(S[0]), "the runner holds it");
    assert_eq!(rig.view().playing, S[0]);

    past_the_loop(&mut rig);
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[0], None));
    assert_eq!(song.live, Scene::neutral(), "landed");
    let from = rig.sent.len();
    past_the_loop(&mut rig);
    let ons = rig.sent[from..]
        .iter()
        .flatten()
        .filter(|e| matches!(e.kind, OrbitKind::On(_)))
        .count();
    assert_eq!(ons, 0, "a neutral scene is silent");
}

/// SAVE TO PROJ: a row per slot, `OVER SCENE n` on a saved one and
/// `SCENE n` on a neutral one, the bar on the playing one. SEQ saves at
/// once over the playing or a neutral slot; over another saved scene, SAVE
/// OVER asks.
#[test]
fn save_to_lists_eight_slots() {
    let mut ui = UiState::new();
    let labels = |ui: &UiState| -> Vec<String> {
        S.iter()
            .map(|&n| band(ui).save_label(n).as_str().to_string())
            .collect()
    };
    let mut want: Vec<String> = (1..=8).map(|n| format!("SCENE {n}")).collect();
    want[0] = "OVER SCENE 1".into();
    assert_eq!(labels(&ui), want);
    // Slot 5 saved.
    ui.project_mut().orbit_mut().live.rings[0].set(SceneRing::MUTE, 1.0);
    let c = Confirmed::<SceneSlot>::unasked(ui.project().orbit(), S[4]);
    ui.project_mut().orbit_mut().overwrite_scene(c).unwrap();
    want[4] = "OVER SCENE 5".into();
    assert_eq!(labels(&ui), want);

    orbit_row(&mut ui, SAVE_TO);
    feed(&mut ui, Input::press(Edit));
    assert_eq!(
        ui.location().settings().unwrap().path(),
        screen_path(Screen::SceneSaveTo)
    );
    assert_eq!(
        ui.location().settings().unwrap().row(),
        0,
        "the playing one"
    );
    plus(&mut ui, 8);
    assert_eq!(ui.location().settings().unwrap().row(), 0, "eight rows");

    // A neutral slot: at once.
    plus(&mut ui, 2);
    tap(&mut ui, Seq);
    assert!(!ui.prompt_open());
    assert_eq!(toast(&mut ui), "SCENE 3 SAVED");
    assert_eq!(slot(&ui, 2), ui.project().orbit().live);
    want[2] = "OVER SCENE 3".into();
    assert_eq!(labels(&ui), want);
    // Another saved slot: it asks.
    ui.project_mut().orbit_mut().live.rings[2].set(SceneRing::MUTE, 1.0);
    plus(&mut ui, 2);
    tap(&mut ui, Seq);
    assert_eq!(question(&ui), "SAVE OVER SCENE 5?");
    tap(&mut ui, Menu);
    assert!(!ui.prompt_open());
    assert_ne!(slot(&ui, 4), ui.project().orbit().live);
    // The playing slot: at once.
    plus(&mut ui, 4);
    assert_eq!(ui.location().settings().unwrap().row(), 0);
    tap(&mut ui, Seq);
    assert!(!ui.prompt_open());
    assert_eq!(toast(&mut ui), "SCENE 1 SAVED");
    assert_eq!(slot(&ui, 0), ui.project().orbit().live);
}

/// Ten loops of MUTATE drift `live` from its slot; RELOAD drops the drift
/// on the loop, and the masks play as saved. With nothing to drop, RELOAD
/// is dimmed and SEQ on it asks nothing.
#[test]
fn reload_drops_the_drift() {
    let mut ui = UiState::new();
    orbit_row(&mut ui, RELOAD);
    assert_eq!(look(&ui, RELOAD), RowLook::Dimmed, "NEW: nothing to drop");
    tap(&mut ui, Seq);
    assert!(!ui.prompt_open());

    let mut ui = Box::new(UiState::new());
    for r in &mut ui.project_mut().orbit_mut().live.rings {
        r.set(SceneRing::MUTATE, 100.0);
    }
    ui.project_mut().orbit_mut().save_scene().unwrap();
    let saved = slot(&ui, 0);
    hold(&mut ui, Seq);
    let mut rig = ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.loops >= 10);
    rig.idle(1);
    assert_ne!(rig.ui.project().orbit().live, saved, "drifted");
    orbit_row(&mut rig.ui, RELOAD);
    assert_eq!(look(&rig.ui, RELOAD), RowLook::Normal);
    rig.tap(Seq);
    assert_eq!(question(&rig.ui), "RELOAD SCENE 1?");
    rig.tap(Seq); // RELOAD
    assert!(!rig.ui.prompt_open());
    assert_eq!(rig.ui.project().orbit().queued(), Some(S[0]));

    past_the_loop(&mut rig);
    let song = rig.ui.project().orbit();
    assert_eq!((song.current(), song.queued()), (S[0], None));
    assert_eq!(song.live, saved, "the drift is dropped");
    let masks = |s: &Scene| s.rings.map(|r| r.ring.hits_mask().bits());
    let heard = rig.view().rings.map(|r| r.hits.bits());
    assert_eq!(heard, masks(&saved), "the saved masks play");
    assert_eq!(look(&rig.ui, RELOAD), RowLook::Dimmed);
    rig.tap(Seq);
    assert!(!rig.ui.prompt_open());
}

/// B2 inside SETTINGS › ORBIT › SAVE TO leaves for Part 2's pages, as
/// from anywhere in SETTINGS.
#[test]
fn b_n_leaves_from_any_depth() {
    let mut ui = UiState::new();
    orbit_row(&mut ui, SAVE_TO);
    feed(&mut ui, Input::press(Edit));
    assert!(ui.location().settings().unwrap().screen() == Some(Screen::SceneSaveTo));
    feed(&mut ui, Input::press(B2));
    assert!(!ui.in_settings());
    assert_eq!(ui.location().part(), Some(PartId::ALL[1]));
    assert!(ui.location().orbit().is_none());
    // B6 too: it is Part 6's key in SETTINGS, never PLAY.
    orbit_row(&mut ui, CLEAR);
    feed(&mut ui, Input::press(B6));
    assert_eq!(ui.location().part(), Some(PartId::ALL[5]));
    assert!(!ui.orbit().run);
}

/// Each scene prompt's `ALL` is its pills' order, and its pills commit by
/// meaning: the pill picked by its answer, wherever it stands, does what
/// it says.
#[test]
fn scene_prompts_answer_by_meaning() {
    assert_eq!(
        ClearSceneAnswer::ALL,
        Opts::Two([ClearSceneAnswer::Clear, ClearSceneAnswer::Cancel])
    );
    assert_eq!(
        ReloadSceneAnswer::ALL,
        Opts::Two([ReloadSceneAnswer::Reload, ReloadSceneAnswer::Cancel])
    );
    assert_eq!(ClearSceneAnswer::CONFIRM, ClearSceneAnswer::Clear);
    assert_eq!(ReloadSceneAnswer::CONFIRM, ReloadSceneAnswer::Reload);
    let clear = ClearScene { scene: S[2] };
    with_view(&clear, &Default::default(), |v| {
        assert_eq!(v.question, "CLEAR SCENE 3?");
        assert_eq!(v.options(), ["CLEAR", "CANCEL"]);
    });
    let reload = ReloadScene { scene: S[2] };
    with_view(&reload, &Default::default(), |v| {
        assert_eq!(v.question, "RELOAD SCENE 3?");
        assert_eq!(v.options(), ["RELOAD", "CANCEL"]);
    });

    // The pill reached by its answer's place in `ALL`.
    let pick = |ui: &mut UiState, at: usize| {
        feed(ui, Input::turn(A, at as i8));
        tap(ui, Seq);
        assert!(!ui.prompt_open());
    };
    let place = |a: ClearSceneAnswer| {
        ClearSceneAnswer::ALL
            .as_slice()
            .iter()
            .position(|&x| x == a)
    };
    let mut ui = UiState::new();
    let one = slot(&ui, 0);
    orbit_row(&mut ui, CLEAR);
    tap(&mut ui, Seq);
    pick(&mut ui, place(ClearSceneAnswer::Cancel).unwrap());
    assert_eq!(slot(&ui, 0), one);
    tap(&mut ui, Seq);
    pick(&mut ui, place(ClearSceneAnswer::Clear).unwrap());
    assert_eq!(slot(&ui, 0), Scene::neutral());

    let place = |a: ReloadSceneAnswer| {
        ReloadSceneAnswer::ALL
            .as_slice()
            .iter()
            .position(|&x| x == a)
    };
    let mut ui = UiState::new();
    ui.project_mut().orbit_mut().live.rings[0].set(SceneRing::MUTE, 1.0);
    orbit_row(&mut ui, RELOAD);
    tap(&mut ui, Seq);
    pick(&mut ui, place(ReloadSceneAnswer::Cancel).unwrap());
    assert_eq!(ui.project().orbit().queued(), None);
    tap(&mut ui, Seq);
    pick(&mut ui, place(ReloadSceneAnswer::Reload).unwrap());
    assert_eq!(ui.project().orbit().queued(), Some(S[0]));
}

/// The crumb follows the playing scene: a switch queued on SCN lands while
/// SETTINGS › ORBIT is open, and the breadcrumb redraws to name it.
#[test]
fn the_crumb_follows_the_playing_scene() {
    use chimera_core::ui::region::RegionKind;
    let mut rig = playing();
    for _ in 0..3 {
        feed(&mut rig.ui, Input::press(Plus));
    }
    rig.frame(Input::turn(A, 1), 1);
    assert_eq!(rig.ui.project().orbit().queued(), Some(S[1]));
    orbit_row(&mut rig.ui, CLEAR);
    let mut fb = screen::Fb::new();
    let perf = chimera_core::ui::perf::PerfStats::zero();
    rig.ui
        .render_dirty_with_scope(&mut fb, &perf, &screen::scope_fixture());
    let before = rig.ui.drawn_key(RegionKind::Crumbs);
    assert_eq!(rig.ui.crumbs().unwrap().to_string(), "SETTINGS › SCENE 1");
    past_the_loop(&mut rig);
    assert_eq!(rig.ui.project().orbit().current(), S[1]);
    rig.ui
        .render_dirty_with_scope(&mut fb, &perf, &screen::scope_fixture());
    assert_ne!(rig.ui.drawn_key(RegionKind::Crumbs), before, "redrawn");
    assert_eq!(rig.ui.crumbs().unwrap().to_string(), "SETTINGS › SCENE 2");
}

/// Once the runner has landed the cleared scene, the UI's `settings` are
/// still the old `live` until its view lands it; the runner takes no
/// knobs from them meanwhile. A slow UI frame across the boundary hears
/// the neutral scene, silent, from the boundary on.
#[test]
fn a_landing_takes_no_knobs_the_ui_has_not_landed() {
    let mut rig = playing();
    orbit_row(&mut rig.ui, CLEAR);
    rig.tap(Seq);
    rig.tap(Seq); // CLEAR
    assert_eq!(rig.ui.project().orbit().queued(), Some(S[0]));
    let loops = rig.view().loops;
    rig.until(|v| v.loops == loops && v.queued.is_some());
    // Slow frames, about 1.3 s of blocks each: the boundary falls inside one.
    let from = rig.sent.len();
    let mut crossed = None;
    while crossed.is_none() {
        rig.idle(997);
        crossed = rig.views[from..]
            .iter()
            .position(|v| v.loops != loops)
            .map(|i| from + i);
    }
    let ons = rig.sent[crossed.unwrap()..]
        .iter()
        .flatten()
        .filter(|e| matches!(e.kind, OrbitKind::On(_)))
        .count();
    assert_eq!(ons, 0, "the old knobs unmute nothing");
}

/// While a switch waits, CLEAR and RELOAD ask nothing and say why, as SEQ's
/// save does: what lands stays what was queued. The song refuses a CLEAR
/// confirmed before the switch was queued, too.
#[test]
fn clear_and_reload_wait_for_a_switch() {
    let mut ui = UiState::new();
    hold(&mut ui, Seq);
    for _ in 0..3 {
        feed(&mut ui, Input::press(Plus));
    }
    let before = slot(&ui, 0);
    let c = Confirmed::<SceneSlot>::unasked(ui.project().orbit(), S[0]);
    feed(&mut ui, Input::turn(A, 1));
    assert_eq!(ui.project().orbit().queued(), Some(S[1]));
    ui.project_mut().orbit_mut().live.rings[0].set(SceneRing::MUTE, 1.0);
    for row in [CLEAR, RELOAD] {
        orbit_row(&mut ui, row);
        assert_eq!(look(&ui, row), RowLook::Normal);
        tap(&mut ui, Seq);
        assert!(!ui.prompt_open(), "row {row}");
        assert_eq!(toast(&mut ui), "SWITCH PENDING");
    }
    assert_eq!(
        ui.project_mut().orbit_mut().clear_scene(c),
        Err(chimera_core::orbit::scene::Refused::SwitchPending)
    );
    assert_eq!(slot(&ui, 0), before);
    assert_eq!(ui.project().orbit().queued(), Some(S[1]));
}
