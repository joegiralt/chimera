//! TAPE's musical UI fixes (#698): START and END turn in time and MIX
//! snaps them to onsets (finding 6), a streamed source shows its
//! polyphony (1), refuses machines on MODEL where the player is (5), and
//! says when XFADE is capped (14); WOW dims at TAPE 0 (14) and LOOP's and
//! PING's FADE under an AMP route (3).

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::tape_engine::Way;
use chimera_core::dsp::tape_engine::read::SourceView;
use chimera_core::dsp::tape_engine::{TapeEngineParams, TapeModel};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::PartId;
use chimera_core::storage::{SourceRef, TapeSource};
use chimera_core::stream::units::SourceRate;
use chimera_core::ui::block_registry::{TAPE_DECK, TAPE_MODEL};
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::held::Reason;
use chimera_core::ui::onset;
use chimera_core::ui::view::{self, SourceState};
use chimera_core::ui::{draw, theme};
use chimera_hal::{ButtonId, EncoderId};
use common::tape::{held_plain, sine};
use screen::*;

type P = TapeEngineParams;

fn tape(id: ParamId) -> ParamAddr {
    ParamAddr::new(BlockRef::TapeEngine, id)
}

fn sound_on(m: Way) -> Sound {
    let mut s = Sound::init(EngineType::Tape);
    s.params.tape.set_way(m);
    s
}

fn fits_band(s: &str) {
    let w = draw::text_width(&theme::FONT_LABEL, s, 0);
    assert!(w <= theme::CONTENT_W, "{s}: {w} px");
}

/// A UI on Part 1's TAPE init at DECK, reading `held` as source `r`.
fn ui_on(r: SourceRef, held: &[u8]) -> chimera_core::ui::UiState {
    let mut ui = chimera_core::ui::UiState::new();
    load_init(&mut ui, EngineType::Tape);
    ui.project_mut().source_store(r, held).unwrap();
    ui.project_mut().edit_part(PartId::ALL[0]).sound.source = Some(TapeSource { r, tune: None });
    plus(&mut ui, 1);
    assert_eq!(page_def_id(&ui), TAPE_DECK.id);
    ui
}

/// A streamed 30 s 24 kHz source on Part 1.
fn streamed_ui() -> chimera_core::ui::UiState {
    use chimera_core::stream::ring::{Rings, ThirdSlots};
    use chimera_core::stream::test_card::TestCard;
    let whole = held_plain(&sine(24_000, 100.7, 720_000), 24_000);
    let rings: &'static Rings = Box::leak(Box::new(Rings::new()));
    let third: &'static ThirdSlots = Box::leak(Box::new(ThirdSlots::new()));
    let (_, card) = rings.sides(third).unwrap();
    let held = TestCard::new(card, &whole, 0).held(&[0, 0], 4_800);
    let r = SourceRef {
        crc: 0x5EED,
        frames: 720_000,
        rate: 24_000,
    };
    ui_on(r, &held)
}

fn band(s: SourceState) -> String {
    let mut b = FmtBuf::new();
    s.text(&mut b);
    b.as_str().to_string()
}

#[test]
fn streamed_source_shows_its_polyphony() {
    let ui = streamed_ui();
    let s = ui.source_state();
    assert_eq!(band(s), "24K · 30.0 S · 7 STREAMS");
    fits_band(&band(s));
    // Wow is billed only when TAPE runs: at 26 kHz it costs a note.
    let held = SourceState::Held {
        rate: SourceRate::new(26_000).unwrap(),
        frames: 26_000,
        cost: Default::default(),
        name: None,
        notes: None,
    };
    assert!(band(held).starts_with("26K · 1.0 S"), "RAM: no polyphony");
    assert_eq!(band(held.streaming(false)), "26K · 1.0 S · 7 STREAMS");
    assert_eq!(band(held.streaming(true)), "26K · 1.0 S · 6 STREAMS");
    assert_eq!(
        band(held.streaming(false).tape_routed(true)),
        band(held.streaming(true))
    );
    // 48 kHz: three.
    let h48 = SourceState::Held {
        rate: SourceRate::new(48_000).unwrap(),
        frames: 48_000,
        cost: Default::default(),
        name: None,
        notes: None,
    };
    assert_eq!(band(h48.streaming(false)), "48K · 1.0 S · 3 STREAMS");
    // 16 kHz fits 11 at ROOT, but there are eight voices (#706).
    let h16 = SourceState::Held {
        rate: SourceRate::new(16_000).unwrap(),
        frames: 24_000,
        cost: Default::default(),
        name: None,
        notes: None,
    };
    assert_eq!(band(h16.streaming(false)), "16K · 1.5 S · 8 STREAMS");
}

