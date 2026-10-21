//! PIT's tuner (ADR 0201): the scope's corner reads the pitch the edited
//! Part's last note sounds at, worked out from the note and the Sound:
//! note and cents at A4 = 440 Hz, then Hz to a tenth. TAPE's source plays
//! at the key when its root is known and reads `ROOT ?` when it isn't;
//! before any note, `REF C4`; between notes, dim. It follows PITCH and
//! FINE the frame they turn, and nothing redraws at idle.

#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::MidiNote;
use chimera_core::dsp::tape_engine::params::{KeyMode, StretchType, Way};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::storage::{SourceRef, TapeSource};
use chimera_core::ui::UiState;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::pit_tuner::Shown;
use chimera_core::ui::pit_tuner::calc::{
    CORNER_W, Known, Light, Reading, Readout, Shift, Tone, lines, pitch, shown,
};
use chimera_core::ui::theme::{self, TextSize};
use chimera_hal::EncoderId;
use screen::*;

const P1: PartId = PartId::ALL[0];

fn read(r: Readout) -> (String, String) {
    let (a, b) = lines(r);
    (a.as_str().into(), b.as_str().into())
}

/// A lit note at `c` cents above MIDI 0.
fn lit(c: f32) -> Readout {
    Readout::Lit(Known::Note(Tone::of_cents(c)))
}

/// A known reading at `c`.
fn at(c: f32) -> Option<Reading> {
    Some(Reading::Known(Known::Note(Tone::of_cents(c))))
}

fn key(n: u8) -> MidiNote {
    MidiNote::new(n).unwrap()
}

fn shown_lit(r: Option<Reading>) -> (String, String) {
    read(Readout::of(r, Light::Lit))
}

fn texts(s: &Shown) -> [String; 2] {
    core::array::from_fn(|i| {
        s.lines[i]
            .as_ref()
            .map_or(String::new(), |l| l.text.as_str().into())
    })
}

fn tape(way: Way, root: f32, period: f32) -> Sound {
    let mut s = Sound::init(EngineType::Tape);
    s.source = Some(TapeSource {
        r: SourceRef {
            crc: 0x5EED,
            frames: 48_000,
            rate: 48_000,
        },
        tune: None,
    });
    s.params.tape.set_way(way);
    s.params.tape.root = root;
    s.params.tape.period = period;
    s
}

// ── Naming ───────────────────────────────────────────────────────────

#[test]
fn a4_reads_440_hz_and_c4_its_own() {
    assert_eq!(read(lit(6_900.0)), ("A4 +0ct".into(), "440.0 Hz".into()));
    assert_eq!(read(lit(6_000.0)), ("C4 +0ct".into(), "261.6 Hz".into()));
    assert_eq!(read(lit(6_901.0)), ("A4 +1ct".into(), "440.3 Hz".into()));
    assert_eq!(read(lit(8_100.0)), ("A5 +0ct".into(), "880.0 Hz".into()));
}

/// Exactly between two notes is the upper one at −50; a hair under it
/// the lower one at +49, both ways from A4.
#[test]
fn cents_turn_over_at_50() {
    let note = |c| read(lit(c)).0;
    assert_eq!(note(6_949.0), "A4 +49ct");
    assert_eq!(note(6_949.4), "A4 +49ct");
    assert_eq!(note(6_949.6), "A#4 -50ct");
    assert_eq!(note(6_950.0), "A#4 -50ct");
    assert_eq!(note(6_951.0), "A#4 -49ct");
    assert_eq!(note(6_850.0), "A4 -50ct");
    assert_eq!(note(6_849.0), "G#4 +49ct");
    assert_eq!(note(-50.0), "C-1 -50ct");
}

/// ALGO's TRNSP, VA's OSC 1 CRSE and FINE, PITCH, FINE and TUNING all
/// sum, as the voice takes them.
#[test]
fn every_engines_offsets_sum() {
    let mut a = Sound::init(EngineType::Algo);
    a.params.pitch.pitch = 7.0;
    a.params.pitch.fine = -25.0;
    a.params.algo.transpose = -12;
    assert_eq!(
        pitch(key(60), &a, 10),
        at(6_000.0 + 700.0 - 25.0 - 1_200.0 + 10.0)
    );
    let mut v = Sound::init(EngineType::Va);
    v.params.va.coarse = [12, -7];
    v.params.va.fine = [3.0, 6.0];
    assert_eq!(pitch(key(69), &v, 0), at(6_900.0 + 1_200.0 + 3.0));
    let m = Sound::init(EngineType::Modal);
    assert_eq!(pitch(key(69), &m, -3), at(6_897.0));
}

