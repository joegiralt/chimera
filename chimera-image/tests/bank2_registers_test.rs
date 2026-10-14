//! The board's bank-2 driver names bank 2's own registers only. It lives
//! here, not in chimera-board (a thumb crate with no host tests), and reads
//! the driver's source as text.

const SRC: &str = include_str!("../../chimera-board/src/bank2.rs");

/// The first address past S3: the bootloader's sectors 4-7 start here.
const S4: u64 = 0x0818_0000;

fn ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `word` in `src` with no identifier character either side.
fn has_word(src: &str, word: &str) -> bool {
    let b = src.as_bytes();
    src.match_indices(word).any(|(i, _)| {
        let before = i.checked_sub(1).is_none_or(|j| !ident(b[j]));
        let after = b.get(i + word.len()).is_none_or(|&c| !ident(c));
        before && after
    })
}

/// Every hex literal's value.
fn hex_literals(src: &str) -> Vec<(String, u64)> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    for (i, _) in src.match_indices("0x") {
        if i > 0 && ident(b[i - 1]) {
            continue;
        }
        let raw: String = src[i + 2..]
            .chars()
            .take_while(|c| c.is_ascii_hexdigit() || *c == '_')
            .collect();
        if let Ok(v) = u64::from_str_radix(&raw.replace('_', ""), 16) {
            out.push((format!("0x{raw}"), v));
        }
    }
    out
}

#[test]
fn only_bank2_registers() {
    let src = SRC.to_ascii_lowercase();
    for no in [
        "bank1()",
        "keyr1",
        "cr1",
        "sr1",
        "ccr1",
        "optkeyr",
        "optcr",
        "optsr",
        "optccr",
        "0x0800_0000",
        "0x08000000",
        "ber",
        "mer",
        "bank1",
    ] {
        assert!(!has_word(&src, no), "chimera-board/src/bank2.rs names {no}");
    }
    for no in ["_prg", ".ber(", ".mer(", "0x5200_200", "0x5200200"] {
        assert!(!src.contains(no), "chimera-board/src/bank2.rs names {no}");
    }
    for (lit, v) in hex_literals(&src) {
        assert!(v < S4, "chimera-board/src/bank2.rs has {lit}, past S3");
    }
    // Not vacuous: the driver is here and goes through bank 2's block.
    for yes in [
        "bank2()",
        "struct flashbank2",
        "keyr.write",
        "unsafe impl flash for flashbank2",
    ] {
        assert!(src.contains(yes), "chimera-board/src/bank2.rs lacks {yes}");
    }
}

#[test]
fn the_scan_catches_what_it_forbids() {
    assert!(has_word("b.ber().set_bit()", "ber"));
    assert!(has_word("(*flash).bank1().cr", "bank1()"));
    assert!(!has_word("number", "ber"));
    assert!(!has_word("bank 1", "bank1"));
    assert_eq!(
        hex_literals("x = 0x0818_0000; y = 0xb2; z = a0x1"),
        [
            ("0x0818_0000".to_string(), 0x0818_0000),
            ("0xb2".to_string(), 0xb2)
        ]
    );
}
