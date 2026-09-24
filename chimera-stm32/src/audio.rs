//! SAI1 Block A audio output — DMA-driven double-buffer.
//!
//! Configures PLL3 for ~48kHz audio clock, sets up SAI1_A as I2S master TX,
//! and drives output via DMA1_Stream0 circular buffer with half/full ISR refill.
//!
//! PLL3: HSE 8MHz / M=1 * N=46 / P=3 = 122.67 MHz SAI kernel clock
//! SAI1_A: MCKDIV=5 → MCLK=12.27MHz → FS=47917Hz

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use stm32h7xx_hal::pac;
use cortex_m::peripheral::NVIC;
use stm32h7xx_hal::pac::interrupt;

use chimera_core::dsp::voice::Voice;
use chimera_core::instrument::Instrument;
use chimera_core::modulation::ModState;
use chimera_core::params::ParamSnapshot;
use chimera_hal::{BLOCK_SIZE, MidiNote, Velocity};

// SAI1 Block A CR1 register address for raw bit manipulation (MCKEN bit 27)
const SAI1_CHA_CR1: *mut u32 = 0x4001_5804 as *mut u32;

// SAI1 Block A data register address: base 0x40015800 + CHA offset 0x04 + DR offset 0x1C
const SAI1_CHA_DR: u32 = 0x4001_5820;

/// The voice pool's place in D2 SRAM, reserved for the port sub-project
/// (ADR 0014): the linker proves `Instrument` fits beside the DMA buffer.
/// Not initialised or read yet; the single `VOICE` below still plays.
// SAFETY: never read or written in this sub-project; the port that
// initialises and accesses it in place must do so under the same
// single-ISR-owner discipline as the other statics in this file.
#[used]
#[unsafe(link_section = ".ram_d2.voices")]
static mut INSTRUMENT: MaybeUninit<Instrument> = MaybeUninit::uninit();

/// DMA audio buffer in RAM_D2 — 256 × i16 = 128 stereo pairs.
/// DMA reads one half while ISR fills the other.
/// Note: RAM_D2 is NOLOAD, so this initializer is not applied by startup code.
/// `prefill_buffer()` must be called before DMA starts.
#[unsafe(link_section = ".ram_d2")]
static mut AUDIO_BUF: [i16; 256] = [0; 256];

/// f32 work buffer for Voice rendering.
static mut WORK_BUF: [f32; BLOCK_SIZE] = [0.0; BLOCK_SIZE];

/// Single voice instance — only accessed from DMA ISR.
static mut VOICE: Option<Voice> = None;

/// Parameter snapshot pointer — UI thread writes, ISR reads.
static mut PARAMS: Option<*const ParamSnapshot> = None;

/// ModState pointer — UI thread writes, ISR reads.
static mut MOD_STATE_PTR: Option<*const ModState> = None;

/// Default empty ModState for when no pointer is set.
static DEFAULT_MOD_STATE: ModState = ModState::new();

/// Render one block of audio from the Voice into the DMA buffer at `offset`.
fn render_block(offset: usize) {
    // SAFETY: only called from single non-reentrant ISR (and prefill during init).
    // No other code accesses these statics concurrently.
    unsafe {
        let voice_ptr = addr_of_mut!(VOICE);
        let voice = match (*voice_ptr).as_mut() {
            Some(v) => v,
            None => {
                let buf = &mut *addr_of_mut!(AUDIO_BUF);
                for i in 0..(BLOCK_SIZE * 2) { buf[offset + i] = 0; }
                return;
            }
        };
        let params_ptr = addr_of_mut!(PARAMS);
        let params = match *params_ptr {
            Some(p) => &*p,
            None => {
                let buf = &mut *addr_of_mut!(AUDIO_BUF);
                for i in 0..(BLOCK_SIZE * 2) { buf[offset + i] = 0; }
                return;
            }
        };

        let work = &mut *addr_of_mut!(WORK_BUF);

        // SAFETY: MOD_STATE_PTR is only written during single-threaded init
        let mod_ptr = addr_of_mut!(MOD_STATE_PTR);
        let mod_state = match *mod_ptr {
            Some(p) => &*p,
            None => &DEFAULT_MOD_STATE,
        };

        // Render full Voice signal chain: Engine → Drive → Filter → Wavefolder → VCA
        voice.render(work, params, mod_state);
        chimera_core::scope::write_samples(work);

        // Convert f32 mono → i16 stereo
        let buf = &mut *addr_of_mut!(AUDIO_BUF);
        for i in 0..BLOCK_SIZE {
            let sample = (work[i].clamp(-1.0, 1.0) * 32767.0) as i16;
            buf[offset + i * 2] = sample;     // left
            buf[offset + i * 2 + 1] = sample; // right
        }
    }
}