// ── TAPE ─────────────────────────────────────────────────────────────

/// A source with a root plays at the key: its root key plays it at its
/// own speed, so PITCH's transposition is what moves it, whatever the
/// root is.
#[test]
fn tape_plays_the_key_over_its_root() {
    for m in [Way::One, Way::Loop, Way::Ping, Way::Tron, Way::Stretch] {
        let mut s = tape(m, 4_800.0, 183.4);
        s.params.pitch.pitch = 7.0;
        assert_eq!(pitch(key(48), &s, 0), at(5_500.0), "{m:?}");
        assert_eq!(shown_lit(pitch(key(48), &s, 0)).0, "G3 +0ct");
        s.params.tape.root = 4_930.0;
        assert_eq!(pitch(key(48), &s, 0), at(5_500.0), "{m:?} any root");
    }
    // TRANSWAVE's cycle plays the note itself.
    let w = tape(Way::Wave, 4_800.0, 0.0);
    assert_eq!(pitch(key(69), &w, 0), at(6_900.0));
}

/// No period: the import found no pitch, so the root is a guess. The
/// corner says so and gives the shift off the source's own speed.
#[test]
fn an_unknown_root_reads_root_unknown_and_the_shift() {
    let mut s = tape(Way::Loop, 6_000.0, 0.0);
    s.params.pitch.pitch = 7.0;
    s.params.pitch.fine = 12.0;
    let p = pitch(key(60), &s, 0);
    assert_eq!(p, Some(Reading::RootUnknown(Shift::of_cents(712.0))));
    assert_eq!(shown_lit(p), ("ROOT ?".into(), "+7st +12ct".into()));
    assert_eq!(shown_lit(pitch(key(48), &s, 0)).1, "-5st +12ct");
}

/// SCRUB runs at 2 · |MOTION| of the source's speed, and stands at 0.
#[test]
fn scrub_follows_motion() {
    let mut s = tape(Way::Scrub, 6_000.0, 100.0);
    s.params.tape.motion = 0.0;
    assert_eq!(pitch(key(60), &s, 0), Some(Reading::Known(Known::Still)));
    assert_eq!(
        read(Readout::Lit(Known::Still)),
        ("STILL".into(), String::new())
    );
    s.params.tape.motion = -0.5;
    assert_eq!(pitch(key(60), &s, 0), at(6_000.0));
    s.params.tape.motion = 1.0;
    assert_eq!(pitch(key(60), &s, 0), at(7_200.0));
}

/// SCRUB's KEY POSITION places the finger and plays every key at ROOT
/// (ADR 0205); KEY PITCH plays the key.
#[test]
fn scrub_key_position_reads_root() {
    let mut s = tape(Way::Scrub, 6_000.0, 100.0);
    s.params.tape.motion = 0.5;
    assert_eq!(s.params.tape.key, KeyMode::Position);
    assert_eq!(pitch(key(72), &s, 0), at(6_000.0));
    assert_eq!(pitch(key(48), &s, 0), at(6_000.0));
    s.params.tape.key = KeyMode::Pitch;
    assert_eq!(pitch(key(72), &s, 0), at(7_200.0));
}

/// Every STRETCH TYPE plays the key's pitch (ADR 0204).
#[test]
fn every_stretch_type_plays_the_key() {
    for k in [StretchType::Mono, StretchType::Poly, StretchType::Grain] {
        let mut s = tape(Way::Stretch, 4_800.0, 183.4);
        s.params.tape.kind = k;
        assert_eq!(pitch(key(55), &s, 0), at(5_500.0), "{k:?}");
    }
}

#[test]
fn tape_with_no_source_reads_nothing() {
    let mut s = tape(Way::Loop, 6_000.0, 100.0);
    s.source = None;
    assert_eq!(pitch(key(60), &s, 0), None);
}

