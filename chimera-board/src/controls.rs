//! HC165 shift register control input via SysTick interrupt.
//!
//! Port F on the LQFP176/144 boards, port A on the LQFP100 (ADR 0140):
//! `wire` sets which before `enable`.
//! Polled at 500Hz from SysTick ISR (matching PreenFM3).
//! Quadrature decoding matches PreenFM3 Encoders.cpp exactly.

use chimera_core::board::{Hc165Pins, Port};
use chimera_core::clock_plan::{cycles_for_ns, systick_reload};
use chimera_core::console::Hand;
use chimera_hal::{
    ButtonId, ButtonState, Controls, Debounce, Edges, EncoderId, Latch, Ms, NUM_BUTTONS,
    NUM_ENCODERS, push_levels,
};
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicI8, AtomicU32, AtomicUsize, Ordering};
use cortex_m::peripheral::syst::SystClkSource;
use cortex_m::peripheral::{SCB, SYST};

use crate::priority::{self, Priority};

const GPIOA_BASE: usize = 0x5802_0000;
const GPIOF_BASE: usize = 0x5802_1400;
const IDR: usize = 0x10;
const BSRR: usize = 0x18;

// The HC165's port and pins, as `wire` set them: written before `enable`'s
// Release, read after the ISR's Acquire. Port F's until then.
static HC_BASE: AtomicUsize = AtomicUsize::new(GPIOF_BASE);
static HC_DATA: AtomicU32 = AtomicU32::new(1 << 2);
static HC_LOAD: AtomicU32 = AtomicU32::new(1 << 1);
static HC_CLK: AtomicU32 = AtomicU32::new(1 << 0);

pub const CONTROLS_HZ: u32 = 500;
// The HC165 was clocked with 100-cycle spins at 400 MHz: keep 250 ns at any clock.
const HC165_HALF_PERIOD_NS: u32 = 250;
static HC165_DELAY: AtomicU32 = AtomicU32::new(100);

/// Encoder bit pairs from PreenFM3 (1-indexed pins → 0-indexed masks)
const ENC_BITS: [(u32, u32); NUM_ENCODERS] = [
    (1 << 16, 1 << 17),
    (1 << 14, 1 << 15),
    (1 << 8, 1 << 9),
    (1 << 19, 1 << 18),
    (1 << 13, 1 << 12),
    (1 << 11, 1 << 10),
];

/// Button bit masks from PreenFM3 (1-indexed pins → 0-indexed)
const BTN_BITS: [u32; NUM_BUTTONS] = [
    1 << 22,
    1 << 20,
    1 << 3,
    1 << 23,
    1 << 2,
    1 << 1,
    1 << 21,
    1 << 4,
    1 << 5,
    1 << 6,
    1 << 7,
    1 << 0,
];

/// N24 quadrature table — 1 count per detent click (vs N12 which gives 2)
const QUAD: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0];

/// The twelve buttons, then the six encoders' pushes.
const NUM_SWITCHES: usize = NUM_BUTTONS + NUM_ENCODERS;

// Shared ISR ↔ main state
static READY: AtomicBool = AtomicBool::new(false);
/// SYSTEM's PUSH ENCODERS: the read takes 32 bits, not 24.
static PUSH: AtomicBool = AtomicBool::new(false);
static ENC_DELTA: [AtomicI8; NUM_ENCODERS] = [const { AtomicI8::new(0) }; NUM_ENCODERS];
/// Each switch's edges since the last `snapshot`. The ISR feeds them; the
/// main loop takes them inside `interrupt::free`, so the two never overlap.
static mut LATCHES: [Latch; NUM_SWITCHES] = [Latch::new(); NUM_SWITCHES];
static mut ENC_STATE: [u8; NUM_ENCODERS] = [0; NUM_ENCODERS];
static mut ENC_DEBOUNCE: [u8; NUM_ENCODERS] = [0; NUM_ENCODERS];
static mut DEBOUNCE: [Debounce; NUM_SWITCHES] = [Debounce::new(); NUM_SWITCHES];
static ISR_TICK: AtomicU32 = AtomicU32::new(0);
static ENC_LAST_EDGE: [AtomicU32; NUM_ENCODERS] = [const { AtomicU32::new(0) }; NUM_ENCODERS];
/// The console's hand: switches it holds, one bit each in `LATCHES`'
/// order, read as pressed before the debounce.
static HAND: AtomicU32 = AtomicU32::new(0);

/// Arms SysTick at `CONTROLS_HZ`, its priority set first so the first tick
/// cannot run at the reset priority (0, the audio level).
pub fn start_systick(mut syst: SYST, scb: &mut SCB, cpu_hz: u32) {
    HC165_DELAY.store(
        cycles_for_ns(cpu_hz, HC165_HALF_PERIOD_NS),
        Ordering::Relaxed,
    );
    priority::set_systick(scb, Priority::SYSTICK);
    syst.disable_counter();
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(systick_reload(cpu_hz, CONTROLS_HZ));
    syst.clear_current();
    syst.enable_interrupt();
    syst.enable_counter();
}

