//! Installing an image into bank 2 so that a power cut at any instant leaves
//! it closed: S0 erased first, the commit word programmed last.

use crate::header::{BANK2_BASE, BANK2_LEN, COMMIT_OFF, FLASH_WORD, HEADER_LEN, Header, Seal};
use crate::judge::{Bank2Memory, Door, ImageRead, Verdict, judge, judge_uncommitted};
use crate::slot::{Evidence, Slot, SlotPlan, SlotSet, own_words};

const SECTOR_LEN: u32 = 128 * 1024;
const _: () = assert!(8 * SECTOR_LEN == BANK2_LEN);

/// A bank-2 sector Chimera may erase. S4..S7 hold the slots (ADR 0272).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank2Sector {
    S0,
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
}

const SECTORS: [Bank2Sector; 8] = [
    Bank2Sector::S0,
    Bank2Sector::S1,
    Bank2Sector::S2,
    Bank2Sector::S3,
    Bank2Sector::S4,
    Bank2Sector::S5,
    Bank2Sector::S6,
    Bank2Sector::S7,
];

impl Bank2Sector {
    pub const fn index(self) -> u8 {
        self as u8
    }

    pub const fn addr(self) -> u32 {
        BANK2_BASE + self.index() as u32 * SECTOR_LEN
    }
}

/// A flash-word address inside S0..=S7, 32-byte aligned.
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

/// One step before the body: an erase, or one of a slot's three words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanOp {
    Erase(Bank2Sector),
    Program(Bank2Word, [u8; 32]),
}

const PLAN_MAX: usize = 4 + 2 * 5;

/// The erases and slot words, in order, before the body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    ops: [PlanOp; PLAN_MAX],
    len: usize,
}

impl Plan {
    pub fn ops(&self) -> &[PlanOp] {
        &self.ops[..self.len]
    }

    fn push(&mut self, op: PlanOp) {
        self.ops[self.len] = op;
        self.len += 1;
    }
}

/// S0 first (it holds the commit word), up to S3 as the body spans them;
/// then MIXER, then EDIT, each in the order its evidence asks (spec § 3.1):
/// one clean tag or marker survives at every instant, and a sector the
/// guard read erased is never erased again.
pub fn plan(h: &Header, slots: &SlotPlan) -> Plan {
    let mut p = Plan {
        ops: [PlanOp::Erase(Bank2Sector::S0); PLAN_MAX],
        len: 0,
    };
    let body = h.body_len.min(BANK2_LEN - HEADER_LEN);
    let last = ((HEADER_LEN + body - 1) / SECTOR_LEN).min(3) as usize;
    for &s in &SECTORS[..=last] {
        p.push(PlanOp::Erase(s));
    }
    for s in Slot::ALL {
        slot_ops(&mut p, s, slots.evidence(s));
    }
    p
}

/// The entry half (S4 erase, tag, code) and the end half (S5 erase,
/// marker): the half holding the live evidence goes last.
fn slot_ops(p: &mut Plan, s: Slot, e: Evidence) {
    let (s4_erased, s5_erased, end_first) = match e {
        Evidence::Tag => (false, false, true),
        Evidence::Marker | Evidence::Mark => (false, false, false),
        // An erased sector takes its words before anything is erased.
        Evidence::Scan {
            s4_erased,
            s5_erased,
        } => (s4_erased, s5_erased, s5_erased),
    };
    let (entry, end) = s.sectors();
    // Slot words sit inside S4..S7, 32-byte aligned (slot_test).
    let [code, tag, marker] = own_words(s).map(|(a, w)| PlanOp::Program(Bank2Word(a), w));
    let halves = [
        (entry, s4_erased, [Some(tag), Some(code)]),
        (end, s5_erased, [Some(marker), None]),
    ];
    let order = if end_first { [1, 0] } else { [0, 1] };
    for i in order {
        let (sector, erased, words) = halves[i];
        if !erased {
            p.push(PlanOp::Erase(sector));
        }
        words.into_iter().flatten().for_each(|w| p.push(w));
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

/// What `install_with` is doing, for the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Erasing,
    Writing,
    Sealing,
}

/// Points inside an install where a power cut leaves a known state: S0
/// erased, a slot's code word written, half the body written, all but the
/// commit word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    EraseStarted,
    SlotWritten(Slot),
    HalfBody,
    BeforeCommit,
}

