// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The oscilloscope screensaver: idle timer and wake, note naming, chord
//! naming, pitch measurement and the screens.

mod screen;

use chimera_core::scope::SCOPE_LEN;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::saver::{self, Idle, Readout, Wake};
use screen::Fb;

fn s(f: impl FnOnce(&mut FmtBuf)) -> String {
    let mut b = FmtBuf::new();
    f(&mut b);
    b.as_str().to_string()
}

// --- Idle ---

#[test]
fn sleeps_after_the_timeout_and_off_never_does() {
    let mut i = Idle::new();
    i.tick(6_999, Some(7_000));
    assert!(!i.asleep());
    i.tick(1, Some(7_000));
    assert!(i.asleep());
    let mut off = Idle::new();
    off.tick(3_600_000, None);
    assert!(!off.asleep());
}

#[test]
fn input_resets_the_clock() {
    let mut i = Idle::new();
    i.tick(6_000, Some(7_000));
    i.touch(true, false);
    i.tick(6_000, Some(7_000));
    assert!(!i.asleep());
}

#[test]
fn any_touch_wakes_and_restarts_the_clock() {
    for (turned, down) in [(true, false), (false, true)] {
        let mut i = Idle::new();
        i.tick(7_000, Some(7_000));
        assert!(i.asleep());
        let w = i.touch(turned, down);
        assert_eq!(w == Wake::FirstDetent, turned, "a turn loses a detent");
        assert!(!i.asleep());
        assert_eq!(i.touch(turned, down), Wake::Pass, "awake: it all acts");
        i.tick(6_999, Some(7_000));
        assert!(!i.asleep());
    }
}

#[test]
fn a_quiet_frame_asleep_stays_asleep() {
    let mut i = Idle::new();
    i.tick(9_000, Some(7_000));
    i.touch(false, false);
    assert!(i.asleep());
}

// --- Naming ---

#[test]
fn nearest_note_and_cents() {
    assert_eq!(saver::nearest(6000), (60, 0));
    assert_eq!(saver::nearest(6012), (60, 12));
    assert_eq!(saver::nearest(6049), (60, 49));
    // Exactly between: the upper note, −50.
    assert_eq!(saver::nearest(6050), (61, -50));
    assert_eq!(saver::nearest(5950), (60, -50));
    assert_eq!(saver::nearest(5997), (60, -3));
    assert_eq!(saver::nearest(5949), (59, 49));
}

#[test]
fn note_names_cross_octaves_and_the_midi_range() {
    let n = |n| s(|b| saver::note_name(b, n));
    assert_eq!(n(60), "C4");
    assert_eq!(n(61), "C#4");
    assert_eq!(n(59), "B3");
    assert_eq!(n(0), "C-1");
    assert_eq!(n(127), "G9");
    // Out of range still names.
    assert_eq!(n(128), "G#9");
    assert_eq!(n(-1), "B-2");
    assert_eq!(n(-12), "C-2");
    assert_eq!(n(151), "G11");
}

#[test]
fn out_and_in_lines() {
    use chimera_core::played::{NoteIn, NoteOut};
    let out = |note, cents| s(|b| saver::out_line(b, NoteOut { note, cents }));
    assert_eq!(out(61, 12), "OUT C#4 +12ct");
    assert_eq!(out(60, -12), "OUT C4 -12ct");
    assert_eq!(out(60, 0), "OUT C4 +0ct");
    // 60 + 24 semitones − 5 cents: octave boundary.
    assert_eq!(out(71, 95), "OUT C5 -5ct");
    assert_eq!(out(60, -50), "OUT C4 -50ct");
    assert_eq!(out(60, 50), "OUT C#4 -50ct");
    let i = NoteIn {
        note: 60,
        vel: 96,
        part: chimera_core::project::PartId::ALL[0],
    };
    assert_eq!(s(|b| saver::in_line(b, i, 1)), "IN  C4  v96  P1");
    assert_eq!(s(|b| saver::in_line(b, i, 3)), "IN  C4  v96  P1  +2");
}

#[test]
fn heard_is_cents_off_the_out_note_or_a_note_of_its_own() {
    let hz = |cents: f32| 440.0 * (cents / 1200.0).exp2();
    assert_eq!(
        s(|b| saver::heard_line(b, hz(15.0), Some(69))),
        "(heard +15ct)"
    );
    assert_eq!(
        s(|b| saver::heard_line(b, hz(-8.0), Some(69))),
        "(heard -8ct)"
    );
    assert_eq!(
        s(|b| saver::heard_line(b, hz(0.0), None)),
        "(heard A4 +0ct)"
    );
    // Two semitones off the OUT note: named.
    assert_eq!(
        s(|b| saver::heard_line(b, hz(200.0), Some(69))),
        "(heard B4 +0ct)"
    );
}

