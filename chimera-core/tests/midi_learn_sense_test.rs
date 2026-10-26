//! Learn senses the kind (spec §§ 6.3, 8.1): the first control heard while
//! armed writes the entry, and learn stays open, sensing what the control
//! sends. Each sample feeds `classify`, and the entry's input and echo are
//! rewritten from the guess; nothing sensed is ever applied. MIX+SEQ, a nav
//! key, a page change or a panel turn ends it with the guess.

#![cfg(feature = "midi-map")]

mod screen;

use chimera_core::MidiChannel;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::filter::FilterKind;
use chimera_core::midi_in::CtrlEvent;
use chimera_core::midi_map::{
    Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, Mappable, MidiMap, NrpnKind,
};
use chimera_core::params::FilterParams;
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry::FILTER;
use chimera_core::ui::learn::Learn;
use chimera_core::ui::nav::PartChain;
use chimera_hal::ButtonId::{B2, Mix, Plus, Seq};
use chimera_hal::EncoderId::F;
use chimera_hal::Ms;
use chimera_midi::ctl::{Ctl, DataValue, Dir, FreeCc, Sel};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover;
use chimera_midi::value::{U7, U14};
use screen::{Input as Key, feed, tap};

const P1: PartId = PartId::ALL[0];
const CUTOFF: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
const CH1: MidiChannel = match MidiChannel::new(0) {
    Some(c) => c,
    None => panic!(),
};
const ASK: &str = "TURN IT RIGHT, THEN LEFT";

/// Part 1 on INIT Algo with the 2040, on FLT, CUTOFF focused, the map
/// empty, learn armed.
fn armed() -> Rig {
    let mut ui = Box::new(screen::stack_ui());
    let was = PartChain::of(&ui.project().part(P1).sound);
    screen::init_part(&mut ui, P1);
    let sound = ui.project_mut().edit_part(P1).sound;
    sound.params.filter.set_kind(FilterKind::K2040);
    sound.map = MidiMap::EMPTY;
    ui.replaced_for_test(was);
    for _ in 0..12 {
        if ui.page_def().id == FILTER.id {
            break;
        }
        feed(&mut ui, Key::press(Plus));
    }
    assert_eq!(ui.page_def().id, FILTER.id);
    feed(&mut ui, Key::turn(F, 1));
    mix_seq(&mut ui);
    assert!(matches!(ui.learn(), Learn::Armed { .. }));
    Rig { ui, now: 20_000 }
}

fn mix_seq(ui: &mut UiState) {
    feed(ui, Key::chord(Mix, Seq).at(10_000));
    feed(ui, Key::release(Seq).and_held(Mix).at(10_100));
    feed(ui, Key::release(Mix).at(10_133));
}

struct Rig {
    ui: Box<UiState>,
    now: u32,
}

impl Rig {
    /// One message on Part 1's channel, a pass of its own.
    fn send(&mut self, c: Ctl) {
        self.now += 4;
        let ev = CtrlEvent::Ctl(CH1, c);
        self.ui.hear_ccs(core::iter::once(ev), Ms(self.now), true);
    }

    fn cc(&mut self, n: u8, v: u8) {
        self.send(Ctl::Cc {
            cc: FreeCc::new(n).unwrap(),
            value: U7::new(v).unwrap(),
        });
    }

    fn ccs(&mut self, n: u8, vs: &[u8]) {
        for &v in vs {
            self.cc(n, v);
        }
    }

    /// Passes with nothing heard: a ramp, had one started, would land.
    fn idle(&mut self, passes: u32) {
        for _ in 0..passes {
            self.now += 4;
            self.ui
                .hear_ccs(core::iter::empty::<CtrlEvent>(), Ms(self.now), true);
        }
    }

    fn entry(&self) -> Binding {
        let map = self.ui.project().part(P1).sound.map;
        assert_eq!(map.len(), 1, "{map:?}");
        map.entries()[0]
    }

    fn input(&self) -> Input {
        self.entry().input
    }

