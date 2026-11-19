//! SETTINGS › PART › MIDI MAP (spec § 8.2): each binding's row names its
//! control, its target and its input; the bar's binding is spelled out
//! under the list with its MIN, MAX and ECHO. A moves the bar; B steps the
//! control number (an NRPN or pressure is fixed), C and D the range, E the
//! input among the control's legal kinds, F the echo. The pickup hint
//! (§ 8.3) and learn's DONE (§ 8.1) on the sound pages.

#![cfg(feature = "midi-map")]

mod screen;

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::block::Motion;
use chimera_core::dsp::modal::ModalParams;
use chimera_core::midi_in::CtrlEvent;
use chimera_core::midi_map::{
    Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, Mappable, MidiMap, NrpnKind,
    Span,
};
use chimera_core::params::FilterParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::hint::{self, Gesture, Hint};
use chimera_core::ui::learn::{DONE, Learn};
use chimera_core::ui::settings::view::RowLook;
use chimera_core::ui::theme;
use chimera_core::ui::view::{SlotCtx, View, view};
use chimera_hal::ButtonId::{B1, Edit, Mix, Plus, Seq};
use chimera_hal::EncoderId::{self, A, B, C, D, E, F};
use chimera_hal::Ms;
use chimera_midi::ctl::{Ctl, Dir, FreeCc};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover::{self, Jump, Pickup, Scale};
use chimera_midi::value::{U7, U14};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use screen::{Fb, Input as Key, feed, render_ui, tap};

const P1: PartId = PartId::ALL[0];
const CH1: MidiChannel = match MidiChannel::new(0) {
    Some(c) => c,
    None => panic!(),
};
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);

fn at(a: ParamAddr) -> Mappable {
    Mappable::new(a).unwrap()
}

fn cc(n: u8) -> LearnCc {
    LearnCc::new(n).unwrap()
}

fn nrpn(msb: u8, lsb: u8) -> U14 {
    U14::from_parts(U7::new(msb).unwrap(), U7::new(lsb).unwrap())
}

fn bind(input: Input, target: ParamAddr) -> Binding {
    Binding {
        input,
        target: at(target),
        span: Span::FULL,
        feedback: Feedback::Off,
    }
}

/// Part 1 on INIT Algo holding `entries`, MIDI MAP open, the bar on the
/// first Sound row (row 1: row 0 is the controller map, ADR 0226).
fn page(entries: &[Binding]) -> Box<UiState> {
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    let mut m = MidiMap::EMPTY;
    for e in entries {
        m.learn(e.input, e.target).unwrap();
        let n = m.len() - 1;
        m.set(n, *e);
    }
    ui.project_mut().edit_part(P1).sound.map = m;
    feed(&mut ui, Key::press(B1));
    screen::to_leaf(&mut ui, &["PART"]);
    for _ in 1..chimera_core::ui::settings::tree::PART.len() {
        feed(&mut ui, Key::press(Plus));
    }
    feed(&mut ui, Key::press(Edit));
    assert_eq!(ui.location().settings().unwrap().row(), 0);
    feed(&mut ui, Key::turn(A, 1));
    assert_eq!(ui.location().settings().unwrap().row(), 1);
    ui
}

fn map(ui: &UiState) -> MidiMap {
    ui.project().part(P1).sound.map
}

fn entry(ui: &UiState, i: usize) -> Binding {
    map(ui).entries()[i]
}

fn turn(ui: &mut UiState, e: EncoderId, d: i8) {
    feed(ui, Key::turn(e, d));
}

/// Sound row `i` (page row `i + 1`).
fn row(ui: &UiState, i: usize) -> (String, String, RowLook) {
    let b = ui.map_band();
    let t = b.text(i + 1);
    (
        t.label.as_str().into(),
        t.word.as_str().into(),
        b.look(i + 1),
    )
}

/// Sound row `i`'s focus words: (head, tail).
fn focus(ui: &UiState, i: usize) -> (String, String) {
    let t = ui.map_band().text(i + 1);
    (t.head.as_str().into(), t.tail.as_str().into())
}

