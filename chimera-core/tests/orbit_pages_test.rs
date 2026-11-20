//! ORBIT's pages as drawn and turned: RHYTHM's pattern, VOICE's bars,
//! NOTES' chips, CHRD's segments and ORB's CUSTOM circle; and the custom
//! set's record. Their screens are locked in `screen_golden_test`
//! (`orbit_rhythm`, `orbit_voice`, `orbit_notes`, `orbit_notes_step`,
//! `orbit_chrd`, `orbit_custom`).

// ORBIT's UI: SEQ hold opens it only with the feature.
#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::orbit::chords::ScaleKind;
use chimera_core::orbit::clock::step_at;
use chimera_core::orbit::scene::{Scene, decode_scale, encode_scale};
use chimera_core::ui::UiState;
use chimera_core::ui::orbit::{ORBIT_CHRD, ORBIT_CUSTOM, ORBIT_ORB, ORBIT_RHYTHM};
use chimera_core::ui::theme;
use chimera_hal::ButtonId::{Edit, Plus, Seq};
use chimera_hal::EncoderId::{A, B, C, D, E};
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
    assert_eq!((steps.get(), mask.hits()), (1, 1));
    let n = steps.get() as usize;
    let now = step_at(screen::phase(0.3), steps) as usize;
    let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) as f32 / n as f32;
    for i in 0..n {
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

/// ORB › CUSTOM, the controller ruling (ADR 0079, amended 2026-10-03):
/// SCALE (C) turned to CUSTOM opens the circle on the scale it was; A
/// walks the cursor, B sets and clears; the root stays; EDIT on ORB opens
/// it whichever cell was turned last (named: ADR 0084).
#[test]
fn orb_custom_circle() {
    let mut ui = Box::new(crate::screen::stack_ui());
    let chords = |ui: &UiState| ui.project().orbit().live().chords;
    let page = |ui: &UiState| ui.page_def().id;
    hold(&mut ui, Seq);
    assert_eq!(page(&ui), ORBIT_ORB.id);
    assert_eq!(chords(&ui).scale, ScaleKind::Minor);

    // SCALE (C) from MINOR up to CUSTOM: the circle, holding MINOR.
    feed(&mut ui, Input::turn(C, 9));
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

    // EDIT: back to ORB, and again to CUSTOM.
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_ORB.id);
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);

    // KEY (B), then BPM (F), turned last: EDIT still goes down to CUSTOM.
    for e in [B, chimera_hal::EncoderId::F] {
        tap(&mut ui, Edit);
        assert_eq!(page(&ui), ORBIT_ORB.id);
        feed(&mut ui, Input::turn(e, 1));
        feed(&mut ui, Input::turn(e, -1));
        assert_eq!(chords(&ui).scale, ScaleKind::Custom);
        tap(&mut ui, Edit);
        assert_eq!(page(&ui), ORBIT_CUSTOM.id, "{e:?} turned last");
    }
    tap(&mut ui, Edit);

    // A set no named scale is stays the user's when SCALE comes back.
    feed(&mut ui, Input::turn(C, 1));
    assert_eq!(
        chords(&ui).scale,
        ScaleKind::Custom,
        "CUSTOM is SCALE's top"
    );
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CUSTOM.id, "SCALE again");
    feed(&mut ui, Input::turn(A, 1));
    feed(&mut ui, Input::turn(B, 1));
    let own = chords(&ui).custom;
    assert!(!Scale::ALL.contains(&own));
    tap(&mut ui, Edit);
    feed(&mut ui, Input::turn(C, -2));
    assert_eq!(chords(&ui).scale, ScaleKind::PentatonicMajor);
    feed(&mut ui, Input::turn(C, 2));
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
    assert_eq!(chords(&ui).custom, own);

    // The screen: `orbit_custom` in `screen_golden_test`.
    let ui = screen::ui_for("orbit_custom");
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
}