/// Points the ISR at the board's HC165 pins. Call before `enable`.
pub fn wire(pins: Hc165Pins) {
    let base = match pins.port {
        Port::A => GPIOA_BASE,
        Port::F => GPIOF_BASE,
    };
    HC_BASE.store(base, Ordering::Relaxed);
    HC_DATA.store(1 << pins.data, Ordering::Relaxed);
    HC_LOAD.store(1 << pins.load, Ordering::Relaxed);
    HC_CLK.store(1 << pins.clk, Ordering::Relaxed);
}

/// SYSTEM's PUSH ENCODERS: off, the read is the 24 bits it always was.
pub fn set_push(on: bool) {
    PUSH.store(on, Ordering::Relaxed);
}

/// Allow ISR to run. Call after the HC165's port is configured and `wire`d.
pub fn enable() {
    READY.store(true, Ordering::Release);
}

/// SysTick ticks since controls started, at `CONTROLS_HZ`: the UI loop's
/// clock for anything timed.
pub fn ticks() -> u32 {
    ISR_TICK.load(Ordering::Relaxed)
}

/// Now, in ms since controls started.
pub fn now_ms() -> Ms {
    Ms(tick_ms(ticks()))
}

/// `tick` in ms. Wrapping, never `tick * 1000 / CONTROLS_HZ`: that
/// overflows `u32` after about 2.4 h.
fn tick_ms(tick: u32) -> u32 {
    tick.wrapping_mul(1000 / CONTROLS_HZ)
}

/// SysTick ISR handler. Reads HC165 and decodes inputs.
///
/// # Safety
/// Only from the SysTick exception, which does not re-enter: it is the one
/// driver of the HC165's LOAD and CLK and the one user of the debounce
/// state, and `snapshot` masks it.
pub unsafe fn isr_tick() {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    let now_ms = Ms(tick_ms(
        ISR_TICK.fetch_add(1, Ordering::Relaxed).wrapping_add(1),
    ));

    let d = HC165_DELAY.load(Ordering::Relaxed);
    // Five loads a tick, then the same register writes as fixed pins.
    let base = HC_BASE.load(Ordering::Relaxed);
    let (idr, bsrr) = ((base + IDR) as *const u32, (base + BSRR) as *mut u32);
    let (data, load, clk) = (
        HC_DATA.load(Ordering::Relaxed),
        HC_LOAD.load(Ordering::Relaxed),
        HC_CLK.load(Ordering::Relaxed),
    );
    let push = cfg!(feature = "push-encoders") && PUSH.load(Ordering::Relaxed);
    let n = if push { 32 } else { 24 };
    // Shift the HC165 chain in: LOAD low then high, then `n` clocks, DATA
    // read while CLK is low. BSRR's top half resets, its bottom half sets.
    // SAFETY: `base` is GPIOA's or GPIOF's fixed register block (`wire`
    // stores no other); IDR and BSRR are its 32-bit registers. The caller
    // is SysTick (this fn's contract), the only one driving LOAD/CLK and
    // reading DATA; BSRR writes touch only those pins.
    let bits = unsafe {
        core::ptr::write_volatile(bsrr, load << 16);
        cortex_m::asm::delay(d);
        core::ptr::write_volatile(bsrr, load);
        cortex_m::asm::delay(d);

        let mut b: u32 = 0;
        for i in 0..n {
            core::ptr::write_volatile(bsrr, clk << 16);
            cortex_m::asm::delay(d);
            if core::ptr::read_volatile(idr) & data != 0 {
                b |= 1 << i;
            }
            core::ptr::write_volatile(bsrr, clk);
            cortex_m::asm::delay(d);
        }
        b
    };

    // Buttons and pushes: active low, debounced, edges latched. Pushes
    // off read released, so one held when they are switched off lets go.
    let pushes = if push {
        push_levels(bits)
    } else {
        [false; NUM_ENCODERS]
    };
    let hand = HAND.load(Ordering::Relaxed);
    let by_hand = |i: usize| hand & (1 << i) != 0;
    // SAFETY: DEBOUNCE is only touched here, from SysTick (this fn's
    // contract), which is not reentrant. LATCHES is shared with
    // `snapshot`, which only touches it inside `interrupt::free`: on this
    // single core that masks SysTick, so the ISR never runs during it, and
    // the main loop cannot run during the ISR. Access is exclusive either
    // way; no ordering beyond that is needed.
    unsafe {
        let latches = &mut *addr_of_mut!(LATCHES);
        let debounce = &mut *addr_of_mut!(DEBOUNCE);
        let raw = BTN_BITS.iter().map(|&m| bits & m == 0).chain(pushes);
        // A push the scan does not read, the hand cannot press either.
        let live = (0..NUM_SWITCHES).map(|i| i < NUM_BUTTONS || push);
        for (i, ((raw, live), (d, latch))) in raw
            .zip(live)
            .zip(debounce.iter_mut().zip(latches.iter_mut()))
            .enumerate()
        {
            latch.level(d.step(raw || (live && by_hand(i))), now_ms);
        }
    }

    // Encoders: quadrature decode with debounce
    for i in 0..NUM_ENCODERS {
        // SAFETY: only accessed from this ISR (single-threaded)
        unsafe {
            if ENC_DEBOUNCE[i] > 0 {
                ENC_DEBOUNCE[i] -= 1;
                continue;
            }

            let (bit1_mask, bit2_mask) = ENC_BITS[i];
            let b1: u8 = if bits & bit1_mask == 0 { 1 } else { 0 };
            let b2: u8 = if bits & bit2_mask == 0 { 1 } else { 0 };
            let cur = b1 | (b2 << 1);

            ENC_STATE[i] <<= 2;
            ENC_STATE[i] &= 0x0F;
            ENC_STATE[i] |= cur;

            let action = QUAD[ENC_STATE[i] as usize];
            if action == 1 {
                ENC_DELTA[i].fetch_add(1, Ordering::Relaxed);
                ENC_LAST_EDGE[i].store(ISR_TICK.load(Ordering::Relaxed), Ordering::Relaxed);
                ENC_DEBOUNCE[i] = 2;
            } else if action == 2 {
                ENC_DELTA[i].fetch_add(-1, Ordering::Relaxed);
                ENC_LAST_EDGE[i].store(ISR_TICK.load(Ordering::Relaxed), Ordering::Relaxed);
                ENC_DEBOUNCE[i] = 2;
            }
        }
    }
}