/// Every kind's row word and focus words, as spec § 8.2's table has them.
#[test]
fn row_words_per_kind() {
    use ByteEncoding::*;
    let c = cc(74);
    let n = nrpn(10, 3);
    let table: [(Input, &str, &str, &str); 10] = [
        (
            Input::Cc(c, CcKind::Absolute(Jump)),
            "JUMP",
            "ABSOLUTE · JUMPS TO THE KNOB",
            "",
        ),
        (
            Input::Cc(c, CcKind::Absolute(Pickup)),
            "PICKUP",
            "ABSOLUTE · WAITS TO PICK UP",
            "",
        ),
        (
            Input::Cc(c, CcKind::Absolute(Scale)),
            "SCALE",
            "ABSOLUTE · SCALES TO MEET",
            "",
        ),
        (
            Input::Cc(c, CcKind::Relative(TwosComplement)),
            "TWOS COMP",
            "RELATIVE · TWOS COMPLEMENT",
            "+1 = 1  -1 = 127",
        ),
        (
            Input::Cc(c, CcKind::Relative(BinaryOffset)),
            "OFFSET",
            "RELATIVE · BINARY OFFSET",
            "+1 = 65  -1 = 63",
        ),
        (
            Input::Cc(c, CcKind::Relative(SignBitDown)),
            "SIGNED",
            "RELATIVE · SIGNED BIT",
            "+1 = 1  -1 = 65",
        ),
        (
            Input::Cc(c, CcKind::Relative(SignBitUp)),
            "SIGNED 2",
            "RELATIVE · SIGNED BIT 2",
            "+1 = 65  -1 = 1",
        ),
        (
            Input::Nrpn(n, NrpnKind::IncDec),
            "INC DEC",
            "RELATIVE · NRPN",
            "INCREMENT AND DECREMENT",
        ),
        (
            Input::Cc(c, CcKind::Button(ButtonMode::Momentary)),
            "MOMENTARY",
            "BUTTON · HELD IS MAX",
            "",
        ),
        (
            Input::Cc(c, CcKind::Button(ButtonMode::Toggle)),
            "TOGGLE",
            "BUTTON · EACH PRESS FLIPS",
            "",
        ),
    ];
    for (input, word, head, tail) in table {
        let ui = page(&[bind(input, CUTOFF)]);
        assert_eq!(row(&ui, 0).1, word, "{input:?}");
        assert_eq!(focus(&ui, 0), (head.into(), tail.into()), "{input:?}");
    }
    // An NRPN's relative and button kinds read as a CC's.
    let ui = page(&[bind(
        Input::Nrpn(n, NrpnKind::Relative(BinaryOffset)),
        CUTOFF,
    )]);
    assert_eq!(row(&ui, 0).1, "OFFSET");
    let ui = page(&[bind(
        Input::Nrpn(n, NrpnKind::Button(ButtonMode::Toggle)),
        CUTOFF,
    )]);
    assert_eq!(row(&ui, 0).1, "TOGGLE");

    // Each control as the row names it; a dead target dimmed.
    let structure = ParamAddr::new(BlockRef::Modal, ModalParams::STRUCTURE);
    let ui = page(&[
        bind(Input::cc(74).unwrap(), CUTOFF),
        bind(
            Input::Cc14(Cc14Msb::new(2).unwrap(), Jump),
            ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE),
        ),
        bind(
            Input::Nrpn(n, NrpnKind::Absolute(Pickup)),
            ParamAddr::new(BlockRef::Part, chimera_core::part::PartParams::LEVEL),
        ),
        bind(
            Input::Pressure(Scale),
            ParamAddr::new(BlockRef::Part, chimera_core::part::PartParams::PAN),
        ),
        bind(Input::cc(16).unwrap(), structure),
    ]);
    assert_eq!(
        row(&ui, 0),
        ("CC 74  CUTOFF".into(), "JUMP".into(), RowLook::Normal)
    );
    assert_eq!(row(&ui, 1).0, "CC 2+34  RESO");
    assert_eq!(
        row(&ui, 2),
        ("NRPN 10:3  LEVEL".into(), "PICKUP".into(), RowLook::Normal)
    );
    assert_eq!(
        row(&ui, 3),
        ("PRESSURE  PAN".into(), "SCALE".into(), RowLook::Normal)
    );
    assert_eq!(
        row(&ui, 4),
        ("CC 16  - STRUCT".into(), "JUMP".into(), RowLook::Dimmed)
    );
    // The bar's MIN, MAX and ECHO.
    let f = ui.map_band().text(1);
    assert_eq!(
        (f.min.as_str(), f.max.as_str(), f.echo.as_str()),
        ("0", "255", "OFF")
    );
}

