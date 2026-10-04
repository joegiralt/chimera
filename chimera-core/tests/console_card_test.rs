use chimera_core::console::{
    CardSpeed, Extents, FILE_BLOCKS, Latency, MAX_RUNS, RANDOM_BLOCKS, RANDOM_READS,
    STREAM_BYTES_PER_S, Throughput, random_starts,
};

#[test]
fn a_stream_is_24_khz_of_12_bits() {
    assert_eq!(STREAM_BYTES_PER_S, 36_000);
}

#[test]
fn latency_takes_min_median_p99_max() {
    let mut us: Vec<u32> = (1..=200).rev().collect();
    let l = Latency::of(&mut us).unwrap();
    assert_eq!(
        (l.min_us, l.median_us, l.p99_us, l.max_us),
        (1, 101, 198, 200)
    );
    assert_eq!(Latency::of(&mut []), None);
    let one = Latency::of(&mut [7]).unwrap();
    assert_eq!(
        (one.min_us, one.median_us, one.p99_us, one.max_us),
        (7, 7, 7, 7)
    );
}

#[test]
fn throughput_never_divides_by_zero() {
    let t = Throughput {
        bytes: 1_000_000,
        us: 500_000,
    };
    assert_eq!((t.bytes_per_s(), t.kb_per_s()), (2_000_000, 2_000));
    assert_eq!(Throughput { bytes: 5, us: 0 }.bytes_per_s(), 5_000_000);
}

#[test]
fn streams_are_the_lower_of_the_two_at_half_rate() {
    let mut s = CardSpeed {
        loaded: Throughput {
            bytes: 1_440_000,
            us: 1_000_000,
        },
        ..CardSpeed::default()
    };
    // 4 KB every 4096 µs: 1 MB/s.
    s.random.p99_us = 4_096;
    assert_eq!(s.streams_seq(), 20);
    assert_eq!(s.streams_random(), 13);
    assert_eq!(s.streams(), 13);
}

#[test]
fn transfers_never_cross_a_run() {
    let mut e = Extents::new();
    e.push(1_000, 10);
    e.push(5_000, 6);
    let t: Vec<_> = e.transfers(4, 12, 4).collect();
    assert_eq!(t, [(1_004, 4), (1_008, 2), (5_000, 4), (5_004, 2)]);
    assert_eq!(e.locate(15), Some((5_005, 1)));
    assert_eq!(e.locate(16), None);
    // Past the runs it stops short.
    assert_eq!(e.transfers(14, 10, 32).map(|(_, n)| n).sum::<u32>(), 2);
}

#[test]
fn extents_count_runs_they_cannot_keep() {
    let mut e = Extents::new();
    for i in 0..MAX_RUNS as u32 + 3 {
        e.push(i * 10, 1);
    }
    assert_eq!(e.seen(), MAX_RUNS as u32 + 3);
    assert!(!e.whole());
    assert_eq!(e.locate(MAX_RUNS as u32), None);
}

#[test]
fn random_starts_are_aligned_inside_the_file_and_spread() {
    let starts: Vec<u32> = random_starts(0).collect();
    assert_eq!(starts.len(), RANDOM_READS);
    assert!(
        starts
            .iter()
            .all(|&b| b % RANDOM_BLOCKS == 0 && b + RANDOM_BLOCKS <= FILE_BLOCKS)
    );
    let mut distinct = starts.clone();
    distinct.sort_unstable();
    distinct.dedup();
    // 256 draws of 1024 slots: about 226 distinct.
    assert!(distinct.len() > RANDOM_READS * 8 / 10);
    assert!(starts.iter().any(|&b| b < FILE_BLOCKS / 4));
    assert!(starts.iter().any(|&b| b >= FILE_BLOCKS * 3 / 4));
}
