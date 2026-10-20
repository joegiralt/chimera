use chimera_engine::stream::bench::Ratio;
use chimera_engine::stream::units::*;
use chimera_engine::voice_alloc::VoiceIdx;
use chimera_hal::units::{Bytes, Millis};

#[test]
fn odd_frames_take_a_whole_pair() {
    for (f, b) in [(0, 0), (1, 3), (2, 3), (3, 6), (4, 6), (2_688, 4_032)] {
        assert_eq!(Frames::new(f).to_bytes(Packing::Twelve), Bytes::new(b));
    }
    assert_eq!(Packing::Twelve.frames_in(Bytes::new(5)), Frames::new(2));
}

#[test]
fn generation_wraps_after_exactly_64() {
    let mut g = Generation::ZERO;
    for _ in 0..63 {
        g = g.next();
        assert_ne!(g, Generation::ZERO);
    }
    assert_eq!(g.next(), Generation::ZERO);
    assert_eq!(Generation::from_bits(64), Generation::ZERO);
}

#[test]
fn until_is_signed_across_the_wrap() {
    let b = AudioBlocks::new;
    assert_eq!(b(u32::MAX - 2).until(b(3)), 6);
    assert_eq!(b(10).until(b(4)), -6);
    assert_eq!(b(u32::MAX - 2).until(b(u32::MAX - 5)), -3);
}

#[test]
fn blocks_in_rounds_down() {
    for (ms, n) in [(1, 0), (2, 1), (4, 3), (15, 11)] {
        assert_eq!(AudioBlocks::blocks_in(Millis::new(ms)), n);
    }
}

#[test]
fn ratio_scales_frames_per_block() {
    let hz = |h| SourceRate::new(h).unwrap();
    let f = |r: Ratio, h| r.frames_per_block_x1000(hz(h)).get();
    assert_eq!(f(Ratio::ONE, 24_000), 32_000);
    assert_eq!(f(Ratio::from_milli(1_200).unwrap(), 24_000), 38_400);
    assert_eq!(f(Ratio::from_milli(32_000).unwrap(), 48_000), 2_048_000);
    assert_eq!(f(Ratio::from_milli(1).unwrap(), 4_000), 5);
    assert!(Ratio::from_milli(0).is_none() && Ratio::from_milli(32_001).is_none());
}

#[test]
fn source_rate_in_format_range() {
    assert!(SourceRate::new(3_999).is_none() && SourceRate::new(48_001).is_none());
    assert!(SourceRate::new(4_000).is_some() && SourceRate::new(48_000).is_some());
}

#[test]
fn voice_idx_below_max_voices() {
    assert!(VoiceIdx::new(7).is_some() && VoiceIdx::new(8).is_none());
    assert_eq!(VoiceIdx::ALL.len(), 8);
}

#[test]
fn frames_add_refuses_overflow() {
    assert_eq!(
        Frames::new(1).checked_add(Frames::new(2)),
        Some(Frames::new(3))
    );
    assert_eq!(Frames::new(u32::MAX).checked_add(Frames::new(1)), None);
}
