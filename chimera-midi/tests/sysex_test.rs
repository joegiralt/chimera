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

mod mts {
    use super::u7;
    use chimera_midi::stream::{Event, Parsed, Parser};
    use chimera_midi::sysex::mts;
    use proptest::prelude::*;

    /// Each key `bytes` retunes and its offset from 12-TET, semitones.
    fn heard(bytes: &[u8]) -> Option<Vec<(u8, f32)>> {
        let mut p = Parser::<408>::new();
        let mut out = None;
        for &b in bytes {
            if let Some(Parsed::Event(Event::SysEx(s))) = p.push(b) {
                out = mts(s, u7(0)).map(|m| {
                    let mut v = Vec::new();
                    m.each(|k, o| v.push((k.get(), o)));
                    v
                });
            }
        }
        out
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn single_note_real_time() {
        // Key 60 to 61 + 0x2000/16384 (61.5); key 62 left alone (7F 7F 7F).
        let v = heard(&[
            0xF0, 0x7F, 0x7F, 8, 2, 0, 2, 60, 61, 0x40, 0, 62, 0x7F, 0x7F, 0x7F, 0xF7,
        ])
        .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].0, 60);
        assert!(close(v[0].1, 1.5));
    }

    #[test]
    fn single_note_bank_form() {
        let v = heard(&[0xF0, 0x7E, 0x00, 8, 7, 0, 0, 1, 69, 68, 0, 0, 0xF7]).unwrap();
        assert_eq!(v, vec![(69, -1.0)]);
    }

    #[test]
    fn a_count_past_the_entries_is_refused() {
        assert_eq!(
            heard(&[0xF0, 0x7F, 0x7F, 8, 2, 0, 2, 60, 61, 0, 0, 0xF7]),
            None
        );
    }

    #[test]
    fn other_devices_and_messages_are_not_mts() {
        assert_eq!(
            heard(&[0xF0, 0x7F, 0x05, 8, 2, 0, 1, 60, 60, 0, 0, 0xF7]),
            None
        );
        assert_eq!(heard(&[0xF0, 0x7E, 0x7F, 6, 1, 0xF7]), None);
        assert_eq!(heard(&[0xF0, 0x7F, 0x7F, 8, 3, 0xF7]), None);
    }

    fn dump(tt: u8, f: impl Fn(u8) -> [u8; 3]) -> Vec<u8> {
        let mut b = vec![0x7E, 0x7F, 8, 1, tt];
        b.extend_from_slice(b"Bohlen-Pierce   ");
        for k in 0..128 {
            b.extend_from_slice(&f(k));
        }
        let cs = b.iter().fold(0, |a, x| a ^ x) & 0x7F;
        b.push(cs);
        let mut m = vec![0xF0];
        m.extend(b);
        m.push(0xF7);
        m
    }

    #[test]
    fn bulk_dump_retunes_every_key() {
        let m = dump(3, |k| [k, 0x20, 0]);
        let v = heard(&m).unwrap();
        assert_eq!(v.len(), 128);
        // Key 127 stays on the range.
        assert!(v[..127].iter().all(|&(_, o)| close(o, 0.25)));
        assert_eq!(v[127], (127, 0.0));
    }

    #[test]
    fn bulk_dump_with_a_bad_checksum_is_refused() {
        let mut m = dump(0, |k| [k, 0, 0]);
        let n = m.len();
        m[n - 2] ^= 1;
        assert_eq!(heard(&m), None);
    }

    #[test]
    fn scale_octave_one_byte() {
        // C -64 cents, E +63, the rest 0x40 (none).
        let mut m = vec![0xF0, 0x7E, 0x7F, 8, 8, 3, 0x7F, 0x7F];
        let mut ss = [0x40; 12];
        ss[0] = 0;
        ss[4] = 0x7F;
        m.extend_from_slice(&ss);
        m.push(0xF7);
        let v = heard(&m).unwrap();
        assert_eq!(v.len(), 128);
        assert!(close(v[60].1, -0.64));
        assert!(close(v[64].1, 0.63));
        assert!(close(v[61].1, 0.0));
    }

    #[test]
    fn scale_octave_two_byte() {
        let mut m = vec![0xF0, 0x7F, 0x7F, 8, 9, 3, 0x7F, 0x7F];
        for pc in 0..12 {
            // 0x2000 is 0; 0x3000 is +50 cents.
            m.extend_from_slice(if pc == 7 { &[0x60, 0] } else { &[0x40, 0] });
        }
        m.push(0xF7);
        let v = heard(&m).unwrap();
        assert!(close(v[67].1, 0.5));
        assert!(close(v[55].1, 0.5));
        assert!(close(v[60].1, 0.0));
    }

    proptest! {
        /// Any body after F0: never panics; what it retunes stays finite
        /// and on the MIDI range.
        #[test]
        fn any_bytes_are_safe(body in proptest::collection::vec(0u8..128, 0..420)) {
            let mut m = vec![0xF0];
            m.extend(body);
            m.push(0xF7);
            for (k, o) in heard(&m).unwrap_or_default() {
                prop_assert!(o.is_finite());
                prop_assert!((0.0..=127.0).contains(&(f32::from(k) + o)));
            }
        }
    }
}

