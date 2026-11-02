use chimera_midi::usb::{Packer, unpack};

fn pack(bytes: &[u8]) -> Vec<[u8; 4]> {
    let mut p = Packer::new();
    bytes.iter().filter_map(|&b| p.push(b)).collect()
}

fn unpacked(pkts: &[[u8; 4]]) -> Vec<u8> {
    pkts.iter()
        .filter_map(|&p| unpack(p))
        .flat_map(|(_, b)| b.as_slice().to_vec())
        .collect()
}

#[test]
fn note_on_unpacks() {
    let (c, b) = unpack([0x09, 0x90, 60, 100]).unwrap();
    assert_eq!((c, b.as_slice()), (0, &[0x90, 60, 100][..]));
}

#[test]
fn cable_is_returned() {
    let (c, b) = unpack([0x19, 0x90, 60, 100]).unwrap();
    assert_eq!((c, b.as_slice()), (1, &[0x90, 60, 100][..]));
}

#[test]
fn reserved_cins_drop() {
    assert!(unpack([0x00, 1, 2, 3]).is_none());
    assert!(unpack([0x01, 1, 2, 3]).is_none());
}

#[test]
fn sysex_spans_packets() {
    let id = [0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7];
    let pkts = pack(&id);
    assert_eq!(pkts, [[0x04, 0xF0, 0x7E, 0x7F], [0x07, 0x06, 0x01, 0xF7]]);
    assert_eq!(unpacked(&pkts), id);
}

#[test]
fn realtime_inside_sysex() {
    let mut p = Packer::new();
    assert_eq!(p.push(0xF0), None);
    assert_eq!(p.push(0x01), None);
    assert_eq!(p.push(0xF8), Some([0x0F, 0xF8, 0, 0]));
    assert_eq!(p.push(0x02), Some([0x04, 0xF0, 0x01, 0x02]));
    assert_eq!(p.push(0xF7), Some([0x05, 0xF7, 0, 0]));
}

#[test]
fn round_trip_every_type() {
    let mut msgs: Vec<Vec<u8>> = vec![
        vec![0x80, 60, 0],
        vec![0x90, 60, 100],
        vec![0xA0, 60, 50],
        vec![0xB0, 7, 99],
        vec![0xC0, 5],
        vec![0xD0, 40],
        vec![0xE0, 1, 64],
        vec![0xF1, 0x23],
        vec![0xF2, 1, 2],
        vec![0xF3, 9],
        vec![0xF6],
    ];
    msgs.extend((0xF8..=0xFF).map(|b| vec![b]));
    for n in 0..=5u8 {
        let mut s = vec![0xF0];
        s.extend((0..n).map(|i| i + 1));
        s.push(0xF7);
        msgs.push(s);
    }
    for m in msgs {
        assert_eq!(unpacked(&pack(&m)), m, "{m:02X?}");
    }
}
