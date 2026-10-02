//! ORBIT's pages as drawn and turned: RHYTHM's pattern, CHRD's segments and
//! the CUSTOM circle; and the custom set's record. Their screens are locked
//! in `screen_golden_test` (`orbit_rhythm`, `orbit_chrd`, `orbit_custom`).

mod screen;

use chimera_core::orbit::chords::ScaleKind;
use chimera_core::orbit::clock::step_at;
use chimera_core::orbit::scene::{Scene, decode_scale, encode_scale};
use chimera_core::ui::UiState;
use chimera_core::ui::orbit::{ORBIT_CHRD, ORBIT_CUSTOM, ORBIT_RHYTHM};
use chimera_core::ui::theme;
use chimera_hal::ButtonId::{Edit, Plus, Seq};
use chimera_hal::EncoderId::{A, B};
use chimera_theory::{Interval, Scale};
use screen::{Input, feed, hold, render, tap};

/// Ring 3 of the init scene at phase 0.3, playing: every step a dot on the
/// band's centre line, a hit filled, the step under the clock lit.
#[test]
fn ring_rhythm_golden() {
    let ui = screen::ui_for("orbit_rhythm");
    assert_eq!(ui.page_def().id, ORBIT_RHYTHM.id);
    let fb = render("orbit_rhythm");
    let ring = Scene::init().rings[2].ring;
    let mask = ring.hits_mask();
    let steps = ring.fill().steps();
    assert_eq!((steps.get(), mask.hits()), (13, 5));
    let now = step_at(screen::phase(0.3), steps) as usize;
    let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32 / 13.0;
    for i in 0..13 {
        let x = theme::VIZ_LEFT + (w * (i as f32 + 0.5)) as i32;
        let centre = fb.at(x, theme::VIZ_BAND_MID);
        let want = match (mask.is_hit(i), i == now) {
            (true, true) => theme::INK,
            (true, false) => theme::ACCENT,
            (false, _) => theme::BAR_REST,
        };
        assert_eq!(centre, want, "step {i}");
        let top = fb.at(x, theme::VIZ_BAND_MID - 14);
        let lit = if i == now {
            theme::ACCENT_SOFT
        } else {
            theme::BG
        };
        assert_eq!(top, lit, "the clock's step {i}");
    }
}

/// CHRD › CUSTOM, the controller ruling: SCALE turned to CUSTOM opens the
/// circle on the scale it was; A walks the cursor, B sets and clears; the
/// root stays; EDIT cycles while SCALE is CUSTOM and only then.
#[test]
fn chrd_custom_circle() {
    let mut ui = Box::new(UiState::new());
    let chords = |ui: &UiState| ui.project().orbit().live.chords;
    let page = |ui: &UiState| ui.page_def().id;
    hold(&mut ui, Seq);
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::press(Plus));
    assert_eq!(page(&ui), ORBIT_CHRD.id);
    assert_eq!(chords(&ui).scale, ScaleKind::Minor);

    // A named scale: EDIT does nothing.
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CHRD.id);

    // SCALE (B) from MINOR up to CUSTOM: the circle, holding MINOR.
    feed(&mut ui, Input::turn(B, 9));
    assert_eq!(chords(&ui).scale, ScaleKind::Custom);
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
    assert_eq!(chords(&ui).custom, Scale::MINOR);

    // A: the cursor round the 12, both ways.
    assert_eq!(ui.orbit().cursor, 0);
    feed(&mut ui, Input::turn(A, 13));
    assert_eq!(ui.orbit().cursor, 1);
    feed(&mut ui, Input::turn(A, -2));
    assert_eq!(ui.orbit().cursor, 11);
    feed(&mut ui, Input::turn(A, 2));
    assert_eq!(ui.orbit().cursor, 1);

    // B right sets the minor second, again and again; left clears it.
    let has = |ui: &UiState, i: usize| chords(ui).custom.contains(Interval::ALL[i]);
    assert!(!has(&ui, 1));
    for _ in 0..2 {
        feed(&mut ui, Input::turn(B, 3));
        assert!(has(&ui, 1));
    }
    let set = chords(&ui).custom;
    for _ in 0..2 {
        feed(&mut ui, Input::turn(B, -1));
        assert!(!has(&ui, 1));
    }
    assert_eq!(chords(&ui).custom, set.toggle(Interval::ALL[1]));

    // The root can't be cleared.
    feed(&mut ui, Input::turn(A, -1));
    assert_eq!(ui.orbit().cursor, 0);
    feed(&mut ui, Input::turn(B, -5));
    assert!(has(&ui, 0));
    assert_eq!(chords(&ui).custom, Scale::MINOR);

    // EDIT: back to CHRD, and again to CUSTOM.
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CHRD.id);
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);

    // A set no named scale is stays the user's when SCALE comes back.
    feed(&mut ui, Input::turn(A, 1));
    feed(&mut ui, Input::turn(B, 1));
    let own = chords(&ui).custom;
    assert!(!Scale::ALL.contains(&own));
    tap(&mut ui, Edit);
    feed(&mut ui, Input::turn(B, -2));
    assert_eq!(chords(&ui).scale, ScaleKind::PentatonicMajor);
    feed(&mut ui, Input::turn(B, 2));
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
    assert_eq!(chords(&ui).custom, own);

    // Turned off CUSTOM, EDIT on CHRD does nothing again.
    tap(&mut ui, Edit);
    feed(&mut ui, Input::turn(B, -1));
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CHRD.id);

    // The screen: `orbit_custom` in `screen_golden_test`.
    let ui = screen::ui_for("orbit_custom");
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
}