    fn cutoff(&self) -> f32 {
        let p = self.ui.project().read_part(P1);
        p.block(CUTOFF.block).unwrap().get(CUTOFF.param)
    }

    /// The label line while sensing.
    fn band(&self) -> String {
        self.ui
            .learn_text()
            .map_or_else(String::new, |(top, ..)| top.as_str().into())
    }

    /// The left turn asked for or the guess (the band's words as drawn, in
    /// the value's place), then the hint line's `MIX + SEQ  DONE`.
    fn hint(&self) -> String {
        let Some((_, words, sure)) = self.ui.learn_text() else {
            return String::new();
        };
        let drawn = self.ui.status_text().unwrap();
        assert_eq!(drawn.as_str(), words.as_str());
        match sure {
            false => words.as_str().into(),
            true => format!("{}  MIX + SEQ  DONE", words.as_str()),
        }
    }
}

fn cc_in(n: u8, k: CcKind) -> Input {
    Input::Cc(LearnCc::new(n).unwrap(), k)
}

fn nrpn(msb: u8, lsb: u8) -> U14 {
    U14::from_parts(U7::new(msb).unwrap(), U7::new(lsb).unwrap())
}

const JUMP: CcKind = CcKind::Absolute(Takeover::Jump);

#[test]
fn first_message_writes_entry_not_applied() {
    let mut r = armed();
    let was = r.cutoff();
    r.cc(21, 100);
    let e = r.entry();
    assert_eq!(e.input, cc_in(21, JUMP));
    assert_eq!(e.target, Mappable::new(CUTOFF).unwrap());
    assert_eq!(e.feedback, Feedback::Off);
    assert_eq!(r.ui.learn(), Learn::Sensing { part: P1, entry: 0 });
    assert_eq!(r.band(), "CC 21 → CUTOFF");
    r.idle(60);
    assert_eq!(r.cutoff(), was, "the first value never applied");
}

#[test]
fn sensed_values_never_applied() {
    let mut r = armed();
    let was = r.cutoff();
    let sweep: Vec<u8> = (0..55).map(|i| 10 + i * 2).collect();
    r.ccs(21, &sweep);
    r.idle(60);
    assert_eq!(r.cutoff(), was);
    assert!(matches!(r.ui.learn(), Learn::Sensing { .. }));
    // Relative bytes too: no detent turned.
    let mut r = armed();
    r.ccs(21, &[1, 1, 1, 1, 0x7F, 0x7F, 0x7F]);
    r.idle(10);
    assert_eq!(r.cutoff(), was);
}

#[test]
fn offset_stream_learns_offset() {
    let mut r = armed();
    r.ccs(21, &[0x41, 0x41, 0x41, 0x3F, 0x3F, 0x3F]);
    assert_eq!(
        r.input(),
        cc_in(21, CcKind::Relative(ByteEncoding::BinaryOffset))
    );
}

#[test]
fn twos_learns_twos() {
    let mut r = armed();
    r.ccs(21, &[1, 1, 2, 0x7F, 0x7F, 0x7E]);
    assert_eq!(
        r.input(),
        cc_in(21, CcKind::Relative(ByteEncoding::TwosComplement))
    );
}

#[test]
fn sign_down_learns_signed() {
    let mut r = armed();
    r.ccs(21, &[1, 1, 1, 0x41, 0x41, 0x41]);
    assert_eq!(
        r.input(),
        cc_in(21, CcKind::Relative(ByteEncoding::SignBitDown))
    );
}

#[test]
fn pot_learns_jump() {
    let mut r = armed();
    r.ccs(21, &[50, 52, 53, 55, 58, 60, 57, 54, 51]);
    assert_eq!(r.input(), cc_in(21, JUMP));
    assert_eq!(r.entry().feedback, Feedback::Off);
}

#[test]
fn button_learns_momentary() {
    let mut r = armed();
    r.ccs(21, &[127, 0, 127, 0]);
    assert_eq!(r.input(), cc_in(21, CcKind::Button(ButtonMode::Momentary)));
}