/// Pre-fill the entire AUDIO_BUF before DMA starts.
pub fn prefill_buffer() {
    render_block(0);
    render_block(128);
}

/// Initialize the voice and connect to parameter snapshot + mod state.
/// # Safety
/// `params_ptr` must point to a ParamSnapshot that outlives the audio system.
/// `mod_ptr` must point to a ModState that outlives the audio system.
pub unsafe fn init_voice(params_ptr: *const ParamSnapshot, mod_ptr: *const ModState) {
    // SAFETY: called once during single-threaded init before ISR is active
    unsafe {
        addr_of_mut!(VOICE).write(Some(Voice::new(chimera_hal::SAMPLE_RATE)));
        addr_of_mut!(PARAMS).write(Some(params_ptr));
        addr_of_mut!(MOD_STATE_PTR).write(Some(mod_ptr));
    }
}

/// Trigger a note on the voice.
pub fn trigger_note(note: MidiNote, velocity: Velocity) {
    // SAFETY: called during init before ISR is active
    unsafe {
        let voice_ptr = addr_of_mut!(VOICE);
        let params_ptr = addr_of_mut!(PARAMS);
        if let (Some(voice), Some(p)) = ((*voice_ptr).as_mut(), *params_ptr) {
            voice.note_on(note, velocity, &*p);
        }
    }
}

/// Configure PLL3 to produce the SAI audio clock.
pub fn init_pll3() {
    let rcc = unsafe { &*pac::RCC::ptr() };

    // 1. Enable SAI1 peripheral clock
    rcc.apb2enr.modify(|_, w| w.sai1en().enabled());
    cortex_m::asm::delay(100);

    // 2. Disable PLL3
    rcc.cr.modify(|_, w| w.pll3on().off());
    while rcc.cr.read().pll3rdy().is_ready() {}

    // 3. Set PLL3 input divider: DIVM3 = 1 (preserve DIVM1/DIVM2)
    rcc.pllckselr.modify(|_, w| unsafe { w.divm3().bits(1) });

    // 4. Set PLL3 multiplier and dividers: N=46 (val 45), P=3 (val 2)
    rcc.pll3divr.write(|w| unsafe {
        w.divn3().bits(45)
         .divp3().bits(2)
         .divq3().bits(1)
         .divr3().bits(1)
    });

    // 5. Configure PLL3: wide VCO range, input range 8-16 MHz, enable P output
    rcc.pllcfgr.modify(|_, w| {
        w.pll3vcosel().wide_vco()
         .pll3rge().range8()
         .divp3en().enabled()
    });

    // 6. Enable PLL3
    rcc.cr.modify(|_, w| w.pll3on().on());
    while !rcc.cr.read().pll3rdy().is_ready() {}

    // 7. Set SAI1 clock source to PLL3_P (0b010)
    rcc.d2ccip1r.modify(|_, w| unsafe { w.sai1sel().bits(0b010) });
}

/// Configure SAI1 Block A as I2S master TX, 16-bit stereo.
pub fn init_sai1a() {
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    let cha = sai1.cha();

    // Disable SAI before configuration
    cha.cr1.modify(|_, w| w.saien().clear_bit());
    while cha.cr1.read().saien().bit_is_set() {}

    // CR1: Master TX, Free I2S, 16-bit, MCKDIV=5
    cha.cr1.write(|w| unsafe {
        w.mode().bits(0b00)      // Master TX
         .prtcfg().bits(0b00)    // Free protocol (I2S)
         .ds().bits(0b100)       // 16-bit data
         .mckdiv().bits(5)       // MCLK divider
    });

    // Set MCKEN (bit 27) via raw register — not in PAC
    unsafe {
        let cr1 = core::ptr::read_volatile(SAI1_CHA_CR1);
        core::ptr::write_volatile(SAI1_CHA_CR1, cr1 | (1 << 27));
    }

    // CR2: FIFO threshold 1/4, flush FIFO
    cha.cr2.write(|w| unsafe {
        w.fth().bits(0b001)
         .fflush().set_bit()
    });

    // FRCR: 32-bit frame, FS active 16 bits
    cha.frcr.write(|w| unsafe {
        w.frl().bits(31)         // Frame length = 32 bits
         .fsall().bits(15)       // FS active for 16 bits
         .fsdef().set_bit()      // FS is channel identification
         .fspol().clear_bit()    // FS active low
         .fsoff().set_bit()      // FS one bit before first data (I2S standard)
    });

    // SLOTR: 2 slots, both active, 16-bit slot size
    cha.slotr.write(|w| unsafe {
        w.nbslot().bits(1)       // 2 slots (N-1)
         .sloten().bits(0b0011)  // Slots 0 and 1 active
         .slotsz().bits(0b01)    // 16-bit slot size
    });

    // Enable DMA request (DMAEN in CR1) — but do NOT enable SAI yet.
    // SAI will be enabled after DMA is configured and buffer is pre-filled.
    cha.cr1.modify(|_, w| w.dmaen().set_bit());
}

