//! Bank 2's phase 0b spike (`bank2-spike` builds only; DEVELOPER, SPIKE):
//! erase bank-2 sector 0, program a test image into it, call the code in
//! it. The address type, the image, the register values and the answers'
//! lines live here; the shell only touches the registers and the flash.
//! RM0433 citations: the bank-2 design's Phase 0b run sheet.

use core::fmt::{self, Write};

use super::{write_cr, write_sr};

/// Bank-2 sector 0, the only sector the spike can address.
pub const SECTOR0: u32 = 0x0810_0000;
pub const SECTOR_LEN: u32 = 128 * 1024;
/// One past sector 0's last byte: sector 1 starts here.
pub const SECTOR0_END: u32 = SECTOR0 + SECTOR_LEN;
/// A flash word: 256 bits, the unit of programming and of ECC.
pub const FLASH_WORD: u32 = 32;

// Sector 0 is bank 2's first, clear of bank 1 (which ends where bank 2
// starts) and of the bootloader's slots at 0x08180000+ (sectors 4-7).
const _: () = assert!(SECTOR0 == 0x0810_0000 && SECTOR0_END == 0x0812_0000);
const _: () = assert!(SECTOR0_END <= 0x0818_0000);

/// A flash word's address inside bank-2 sector 0, 32-byte aligned. Only
/// `new` and `at` make one, and both refuse anything outside the sector:
/// bank 1, the bootloader and sectors 1-7 can't be named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bank2Sector0Addr(u32);

impl Bank2Sector0Addr {
    pub const fn new(addr: u32) -> Option<Self> {
        if addr >= SECTOR0 && addr < SECTOR0_END && addr.is_multiple_of(FLASH_WORD) {
            Some(Bank2Sector0Addr(addr))
        } else {
            None
        }
    }