/// A mapped CC is refused what a turn is (review of #761, m3): on a
/// streamed source, DIRECTION onto PING-PONG or MODEL onto SCRUB writes
/// nothing, so a key never plays into silence; FORWARD streams, written.
/// Mapped CCs apply only with `midi-map` (#507).
#[cfg(feature = "midi-map")]
#[test]
fn a_cc_is_refused_what_a_turn_is() {
    use chimera_core::dsp::tape_engine::{Direction, TapeMachine};
    use chimera_core::midi_in::{Cc, CcEvent};
    use chimera_core::midi_map::Mappable;
    use chimera_core::ui::held::{DIRECTION, MODEL};
    let mut ui = streamed_ui();
    let p = PartId::ALL[0];
    let ch = ui.project().part(p).mix.channel;
    {
        let s = ui.project_mut().edit_part(p).sound;
        for (cc, a) in [(30, DIRECTION), (31, MODEL)] {
            s.map
                .learn(
                    chimera_core::midi_map::Input::cc(cc).unwrap(),
                    Mappable::new(a).unwrap(),
                )
                .unwrap();
        }
    }
    let mut now = 0;
    // Four values a CC: value `k` is its bin's middle, 32 apart.
    let mut cc = |ui: &mut chimera_core::ui::UiState, cc: u8, k: u8| {
        for _ in 0..4 {
            now += 20;
            let ev = CcEvent {
                channel: ch,
                cc: Cc::new(cc).unwrap(),
                value: 32 * k + 16,
            };
            ui.hear_ccs([ev].into_iter(), chimera_hal::Ms(now), true);
        }
    };
    assert_eq!(ui.params().tape.way(), Way::Loop);
    cc(&mut ui, 30, Direction::Ping as u8);
    assert_eq!(ui.params().tape.way(), Way::Loop, "PING-PONG refused");
    cc(&mut ui, 31, TapeMachine::Scrub.index());
    assert_eq!(ui.params().tape.machine, TapeMachine::Walk, "SCRUB refused");
    cc(&mut ui, 30, Direction::Forward as u8);
    assert_eq!(ui.params().tape.way(), Way::One, "FORWARD streams");
}

#[test]
fn direction_holds_what_a_streamed_source_refuses() {
    use chimera_core::dsp::tape_engine::{Direction, TapeMachine};
    use chimera_core::ui::held::{Pick, TapeTurn};
    let mut ui = streamed_ui();
    assert_eq!(ui.params().tape.way(), Way::Loop);
    feed(&mut ui, Input::turn(EncoderId::D, 1)); // LOOP → PING-PONG: refused
    assert_eq!(ui.params().tape.way(), Way::Loop, "nothing written");
    let shown = ui.held_choice().expect("PING-PONG held");
    assert_eq!(shown.held.code(), Direction::Ping as u8);
    assert!(matches!(shown.why, Reason::NeedsRam(_)));
    let why = chimera_core::ui::held::reason(shown.why);
    assert_eq!(why.as_str(), "PING NEEDS RAM · STREAMED");
    fits_band(why.as_str());
    // The held cell reads PING-PONG, dimmed.
    let shot = render_ui(&ui);
    assert!(cell_reads(&shot, 3, "PING"));
    // REVERSE reads back, so it is held too.
    feed(&mut ui, Input::turn(EncoderId::D, -2));
    assert_eq!(
        ui.held_choice().map(|s| s.held.code()),
        Some(Direction::Reverse as u8)
    );
    // On from the held choice: FORWARD streams: written.
    feed(&mut ui, Input::turn(EncoderId::D, -1));
    assert_eq!(ui.params().tape.way(), Way::One);
    assert!(ui.held_choice().is_none());
    // TRON streams.
    let mut t = ui.project().part(PartId::ALL[0]).sound.clone();
    t.params.tape.tron = chimera_core::dsp::tape_engine::Tron::On;
    let h = Pick::Tape(TapeTurn::Direction(Direction::Forward));
    assert_eq!(h.why(&t, true), None);
    // Each machine that seeks says so.
    for m in [TapeMachine::Scrub, TapeMachine::Stretch, TapeMachine::Wave] {
        let h = Pick::Tape(TapeTurn::Model(m));
        let s = &ui.project().part(PartId::ALL[0]).sound;
        let w = h.why(s, true).expect("refused");
        fits_band(chimera_core::ui::held::reason(w).as_str());
        assert_eq!(h.why(s, false), None, "a RAM source plays it");
    }
}

