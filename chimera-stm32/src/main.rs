#![no_std]
#![no_main]

mod audio;

mod controls;
mod display;

use chimera_core::ui::UiState;
use chimera_core::ui::perf::PerfTracker;
use chimera_hal::ChimeraDisplay;
use controls::Stm32Controls;
use cortex_m_rt::{entry, exception, pre_init};
use display::Stm32Display;
use panic_halt as _;
use stm32h7xx_hal::{pac, prelude::*, spi};

/// Set VTOR to our vector table. Our custom bootloader (chimera-bootloader)
/// provides a clean peripheral state, so no other cleanup is needed.
#[pre_init]
unsafe fn before_main() {
    // SAFETY: runs once, before `main` and before interrupts are enabled, on
    // a single core. 0xE000_ED08 is the SCB->VTOR register (a valid, aligned,
    // memory-mapped address on every Cortex-M7), and 0x0802_0000 is our
    // linked vector table's flash address.
    unsafe {
        core::ptr::write_volatile(0xE000_ED08 as *mut u32, 0x0802_0000);
    }
}

#[exception]
fn SysTick() {
    controls::isr_tick();
}

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();
    let pwr = dp.PWR.constrain();
    let pwrcfg = pwr.freeze();
    let rcc = dp.RCC.constrain();
    let ccdr = rcc
        .use_hse(8.MHz())
        .sys_ck(400.MHz())
        .hclk(200.MHz())
        .pclk1(100.MHz())
        .pclk2(100.MHz())
        .pclk3(100.MHz())
        .pclk4(100.MHz())
        .pll1_q_ck(200.MHz())
        .freeze(pwrcfg, &dp.SYSCFG);

    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);
    let gpiof = dp.GPIOF.split(ccdr.peripheral.GPIOF);
    let _hc_data = gpiof.pf2.into_floating_input();
    let _hc_load = gpiof.pf1.into_push_pull_output();
    let _hc_clk = gpiof.pf0.into_push_pull_output();

    let mut led = gpioe.pe1.into_push_pull_output();
    let mut backlight = gpioe.pe11.into_push_pull_output();
    backlight.set_high();

    // SAI1 pins (AF6) for audio DAC 1
    let _sai_mclk = gpioe.pe2.into_alternate::<6>();
    let _sai_fs = gpioe.pe4.into_alternate::<6>();
    let _sai_sck = gpioe.pe5.into_alternate::<6>();
    let _sai_sd_a = gpioe.pe6.into_alternate::<6>();
    led.set_high();

    // SPI1 display pins
    let mut sck = gpioa.pa5.into_alternate::<5>();
    let mut mosi = gpioa.pa7.into_alternate::<5>();
    sck.set_speed(stm32h7xx_hal::gpio::Speed::High);
    mosi.set_speed(stm32h7xx_hal::gpio::Speed::High);
    let dc = gpiod.pd8.into_push_pull_output();
    let reset = gpiod.pd9.into_push_pull_output();
    let cs = gpiod.pd10.into_push_pull_output();

    let spi = dp.SPI1.spi(
        (sck, spi::NoMiso, mosi),
        spi::Config::new(spi::MODE_0),
        50.MHz(),
        ccdr.peripheral.SPI1,
        &ccdr.clocks,
    );

    let mut display = Stm32Display::new(spi, dc, reset, cs);
    cortex_m::asm::delay(100_000_000); // ~250ms power-on delay
    display.init();

    let mut controls = Stm32Controls::new();
    let mut ui = UiState::new();
    let perf = PerfTracker::new();

    controls::start_systick(200_000_000);
    controls::enable();

    // Audio init — DMA-driven, main loop has no audio responsibilities
    audio::init_pll3();
    audio::init_sai1a(); // Configures SAI but does NOT enable it

    // Connect voice to UI params and trigger test note
    // SAFETY: ui.performance lives in main's stack frame which never returns (-> !).
    // Part 0's sound params/mod_state outlive the audio DMA for the same reason.
    unsafe {
        audio::init_voice(
            &ui.performance.parts[0].sound.params as *const _,
            &ui.performance.parts[0].sound.mod_state as *const _,
        );
    }
    audio::trigger_note(chimera_hal::MidiNote::A4, chimera_hal::Velocity::DEFAULT);

    audio::prefill_buffer(); // Fill buffer with first rendered audio
    audio::init_dma(); // Configure + enable DMA1_Stream0
    audio::enable_sai(); // Now enable SAI — DMA begins transferring

    // Initial render
    ui.update();
    ui.render(&mut display, &perf.stats);
    display.flush();
    ui.prime_regions(&perf.stats);
    led.set_low();

    loop {
        // Controls + display
        controls.snapshot();
        let has_input = controls.has_activity();

        if has_input {
            ui.handle_input(&controls);
        }

        ui.update();

        let flush_list = ui.render_dirty(&mut display, &perf.stats);

        for &(ys, ye) in &flush_list {
            if ys != ye {
                display.flush_region(ys, ye);
            }
        }
    }
}
