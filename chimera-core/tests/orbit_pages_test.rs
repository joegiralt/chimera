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

/// ORB at phase 0.3, ring 2 just fired: every ring's steps round its
/// circle, a hit filled, an accent ringed in INK2, the step under the
/// clock lit, ring 4 dark (muted); the progression's first segment lit;
/// the sweep at 0.3 of the turn. One tick after ring 2 fires again its
/// step flashes, and is back by the third.
#[test]
fn orb_page_golden() {
    use chimera_core::orbit::RingIdx;
    use chimera_core::orbit::euclid::accents;
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::orbit::ORBIT_ORB;
    use chimera_core::ui::orbit::draw::{BAND_R, orb_xy, ring_r};
    let ui = screen::ui_for("orbit_orb");
    assert_eq!(ui.page_def().id, ORBIT_ORB.id);
    let fb = render("orbit_orb");
    let scene = Scene::init();
    let phase = screen::phase(0.3);
    let step_xy = |r: RingIdx, i: usize, n: usize| orb_xy(i as f32 / n as f32, ring_r(r) as f32);
    for r in RingIdx::ALL {
        let ring = &scene.rings[r.get() as usize].ring;
        let mask = ring.hits_mask();
        let steps = ring.fill().steps();
        let n = steps.get() as usize;
        let now = step_at(phase, steps) as usize;
        let accented = accents(mask, ring.accent);
        for i in 0..n {
            let (x, y) = step_xy(r, i, n);
            let want = match (ring.mute, mask.is_hit(i), i == now) {
                (true, true, _) => theme::PILL_EDGE,
                (true, false, _) => theme::FAINT,
                (false, true, true) => theme::INK,
                (false, true, false) => theme::ACCENT,
                (false, false, _) => theme::BAR_REST,
            };
            assert_eq!(fb.at(x, y), want, "ring {} step {i}", r.get() + 1);
            if accented.is_hit(i) && !ring.mute {
                assert_eq!(
                    fb.at(x + 4, y),
                    theme::INK2,
                    "ring {} accent {i}",
                    r.get() + 1
                );
            }
        }
    }
    assert!(scene.rings[3].ring.mute, "ring 4 is the muted one");

    // The progression: four segments, the first lit while it plays.
    let segs = scene.chords.segs.get() as usize;
    assert_eq!(segs, 4);
    for s in 0..segs {
        let (x, y) = orb_xy((s as f32 + 0.5) / segs as f32, BAND_R as f32);
        let want = if s == 0 {
            theme::ACCENT
        } else {
            theme::ACCENT_SOFT
        };
        assert_eq!(fb.at(x, y), want, "segment {s}");
    }

    // The sweep: a line at 0.3 of the turn, between the outer rings.
    let mid = (ring_r(RingIdx::ALL[0]) + ring_r(RingIdx::ALL[1])) as f32 / 2.0;
    let (x, y) = orb_xy(0.3, mid);
    let near = (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (x + dx, y + dy)));
    assert!(
        near.clone().any(|(x, y)| fb.at(x, y) == theme::MID),
        "the hand at 0.3"
    );
    let (x, y) = orb_xy(0.8, mid);
    assert_eq!(fb.at(x, y), theme::BG, "no hand across");

    // Ring 2 fires again: its step flashes, larger, and fades by the third tick.
    let mut ui = Box::new(ui);
    let r2 = RingIdx::ALL[1];
    let steps = scene.rings[1].ring.fill().steps();
    let (x, y) = step_xy(r2, step_at(phase, steps) as usize, steps.get() as usize);
    let at_rest = screen::render_ui(&ui);
    assert_eq!(at_rest.at(x + 3, y), theme::BG, "at rest, a small dot");
    let mut v = screen::orb_view(0.3);
    v.rings[1].fired = 2;
    ui.set_orbit_view(&v);
    ui.update(UiTick::for_test());
    let lit = screen::render_ui(&ui);
    assert_eq!(lit.at(x + 3, y), theme::INK, "the flash");
    ui.update(UiTick::for_test());
    ui.update(UiTick::for_test());
    assert!(
        screen::render_ui(&ui).px == at_rest.px,
        "back by the third tick"
    );
}

