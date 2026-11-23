// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `fault` (fault-inject builds): provokes a DMA transfer error on a trailing
//! audio stream, to test its re-arm on a unit with no debug probe; `fault
//! ndtr` dumps the three streams' registers to see what the re-arm left;
//! `fault card` fails the card's next transfer twice: its retry, then the
//! read fault and the UI's recovery.

use core::fmt::{self, Write};

use super::{Arg, Words};

/// The stream to fault: DMA1 stream 1 or 2 (DAC pairs 2 and 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultArg {
    Dma(u8),
    Ndtr,
    Card,
}

impl Arg for FaultArg {
    const USAGE: &'static str = "dma 1, dma 2, ndtr or card";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let first = words.next()?;
        for (w, arg) in [(&b"ndtr"[..], FaultArg::Ndtr), (b"card", FaultArg::Card)] {
            if first.eq_ignore_ascii_case(w) {
                return words.next().is_none().then_some(arg);
            }
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