// --- The parameter dump (#984) ---

use chimera_midi::sysex::{DumpWho, PartNo, dump_end, dump_request};
use chimera_midi::value::Channel;

fn dump_ask(bytes: &[u8]) -> Option<DumpWho> {
    let mut p = Parser::<16>::new();
    let mut out = None;
    for &b in bytes {
        if let Some(Parsed::Event(Event::SysEx(s))) = p.push(b) {
            out = dump_request(s.bytes());
        }
    }
    out
}

/// `F0 7D 43 48 01 <who> F7`: a Part by number, the Part on a channel,
/// or the one being edited.
#[test]
fn a_dump_request_names_a_part_a_channel_or_the_edited_part() {
    let ask = |who| dump_ask(&[0xF0, 0x7D, 0x43, 0x48, 0x01, who, 0xF7]);
    for n in 0..16 {
        assert_eq!(ask(n), Some(DumpWho::Part(PartNo::new(n).unwrap())));
        assert_eq!(ask(0x10 | n), Some(DumpWho::Channel(Channel::new(n).unwrap())));
    }
    assert_eq!(ask(0x7F), Some(DumpWho::Editing));
    for who in 0x20..0x7F {
        assert_eq!(ask(who), None, "{who:#x}");
    }
    // Every ask is its wire byte, and back.
    for who in 0..=0x7F {
        if let Some(w) = ask(who) {
            assert_eq!(w.wire().get(), who);
            assert_eq!(DumpWho::from_wire(w.wire()), Some(w));
        }
    }
    assert_eq!(PartNo::new(16), None);
}

#[test]
fn only_chimeras_own_request_is_one() {
    for b in [
        &[0xF0, 0x7D, 0x43, 0x48, 0x01, 0xF7][..],
        &[0xF0, 0x7D, 0x43, 0x48, 0x01, 0x00, 0x00, 0xF7],
        &[0xF0, 0x7D, 0x43, 0x48, 0x02, 0x00, 0xF7],
        &[0xF0, 0x7D, 0x48, 0x43, 0x01, 0x00, 0xF7],
        &[0xF0, 0x7E, 0x43, 0x48, 0x01, 0x00, 0xF7],
        &[0xF0, 0x41, 0x43, 0x48, 0x01, 0x00, 0xF7],
        &[0xF0, 0x7E, 0x7F, 6, 1, 0xF7],
    ] {
        assert_eq!(dump_ask(b), None, "{b:02X?}");
    }
}

/// The head is the identity reply's own maker and family: `7D`, "CH".
#[test]
fn the_dumps_end_names_the_part_that_answered() {
    let mut b = [0u8; 16];
    let mut w = Writer::new(&mut b);
    assert!(dump_end(PartNo::new(3).unwrap(), &mut w));
    let n = w.len();
    assert_eq!(&b[..n], [0xF0, 0x7D, 0x43, 0x48, 0x02, 0x03, 0xF7]);
    let mut r = [0u8; 16];
    let mut w = Writer::new(&mut r);
    assert!(identity_reply(u7(0), &ID, &mut w));
    assert_eq!(r[5..8], b[1..4], "maker and family as the reply gives them");
}
