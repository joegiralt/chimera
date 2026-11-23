// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_image::Bank2Sector::*;
use chimera_image::slot::*;

fn u32_at(w: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(w[o..o + 4].try_into().unwrap())
}

/// The 64-byte stub as flash holds it: code word, then tag word.
fn stub(s: Slot) -> [u8; 64] {
    let mut b = [0u8; 64];
    b[..32].copy_from_slice(&code_word(s));
    b[32..].copy_from_slice(&tag_word(s));
    b
}

#[test]
fn code_word_vectors() {
    for s in Slot::ALL {
        let w = code_word(s);
        assert_eq!(u32_at(&w, 0), 0x2002_0000);
        assert_eq!(u32_at(&w, 4), s.base() + 9);
    }
}

#[test]
fn code_word_decodes_to_the_restart() {
    let want: [u16; 12] = [
        0x4805, // ldr r0, [pc, #20]
        0x4906, // ldr r1, [pc, #24]
        0x6008, // str r0, [r1]
        0xF3BF, 0x8F4F, // dsb sy
        0xF3BF, 0x8F6F, // isb sy
        0x6801, // ldr r1, [r0]
        0x6842, // ldr r2, [r0, #4]
        0xF381, 0x8808, // msr msp, r1
        0x4710, // bx r2
    ];
    for s in Slot::ALL {
        let b = stub(s);
        let hw: Vec<u16> = b[8..32]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(hw, want, "{s:?}");
        // LDR (literal) T1: Align(PC, 4) + imm8 * 4, PC = instr + 4.
        let lit = |at: usize| {
            let h = hw[(at - 8) / 2];
            assert_eq!(h & 0xF800, 0x4800);
            (((at + 4) & !3) + (h as usize & 0xFF) * 4, (h >> 8) & 7)
        };
        assert_eq!(lit(0x08), (0x20, 0), "r0 <- base+0x20");
        assert_eq!(lit(0x0A), (0x24, 1), "r1 <- base+0x24");
        assert_eq!(u32_at(&b, 0x20), 0x0802_0000, "bank 1's vectors");
        assert_eq!(u32_at(&b, 0x24), 0xE000_ED08, "SCB->VTOR");
        // The reset vector lands on the first instruction, Thumb bit set.
        assert_eq!(u32_at(&b, 4) - s.base(), 0x08 | 1);
    }
}

#[test]
fn tag_word_layout() {
    for s in Slot::ALL {
        let b = stub(s);
        assert_eq!(u32_at(&b, 0x20), 0x0802_0000);
        assert_eq!(u32_at(&b, 0x24), 0xE000_ED08);
        assert_eq!(b[0x28..0x30], [0xFF; 8]);
        assert_eq!(&b[0x30..0x3C], TAG);
        assert_eq!(b[0x3C], 1);
        assert_eq!(b[0x3D], s.index());
        assert_eq!(b[0x3E..0x40], [0xFF; 2]);
    }
}

#[test]
fn end_marker_layout() {
    for s in Slot::ALL {
        let w = end_marker(s);
        assert_eq!(&w[..16], MARKER);
        assert_eq!(w[16], 1);
        assert_eq!(w[17], s.index());
        assert_eq!(w[18..], [0xFF; 14]);
    }
}

#[test]
fn slot_addresses() {
    use Slot::*;
    assert_eq!((Mixer.index(), Edit.index()), (0, 1));
    assert_eq!(Mixer.base(), 0x0818_0000);
    assert_eq!(Mixer.marker_addr(), 0x081B_FFE0);
    assert_eq!(Mixer.sectors(), (S4, S5));
    assert_eq!(Edit.base(), 0x081C_0000);
    assert_eq!(Edit.marker_addr(), 0x081F_FFE0);
    assert_eq!(Edit.sectors(), (S6, S7));
    for s in Slot::ALL {
        let (entry, end) = s.sectors();
        assert_eq!(entry.addr(), s.base());
        assert_eq!(end.addr() + 0x2_0000 - 32, s.marker_addr());
    }
    assert_eq!(RESERVE, 1024);
}

#[test]
fn slot_sets() {
    use Slot::*;
    assert_eq!(SlotSet::NONE.bits(), 0);
    assert_eq!(SlotSet::ALL.bits(), 0b11);
    assert_eq!(SlotSet::NONE.with(Mixer).with(Edit), SlotSet::ALL);
    let e = SlotSet::NONE.with(Edit);
    assert!(e.has(Edit) && !e.has(Mixer));
    assert_eq!(e.bits(), 0b10);
    assert!(
        Slot::ALL
            .iter()
            .all(|&s| SlotSet::ALL.has(s) && !SlotSet::NONE.has(s))
    );
}