/// Told between flash ops; it never touches the flash.
pub trait Progress {
    fn phase(&mut self, p: Phase);
    fn mark(&mut self, m: Mark);
}

impl Progress for () {
    fn phase(&mut self, _: Phase) {}
    fn mark(&mut self, _: Mark) {}
}

/// Writes `staged` into bank 2: judge it, erase S0 first, then S1-S3 and
/// the slots as `slots` (the guard's, run first) orders them, body from
/// 0x100 up bar the six slot words, header words 0x00..0xDF, judge flash
/// uncommitted, commit word last, then open. Stopped anywhere short of the commit, bank 2 is never `Good`
/// for `want`: its commit word is erased or torn, or, S0's erase not begun,
/// the old image is untouched. `done < body_len <= BANK2_LEN - HEADER_LEN`
/// (a `Good` header) bounds the offsets. `progress` hears each phase
/// before it starts, and each `Mark` once reached: after S0's erase, after
/// each slot's code word, after the block holding the body's middle word,
/// before the commit word.
pub fn install_with(
    flash: &mut impl Flash,
    staged: &mut impl ImageRead,
    want: &Seal,
    buf: &mut [u8; 512],
    progress: &mut impl Progress,
    slots: &SlotPlan,
) -> Result<Header, InstallFail> {
    let h = match judge(staged, want) {
        Verdict::Good(h) => h,
        v => return Err(InstallFail::Staged(v)),
    };
    progress.phase(Phase::Erasing);
    for &op in plan(&h, slots).ops() {
        match op {
            PlanOp::Erase(s) => {
                flash
                    .erase(s)
                    .map_err(|f| InstallFail::Flash(f, FlashAt::Sector(s)))?;
                if s == Bank2Sector::S0 {
                    progress.mark(Mark::EraseStarted);
                }
            }
            PlanOp::Program(w, data) => {
                flash
                    .program(w, &data)
                    .map_err(|f| InstallFail::Flash(f, FlashAt::Word(w)))?;
                if let Some(s) = Slot::ALL.into_iter().find(|s| s.base() == w.get()) {
                    progress.mark(Mark::SlotWritten(s));
                }
            }
        }
    }
    progress.phase(Phase::Writing);
    let half = h.body_len / 2 / FLASH_WORD * FLASH_WORD;
    let mut done = 0u32;
    while done < h.body_len {
        let n = (h.body_len - done).min(buf.len() as u32);
        let off = HEADER_LEN + done;
        let block = &mut buf[..n as usize];
        staged.read(off, block).map_err(|_| InstallFail::Io)?;
        program_words(flash, off, block)?;
        if (done..done + n).contains(&half) {
            progress.mark(Mark::HalfBody);
        }
        done += n;
    }
    progress.phase(Phase::Sealing);
    let hb = h.to_bytes(false);
    program_words(flash, 0, &hb[..COMMIT_OFF as usize])?;
    match judge_uncommitted(flash, want) {
        Verdict::Good(r) if r == h => {}
        Verdict::Good(_) => return Err(InstallFail::Readback(Verdict::Corrupt)),
        v => return Err(InstallFail::Readback(v)),
    }
    progress.mark(Mark::BeforeCommit);
    let committed = h.to_bytes(true);
    program_words(flash, COMMIT_OFF, &committed[COMMIT_OFF as usize..])?;
    Door::open(flash, want).map_err(InstallFail::Readback)?;
    Ok(h)
}

/// `data` at bank-2 offset `off`, word by word; erased words and the six
/// slot words (the plan's, and the read-back CRC checks them) are skipped.
fn program_words(flash: &mut impl Flash, off: u32, data: &[u8]) -> Result<(), InstallFail> {
    let mut addr = BANK2_BASE + off;
    for w in data.as_chunks::<32>().0 {
        let slot_word = Slot::ALL
            .into_iter()
            .any(|s| [s.base(), s.tag_addr(), s.marker_addr()].contains(&addr));
        if !slot_word && w.iter().any(|&b| b != 0xFF) {
            let at = Bank2Word::new(addr).ok_or(InstallFail::Readback(Verdict::Corrupt))?;
            flash
                .program(at, w)
                .map_err(|f| InstallFail::Flash(f, FlashAt::Word(at)))?;
        }
        addr = addr.saturating_add(FLASH_WORD);
    }
    Ok(())
}

