use chimera_midi::out::Writer;
use chimera_midi::stream::{Event, Parsed, Parser};
use chimera_midi::sysex::{Identity, Maker, Request, identity_reply, request};
use chimera_midi::value::{U7, U14};

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

fn ask(bytes: &[u8], own: u8) -> Option<Request> {
    let mut p = Parser::<16>::new();
    let mut out = None;
    for &b in bytes {
        if let Some(Parsed::Event(Event::SysEx(s))) = p.push(b) {
            out = request(s, u7(own));
        }
    }
    out
}

#[test]
fn identity_request_7f_and_own() {
    assert_eq!(
        ask(&[0xF0, 0x7E, 0x7F, 6, 1, 0xF7], 0),
        Some(Request::Identity)
    );
    assert_eq!(
        ask(&[0xF0, 0x7E, 0x00, 6, 1, 0xF7], 0),
        Some(Request::Identity)
    );
    assert_eq!(
        ask(&[0xF0, 0x7E, 0x05, 6, 1, 0xF7], 5),
        Some(Request::Identity)
    );
}

#[test]
fn other_device_ignored() {
    assert_eq!(ask(&[0xF0, 0x7E, 0x01, 6, 1, 0xF7], 0), None);
    assert_eq!(ask(&[0xF0, 0x7D, 0x7F, 6, 1, 0xF7], 0), None);
    assert_eq!(ask(&[0xF0, 0x7E, 0x7F, 6, 2, 0xF7], 0), None);
    assert_eq!(ask(&[0xF0, 0x7E, 0x7F, 6, 1, 0, 0xF7], 0), None);
    assert_eq!(ask(&[0xF0, 0xF7], 0), None);
    // Short bodies.
    assert_eq!(ask(&[0xF0, 0x7E, 0x7F, 6, 0xF7], 0), None);
    assert_eq!(ask(&[0xF0, 0x7E, 0xF7], 0), None);
}

const ID: Identity = Identity {
    maker: Maker::NonCommercial,
    family: U14::from_parts(U7::new(0x48).unwrap(), U7::new(0x43).unwrap()),
    member: U14::from_parts(U7::new(0).unwrap(), U7::new(1).unwrap()),
    version: [U7::ZERO, U7::new(3).unwrap(), U7::ZERO, U7::ZERO],
};

#[test]
fn identity_reply_golden_7d() {
    let mut b = [0u8; 32];
    let mut w = Writer::new(&mut b);
    assert!(identity_reply(u7(0), &ID, &mut w));
    assert_eq!(w.len(), 15);
    assert_eq!(
        b[..15],
        [
            0xF0, 0x7E, 0, 6, 2, 0x7D, 0x43, 0x48, 1, 0, 0, 3, 0, 0, 0xF7
        ]
    );
}

#[test]
fn reply_is_all_or_nothing_and_maker_widths() {
    let mut b = [0u8; 14];
    let mut w = Writer::new(&mut b);
    assert!(!identity_reply(u7(0), &ID, &mut w));
    assert_eq!(w.len(), 0);
    let mut b = [0u8; 32];
    let mut w = Writer::new(&mut b);
    let id = Identity {
        maker: Maker::Three(u7(0x20), u7(0x33)),
        ..ID
    };
    assert!(identity_reply(u7(0), &id, &mut w));
    assert_eq!(w.len(), 17);
    assert_eq!(b[5..8], [0, 0x20, 0x33]);
    let mut w = Writer::new(&mut b);
    let id = Identity {
        maker: Maker::One(u7(0x41)),
        ..ID
    };
    assert!(identity_reply(u7(0), &id, &mut w));
    assert_eq!(b[5], 0x41);
}
