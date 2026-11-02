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

/// Each channel message's packet: CIN is the status's high nibble.
#[test]
fn channel_packets_carry_their_cin() {
    let cases: [(&[u8], [u8; 4]); 7] = [
        (&[0x81, 60, 0], [0x08, 0x81, 60, 0]),
        (&[0x92, 60, 100], [0x09, 0x92, 60, 100]),
        (&[0xA3, 60, 50], [0x0A, 0xA3, 60, 50]),
        (&[0xB4, 7, 99], [0x0B, 0xB4, 7, 99]),
        (&[0xC5, 5], [0x0C, 0xC5, 5, 0]),
        (&[0xD6, 40], [0x0D, 0xD6, 40, 0]),
        (&[0xEF, 1, 64], [0x0E, 0xEF, 1, 64]),
    ];
    for (m, p) in cases {
        assert_eq!(pack(m), [p], "{m:02X?}");
    }
}

/// Running status: every packet still carries the full status and its CIN.
#[test]
fn running_status_packets_carry_the_status() {
    assert_eq!(
        pack(&[0x90, 0x3C, 0x64, 0x3E, 0x64]),
        [[0x09, 0x90, 0x3C, 0x64], [0x09, 0x90, 0x3E, 0x64]]
    );
    assert_eq!(
        pack(&[0xC1, 5, 6, 7]),
        [[0x0C, 0xC1, 5, 0], [0x0C, 0xC1, 6, 0], [0x0C, 0xC1, 7, 0]]
    );
}

/// Two NRPNs as the echo writes them: the second wholly under running
/// status.
#[test]
fn nrpn_under_running_status() {
    let bytes = [
        0xB0, 0x63, 0x01, 0x62, 0x02, 0x06, 0x40, 0x26, 0x00, // first
        0x63, 0x01, 0x62, 0x03, 0x06, 0x10, 0x26, 0x7F, // second
    ];
    assert_eq!(
        pack(&bytes),
        [
            [0x0B, 0xB0, 0x63, 0x01],
            [0x0B, 0xB0, 0x62, 0x02],
            [0x0B, 0xB0, 0x06, 0x40],
            [0x0B, 0xB0, 0x26, 0x00],
            [0x0B, 0xB0, 0x63, 0x01],
            [0x0B, 0xB0, 0x62, 0x03],
            [0x0B, 0xB0, 0x06, 0x10],
            [0x0B, 0xB0, 0x26, 0x7F],
        ]
    );
}

/// A status inside a SysEx ends it: the unsent part is dropped, the new
/// message packs whole, and a late F7 is stray.
#[test]
fn a_status_aborts_sysex() {
    assert_eq!(
        pack(&[0xF0, 0x7E, 0x7F, 0x06, 0x01, 0x90, 0x3C, 0x64, 0xF7]),
        [[0x04, 0xF0, 0x7E, 0x7F], [0x09, 0x90, 0x3C, 0x64]]
    );
}
