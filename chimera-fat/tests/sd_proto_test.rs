use chimera_fat::sd_addr::CmdArg;
use chimera_fat::sd_proto::*;
use chimera_hal::units::{BlockCount, BytesPerSec};

#[test]
fn command_frames_match_the_sd_spec() {
    assert_eq!(command(CMD0, CmdArg::ZERO), [0x40, 0, 0, 0, 0, 0x95]);
    assert_eq!(command(CMD8, CmdArg::IF_COND)[5], 0x87);
    assert_eq!(command(CMD17, CmdArg::ZERO)[5], 0x55);
    assert_eq!(command(CMD18, CmdArg::ZERO)[0], 0x52);
    assert_eq!(command(CMD12, CmdArg::ZERO)[0], 0x4C);
}

#[test]
fn crc16_of_512_ff_is_7fa1() {
    let data = [0xFF; 512];
    assert_eq!(crc16(&data), 0x7FA1);
    let (a, b) = data.split_at(200);
    assert_eq!(crc16_update(crc16_update(0, a), b), 0x7FA1);
}

#[test]
fn token_found_at_every_offset() {
    for at in 0..1024 {
        let mut buf = [0xFF; 1024];
        buf[at] = DATA_TOKEN;
        let mut s = TokenScan::new();
        assert_eq!(s.feed(&buf), Scan::Token { at });
        // split at every edge up to the token and just past it
        for cut in [at, at + 1, at.saturating_sub(1)] {
            let cut = cut.min(1024);
            let mut s = TokenScan::new();
            let (a, b) = buf.split_at(cut);
            match s.feed(a) {
                Scan::Token { at: t } => {
                    assert_eq!(t, at);
                    assert!(at < cut);
                }
                Scan::Waiting => assert_eq!(s.feed(b), Scan::Token { at: at - cut }),
                Scan::Error(e) => panic!("error {e}"),
            }
        }
    }
}

#[test]
fn error_token_reported() {
    let mut s = TokenScan::new();
    assert_eq!(s.feed(&[0xFF, 0xFF, 0b0000_0101]), Scan::Error(0x05));
}

#[test]
fn token_scan_times_out() {
    let clock = BytesPerSec::spi(25_000_000);
    let mut s = TokenScan::new();
    assert!(!s.timed_out(clock));
    assert_eq!(s.feed(&[0xFF; 312_499]), Scan::Waiting);
    assert!(!s.timed_out(clock));
    assert_eq!(s.feed(&[0xFF]), Scan::Waiting);
    assert!(s.timed_out(clock));
}

fn block(n: u8) -> [u8; BLOCK_WITH_CRC] {
    let mut b = [0u8; BLOCK_WITH_CRC];
    for (i, x) in b[..512].iter_mut().enumerate() {
        *x = (i as u8).wrapping_mul(7).wrapping_add(n);
    }
    let c = crc16(&b[..512]);
    b[512..].copy_from_slice(&c.to_be_bytes());
    b
}

/// What the card clocks out for a CMD18 of `gaps.len()` blocks: each
/// block's idle gap, its token, its data and CRC16.
fn stream(gaps: &[usize]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, &g) in gaps.iter().enumerate() {
        out.extend(std::iter::repeat_n(0xFF, g));
        out.push(DATA_TOKEN);
        out.extend_from_slice(&block(i as u8));
    }
    out
}

fn count(n: usize) -> BlockCount {
    BlockCount::new(n as u32).unwrap()
}

/// The shell, played: each burst `need` bytes (at most `cap`), landed at
/// the data's end. The data, the bursts, and whether every burst fit the
/// staging's bound.
fn play(card: &[u8], n: usize, cap: usize) -> Result<(Vec<u8>, usize), ProtoError> {
    let mut buf = vec![0xEE; n * STREAM_BLOCK];
    let (end, bursts) = play_at(&mut buf, 0, card, n, cap)?;
    assert_eq!(end, n * BLOCK_BYTES);
    buf.truncate(end);
    Ok((buf, bursts))
}