/// The words fit their lines: the row's word beside a short label, the
/// focus lines, the hint lines and the legend within the screen's margins.
#[test]
fn map_words_fit() {
    use chimera_core::ui::draw::text_width;
    use theme::{CONTENT_W, FONT_LABEL, FONT_LABEL_BOLD, LABEL_TRACKING as T};
    let n = nrpn(127, 127);
    let mut all = vec![];
    for i in [
        Input::cc(119).unwrap(),
        Input::Nrpn(n, NrpnKind::Absolute(Jump)),
    ] {
        all.extend(i.kinds());
    }
    for input in all {
        let ui = page(&[bind(input, CUTOFF)]);
        let f = ui.map_band().text(1);
        let head = text_width(&FONT_LABEL_BOLD, f.head.as_str(), T);
        let tail = text_width(&FONT_LABEL, f.tail.as_str(), T);
        assert!(
            head <= CONTENT_W && tail <= CONTENT_W,
            "{input:?}: {head} {tail}"
        );
        let values = f.values();
        for h in [f.hint(), &values[..]] {
            assert!(hint::width(h) <= CONTENT_W, "{input:?}: {h:?}");
        }
    }
    use chimera_core::ui::cc::{PICKUP_DOWN, PICKUP_UP};
    for h in [PICKUP_UP, PICKUP_DOWN, DONE] {
        assert!(hint::width(h) <= CONTENT_W, "{h:?}");
    }
    assert_eq!(hint::width(DONE), chimera_core::ui::learn::DONE_W);
    let legend = ui_legend(&page(&[bind(Input::cc(74).unwrap(), CUTOFF)]));
    assert!(text_width(&FONT_LABEL, &legend, 0) <= CONTENT_W, "{legend}");
}

fn ui_legend(ui: &UiState) -> String {
    ui.legend_for_test().unwrap().into()
}

/// The spec's hint words, split over the bar's lines (spec § 8.2 and ADR
/// 0086): B and E under the values, C, D and F with theirs; MIX+MINUS in
/// the legend.
#[test]
fn hint_names_every_key() {
    let ui = page(&[bind(Input::cc(74).unwrap(), CUTOFF)]);
    let t = ui.map_band().text(1);
    assert_eq!(
        t.hint(),
        &[
            Gesture("B", "CONTROL"),
            Gesture("E", "INPUT"),
            Gesture("EDIT", "TARGET")
        ][..]
    );
    let keys: Vec<&str> = t.values().iter().map(|g| g.0).collect();
    assert_eq!(keys, ["C MIN", "D MAX", "F ECHO"]);
    let l = ui_legend(&ui);
    assert!(
        l.contains("A MOVE") && l.contains("MIX+MINUS DELETES"),
        "{l}"
    );
}

/// A moves the bar; B to F edit the row under it and leave it.
#[test]
fn a_moves_the_bar_b_to_f_edit() {
    let reso = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
    let mut ui = page(&[
        bind(Input::cc(74).unwrap(), CUTOFF),
        bind(Input::cc(71).unwrap(), reso),
    ]);
    let bar = |ui: &UiState| ui.location().settings().unwrap().row();
    turn(&mut ui, A, 1);
    assert_eq!(bar(&ui), 2);
    for (e, d) in [(B, 1), (C, 1), (D, -1), (E, 1), (F, 1)] {
        let rev = ui.project().rev();
        turn(&mut ui, e, d);
        assert_eq!(bar(&ui), 2, "{e:?} keeps the bar");
        assert_ne!(ui.project().rev(), rev, "{e:?} edits");
    }
    assert_eq!(
        entry(&ui, 0),
        bind(Input::cc(74).unwrap(), CUTOFF),
        "row 0 untouched"
    );
}

