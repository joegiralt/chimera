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
