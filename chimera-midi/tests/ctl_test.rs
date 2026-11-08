mod common;
use chimera_midi::ctl::*;
use chimera_midi::stream::*;
use chimera_midi::value::*;
use common::*;

fn u14(n: u16) -> U14 {
    U14::new(n).unwrap()
}
fn heard(bytes: &[u8]) -> Vec<Heard> {
    hear_bytes(&mut Controllers::new(), bytes)
        .into_iter()
        .map(|(_, h)| h)
        .collect()
}
fn last(bytes: &[u8]) -> Heard {
    *heard(bytes).last().unwrap()
}
fn data(sel: Sel, value: DataValue) -> Heard {
    Heard::Ctl(Ctl::Data { sel, value })
}

fn nrpn(msb: u8, lsb: u8) -> Sel {
    Sel::Nrpn(U14::from_parts(u7(msb), u7(lsb)))
}
fn rpn(msb: u8, lsb: u8) -> Sel {
    Sel::Rpn(U14::from_parts(u7(msb), u7(lsb)))
}

#[test]
fn nrpn_then_rpn_last_wins() {
    assert_eq!(
        last(&[0xB0, 99, 1, 98, 2, 101, 0, 100, 0, 6, 9]),
        data(rpn(0, 0), DataValue::Coarse(u7(9)))
    );
    assert_eq!(
        last(&[0xB0, 101, 0, 100, 0, 99, 1, 98, 2, 6, 9]),
        data(nrpn(1, 2), DataValue::Coarse(u7(9)))
    );
}

#[test]
fn rpn_null_makes_data_inert() {
    assert_eq!(
        last(&[0xB0, 99, 1, 98, 2, 101, 127, 100, 127, 6, 9]),
        Heard::Consumed
    );
}

#[test]
fn nrpn_null_too() {
    assert_eq!(
        last(&[0xB0, 101, 0, 100, 0, 99, 127, 98, 127, 6, 9]),
        Heard::Consumed
    );
}

#[test]
fn cc6_alone_emits_coarse() {
    assert_eq!(
        last(&[0xB0, 99, 1, 98, 2, 6, 64]),
        data(nrpn(1, 2), DataValue::Coarse(u7(64)))
    );
}

#[test]
fn cc38_refines_with_held_msb() {
    assert_eq!(
        last(&[0xB0, 99, 1, 98, 2, 6, 3, 38, 5]),
        data(nrpn(1, 2), DataValue::Fine(u14(3 << 7 | 5)))
    );
}

#[test]
fn cc38_without_msb_uses_zero() {
    assert_eq!(
        last(&[0xB0, 99, 1, 98, 2, 38, 5]),
        data(nrpn(1, 2), DataValue::Fine(u14(5)))
    );
}

#[test]
fn new_selection_clears_msb() {
    assert_eq!(
        last(&[0xB0, 99, 1, 98, 2, 6, 3, 98, 4, 38, 5]),
        data(nrpn(1, 4), DataValue::Fine(u14(5)))
    );
}

#[test]
fn inc_dec_steps_once_ignoring_data() {
    let h = heard(&[0xB0, 99, 1, 98, 2, 96, 0, 97, 127, 96, 55]);
    let s = |dir| {
        Heard::Ctl(Ctl::Step {
            sel: nrpn(1, 2),
            dir,
        })
    };
    assert_eq!(h[2..], [s(Dir::Up), s(Dir::Down), s(Dir::Up)][..]);
}

#[test]
fn inc_dec_on_rpn0() {
    let h = heard(&[0xB0, 101, 0, 100, 0, 96, 1, 97, 1]);
    assert_eq!(
        h[2],
        Heard::Ctl(Ctl::Step {
            sel: rpn(0, 0),
            dir: Dir::Up
        })
    );
    assert_eq!(
        h[3],
        Heard::Ctl(Ctl::Step {
            sel: rpn(0, 0),
            dir: Dir::Down
        })
    );
}

#[test]
fn nothing_selected_is_inert() {
    assert_eq!(
        heard(&[0xB0, 6, 9, 38, 9, 96, 0, 97, 0]),
        vec![Heard::Consumed; 4]
    );
}

