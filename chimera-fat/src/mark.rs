// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sweep mark (ADR 0094), `/CHIMERA/SWEEP.MRK`: one block, `CLEAN`
//! or not. A change that writes turns a clean mark `DIRTY` before its
//! first write and `CLEAN` after its last (`FatStore`); a mark written in
//! place changes no FAT value or entry, so it can strand nothing itself.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::fs::Fs;
use chimera_hal::store::{DOUBT_HALT, Dir, FileName, Mark};

/// The mark's block when clean.
pub const CLEAN: [u8; BLOCK] = {
    const TEXT: &[u8] = b"CHIMERA SWEEP MARK: CLEAN. ANY OTHER BLOCK OWES A LOST-CLUSTER SWEEP.\n";
    let mut b = [0; BLOCK];
    let mut i = 0;
    while i < BLOCK {
        b[i] = if i < TEXT.len() {
            TEXT[i]
        } else {
            0x5A ^ (i as u8)
        };
        i += 1;
    }
    b
};

/// The mark's block when dirty: `CLEAN`'s complement, different in every
/// byte, so a write of it torn anywhere, or noise, never reads clean.
pub const DIRTY: [u8; BLOCK] = {
    let mut b = CLEAN;
    let mut i = 0;
    while i < BLOCK {
        b[i] = !b[i];
        i += 1;
    }
    b
};

/// The mark after `n` doubting boots in a row: `DIRTY` with its first
/// byte moved by `n`, still different from `CLEAN` in every byte. A torn
/// write of one reads plain dirty: a shorter streak, one more sweep.
pub const fn doubted(n: u8) -> [u8; BLOCK] {
    let mut b = DIRTY;
    b[0] ^= n;
    b
}

/// An asked sweep, carrying `n` (0..=`DOUBT_HALT`) doubting boots: like
/// `doubted`, its first byte moved by `0x10 | n`.
pub const fn asked(n: u8) -> [u8; BLOCK] {
    let mut b = DIRTY;
    b[0] ^= 0x10 | n;
    b
}

pub fn name() -> FileName {
    FileName::new(Dir::Chimera, b"SWEEP", b"MRK").expect("an 8.3 name")
}

/// The mark, and its block if the file is one block long.
pub(crate) fn read<B: Blocks>(
    fs: &mut Fs<'_, B>,
) -> Result<(Mark, Option<u32>), FsError<B::Error>> {
    if !fs.has_dir(Dir::Chimera)? {
        return Ok((Mark::Unused, None));
    }
    let at = match fs.one_block(name()) {
        Ok(Some(at)) => at,
        Ok(None) | Err(FsError::Corrupt) => return Ok((Mark::Dirty, None)),
        Err(FsError::NotFound) => return Ok((Mark::Missing, None)),
        Err(e) => return Err(e),
    };
    let mut b = [0; BLOCK];
    fs.blocks().read(at, &mut b).map_err(FsError::Dev)?;
    let m = if b == CLEAN {
        Mark::Clean
    } else if let Some(n) = (1..=DOUBT_HALT).find(|&n| b == doubted(n)) {
        Mark::Doubted(n)
    } else if let Some(n) = (0..=DOUBT_HALT).find(|&n| b == asked(n)) {
        Mark::Asked(n)
    } else {
        Mark::Dirty
    };
    Ok((m, Some(at)))
}

/// A sweep that ran without a doubt but left work: a doubt count back to
/// plain `DIRTY`, in place. Any other mark is as it should be.
pub(crate) fn unsettled<B: Blocks>(fs: &mut Fs<'_, B>) -> Result<(), FsError<B::Error>> {
    if let (Mark::Doubted(_) | Mark::Asked(_), Some(at)) = read(fs)? {
        fs.blocks().write(at, &DIRTY).map_err(FsError::Dev)?;
    }
    Ok(())
}

/// After a boot's doubt. With `count`, one more doubting boot; without,
/// an asked mark's carried count put back. In place only: on a card in
/// doubt nothing allocates, so a missing mark, one of another length or
/// one whose chain doesn't hold it keeps no count (the boot goes on
/// sweeping). Returns the count now, 0 if none is kept.
pub(crate) fn doubt<B: Blocks>(fs: &mut Fs<'_, B>, count: bool) -> Result<u8, FsError<B::Error>> {
    let (m, at) = read(fs)?;
    let n = match (m, count) {
        (Mark::Unused, _) => return Ok(0),
        (Mark::Doubted(n), false) => return Ok(n),
        (Mark::Clean | Mark::Dirty | Mark::Missing, false) => return Ok(0),
        (Mark::Asked(n), false) => n,
        (Mark::Doubted(n) | Mark::Asked(n), true) => (n + 1).min(DOUBT_HALT),
        (Mark::Clean | Mark::Dirty | Mark::Missing, true) => 1,
    };
    let block = if n == 0 { DIRTY } else { doubted(n) };
    let Some(at) = at else { return Ok(0) };
    fs.blocks().write(at, &block).map_err(FsError::Dev)?;
    Ok(n)
}

/// The mark `Asked`, keeping its doubt count; made if it is missing.
pub(crate) fn request<B: Blocks>(fs: &mut Fs<'_, B>) -> Result<Mark, FsError<B::Error>> {
    let (m, at) = read(fs)?;
    let n = match m {
        Mark::Unused => return Ok(m),
        Mark::Doubted(n) | Mark::Asked(n) => n,
        Mark::Clean | Mark::Dirty | Mark::Missing => 0,
    };
    match (m, at) {
        (_, Some(at)) => fs.blocks().write(at, &asked(n)).map_err(FsError::Dev)?,
        (Mark::Missing, None) => _ = fs.write(name(), &mut |w| w.put(&asked(n)))?,
        _ => return Ok(m),
    }
    Ok(Mark::Asked(n))
}

/// The mark clean, after a settled sweep: in place, or made (a settled
/// sweep found every chain whole, the mark's old one included). No
/// `/CHIMERA`: nothing to mark, `false`.
pub(crate) fn clear<B: Blocks>(fs: &mut Fs<'_, B>) -> Result<bool, FsError<B::Error>> {
    match read(fs)? {
        (Mark::Unused, _) => Ok(false),
        (Mark::Clean, _) => Ok(true),
        (_, Some(at)) => {
            fs.blocks().write(at, &CLEAN).map_err(FsError::Dev)?;
            Ok(true)
        }
        (_, None) => {
            fs.write(name(), &mut |w| w.put(&CLEAN))?;
            Ok(true)
        }
    }
}