#[test]
fn direction_turns_freely_on_a_ram_source() {
    let held = held_plain(&sine(24_000, 100.7, 24_000), 24_000);
    let r = SourceRef {
        crc: 0xA,
        frames: 24_000,
        rate: 24_000,
    };
    let mut ui = ui_on(r, &held);
    feed(&mut ui, Input::turn(EncoderId::D, 1));
    assert_eq!(ui.params().tape.way(), Way::Ping);
    assert!(ui.held_choice().is_none());
}

#[test]
fn wow_dims_while_tape_rests() {
    let mut s = sound_on(Way::Loop);
    assert!(view::dimmed(tape(P::WOW), &s), "TAPE 0: no wow");
    assert_eq!(view::tape_note(tape(P::WOW), &s), Some(view::WOW_AT_REST));
    s.params.tape.tape = 0.2;
    assert!(!view::dimmed(tape(P::WOW), &s));
    assert_eq!(view::tape_note(tape(P::WOW), &s), None);
    fits_band(view::WOW_AT_REST);
}

#[test]
fn fade_steps_aside_for_amp_release() {
    // INIT routes ENV 1 → VCA: AMP's release is the note's one release.
    for m in [Way::Loop, Way::Ping] {
        let s = sound_on(m);
        assert!(view::dimmed(tape(P::DRAG), &s), "{m:?}");
        assert_eq!(view::tape_note(tape(P::DRAG), &s), Some(view::FADE_ASIDE));
    }
    // ONE's FADE keeps its HOLD step live; TRON's FADE is the machine.
    for m in [Way::One, Way::Tron] {
        assert!(!view::dimmed(tape(P::DRAG), &sound_on(m)), "{m:?}");
    }
    // Below HOLD, ONE's band says AMP releases it (#706); at HOLD, nothing.
    let src = SourceState::None;
    let mut one = sound_on(Way::One);
    one.params.tape.drag = 0.5;
    assert_eq!(
        view::band_note(tape(P::DRAG), &one, src),
        Some(view::FADE_HOLD_ONLY)
    );
    one.params.tape.drag = chimera_core::dsp::tape_engine::FADE_HOLD;
    assert_eq!(view::band_note(tape(P::DRAG), &one, src), None);
    fits_band(view::FADE_HOLD_ONLY);
    // No route into VCA: the key gates the voice, and FADE releases it.
    let mut s = sound_on(Way::Loop);
    s.mod_state = Default::default();
    assert!(!view::dimmed(tape(P::DRAG), &s));
    fits_band(view::FADE_ASIDE);
}

#[test]
fn streamed_xfade_says_its_cap() {
    let s = sound_on(Way::Loop);
    let ram = SourceState::Held {
        rate: SourceRate::new(24_000).unwrap(),
        frames: 720_000,
        cost: Default::default(),
        name: None,
        notes: None,
    };
    let streamed = ram.streaming(false);
    let x = tape(P::XFADE);
    // XFADE 0.1 of a 30 s loop asks 3 s: the loop head holds 100 ms.
    assert_eq!(view::band_note(x, &s, streamed), Some(view::XFADE_CAPPED));
    assert_eq!(view::band_note(x, &s, ram), None, "RAM holds any fade");
    let mut short = s.clone();
    short.params.tape.xfade = 0.003; // 90 ms
    assert_eq!(view::band_note(x, &short, streamed), None);
    fits_band(view::XFADE_CAPPED);
    // On the page: the band says it where the hint goes.
    let mut ui = streamed_ui();
    tap(&mut ui, ButtonId::Seq); // PLAY's page
    assert_eq!(ui.page_def().id, TAPE_MODEL.id);
    feed(&mut ui, Input::turn(EncoderId::E, 1)); // XFADE
    settle(&mut ui);
    assert!(label_reads(&render_ui(&ui), view::XFADE_CAPPED));
}

