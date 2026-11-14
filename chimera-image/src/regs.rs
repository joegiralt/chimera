//! Bank 2's flash-controller register values (RM0433 FLASH_KEYR2, CR2, SR2,
//! CCR2), pure: the board writes them, this decides them. Bank erase has no
//! public bit and no helper sets it; a sector is a `Bank2Sector`, S0-S7.

use crate::install::{Bank2Sector, FlashFail};

/// FLASH_KEYR2's unlock sequence, in order.
pub const KEYS: [u32; 2] = [0x4567_0123, 0xcdef_89ab];

// FLASH_CR2 bits.
pub const CR_LOCK: u32 = 1 << 0;
pub const CR_PG: u32 = 1 << 1;
pub const CR_SER: u32 = 1 << 2;
const CR_BER: u32 = 1 << 3;
const CR_PSIZE: u32 = 3 << 4;
const CR_FW: u32 = 1 << 6;
pub const CR_START: u32 = 1 << 7;
const CR_SNB: u32 = 7 << 8;
/// ×32: the parallelism bank 1's working CR1 has, allowed from VDD 1.62 V.
pub const PSIZE_X32: u32 = 2;

// FLASH_SR2 bits; FLASH_CCR2 clears each at the same position.
pub const SR_BSY: u32 = 1 << 0;
pub const SR_WBNE: u32 = 1 << 1;
pub const SR_QW: u32 = 1 << 2;
const SR_EOP: u32 = 1 << 16;
/// A read hit a double ECC error.
pub const SR_DBECCERR: u32 = 1 << 26;
/// A read's single ECC error, corrected silently.
pub const SR_SNECCERR: u32 = 1 << 25;
/// WRPERR, PGSERR, STRBERR, INCERR, OPERR, RDPERR, RDSERR, SNECCERR,
/// DBECCERR.
pub const SR_ERRORS: u32 = (0b111 << 17) | (0x3f << 21);
/// The CCR2 word that clears every error flag and EOP.
pub const CCR_CLEAR: u32 = SR_ERRORS | SR_EOP;
const _: () = assert!(SR_ERRORS & SR_DBECCERR != 0);
const _: () = assert!(SR_ERRORS & SR_SNECCERR != 0);

/// `cr` with sector `s`'s erase set up, START not yet: SER, SNB, PSIZE ×32;
/// PG, BER, FW and START clear; the rest kept.
pub const fn erase_cr(cr: u32, s: Bank2Sector) -> u32 {
    (cr & !(CR_PG | CR_BER | CR_PSIZE | CR_FW | CR_START | CR_SNB))
        | CR_SER
        | (PSIZE_X32 << 4)
        | ((s.index() as u32) << 8)
}

/// `cr` set up for programming: PG, PSIZE ×32; SER, BER, FW, START and
/// SNB clear; the rest kept.
pub const fn program_cr(cr: u32) -> u32 {
    (cr & !(CR_SER | CR_BER | CR_PSIZE | CR_FW | CR_START | CR_SNB)) | CR_PG | (PSIZE_X32 << 4)
}

/// `cr` relocked: LOCK set, and if `sr` is idle PG, SER and START cleared.
/// A still-running operation keeps them.
pub const fn relock_cr(cr: u32, sr: u32) -> u32 {
    if busy(sr) {
        cr | CR_LOCK
    } else {
        (cr & !(CR_PG | CR_SER | CR_START)) | CR_LOCK
    }
}

/// An operation runs or a word waits: BSY, QW or WBNE.
pub const fn busy(sr: u32) -> bool {
    sr & (SR_BSY | SR_QW | SR_WBNE) != 0
}

/// After a poll: an error flag wins, then a deadline passed while busy.
/// `None`: done, or still going.
pub const fn settle(sr: u32, timed_out: bool) -> Option<FlashFail> {
    if sr & SR_ERRORS != 0 {
        Some(FlashFail::Errors(sr & SR_ERRORS))
    } else if busy(sr) && timed_out {
        Some(FlashFail::Timeout)
    } else {
        None
    }
}

/// Sector erase at most, ms: DS (H743/753) Table 51 gives 4 s max at ×8
/// and 3.6 s at ×16, so ×32 is under 4 s; half again over that.
pub const ERASE_TIMEOUT_MS: u32 = 6_000;
/// One flash word's programming at most, ms: Table 51 gives 580 µs max
/// at ×8, 260 µs at ×32.
pub const WORD_TIMEOUT_MS: u32 = 5;
/// The cycle counter wraps in 2^32 cycles (8.9 s at 480 MHz): the
/// deadline sits well inside one wrap.
const _: () = assert!((ERASE_TIMEOUT_MS as u64) * 480_000 < u32::MAX as u64 / 4 * 3);

pub const fn ms_cycles(ms: u32, cpu_hz: u32) -> u32 {
    ms.saturating_mul(cpu_hz / 1000)
}