    /// The word `offset` bytes into the sector. Used in a `const`, an
    /// offset outside the sector or off a word boundary fails the build.
    pub const fn at(offset: u32) -> Self {
        let Some(addr) = SECTOR0.checked_add(offset) else {
            panic!("offset overflows")
        };
        match Self::new(addr) {
            Some(a) => a,
            None => panic!("not a flash word in bank-2 sector 0"),
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The test image's pattern words, before the code's word.
pub const PATTERN_WORDS: usize = 4;
pub const IMAGE_WORDS: usize = PATTERN_WORDS + 1;
/// `adds r0, r0, #42` (T1 ADDS Rdn,#imm8: 0x302a), `bx lr` (0x4770):
/// returns its argument plus `CODE_ADDS`. Position independent.
pub const CODE: [u8; 4] = [0x2a, 0x30, 0x70, 0x47];
pub const CODE_ADDS: u32 = 42;
/// `b2call`'s argument, and the answer it must get.
pub const CALL_ARG: u32 = 1;
pub const CALL_WANT: u32 = CALL_ARG + CODE_ADDS;

/// Image word `k`'s address.
pub const fn word_at(k: usize) -> Bank2Sector0Addr {
    Bank2Sector0Addr::at(k as u32 * FLASH_WORD)
}

pub const CODE_AT: Bank2Sector0Addr = word_at(PATTERN_WORDS);
const _: () = assert!(word_at(IMAGE_WORDS - 1).get() + FLASH_WORD <= SECTOR0_END);

/// Bytes into one flash word, little-endian, padded with 0xFF (erased:
/// programming it changes nothing).
pub const fn pack(bytes: &[u8]) -> [u32; 8] {
    let mut w = [u32::MAX; 8];
    let mut i = 0;
    while i < bytes.len() && i < 32 {
        let shift = (i % 4) * 8;
        w[i / 4] = (w[i / 4] & !(0xff << shift)) | ((bytes[i] as u32) << shift);
        i += 1;
    }
    w
}

/// Pattern word `k`: each 32-bit word names its place, odd ones
/// inverted, so every bit is programmed 0 somewhere and a misplaced word
/// shows.
const fn pattern(k: usize) -> [u32; 8] {
    let mut w = [0; 8];
    let mut j = 0;
    while j < 8 {
        let v = 0xb2c0_0000 | ((k as u32) << 8) | j as u32;
        w[j] = if j % 2 == 1 { !v } else { v };
        j += 1;
    }
    w
}

/// The image: `PATTERN_WORDS` pattern words, then `CODE` in its own word.
pub const IMAGE: [[u32; 8]; IMAGE_WORDS] = {
    let mut img = [[0; 8]; IMAGE_WORDS];
    let mut k = 0;
    while k < PATTERN_WORDS {
        img[k] = pattern(k);
        k += 1;
    }
    img[PATTERN_WORDS] = pack(&CODE);
    img
};

/// The words read back, against the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readback {
    /// Every word 0xFFFFFFFF.
    Erased,
    Matches,
    /// The first word that differs from the image.
    Differs {
        word: u8,
        index: u8,
        got: u32,
        want: u32,
    },
}

pub fn compare(got: &[[u32; 8]; IMAGE_WORDS]) -> Readback {
    if got.iter().flatten().all(|&x| x == u32::MAX) {
        return Readback::Erased;
    }
    (0u8..)
        .zip(got.iter().zip(&IMAGE))
        .flat_map(|(k, (g, w))| (0u8..).zip(g.iter().zip(w)).map(move |(j, p)| (k, j, p)))
        .find(|(_, _, (g, w))| g != w)
        .map_or(Readback::Matches, |(word, index, (&got, &want))| {
            Readback::Differs {
                word,
                index,
                got,
                want,
            }
        })
}

/// The code's word holds exactly `CODE` and its padding: only then may
/// `b2call` call it.
pub fn code_ok(word: &[u32; 8]) -> bool {
    *word == IMAGE[PATTERN_WORDS]
}

// FLASH_CRx bits (RM0433 FLASH_CR1/2).
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

// FLASH_SRx bits; FLASH_CCRx clears each at the same position.
pub const SR_BSY: u32 = 1 << 0;
pub const SR_WBNE: u32 = 1 << 1;
pub const SR_QW: u32 = 1 << 2;
const SR_EOP: u32 = 1 << 16;
/// WRPERR, PGSERR, STRBERR, INCERR, OPERR, RDPERR, RDSERR, SNECCERR,
/// DBECCERR.
pub const SR_ERRORS: u32 = (0b111 << 17) | (0x3f << 21);
/// The CCRx word that clears every error flag and EOP.
pub const CCR_CLEAR: u32 = SR_ERRORS | SR_EOP;

/// The `cr` with sector 0's erase set up, START not yet: SER, SNB 0,
/// PSIZE ×32; PG, BER, FW and START clear; the rest kept.
pub const fn erase_cr(cr: u32) -> u32 {
    (cr & !(CR_PG | CR_BER | CR_PSIZE | CR_FW | CR_START | CR_SNB)) | CR_SER | (PSIZE_X32 << 4)
}

/// The `cr` set up for programming: PG, PSIZE ×32; SER, BER, FW and
/// START clear; the rest kept.
pub const fn program_cr(cr: u32) -> u32 {
    (cr & !(CR_SER | CR_BER | CR_PSIZE | CR_FW | CR_START | CR_SNB)) | CR_PG | (PSIZE_X32 << 4)
}

/// An operation runs or a word waits: BSY, QW or WBNE.
pub const fn busy(sr: u32) -> bool {
    sr & (SR_BSY | SR_QW | SR_WBNE) != 0
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

/// Why a flash run stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    /// BSY, QW or WBNE already set: nothing started.
    Busy,
    /// LOCK still set after the key sequence.
    Locked,
    /// Erase only: the sector was not erased before programming.
    NotErased,
    /// An SR error flag.
    Errors,
    /// The deadline passed with the operation still running.
    Timeout,
}

impl Fail {
    const fn message(self) -> &'static str {
        match self {
            Fail::Busy => "flash busy, nothing started",
            Fail::Locked => "cr2 still locked after the keys",
            Fail::NotErased => "sector 0 not erased, b2erase first",
            Fail::Errors => "flash error",
            Fail::Timeout => "timeout",
        }
    }
}

/// After a poll: an error flag wins, then a deadline passed while busy.
/// `None`: done.
pub const fn settle(sr: u32, timed_out: bool) -> Option<Fail> {
    if sr & SR_ERRORS != 0 {
        Some(Fail::Errors)
    } else if busy(sr) {
        if timed_out { Some(Fail::Timeout) } else { None }
    } else {
        None
    }
}

/// One erase or program, as it ended: SR2 read before the relock, CR2
/// after it (LOCK shows the relock), and the cycles it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub cr: u32,
    pub sr: u32,
    pub cycles: u32,
    pub fail: Option<Fail>,
}