/// E cycles the kinds legal for the control, the control and echo kept.
#[test]
fn e_cycles_only_legal_kinds() {
    let n = nrpn(10, 3);
    for (input, count) in [
        (Input::cc(74).unwrap(), 9),
        (Input::Cc14(Cc14Msb::new(2).unwrap(), Jump), 3),
        (Input::Pressure(Jump), 3),
        (Input::Nrpn(n, NrpnKind::Absolute(Jump)), 10),
    ] {
        let mut e = bind(input, CUTOFF);
        e.feedback = Feedback::Value;
        let mut ui = page(&[e]);
        let legal: Vec<Input> = input.kinds().collect();
        assert_eq!(legal.len(), count);
        let mut seen = vec![input];
        for _ in 1..count {
            turn(&mut ui, E, 1);
            let now = entry(&ui, 0);
            assert!(legal.contains(&now.input), "{:?}", now.input);
            assert_eq!(now.feedback, Feedback::Value, "E never changes F");
            assert_eq!(now.target, e.target);
            seen.push(now.input);
        }
        seen.dedup();
        assert_eq!(seen, legal, "every kind once, in order");
        turn(&mut ui, E, 1);
        assert_eq!(entry(&ui, 0).input, input, "it wraps");
        turn(&mut ui, E, -1);
        assert_eq!(entry(&ui, 0).input, legal[count - 1], "and back");
    }
    // IncDec is an NRPN's only.
    let mut ui = page(&[bind(Input::cc(74).unwrap(), CUTOFF)]);
    for _ in 0..12 {
        turn(&mut ui, E, 1);
        assert!(matches!(entry(&ui, 0).input, Input::Cc(..)));
    }
}

/// F: up is ON, down is OFF; the input stays.
#[test]
fn f_toggles_echo() {
    let mut ui = page(&[bind(Input::cc(74).unwrap(), CUTOFF)]);
    turn(&mut ui, F, 1);
    assert_eq!(entry(&ui, 0).feedback, Feedback::Value);
    assert_eq!(ui.map_band().text(1).echo.as_str(), "ON");
    let rev = ui.project().rev();
    turn(&mut ui, F, 1);
    assert_eq!(entry(&ui, 0).feedback, Feedback::Value);
    assert_eq!(ui.project().rev(), rev, "ON stays ON");
    turn(&mut ui, F, -1);
    assert_eq!(entry(&ui, 0).feedback, Feedback::Off);
    assert_eq!(entry(&ui, 0).input, Input::cc(74).unwrap());
}

/// Each key's edit redraws: the dirty bands land on the whole screen.
#[test]
fn map_edits_redraw() {
    let reso = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
    let mut ui = page(&[
        bind(Input::cc(74).unwrap(), CUTOFF),
        bind(Input::cc(71).unwrap(), reso),
    ]);
    let mut fb = Fb::new();
    redraw(&mut ui, &mut fb);
    for (e, d) in [(A, 1), (B, 1), (C, 3), (D, -3), (E, 3), (F, 1), (E, 1)] {
        turn(&mut ui, e, d);
        redraw(&mut ui, &mut fb);
        assert!(fb.px == render_ui(&ui).px, "{e:?}: the dirty bands miss it");
    }
}

/// B steps a CC past the reserved numbers and a 14-bit pair along its
/// MSBs, its kind kept, clamped at the ends.
#[test]
fn b_skips_reserved_and_steps_cc14() {
    let pick = CcKind::Absolute(Pickup);
    let mut ui = page(&[bind(Input::Cc(cc(63), pick), CUTOFF)]);
    turn(&mut ui, B, 1);
    assert_eq!(
        entry(&ui, 0).input,
        Input::Cc(cc(65), pick),
        "64 is sustain"
    );
    turn(&mut ui, B, -1);
    assert_eq!(entry(&ui, 0).input, Input::Cc(cc(63), pick));
    let mut ui = page(&[bind(Input::Cc(cc(119), pick), CUTOFF)]);
    turn(&mut ui, B, 3);
    assert_eq!(
        entry(&ui, 0).input,
        Input::Cc(cc(119), pick),
        "120-127 are mode"
    );

    let msb = |n| Cc14Msb::new(n).unwrap();
    let mut ui = page(&[bind(Input::Cc14(msb(5), Scale), CUTOFF)]);
    turn(&mut ui, B, 1);
    assert_eq!(
        entry(&ui, 0).input,
        Input::Cc14(msb(7), Scale),
        "6 is data entry"
    );
    assert_eq!(row(&ui, 0).0, "CC 7+39  CUTOFF");
    turn(&mut ui, B, 30);
    assert_eq!(
        entry(&ui, 0).input,
        Input::Cc14(msb(31), Scale),
        "31 the last MSB"
    );
    turn(&mut ui, B, -40);
    assert_eq!(
        entry(&ui, 0).input,
        Input::Cc14(msb(1), Scale),
        "1 the first"
    );
}

