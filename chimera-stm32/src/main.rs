#![no_std]
#![no_main]

mod audio;
mod bitbang_spi;
mod controls;
mod display;
mod midi;

use bitbang_spi::BitBangSpi;
use chimera_core::ui::UiState;
use chimera_core::ui::perf::PerfTracker;
use chimera_hal::{ChimeraDisplay, Controls};
use controls::Stm32Controls;
use cortex_m_rt::{entry, exception};
use display::Stm32Display;
use panic_halt as _;
use stm32h7xx_hal::{pac, prelude::*};

#[exception]
fn SysTick() {
    controls::isr_tick();
}

#[entry]
fn main() -> ! {
    unsafe {
        core::ptr::write_volatile(0xE000_ED08 as *mut u32, 0x0802_0000);
    }

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

    let sck = gpioa.pa5.into_push_pull_output();
    let mosi = gpioa.pa7.into_push_pull_output();
    let dc = gpiod.pd8.into_push_pull_output();
    let reset = gpiod.pd9.into_push_pull_output();
    let cs = gpiod.pd10.into_push_pull_output();
    let spi = BitBangSpi::new(sck, mosi, 0x5802_0000, 5, 7);
    let mut display = Stm32Display::new(spi, dc, reset, cs);
    display.init();

    let mut controls = Stm32Controls::new();
    let mut ui = UiState::new();
    let perf = PerfTracker::new();

    controls::start_systick(200_000_000);
    controls::enable();

    // Audio init
    audio::init_pll3();
    audio::init_sai1a();

    // Initial render
    ui.update();
    ui.render(&mut display, &perf.stats);
    display.flush();
    ui.prime_regions(&perf.stats);
    led.set_low();

    let mut audio_phase: f32 = 0.0;

    loop {
        // Feed SAI FIFO with 440 Hz sine
        while audio::sai_fifo_has_room() {
            let sample = libm::sinf(audio_phase * 2.0 * core::f32::consts::PI);
            let i32_sample = (sample * 0.5 * (i32::MAX as f32)) as i32;
            audio::write_sai_data(i32_sample); // left
            audio::write_sai_data(i32_sample); // right
            audio_phase += 440.0 / 47917.0;
            if audio_phase >= 1.0 { audio_phase -= 1.0; }
        }

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
