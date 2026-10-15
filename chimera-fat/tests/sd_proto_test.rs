use chimera_fat::sd_addr::CmdArg;
use chimera_fat::sd_proto::*;
use chimera_hal::units::{BlockCount, Bytes, BytesPerSec};

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

/// Plays the shell on one block: scans with the token at `pos` (after
/// `empty` blank scans), copies the tail, lands the rest.
fn play(
    walk: Walk,
    blk: &[u8; BLOCK_WITH_CRC],
    pos: usize,
    empty: usize,
) -> Result<([u8; BLOCK_WITH_CRC], Walk), ProtoError> {
    let Walk::Scan(mut sc) = walk else {
        panic!("not scanning")
    };
    for _ in 0..empty {
        sc = match sc.scanned(&[0xFF; SCAN_BYTES])? {
            ScanOutcome::Waiting(s) => s,
            ScanOutcome::Token { .. } => panic!("early token"),
        };
    }
    let mut burst = [0xFF; SCAN_BYTES];
    burst[pos] = DATA_TOKEN;
    let n = SCAN_BYTES - pos - 1;
    burst[pos + 1..].copy_from_slice(&blk[..n]);
    let ScanOutcome::Token { tail, rest } = sc.scanned(&burst)? else {
        panic!("token missed")
    };
    assert_eq!(tail, pos + 1..SCAN_BYTES);
    assert_eq!(rest.have(), Bytes::new(n as u32));
    assert_eq!(rest.want(), Bytes::new((BLOCK_WITH_CRC - n) as u32));
    let mut out = [0u8; BLOCK_WITH_CRC];
    out[..tail.len()].copy_from_slice(&burst[tail]);
    out[n..].copy_from_slice(&blk[n..]);
    Ok((out, rest.landed(&out)?))
}

#[test]
fn multiread_walks_nine_blocks_with_tokens_anywhere() {
    let mut w = Walk::new(BlockCount::new(9).unwrap());
    let plan = [
        (0, 0),
        (1, 0),
        (64, 0),
        (127, 0),
        (5, 3),
        (0, 1),
        (127, 3),
        (30, 2),
        (99, 0),
    ];
    for (i, (pos, empty)) in plan.into_iter().enumerate() {
        let b = block(i as u8);
        let (out, next) = play(w, &b, pos, empty).unwrap();
        assert_eq!(out, b);
        w = next;
    }
    assert!(matches!(w, Walk::Stop));
}

#[test]
fn the_block_index_advances_per_landing() {
    let mut w = Walk::new(BlockCount::new(3).unwrap());
    for i in 0..3 {
        let Walk::Scan(sc) = w else {
            panic!("stopped early")
        };
        let mut burst = [0xFF; SCAN_BYTES];
        burst[0] = DATA_TOKEN;
        let ScanOutcome::Token { rest, .. } = sc.scanned(&burst).unwrap() else {
            panic!()
        };
        assert_eq!(rest.block(), i);
        w = rest.landed(&block(0)).unwrap();
    }
    assert!(matches!(w, Walk::Stop));
}

#[test]
fn multiread_flags_bad_crc_and_the_walk_ends() {
    let w = Walk::new(BlockCount::new(2).unwrap());
    let mut b = block(1);
    b[100] ^= 1;
    assert_eq!(play(w, &b, 0, 0).unwrap_err(), ProtoError::Crc);
}

#[test]
fn multiread_reports_an_error_token_and_the_walk_ends() {
    let mut w = Walk::new(BlockCount::new(9).unwrap());
    for i in 0..2 {
        w = play(w, &block(i), 3, 0).unwrap().1;
    }
    let Walk::Scan(sc) = w else { panic!() };
    let mut burst = [0xFF; SCAN_BYTES];
    burst[10] = 0x02;
    assert_eq!(
        sc.scanned(&burst).unwrap_err(),
        ProtoError::ErrorToken(0x02)
    );
}

#[test]
fn multiread_times_out_waiting_for_a_token() {
    let Walk::Scan(mut sc) = Walk::new(BlockCount::new(1).unwrap()) else {
        panic!()
    };
    let clock = BytesPerSec::spi(25_000_000);
    for _ in 0..(312_500 / SCAN_BYTES) {
        assert!(!sc.timed_out(clock));
        sc = match sc.scanned(&[0xFF; SCAN_BYTES]).unwrap() {
            ScanOutcome::Waiting(s) => s,
            _ => panic!(),
        };
    }
    sc = match sc.scanned(&[0xFF; SCAN_BYTES]).unwrap() {
        ScanOutcome::Waiting(s) => s,
        _ => panic!(),
    };
    assert!(sc.timed_out(clock));
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