#[test]
fn cc14_pair_learns_cc14() {
    let mut r = armed();
    for (msb, lsb) in [(10, 0), (10, 64), (11, 0), (11, 64), (12, 0)] {
        r.cc(2, msb);
        r.cc(34, lsb);
    }
    assert_eq!(
        r.input(),
        Input::Cc14(Cc14Msb::new(2).unwrap(), Takeover::Jump),
        "CC 34 is the pair's LSB half, no restart"
    );
    assert_eq!(r.band(), "14-BIT CC 2 → CUTOFF");
}

#[test]
fn nrpn_inc_dec_learns_incdec() {
    let mut r = armed();
    let n = nrpn(10, 3);
    for _ in 0..3 {
        r.send(Ctl::Step {
            sel: Sel::Nrpn(n),
            dir: Dir::Up,
        });
    }
    assert_eq!(r.input(), Input::Nrpn(n, NrpnKind::IncDec));
    assert_eq!(r.band(), "NRPN 10:3 → CUTOFF");
}

#[test]
fn relative_and_14_bit_learn_echo_on() {
    let mut r = armed();
    r.ccs(21, &[1, 1, 0x7F, 0x7F]);
    assert_eq!(r.entry().feedback, Feedback::Value, "relative");

    let mut r = armed();
    for (msb, lsb) in [(10, 0), (10, 64), (11, 0)] {
        r.cc(2, msb);
        r.cc(34, lsb);
    }
    assert_eq!(r.entry().feedback, Feedback::Value, "14-bit CC");

    let mut r = armed();
    let n = nrpn(10, 3);
    r.send(Ctl::Step {
        sel: Sel::Nrpn(n),
        dir: Dir::Down,
    });
    assert_eq!(r.entry().feedback, Feedback::Value, "inc/dec");

    // An NRPN that sends a fine value is 14-bit.
    let mut r = armed();
    for v in [100u16, 300, 600] {
        let v = U14::new(v).unwrap();
        r.send(Ctl::Data {
            sel: Sel::Nrpn(n),
            value: DataValue::Coarse(v.msb()),
        });
        r.send(Ctl::Data {
            sel: Sel::Nrpn(n),
            value: DataValue::Fine(v),
        });
    }
    let e = r.entry();
    assert_eq!(e.input, Input::Nrpn(n, NrpnKind::Absolute(Takeover::Jump)));
    assert_eq!(e.feedback, Feedback::Value, "NRPN with fine");
}

#[test]
fn absolute_and_button_learn_echo_off() {
    let mut r = armed();
    r.ccs(21, &[50, 52, 54, 56]);
    assert_eq!(r.entry().feedback, Feedback::Off, "absolute");

    let mut r = armed();
    r.ccs(21, &[127, 0, 127]);
    assert_eq!(r.entry().feedback, Feedback::Off, "button");

    let mut r = armed();
    let n = nrpn(10, 3);
    for v in [20, 22, 25] {
        r.send(Ctl::Data {
            sel: Sel::Nrpn(n),
            value: DataValue::Coarse(U7::new(v).unwrap()),
        });
    }
    let e = r.entry();
    assert_eq!(e.input, Input::Nrpn(n, NrpnKind::Absolute(Takeover::Jump)));
    assert_eq!(e.feedback, Feedback::Off, "NRPN coarse only");

    let mut r = armed();
    for v in [10, 30, 50] {
        r.send(Ctl::Pressure(U7::new(v).unwrap()));
    }
    let e = r.entry();
    assert_eq!(e.input, Input::Pressure(Takeover::Jump));
    assert_eq!(e.feedback, Feedback::Off, "pressure");
    assert_eq!(r.band(), "PRESSURE → CUTOFF");
}