// ── The corner ───────────────────────────────────────────────────────

/// The widest lines fit the corner at every TEXT SIZE, right on the
/// band's edge, above the scope's middle.
#[test]
fn the_corner_fits_at_every_text_size() {
    let worst = [
        Readout::Reference(Known::Note(Tone::of_cents(-50.0))),
        lit(13_649.0),
        Readout::RootUnknown(Shift::of_cents(-4_850.0), Light::Reference),
        Readout::Reference(Known::Still),
    ];
    for size in [TextSize::Normal, TextSize::Large] {
        for r in worst {
            let s = shown(r, size);
            let (x, y, w, h) = s.back;
            assert!(w <= CORNER_W + 4, "{size:?} {r:?}: {w}");
            assert!(x + w == theme::VIZ_RIGHT + 1 && y == theme::VIZ_BAND_TOP);
            assert!(y + h < theme::VIZ_BAND_MID, "{size:?}: {h}");
            for l in s.lines.iter().flatten() {
                assert!(l.x >= x, "{size:?} {}", l.text.as_str());
            }
        }
    }
    let none = shown(Readout::None, TextSize::Normal);
    assert!(none.lines.iter().all(Option::is_none) && none.back.2 == 0);
}

/// A reference and a source with no root say so on the second line; a
/// dim note reads as a lit one, only its ink differs.
#[test]
fn reference_and_root_unknown_say_so() {
    let c4 = Known::Note(Tone::of_cents(6_000.0));
    assert_eq!(read(Readout::Reference(c4)).1, "REF C4 261.6 Hz");
    assert_eq!(read(Readout::Dim(c4)), read(Readout::Lit(c4)));
    let r = Readout::RootUnknown(Shift::of_cents(-1_200.0), Light::Reference);
    assert_eq!(read(r), ("ROOT ?".into(), "REF C4 -12st +0ct".into()));
    assert_eq!(Tone::of_cents(6_900.0).hz().get(), 440.0);
}

fn tick(ui: &mut UiState) {
    ui.update(chimera_core::ui::animation::UiTick::for_test());
}

fn pit(engine: EngineType) -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    stack_map(&mut ui);
    load_init(&mut ui, engine);
    to_pitch(&mut ui, engine);
    settle(&mut ui);
    ui
}

fn colors(s: &Shown) -> Vec<embedded_graphics::pixelcolor::Rgb565> {
    s.lines.iter().flatten().map(|l| l.color).collect()
}

/// Before any note the corner reads C4 as a reference, dim; after one, its
/// pitch, dim while the Part is silent and lit while it sounds.
#[test]
fn silence_reads_the_last_note_dim_and_the_reference_before_any() {
    let mut ui = pit(EngineType::Algo);
    ui.hear_note(P1, None, false);
    tick(&mut ui);
    assert_eq!(texts(ui.tuner()), ["C4 +0ct", "REF C4 261.6 Hz"]);
    assert_eq!(colors(ui.tuner()), [theme::MID; 2]);
    ui.hear_note(P1, MidiNote::new(69), false);
    tick(&mut ui);
    assert_eq!(texts(ui.tuner()), ["A4 +0ct", "440.0 Hz"]);
    assert_eq!(colors(ui.tuner()), [theme::MID; 2]);
    ui.hear_note(P1, MidiNote::new(69), true);
    tick(&mut ui);
    assert_eq!(colors(ui.tuner()), [theme::INK, theme::INK2]);
    // Off PIT, nothing.
    feed(&mut ui, Input::press(chimera_hal::ButtonId::Edit));
    tick(&mut ui);
    assert_ne!(
        ui.page_def().viz,
        chimera_core::ui::block_def::VizType::Tuner
    );
    assert!(ui.tuner().lines.iter().all(Option::is_none));
}

