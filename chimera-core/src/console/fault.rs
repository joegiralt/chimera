//! `fault` (fault-inject builds): provokes a DMA transfer error on a trailing
//! audio stream, to test its re-arm on a unit with no debug probe; `fault
//! ndtr` dumps the three streams' registers to see what the re-arm left.

use core::fmt::{self, Write};

use super::{Arg, Words};

/// The stream to fault: DMA1 stream 1 or 2 (DAC pairs 2 and 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultArg {
    Dma(u8),
    Ndtr,
}

impl Arg for FaultArg {
    const USAGE: &'static str = "dma 1, dma 2 or ndtr";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let first = words.next()?;
        if first.eq_ignore_ascii_case(b"ndtr") {
            return words.next().is_none().then_some(FaultArg::Ndtr);
        }
        if !first.eq_ignore_ascii_case(b"dma") {
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

/// One DMA stream, read at one instant, and the NDTR range seen while the
/// shell sampled for a few ring periods. A healthy circular stream reaches
/// near its ring length (256) as `ndtr_max`; one that restarted mid-ring and
/// reloads a partial count tops out below it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamSnap {
    pub cr: u32,
    pub ndtr: u16,
    pub m0ar: u32,
    pub ndtr_min: u16,
    pub ndtr_max: u16,
}

pub fn write_snaps(snaps: &[StreamSnap; 3], t: &mut impl Write) -> fmt::Result {
    snaps.iter().enumerate().try_for_each(|(i, s)| {
        writeln!(
            t,
            "stream{i} cr={:08x} ndtr={} m0ar={:08x} ndtr_min={} ndtr_max={}",
            s.cr, s.ndtr, s.m0ar, s.ndtr_min, s.ndtr_max
        )
    })
}