/// CHRD is the progression only: SEGS, DEGREE and PLAY on the top row,
/// A to C, so the focus starts on a real cell; D to F are reserved; KEY,
/// SCALE and BARS are ORB's alone, and EDIT there never opens CUSTOM.
#[test]
fn chrd_is_the_progression_only() {
    use chimera_core::orbit::runner::Capture;
    use chimera_core::ui::block_def::SlotBinding;
    use chimera_hal::EncoderId::F;
    let mut ui = Box::new(crate::screen::stack_ui());
    hold(&mut ui, Seq);
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::press(Plus));
    assert_eq!(ui.page_def().id, ORBIT_CHRD.id);
    assert_eq!(ui.focused_slot(), 0);
    assert!(
        !matches!(ORBIT_CHRD.params[0].binding, SlotBinding::Empty),
        "the focus starts on SEGS"
    );
    let before = ui.project().orbit().live().chords;
    for e in [D, E, F] {
        feed(&mut ui, Input::turn(e, 3));
        feed(&mut ui, Input::turn(e, -1));
    }
    assert_eq!(
        ui.project().orbit().live().chords,
        before,
        "D to F: nothing"
    );
    assert_eq!(ui.orbit().capture, Capture::Off);
    // SEGS on A, DEGREE on B, PLAY on C.
    feed(&mut ui, Input::turn(A, -1));
    assert_ne!(ui.project().orbit().live().chords.segs, before.segs);
    feed(&mut ui, Input::turn(B, 1));
    assert_ne!(ui.project().orbit().live().chords.degrees, before.degrees);
    feed(&mut ui, Input::turn(C, 1));
    assert!(matches!(ui.orbit().capture, Capture::Segment(_)));

    // SCALE CUSTOM, from ORB: CHRD's EDIT stays.
    ui.project_mut()
        .orbit_mut()
        .live_mut_for_test()
        .chords
        .scale = ScaleKind::Custom;
    tap(&mut ui, Edit);
    assert_eq!(ui.page_def().id, ORBIT_CHRD.id);
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
/// CUSTOM cursor and set, VOICE's bars, NOTES' chips and the chip picked. Each frame matches a full render.
#[test]
fn the_dirty_path_follows_orbit() {
    use chimera_core::ui::perf::PerfStats;
    use chimera_hal::ButtonId::{B1, B2, Mix};
    use chimera_hal::EncoderId::{E, F};
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
    // Ring 3 has one step: ring 2's eight show the clock move.
    tap(&mut ui, B2);
    frame(&mut ui, &mut fb, "ring 2");
    ui.set_orbit_view(&orbit_view(0.6));
    frame(&mut ui, &mut fb, "a later step");
    tap(&mut ui, B1);
    frame(&mut ui, &mut fb, "ring 1");

    let mut ui = Box::new(ui_for("orbit_chrd"));
    let mut fb = Fb::new();
    frame(&mut ui, &mut fb, "CHRD");
    feed(&mut ui, Input::turn(B, 1).and_held(Mix));
    frame(&mut ui, &mut fb, "segment 4");
    feed(&mut ui, Input::turn(B, -3).and_held(Mix));
    frame(&mut ui, &mut fb, "segment 1, the same degree");
    // Bar 4 of 4: the last segment.
    let mut v = orbit_view(0.8);
    (v.pos.bar, v.seg) = (3, 3);
    ui.set_orbit_view(&v);
    frame(&mut ui, &mut fb, "the last segment playing");
    feed(&mut ui, Input::turn(C, 1));
    frame(&mut ui, &mut fb, "PLAY armed");
    screen::played(&mut ui, 1, &[57, 60, 64]);
    frame(&mut ui, &mut fb, "a chord played in");
    // Segment 4 has segment 1's degree: only PLAY's text tells them apart.
    feed(&mut ui, Input::turn(B, 3).and_held(Mix));
    frame(&mut ui, &mut fb, "segment 4, the same degree, none played");
    screen::played(&mut ui, 2, &[57, 60, 64, 67]);
    feed(&mut ui, Input::turn(B, -3).and_held(Mix));
    frame(&mut ui, &mut fb, "segment 1 again");
    feed(&mut ui, Input::turn(B, 3).and_held(Mix));
    frame(
        &mut ui,
        &mut fb,
        "segment 4, the same degree, another chord",
    );

    let mut ui = Box::new(ui_for("orbit_voice"));
    let mut fb = Fb::new();
    frame(&mut ui, &mut fb, "VOICE");
    feed(&mut ui, Input::turn(B, 10));
    frame(&mut ui, &mut fb, "VEL");
    let mut v = orbit_view(0.6);
    v.rings[2].step = 4;
    ui.set_orbit_view(&v);
    frame(&mut ui, &mut fb, "a later step");

    let mut ui = Box::new(ui_for("orbit_notes_step"));
    let mut fb = Fb::new();
    frame(&mut ui, &mut fb, "NOTES, STEP");
    feed(&mut ui, Input::turn(F, 1).and_held(Mix));
    frame(&mut ui, &mut fb, "the next step picked");
    feed(&mut ui, Input::turn(F, 2));
    frame(&mut ui, &mut fb, "its note");
    feed(&mut ui, Input::turn(E, -1).and_held(Mix));
    frame(&mut ui, &mut fb, "CLEAR down");
    feed(&mut ui, Input::turn(chimera_hal::EncoderId::D, 1));
    frame(&mut ui, &mut fb, "LEARN armed");
    feed(&mut ui, Input::turn(A, 1));
    frame(&mut ui, &mut fb, "POOL");
    ui.set_orbit_view(&orbit_view(0.6));
    frame(&mut ui, &mut fb, "a later step");

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
    use chimera_theory::Degree;
    let mut ui = Box::new(crate::screen::stack_ui());
    hold(&mut ui, Seq);
    feed(&mut ui, Input::press(Plus));
    feed(&mut ui, Input::press(Plus));
    assert_eq!(ui.page_def().id, ORBIT_CHRD.id);
    let c = &mut ui.project_mut().orbit_mut().live_mut_for_test().chords;
    c.scale = ScaleKind::PentatonicMinor;
    c.degrees[0] = Degree::ALL[11];
    feed(&mut ui, Input::turn(B, -1));
    assert_eq!(ui.project().orbit().live().chords.degrees[0], Degree::IV);
    let c = &mut ui.project_mut().orbit_mut().live_mut_for_test().chords;
    c.degrees[0] = Degree::ALL[11];
    feed(&mut ui, Input::turn(B, 1));
    assert_eq!(ui.project().orbit().live().chords.degrees[0], Degree::V);
}