/// One read of `n` blocks, its data packed on from `start`: where its data
/// ends, and its bursts.
fn play_at(
    buf: &mut [u8],
    start: usize,
    card: &[u8],
    n: usize,
    cap: usize,
) -> Result<(usize, usize), ProtoError> {
    let (mut walk, mut end, mut clocked, mut bursts) = (Walk::new(count(n)), start, 0, 0);
    while let Walk::Read(r) = walk {
        let need = usize::try_from(r.need().get()).unwrap();
        assert!(need > 0);
        assert!(end + need <= buf.len(), "a burst past the staging");
        let k = need.min(cap);
        assert!(clocked + k <= card.len(), "clocked past the last CRC");
        buf[end..end + k].copy_from_slice(&card[clocked..clocked + k]);
        clocked += k;
        bursts += 1;
        (walk, end) = r.landed(&mut buf[..end + k], end)?;
    }
    assert_eq!(clocked, card.len(), "the walk stops at the last CRC");
    Ok((end, bursts))
}

fn data(n: usize) -> Vec<u8> {
    (0..n)
        .flat_map(|i| block(i as u8)[..512].to_vec())
        .collect()
}

#[test]
fn a_multiread_packs_nine_blocks_whatever_their_gaps() {
    let plans: [[usize; 9]; 4] = [
        [0; 9],
        [3_000, 20, 20, 20, 20, 20, 20, 20, 20],
        [1, 127, 128, 129, 513, 514, 515, 0, 1_031],
        [600, 0, 3, 0, 7, 0, 0, 2, 64],
    ];
    for gaps in plans {
        let (got, _) = play(&stream(&gaps), 9, usize::MAX).unwrap();
        assert_eq!(got, data(9), "{gaps:?}");
    }
}

#[test]
fn a_second_piece_packs_on_after_the_first() {
    // A chunk across a run's edge: 5 blocks, then 4 from another CMD18,
    // landing after the first piece's data, within the staging's bound.
    let data9 = data(9);
    for cap in [usize::MAX, 7, 515] {
        let mut buf = vec![0xEE; 9 * STREAM_BLOCK];
        let first = stream(&[40, 3, 0, 9, 1]);
        let (end, _) = play_at(&mut buf, 0, &first, 5, cap).unwrap();
        assert_eq!(end, 5 * BLOCK_BYTES);
        let mut second = Vec::new();
        for (i, g) in [700, 2, 2, 0].into_iter().enumerate() {
            second.extend(std::iter::repeat_n(0xFF, g));
            second.push(DATA_TOKEN);
            second.extend_from_slice(&block(5 + i as u8));
        }
        let (end, _) = play_at(&mut buf, end, &second, 4, cap).unwrap();
        assert_eq!(end, 9 * BLOCK_BYTES);
        assert_eq!(buf[..end], data9[..], "cap {cap}");
    }
}

#[test]
fn bursts_may_end_anywhere_in_a_block() {
    let card = stream(&[5, 0, 130]);
    for cap in [
        1, 2, 3, 5, 6, 7, 127, 128, 511, 512, 513, 514, 515, 516, 1_029,
    ] {
        let (got, _) = play(&card, 3, cap).unwrap();
        assert_eq!(got, data(3), "cap {cap}");
    }
}

#[test]
fn a_chunk_takes_a_few_bursts_not_one_a_token_and_a_block() {
    // About 1 ms to the first token at 25 MHz, then short gaps: the old
    // walk took a 128 B scan a burst to the first token (24), then a scan
    // and a rest a block (17 more).
    let gaps = [3_000, 20, 20, 20, 20, 20, 20, 20, 20];
    let (_, bursts) = play(&stream(&gaps), 9, usize::MAX).unwrap();
    assert!(bursts <= 3, "{bursts}");
    let (_, bursts) = play(&stream(&[0; 9]), 9, usize::MAX).unwrap();
    assert_eq!(bursts, 1);
}

