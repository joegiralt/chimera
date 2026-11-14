//! The board's bank-2 driver names bank 2's own registers only. It lives
//! here, not in chimera-board (a thumb crate with no host tests), and reads
//! the driver's source as text.

const SRC: &str = include_str!("../../chimera-board/src/bank2.rs");

/// The first address past bank 2's S7.
const BANK2_END: u64 = 0x0820_0000;
/// Peripheral and system registers start here.
const REGS: u64 = 0x4000_0000;

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

#[test]
fn no_literal_past_bank_2() {
    for (lit, v) in hex_literals(SRC) {
        assert!(
            !(BANK2_END..REGS).contains(&v),
            "chimera-board/src/bank2.rs has {lit}, past bank 2"
        );
    }
}

/// From `at` to the end of its fn: the first `}` at an indent of 0 or 4.
fn rest_of_fn(src: &str, at: usize) -> &str {
    let end = ["\n}\n", "\n    }\n"]
        .iter()
        .filter_map(|c| src[at..].find(c))
        .min()
        .map_or(src.len(), |e| at + e);
    &src[at..end]
}

/// Each masked read (review B2, N3): `cpsid f` is followed in its own fn by
/// BFHFNMIGN cleared, then `cpsie f`, and only then SR2's ECC flags read
/// and cleared, so a pended fault still sees DBECCERR.
#[test]
fn masked_reads_clear_bfhfnmign() {
    let starts: Vec<usize> = SRC.match_indices("\"cpsid f\"").map(|(i, _)| i).collect();
    assert!(
        !starts.is_empty(),
        "no masked read in chimera-board/src/bank2.rs"
    );
    for at in starts {
        let body = rest_of_fn(SRC, at);
        let pos = |what: &str| {
            body.find(what)
                .unwrap_or_else(|| panic!("masked read at {at} lacks {what}"))
        };
        let set = pos("| CCR_BFHFNMIGN");
        let clear = pos("& !CCR_BFHFNMIGN");
        let on = pos("\"cpsie f\"");
        let sr2 = pos("clear_ecc(");
        assert!(set < clear, "BFHFNMIGN cleared before it is set");
        assert!(clear < on, "faults unmasked before BFHFNMIGN is cleared");
        assert!(on < sr2, "SR2 cleared before `cpsie f`");
        assert!(
            body[..on].matches("\"cpsid f\"").count() == 1,
            "one `cpsid f` per masked read"
        );
    }
}
