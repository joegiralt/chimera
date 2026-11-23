// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_midi::ctl::{Dir, FreeCc};
use chimera_midi::learn::*;
use chimera_midi::rel::ByteEncoding::*;
use chimera_midi::value::U7;

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}
fn cc(n: u8) -> SeenCtl {
    SeenCtl::Cc(FreeCc::new(n).unwrap())
}
fn seen(samples: &[(SeenCtl, u8)]) -> Seen {
    let mut s = Seen::new();
    for &(c, v) in samples {
        assert!(s.push(c, u7(v)));
    }
    s
}
fn on_cc(n: u8, values: &[u8]) -> Seen {
    seen(&values.iter().map(|&v| (cc(n), v)).collect::<Vec<_>>())
}

/// Fixture lines: `cc <number> <hex>`, `data <hex>`, `fine <hex>`,
/// `step up|down`, `pressure <hex>`; `#` comments.
fn fixture(name: &str) -> Seen {
    let path = format!(
        "{}/tests/fixtures/learn/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let hex = |t: &str| u7(u8::from_str_radix(t, 16).unwrap());
    let mut s = Seen::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let t: Vec<&str> = line.split_whitespace().collect();
        let (c, v) = match t.as_slice() {
            ["cc", n, v] => (cc(n.parse().unwrap()), hex(v)),
            ["data", v] => (SeenCtl::Data, hex(v)),
            ["fine", v] => (SeenCtl::DataFine, hex(v)),
            ["step", "up"] => (SeenCtl::Step(Dir::Up), U7::ZERO),
            ["step", "down"] => (SeenCtl::Step(Dir::Down), U7::ZERO),
            ["pressure", v] => (SeenCtl::Pressure, hex(v)),
            _ => panic!("{path}: bad line {line:?}"),
        };
        assert!(s.push(c, v), "{path}: more than {} samples", Seen::CAP);
    }
    s
}

#[test]
fn ec4_ccr1_is_twos() {
    assert_eq!(
        classify(&fixture("ec4_ccr1")),
        Guess::Relative(TwosComplement, Sure::Yes)
    );
}

#[test]
fn ec4_ccr2_is_offset() {
    assert_eq!(
        classify(&fixture("ec4_ccr2")),
        Guess::Relative(BinaryOffset, Sure::Yes)
    );
}

#[test]
fn bcr_rel1_is_twos() {
    assert_eq!(
        classify(&fixture("bcr_rel1")),
        Guess::Relative(TwosComplement, Sure::Yes)
    );
}

#[test]
fn bcr_rel2_is_offset() {
    assert_eq!(
        classify(&fixture("bcr_rel2")),
        Guess::Relative(BinaryOffset, Sure::Yes)
    );
}

#[test]
fn bcr_rel3_is_sign_down() {
    assert_eq!(
        classify(&fixture("bcr_rel3")),
        Guess::Relative(SignBitDown, Sure::Yes)
    );
}

#[test]
fn mackie_vpot_is_sign_down() {
    assert_eq!(
        classify(&fixture("mackie_vpot")),
        Guess::Relative(SignBitDown, Sure::Yes)
    );
}

#[test]
fn ableton_signed_is_sign_up() {
    assert_eq!(
        classify(&fixture("ableton_signed")),
        Guess::Relative(SignBitUp, Sure::Yes)
    );
}

#[test]
fn one_direction_guess_not_sure() {
    // 01-like → two's complement, 41-like → binary offset, both unsure
    assert_eq!(
        classify(&on_cc(21, &[1, 1, 2, 1])),
        Guess::Relative(TwosComplement, Sure::No)
    );
    assert_eq!(
        classify(&on_cc(21, &[0x41, 0x41, 0x43, 0x41])),
        Guess::Relative(BinaryOffset, Sure::No)
    );
}

#[test]
fn pot_sweep_is_absolute() {
    let up: Vec<u8> = (40..64).collect();
    assert_eq!(classify(&on_cc(74, &up)), Guess::Absolute);
    let down: Vec<u8> = (100..124).rev().collect();
    assert_eq!(classify(&on_cc(74, &down)), Guess::Absolute);
    // up then back down
    assert_eq!(
        classify(&on_cc(74, &[1, 3, 6, 10, 14, 12, 9, 5, 2, 0])),
        Guess::Absolute
    );
}

