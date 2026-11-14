use std::collections::HashMap;

use chimera_image::slot::*;

const SLOT_LEN: u32 = 0x4_0000;

/// Flash words by address, erased unless set; every read is logged.
#[derive(Default)]
struct SimSlots {
    words: HashMap<u32, Word>,
    reads: Vec<u32>,
}

impl SimSlots {
    fn set(&mut self, addr: u32, bytes: [u8; 32]) {
        self.words.insert(addr, Word { bytes, ecc: false });
    }
    fn ecc(&mut self, addr: u32) {
        self.words.insert(
            addr,
            Word {
                bytes: [0x5A; 32],
                ecc: true,
            },
        );
    }
}

impl SlotRead for SimSlots {
    fn word(&mut self, addr: u32) -> Word {
        assert_eq!(addr % 32, 0);
        self.reads.push(addr);
        self.words.get(&addr).copied().unwrap_or(Word {
            bytes: [0xFF; 32],
            ecc: false,
        })
    }
}

fn own_words(s: Slot) -> [(u32, [u8; 32]); 3] {
    [
        (s.base(), code_word(s)),
        (s.base() + 0x20, tag_word(s)),
        (s.marker_addr(), end_marker(s)),
    ]
}

fn in_slot(s: Slot, addr: u32) -> bool {
    (s.base()..s.base() + SLOT_LEN).contains(&addr)
}

#[test]
fn erased_slots_pass_and_are_planned_erased() {
    let mut r = SimSlots::default();
    assert_eq!(
        guard(&mut r, SlotSet::NONE),
        Ok(SlotPlan {
            erased: SlotSet::ALL
        })
    );
    // Both sectors of both slots, every word.
    assert_eq!(r.reads.len() as u32, 2 * (2 + SLOT_LEN / 32));
}

#[test]
fn a_clean_tag_is_ours_and_nothing_else_is_read() {
    for s in Slot::ALL {
        let mut r = SimSlots::default();
        r.set(s.base() + 0x20, tag_word(s));
        let p = guard(&mut r, SlotSet::NONE).unwrap();
        assert!(!p.erased.has(s));
        let reads: Vec<u32> = r.reads.iter().copied().filter(|&a| in_slot(s, a)).collect();
        assert_eq!(reads, [s.base() + 0x20], "{s:?}");
    }
}

#[test]
fn a_clean_marker_is_ours() {
    for s in Slot::ALL {
        let mut r = SimSlots::default();
        r.set(s.marker_addr(), end_marker(s));
        r.set(s.base() + 0x1000, [0; 32]); // never scanned
        let p = guard(&mut r, SlotSet::NONE).unwrap();
        assert!(!p.erased.has(s));
        let reads: Vec<u32> = r.reads.iter().copied().filter(|&a| in_slot(s, a)).collect();
        assert_eq!(reads, [s.base() + 0x20, s.marker_addr(), s.base()], "{s:?}");
    }
}

#[test]
fn a_clean_marker_with_foreign_code_in_s4_is_refused() {
    for s in Slot::ALL {
        let mut v = [0xFF; 32];
        v[..4].copy_from_slice(&0x2002_0000u32.to_le_bytes());
        v[4..8].copy_from_slice(&0x1FF0_9800u32.to_le_bytes());
        let mut r = SimSlots::default();
        r.set(s.marker_addr(), end_marker(s));
        r.set(s.base(), v);
        r.set(s.base() + 0x40, [0; 32]);
        assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(s)));
    }
}

#[test]
fn a_clean_marker_with_our_code_torn_or_erased_is_ours() {
    let s = Slot::Mixer;
    let mut r = SimSlots::default();
    r.set(s.marker_addr(), end_marker(s));
    r.ecc(s.base());
    assert!(guard(&mut r, SlotSet::NONE).is_ok());
}

#[test]
fn an_ecc_tag_with_its_text_falls_to_the_scan() {
    let s = Slot::Mixer;
    let ecc_tag = Word {
        bytes: tag_word(s),
        ecc: true,
    };
    let mut r = SimSlots::default();
    r.words.insert(s.base() + 0x20, ecc_tag);
    let p = guard(&mut r, SlotSet::NONE).unwrap();
    assert!(!p.erased.has(s));
    assert!(p.erased.has(Slot::Edit));

    let mut r = SimSlots::default();
    r.words.insert(s.base() + 0x20, ecc_tag);
    r.set(s.marker_addr() - 0x1_0000, [0; 32]);
    assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(s)));
}

