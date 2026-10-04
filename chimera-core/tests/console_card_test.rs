use chimera_core::console::{
    CYCLE_SPAN_MS, CardArg, CardKind, CardSpeed, Console, Extents, FILE_BLOCKS, FILES, Latency,
    MAX_RUNS, RANDOM_READS, READ_BLOCKS, Refusal, Request, STREAM_BYTES_PER_S, STREAM_ROUNDS,
    STREAMS, Throughput, interleaved, random_reads, span_us,
};

#[test]
fn a_stream_is_24_khz_of_12_bits() {
    assert_eq!(STREAM_BYTES_PER_S, 36_000);
}

fn parse(line: &str) -> Option<Result<Request, Refusal>> {
    let mut c = Console::new();
    line.bytes().chain(*b"\n").filter_map(|b| c.push(b)).last()
}

#[test]
fn card_takes_clean_a_seed_or_nothing() {
    assert_eq!(parse("card"), Some(Ok(Request::Card(CardArg::Run(None)))));
    assert_eq!(parse("card CLEAN"), Some(Ok(Request::Card(CardArg::Clean))));
    assert_eq!(
        parse("card seed=4294967295"),
        Some(Ok(Request::Card(CardArg::Run(Some(u32::MAX)))))
    );
    for bad in [
        "card seed=",
        "card seed=-1",
        "card seed=4294967296",
        "card x",
        "card clean 1",
    ] {
        assert!(
            matches!(parse(bad), Some(Err(Refusal::Arguments(_)))),
            "{bad}"
        );
    }
}

#[test]
fn latency_takes_min_median_p95_p99_max() {
    let mut us: Vec<u32> = (1..=1_000).rev().collect();
    let l = Latency::of(&mut us).unwrap();
    assert_eq!(
        (l.min_us, l.median_us, l.p95_us, l.p99_us, l.max_us),
        (1, 501, 950, 990, 1_000)
    );
    assert_eq!(Latency::of(&mut []), None);
    let one = Latency::of(&mut [7]).unwrap();
    assert_eq!(
        (one.min_us, one.p95_us, one.p99_us, one.max_us),
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
fn streams_are_half_the_interleaved_rate() {
    let mut s = CardSpeed::default();
    assert_eq!(s.streams(), None, "nothing interleaved, no count");
    s.interleaved = Some(Throughput {
        bytes: 1_440_000,
        us: 1_000_000,
    });
    assert_eq!(s.streams(), Some(20));
}

#[test]
fn a_span_past_the_wrap_is_refused() {
    let hz = 480_000_000;
    assert_eq!(span_us(480_000, hz, 1), Some(1_000));
    assert_eq!(
        span_us(u32::MAX, hz, CYCLE_SPAN_MS - 1),
        Some(u32::MAX / 480)
    );
    assert_eq!(
        span_us(5, hz, CYCLE_SPAN_MS),
        None,
        "wrapped: the cycles lie"
    );
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
    assert_eq!(e.first(), Some(1_000));
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
fn random_reads_cover_both_files_and_repeat_by_seed() {
    let reads: Vec<_> = random_reads(42).collect();
    const { assert!(RANDOM_READS >= 1_000, "enough for a p99") };
    assert_eq!(reads.len(), RANDOM_READS);
    assert!(
        reads == random_reads(42).collect::<Vec<_>>(),
        "a seed repeats"
    );
    for other in [0, 1, 43] {
        assert!(
            reads != random_reads(other).collect::<Vec<_>>(),
            "seed {other}"
        );
    }
    assert!(random_reads(0).collect::<Vec<_>>() != random_reads(1).collect::<Vec<_>>());
    assert!(
        reads
            .iter()
            .all(|&(f, b)| f < FILES && b % READ_BLOCKS == 0 && b + READ_BLOCKS <= FILE_BLOCKS)
    );
    for f in 0..FILES {
        let n = reads.iter().filter(|r| r.0 == f).count();
        assert!(n > RANDOM_READS / 3, "file {f}: {n}");
    }
    assert!(reads.iter().any(|&(_, b)| b < FILE_BLOCKS / 4));
    assert!(reads.iter().any(|&(_, b)| b >= FILE_BLOCKS * 3 / 4));
}

#[test]
fn interleaved_turns_through_every_stream_each_round() {
    let reads: Vec<_> = interleaved().collect();
    assert_eq!(reads.len(), (STREAMS * STREAM_ROUNDS) as usize);
    let first: Vec<_> = reads[..STREAMS as usize].to_vec();
    let mut distinct = first.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        STREAMS as usize,
        "each stream its own place"
    );
    for f in 0..FILES {
        assert_eq!(
            first.iter().filter(|r| r.0 == f).count(),
            STREAMS as usize / FILES
        );
    }
    for (i, &(f, b)) in reads.iter().enumerate().skip(STREAMS as usize) {
        let (pf, pb) = reads[i - STREAMS as usize];
        assert_eq!(
            (f, b),
            (pf, pb + READ_BLOCKS),
            "a stream reads on from its last"
        );
        assert!(b + READ_BLOCKS <= FILE_BLOCKS);
    }
}

#[test]
fn the_card_kind_follows_addressing_and_size() {
    assert_eq!(CardKind::of(false, 2_000_000_000).name(), "SDSC");
    assert_eq!(CardKind::of(true, 31_914_983_424).name(), "SDHC");
    assert_eq!(CardKind::of(true, 63_864_569_856).name(), "SDXC");
}