/// An NRPN or pressure control is fixed: B changes nothing, and the hint
/// says to re-learn.
#[test]
fn nrpn_row_b_does_nothing_and_hints() {
    for input in [
        Input::Nrpn(nrpn(10, 3), NrpnKind::Absolute(Jump)),
        Input::Pressure(Jump),
    ] {
        let mut ui = page(&[bind(input, CUTOFF)]);
        let rev = ui.project().rev();
        turn(&mut ui, B, 1);
        turn(&mut ui, B, -3);
        assert_eq!(ui.project().rev(), rev, "{input:?}");
        assert_eq!(entry(&ui, 0).input, input);
        assert_eq!(
            ui.map_band().text(1).hint(),
            &[
                Gesture("B", "FIXED"),
                Gesture("E", "INPUT"),
                Gesture("EDIT", "TARGET")
            ][..]
        );
    }
}

/// DELETE's question names the control as the row does.
#[test]
fn delete_names_the_control() {
    let mut ui = page(&[bind(Input::Nrpn(nrpn(10, 3), NrpnKind::IncDec), CUTOFF)]);
    feed(&mut ui, Key::chord(Mix, chimera_hal::ButtonId::Minus));
    let (q, r) = ui.prompt_words_for_test().expect("a prompt");
    assert_eq!((q.as_str(), r.as_str()), ("DELETE NRPN 10:3?", "CUTOFF"));
}

// The pickup hint and learn's DONE on the sound pages.

/// Part 1 on INIT Algo, empty map, on the page `def_id` and its first
/// continuous mappable cell focused: (the UI, that cell's addr).
fn on_page(to: impl Fn(&mut UiState)) -> (Box<UiState>, ParamAddr) {
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part(&mut ui, P1);
    ui.project_mut().edit_part(P1).sound.map = MidiMap::EMPTY;
    to(&mut ui);
    let ctx = SlotCtx::read(&ui.project().part(P1).sound.params, Op::A);
    let def = ui.page_def();
    let (slot, a) = (0..def.params.len())
        .find_map(|i| {
            let a = view(def, i, &ctx).addr()?;
            let continuous = matches!(a.spec()?.motion(), Motion::Continuous(_));
            (continuous && Mappable::new(a).is_some()).then_some((i, a))
        })
        .expect("a continuous mappable cell");
    assert_ne!(view(def, slot, &ctx), View::Empty);
    let enc = chimera_hal::ALL_ENCODERS[slot];
    turn(&mut ui, enc, 1);
    turn(&mut ui, enc, -1);
    // Near the middle, so the knob can wait either way.
    for _ in 0..200 {
        match place(&ui, a) {
            p if p > 0.6 => turn(&mut ui, enc, -1),
            p if p < 0.4 => turn(&mut ui, enc, 1),
            _ => break,
        }
    }
    let p = place(&ui, a);
    assert!((0.4..=0.6).contains(&p), "{a:?} at {p}");
    (ui, a)
}

fn to_pitch(ui: &mut UiState) {
    screen::to_pitch(ui, chimera_core::params::EngineType::Algo);
}

fn to_filter(ui: &mut UiState) {
    for _ in 0..12 {
        if ui.page_def().id == FILTER.id {
            break;
        }
        feed(ui, Key::press(Plus));
    }
    assert_eq!(ui.page_def().id, FILTER.id);
}

fn send(ui: &mut UiState, now: &mut u32, c: Ctl) {
    *now += 4;
    ui.hear_ccs(core::iter::once(CtrlEvent::Ctl(CH1, c)), Ms(*now), true);
}

fn cc_msg(n: u8, v: u8) -> Ctl {
    Ctl::Cc {
        cc: FreeCc::new(n).unwrap(),
        value: U7::new(v).unwrap(),
    }
}

fn place(ui: &UiState, a: ParamAddr) -> f32 {
    let v = ui
        .project()
        .read_part(P1)
        .block(a.block)
        .unwrap()
        .get(a.param);
    a.spec().unwrap().normalize(v)
}

/// Whether `h` is drawn with its left edge at `x`, baseline `y`: every
/// pixel it lights, lit the same in `shot`.
fn drawn_at(shot: &Fb, h: Hint, x: i32, y: i32) -> bool {
    let mut want = Fb::new();
    hint::draw(&mut want, h, x, y);
    let mut any = false;
    for py in (y - 12).max(0)..(y + 4).min(320) {
        for px in 0..240 {
            let w = want.at(px, py);
            if w != Rgb565::BLACK {
                any = true;
                if shot.at(px, py) != w {
                    return false;
                }
            }
        }
    }
    any
}