// --- Main-thread interface ---

/// The console's hand for this lap: its switches into the next scans, its
/// detents where the quadrature decode puts a hand's. Before `snapshot`.
pub fn hand(h: &Hand) {
    HAND.store(h.down, Ordering::Relaxed);
    let now = ISR_TICK.load(Ordering::Relaxed);
    for ((&d, delta), edge) in h.detents.iter().zip(&ENC_DELTA).zip(&ENC_LAST_EDGE) {
        if d != 0 {
            delta.fetch_add(d, Ordering::Relaxed);
            edge.store(now, Ordering::Relaxed);
        }
    }
}

pub struct Stm32Controls {
    /// The buttons', then the pushes'.
    edges: [Edges; NUM_SWITCHES],
    now_ms: Ms,
    enc: [i8; NUM_ENCODERS],
    burst: [u8; NUM_ENCODERS],
}

impl Stm32Controls {
    pub(crate) fn new() -> Self {
        Self {
            edges: [Edges::default(); NUM_SWITCHES],
            now_ms: Ms(0),
            enc: [0; NUM_ENCODERS],
            burst: [0; NUM_ENCODERS],
        }
    }

    /// Read and clear accumulated ISR state. Call once per frame.
    pub fn snapshot(&mut self) {
        cortex_m::interrupt::free(|_| {
            // SAFETY: SysTick is masked here, so the ISR (LATCHES' only
            // other user) cannot run; see `isr_tick`.
            let latches = unsafe { &mut *addr_of_mut!(LATCHES) };
            for (e, latch) in self.edges.iter_mut().zip(latches.iter_mut()) {
                *e = latch.take();
            }
        });
        let now = ISR_TICK.load(Ordering::Relaxed);
        self.now_ms = Ms(tick_ms(now));
        for i in 0..NUM_ENCODERS {
            let raw = ENC_DELTA[i].swap(0, Ordering::Relaxed);
            // A burst ends after the gap, in 500 Hz ticks.
            let last = ENC_LAST_EDGE[i].load(Ordering::Relaxed);
            if now.wrapping_sub(last) > chimera_hal::ENCODER_BURST_GAP_MS / 2 {
                self.burst[i] = 0;
            }
            if raw != 0 {
                let dir: i8 = if raw > 0 { 1 } else { -1 };
                let edges = raw.unsigned_abs();
                let mut total: i8 = 0;
                for _ in 0..edges {
                    total = total.saturating_add(dir * chimera_hal::encoder_accel(self.burst[i]));
                    self.burst[i] = self.burst[i].saturating_add(1);
                }
                self.enc[i] = total;
            } else {
                self.enc[i] = 0;
            }
        }
    }
}

impl Controls for Stm32Controls {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.enc[id as usize]
    }

    fn button_state(&self, id: ButtonId) -> ButtonState {
        ButtonState::from_edges(self.edges[id as usize])
    }

    fn edges(&self, id: ButtonId) -> Edges {
        self.edges[id as usize]
    }

    fn now_ms(&self) -> Ms {
        self.now_ms
    }

    fn push(&self, id: EncoderId) -> Edges {
        self.edges[NUM_BUTTONS + id as usize]
    }
}