#[test]
fn an_ecc_marker_with_s4_erased_is_ours() {
    let s = Slot::Edit;
    let mut r = SimSlots::default();
    r.ecc(s.marker_addr());
    let p = guard(&mut r, SlotSet::NONE).unwrap();
    assert!(!p.erased.has(s));
}

#[test]
fn tag_version_is_ignored_offset_is_not() {
    let s = Slot::Mixer;
    let mut t = tag_word(s);
    t[0x1C] = 7;
    let mut r = SimSlots::default();
    r.set(s.base() + 0x20, t);
    r.set(s.base() + 0x1000, [0; 32]);
    assert!(guard(&mut r, SlotSet::NONE).is_ok());

    let mut r = SimSlots::default();
    r.set(s.base() + 0x40, tag_word(s));
    assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(Slot::Mixer)));
}

#[test]
fn torn_own_words_in_an_erased_slot_are_ours() {
    let mut seed = 0x1234_5678u32;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        seed >> 8
    };
    for s in Slot::ALL {
        for (addr, want) in own_words(s) {
            let mut r = SimSlots::default();
            r.ecc(addr);
            let p = guard(&mut r, SlotSet::NONE)
                .unwrap_or_else(|e| panic!("{s:?} {addr:#x} ecc: {e:?}"));
            assert!(!p.erased.has(s));

            for _ in 0..64 {
                let mut w = want;
                // A torn program only clears bits: restore some of the 0s.
                for _ in 0..1 + rnd() % 40 {
                    let bit = rnd() % 256;
                    w[bit as usize / 8] |= 1 << (bit % 8);
                }
                let mut r = SimSlots::default();
                r.set(addr, w);
                assert!(
                    guard(&mut r, SlotSet::NONE).is_ok(),
                    "{s:?} {addr:#x} {w:02x?}"
                );
            }
        }
        // All three torn at once.
        let mut r = SimSlots::default();
        for (addr, _) in own_words(s) {
            r.ecc(addr);
        }
        assert!(guard(&mut r, SlotSet::NONE).is_ok());
    }
}

#[test]
fn foreign_trampoline_is_refused() {
    let s = Slot::Mixer;
    let mut w = [0xFF; 32];
    w[..4].copy_from_slice(&0x2002_0000u32.to_le_bytes());
    w[4..8].copy_from_slice(&0x1FF0_9800u32.to_le_bytes());
    let mut r = SimSlots::default();
    r.set(s.base(), w);
    assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(Slot::Mixer)));
}

#[test]
fn one_foreign_word_anywhere_else_is_refused() {
    let s = Slot::Mixer;
    let mut r = SimSlots::default();
    let mut w = [0xFF; 32];
    w[13] = 0xFE;
    r.set(s.base() + SLOT_LEN * 3 / 4, w);
    assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(Slot::Mixer)));

    let s = Slot::Edit;
    let mut r = SimSlots::default();
    r.set(s.base() + 0x60, [0; 32]);
    assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(Slot::Edit)));
}

#[test]
fn an_ecc_word_outside_the_three_is_refused() {
    let s = Slot::Mixer;
    let mut r = SimSlots::default();
    r.ecc(s.base() + 0x400);
    assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(Slot::Mixer)));
    // A flagged word that reads 0xFF is still not erased.
    let mut r = SimSlots::default();
    r.words.insert(
        s.base() + 0x400,
        Word {
            bytes: [0xFF; 32],
            ecc: true,
        },
    );
    assert_eq!(guard(&mut r, SlotSet::NONE), Err(SlotForeign(Slot::Mixer)));
}

#[test]
fn ours_mark_skips_the_slot() {
    let mut r = SimSlots::default();
    r.set(Slot::Edit.base() + 0x80, [0; 32]); // foreign, but marked ours
    let p = guard(&mut r, SlotSet::NONE.with(Slot::Edit)).unwrap();
    assert!(p.erased.has(Slot::Mixer));
    assert!(!p.erased.has(Slot::Edit));
    assert!(r.reads.iter().all(|&a| !in_slot(Slot::Edit, a)));
}