/// Whether `h` is drawn anywhere along baseline `y`.
fn drawn_on(shot: &Fb, h: Hint, y: i32) -> bool {
    (0..240 - hint::width(h)).any(|x| drawn_at(shot, h, x, y))
}

/// A waiting pickup names the way to turn, on a focus-band page and on
/// FLT (BigViz), and clears once the knob catches the param.
#[test]
fn pickup_hint_up_and_down() {
    use chimera_core::ui::cc::{PICKUP_DOWN, PICKUP_UP};
    for (to, y) in [
        (to_pitch as fn(&mut UiState), hint::FOCUS_Y),
        (to_filter, theme::BIGVIZ_STATUS_Y),
    ] {
        for (v, dir, h, other) in [
            (0, Dir::Up, PICKUP_UP, PICKUP_DOWN),
            (127, Dir::Down, PICKUP_DOWN, PICKUP_UP),
        ] {
            let (mut ui, a) = on_page(to);
            let mut m = MidiMap::EMPTY;
            let e = bind(Input::Cc(cc(21), CcKind::Absolute(Pickup)), a);
            m.learn(e.input, e.target).unwrap();
            m.set(0, e);
            ui.project_mut().edit_part(P1).sound.map = m;
            let mut now = 10_000;
            let was = place(&ui, a);
            assert_eq!(ui.pickup(), None, "nothing heard: no hint");
            assert!(!drawn_on(&render_ui(&ui), h, y));

            // The screen as the unit draws it: only its dirty bands redraw.
            let mut dirty = Fb::new();
            redraw(&mut ui, &mut dirty);
            send(&mut ui, &mut now, cc_msg(21, v));
            assert_eq!(ui.pickup(), Some(dir), "{a:?} at {was}");
            redraw(&mut ui, &mut dirty);
            assert!(drawn_on(&dirty, h, y), "{a:?}: the hint's band redraws");
            assert_eq!(place(&ui, a), was, "waiting moves nothing");
            let shot = render_ui(&ui);
            assert!(drawn_on(&shot, h, y), "{a:?}: {dir:?} drawn");
            assert!(!drawn_on(&shot, other, y));

            // Onto the param: engaged, the hint gone.
            let here = (was * 127.0).round() as u8;
            send(&mut ui, &mut now, cc_msg(21, here));
            assert_eq!(ui.pickup(), None, "engaged");
            let shot = render_ui(&ui);
            assert!(!drawn_on(&shot, h, y) && !drawn_on(&shot, other, y));
        }
    }
}

/// THEME HINTS OFF: a pickup waits as before, but no hint is drawn.
#[test]
fn pickup_hint_absent_with_hints_off() {
    use chimera_core::ui::cc::PICKUP_UP;
    use chimera_core::ui::theme_settings::ThemeSettings;
    let (mut ui, a) = on_page(to_pitch);
    ui.set_theme(ThemeSettings {
        hints: false,
        ..ThemeSettings::DEFAULT
    });
    let mut m = MidiMap::EMPTY;
    let e = bind(Input::Cc(cc(21), CcKind::Absolute(Pickup)), a);
    m.learn(e.input, e.target).unwrap();
    m.set(0, e);
    ui.project_mut().edit_part(P1).sound.map = m;
    let mut now = 10_000;
    send(&mut ui, &mut now, cc_msg(21, 0));
    assert_eq!(ui.pickup(), Some(Dir::Up), "it still waits");
    assert!(!drawn_on(&render_ui(&ui), PICKUP_UP, hint::FOCUS_Y));
}

/// A panel turn takes the hint down: the direction it showed is stale
/// until the knob's next message (#820).
#[test]
fn a_panel_turn_clears_the_pickup_hint() {
    let (mut ui, a) = on_page(to_filter);
    let mut m = MidiMap::EMPTY;
    let e = bind(Input::Cc(cc(21), CcKind::Absolute(Pickup)), a);
    m.learn(e.input, e.target).unwrap();
    m.set(0, e);
    ui.project_mut().edit_part(P1).sound.map = m;
    let mut now = 10_000;
    send(&mut ui, &mut now, cc_msg(21, 0));
    assert_eq!(ui.pickup(), Some(Dir::Up));
    let ctx = SlotCtx::read(&ui.project().part(P1).sound.params, Op::A);
    let def = ui.page_def();
    let slot = (0..def.params.len())
        .find(|&i| view(def, i, &ctx).addr() == Some(a))
        .unwrap();
    turn(&mut ui, chimera_hal::ALL_ENCODERS[slot], 1);
    send(
        &mut ui,
        &mut now,
        Ctl::Pressure(chimera_midi::value::U7::ZERO),
    );
    assert_eq!(ui.pickup(), None, "turned: no stale hint");
}