/// The beat dot, left of the sounding dot on every kind of page: absent
/// while ORBIT is stopped; running, filled on a beat's first tick, part
/// filled on the next, an outline by the third, and filled again on the
/// next beat. The CPU label moves left by one dot. The dirty path
/// redraws each step as a full render would.
#[test]
fn header_dot_pulses() {
    use chimera_core::orbit::runner::OrbitView;
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::RegionKind;
    use screen::{Fb, audio_fixture, orbit_view, scope_fixture};
    const LOAD: PerfStats = PerfStats {
        frame_us: 0,
        audio_us: 0,
        audio_load_pct: 42,
    };
    let full = |ui: &UiState| {
        let mut fb = Fb::new();
        ui.render_with_audio(&mut fb, &LOAD, Some(&audio_fixture()), &scope_fixture());
        fb
    };
    // The CPU label's right edge: the header's rightmost MID pixel.
    let cpu_right = |fb: &Fb| {
        (0..theme::SCREEN_W)
            .rev()
            .find(|&x| (0..theme::HEADER_BOTTOM).any(|y| fb.at(x, y) == theme::MID))
    };
    let (x, y) = (theme::ORBIT_DOT_X, theme::HEADER_DOT_Y);
    let r = theme::HEADER_DOT_R;
    for (name, cpu) in [
        ("algo_level", true),
        ("mixer_part", true),
        ("settings_top", false),
        ("settings_personal_theme", false),
        ("orbit_orb", true),
    ] {
        let mut ui = Box::new(screen::ui_for(name));
        ui.set_orbit_view(&OrbitView::default());
        screen::settle(&mut ui);
        let mut dirty = Fb::new();
        // The band the dots are in: a page's header, SETTINGS' crumbs.
        let head = |ui: &UiState| {
            ui.drawn_key(RegionKind::Header)
                .or(ui.drawn_key(RegionKind::Crumbs))
                .expect("a header band")
        };
        let mut step = |ui: &mut UiState, what: &str| {
            ui.update(UiTick::for_test());
            ui.render_dirty_with_audio(&mut dirty, &LOAD, Some(&audio_fixture()), &scope_fixture());
            let fb = full(ui);
            assert!(dirty.px == fb.px, "{name}: {what}, dirty");
            (fb, head(ui))
        };
        let (stopped, _) = step(&mut ui, "stopped");
        for dx in -r..=r {
            assert_eq!(stopped.at(x + dx, y), theme::BG, "{name}: stopped, no dot");
        }

        ui.set_orbit_view(&orbit_view(0.0));
        let (first, k1) = step(&mut ui, "the beat");
        assert_eq!(first.at(x, y), theme::ACCENT, "{name}: filled");
        assert_eq!(first.at(x + 2, y), theme::ACCENT, "{name}: filled");
        let (second, k2) = step(&mut ui, "fading");
        assert_eq!(second.at(x, y), theme::ACCENT, "{name}: part filled");
        assert_eq!(second.at(x + 2, y), theme::BG, "{name}: part filled");
        let (third, k3) = step(&mut ui, "faded");
        assert_eq!(third.at(x, y), theme::BG, "{name}: an outline");
        assert_eq!(third.at(x + r, y), theme::ACCENT, "{name}: an outline");
        let (still, k4) = step(&mut ui, "between beats");
        assert!(still.px == third.px, "{name}: still between beats");
        // The band redraws as the dot fades, and not between beats.
        assert!(k1 != k2 && k2 != k3, "{name}: each fade step redraws");
        assert_eq!(k3, k4, "{name}: the band rests between beats");

        ui.set_orbit_view(&orbit_view(0.25));
        let (next, _) = step(&mut ui, "the next beat");
        assert_eq!(next.at(x + 2, y), theme::ACCENT, "{name}: the next beat");

        if cpu {
            let (s, run) = (cpu_right(&stopped), cpu_right(&third));
            assert_eq!(
                s.zip(run).map(|(s, run)| s - run),
                Some(theme::HEADER_DOT_X - theme::ORBIT_DOT_X),
                "{name}: the CPU label moves left by one dot"
            );
        }
    }
}

/// ORB's encoders: A picks the ring, one to five; F sets the BPM.
#[test]
fn orb_encoders() {
    use chimera_core::ui::orbit::ORBIT_ORB;
    use chimera_hal::EncoderId::F;
    let mut ui = Box::new(UiState::new());
    hold(&mut ui, Seq);
    assert_eq!(ui.page_def().id, ORBIT_ORB.id);
    let ring = |ui: &UiState| ui.location().orbit().unwrap().ring().get();
    assert_eq!(ring(&ui), 0);
    feed(&mut ui, Input::turn(A, 2));
    assert_eq!(ring(&ui), 2);
    assert_eq!(ui.page_def().id, ORBIT_ORB.id);
    feed(&mut ui, Input::turn(A, 9));
    assert_eq!(ring(&ui), 4);
    feed(&mut ui, Input::turn(A, -9));
    assert_eq!(ring(&ui), 0);
    let bpm = |ui: &UiState| ui.project().orbit().bpm.get();
    assert_eq!(bpm(&ui), 120);
    feed(&mut ui, Input::turn(F, 5));
    assert_eq!(bpm(&ui), 125);
    feed(&mut ui, Input::turn(F, -10));
    assert_eq!(bpm(&ui), 115);
}

/// `OrbitStatus::IDLE` is what `of` makes of the runner's first view:
/// the two derivations can't drift.
#[test]
fn idle_is_the_first_view() {
    use chimera_core::orbit::runner::OrbitView;
    use chimera_core::ui::orbit::OrbitStatus;
    assert_eq!(OrbitStatus::IDLE, OrbitStatus::of(&OrbitView::default()));
}
