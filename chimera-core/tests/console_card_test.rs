use chimera_core::console::{
    CYCLE_SPAN_MS, CardArg, CardKind, CardSpeed, Console, FILE_BLOCKS, FILES, Latency,
    MIN_APART_PER_MILLE, RANDOM_READS, READ_BLOCKS, Refusal, Request, STREAM_BYTES_PER_S,
    STREAM_ROUNDS, STREAMS, Throughput, apart_per_mille, interleaved, random_reads, span_us,
};
use chimera_hal::store::{MAX_RUNS, Runs};
use chimera_hal::units::{BlockCount, FileBlock, Lba};

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
fn streams_come_from_a_p95_round_at_half_rate() {
    let mut s = CardSpeed {
        interleaved: Some(Throughput {
            bytes: 10_000_000,
            us: 1_000_000,
        }),
        ..CardSpeed::default()
    };
    assert_eq!(s.streams(), None, "no rounds timed, no count");
    // 8 streams × 4096 B a round, p95 at 45.5 ms: 720 KB/s, 10 at half.
    s.rounds = Some(Latency {
        median_us: 1_000,
        p95_us: 45_511,
        ..Latency::default()
    });
    assert_eq!(s.streams(), Some(10), "the tail, not the mean or median");
}

#[test]
fn files_apart_is_per_mille_of_the_card() {
    let card = 8_000_000_000u64; // 15,625,000 blocks
    assert_eq!(apart_per_mille(0, 7_812_500, card), 500);
    assert_eq!(apart_per_mille(7_812_500, 0, card), 500);
    assert!(u64::from(apart_per_mille(100, 3_000_000, card)) < MIN_APART_PER_MILLE);
    assert_eq!(
        apart_per_mille(1, 2, 0),
        1_000,
        "no size: never divides by zero"
    );
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
    let mut r = Runs::new();
    r.push(Lba::new(1_000), BlockCount::new(10).unwrap());
    r.push(Lba::new(5_000), BlockCount::new(6).unwrap());
    let e = r.finish().unwrap();
    let max = |n| BlockCount::new(n).unwrap();
    let t: Vec<_> = e
        .transfers(FileBlock::new(4), 12, max(4))
        .map(|(a, n)| (a.get(), n.get()))
        .collect();
    assert_eq!(t, [(1_004, 4), (1_008, 2), (5_000, 4), (5_004, 2)]);
    let at = e.locate(FileBlock::new(15)).unwrap();
    assert_eq!((at.0.get(), at.1.get()), (5_005, 1));
    assert_eq!(e.locate(FileBlock::new(16)), None);
    assert_eq!(e.first(), Lba::new(1_000));
    let past = e.transfers(FileBlock::new(14), 10, max(32));
    assert_eq!(past.map(|(_, n)| n.get()).sum::<u32>(), 2);
}

