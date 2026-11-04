//! A statistical profiler for the bench (`pc-sample`, #793): TIM6 fires
//! about every 2,400 timer clocks and records the PC it interrupted, while
//! `arm` holds. The bench runs in thread mode with no other interrupt
//! unmasked, so every sample lands in the code it times. Bank 1, so the
//! vector table's entry needs no door.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use stm32h7xx_hal::pac;

pub const N: usize = 2048;
/// Timer clocks between samples: not a divisor of a block's cycles, so
/// samples walk through the block instead of hitting one spot.
const PERIOD: u32 = 2_399;

// In D2 (NOLOAD, unzeroed): a sample is written before it is read.
#[unsafe(link_section = ".ram_d2.voices")]
static mut SAMPLES: [u32; N] = [0; N];
static TAKEN: AtomicUsize = AtomicUsize::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);

// The vector's entry: the exception frame is at MSP (the bench runs on
// MSP), its stacked PC 24 B in; r7, the frame pointer, as interrupted.
core::arch::global_asm!(
    ".section .text.TIM6_DAC,\"ax\",%progbits",
    ".global TIM6_DAC",
    ".type TIM6_DAC,%function",
    ".thumb_func",
    "TIM6_DAC:",
    "mrs r0, msp",
    "mov r1, r7",
    "b {f}",
    f = sym sample,
);

unsafe extern "C" {
    fn __aeabi_memclr();
    fn __aeabi_memclr4();
    fn __aeabi_memclr8();
    fn __aeabi_memcpy();
    fn __aeabi_memcpy4();
    fn __aeabi_memcpy8();
    fn __aeabi_memset();
    fn __aeabi_memset4();
    fn __aeabi_memset8();
}

/// The `mem*` builtins' span, give or take the last one's length: a PC in
/// it is charged to its caller instead (`[r7 + 4]`), stored odd (a
/// return address keeps its Thumb bit, a stacked PC never has it).
fn mem_span() -> (u32, u32) {
    let f: [unsafe extern "C" fn(); 9] = [
        __aeabi_memclr,
        __aeabi_memclr4,
        __aeabi_memclr8,
        __aeabi_memcpy,
        __aeabi_memcpy4,
        __aeabi_memcpy8,
        __aeabi_memset,
        __aeabi_memset4,
        __aeabi_memset8,
    ];
    let a = f.map(|f| f as usize as u32 & !1);
    let (lo, hi) = (a.iter().min(), a.iter().max());
    (lo.copied().unwrap_or(0), hi.map_or(0, |h| h + 0xd0))
}

/// The caller of the frame at `fp`, if `fp` looks like RAM.
fn caller(fp: u32) -> Option<u32> {
    let ram = matches!(fp >> 24, 0x20 | 0x24 | 0x30) && fp & 3 == 0;
    // SAFETY: r7 is always the frame pointer in this build (no function
    // uses it as a general register), so a RAM-region `fp` points into a
    // live stack frame; the region check guards only the rare prologue PC.
    ram.then(|| unsafe { ((fp + 4) as *const u32).read_volatile() } | 1)
}

extern "C" fn sample(frame: *const u32, fp: u32) {
    // SAFETY: the bench only steals TIM6 here and in `start`; this clears
    // its update flag, nothing else.
    let tim = unsafe { &*pac::TIM6::ptr() };
    tim.sr.write(|w| w.uif().clear_bit());
    if !ARMED.load(Ordering::Relaxed) {
        return;
    }
    let i = TAKEN.load(Ordering::Relaxed);
    if i < N {
        // SAFETY: `frame` is the exception frame the hardware stacked at
        // MSP; word 6 is the interrupted PC. `SAMPLES[i]` is written only
        // here, and read only once `stop` has disarmed.
        unsafe {
            let mut pc = frame.add(6).read_volatile();
            let (lo, hi) = mem_span();
            if (lo..hi).contains(&pc) {
                pc = caller(fp).unwrap_or(pc);
            }
            (&mut *core::ptr::addr_of_mut!(SAMPLES))[i] = pc;
        }
        TAKEN.store(i + 1, Ordering::Relaxed);
    }
}

/// TIM6 running and unmasked, sampling nothing until `arm`.
pub fn start() {
    // SAFETY: the bench runs alone before `engine::init`; nothing else
    // touches RCC's APB1L enable or TIM6.
    let (rcc, tim) = unsafe { (&*pac::RCC::ptr(), &*pac::TIM6::ptr()) };
    rcc.apb1lenr.modify(|_, w| w.tim6en().set_bit());
    tim.psc.write(|w| w.psc().bits(0));
    tim.arr.write(|w| w.arr().bits(PERIOD as u16));
    tim.dier.write(|w| w.uie().set_bit());
    tim.cr1.write(|w| w.cen().set_bit());
    // SAFETY: `sample` touches only TIM6's flag and this module's statics.
    unsafe { cortex_m::peripheral::NVIC::unmask(pac::Interrupt::TIM6_DAC) };
}

/// TIM6 masked, stopped and unclocked, so later pages and audio run
/// without it.
pub fn stop() {
    disarm();
    cortex_m::peripheral::NVIC::mask(pac::Interrupt::TIM6_DAC);
    // SAFETY: as in `start`; the interrupt is masked, so `sample` no longer
    // runs.
    let (rcc, tim) = unsafe { (&*pac::RCC::ptr(), &*pac::TIM6::ptr()) };
    tim.cr1.write(|w| w.cen().clear_bit());
    tim.sr.write(|w| w.uif().clear_bit());
    rcc.apb1lenr.modify(|_, w| w.tim6en().clear_bit());
    cortex_m::peripheral::NVIC::unpend(pac::Interrupt::TIM6_DAC);
}

/// Samples from empty while armed.
pub fn arm() {
    TAKEN.store(0, Ordering::Relaxed);
    ARMED.store(true, Ordering::Release);
}

pub fn disarm() {
    ARMED.store(false, Ordering::Release);
}

/// The samples taken since `arm`, sorted. Only while disarmed.
pub fn taken() -> &'static mut [u32] {
    disarm();
    cortex_m::asm::dsb();
    let n = TAKEN.load(Ordering::Acquire);
    // SAFETY: disarmed, so `sample` no longer writes `SAMPLES`; the bench
    // holds this slice only until its next `arm`.
    let s = unsafe { &mut (&mut *core::ptr::addr_of_mut!(SAMPLES))[..n] };
    s.sort_unstable();
    s
}