/// Tuning by eye between notes: each FINE detent moves the cents the
/// frame it turns, and PITCH the note, with no note held.
#[test]
fn fine_moves_the_cents_the_frame_it_turns() {
    let mut ui = pit(EngineType::Modal);
    ui.hear_note(P1, MidiNote::new(60), false);
    tick(&mut ui);
    assert_eq!(texts(ui.tuner())[0], "C4 +0ct");
    for (by, want) in [(1, "C4 +1ct"), (11, "C4 +12ct"), (-30, "C4 -18ct")] {
        let was = ui.params().pitch.fine;
        feed(&mut ui, Input::turn(EncoderId::B, by));
        tick(&mut ui);
        assert_eq!(
            ui.params().pitch.fine - was,
            f32::from(by),
            "a detent a cent"
        );
        assert_eq!(texts(ui.tuner())[0], want);
    }
    feed(&mut ui, Input::turn(EncoderId::A, 2));
    tick(&mut ui);
    assert_eq!(texts(ui.tuner())[0], "D4 -18ct");
}

/// TAPE on PIT: the source's root and PITCH, and `ROOT ?` on one without.
#[test]
fn tape_on_pit_reads_root_and_transposition() {
    let mut ui = pit(EngineType::Tape);
    let s = tape(Way::Loop, 4_800.0, 183.4);
    let edit = &mut ui.project_mut().edit_part(P1).sound;
    edit.source = s.source;
    edit.params.tape.root = 4_800.0;
    edit.params.tape.period = 183.4;
    ui.hear_note(P1, MidiNote::new(48), true);
    feed(&mut ui, Input::turn(EncoderId::A, 7));
    tick(&mut ui);
    assert_eq!(texts(ui.tuner()), ["G3 +0ct", "196.0 Hz"]);
    ui.project_mut().edit_part(P1).sound.params.tape.period = 0.0;
    tick(&mut ui);
    assert_eq!(texts(ui.tuner()), ["ROOT ?", "+7st +0ct"]);
}

// ── Drawing ──────────────────────────────────────────────────────────

fn full(ui: &UiState) -> Fb {
    let mut fb = Fb::new();
    ui.render_with_scope(
        &mut fb,
        &PerfStats::zero(),
        &[0.0; chimera_core::scope::SCOPE_LEN],
    );
    fb
}

fn dirty(ui: &mut UiState, fb: &mut Fb) -> Vec<(u16, u16)> {
    ui.render_dirty_with_scope(
        fb,
        &PerfStats::zero(),
        &[0.0; chimera_core::scope::SCOPE_LEN],
    )
    .into_iter()
    .filter(|r| *r != (0, 0))
    .collect()
}

fn primed(ui: &mut UiState) -> Fb {
    let mut fb = full(ui);
    let quiet = [0.0; chimera_core::scope::SCOPE_LEN];
    ui.prime_regions(&PerfStats::zero(), None, &quiet);
    assert!(dirty(ui, &mut fb).is_empty(), "primed");
    fb
}

/// Silent, sounding or before any note: once drawn, nothing redraws while
/// nothing moves.
#[test]
fn nothing_redraws_at_idle() {
    for engine in [EngineType::Algo, EngineType::Tape, EngineType::Va] {
        for (note, sounds) in [
            (None, false),
            (MidiNote::new(69), false),
            (MidiNote::new(69), true),
        ] {
            let mut ui = pit(engine);
            ui.hear_note(P1, note, sounds);
            settle(&mut ui);
            let mut fb = primed(&mut ui);
            for _ in 0..40 {
                tick(&mut ui);
                assert!(
                    dirty(&mut ui, &mut fb).is_empty(),
                    "{engine:?} {note:?} {sounds}"
                );
            }
        }
    }
}

/// FINE turned and easing: the dirty screen is the full one every frame,
/// the corner's cents moving with it.
#[test]
fn a_dirty_redraw_equals_a_full_one_while_fine_turns() {
    for size in [TextSize::Normal, TextSize::Large] {
        let mut ui = pit(EngineType::Algo);
        set_text(&mut ui, size);
        ui.hear_note(P1, MidiNote::new(69), false);
        settle(&mut ui);
        let mut fb = primed(&mut ui);
        for frame in 0..30 {
            if frame % 3 == 0 {
                feed(&mut ui, Input::turn(EncoderId::B, 7));
            }
            tick(&mut ui);
            dirty(&mut ui, &mut fb);
            assert!(fb.px == full(&ui).px, "{size:?} frame {frame}");
        }
    }
}
