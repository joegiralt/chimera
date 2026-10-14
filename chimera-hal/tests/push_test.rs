use chimera_hal::{DEBOUNCE_TICKS, Debounce, NUM_ENCODERS, PUSH_BITS, push_levels};

/// All released: every push bit high, as is the top byte.
const NONE_PRESSED: u32 = 0xFF00_0000;

#[test]
fn each_push_is_its_own_bit() {
    assert_eq!(push_levels(NONE_PRESSED), [false; NUM_ENCODERS]);
    for (i, bit) in PUSH_BITS.iter().enumerate() {
        let mut want = [false; NUM_ENCODERS];
        want[i] = true;
        assert_eq!(push_levels(NONE_PRESSED & !bit), want, "encoder {i}");
    }
}

#[test]
fn the_pushes_are_the_top_byte_and_distinct() {
    let all = PUSH_BITS.iter().fold(0, |a, b| {
        assert_eq!(a & b, 0, "a bit used twice");
        a | b
    });
    assert_eq!(
        all & 0x00FF_FFFF,
        0,
        "pushes clash with the 24 buttons and encoders"
    );
    // Bit 25 and 24 are the fourth register's spare inputs.
    assert_eq!(all, 0xFC00_0000);
}

#[test]
fn a_board_without_pushes_reads_none_pressed() {
    // No fourth register: the top byte shifts in zeros.
    assert_eq!(push_levels(0x00FF_FFFF), [false; NUM_ENCODERS]);
    assert_eq!(push_levels(0), [false; NUM_ENCODERS]);
    // A spare input high is a fourth register: five pushes held.
    assert_eq!(
        push_levels(1 << 24 | PUSH_BITS[0]),
        [false, true, true, true, true, true]
    );
}

#[test]
fn a_push_settles_after_three_ticks_and_ignores_a_bounce() {
    let mut d = Debounce::new();
    for _ in 1..DEBOUNCE_TICKS {
        assert!(!d.step(true));
    }
    assert!(d.step(true));
    // A one-tick bounce never shows.
    assert!(d.step(false));
    assert!(d.step(true));
    for _ in 1..DEBOUNCE_TICKS {
        assert!(d.step(false));
    }
    assert!(!d.step(false));
}