#[test]
fn extents_refuse_runs_they_cannot_keep() {
    let mut r = Runs::new();
    for i in 0..MAX_RUNS as u32 + 3 {
        r.push(Lba::new(i * 10), BlockCount::ONE);
    }
    assert_eq!(r.finish().err().map(|f| f.runs), Some(MAX_RUNS as u32 + 3));
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
    for other in [0, 1, 43, 42 ^ 1, u32::MAX] {
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

mod dma {
    use chimera_core::console::{
        CardKind, DmaOwn, DmaReport, Histogram, IrqCounts, LinkFit, Percentile, Spread,
        StreamLines, fit_line, write_dma,
    };
    use chimera_core::stream::bench::BenchReport;
    use chimera_core::ui::about_page::THIS;
    use chimera_hal::units::{Bytes, Micros, Millis, StreamCount};

    fn us(n: u32) -> Percentile {
        Percentile::Within(Micros::new(n))
    }

    #[test]
    fn histogram_percentiles_by_nearest_rank() {
        let mut h = Box::new(Histogram::<2_500>::new());
        // One sample in each of the first 1,000 buckets, in no order.
        for i in (0..1_000u32).rev() {
            h.record(Micros::new(i * 100 + 50));
        }
        assert_eq!(h.count(), 1_000);
        assert_eq!(h.percentile(500), us(50_000), "the bucket's upper edge");
        assert_eq!(h.percentile(990), us(99_000));
        assert_eq!(h.percentile(999), us(99_900));
        assert_eq!(h.percentile(1_000), us(100_000));
        assert_eq!(h.percentile(0), us(100), "rank 1 at least");
        h.record(Micros::new(250_000));
        assert_eq!(h.percentile(1_000), Percentile::Over, "past 250 ms");
        assert_eq!(h.percentile(500), us(50_100));
        h.clear();
        assert_eq!((h.count(), h.percentile(500)), (0, us(0)), "empty");
        h.record(Micros::new(99));
        assert_eq!(h.spread().max, us(100));
        assert_eq!(size_of::<Histogram<2_500>>(), 10_004);
    }

    #[test]
    fn fit_recovers_latency_and_bandwidth() {
        // L = 2,000 µs, BW = 10 MB/s: 10 B a µs.
        let pts: Vec<_> = [1u32, 2, 4, 8, 16]
            .iter()
            .map(|k| {
                let b = k * 1_024;
                (Bytes::new(b), Micros::new(2_000 + b / 10))
            })
            .collect();
        let LinkFit { latency, bandwidth } = fit_line(&pts).unwrap();
        assert!((1_980..=2_020).contains(&latency.get()), "{latency:?}");
        assert!(
            (9_900_000..=10_100_000).contains(&bandwidth.get()),
            "{bandwidth:?}"
        );
        assert_eq!(fit_line(&pts[..1]), None, "one point");
        assert_eq!(fit_line(&[pts[2], pts[2], pts[2]]), None, "one size");
        let falling = [pts[0], (pts[4].0, Micros::new(100))];
        assert_eq!(fit_line(&falling), None, "a slope at or under 0");
        assert_eq!(fit_line(&[]), None);
    }

    fn report(own: DmaOwn) -> DmaReport {
        DmaReport {
            spread: Spread {
                n: 20_000,
                median: us(2_100),
                p95: us(2_400),
                p99: us(3_000),
                p999: us(9_000),
                max: Percentile::Over,
            },
            overruns: 0,
            loaded_load_pct: 72,
            card: CardKind::Sdhc,
            card_mb: 31_914,
            aborts: 1,
            stop_errors: 0,
            card_irq_us_max: Micros::new(312),
            irq: IrqCounts {
                entries: 900,
                reads: 40,
                bursts: 760,
                bursts_max: 31,
            },
            own,
        }
    }

    fn text(r: &DmaReport) -> String {
        let mut s = String::new();
        write_dma(r, &mut s).unwrap();
        s
    }

    fn common() -> String {
        let plus = if THIS.dirty() { "+" } else { "" };
        format!(
            "n 20000\nmedian_us 2100\np95_us 2400\np99_us 3000\np999_us 9000\nmax_us over\n\
             overruns 0\nloaded_load_pct 72\ncard SDHC\ncard_mb 31914\ngit {}{plus}\n\
             aborts 1\nstop_errors 0\ncard_irq_us_max 312\n\
             card_irqs 900\ncard_reads 40\ncard_read_bursts 760\ncard_read_bursts_max 31\n",
            THIS.hash()
        )
    }

    fn streams() -> StreamLines {
        StreamLines {
            n_streams: StreamCount::new(2).unwrap(),
            report: BenchReport {
                empties: 0,
                late_max_blocks: 3,
                blocks: 450_000,
                order_errors: 0,
                check_errors: [0, 2, 0, 0, 0, 0, 0, 9],
            },
            fills_lost: 4,
            runs: [1, 3, 1, 1, 1, 1, 1, 1],
        }
    }

    #[test]
    fn dma_report_text() {
        let want = common()
            + "n_streams 2\nempties 0\nlate_max_blocks 3\nblocks 450000\norder_errors 0\n\
               check_errors 2\ncheck_errors_0 0\ncheck_errors_1 2\nfills_lost 4\nruns_0 1\nruns_1 3\n";
        assert_eq!(text(&report(DmaOwn::Streams(streams()))), want);

        let chunk = DmaOwn::Chunk {
            idle: None,
            fit: Some(LinkFit {
                latency: Micros::new(1_850),
                bandwidth: chimera_hal::units::BytesPerSec::new(2_345_678),
            }),
            crc16: Micros::new(41),
        };
        let want =
            common() + "idle_fit none\nfit_latency_us 1850\nfit_bandwidth_kbs 2345\ncrc16_us 41\n";
        assert_eq!(text(&report(chunk)), want);

        let ui = DmaOwn::Ui {
            streams: streams(),
            ui_fps_min: 31,
            ui_frozen_max: Millis::new(140),
            save_ok: true,
        };
        assert!(text(&report(ui)).ends_with("ui_fps_min 31\nui_frozen_max_ms 140\nsave_ok 1\n"));
        let reads = DmaOwn::Reads {
            streams: streams(),
            ui_reads: 100,
            ui_reads_ok: 99,
            faults: 0,
        };
        assert!(text(&report(reads)).ends_with("ui_reads 100\nui_reads_ok 99\nfaults 0\n"));
        let stall = DmaOwn::Stall {
            streams: streams(),
            write_busy_errors: 0,
        };
        assert!(text(&report(stall)).ends_with("runs_1 3\nwrite_busy_errors 0\n"));
        let back = DmaOwn::Back {
            back_mean: Micros::new(2_600),
            forward_mean: Micros::new(2_500),
        };
        assert!(text(&report(back)).ends_with("back_mean_us 2600\nforward_mean_us 2500\n"));
        assert_eq!(text(&report(DmaOwn::Burst)), common());
    }

    #[test]
    fn a_dma_verb_answers_at_once() {
        use chimera_core::console::{DmaAnswer, DmaRefusal, DmaVerb, Inject, write_dma_answer};
        let say = |a: DmaAnswer| {
            let mut s = String::new();
            write_dma_answer(&a, &mut s).unwrap();
            s
        };
        let n = StreamCount::new(8).unwrap();
        assert_eq!(
            say(DmaAnswer::Started(DmaVerb::Sustain(n))),
            "OK started sustain 8\n"
        );
        assert_eq!(
            say(DmaAnswer::Started(DmaVerb::Inject(Inject::Te))),
            "OK started inject te\n"
        );
        assert_eq!(say(DmaAnswer::Running(42)), "running 42\nOK\n");
        assert_eq!(say(DmaAnswer::Stopped), "OK stopped\n");
        assert_eq!(say(DmaAnswer::Refused(DmaRefusal::Busy)), "ERR busy\n");
        assert_eq!(say(DmaAnswer::Refused(DmaRefusal::Beds)), "ERR beds\n");
        assert_eq!(
            say(DmaAnswer::Refused(DmaRefusal::NoRun)),
            "ERR card: no dma run\n"
        );
        assert_eq!(
            say(DmaAnswer::Refused(DmaRefusal::Absent)),
            "ERR card dma is not in this build\n"
        );
        let r = report(DmaOwn::Burst);
        assert_eq!(say(DmaAnswer::Report(r)), common() + "OK\n");
    }
}