#[test]
fn pot_wiggle_near_64_is_absolute() {
    let wiggle = [62, 63, 64, 65, 66, 65, 64, 63, 62, 63, 64, 65];
    assert_eq!(classify(&on_cc(74, &wiggle)), Guess::Absolute);
}

#[test]
fn button_0_127() {
    assert_eq!(classify(&on_cc(80, &[127, 0, 127, 0])), Guess::Button);
    assert_eq!(
        classify(&on_cc(80, &[127, 127, 127])),
        Guess::Button,
        "toggle sending press only"
    );
    assert_eq!(
        classify(&on_cc(80, &[100, 10, 100, 10])),
        Guess::Button,
        "two values > 64 apart"
    );
}

/// One 01 and one 7F looks like a button; a repeat proves an encoder.
#[test]
fn button_then_relative_self_corrects() {
    assert_eq!(classify(&on_cc(21, &[0x01, 0x7F])), Guess::Button);
    assert_eq!(
        classify(&on_cc(21, &[0x01, 0x7F, 0x01, 0x01])),
        Guess::Relative(TwosComplement, Sure::Yes)
    );
}

#[test]
fn cc2_then_cc34_is_paired14() {
    let s = seen(&[
        (cc(2), 10),
        (cc(34), 5),
        (cc(2), 10),
        (cc(34), 90),
        (cc(2), 11),
        (cc(34), 3),
    ]);
    assert_eq!(classify(&s), Guess::Paired14);
    // half or fewer of the MSBs paired: not a pair
    let s = seen(&[
        (cc(2), 10),
        (cc(34), 5),
        (cc(2), 11),
        (cc(2), 12),
        (cc(2), 13),
    ]);
    assert_eq!(classify(&s), Guess::Absolute);
    // n must be 1..=31: CC 40 then CC 72 is no pair
    let s = seen(&[(cc(40), 10), (cc(72), 5), (cc(40), 11), (cc(72), 5)]);
    assert_ne!(classify(&s), Guess::Paired14);
}

#[test]
fn cc6_never_paired() {
    // CC 6 / CC 38 are data entry: the NRPN coarse/fine halves, never a 14-bit CC pair
    let s = seen(&[
        (SeenCtl::Data, 10),
        (SeenCtl::DataFine, 10),
        (SeenCtl::Data, 11),
        (SeenCtl::DataFine, 11),
        (SeenCtl::Data, 11),
        (SeenCtl::DataFine, 11),
    ]);
    assert_eq!(classify(&s), Guess::Absolute);
    assert!(
        s.fine_seen(),
        "an NRPN that sent CC 38 is reported for echo"
    );
    assert!(!on_cc(2, &[1, 2]).fine_seen());
}

#[test]
fn nrpn_coarse_runs_classify_like_cc() {
    let s = seen(&[
        (SeenCtl::Data, 1),
        (SeenCtl::Data, 1),
        (SeenCtl::Data, 0x7F),
        (SeenCtl::Data, 0x7F),
    ]);
    assert_eq!(classify(&s), Guess::Relative(TwosComplement, Sure::Yes));
}

#[test]
fn steps_are_reported_not_classified() {
    let s = seen(&[(SeenCtl::Step(Dir::Up), 0), (SeenCtl::Step(Dir::Down), 0)]);
    assert!(s.stepped(), "Task 12 maps a stepped Seen to IncDec");
    assert!(!on_cc(2, &[1]).stepped());
}

#[test]
fn seen_holds_24_then_refuses() {
    let mut s = Seen::new();
    for i in 0..Seen::CAP {
        assert!(s.push(cc(21), u7(i as u8)));
    }
    assert_eq!(Seen::CAP, 24);
    assert!(!s.push(cc(21), u7(0)));
    assert_eq!(s.len(), 24);
}

#[test]
fn empty_and_single_are_absolute() {
    assert_eq!(classify(&Seen::new()), Guess::Absolute);
    assert_eq!(classify(&on_cc(21, &[1])), Guess::Absolute);
}
