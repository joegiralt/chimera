//! check-slots on synthetic 1 MB images.

#[allow(dead_code)]
#[path = "../src/bank2.rs"]
mod bank2;

use chimera_image::slot::{code_word, end_marker, tag_word};
use chimera_image::{BANK2_BASE, BANK2_LEN, Slot, UNSEALED_HEADER};

fn good() -> Vec<u8> {
    let mut b = vec![0xFFu8; BANK2_LEN as usize];
    b[..UNSEALED_HEADER.len()].copy_from_slice(&UNSEALED_HEADER);
    for s in Slot::ALL {
        for (addr, w) in [
            (s.base(), code_word(s)),
            (s.tag_addr(), tag_word(s)),
            (s.marker_addr(), end_marker(s)),
        ] {
            let o = (addr - BANK2_BASE) as usize;
            b[o..o + 32].copy_from_slice(&w);
        }
    }
    b
}

#[test]
fn a_good_image_passes() {
    let b = good();
    for (s, o) in [(Slot::Mixer, 0x80000), (Slot::Edit, 0xC0000)] {
        assert_eq!((s.base() - BANK2_BASE) as usize, o);
        assert_eq!(b[o..o + 32], code_word(s));
        assert_eq!(b[o + 0x20..o + 0x40], tag_word(s));
        assert_eq!(b[o + 0x3FFE0..o + 0x40000], end_marker(s));
    }
    assert_eq!(bank2::check_slots(&b), Ok(()));
}

#[test]
fn wrong_length_fails() {
    let b = good();
    let e = bank2::check_slots(&b[..b.len() - 32]).unwrap_err();
    assert!(e.contains("1048576"), "{e}");
}

#[test]
fn a_zero_in_a_reserve_fails() {
    let mut b = good();
    b[0x80100] = 0;
    let e = bank2::check_slots(&b).unwrap_err();
    assert!(e.contains("0x08180100"), "{e}");
}

#[test]
fn a_moved_marker_fails() {
    let mut b = good();
    let m = end_marker(Slot::Mixer);
    b[0xBFFE0..0xC0000].fill(0xFF);
    b[0xBFFC0..0xBFFE0].copy_from_slice(&m);
    assert!(bank2::check_slots(&b).is_err());
}

#[test]
fn a_wrong_stub_byte_fails() {
    let mut b = good();
    b[0x80000 + 9] ^= 1;
    assert!(bank2::check_slots(&b).is_err());
}

#[test]
fn every_word_of_every_slot_is_checked() {
    for s in Slot::ALL {
        let base = (s.base() - BANK2_BASE) as usize;
        for (off, what) in [(0, "stub"), (0x20, "tag"), (0x3FFE0, "end marker")] {
            let mut b = good();
            b[base + off] ^= 1;
            let e = bank2::check_slots(&b).unwrap_err();
            assert!(
                e.contains(&format!("{:#010x}", s.base() + off as u32)),
                "{e}"
            );
            assert!(e.contains(what) && e.contains(&format!("{s:?}")), "{e}");
        }
    }
}

#[test]
fn the_reserve_is_base_0x40_to_0x400() {
    for s in Slot::ALL {
        let base = (s.base() - BANK2_BASE) as usize;
        for off in [0x40, 0x3FF] {
            let mut b = good();
            b[base + off] = 0;
            let e = bank2::check_slots(&b).unwrap_err();
            assert!(
                e.contains(&format!("{:#010x}", s.base() + off as u32)),
                "{e}"
            );
        }
        let mut b = good();
        b[base + 0x400] = 0x12;
        assert_eq!(bank2::check_slots(&b), Ok(()));
    }
}

#[test]
fn too_long_fails() {
    let mut b = good();
    b.extend([0xFF; 32]);
    assert!(bank2::check_slots(&b).unwrap_err().contains("1048576"));
}