/// Every set of the 12 round-trips through `OrbitScale`; a file's bit 0
/// is set whatever it says. SCALE's codes are pinned by `orbit_codes_pinned`.
#[test]
fn custom_scale_round_trips() {
    for m in 0..4096u16 {
        let set: Vec<Interval> = (0..12)
            .filter(|i| m >> i & 1 == 1)
            .map(|i| Interval::ALL[i])
            .collect();
        let s = Scale::custom(&set);
        let p = encode_scale(5, s);
        assert_eq!(p[0], 5);
        assert_eq!(u16::from_le_bytes([p[1], p[2]]), m | 1, "{m:#05x}");
        assert_eq!(decode_scale(&p), Some((5, s)), "{m:#05x}");
    }
    let fifth = Scale::custom(&[Interval::FIFTH]);
    assert_eq!(decode_scale(&[2, 0x80, 0x00]), Some((2, fifth)), "root set");
    assert_eq!(
        decode_scale(&[0, 0x00, 0x00]),
        Some((0, Scale::custom(&[])))
    );
    let all = Scale::custom(&Interval::ALL);
    assert_eq!(
        decode_scale(&[0, 0xFF, 0xFF]),
        Some((0, all)),
        "bits past 11"
    );
    assert_eq!(decode_scale(&[0, 0x01]), None, "short");
    assert_eq!(decode_scale(&[0, 0x01, 0x00, 0x00]), None, "long");
}

/// The dirty path redraws what ORBIT's own data moves: the ring in the
/// header, the segment DEGREE edits, the runner's step and segment, the
/// CUSTOM cursor and set. Each frame matches a full render.
#[test]
fn the_dirty_path_follows_orbit() {
    use chimera_core::ui::perf::PerfStats;
    use chimera_hal::ButtonId::{B1, Mix};
    use chimera_hal::EncoderId::E;
    use screen::{Fb, audio_fixture, orbit_view, render_ui, scope_fixture, settle, ui_for};

    fn frame(ui: &mut UiState, fb: &mut Fb, what: &str) {
        settle(ui);
        ui.render_dirty_with_audio(
            fb,
            &PerfStats::zero(),
            Some(&audio_fixture()),
            &scope_fixture(),
        );
        assert!(fb.px == render_ui(ui).px, "{what}");
    }

    let mut ui = Box::new(ui_for("orbit_rhythm"));
    let mut fb = Fb::new();
    frame(&mut ui, &mut fb, "RHYTHM");
    ui.set_orbit_view(&orbit_view(0.6));
    frame(&mut ui, &mut fb, "a later step");
    tap(&mut ui, B1);
    frame(&mut ui, &mut fb, "ring 1");

    let mut ui = Box::new(ui_for("orbit_chrd"));
    let mut fb = Fb::new();
    frame(&mut ui, &mut fb, "CHRD");
    feed(&mut ui, Input::turn(E, 1).and_held(Mix));
    frame(&mut ui, &mut fb, "segment 4");
    feed(&mut ui, Input::turn(E, -3).and_held(Mix));
    frame(&mut ui, &mut fb, "segment 1, the same degree");
    ui.set_orbit_view(&orbit_view(0.8));
    frame(&mut ui, &mut fb, "the last segment playing");

    let mut ui = Box::new(ui_for("orbit_custom"));
    let mut fb = Fb::new();
    frame(&mut ui, &mut fb, "CUSTOM");
    feed(&mut ui, Input::turn(A, 1));
    frame(&mut ui, &mut fb, "the cursor");
    feed(&mut ui, Input::turn(B, 1));
    frame(&mut ui, &mut fb, "a note set");
}