#[test]
fn selection_survives_reset_all_controllers() {
    // Layer 1 turns CC 121 into a Mode event, which layer 2 never sees.
    let mut c = Controllers::new();
    let mut p = Parser::<32>::new();
    let (mut got, mut reset) = (None, false);
    for b in [0xB0, 99, 1, 98, 2, 121, 0, 6, 9] {
        match p.push(b) {
            Some(Parsed::Event(Event::Voice(ch, v))) => got = Some(c.hear(ch, v)),
            Some(Parsed::Event(Event::Mode(_, ChannelMode::ResetAllControllers))) => reset = true,
            _ => {}
        }
    }
    assert!(reset, "layer 1 saw the reset");
    assert_eq!(got, Some(data(nrpn(1, 2), DataValue::Coarse(u7(9)))));
}

fn prog(bank: Option<u16>, p: u8) -> Heard {
    Heard::Ctl(Ctl::Program {
        bank: bank.map(u14),
        program: u7(p),
    })
}

#[test]
fn bank_latch_msb_only() {
    assert_eq!(last(&[0xB0, 0, 2, 0xC0, 5]), prog(Some(2 << 7), 5));
}
#[test]
fn bank_latch_lsb_only() {
    assert_eq!(last(&[0xB0, 32, 3, 0xC0, 5]), prog(Some(3), 5));
}
#[test]
fn bank_latch_both() {
    assert_eq!(
        last(&[0xB0, 0, 2, 32, 3, 0xC0, 5]),
        prog(Some(2 << 7 | 3), 5)
    );
}
#[test]
fn bank_latch_cleared_by_program() {
    assert_eq!(last(&[0xB0, 0, 2, 0xC0, 5, 0xC0, 6]), prog(None, 6));
    assert_eq!(last(&[0xC0, 6]), prog(None, 6));
}

#[test]
fn cc64_is_note_path() {
    assert_eq!(last(&[0xB0, 64, 127]), Heard::Note);
    assert_eq!(last(&[0x90, 60, 100]), Heard::Note);
    assert_eq!(last(&[0xE0, 0, 64]), Heard::Note);
    assert_eq!(last(&[0xA0, 60, 9]), Heard::Note);
}

#[test]
fn pressure_and_free_cc() {
    assert_eq!(last(&[0xD0, 9]), Heard::Ctl(Ctl::Pressure(u7(9))));
    assert_eq!(
        last(&[0xB0, 74, 9]),
        Heard::Ctl(Ctl::Cc {
            cc: FreeCc::new(74).unwrap(),
            value: u7(9)
        })
    );
}

#[test]
fn channels_are_isolated() {
    let mut c = Controllers::new();
    let h = hear_bytes(&mut c, &[0xB0, 99, 1, 98, 2, 0xB1, 6, 9, 0xB0, 6, 9]);
    assert_eq!(h[2], (1, Heard::Consumed));
    assert_eq!(h[3], (0, data(nrpn(1, 2), DataValue::Coarse(u7(9)))));
}

#[test]
fn running_status_nrpn_stream() {
    let h = heard(&[0xB0, 0x63, 0x0A, 0x62, 0x03, 0x06, 0x40, 0x26, 0x00]);
    let sel = nrpn(0x0A, 3);
    assert_eq!(h[2], data(sel, DataValue::Coarse(u7(0x40))));
    assert_eq!(h[3], data(sel, DataValue::Fine(u14(0x40 << 7))));
}

#[test]
fn daw_thinned_repeat_98_keeps_msb() {
    assert_eq!(
        last(&[0xB0, 99, 5, 98, 1, 98, 2, 6, 9]),
        data(nrpn(5, 2), DataValue::Coarse(u7(9)))
    );
}

#[test]
fn free_cc_refuses_the_rest() {
    for n in 0..=255u8 {
        let refused = matches!(n, 0 | 6 | 32 | 38 | 64 | 96..=101) || n >= 120;
        assert_eq!(FreeCc::new(n).is_none(), refused, "{n}");
    }
}
