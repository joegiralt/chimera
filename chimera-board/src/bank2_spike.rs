//! The bank-2 spike's shell (`bank2-spike`; DEVELOPER, SPIKE): erases,
//! programs and calls in bank-2 sector 0 through bank 2's own key,
//! control, status and clear registers, nothing else. The addresses,
//! register values and verdicts are `chimera_core::console::bank2`'s.
//!
//! It runs from the UI loop, inside the console's `service`, before the
//! answer starts. The audio interrupt and the controls tick keep running
//! from bank 1 meanwhile (RM0433 read-while-write: a bank-2 erase or
//! program does not stall bank-1 reads), so the watchdog stays kicked by
//! the tick, not by this loop. Nothing linked lives in bank 2 (memory.x
//! ends FLASH at 0x08100000), so no interrupt reads it while it is busy.
//! MPU region 2 is `Write` (uncached) for an erase or program, `Open` for
//! the call (`set_mpu` invalidates the whole caches into it); `Read` is
//! uncached, so no read here needs cache maintenance.

use core::sync::atomic::Ordering;

use chimera_core::console::{
    Audio, B2Answer, CALL_ARG, CCR_CLEAR, CODE_AT, CR_LOCK, CR_PG, CR_SER, CR_START,
    ERASE_TIMEOUT_MS, Fail, IMAGE, IMAGE_WORDS, PATTERN_WORDS, Readback, Refusal, Request, Run,
    WORD_TIMEOUT_MS, busy, code_ok, compare, erase_cr, ms_cycles, program_cr, settle, word_at,
};
use cortex_m::peripheral::DWT;
use stm32h7xx_hal::pac;

use crate::bank2::{Mpu2, mpu, set_mpu};
use crate::shared::{BLOCKS, OVERRUNS};
use chimera_core::bank2::DOOR_CELL;

/// FLASH_KEYR2's unlock sequence (RM0433 FLASH_KEYR).
const KEY1: u32 = 0x4567_0123;
const KEY2: u32 = 0xcdef_89ab;

type Bank = pac::flash::BANK;

fn bank2() -> &'static Bank {
    // SAFETY: FLASH's fixed register block; this module alone writes
    // bank 2's registers, from the UI loop only, and no interrupt touches
    // the flash controller.
    unsafe { (*pac::FLASH::ptr()).bank2() }
}

fn cr() -> u32 {
    bank2().cr.read().bits()
}

fn sr() -> u32 {
    bank2().sr.read().bits()
}