#[test]
fn start_and_end_turn_in_time() {
    // A slow detent is 1 ms, a fast one 100 ms, a full-speed one at
    // least a hundredth of the source.
    let (rate, frames) = (24_000u16, 600_000u32); // 25 s
    let ms = |v: f32| v * 25_000.0;
    assert!((ms(onset::stepped(0.0, 1, rate, frames)) - 1.0).abs() < 1e-3);
    assert!((ms(onset::stepped(0.0, 4, rate, frames)) - 16.0).abs() < 1e-2);
    assert!((ms(onset::stepped(0.5, -1, rate, frames)) - 12_499.0).abs() < 0.01);
    assert!((ms(onset::stepped(0.0, 10, rate, frames)) - 250.0).abs() < 0.1);
    assert_eq!(onset::stepped(1.0, 3, rate, frames), 1.0);
    let text = |v| {
        let mut b = FmtBuf::new();
        onset::time_text(&mut b, v, rate, frames);
        b.as_str().to_string()
    };
    assert_eq!(text(137.0 / 25_000.0), "137 MS");
    assert_eq!(text(0.4938), "12.345 S");
    for t in ["999 MS", "88.888 S"] {
        let w = draw::text_width(&theme::FONT_VALUE, t, 0);
        assert!(w <= theme::CELL_COL_W - 4, "{t}: {w}");
    }

    // ONE's START on DECK: a detent is a millisecond of the source.
    let held = held_plain(&sine(24_000, 100.7, 96_000), 24_000);
    let r = SourceRef {
        crc: 0xB,
        frames: 96_000,
        rate: 24_000,
    };
    let mut ui = ui_on(r, &held);
    feed(&mut ui, Input::turn(EncoderId::D, -2)); // FORWARD
    assert_eq!(ui.params().tape.way(), Way::One);
    feed(&mut ui, Input::press(chimera_hal::ButtonId::Minus)); // SRC: START
    feed(&mut ui, Input::turn(EncoderId::B, 1).at(1_000));
    let start = ui.params().tape.start;
    assert!((start * 4_000.0 - 1.0).abs() < 1e-3, "{start}");
    settle(&mut ui);
    assert!(cell_reads(&render_ui(&ui), 1, "1 MS"));
}

/// Silence, then hits at `at`: a 5 ms burst of a 1 kHz sine at 24 kHz.
fn hits(n: u32, at: &[u32]) -> Vec<f32> {
    let mut x = vec![0.0f32; n as usize];
    for &a in at {
        for i in 0..120u32 {
            let k = (a + i) as usize;
            x[k] = 0.8 * libm::sinf(core::f32::consts::TAU * i as f32 / 24.0);
        }
    }
    x
}

#[test]
fn onsets_are_found_either_way() {
    let held = held_plain(&hits(48_000, &[12_000, 30_000]), 24_000);
    let v = SourceView::new(&held).unwrap();
    let near = |f: Option<u32>, want: u32| {
        let f = f.expect("an onset");
        assert!(f.abs_diff(want) <= 2, "{f} for {want}");
    };
    near(onset::onset_in(&v, 0, true), 12_000);
    near(onset::onset_in(&v, 12_000, true), 30_000);
    near(onset::onset_in(&v, 40_000, false), 30_000);
    near(onset::onset_in(&v, 30_000, false), 12_000);
    assert_eq!(onset::onset_in(&v, 30_000, true), None);
    assert_eq!(onset::onset_in(&v, 12_000, false), None);
    // Through the held bytes, as the UI reads them.
    near(onset::onset(&held, 48_000, 0, true), 12_000);
}

