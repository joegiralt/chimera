//! `fault` (fault-inject builds): provokes a DMA transfer error on a trailing
//! audio stream, to test its re-arm on a unit with no debug probe.

use super::{Arg, Words};

/// The stream to fault: DMA1 stream 1 or 2 (DAC pairs 2 and 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultArg {
    Dma(u8),
}

impl Arg for FaultArg {
    const USAGE: &'static str = "dma 1 or dma 2";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        if !words.next()?.eq_ignore_ascii_case(b"dma") {
            return None;
        }
        let n = match words.next()? {
            b"1" => 1,
            b"2" => 2,
            _ => return None,
        };
        words.next().is_none().then_some(FaultArg::Dma(n))
    }
}
