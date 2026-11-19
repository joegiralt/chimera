//! Encoder feel (ADR 0279, #1005): lists and menus accelerate gentler than
//! a parameter, and SEQ + turn steps fine with no push.

mod common;
mod screen;

use chimera_core::project::PartId;
use chimera_core::ui::hold::{HoldGates, Press, SeqHold};
use chimera_core::ui::{UiState, feel, hint, theme};
use chimera_hal::{
    Accel, BurstPos, ButtonId, ButtonState, Controls, Detents, Edges, EncoderId, Ms, Turn,
    encoder_accel,
};

fn turn(d: i8, burst: u8) -> Turn {
    Turn::at(Detents::new(d), BurstPos::new(burst))
}

/// The panel's acceleration, as it was: 1, 2, 4, 5, 10, then 10.
#[test]
fn a_param_accelerates_as_before() {
    for burst in 0..20u8 {
        for detents in 1..=6i8 {
            let want: i32 = (0..detents as u8)
                .map(|k| i32::from(encoder_accel(burst.saturating_add(k))))
                .sum();
            let t = turn(detents, burst);
            assert_eq!(i32::from(Accel::Param.ticks(t).get()), want.min(127));
            let back = turn(-detents, burst);
            assert_eq!(Accel::Param.ticks(back).get(), -Accel::Param.ticks(t).get());
        }
    }
}

/// A list moves one row a detent for its first four, never more than
/// three a detent, and never further than a parameter would.
#[test]
fn a_list_accelerates_gentler() {
    for burst in 0..4u8 {
        assert_eq!(Accel::List.ticks(turn(1, burst)).get(), 1);
    }
    for burst in 0..40u8 {
        let t = turn(1, burst);
        assert!(Accel::List.ticks(t).get() <= 3);
        assert!(Accel::List.ticks(t).get() <= Accel::Param.ticks(t).get());
    }
    assert_eq!(Accel::List.ticks(turn(0, 9)).get(), 0);
}

/// A shell that does not accelerate (the desktop, a test's hand) reports
/// ticks as they are, under either curve.
#[test]
fn a_steady_turn_is_its_ticks() {
    for d in [-9i8, -1, 0, 1, 5, 12] {
        let t = Turn::steady(d);
        assert_eq!(
            (Accel::Param.ticks(t).get(), Accel::List.ticks(t).get()),
            (d, d)
        );
    }
}

/// −128 detents has no negation: it is never one, and its ticks are
/// −127's.
#[test]
fn detents_are_never_minus_128() {
    assert_eq!(Detents::new(i8::MIN).get(), -127);
    let t = Turn::at(Detents::new(i8::MIN), BurstPos::START);
    assert_eq!(Accel::Param.ticks(t).get(), -127);
    assert_eq!(Turn::steady(i8::MIN).detents().get(), -127);
}

/// The wake detent taken off: one fewer, the rest a place further into
/// the burst; a steady turn stays steady.
#[test]
fn consume_first_takes_one_detent() {
    assert_eq!(turn(3, 2).consume_first(), turn(2, 3));
    assert_eq!(turn(-1, 0).consume_first(), turn(0, 1));
    assert_eq!(Turn::steady(-2).consume_first(), Turn::steady(-1));
    assert_eq!(Turn::steady(0).consume_first(), Turn::steady(0));
}

/// A burst starts again after a rest, and when SEQ goes down or up: a
/// fine turn's release never hands the plain turn after it a burst it
/// counted through (review A6).
#[test]
fn a_seq_edge_restarts_the_burst() {
    let deep = BurstPos::new(9);
    let held = Edges {
        down: true,
        ..Edges::default()
    };
    assert_eq!(deep.next(false, Edges::default()), deep);
    assert_eq!(deep.next(false, held), deep);
    assert_eq!(deep.next(true, Edges::default()), BurstPos::START);
    let pressed = Edges {
        pressed_at: Some(Ms(4)),
        ..held
    };
    assert_eq!(deep.next(false, pressed), BurstPos::START);
    let released = Edges {
        released_at: Some(Ms(4)),
        ..Edges::default()
    };
    assert_eq!(deep.next(false, released), BurstPos::START);
}

/// SEQ's state, once a frame: up, held, or held through a turn on a cell
/// that steps fine; MIX's chord wins.
#[test]
fn seq_hold_is_one_state() {
    assert_eq!(SeqHold::of(false, false, true), SeqHold::Up);
    assert_eq!(SeqHold::of(true, false, false), SeqHold::Held);
    assert_eq!(SeqHold::of(true, false, true), SeqHold::Turned);
    assert_eq!(SeqHold::of(true, true, true), SeqHold::Held);
}

/// One frame: turns (detents at a burst position) and held buttons.
#[derive(Default)]
struct Frame {
    turns: Vec<(EncoderId, Turn)>,
    held: Vec<ButtonId>,
    now: u32,
}

