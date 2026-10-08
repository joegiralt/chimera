//! The sweep mark (ADR 0094), `/CHIMERA/SWEEP.MRK`: one block, `CLEAN`
//! or not. A change that writes turns a clean mark `DIRTY` before its
//! first write and `CLEAN` after its last (`FatStore`); a mark written in
//! place changes no FAT value or entry, so it can strand nothing itself.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::fs::Fs;
use chimera_hal::store::{Dir, FileName, Mark};

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
    let m = if b == CLEAN { Mark::Clean } else { Mark::Dirty };
    Ok((m, Some(at)))
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