/// DEGREE turns from what plays: a degree past a smaller set (left over
/// from a larger one) steps down from the set's last, never stays stuck
/// past it.
#[test]
fn degree_turns_from_the_sets_last() {
    use chimera_core::orbit::chords::ScaleKind;
    use chimera_hal::EncoderId::E;
    use chimera_theory::Degree;
    let mut ui = Box::new(UiState::new());
    hold(&mut ui, Seq);
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::press(Plus));
    assert_eq!(ui.page_def().id, ORBIT_CHRD.id);
    let c = &mut ui.project_mut().orbit_mut().live.chords;
    c.scale = ScaleKind::PentatonicMinor;
    c.degrees[0] = Degree::ALL[11];
    feed(&mut ui, Input::turn(E, -1));
    assert_eq!(ui.project().orbit().live.chords.degrees[0], Degree::IV);
    let c = &mut ui.project_mut().orbit_mut().live.chords;
    c.degrees[0] = Degree::ALL[11];
    feed(&mut ui, Input::turn(E, 1));
    assert_eq!(ui.project().orbit().live.chords.degrees[0], Degree::V);
}

/// EDIT on CUSTOM returns to CHRD even when SCALE is named under it (a
/// load or a scene switch changed it), so the page is never a dead end.
#[test]
fn custom_edit_returns_with_scale_named() {
    let mut ui = Box::new(UiState::new());
    hold(&mut ui, Seq);
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::turn(B, 9));
    assert_eq!(ui.page_def().id, ORBIT_CUSTOM.id);
    ui.project_mut().orbit_mut().live.chords.scale = ScaleKind::Dorian;
    tap(&mut ui, Edit);
    assert_eq!(ui.page_def().id, ORBIT_CHRD.id);
    tap(&mut ui, Edit);
    assert_eq!(ui.page_def().id, ORBIT_CHRD.id, "named: no CUSTOM");
}

/// CUSTOM's cursor eases the short way: across 11 and 0 it moves one
/// note, not round the circle.
#[test]
fn the_cursor_eases_the_short_way() {
    use chimera_core::ui::orbit::draw::nearest_turn;
    assert_eq!(nearest_turn(0.0, 11), -1.0);
    assert_eq!(nearest_turn(11.0, 0), 12.0);
    assert_eq!(nearest_turn(-1.0, 11), -1.0);
    assert_eq!(nearest_turn(12.0, 1), 13.0);
    assert_eq!(nearest_turn(3.0, 5), 5.0);
    assert_eq!(nearest_turn(2.5, 8), 8.0);
    assert_eq!(nearest_turn(2.0, 9), -3.0);
    // Mid-ease, the circle draws between the two notes, near the top.
    let mut ui = Box::new(screen::ui_for("orbit_custom"));
    feed(&mut ui, Input::turn(A, -6)); // 6 → 0
    screen::settle(&mut ui);
    feed(&mut ui, Input::turn(A, -1)); // 0 → 11
    let mid = screen::render_ui(&ui).px;
    ui.update(chimera_core::ui::animation::UiTick::for_test());
    let moving = screen::render_ui(&ui).px;
    screen::settle(&mut ui);
    let rest = screen::render_ui(&ui).px;
    assert!(mid != moving && moving != rest, "it eases");
    assert_eq!(ui.orbit().cursor, 11);
    // The circle's lower half (y 112..180) never sees the cursor pass.
    let low = |px: &[u16]| px[112 * screen::W..180 * screen::W].to_vec();
    assert!(low(&moving) == low(&rest), "not round the bottom");
}