/// Enable SAI1_A — call after DMA is configured and buffer is pre-filled.
pub fn enable_sai() {
    // SAFETY: single-threaded init, SAI1 peripheral access
    let sai1 = unsafe { &*pac::SAI1::ptr() };
    sai1.cha().cr1.modify(|_, w| w.saien().set_bit());
}

/// Configure DMA1_Stream0 for circular transfer from AUDIO_BUF to SAI1_A.
///
/// Must be called after `init_sai1a()` and `prefill_buffer()`, before `enable_sai()`.
pub fn init_dma() {
    // SAFETY: single-threaded init, peripheral register access before interrupts are unmasked
    let rcc = unsafe { &*pac::RCC::ptr() };
    let dma1 = unsafe { &*pac::DMA1::ptr() };
    let dmamux = unsafe { &*pac::DMAMUX1::ptr() };

    // Enable DMA1 clock
    rcc.ahb1enr.modify(|_, w| w.dma1en().set_bit());
    cortex_m::asm::delay(100);

    // Disable stream before configuration
    dma1.st[0].cr.modify(|_, w| w.en().disabled());
    while dma1.st[0].cr.read().en().is_enabled() {}

    // Clear all interrupt flags for stream 0
    dma1.lifcr.write(|w| {
        w.ctcif0().clear()
         .chtif0().clear()
         .cteif0().clear()
         .cdmeif0().clear()
         .cfeif0().clear()
    });

    // SAFETY: 87 is the valid DMAMUX request ID for SAI1_A (RM0433 Table 121)
    dmamux.ccr[0].modify(|_, w| unsafe { w.dmareq_id().bits(87) });

    // SAFETY: writing valid peripheral/memory addresses and transfer count
    dma1.st[0].par.write(|w| unsafe { w.pa().bits(SAI1_CHA_DR) });
    dma1.st[0].m0ar.write(|w| unsafe {
        // SAFETY: AUDIO_BUF is static, address stable for lifetime of program
        w.m0a().bits(core::ptr::addr_of!(AUDIO_BUF) as u32)
    });
    dma1.st[0].ndtr.write(|w| w.ndt().bits(256));

    dma1.st[0].cr.write(|w| {
        w.dir().memory_to_peripheral()
         .circ().enabled()
         .minc().incremented()
         .pinc().fixed()
         .msize().bits16()
         .psize().bits16()
         .pl().very_high()
         .htie().enabled()
         .tcie().enabled()
    });

    // SAFETY: DMA1_STR0 ISR is defined in this module; buffer is pre-filled;
    // unmasking is safe because the ISR only touches AUDIO_BUF, WORK_BUF, VOICE, and PARAMS.
    unsafe {
        let mut core = cortex_m::Peripherals::steal();
        core.NVIC.set_priority(pac::Interrupt::DMA1_STR0, 3);
        NVIC::unmask(pac::Interrupt::DMA1_STR0);
    }

    // Enable DMA stream
    dma1.st[0].cr.modify(|_, w| w.en().enabled());
}

#[interrupt]
fn DMA1_STR0() {
    // SAFETY: ISR has exclusive access to DMA1 status/clear registers;
    // render_block only touches AUDIO_BUF, WORK_BUF, VOICE, and PARAMS from this single ISR
    let dma1 = unsafe { &*pac::DMA1::ptr() };

    if dma1.lisr.read().htif0().is_half() {
        dma1.lifcr.write(|w| w.chtif0().clear());
        render_block(0);
    }

    if dma1.lisr.read().tcif0().is_complete() {
        dma1.lifcr.write(|w| w.ctcif0().clear());
        render_block(128);
    }
}
