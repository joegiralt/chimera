use chimera_midi::ctl::{DataValue, FreeCc};
use chimera_midi::out::{Cc14Msb, Running, Writer, din_us};
use chimera_midi::value::{Channel, U7, U14};

fn ch(c: u8) -> Channel {
    Channel::new(c).unwrap()
}
fn cc(n: u8) -> FreeCc {
    FreeCc::new(n).unwrap()
}
fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

#[test]
fn cc_with_running_status_is_two_bytes_after_first() {
    let mut b = [0u8; 16];
    let mut w = Writer::new(&mut b);
    assert!(w.cc(ch(0), cc(74), u7(1)));
    assert_eq!(w.len(), 3);
    assert!(w.cc(ch(0), cc(74), u7(2)));
    assert_eq!(w.len(), 5);
    assert!(w.cc(ch(1), cc(74), u7(3)));
    assert_eq!(w.len(), 8);
    w.clear_status();
    assert!(w.cc(ch(1), cc(74), u7(4)));
    assert_eq!(w.len(), 11);
    assert_eq!(b[..11], [0xB0, 74, 1, 74, 2, 0xB1, 74, 3, 0xB1, 74, 4]);
}

#[test]
fn cc14_is_msb_then_lsb() {
    let mut b = [0u8; 16];
    let mut w = Writer::new(&mut b);
    assert!(w.cc14(ch(2), Cc14Msb::new(1).unwrap(), U14::new(0x1234).unwrap()));
    assert_eq!(w.len(), 5);
    assert_eq!(b[..5], [0xB2, 1, 0x24, 33, 0x34]);
}

#[test]
fn cc14_msb_range() {
    assert!(Cc14Msb::new(0).is_none());
    assert!(Cc14Msb::new(6).is_none());
    assert!(Cc14Msb::new(32).is_none());
    assert!(Cc14Msb::new(1).is_some() && Cc14Msb::new(31).is_some());
}

#[test]
fn nrpn_bytes_golden() {
    let mut b = [0u8; 32];
    let mut w = Writer::new(&mut b);
    let fine = DataValue::Fine(U14::from_parts(u7(0x40), u7(0)));
    assert!(w.nrpn(ch(0), U14::from_parts(u7(0x0A), u7(3)), fine));
    assert_eq!(w.len(), 9);
    assert!(w.nrpn(ch(0), U14::from_parts(u7(0x0A), u7(4)), fine));
    assert_eq!(w.len(), 9 + 8);
    assert_eq!(
        b[..17],
        [
            0xB0, 0x63, 0x0A, 0x62, 0x03, 0x06, 0x40, 0x26, 0x00, 0x63, 0x0A, 0x62, 0x04, 0x06,
            0x40, 0x26, 0x00
        ]
    );
}

#[test]
fn nrpn_coarse_has_no_lsb() {
    let mut b = [0u8; 16];
    let mut w = Writer::new(&mut b);
    assert!(w.nrpn(ch(0), U14::new(0).unwrap(), DataValue::Coarse(u7(5))));
    assert_eq!(w.len(), 7);
    assert_eq!(b[..7], [0xB0, 0x63, 0, 0x62, 0, 0x06, 5]);
}

#[test]
fn writer_never_splits_a_message() {
    for cap in 0..=9 {
        let mut b = [0xEEu8; 9];
        let mut w = Writer::new(&mut b[..cap]);
        let ok = w.nrpn(
            ch(0),
            U14::new(0).unwrap(),
            DataValue::Fine(U14::new(0).unwrap()),
        );
        assert_eq!(ok, cap >= 9);
        assert_eq!(w.len(), if ok { 9 } else { 0 });
        assert!(b[cap..].iter().all(|&x| x == 0xEE));
        if !ok {
            assert!(b[..cap].iter().all(|&x| x == 0xEE));
        }
    }
    let mut b = [0u8; 4];
    let mut w = Writer::new(&mut b);
    assert!(w.cc(ch(0), cc(1), u7(1)));
    assert!(!w.cc(ch(1), cc(1), u7(1)));
    assert_eq!(w.len(), 3);
    assert!(!w.sysex(&[0x7E]));
    assert_eq!(w.len(), 3);
}

#[test]
fn sysex_cancels_running_status_and_rejects_high_bytes() {
    let mut b = [0u8; 16];
    let mut w = Writer::new(&mut b);
    assert!(w.cc(ch(0), cc(1), u7(1)));
    assert!(!w.sysex(&[0x80]));
    assert!(w.sysex(&[0x7D]));
    assert!(w.cc(ch(0), cc(1), u7(2)));
    assert_eq!(b[..9], [0xB0, 1, 1, 0xF0, 0x7D, 0xF7, 0xB0, 1, 2]);
}

#[test]
fn din_us_cc_is_960() {
    assert_eq!(din_us(3), 960);
    assert_eq!(din_us(2), 640);
}

#[test]
fn a_writer_after_another_keeps_its_running_status() {
    let mut a = [0u8; 8];
    let mut w = Writer::new(&mut a);
    assert!(w.cc(ch(2), cc(20), u7(5)));
    let r = w.running();
    let mut b = [0u8; 8];
    let mut w = Writer::after(&mut b, r);
    assert!(w.cc(ch(2), cc(21), u7(6)));
    assert_eq!(&b[..2], &[21, 6], "the status left out");
    let mut c = [0u8; 8];
    let mut w = Writer::after(&mut c, r);
    assert!(w.cc(ch(3), cc(21), u7(6)));
    assert_eq!(&c[..3], &[0xB3, 21, 6], "another channel sends its own");
    let mut d = [0u8; 8];
    let mut w = Writer::after(&mut d, Running::NONE);
    assert!(w.cc(ch(2), cc(21), u7(6)));
    assert_eq!(w.len(), 3);
}