impl Controls for Frame {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        Accel::Param.ticks(self.turn(id)).get()
    }
    fn turn(&self, id: EncoderId) -> Turn {
        self.turns
            .iter()
            .find(|t| t.0 == id)
            .map_or(Turn::default(), |t| t.1)
    }
    fn button_state(&self, id: ButtonId) -> ButtonState {
        if self.held.contains(&id) {
            ButtonState::Held
        } else {
            ButtonState::Up
        }
    }
    fn now_ms(&self) -> Ms {
        Ms(self.now)
    }
}

fn level(ui: &UiState) -> f32 {
    ui.project().part(PartId::ALL[0]).mix.level
}

/// Part 1's mixer PART page: D is LEVEL, 0.8, a step 1/128.
fn part_page() -> Box<UiState> {
    let mut ui = Box::new(UiState::new());
    screen::feed(&mut ui, screen::Input::chord(ButtonId::Mix, ButtonId::B1));
    screen::feed(&mut ui, screen::Input::press(ButtonId::Minus));
    ui
}

/// Deep in a fast burst, a plain turn accelerates; SEQ held, each detent
/// is one fine step (a tenth with pushes built in, else a whole one).
/// Bank 2's (`ui::feel`): without TAPE's UI its door is closed.
#[cfg(feature = "tape")]
#[test]
fn seq_and_a_turn_step_fine() {
    let fast = turn(-3, 8);
    let mut plain = part_page();
    plain.handle_input(&Frame {
        turns: vec![(EncoderId::D, fast)],
        ..Frame::default()
    });
    assert!(level(&plain) < 0.8 - 20.0 / 128.0, "{}", level(&plain));

    let mut ui = part_page();
    ui.handle_input(&Frame {
        turns: vec![(EncoderId::D, fast)],
        held: vec![ButtonId::Seq],
        ..Frame::default()
    });
    let step = if cfg!(feature = "push-encoders") {
        0.1 / 128.0
    } else {
        1.0 / 128.0
    };
    assert!(
        (level(&ui) - (0.8 - 3.0 * step)).abs() < 1e-6,
        "{}",
        level(&ui)
    );
}

/// SEQ's edges, by hand.
fn seq_edges(down: bool, pressed: Option<u32>, released: Option<u32>) -> Edges {
    Edges {
        down,
        pressed_at: pressed.map(Ms),
        released_at: released.map(Ms),
    }
}

/// SEQ held through a fine turn is no key: its release is no tap (no
/// move up) and holding on never opens ORBIT.
#[test]
fn a_turn_mutes_seq() {
    let mut g = HoldGates::new();
    // Pressed at 0, turned at 100, held to 900, let go.
    let frames = [
        (seq_edges(true, Some(0), None), SeqHold::Held, 0),
        (seq_edges(true, None, None), SeqHold::Turned, 100),
        (seq_edges(true, None, None), SeqHold::Held, 800),
        (seq_edges(false, None, Some(900)), SeqHold::Up, 900),
    ];
    for (e, seq, now) in frames {
        let p = g.step(&SeqFrame { seq: e, now }, seq);
        assert_eq!(p.seq, None::<Press>, "at {now} ms");
    }
}

/// SEQ's edges alone.
struct SeqFrame {
    seq: Edges,
    now: u32,
}

impl Controls for SeqFrame {
    fn encoder_delta(&self, _: EncoderId) -> i8 {
        0
    }
    fn button_state(&self, _: ButtonId) -> ButtonState {
        ButtonState::Up
    }
    fn edges(&self, id: ButtonId) -> Edges {
        if id == ButtonId::Seq {
            self.seq
        } else {
            Edges::default()
        }
    }
    fn now_ms(&self) -> Ms {
        Ms(self.now)
    }
}

/// A SEQ hold with no turn still opens ORBIT at 500 ms, as before.
#[test]
fn an_untouched_seq_hold_still_holds() {
    let mut g = HoldGates::new();
    let f = |seq, now| SeqFrame { seq, now };
    let held = SeqHold::Held;
    assert_eq!(
        g.step(&f(seq_edges(true, Some(0), None), 0), held).seq,
        None
    );
    assert_eq!(g.step(&f(seq_edges(true, None, None), 499), held).seq, None);
    assert_eq!(
        g.step(&f(seq_edges(true, None, None), 500), held).seq,
        Some(Press::Hold)
    );
}

/// A param cell on OUT's VOLUME in `fmt`.
fn cell(fmt: chimera_core::block::ValFmt) -> chimera_core::ui::view::View {
    use chimera_core::addr::{BlockRef, ParamAddr};
    use chimera_core::params::OutParams;
    chimera_core::ui::view::View::Param {
        addr: ParamAddr::new(BlockRef::Out, OutParams::VOLUME),
        label: "X",
        fmt,
        short: None,
    }
}

/// A pick from names turns as a list; a level as a parameter.
#[test]
fn a_choice_turns_as_a_list() {
    use chimera_core::block::ValFmt;
    use chimera_core::ui::view::View;
    assert_eq!(feel::accel(&cell(ValFmt::Uni)), Accel::Param);
    assert_eq!(feel::accel(&cell(ValFmt::Names(&["A", "B"]))), Accel::List);
    assert_eq!(feel::accel(&View::SelectPad), Accel::List);
}