#[test]
fn multiread_flags_bad_crc_and_the_walk_ends() {
    let mut card = stream(&[4, 4]);
    card[4 + 1 + 515 + 4 + 1 + 100] ^= 1;
    assert_eq!(play(&card, 2, usize::MAX).unwrap_err(), ProtoError::Crc);
    let mut card = stream(&[4, 4]);
    card[4 + 1 + 513] ^= 1;
    assert_eq!(play(&card, 2, 7).unwrap_err(), ProtoError::Crc);
}

#[test]
fn multiread_reports_an_error_token_and_the_walk_ends() {
    let mut card = stream(&[3, 3, 30]);
    card[2 * 518 + 10] = 0x02;
    assert_eq!(
        play(&card, 3, usize::MAX).unwrap_err(),
        ProtoError::ErrorToken(0x02)
    );
}

#[test]
fn multiread_times_out_waiting_for_a_token() {
    let Walk::Read(mut r) = Walk::new(count(1)) else {
        panic!()
    };
    let clock = BytesPerSec::spi(25_000_000);
    let mut buf = vec![0xFF; STREAM_BLOCK];
    let mut idle = 0;
    while idle < 312_500 {
        assert!(!r.timed_out(clock));
        let n = usize::try_from(r.need().get()).unwrap();
        let Walk::Read(next) = r.landed(&mut buf[..n], 0).unwrap().0 else {
            panic!()
        };
        (r, idle) = (next, idle + n);
    }
    assert!(r.timed_out(clock));
}

#[test]
fn a_bench_read_lands_every_burst_at_the_start() {
    // A read longer than the staging: every burst lands at 0, its blocks
    // still checked.
    let gaps = [9; 20];
    let card = stream(&gaps);
    let mut buf = vec![0; 4 * STREAM_BLOCK];
    let (mut walk, mut at) = (Walk::new(count(20)), 0);
    while let Walk::Read(r) = walk {
        let k = usize::try_from(r.need().get()).unwrap().min(buf.len());
        buf[..k].copy_from_slice(&card[at..at + k]);
        at += k;
        walk = r.landed(&mut buf[..k], 0).unwrap().0;
    }
    assert_eq!(at, card.len());
    // A bad block long after the staging wrapped is still caught.
    let mut bad = card.clone();
    bad[17 * (9 + STREAM_BLOCK) + 9 + 1 + 300] ^= 4;
    let mut walk = Walk::new(count(20));
    let mut at = 0;
    let err = loop {
        let Walk::Read(r) = walk else {
            panic!("the bad block passed")
        };
        let k = usize::try_from(r.need().get()).unwrap().min(buf.len());
        buf[..k].copy_from_slice(&bad[at..at + k]);
        at += k;
        match r.landed(&mut buf[..k], 0) {
            Ok((w, _)) => walk = w,
            Err(e) => break e,
        }
    };
    assert_eq!(err, ProtoError::Crc);
}

#[test]
fn stop_response_skips_junk_before_r1() {
    let mk = |v: [u8; 9]| stop_response(&v);
    assert_eq!(
        mk([0x3C, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
        Ok(())
    );
    assert_eq!(
        mk([0xFF, 0xFF, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
        Ok(())
    );
    assert_eq!(
        mk([0x3C, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
        Err(ProtoError::Timeout)
    );
    assert_eq!(
        mk([0x3C, 0x04, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF]),
        Err(ProtoError::R1(0x04))
    );
}

#[test]
fn r1_with_any_error_bit_is_refused() {
    assert_eq!(check_r1(0x00), Ok(()));
    assert_eq!(check_r1(0x04), Err(ProtoError::R1(0x04)));
    assert_eq!(check_r1(0x01), Err(ProtoError::R1(0x01)));
}