#[test]
fn mix_turn_snaps_start_to_the_next_onset() {
    let held = held_plain(&hits(48_000, &[12_000, 30_000]), 24_000);
    let r = SourceRef {
        crc: 0xC,
        frames: 48_000,
        rate: 24_000,
    };
    let mut ui = ui_on(r, &held);
    feed(&mut ui, Input::turn(EncoderId::D, -2)); // FORWARD
    tap(&mut ui, ButtonId::Minus); // SRC: START, END
    let mix = |d| Input::turn(EncoderId::B, d).and_held(ButtonId::Mix);
    feed(&mut ui, mix(1));
    let at = |ui: &chimera_core::ui::UiState| (ui.params().tape.start * 48_000.0).round() as u32;
    assert!(at(&ui).abs_diff(12_000) <= 2, "{}", at(&ui));
    feed(&mut ui, mix(1));
    assert!(at(&ui).abs_diff(30_000) <= 2, "{}", at(&ui));
    feed(&mut ui, mix(1)); // none after: stays
    assert!(at(&ui).abs_diff(30_000) <= 2);
    feed(&mut ui, mix(-1));
    assert!(at(&ui).abs_diff(12_000) <= 2, "{}", at(&ui));
    // The cell says so in its band.
    let s = &ui.project().part(PartId::ALL[0]).sound;
    let none = view::SlotCtx::read(&s.params, chimera_core::addr::Op::A);
    let ctx = view::SlotCtx {
        src: ui.source_state(),
        ..none
    };
    for i in [1, 2] {
        let v = view::view(&chimera_core::ui::block_registry::TAPE_SRC, i, &ctx);
        assert_eq!(v.hint(&ctx), view::ONSET_HINT, "{}", v.label());
        // With no source held, MIX + turn is the plain snap: no onset to
        // name (review of #761, m4).
        let v = view::view(&chimera_core::ui::block_registry::TAPE_SRC, i, &none);
        assert_ne!(v.hint(&none), view::ONSET_HINT, "{}", v.label());
    }
    assert!(chimera_core::ui::hint::width(view::ONSET_HINT) <= theme::CONTENT_W);
}

#[test]
fn model_page_reads_its_notes() {
    // WALK's model page at INIT (LOOP): WOW and RETRIG dimmed, each saying
    // why.
    let mut ui = chimera_core::ui::UiState::new();
    load_init(&mut ui, EngineType::Tape);
    plus(&mut ui, 1);
    tap(&mut ui, ButtonId::Seq); // down the page row's stack
    assert_eq!(ui.page_def().id, TAPE_MODEL.id);
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // WOW: dimmed, still focused
    settle(&mut ui);
    assert!(label_reads(&render_ui(&ui), view::WOW_AT_REST));
    assert_eq!(
        ui.params().tape.wow,
        chimera_core::dsp::tape_engine::Wow::Capstan,
        "dimmed: not turned"
    );
    feed(&mut ui, Input::turn(EncoderId::F, 1)); // RETRIG: LOOP never cuts
    settle(&mut ui);
    assert!(label_reads(&render_ui(&ui), view::RETRIG_ONCE_ONLY));
    assert_eq!(
        ui.params().tape.retrig,
        chimera_core::dsp::tape_engine::Retrig::Stack
    );
    assert_eq!(ui.params().tape.model(), TapeModel::Walk);
}

/// `text` in the label font where the focus band's hint goes, in MID.
fn label_reads(shot: &Fb, text: &str) -> bool {
    use chimera_core::ui::hint::FOCUS_Y;
    use chimera_core::ui::theme::{FONT_LABEL, MARGIN_X, MID};
    let t = chimera_core::ui::source_cell::tracking(text);
    let mut want = Fb::new();
    draw::text_tracked(&mut want, &FONT_LABEL, text, MARGIN_X, FOCUS_Y, MID, t);
    let mut any = false;
    for y in 0..theme::SCREEN_H {
        for x in 0..theme::SCREEN_W {
            if want.px[(y * theme::SCREEN_W + x) as usize] != 0 {
                any = true;
                if shot.at(x, y) != want.at(x, y) {
                    return false;
                }
            }
        }
    }
    any
}

/// SEQ + turn on DIRECTION, on a streamed source, is refused as a turn
/// is (review A2): the fine turn changes the step, never the guards.
#[test]
fn a_seq_turn_is_refused_what_a_turn_is() {
    let mut ui = streamed_ui();
    assert_eq!(ui.params().tape.way(), Way::Loop);
    feed(
        &mut ui,
        Input::held(ButtonId::Seq).and_turn(EncoderId::D, 1),
    );
    assert_eq!(ui.params().tape.way(), Way::Loop, "PING-PONG refused");
    assert!(ui.held_choice().is_some(), "held, dimmed, with its reason");
}