// --- Chords: lowest note tried as the root first, then the other tones
// rising; slash chords name the lowest note; "?" when nothing fits ---

fn chord(keys: &[u8]) -> String {
    s(|b| saver::chord_name(b, keys))
}

#[test]
fn nothing_one_and_two_notes() {
    assert_eq!(chord(&[]), "");
    assert_eq!(chord(&[60]), "C4");
    assert_eq!(chord(&[60, 67]), "P5");
    assert_eq!(chord(&[60, 64]), "M3");
    assert_eq!(chord(&[60, 63]), "m3");
    assert_eq!(chord(&[60, 72]), "P8");
    assert_eq!(chord(&[60, 66]), "TT");
    // Compound intervals fold: a ninth is a second.
    assert_eq!(chord(&[60, 74]), "M2");
}

#[test]
fn triads_and_sevenths() {
    assert_eq!(chord(&[60, 64, 67]), "C");
    assert_eq!(chord(&[60, 63, 67]), "Cm");
    assert_eq!(chord(&[60, 63, 66]), "Cdim");
    assert_eq!(chord(&[60, 64, 68]), "Caug");
    assert_eq!(chord(&[60, 64, 67, 70]), "C7");
    assert_eq!(chord(&[60, 64, 67, 71]), "Cmaj7");
    assert_eq!(chord(&[60, 63, 67, 70]), "Cm7");
    assert_eq!(chord(&[60, 63, 66, 70]), "Cm7b5");
    assert_eq!(chord(&[60, 63, 66, 69]), "Cdim7");
    assert_eq!(chord(&[61, 65, 68]), "C#");
    // The user's example: C E G A# is a C7, written across octaves.
    assert_eq!(chord(&[48, 64, 79, 82]), "C7");
}

#[test]
fn sus_chords() {
    assert_eq!(chord(&[60, 62, 67]), "Csus2");
    assert_eq!(chord(&[60, 65, 67]), "Csus4");
    assert_eq!(chord(&[60, 65, 67, 70]), "C7sus4");
}

#[test]
fn inversions_are_slash_chords() {
    assert_eq!(chord(&[64, 67, 72]), "C/E");
    assert_eq!(chord(&[67, 72, 76]), "C/G");
    assert_eq!(chord(&[64, 67, 70, 72]), "C7/E");
    assert_eq!(chord(&[67, 70, 72, 76]), "C7/G");
    assert_eq!(chord(&[70, 72, 76, 79]), "C7/A#");
    assert_eq!(chord(&[64, 69, 72]), "Am/E");
}

#[test]
fn ambiguous_sets_prefer_the_lowest_note_as_root() {
    // C E G A is C6 with C lowest, Am7/C otherwise named: root position wins.
    assert_eq!(chord(&[60, 64, 67, 69]), "C6");
    // The same pitch classes with A lowest read as Am7.
    assert_eq!(chord(&[57, 60, 64, 67]), "Am7");
    // The augmented triad is symmetric: the lowest note names it.
    assert_eq!(chord(&[64, 68, 72]), "Eaug");
    assert_eq!(chord(&[68, 72, 76]), "G#aug");
    // Octave doublings change nothing.
    assert_eq!(chord(&[48, 60, 64, 67, 76]), "C");
}

#[test]
fn unrecognised_sets_are_a_question_mark() {
    assert_eq!(chord(&[60, 61, 62]), "?");
    assert_eq!(chord(&[60, 61, 62, 63, 64, 65, 66]), "?");
}

// --- Measuring ---

const FS: f32 = 48_000.0;

fn wave(freq: f32, f: impl Fn(f32) -> f32) -> [f32; SCOPE_LEN] {
    core::array::from_fn(|i| f(freq * i as f32 / FS))
}

fn sine(p: f32) -> f32 {
    (p * core::f32::consts::TAU).sin() * 0.5
}

fn saw(p: f32) -> f32 {
    (p.fract() * 2.0 - 1.0) * 0.5
}

fn cents_off(measured: f32, want: f32) -> f32 {
    1200.0 * (measured / want).log2()
}

#[test]
fn measures_sines_and_saws_within_three_cents() {
    for hz in [300.0, 329.63, 440.0, 523.25, 880.0, 1000.0, 1760.0, 2500.0] {
        for (name, f) in [("sine", sine as fn(f32) -> f32), ("saw", saw)] {
            let m = saver::measure(&wave(hz, f)).unwrap_or_else(|| panic!("{name} {hz}: none"));
            let off = cents_off(m, hz);
            assert!(
                off.abs() < 3.0,
                "{name} {hz} Hz measured {m} ({off:+.2} ct)"
            );
        }
    }
}

