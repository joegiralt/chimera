//! Installing an image into bank 2 so that a power cut at any instant leaves
//! it closed: S0 erased first, the commit word programmed last.

use crate::header::{BANK2_BASE, BANK2_LEN, COMMIT_OFF, FLASH_WORD, HEADER_LEN, Header, Seal};
use crate::judge::{Bank2Memory, Door, ImageRead, Verdict, judge, judge_uncommitted};

const SECTOR_LEN: u32 = 128 * 1024;
const _: () = assert!(4 * SECTOR_LEN == BANK2_LEN);

/// A bank-2 sector Chimera may erase. S4..S7 are the bootloader's and have
/// no name here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank2Sector {
    S0,
    S1,
    S2,
    S3,
}

const SECTORS: [Bank2Sector; 4] = [
    Bank2Sector::S0,
    Bank2Sector::S1,
    Bank2Sector::S2,
    Bank2Sector::S3,
];

impl Bank2Sector {
    pub const fn index(self) -> u8 {
        self as u8
    }

    pub const fn addr(self) -> u32 {
        BANK2_BASE + self.index() as u32 * SECTOR_LEN
    }
}

/// A flash-word address inside S0..=S3, 32-byte aligned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bank2Word(u32);

impl Bank2Word {
    pub const fn new(addr: u32) -> Option<Self> {
        if addr >= BANK2_BASE && addr - BANK2_BASE < BANK2_LEN && addr.is_multiple_of(FLASH_WORD) {
            Some(Bank2Word(addr))
        } else {
            None
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashFail {
    Busy,
    Locked,
    /// The MPU or the door not as the writer left them: nothing started.
    NotWritable,
    /// The status register's error bits.
    Errors(u32),
    Timeout,
    /// A program over a word not all 0xFF.
    NotErased,
    /// The cycle counter isn't counting: no op could be timed out.
    NoClock,
}

/// Bank 2's flash controller.
///
/// # Safety
/// As `Bank2Memory`; `erase` and `program` act on this chip's bank 2 only,
/// at the sector or word named, and return `Ok` only once the op completed
/// without an error flag.
pub unsafe trait Flash: Bank2Memory {
    fn erase(&mut self, s: Bank2Sector) -> Result<(), FlashFail>;
    fn program(&mut self, w: Bank2Word, data: &[u8; 32]) -> Result<(), FlashFail>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    /// S0 first: erasing it removes the commit word before anything else.
    pub sectors: &'static [Bank2Sector],
    pub body_words: u32,
}

/// The sectors the image spans, S0 up to the last; a body past the bank is
/// clamped to it (`judge` refuses one anyway).
pub fn plan(h: &Header) -> Plan {
    let body = h.body_len.min(BANK2_LEN - HEADER_LEN);
    let last = (HEADER_LEN + body - 1) / SECTOR_LEN;
    Plan {
        sectors: SECTORS.get(..=last as usize).unwrap_or(&SECTORS),
        body_words: body / FLASH_WORD,
    }
}

/// Where a flash op failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashAt {
    Sector(Bank2Sector),
    Word(Bank2Word),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallFail {
    /// The staged file isn't `Good` for this build; flash untouched.
    Staged(Verdict),
    Flash(FlashFail, FlashAt),
    /// What flash judged after writing; the commit word stays erased unless
    /// this came from the final `Door::open`.
    Readback(Verdict),
    /// The staged file failed a read mid-copy.
    Io,
}

/// Writes `staged` into bank 2: judge it, erase S0 first, body from 0x100
/// up, header words 0x00..0xDF, judge flash uncommitted, commit word last,
/// then open. Stopped anywhere short of the commit, bank 2 is never `Good`
/// for `want`: its commit word is erased or torn, or, S0's erase not begun,
/// the old image is untouched. `done < body_len <= BANK2_LEN - HEADER_LEN`
/// (a `Good` header) bounds the offsets.
pub fn install(
    flash: &mut impl Flash,
    staged: &mut impl ImageRead,
    want: &Seal,
    buf: &mut [u8; 512],
) -> Result<Header, InstallFail> {
    let h = match judge(staged, want) {
        Verdict::Good(h) => h,
        v => return Err(InstallFail::Staged(v)),
    };
    let p = plan(&h);
    for &s in p.sectors {
        flash
            .erase(s)
            .map_err(|f| InstallFail::Flash(f, FlashAt::Sector(s)))?;
    }
    let mut done = 0u32;
    while done < h.body_len {
        let n = (h.body_len - done).min(buf.len() as u32);
        let off = HEADER_LEN + done;
        let block = &mut buf[..n as usize];
        staged.read(off, block).map_err(|_| InstallFail::Io)?;
        program_words(flash, off, block)?;
        done += n;
    }
    let hb = h.to_bytes(false);
    program_words(flash, 0, &hb[..COMMIT_OFF as usize])?;
    match judge_uncommitted(flash, want) {
        Verdict::Good(r) if r == h => {}
        Verdict::Good(_) => return Err(InstallFail::Readback(Verdict::Corrupt)),
        v => return Err(InstallFail::Readback(v)),
    }
    let committed = h.to_bytes(true);
    program_words(flash, COMMIT_OFF, &committed[COMMIT_OFF as usize..])?;
    Door::open(flash, want).map_err(InstallFail::Readback)?;
    Ok(h)
}

/// `data` at bank-2 offset `off`, word by word; erased words are skipped.
fn program_words(flash: &mut impl Flash, off: u32, data: &[u8]) -> Result<(), InstallFail> {
    let mut addr = BANK2_BASE + off;
    for w in data.as_chunks::<32>().0 {
        if w.iter().any(|&b| b != 0xFF) {
            let at = Bank2Word::new(addr).ok_or(InstallFail::Readback(Verdict::Corrupt))?;
            flash
                .program(at, w)
                .map_err(|f| InstallFail::Flash(f, FlashAt::Word(at)))?;
        }
        addr = addr.saturating_add(FLASH_WORD);
    }
    Ok(())
}

/// RTC_BKP14R after a double ECC error read from S0-S3: the HardFault
/// handler's mark before it resets. The backup domain survives the reset.
pub const BANK2_BAD: u32 = 0xB2BA_D0EC;
/// RTC_BKP14R while the boot installs the staged file: cleared when the
/// install ends, kept by a fault during it.
pub const BANK2_INSTALLING: u32 = 0xB21A_57A1;

/// What the boot does with bank 2 before reading it, by BKP14R.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAction {
    Judge,
    /// A word faulted: erase S0 (erase clears its ECC); bank 2 is then
    /// `Empty` by construction, never read; reinstall.
    EraseS0ThenJudge,
    /// The last boot's install never ended: erase S0, read nothing, install
    /// nothing this boot, so a word that faults again costs one more reset.
    EraseS0NoInstall,
}

impl BootAction {
    /// The staged file may be installed this boot.
    pub fn may_install(self) -> bool {
        self != BootAction::EraseS0NoInstall
    }

    /// After S0's erase succeeded: the verdict the boot's install may go
    /// over, `Empty` without a read (a word that faults even after its
    /// erase can't fault this boot), or `None`.
    pub fn after_erase(self) -> Option<Verdict> {
        self.may_install().then_some(Verdict::Empty)
    }
}

pub fn boot_action(mark: u32) -> BootAction {
    match mark {
        BANK2_BAD => BootAction::EraseS0ThenJudge,
        BANK2_INSTALLING => BootAction::EraseS0NoInstall,
        _ => BootAction::Judge,
    }
}

/// BKP14R's word after a bank-2 ECC fault, from what it held.
pub const fn fault_mark(before: u32) -> u32 {
    if before == BANK2_INSTALLING {
        BANK2_INSTALLING
    } else {
        BANK2_BAD
    }
}

/// SCB_CFSR's bus-fault bits: IBUSERR, PRECISERR, IMPRECISERR.
const CFSR_BUS: u32 = 0b111 << 8;
/// FLASH_ECC_FA2R's FAIL_ECC_ADDR2[14:0]: the faulting 256-bit word's index
/// in bank 2 (RM0433 § FLASH_ECC_FA2R; 2^15 words span the 1 MiB bank).
const FA_WORD: u32 = 0x7fff;

/// A fault that a double ECC error in S0-S3 explains: a bus fault (CFSR),
/// SR2.DBECCERR, and FA2R's word inside S0-S3. Anything else is not
/// bank 2's to clear.
pub const fn bank2_ecc_fault(cfsr: u32, sr2: u32, fa2r: u32) -> bool {
    cfsr & CFSR_BUS != 0
        && sr2 & crate::regs::SR_DBECCERR != 0
        && fa2r & FA_WORD < BANK2_LEN / FLASH_WORD
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoInstall {
    Install,
    Skip,
}

/// Both judged against this build's seal: install a good staged file unless
/// flash already holds it.
pub fn auto_install(flash: &Verdict, staged: &Verdict) -> AutoInstall {
    match staged {
        Verdict::Good(_) if wants_staged(flash) => AutoInstall::Install,
        _ => AutoInstall::Skip,
    }
}

/// `auto_install` could answer `Install` over `flash`: otherwise the staged
/// file needn't be read at all.
pub fn wants_staged(flash: &Verdict) -> bool {
    !matches!(flash, Verdict::Good(_))
}
