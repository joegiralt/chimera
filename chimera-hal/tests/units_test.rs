// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_hal::Ms;
use chimera_hal::units::*;

#[test]
fn lba_offset_refuses_overflow() {
    assert_eq!(Lba::new(10).offset(5), Some(Lba::new(15)));
    assert_eq!(Lba::new(u32::MAX).offset(1), None);
}

#[test]
fn bytes_split_at_block_edges() {
    for (b, blk, off) in [(0, 0, 0), (511, 0, 511), (512, 1, 0), (513, 1, 1)] {
        assert_eq!(
            Bytes::new(b).split_block(),
            (FileBlock::new(blk), Bytes::new(off))
        );
    }
}

#[test]
fn token_window_in_bytes() {
    let spi = BytesPerSec::spi(25_000_000);
    assert_eq!(spi.bytes_in(Millis::new(100)), Bytes::new(312_500));
    assert_eq!(spi.bytes_in(Millis::new(u32::MAX)), Bytes::new(u32::MAX));
}

#[test]
fn throughput_from_bytes_and_time() {
    assert_eq!(
        Bytes::new(4_608).per(Micros::new(1_000)),
        BytesPerSec::new(4_608_000)
    );
    assert_eq!(
        Bytes::new(5).per(Micros::new(0)),
        Bytes::new(5).per(Micros::new(1))
    );
    assert_eq!(Bytes::new(u32::MAX).per(Micros::new(1)).get(), u32::MAX);
}

#[test]
fn ms_elapsed_across_wrap() {
    assert_eq!(Ms(5).elapsed_since(Ms(u32::MAX - 4)), Millis::new(10));
}

#[test]
fn stream_count_is_one_to_eight() {
    assert!(StreamCount::new(0).is_none() && StreamCount::new(9).is_none());
    assert!(StreamCount::new(1).is_some() && StreamCount::new(8).is_some());
    for n in 0..=8 {
        let x = StreamCount::new(n);
        assert_eq!(StreamCount::from_bits(StreamCount::to_bits(x)), x);
    }
}

#[test]
fn bytes_add_refuses_overflow() {
    assert_eq!(
        Bytes::new(1).checked_add(Bytes::new(2)),
        Some(Bytes::new(3))
    );
    assert_eq!(Bytes::new(u32::MAX).checked_add(Bytes::new(1)), None);
}

mod boot_card_units {
    use chimera_hal::store::BootCard;
    use chimera_hal::units::{Micros, Millis, NonZeroMillis};

    #[test]
    fn millis_add_saturates() {
        assert_eq!(
            Millis::new(3).saturating_add(Millis::new(4)),
            Millis::new(7)
        );
        assert_eq!(Millis::MAX.saturating_add(Millis::new(1)), Millis::MAX);
        assert_eq!(Millis::ZERO, Millis::new(0));
    }

    #[test]
    fn millis_as_micros_saturates() {
        assert_eq!(Millis::new(10).as_micros(), Micros::new(10_000));
        assert_eq!(Millis::MAX.as_micros(), Micros::new(u32::MAX));
    }

    #[test]
    fn non_zero_millis_refuses_zero() {
        assert_eq!(NonZeroMillis::new(0), None);
        assert_eq!(
            NonZeroMillis::new(10).map(NonZeroMillis::get),
            Some(Millis::new(10))
        );
    }

    fn all(ms: Millis) -> [Option<BootCard>; 5] {
        [
            None,
            Some(BootCard::Ready(ms)),
            Some(BootCard::Busy(ms)),
            Some(BootCard::Absent(ms)),
            Some(BootCard::Late(ms)),
        ]
    }

    #[test]
    fn boot_card_bits_round_trip() {
        for ms in [0, 1, 999, 1_000, BootCard::MAX_MS.get()] {
            for b in all(Millis::new(ms)) {
                assert_eq!(BootCard::from_bits(BootCard::to_bits(b)), b, "{b:?}");
            }
        }
    }

    /// Past `MAX_MS` a time saturates there, never wraps into the tag.
    #[test]
    fn boot_card_bits_saturate_at_max_ms() {
        for (b, max) in all(Millis::MAX).into_iter().zip(all(BootCard::MAX_MS)) {
            assert_eq!(BootCard::from_bits(BootCard::to_bits(b)), max);
        }
    }

    #[test]
    fn unknown_bits_read_as_untried() {
        assert_eq!(BootCard::from_bits(u32::MAX), None);
    }
}

#[test]
fn cycles_as_millis_rounds_down() {
    let per_ms = core::num::NonZeroU32::new(480_000).unwrap();
    assert_eq!(Cycles::new(479_999).as_millis(per_ms), Millis::new(0));
    assert_eq!(
        Cycles::new(480_000 * 7 + 5).as_millis(per_ms),
        Millis::new(7)
    );
}

#[test]
fn hz_per_ms_rounds_down_and_refuses_under_1_khz() {
    assert_eq!(
        Hz::new(480_000_000).per_ms().map(|n| n.get()),
        Some(480_000)
    );
    assert_eq!(Hz::new(999).per_ms(), None);
}