/// CR2 unlocked; dropping it relocks, whatever happened.
struct Unlocked(&'static Bank);

impl Unlocked {
    /// Bank 2 idle, then the key sequence if CR2 is locked.
    fn new() -> Result<Self, Fail> {
        let b = bank2();
        if busy(sr()) {
            return Err(Fail::Busy);
        }
        if cr() & CR_LOCK != 0 {
            for key in [KEY1, KEY2] {
                // SAFETY: KEYR2 takes the two documented keys in order;
                // it unlocks bank 2's CR only, never the option bytes.
                b.keyr.write(|w| unsafe { w.bits(key) });
            }
        }
        let u = Unlocked(b);
        if cr() & CR_LOCK != 0 {
            return Err(Fail::Locked);
        }
        Ok(u)
    }

    fn set_cr(&self, v: u32) {
        // SAFETY: `v` comes from `erase_cr`, `program_cr` or adds START:
        // bank-2 sector 0's erase or programming at ×32, never a bank or
        // mass erase.
        self.0.cr.write(|w| unsafe { w.bits(v) });
        cortex_m::asm::dsb();
    }

    fn clear_flags(&self) {
        // SAFETY: CCR2 only clears SR2's flags, one bit each.
        self.0.ccr.write(|w| unsafe { w.bits(CCR_CLEAR) });
    }
}

impl Drop for Unlocked {
    fn drop(&mut self) {
        let c = cr();
        // A still-running operation keeps SER or PG: only LOCK is set.
        let v = if busy(sr()) {
            c | CR_LOCK
        } else {
            (c & !(CR_PG | CR_SER | CR_START)) | CR_LOCK
        };
        // SAFETY: clears the operation bits and sets LOCK, which only a
        // new key sequence clears.
        self.0.cr.write(|w| unsafe { w.bits(v) });
    }
}

/// Polls SR2 until idle, an error flag, or `limit` cycles from `start`.
fn wait(start: u32, limit: u32) -> (u32, Option<Fail>) {
    loop {
        let s = sr();
        let late = DWT::cycle_count().wrapping_sub(start) > limit;
        match settle(s, late) {
            Some(f) => return (s, Some(f)),
            None if !busy(s) => return (s, None),
            None => {}
        }
    }
}

/// The image's words as the flash holds them now.
fn read_image() -> [[u32; 8]; IMAGE_WORDS] {
    let mut got = [[0; 8]; IMAGE_WORDS];
    for (k, row) in got.iter_mut().enumerate() {
        let at = word_at(k).get();
        for (i, w) in (0u32..).zip(row.iter_mut()) {
            // SAFETY: an aligned word inside bank-2 sector 0 (`word_at`),
            // always mapped; a load never programs or erases.
            *w = unsafe { core::ptr::read_volatile((at + 4 * i) as *const u32) };
        }
    }
    got
}

fn audio() -> [u32; 2] {
    [
        OVERRUNS.load(Ordering::Relaxed),
        BLOCKS.load(Ordering::Relaxed),
    ]
}

/// Sector 0 erased: SER, SNB 0, PSIZE ×32, START, then BSY and QW polled
/// against `ERASE_TIMEOUT_MS`. No retry.
fn erase(cpu_hz: u32) -> Run {
    let start = DWT::cycle_count();
    let (s, fail) = match Unlocked::new() {
        Err(f) => (sr(), Some(f)),
        Ok(u) => {
            u.clear_flags();
            let v = erase_cr(cr());
            u.set_cr(v);
            u.set_cr(v | CR_START);
            wait(start, ms_cycles(ERASE_TIMEOUT_MS, cpu_hz))
        }
    };
    let cycles = DWT::cycle_count().wrapping_sub(start);
    Run {
        cr: cr(),
        sr: s,
        cycles,
        fail,
    }
}

/// The image programmed one 256-bit word at a time, each waited for
/// (WBNE and QW clear) against `WORD_TIMEOUT_MS`; refused unless erased.
fn program(cpu_hz: u32) -> (Run, Readback) {
    let start = DWT::cycle_count();
    let before = compare(&read_image());
    let (s, fail) = if before != Readback::Erased {
        (sr(), Some(Fail::NotErased))
    } else {
        match Unlocked::new() {
            Err(f) => (sr(), Some(f)),
            Ok(u) => {
                u.clear_flags();
                u.set_cr(program_cr(cr()));
                let mut out = (sr(), None);
                for (k, word) in IMAGE.iter().enumerate() {
                    let at = word_at(k).get();
                    for (i, &x) in (0u32..).zip(word) {
                        // SAFETY: an aligned word of a flash word in
                        // bank-2 sector 0 (`word_at`), CR2.PG set: the
                        // store fills bank 2's write buffer, eight make
                        // one 256-bit word.
                        unsafe { core::ptr::write_volatile((at + 4 * i) as *mut u32, x) };
                    }
                    cortex_m::asm::dsb();
                    out = wait(DWT::cycle_count(), ms_cycles(WORD_TIMEOUT_MS, cpu_hz));
                    if out.1.is_some() {
                        break;
                    }
                }
                out
            }
        }
    };
    let cycles = DWT::cycle_count().wrapping_sub(start);
    let run = Run {
        cr: cr(),
        sr: s,
        cycles,
        fail,
    };
    (
        run,
        if fail.is_some() {
            before
        } else {
            compare(&read_image())
        },
    )
}

/// The code called only if its word is exactly the image's.
fn call() -> Result<u32, Readback> {
    let words = read_image();
    if !code_ok(&words[PATTERN_WORDS]) {
        return Err(compare(&words));
    }
    // SAFETY: the word at CODE_AT is, read back just now, exactly `CODE`:
    // `adds r0, r0, #42; bx lr`, which reads r0, writes r0 and the flags,
    // and returns. Thumb bit set; AAPCS argument and result in r0.
    let f: extern "C" fn(u32) -> u32 =
        unsafe { core::mem::transmute((CODE_AT.get() | 1) as usize) };
    Ok(f(CALL_ARG))
}

/// `f` with the door closed and S0-S3 writable, then readable again.
fn writing<R>(f: impl FnOnce() -> R) -> R {
    DOOR_CELL.close();
    set_mpu(Mpu2::Write);
    let r = f();
    set_mpu(Mpu2::Read);
    r
}

/// The spike's command run, with the audio's counters around it. `None`
/// for any other request.
pub fn run(req: &Result<Request, Refusal>, cpu_hz: u32) -> Option<B2Answer> {
    let audio_around = |a: [u32; 2]| {
        let b = audio();
        Audio {
            overruns: [a[0], b[0]],
            blocks: [a[1], b[1]],
        }
    };
    match req {
        Ok(Request::B2erase(_)) => {
            let a = audio();
            let run = writing(|| erase(cpu_hz));
            Some(B2Answer::Erase {
                run,
                audio: audio_around(a),
                cpu_hz,
            })
        }
        Ok(Request::B2prog(_)) => {
            let a = audio();
            let (run, readback) = writing(|| program(cpu_hz));
            Some(B2Answer::Prog {
                run,
                audio: audio_around(a),
                cpu_hz,
                readback,
            })
        }
        Ok(Request::B2call(_)) => {
            let was = mpu();
            set_mpu(Mpu2::Open);
            let r = call();
            set_mpu(was);
            Some(B2Answer::Call(r))
        }
        Ok(Request::B2stat(_)) => Some(B2Answer::Stat {
            cr: cr(),
            sr: sr(),
            words: read_image(),
        }),
        _ => None,
    }
}
