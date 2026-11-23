// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "fault-inject")]
use chimera_core::console::{Console, FaultArg, Refusal, Request};

fn parse(line: &str) -> Option<Result<Request, Refusal>> {
    let mut c = Console::new();
    line.bytes().chain(*b"\n").filter_map(|b| c.push(b)).last()
}

#[test]
fn fault_takes_dma_1_or_2_only() {
    assert_eq!(
        parse("fault dma 1"),
        Some(Ok(Request::Fault(FaultArg::Dma(1))))
    );
    assert_eq!(
        parse("fault DMA 2"),
        Some(Ok(Request::Fault(FaultArg::Dma(2))))
    );
    assert_eq!(
        parse("fault ndtr"),
        Some(Ok(Request::Fault(FaultArg::Ndtr)))
    );
    for bad in ["fault ndtr 1"] {
        assert!(
            matches!(parse(bad), Some(Err(Refusal::Arguments(_)))),
            "{bad}"
        );
    }
    for bad in [
        "fault",
        "fault dma",
        "fault dma 0",
        "fault dma 3",
        "fault dma 1 x",
        "fault sai 1",
    ] {
        assert!(
            matches!(parse(bad), Some(Err(Refusal::Arguments(_)))),
            "{bad}"
        );
    }
}

/// `fault card`: the card's next transfer fails twice (a retry, then the
/// read fault); it takes no argument.
#[test]
fn fault_card_parses_alone() {
    assert_eq!(
        parse("fault card"),
        Some(Ok(Request::Fault(FaultArg::Card)))
    );
    assert_eq!(
        parse("fault CARD"),
        Some(Ok(Request::Fault(FaultArg::Card)))
    );
    for bad in ["fault card x", "fault card 2"] {
        assert!(
            matches!(parse(bad), Some(Err(Refusal::Arguments(_)))),
            "{bad}"
        );
    }
}