/// EDIT on CUSTOM returns to ORB even when SCALE is named under it (a
/// load or a scene switch changed it), so the page is never a dead end.
#[test]
fn custom_edit_returns_with_scale_named() {
    let mut ui = Box::new(crate::screen::stack_ui());
    hold(&mut ui, Seq);
    feed(&mut ui, Input::turn(C, 9));
    assert_eq!(ui.page_def().id, ORBIT_CUSTOM.id);
    ui.project_mut()
        .orbit_mut()
        .live_mut_for_test()
        .chords
        .scale = ScaleKind::Dorian;
    tap(&mut ui, Edit);
    assert_eq!(ui.page_def().id, ORBIT_ORB.id);
    tap(&mut ui, Edit);
    assert_eq!(
        ui.page_def().id,
        ORBIT_CUSTOM.id,
        "named: CUSTOM all the same"
    );
}

/// CUSTOM's cursor eases the short way: across 11 and 0 it moves one
/// note, not round the circle.
#[test]
fn the_cursor_eases_the_short_way() {
    use chimera_core::ui::animation::nearest;
    let nearest_turn = |from, to: u8| nearest(from, to as f32, 12.0);
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

/// ORB at phase 0.3, ring 1 just fired: every ring's steps round its
/// circle, a hit filled, an accent ringed in INK2, the step under the
/// clock lit, ring 5 dark (muted); the progression's first segment lit;
/// the sweep at 0.3 of the turn. One tick after ring 1 fires again its
/// step flashes, and is back by the third.
#[test]
fn orb_page_golden() {
    use chimera_core::orbit::RingIdx;
    use chimera_core::orbit::euclid::accents;
    use chimera_core::ui::animation::UiTick;
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
        let accented = accents(mask, ring.accent());
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
    assert!(scene.rings[4].ring.mute, "ring 5 is the muted one");

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

    // Ring 1 fires again: its step flashes, larger, and fades by the third tick.
    let mut ui = Box::new(ui);
    let r1 = RingIdx::ALL[0];
    let steps = scene.rings[0].ring.fill().steps();
    let (x, y) = step_xy(r1, step_at(phase, steps) as usize, steps.get() as usize);
    let at_rest = screen::render_ui(&ui);
    assert_eq!(at_rest.at(x + 3, y), theme::BG, "at rest, a small dot");
    let mut v = screen::orb_view(0.3);
    v.rings[0].fired = 2;
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
        // The header's rows: below them MIXER › PRT's meters (ADR 0293)
        // read the shared Part peaks, which other tests' audio raises.
        let head_px = |fb: &Fb| fb.px[..screen::W * theme::HEADER_BOTTOM as usize].to_vec();
        assert!(
            head_px(&still) == head_px(&third),
            "{name}: still between beats"
        );
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
    use chimera_hal::EncoderId::F;
    let mut ui = Box::new(crate::screen::stack_ui());
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

/// Under a MIDI clock F is inert: the internal BPM waits for the fallback.
/// Back on the internal clock it turns again.
#[test]
fn orb_bpm_is_inert_under_midi() {
    use chimera_core::orbit::runner::OrbitView;
    use chimera_hal::EncoderId::F;
    let mut ui = Box::new(crate::screen::stack_ui());
    hold(&mut ui, Seq);
    let bpm = |ui: &UiState| ui.project().orbit().bpm.get();
    ui.set_orbit_view(&screen::midi_view(0.0));
    feed(&mut ui, Input::turn(F, 5));
    assert_eq!(bpm(&ui), 120, "dimmed under MIDI");
    ui.set_orbit_view(&OrbitView::default());
    feed(&mut ui, Input::turn(F, 5));
    assert_eq!(bpm(&ui), 125);
}

/// `OrbitStatus::IDLE` is what `of` makes of the runner's first view:
/// the two derivations can't drift.
#[test]
fn idle_is_the_first_view() {
    use chimera_core::orbit::runner::OrbitView;
    use chimera_core::ui::orbit::OrbitStatus;
    assert_eq!(OrbitStatus::IDLE, OrbitStatus::of(&OrbitView::default()));
}

/// Ring `n`'s NOTES page (two EDITs from RHYTHM).
fn to_notes(ui: &mut UiState, n: usize) {
    screen::to_ring(ui, n);
    tap(ui, Edit);
    tap(ui, Edit);
    assert_eq!(ui.page_def().id, chimera_core::ui::orbit::ORBIT_NOTES.id);
}

/// What `act` sent the runner, folded as it applies it: read from the
/// log, and then acked as a view would, so the log never fills.
fn sent(ui: &mut UiState, act: impl FnOnce(&mut UiState)) -> chimera_core::orbit::channel::Batch {
    use chimera_core::orbit::channel::receive;
    use chimera_core::orbit::runner::OrbitView;
    let mut at = ui.orbit().log().head();
    act(ui);
    let b = receive(ui.orbit().log(), &mut at);
    ui.set_orbit_view(&OrbitView {
        acked: at,
        ..OrbitView::default()
    });
    b
}

/// The selected ring, as an index.
fn sel(ui: &UiState) -> chimera_core::orbit::RingIdx {
    ui.location().orbit().unwrap().ring()
}

/// The selected ring as the project holds it.
fn sel_ring(ui: &UiState) -> chimera_core::orbit::scene::SceneRing {
    let r = ui.location().orbit().unwrap().ring();
    ui.project().orbit().live().rings[r.get() as usize]
}

/// RING · VOICE and RING · NOTES: each cell's label, range and format,
/// and the goldens (`orbit_voice`, `orbit_notes`, `orbit_notes_step` in
/// `screen_golden_test`): gates as bars, an accent taller in INK2, the
/// pool as chips, the selected one outlined.
#[test]
fn voice_and_notes_pages() {
    use chimera_core::block::ValFmt;
    use chimera_core::orbit::scene::SceneRing;
    use chimera_core::ui::block_def::SlotBinding;
    use chimera_core::ui::orbit::draw::{OrbitFrame, cell_text};
    use chimera_core::ui::orbit::{ORBIT_NOTES, ORBIT_VOICE, OrbitCell};
    use chimera_hal::ButtonId::Mix;
    use chimera_hal::EncoderId::{D, F};

    let ring = |id| SlotBinding::SelectedRing(id);
    let voice = [
        (ring(SceneRing::GATE), "GATE", 0.0, 100.0),
        (ring(SceneRing::VEL), "VEL", 1.0, 127.0),
        (ring(SceneRing::ACCENT), "ACCENT", 0.0, 32.0),
        (ring(SceneRing::OCTAVE), "OCTAVE", 0.0, 8.0),
        (ring(SceneRing::RANGE), "RANGE", 0.0, 3.0),
        (ring(SceneRing::SNAP), "SNAP", 0.0, 1.0),
    ];
    for (slot, (binding, label, min, max)) in ORBIT_VOICE.params.iter().zip(voice) {
        assert_eq!(slot.binding, binding, "{label}");
        let s = slot.spec().unwrap();
        assert_eq!((slot.label(), s.min, s.max), (label, min, max));
        assert_eq!(slot.format(), s.fmt, "{label}");
    }
    let notes = [
        (ring(SceneRing::MODE), "MODE", 0.0, 1.0),
        (ring(SceneRing::WALK), "WALK", 0.0, 3.0),
        (ring(SceneRing::TARGET), "TARGET", 0.0, 5.0),
    ];
    for (slot, (binding, label, min, max)) in ORBIT_NOTES.params.iter().zip(notes) {
        assert_eq!(slot.binding, binding, "{label}");
        let s = slot.spec().unwrap();
        assert_eq!((slot.label(), s.min, s.max), (label, min, max));
    }
    let cells = [OrbitCell::Learn, OrbitCell::Clear, OrbitCell::Note];
    for (slot, cell) in ORBIT_NOTES.params[3..].iter().zip(cells) {
        assert_eq!(slot.binding, SlotBinding::Orbit(cell));
    }
    assert_eq!(
        cells.map(OrbitCell::label),
        ["LEARN", "CLEAR", "NOTE"],
        "the cells' labels"
    );
    assert_eq!(OrbitCell::Note.fmt(), ValFmt::Int(127));

    // The cells' text on ring 2's NOTES: its pool [69, 72, 76].
    let mut ui = Box::new(crate::screen::stack_ui());
    to_notes(&mut ui, 2);
    let text = |ui: &UiState, c: OrbitCell| {
        let f = OrbitFrame::new(ui.project().orbit(), ui.orbit());
        cell_text(c, &f, ui.location().orbit().unwrap().ring())
            .as_str()
            .to_string()
    };
    assert_eq!(text(&ui, OrbitCell::Note), "A4");
    assert_eq!(text(&ui, OrbitCell::Learn), "OFF");
    assert_eq!(text(&ui, OrbitCell::Clear), "C4 / RND");
    // MIX + NOTE picks the chip, clamped to the pool.
    feed(&mut ui, Input::turn(F, 1).and_held(Mix));
    assert_eq!(text(&ui, OrbitCell::Note), "C5");
    feed(&mut ui, Input::turn(F, 9).and_held(Mix));
    assert_eq!(text(&ui, OrbitCell::Note), "E5");
    // NOTE runs C-1 to G9, a semitone a detent; the pool re-sorts under
    // it and the chip follows its note.
    feed(&mut ui, Input::turn(F, 127));
    assert_eq!(text(&ui, OrbitCell::Note), "G9");
    feed(&mut ui, Input::turn(F, -127));
    feed(&mut ui, Input::turn(F, -127));
    assert_eq!(text(&ui, OrbitCell::Note), "C-1");
    let pool = sel_ring(&ui).ring.pool;
    assert_eq!(
        pool.notes().iter().map(|n| n.get()).collect::<Vec<_>>(),
        [0, 69, 72]
    );
    // LEARN arms a capture (orbit_capture_test) and edits nothing.
    let before = sel_ring(&ui);
    feed(&mut ui, Input::turn(D, 3));
    assert_eq!(sel_ring(&ui), before);
    assert_eq!(text(&ui, OrbitCell::Learn), "ON");
    feed(&mut ui, Input::turn(D, -1));
    assert_eq!(text(&ui, OrbitCell::Learn), "OFF");
    // OCTAVE 3 names CLEAR's C.
    let r = ui.location().orbit().unwrap().ring();
    ui.project_mut().orbit_mut().live_mut_for_test().rings[r.get() as usize]
        .ring
        .octave = chimera_core::orbit::ring::Octave::new(3).unwrap();
    assert_eq!(text(&ui, OrbitCell::Clear), "C3 / RND");

    // The goldens. VOICE: E(5,8), two accents, GATE 60, VEL 90, step 2
    // under the clock. A hit a bar, an accent taller (VEL + 30) in INK2;
    // a rest none.
    let ui = screen::ui_for("orbit_voice");
    assert_eq!(ui.page_def().id, ORBIT_VOICE.id);
    let fb = render("orbit_voice");
    let ring = sel_ring(&ui).ring;
    let mask = ring.hits_mask();
    assert_eq!((mask.steps().get(), mask.hits(), ring.accent()), (8, 5, 2));
    let accented = chimera_core::orbit::euclid::accents(mask, ring.accent());
    let base = theme::VIZ_BAND_BOTTOM - 6;
    let w = (theme::VIZ_RIGHT - theme::VIZ_LEFT) / 8;
    let (low, high) = (base - 2, base - 33);
    for i in (0..8).filter(|&i| i != 2) {
        let x = theme::VIZ_LEFT + w * i as i32 + 4;
        let want = match (mask.is_hit(i), accented.is_hit(i)) {
            (true, true) => (theme::INK2, theme::INK2),
            (true, false) => (theme::ACCENT, theme::BG),
            (false, _) => (theme::BG, theme::BG),
        };
        assert_eq!((fb.at(x, low), fb.at(x, high)), want, "step {i}");
    }
    assert!(accented.hits() == 2 && mask.is_hit(2) && !accented.is_hit(2));
    let x = theme::VIZ_LEFT + w * 2 + 4;
    assert_eq!(fb.at(x, low), theme::INK, "the clock's step");
    // GATE 60 %: a bar ends about 60 % into its step.
    let x0 = theme::VIZ_LEFT + 1;
    assert_eq!(fb.at(x0 + 14, low), theme::INK2);
    assert_eq!(fb.at(x0 + 16, low), theme::BG);

    // NOTES: eight chips, the sixth outlined in INK2.
    let ui = screen::ui_for("orbit_notes");
    assert_eq!(ui.page_def().id, ORBIT_NOTES.id);
    assert_eq!(ui.orbit().chip, 5);
    let fb = render("orbit_notes");
    // Laid at fractional x: chip i at a slot of (216 + 4) / 8.
    let slot = (theme::VIZ_RIGHT - theme::VIZ_LEFT + 4) as f32 / 8.0;
    let y = theme::VIZ_BAND_TOP + 18 + 17;
    for i in 0..8 {
        let x = theme::VIZ_LEFT + (slot * i as f32).round() as i32;
        let want = if i == 5 {
            theme::INK2
        } else {
            theme::ACCENT_SOFT
        };
        assert_eq!(fb.at(x, y), want, "chip {i}");
    }
    let ui = screen::ui_for("orbit_notes_step");
    assert_eq!(ui.page_def().id, ORBIT_NOTES.id);
    assert_eq!(
        sel_ring(&ui).ring.mode,
        chimera_core::orbit::ring::NoteMode::Step
    );
}

/// The NOTES line's figure: a pool of 3 on 5 hits comes round in 15, and
/// in 30 walked over one octave more (spec § 3.4).
#[test]
fn comes_round_in() {
    use chimera_core::orbit::euclid::{Fill, Steps};
    use chimera_core::orbit::ring::{Octave, Range, Ring};
    use chimera_core::orbit::walk::Pool;
    use chimera_core::ui::orbit::draw::notes_line;
    use chimera_theory::Note;
    let fill = Fill::new(Steps::new(8).unwrap(), 5, 0).unwrap();
    let mut r = Ring::new(fill, Octave::new(4).unwrap());
    let n = |x| Note::new(x).unwrap();
    r.pool = Pool::new(&[n(60), n(64), n(67)]).unwrap();
    assert_eq!(r.comes_round_in(), Some(15));
    r.range = Range::One;
    assert_eq!(r.comes_round_in(), Some(30));
    r.pool = Pool::one(n(60));
    r.range = Range::Zero;
    assert_eq!(r.comes_round_in(), Some(5));
    // DOWN as UP; PING-PONG plays its ends once: 2(L − 1), one note 1;
    // RANDOM never comes round.
    use chimera_core::orbit::walk::Walk;
    r.walk = Walk::Down;
    assert_eq!(r.comes_round_in(), Some(5));
    r.walk = Walk::PingPong;
    assert_eq!(r.comes_round_in(), Some(5), "one note");
    r.pool = Pool::new(&[n(60), n(64), n(67)]).unwrap();
    assert_eq!(r.comes_round_in(), Some(20), "lcm(4, 5)");
    r.walk = Walk::Random;
    assert_eq!(r.comes_round_in(), None);
    let (_, tail) = notes_line(&r);
    assert_eq!(tail.as_str(), "COMES ROUND IN -");
    r.walk = Walk::Up;
    // The factory ring 2: PING-PONG over 3 notes on 3 hits, 12; at
    // RANGE 1 (the contact sheet's frame 06) six notes, 30.
    let mut two = chimera_core::orbit::scene::Scene::init().rings[1].ring;
    assert_eq!(two.walk, Walk::PingPong);
    assert_eq!(two.comes_round_in(), Some(12));
    two.range = Range::One;
    assert_eq!(two.comes_round_in(), Some(30));
    // A STEP ring comes round with the ring: no figure.
    two.mode = chimera_core::orbit::ring::NoteMode::Step;
    let (head, tail) = notes_line(&two);
    assert_eq!((head.as_str(), tail.as_str()), ("STEPS 8 · HITS 3", ""));
    // The line NOTES draws.
    r.range = Range::One;
    r.pool = Pool::new(&[n(60), n(64), n(67)]).unwrap();
    let (head, tail) = notes_line(&r);
    assert_eq!(
        format!("{}{}", head.as_str(), tail.as_str()),
        "POOL 3 · HITS 5 · COMES ROUND IN 30"
    );
}

/// A STEP ring plays each step's own note, in step order, as NOTE set
/// them; OCTAVE moves them all. Through the UI, then a running ORBIT.
#[test]
fn step_ring_plays_its_steps() {
    use chimera_core::orbit::engine::OrbitKind;
    use chimera_core::orbit::ring::NoteMode;
    use chimera_hal::ButtonId::Mix;
    use chimera_hal::EncoderId::{D, F};
    let mut ui = Box::new(crate::screen::stack_ui());
    // Ring 2: E(3,8), hits on steps 0, 3 and 6.
    to_notes(&mut ui, 2);
    feed(&mut ui, Input::turn(A, -1));
    assert_eq!(sel_ring(&ui).ring.mode, NoteMode::Step);
    // Every step starts at OCTAVE's C: C4. Steps 0, 3, 6 to C, D, E.
    assert!(sel_ring(&ui).ring.notes.iter().all(|n| n.get() == 60));
    let b = sent(&mut ui, |ui| {
        feed(ui, Input::turn(F, 3).and_held(Mix));
        feed(ui, Input::turn(F, 2));
        feed(ui, Input::turn(F, 3).and_held(Mix));
        feed(ui, Input::turn(F, 4));
    });
    let notes = sel_ring(&ui).ring.notes;
    assert_eq!(
        [notes[0], notes[3], notes[6]].map(|n| n.get()),
        [60, 62, 64]
    );
    assert!(b.notes.contains(sel(&ui)), "the edit is sent");
    // VOICE: SNAP to SCALE, so A minor keeps C, D and E as they are.
    tap(&mut ui, Edit);
    tap(&mut ui, Edit);
    assert_eq!(ui.page_def().id, chimera_core::ui::orbit::ORBIT_VOICE.id);
    feed(&mut ui, Input::turn(F, -1));

    let played = |song: &chimera_core::orbit::scene::OrbitSong| {
        let mut rig = common::orbit::OrbitRig::on_init();
        *rig.song() = *song;
        rig.play(true);
        let mut out = Vec::new();
        while out.len() < 6 {
            rig.block();
            out.extend(
                rig.sent
                    .iter()
                    .filter(|e| e.ring.get() == 1 && matches!(e.kind, OrbitKind::On(_)))
                    .map(|e| e.note.get()),
            );
        }
        out
    };
    assert_eq!(played(ui.project().orbit()), [60, 62, 64, 60, 62, 64]);
    // OCTAVE up one: the same steps, an octave higher.
    feed(&mut ui, Input::turn(D, 1));
    assert_eq!(played(ui.project().orbit()), [72, 74, 76, 72, 74, 76]);
}

/// CLEAR down fills the pool (or every step) with notes of the key in the
/// ring's register, new dice each time; CLEAR up leaves one note at
/// OCTAVE's C. Each is sent as an edit of the ring's notes.
#[test]
fn clear_down_randomises_in_key() {
    use chimera_core::orbit::MAX_POOL;
    use chimera_core::orbit::ring::Range;
    use chimera_hal::ButtonId::Mix;
    use chimera_hal::EncoderId::E;
    let mut ui = Box::new(crate::screen::stack_ui());
    to_notes(&mut ui, 2);
    let key = ui.project().orbit().live().chords.key();
    let in_register = |ui: &UiState, n: u8| {
        let r = sel_ring(ui).ring;
        let lo = r.octave.low().get();
        let hi = lo + 12 * (r.range.octaves() + 1);
        (lo..hi).contains(&n) && key.contains(chimera_theory::Note::new(n).unwrap().pc())
    };
    let mut seen = Vec::new();
    for range in [Range::Zero, Range::Two] {
        let r = ui.location().orbit().unwrap().ring();
        ui.project_mut().orbit_mut().live_mut_for_test().rings[r.get() as usize]
            .ring
            .range = range;
        for _ in 0..3 {
            let b = sent(&mut ui, |ui| feed(ui, Input::turn(E, -1).and_held(Mix)));
            let s = sel_ring(&ui);
            assert!(b.notes.contains(sel(&ui)), "an EditNotes for the ring");
            assert_eq!(s.ring.pool.len(), MAX_POOL, "the pool filled");
            for n in s.ring.pool.notes() {
                assert!(in_register(&ui, n.get()), "{n} in A minor, {range:?}");
            }
            assert!(!seen.contains(&s.ring.pool), "new dice each turn");
            seen.push(s.ring.pool);
        }
    }
    // CLEAR up: one note at OCTAVE's C.
    let b = sent(&mut ui, |ui| feed(ui, Input::turn(E, 2).and_held(Mix)));
    let s = sel_ring(&ui);
    assert!(b.notes.contains(sel(&ui)));
    assert_eq!(s.ring.pool.notes(), [s.ring.octave.low()]);

    // STEP: every step.
    feed(&mut ui, Input::turn(A, -1));
    feed(&mut ui, Input::turn(E, -1).and_held(Mix));
    let s = sel_ring(&ui);
    assert!(s.ring.notes.iter().all(|n| in_register(&ui, n.get())));
    assert!(
        s.ring.notes.iter().any(|n| *n != s.ring.notes[0]),
        "random, not one note"
    );
    assert_eq!(s.ring.pool.len(), 1, "the pool untouched");
    feed(&mut ui, Input::turn(E, 1).and_held(Mix));
    let s = sel_ring(&ui);
    assert!(s.ring.notes.iter().all(|n| *n == s.ring.octave.low()));
}

/// CLEAR acts only with MIX held (the controller's ruling, ADR 0074): a
/// plain turn either way, as a slip reaching for NOTE, leaves the notes.
#[test]
fn clear_needs_mix() {
    use chimera_core::ui::orbit::draw::{OrbitFrame, focus};
    use chimera_core::ui::orbit::{ORBIT_NOTES, OrbitCell};
    use chimera_hal::ButtonId::Mix;
    use chimera_hal::EncoderId::E;
    let mut ui = Box::new(crate::screen::stack_ui());
    to_notes(&mut ui, 2);
    for mode in [0, -1] {
        feed(&mut ui, Input::turn(A, mode));
        let before = sel_ring(&ui);
        for d in [-1, 1, -5, 5] {
            feed(&mut ui, Input::turn(E, d));
            assert_eq!(sel_ring(&ui), before, "a plain turn of {d}");
        }
        let b = sent(&mut ui, |ui| feed(ui, Input::turn(E, -1).and_held(Mix)));
        assert!(b.notes.contains(sel(&ui)), "MIX + turn");
    }
    // Its focus names it over C4 / RND, its gesture under (ADR 0086).
    let r = ui.location().orbit().unwrap().ring();
    let f = OrbitFrame::new(ui.project().orbit(), ui.orbit());
    let ctx = chimera_core::ui::view::SlotCtx::read(
        &ui.project().part(ui.active_part).sound.params,
        chimera_core::addr::Op::A,
    )
    .on_ring(r);
    let fo = focus(&ORBIT_NOTES, 4, &ctx, &f, true, |_| 0.0).unwrap();
    assert_eq!((fo.label.as_str(), fo.text.as_str()), ("CLEAR", "C4 / RND"));
    assert_eq!(fo.hint, OrbitCell::Clear.hint(true));
}

/// ORB's B to E are KEY, SCALE, BARS and SCENE (ADR 0079): KEY, SCALE
/// and BARS live there alone; SCENE does what SCN's does, its queue and
/// cancel included.
#[test]
fn orb_owns_the_songs_frame() {
    let fresh = || {
        let mut ui = Box::new(crate::screen::stack_ui());
        hold(&mut ui, Seq);
        ui
    };
    let chords = |ui: &UiState| ui.project().orbit().live().chords;
    let mut ui = fresh();
    let c0 = chords(&ui);
    feed(&mut ui, Input::turn(B, 2));
    assert_ne!(chords(&ui).tonic, c0.tonic, "B: KEY");
    assert_eq!((chords(&ui).scale, chords(&ui).bars), (c0.scale, c0.bars));
    let c1 = chords(&ui);
    feed(&mut ui, Input::turn(C, 1));
    assert_ne!(chords(&ui).scale, c1.scale, "C: SCALE");
    assert_eq!((chords(&ui).tonic, chords(&ui).bars), (c1.tonic, c1.bars));
    let c2 = chords(&ui);
    feed(&mut ui, Input::turn(D, 1));
    assert_ne!(chords(&ui).bars, c2.bars, "D: BARS");
    assert_eq!((chords(&ui).tonic, chords(&ui).scale), (c2.tonic, c2.scale));
    assert_eq!(ui.page_def().id, ORBIT_ORB.id);

    // E: SCENE, turn for turn as on SCN.
    let state = |ui: &UiState| ui.project().orbit().queued();
    let (mut on_orb, mut on_scn) = (fresh(), fresh());
    for _ in 0..3 {
        feed(&mut on_scn, Input::press(Plus));
    }
    for t in [1, 2, -1, -9] {
        feed(&mut on_orb, Input::turn(E, t));
        feed(&mut on_scn, Input::turn(A, t));
        assert_eq!(state(&on_orb), state(&on_scn), "E by {t}");
    }
    let mut ui = fresh();
    feed(&mut ui, Input::turn(E, 1));
    assert!(state(&ui).is_some(), "SCENE queued");
    feed(&mut ui, Input::turn(E, -1));
    assert_eq!(state(&ui), None, "and cancelled");
}

/// Review Focus 2: CLEAR down turned once a frame while ORBIT runs rolls
/// new dice every turn, and the runner plays the last roll.
#[test]
fn clear_down_rolls_new_dice_through_the_runner() {
    use chimera_hal::ButtonId::Mix;
    let mut ui = Box::new(crate::screen::stack_ui());
    to_notes(&mut ui, 2);
    let mut rig = screen::ShellRig::new(ui);
    rig.b6();
    rig.until(|v| v.running);
    let mut pools = Vec::new();
    for _ in 0..10 {
        rig.frame(Input::turn(E, -1).and_held(Mix), 1);
        let p = sel_ring(&rig.ui).ring.pool;
        assert!(!pools.contains(&p), "new dice each turn");
        pools.push(p);
    }
    rig.idle(1);
    assert_eq!(
        rig.view().rings[1].pool,
        pools[9],
        "the runner plays the last"
    );
}

/// EDIT on ORB always opens CUSTOM (ADR 0084): with SCALE named the
/// circle shows its notes and SCALE stays named until a note is toggled;
/// the first toggle makes it CUSTOM from those notes, the toggle applied.
#[test]
fn edit_opens_custom_on_a_named_scale() {
    let mut ui = Box::new(crate::screen::stack_ui());
    let chords = |ui: &UiState| ui.project().orbit().live().chords;
    let page = |ui: &UiState| ui.page_def().id;
    hold(&mut ui, Seq);
    assert_eq!(chords(&ui).scale, ScaleKind::Minor);

    // Down to CUSTOM and back, nothing toggled: SCALE stays MINOR.
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
    assert_eq!(chords(&ui).set(), Scale::MINOR, "the circle shows MINOR");
    feed(&mut ui, Input::turn(A, 1));
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_ORB.id);
    assert_eq!(chords(&ui).scale, ScaleKind::Minor);
    // SEQ is up: back to ORB too.
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
    tap(&mut ui, Seq);
    assert_eq!(page(&ui), ORBIT_ORB.id);
    assert_eq!(chords(&ui).scale, ScaleKind::Minor);

    // A note already out, turned left: no toggle, still MINOR.
    tap(&mut ui, Edit);
    assert_eq!(ui.orbit().cursor, 1);
    feed(&mut ui, Input::turn(B, -1));
    assert_eq!(chords(&ui).scale, ScaleKind::Minor);

    // The first toggle: CUSTOM, MINOR's other eleven kept, the minor
    // second in; on `live` and its slot alike.
    feed(&mut ui, Input::turn(B, 1));
    let want = Scale::MINOR.toggle(Interval::ALL[1]);
    assert_eq!(chords(&ui).scale, ScaleKind::Custom);
    assert_eq!(chords(&ui).custom, want);
    let slot = ui.project().orbit().current();
    assert_eq!(ui.project().orbit().scene(slot).chords.custom, want);
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
    tap(&mut ui, Edit);
    assert_eq!(page(&ui), ORBIT_ORB.id);

    // The screen: `orbit_custom_named` in `screen_golden_test`.
    let ui = screen::ui_for("orbit_custom_named");
    assert_eq!(page(&ui), ORBIT_CUSTOM.id);
    assert_eq!(chords(&ui).scale, ScaleKind::Minor);
}

/// ORB's RING cell names the ring's Part (ADR 0084): NEW puts rings 3
/// and 4 on Part 3.
#[test]
fn ring_cell_names_its_part() {
    use chimera_core::orbit::RingIdx;
    use chimera_core::ui::orbit::OrbitCell;
    use chimera_core::ui::orbit::draw::{OrbitFrame, cell_text};
    let mut ui = Box::new(crate::screen::stack_ui());
    hold(&mut ui, Seq);
    let f = OrbitFrame::new(ui.project().orbit(), ui.orbit());
    let text = |r: usize| {
        cell_text(OrbitCell::Ring, &f, RingIdx::ALL[r])
            .as_str()
            .to_owned()
    };
    assert_eq!(text(0), "1 › P1");
    assert_eq!(text(2), "3 › P3");
    assert_eq!(text(3), "4 › P3");
}

/// A TARGET change redraws ORB: its circle's labels and the RING cell.
#[test]
fn a_target_change_redraws_orb() {
    use chimera_core::orbit::scene::Target;
    use chimera_core::project::PartId;
    use chimera_core::ui::region::RegionKind;
    let mut ui = Box::new(screen::ui_for("orbit_orb"));
    let mut fb = screen::Fb::new();
    let perf = chimera_core::ui::perf::PerfStats::zero();
    ui.render_dirty_with_scope(&mut fb, &perf, &screen::scope_fixture());
    let keys = |ui: &UiState| {
        (
            ui.drawn_key(RegionKind::Viz),
            ui.drawn_key(RegionKind::Cells),
        )
    };
    let before = keys(&ui);
    let ring = ui.location().orbit().unwrap().ring().get() as usize;
    ui.project_mut().orbit_mut().live_mut_for_test().rings[ring].target =
        Target::Part(PartId::ALL[5]);
    screen::settle(&mut ui);
    ui.render_dirty_with_scope(&mut fb, &perf, &screen::scope_fixture());
    let after = keys(&ui);
    assert_ne!(after.0, before.0, "the circle redraws");
    assert_ne!(after.1, before.1, "the RING cell redraws");
}