#[test]
fn a_detuned_pair_measures_between_the_two_inside_the_pair() {
    let hz = 440.0;
    for detune in [3.0f32, 8.0, 15.0] {
        let up = hz * (detune / 1200.0).exp2();
        let buf: [f32; SCOPE_LEN] =
            core::array::from_fn(|i| 0.5 * saw(hz * i as f32 / FS) + 0.5 * saw(up * i as f32 / FS));
        let m = saver::measure(&buf).expect("a pair");
        let off = cents_off(m, hz);
        assert!(
            (-2.0..detune + 2.0).contains(&off),
            "pair +{detune} ct measured {off:+.2}"
        );
    }
}

/// A strong 5th harmonic adds rising crossings just short of each
/// period: measure used to re-find one forever and freeze the UI loop.
/// Bounded by a timeout, so a hang fails instead of hanging the suite.
#[test]
fn rising_crossings_short_of_the_period_never_hang() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for k in 0..120 {
            let hz = 300.0 + k as f32 * 23.0;
            let _ = saver::measure(&wave(hz, |p| sine(p) + 1.2 * sine(5.0 * p)));
        }
        let _ = tx.send(());
    });
    assert!(
        rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(),
        "measure hung"
    );
}

#[test]
fn silence_dc_and_noise_measure_nothing() {
    assert_eq!(saver::measure(&[0.0; SCOPE_LEN]), None);
    assert_eq!(saver::measure(&[0.3; SCOPE_LEN]), None);
    assert_eq!(saver::measure(&wave(440.0, |p| sine(p) * 0.001)), None);
    // Below the window's reach: 100 Hz is half a period of scope.
    assert_eq!(saver::measure(&wave(100.0, sine)), None);
    let mut x = 0x1234_5678u32;
    for _ in 0..20 {
        let noise: [f32; SCOPE_LEN] = core::array::from_fn(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x as f32 / u32::MAX as f32 - 0.5) * 0.8
        });
        assert_eq!(saver::measure(&noise), None);
    }
}

// --- Screens ---

fn readout(keys: &[u8], heard: Option<f32>) -> Readout {
    use chimera_core::played::{NoteIn, NoteOut};
    let mut r = Readout::NONE;
    r.keys[..keys.len()].copy_from_slice(keys);
    r.held = keys.len();
    if let Some(&k) = keys.last() {
        r.input = Some(NoteIn {
            note: k,
            vel: 96,
            part: chimera_core::project::PartId::ALL[0],
        });
        r.out = Some(NoteOut { note: k, cents: 12 });
        r.heard = heard;
    }
    r
}

fn shot(name: &str, scope: &[f32; SCOPE_LEN], r: &Readout) -> u64 {
    let mut fb = Fb::new();
    saver::draw(&mut fb, scope, r);
    fb.dump(name);
    assert_eq!(fb.oob, 0, "{name} drew off screen");
    fb.hash()
}

#[test]
fn screen_goldens() {
    let silent = [0.0; SCOPE_LEN];
    let c4 = wave(261.63, saw);
    let a4 = wave(440.0, sine);
    let cases = [
        (
            "saver_idle",
            shot("saver_idle", &silent, &Readout::NONE),
            0xeff3a71ffa7f9f25u64,
        ),
        (
            "saver_note",
            shot("saver_note", &a4, &readout(&[69], Some(443.0))),
            0xe4e6e4d14ebf4ce3,
        ),
        (
            "saver_chord",
            shot("saver_chord", &c4, &readout(&[60, 64, 67, 70], None)),
            0x41746045c17f4cec,
        ),
        (
            "saver_two",
            shot("saver_two", &a4, &readout(&[60, 67], Some(440.0))),
            0x6dff4a2b1ea154da,
        ),
    ];
    for (n, got, want) in cases {
        assert_eq!(got, want, "{n}: {got:#018x}");
    }
}

// --- In the UI ---

use chimera_core::ui::UiState;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::theme_settings::{Saver, ThemeSettings};
use chimera_hal::ButtonId;
use screen::Input;

fn idle(ui: &mut UiState, ms: u32) {
    ui.step_idle(ms, false);
}

/// Asleep at the default 5 min, not a ms before. The waking press wakes
/// the screen and acts (ADR 0147).
#[test]
fn the_ui_sleeps_and_the_waking_touch_acts() {
    let mut ui = UiState::new();
    assert_eq!(ui.theme().saver, Saver::DEFAULT);
    idle(&mut ui, DEFAULT_MS - 1);
    let mut fb = Fb::new();
    let scope = [0.0; SCOPE_LEN];
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope);
    let awake = fb.hash();
    idle(&mut ui, 1);
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope);
    assert_ne!(fb.hash(), awake, "asleep");
    let loc = ui.location();
    screen::feed(&mut ui, Input::press(ButtonId::B2).at(0));
    screen::feed(&mut ui, Input::release(ButtonId::B2).at(50));
    assert_ne!(ui.location(), loc, "the waking press navigated");
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &scope);
    assert_ne!(fb.hash(), awake, "awake, on the new page");
}