#[test]
fn another_control_restarts() {
    let mut r = armed();
    r.ccs(21, &[1, 1, 0x7F]);
    assert_eq!(
        r.input(),
        cc_in(21, CcKind::Relative(ByteEncoding::TwosComplement))
    );
    // Another CC: sensing starts over on it, its samples alone.
    r.ccs(22, &[60, 62, 64]);
    assert_eq!(r.input(), cc_in(22, JUMP));
    assert_eq!(r.entry().feedback, Feedback::Off);
    assert_eq!(r.band(), "CC 22 → CUTOFF");
    assert!(matches!(r.ui.learn(), Learn::Sensing { .. }));
    // A message on another channel is not learn's: no restart.
    r.ui.hear_ccs(
        core::iter::once(CtrlEvent::Ctl(
            MidiChannel::new(5).unwrap(),
            Ctl::Cc {
                cc: FreeCc::new(23).unwrap(),
                value: U7::new(1).unwrap(),
            },
        )),
        Ms(r.now + 4),
        true,
    );
    assert_eq!(r.input(), cc_in(22, JUMP));
    // Back to 21: a fresh start, the old relative samples gone.
    r.ccs(21, &[40, 42]);
    assert_eq!(r.input(), cc_in(21, JUMP));
}

#[test]
fn nav_ends_with_guess() {
    let ends: [fn(&mut UiState); 3] = [|ui| tap(ui, B2), mix_seq, |ui| feed(ui, Key::turn(F, 1))];
    for (i, end) in ends.into_iter().enumerate() {
        let mut r = armed();
        r.ccs(21, &[1, 1, 0x7F, 0x7F]);
        let want = cc_in(21, CcKind::Relative(ByteEncoding::TwosComplement));
        assert_eq!(r.input(), want, "{i}");
        end(&mut r.ui);
        assert_eq!(r.ui.learn(), Learn::Off, "{i}");
        assert_eq!(r.input(), want, "{i}: the guess stands");
        assert_eq!(r.entry().feedback, Feedback::Value, "{i}");
        // Now the control is heard as mapped: a detent turns CUTOFF (down:
        // INIT's sits at the top).
        let was = r.cutoff();
        r.cc(21, 0x7F);
        r.idle(4);
        assert!(r.cutoff() < was, "{i}: applied after the end");
    }
}

#[test]
fn hint_asks_for_left_until_sure() {
    let mut r = armed();
    assert_eq!(r.hint(), "", "armed: no sensing hint yet");
    r.cc(21, 1);
    assert_eq!(r.hint(), ASK);
    r.ccs(21, &[1, 1, 1]);
    assert_eq!(r.hint(), ASK, "one direction only: not sure");
    r.cc(21, 0x7F);
    assert_eq!(r.hint(), "RELATIVE · TWOS COMPLEMENT  MIX + SEQ  DONE");
    assert_eq!(r.band(), "CC 21 → CUTOFF");

    let mut r = armed();
    r.cc(21, 0x41);
    r.ccs(21, &[0x41, 0x41]);
    assert_eq!(r.hint(), ASK);
    r.ccs(21, &[0x3F, 0x3F]);
    assert_eq!(r.hint(), "RELATIVE · BINARY OFFSET  MIX + SEQ  DONE");

    // A pot is sure once it has turned a little.
    let mut r = armed();
    r.cc(21, 50);
    assert_eq!(r.hint(), ASK);
    r.ccs(21, &[52, 54]);
    assert_eq!(r.hint(), "ABSOLUTE  MIX + SEQ  DONE");

    let mut r = armed();
    r.ccs(21, &[127, 0, 127]);
    assert_eq!(r.hint(), "BUTTON  MIX + SEQ  DONE");

    let mut r = armed();
    for (msb, lsb) in [(10, 0), (10, 64)] {
        r.cc(2, msb);
        r.cc(34, lsb);
    }
    assert_eq!(r.hint(), "14-BIT CC  MIX + SEQ  DONE");
}

/// Numbers print whole, one to three digits.
#[test]
fn label_line_prints_numbers() {
    let mut r = armed();
    r.cc(119, 5);
    assert_eq!(r.band(), "CC 119 → CUTOFF");
    let mut r = armed();
    r.send(Ctl::Step {
        sel: Sel::Nrpn(nrpn(100, 0)),
        dir: Dir::Up,
    });
    assert_eq!(r.band(), "NRPN 100:0 → CUTOFF");
}