/// RTC_BKP14R after a double ECC error read from bank 2: the HardFault
/// handler's mark before it resets. The backup domain survives the reset.
pub const BANK2_BAD: u32 = 0xB2BA_D0EC;
/// RTC_BKP14R while the boot installs the staged file: cleared when the
/// install ends, kept by a fault during it.
pub const BANK2_INSTALLING: u32 = 0xB21A_57A1;

/// RTC_BKP14R while the guard reads slots (the § 3.2 fallback): this base
/// with the slots' bits in its low two. A fault keeps it, and the next boot
/// takes those slots as ours, torn, and installs over them.
pub const SLOT_TORN: u32 = 0xB25C_0700;
const SLOT_BITS: u32 = 3;
const _: () = assert!(SLOT_TORN & SLOT_BITS == 0);
const _: () = assert!(BANK2_BAD & !SLOT_BITS != SLOT_TORN);
const _: () = assert!(BANK2_INSTALLING & !SLOT_BITS != SLOT_TORN);

/// BKP14R's slot mark for `torn`.
pub const fn slot_mark(torn: SlotSet) -> u32 {
    SLOT_TORN | torn.bits() as u32
}

/// The slots a slot mark names; `None` for any other word.
pub fn slot_mark_set(m: u32) -> Option<SlotSet> {
    (m & !SLOT_BITS == SLOT_TORN).then(|| SlotSet::from_bits((m & SLOT_BITS) as u8))
}

/// The mark the boot writes before the guard reads `reading`'s words, on a
/// boot that took `torn` as ours: a fault keeps both, so the bits only grow
/// and a fault on each slot in turn ends with nothing left to read. The
/// boot puts `BANK2_INSTALLING` back once the guard returns.
pub const fn guard_marks(torn: SlotSet, reading: Slot) -> u32 {
    slot_mark(torn.with(reading))
}

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
    /// A guard read faulted: no erase, judge as normal, install allowed,
    /// and the guard takes these slots as ours unread.
    InstallSlotsOurs(SlotSet),
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
        match self {
            BootAction::EraseS0ThenJudge => Some(Verdict::Empty),
            _ => None,
        }
    }
}

pub fn boot_action(mark: u32) -> BootAction {
    match mark {
        BANK2_BAD => BootAction::EraseS0ThenJudge,
        BANK2_INSTALLING => BootAction::EraseS0NoInstall,
        m => match slot_mark_set(m) {
            Some(torn) => BootAction::InstallSlotsOurs(torn),
            None => BootAction::Judge,
        },
    }
}

/// BKP14R's word after a bank-2 ECC fault, from what it held: the install's
/// mark and a slot mark stay, anything else becomes `BANK2_BAD`.
pub const fn fault_mark(before: u32) -> u32 {
    if before == BANK2_INSTALLING || before & !SLOT_BITS == SLOT_TORN {
        before
    } else {
        BANK2_BAD
    }
}

/// SCB_CFSR's bus-fault bits: IBUSERR, PRECISERR, IMPRECISERR.
const CFSR_BUS: u32 = 0b111 << 8;
/// FLASH_ECC_FA2R's FAIL_ECC_ADDR2[14:0]: the faulting 256-bit word's index
/// in bank 2 (RM0433 § FLASH_ECC_FA2R; 2^15 words span the 1 MiB bank).
const FA_WORD: u32 = 0x7fff;

/// A fault that a double ECC error in bank 2 explains: a bus fault (CFSR),
/// SR2.DBECCERR, and FA2R's word inside the bank (every index is, now that
/// it spans S0-S7). Anything else is not bank 2's to clear.
#[allow(
    clippy::bad_bit_mask,
    reason = "always true at 1 MB; kept so a shorter bank re-arms it"
)]
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

/// Of staged files judged in the boot's order, the first `Good`: its place
/// and header. Lazy: nothing after it is judged.
pub fn pick_staged(verdicts: impl IntoIterator<Item = Verdict>) -> Option<(usize, Header)> {
    verdicts.into_iter().enumerate().find_map(|(i, v)| match v {
        Verdict::Good(h) => Some((i, h)),
        _ => None,
    })
}
