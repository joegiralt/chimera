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
        assert_eq!(Bytes::new(b).split_block(), (FileBlock::new(blk), Bytes::new(off)));
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
    assert_eq!(Bytes::new(4_608).per(Micros::new(1_000)), BytesPerSec::new(4_608_000));
    assert_eq!(Bytes::new(5).per(Micros::new(0)), Bytes::new(5).per(Micros::new(1)));
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
    assert_eq!(Bytes::new(1).checked_add(Bytes::new(2)), Some(Bytes::new(3)));
    assert_eq!(Bytes::new(u32::MAX).checked_add(Bytes::new(1)), None);
}
