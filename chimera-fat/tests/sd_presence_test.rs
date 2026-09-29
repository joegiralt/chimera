use chimera_fat::sd::{CMD0, NCR_BYTES, r1_within_ncr};

#[test]
fn cmd0_is_go_idle_with_its_crc() {
    assert_eq!(CMD0, [0x40, 0, 0, 0, 0, 0x95]);
    assert_eq!(NCR_BYTES, 8);
}

/// An empty slot: MISO is pulled up, every byte reads 0xFF.
#[test]
fn all_ones_is_no_answer() {
    assert_eq!(r1_within_ncr(&[0xFF; NCR_BYTES]), None);
    assert_eq!(r1_within_ncr(&[]), None);
}

/// The first byte with bit 7 clear is R1, at any position within NCR.
#[test]
fn r1_is_the_first_byte_with_bit_7_clear() {
    for at in 0..NCR_BYTES {
        let mut b = [0xFF; NCR_BYTES];
        b[at] = 0x01;
        assert_eq!(r1_within_ncr(&b), Some(0x01), "R1 at byte {at}");
    }
    assert_eq!(r1_within_ncr(&[0xFF, 0x05, 0x01]), Some(0x05));
}

/// A busy card holds DO low: that is a card, not an empty slot.
#[test]
fn busy_low_is_an_answer() {
    assert_eq!(r1_within_ncr(&[0x00; NCR_BYTES]), Some(0x00));
}

/// Bus noise with bit 7 set is not R1.
#[test]
fn high_bytes_are_not_r1() {
    assert_eq!(r1_within_ncr(&[0xFE, 0x80, 0xC1, 0xFF]), None);
}

/// Only NCR's bytes count: an answer past them is too late.
#[test]
fn answer_past_ncr_is_none() {
    let mut b = [0xFF; NCR_BYTES + 1];
    b[NCR_BYTES] = 0x01;
    assert_eq!(r1_within_ncr(&b), None);
}