/// A choice (a list turn, already a row a detent) takes no fine turn and
/// says none (review A5); a value does.
#[test]
fn a_choice_takes_no_fine_turn() {
    use chimera_core::block::ValFmt;
    use chimera_core::ui::view::{View, Words};
    assert!(feel::takes(&cell(ValFmt::Uni), Words::Plain));
    assert!(!feel::takes(
        &cell(ValFmt::Names(&["A", "B"])),
        Words::Plain
    ));
    assert!(!feel::takes(&View::SelectPad, Words::Pad));
}

/// Every cell that takes the fine turn says so, beside its own words, on
/// one line; a cell whose words leave no room takes none (review A3).
#[test]
fn every_fine_cell_hints_it() {
    use chimera_core::block::ValFmt;
    use chimera_core::ui::feel::Band;
    use chimera_core::ui::view::{self, Words};
    let words = [
        Words::Plain,
        Words::Learn,
        Words::Pad,
        Words::Own(view::ONSET_HINT),
        Words::Own(view::KEY_HINT),
        Words::Own(view::ROLL_HINT),
    ];
    let v = cell(ValFmt::Uni);
    for w in words {
        for band in [Band::Sound, Band::Mixer { mix: false }] {
            let h = feel::hint(&v, w, band);
            assert!(hint::width(h) <= theme::CONTENT_W, "{w:?}: {h:?}");
            let says = h.iter().any(|g| g.1 == "FINE");
            assert_eq!(says, feel::takes(&v, w), "{w:?} {band:?}: {h:?}");
        }
    }
    // The plain, the mapped and the pad's cells keep it.
    for w in [Words::Plain, Words::Learn, Words::Pad] {
        assert!(feel::takes(&v, w), "{w:?}");
    }
}

/// The focused cell's band hint says `keys`.
fn says(ui: &UiState, keys: &str) -> bool {
    ui.focus_hint_for_test().iter().any(|g| g.0 == keys)
}

/// A mixer value cell hints the fine turn at rest, before SEQ is down (a
/// pause on SEQ opens ORBIT at 500 ms); MIX held, the PART jump MIX
/// starts (review A4).
#[cfg(feature = "tape")]
#[test]
fn a_mixer_cell_hints_fine_at_rest_and_the_part_jump_under_mix() {
    let mut ui = part_page();
    ui.set_midi(chimera_core::ui::midi_settings::MidiSettings::DEFAULT.with_push(false));
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, 1));
    assert!(says(&ui, "SEQ+TURN"), "{:?}", ui.focus_hint_for_test());
    screen::feed(&mut ui, screen::Input::held(ButtonId::Mix));
    assert!(says(&ui, "MIX + B1-6"), "{:?}", ui.focus_hint_for_test());
    screen::feed(&mut ui, screen::Input::release(ButtonId::Mix));
    assert!(says(&ui, "SEQ+TURN"));
}

/// A DRUM pad cell takes the fine turn and says it beside MIX + -/+ PAD
/// (review A3).
#[cfg(feature = "tape")]
#[test]
fn a_pad_cell_hints_fine_beside_the_pad() {
    let mut ui = Box::new(UiState::new());
    ui.set_midi(chimera_core::ui::midi_settings::MidiSettings::DEFAULT.with_push(false));
    screen::load_init(&mut ui, chimera_core::params::EngineType::Drum);
    screen::feed(&mut ui, screen::Input::press(ButtonId::Plus)); // MCH
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, 1));
    let h = ui.focus_hint_for_test();
    assert!(says(&ui, "MIX + -/+") && says(&ui, "SEQ+TURN"), "{h:?}");
}

/// SEQ pressed, `e` turned, SEQ let go.
fn seq_turn_release(ui: &mut UiState, e: EncoderId) {
    screen::feed(ui, screen::Input::press(ButtonId::Seq).at(0));
    screen::feed(ui, screen::Input::held(ButtonId::Seq).and_turn(e, 1).at(50));
    screen::feed(ui, screen::Input::release(ButtonId::Seq).at(100));
}

/// Where the fine turn does not apply (SETTINGS' RANDOMIZE), a turn
/// during SEQ's tap leaves it a tap (review A8).
#[test]
fn a_plain_turn_leaves_seq_its_tap() {
    let at_randomize = || {
        let mut ui = Box::new(UiState::new());
        screen::to_leaf(&mut ui, &["PART", "RANDOMIZE"]);
        ui
    };
    let mut tapped = at_randomize();
    screen::tap(&mut tapped, ButtonId::Seq);
    let mut turned = at_randomize();
    seq_turn_release(&mut turned, EncoderId::E);
    assert_ne!(tapped.location(), at_randomize().location(), "SEQ moves");
    assert_eq!(turned.location(), tapped.location());
}

/// On a value cell the turn is fine and SEQ's release is no tap.
#[cfg(feature = "tape")]
#[test]
fn a_fine_turn_takes_seqs_tap() {
    let mut ui = part_page();
    let at = ui.location();
    seq_turn_release(&mut ui, EncoderId::D);
    assert_eq!(ui.location(), at);
}