const DEFAULT_MS: u32 = 300_000;

#[test]
fn off_never_sleeps() {
    let mut ui = UiState::new();
    ui.set_theme(ThemeSettings {
        saver: Saver::OFF,
        ..ThemeSettings::DEFAULT
    });
    idle(&mut ui, 3_600_000);
    let mut fb = Fb::new();
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &[0.0; SCOPE_LEN]);
    let mut saver_fb = Fb::new();
    saver::draw(&mut saver_fb, &[0.0; SCOPE_LEN], &Readout::NONE);
    assert_ne!(fb.hash(), saver_fb.hash(), "a page, not the saver");
}

#[test]
fn card_work_is_not_idle_time() {
    let mut ui = UiState::new();
    ui.step_idle(DEFAULT_MS - 1_000, false);
    // A blocking save or load: its time doesn't count, and the clock restarts.
    ui.step_idle(5_000, true);
    ui.step_idle(1_000, false);
    let mut fb = Fb::new();
    let mut saver_fb = Fb::new();
    saver::draw(&mut saver_fb, &[0.0; SCOPE_LEN], &Readout::NONE);
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &[0.0; SCOPE_LEN]);
    assert_ne!(fb.hash(), saver_fb.hash(), "awake");
    ui.step_idle(DEFAULT_MS, false);
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &[0.0; SCOPE_LEN]);
    assert_eq!(fb.hash(), saver_fb.hash(), "asleep after 5 min of nothing");
    // Busy asleep stays asleep (no wake without a touch).
    ui.step_idle(100, true);
    ui.render_with_scope(&mut fb, &PerfStats::zero(), &[0.0; SCOPE_LEN]);
    assert_eq!(fb.hash(), saver_fb.hash());
}

// --- Heard is shown only when trustworthy ---

fn out(note: u8, cents: i16) -> Option<chimera_core::played::NoteOut> {
    Some(chimera_core::played::NoteOut { note, cents })
}

fn hz(midi_cents: f32) -> f32 {
    440.0 * ((midi_cents - 6900.0) / 1200.0).exp2()
}

#[test]
fn heard_needs_a_note_in_reach_and_agreement() {
    use saver::trusted;
    // E4, one note, heard right: shown. An octave up, a chord, nothing
    // played, a bass note below the window: not.
    assert!(trusted(hz(6400.0), out(64, 0), 1));
    assert!(trusted(hz(6440.0), out(64, 0), 1));
    assert!(!trusted(hz(7600.0), out(64, 0), 1), "an octave up");
    assert!(!trusted(hz(5200.0), out(64, 0), 1), "an octave down");
    assert!(!trusted(hz(6400.0), out(64, 0), 3), "a chord");
    assert!(!trusted(hz(6400.0), None, 0));
    assert!(
        !trusted(hz(5500.0), out(55, 0), 1),
        "G3 is below the window"
    );
    assert!(!trusted(hz(6000.0), out(60, 0), 1), "C4 too");
}

/// A saw with a strong 2nd harmonic, and a 200 Hz tone: never a wrong
/// octave, whatever the measure says.
#[test]
fn a_strong_second_harmonic_never_shows_the_octave() {
    let two = |f: f32| -> [f32; SCOPE_LEN] {
        core::array::from_fn(|i| {
            let p = f * i as f32 / 48_000.0 * core::f32::consts::TAU;
            0.15 * p.sin() + 0.5 * (2.0 * p).sin()
        })
    };
    // A3 (220 Hz): the window holds one period, so the measure may read 440.
    let m = saver::measure(&two(220.0));
    if let Some(h) = m {
        assert!(!saver::trusted(h, out(57, 0), 1), "{h}");
    }
    // E4 (329.6 Hz): either right or hidden.
    if let Some(h) = saver::measure(&two(329.63)) {
        let shown = saver::trusted(h, out(64, 0), 1);
        let off = 1200.0 * (h / 329.63).log2();
        assert!(!shown || off.abs() < 100.0, "{h} shown {off:+.0} ct off");
    }
    // 200 Hz: nothing, or hidden.
    let sine200: [f32; SCOPE_LEN] =
        core::array::from_fn(|i| (200.0 * i as f32 / 48_000.0 * core::f32::consts::TAU).sin());
    if let Some(h) = saver::measure(&sine200) {
        assert!(!saver::trusted(h, out(55, 0), 1), "{h}");
    }
}