/// Redraw what changed on `fb`, as the shells do each frame.
fn redraw(ui: &mut UiState, fb: &mut Fb) {
    ui.render_dirty_with_audio(
        fb,
        &chimera_core::ui::perf::PerfStats::zero(),
        Some(&screen::audio_fixture()),
        &screen::scope_fixture(),
    );
}

/// A Jump binding never waits: no hint.
#[test]
fn jump_never_hints() {
    let (mut ui, a) = on_page(to_pitch);
    let mut m = MidiMap::EMPTY;
    m.learn(Input::Cc(cc(21), CcKind::Absolute(Takeover::Jump)), at(a))
        .unwrap();
    ui.project_mut().edit_part(P1).sound.map = m;
    let mut now = 10_000;
    send(&mut ui, &mut now, cc_msg(21, 0));
    assert_eq!(ui.pickup(), None);
}

fn mix_seq(ui: &mut UiState) {
    feed(ui, Key::chord(Mix, Seq).at(10_000));
    feed(ui, Key::release(Seq).and_held(Mix).at(10_100));
    feed(ui, Key::release(Mix).at(10_133));
}

/// Once sensing is sure, MIX + SEQ  DONE shows: on the focus band's hint
/// line, and on a BigViz page after the guess on its status line.
#[test]
fn learn_done_drawn_once_sure() {
    for (to, y) in [
        (to_pitch as fn(&mut UiState), hint::FOCUS_Y),
        (to_filter, theme::BIGVIZ_STATUS_Y),
    ] {
        let (mut ui, _) = on_page(to);
        mix_seq(&mut ui);
        assert!(matches!(ui.learn(), Learn::Armed { .. }));
        let mut now = 20_000;
        send(&mut ui, &mut now, cc_msg(21, 50));
        assert!(matches!(ui.learn(), Learn::Sensing { .. }));
        assert!(!drawn_on(&render_ui(&ui), DONE, y), "not sure: no DONE");
        for v in [52, 54] {
            send(&mut ui, &mut now, cc_msg(21, v));
        }
        assert!(ui.learn_text().unwrap().2, "sure");
        assert!(drawn_on(&render_ui(&ui), DONE, y), "sure: DONE");
        tap(&mut ui, chimera_hal::ButtonId::Menu);
    }
}

/// A pickup on VA's STABILITY waits for a CC below the knob and hints UP (#838).
#[test]
fn va_stability_pickup() {
    use chimera_core::ui::block_registry::{VA_CHAIN, VA_HOME};
    let mut ui = Box::new(screen::stack_ui());
    screen::init_part_engine(&mut ui, P1, chimera_core::params::EngineType::Va);
    screen::plus(&mut ui, VA_CHAIN.engine_node());
    assert_eq!(ui.page_def().id, VA_HOME.id);
    let ctx = SlotCtx::read(&ui.project().part(P1).sound.params, Op::A);
    let a = view(ui.page_def(), 5, &ctx).addr().unwrap();
    let mut m = MidiMap::EMPTY;
    let e = bind(Input::Cc(cc(22), CcKind::Absolute(Pickup)), a);
    m.learn(e.input, e.target).unwrap();
    m.set(0, e);
    ui.project_mut().edit_part(P1).sound.map = m;
    let mut now = 20_000;
    let pass = |ui: &mut UiState, now: &mut u32, n: u32| {
        for _ in 0..n {
            *now += 16;
            ui.hear_ccs(core::iter::empty::<CtrlEvent>(), Ms(*now), true);
        }
    };
    pass(&mut ui, &mut now, 2);
    turn(&mut ui, F, 6);
    pass(&mut ui, &mut now, 2);
    let t = place(&ui, a);
    assert!(t > 0.1, "the knob sits above CC 10");
    send(&mut ui, &mut now, cc_msg(22, 10));
    pass(&mut ui, &mut now, 100);
    assert_eq!(place(&ui, a), t, "a pickup holds the param");
    assert_eq!(ui.pickup(), Some(Dir::Up));
}