/// The audio's counters before and after the command: overruns, and
/// blocks rendered (they moved: the audio ran).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Audio {
    pub overruns: [u32; 2],
    pub blocks: [u32; 2],
}

/// What the spike did, reached before its answer starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum B2Answer {
    Erase {
        run: Run,
        audio: Audio,
        cpu_hz: u32,
    },
    Prog {
        run: Run,
        audio: Audio,
        cpu_hz: u32,
        readback: Readback,
    },
    /// `Ok`: the code's word matched and the call returned this.
    /// `Err`: it didn't match; nothing called.
    Call(Result<u32, Readback>),
    Stat {
        cr: u32,
        sr: u32,
        words: [[u32; 8]; IMAGE_WORDS],
    },
}

fn write_audio(a: &Audio, w: &mut impl Write) -> fmt::Result {
    let [o0, o1] = a.overruns;
    writeln!(
        w,
        "audio overruns {o0} -> {o1} (+{}) blocks +{}",
        o1.wrapping_sub(o0),
        a.blocks[1].wrapping_sub(a.blocks[0])
    )
}

fn write_run(run: &Run, cpu_hz: u32, w: &mut impl Write) -> fmt::Result {
    let ms = u64::from(run.cycles) * 1000 / u64::from(cpu_hz.max(1));
    writeln!(w, "elapsed_ms {ms} cycles {}", run.cycles)?;
    write_cr(2, run.cr, w)?;
    write_sr("sr2", run.sr, w)
}

fn write_readback(r: &Readback, w: &mut impl Write) -> fmt::Result {
    match *r {
        Readback::Erased => w.write_str("readback erased\n"),
        Readback::Matches => w.write_str("readback matches\n"),
        Readback::Differs {
            word,
            index,
            got,
            want,
        } => writeln!(
            w,
            "readback differs word {word} index {index} at {:08x} got {got:08x} want {want:08x}",
            word_at(usize::from(word)).get() + 4 * u32::from(index)
        ),
    }
}

/// The spike's lines. `Ok(Some(why))`: the answer ends `ERR <cmd>: why`.
pub fn write_b2(a: &B2Answer, w: &mut impl Write) -> Result<Option<&'static str>, fmt::Error> {
    let (start, end) = (word_at(0).get(), CODE_AT.get() + FLASH_WORD);
    match a {
        B2Answer::Erase { run, audio, cpu_hz } => {
            writeln!(
                w,
                "b2erase sector 0 {SECTOR0:08x}..{:08x} psize {PSIZE_X32} timeout_ms {ERASE_TIMEOUT_MS}",
                SECTOR0_END - 1
            )?;
            write_run(run, *cpu_hz, w)?;
            write_audio(audio, w)?;
            Ok(run.fail.map(Fail::message))
        }
        B2Answer::Prog {
            run,
            audio,
            cpu_hz,
            readback,
        } => {
            writeln!(
                w,
                "b2prog words {IMAGE_WORDS} {start:08x}..{:08x} code at {:08x} psize {PSIZE_X32} \
                 word_timeout_ms {WORD_TIMEOUT_MS}",
                end - 1,
                CODE_AT.get()
            )?;
            write_run(run, *cpu_hz, w)?;
            write_readback(readback, w)?;
            write_audio(audio, w)?;
            Ok(match (run.fail, readback) {
                (Some(f), _) => Some(f.message()),
                (None, Readback::Matches) => None,
                (None, _) => Some("readback differs from the image"),
            })
        }
        B2Answer::Call(Err(r)) => {
            writeln!(w, "b2call code at {:08x}: not called", CODE_AT.get())?;
            write_readback(r, w)?;
            Ok(Some("the code's word differs, not called"))
        }
        B2Answer::Call(Ok(got)) => {
            writeln!(
                w,
                "b2call {:08x} f({CALL_ARG}) = {got} want {CALL_WANT}",
                CODE_AT.get() | 1
            )?;
            Ok((*got != CALL_WANT).then_some("wrong return value"))
        }
        B2Answer::Stat { cr, sr, words } => {
            write_cr(2, *cr, w)?;
            write_sr("sr2", *sr, w)?;
            for (k, ws) in words.iter().enumerate() {
                write!(w, "peek {:08x}", word_at(k).get())?;
                ws.iter().try_for_each(|x| write!(w, " {x:08x}"))?;
                w.write_char('\n')?;
            }
            write_readback(&compare(words), w)?;
            Ok(None)
        }
    }
}
