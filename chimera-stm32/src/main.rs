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

    // 256-entry sine lookup table (i32, 50% amplitude)
    static SINE_TABLE: [i32; 256] = {
        // Generated: (sin(i/256 * 2*PI) * 0.5 * i32::MAX) for i in 0..256
        // Using const evaluation trick with precomputed values
        let mut table = [0i32; 256];
        let mut i = 0;
        while i < 256 {
            // Approximate: sin(x) via polynomial for const eval
            // x = i/256 * 2*PI
            let t = i as f64 / 256.0;
            let x = t * 2.0 * 3.14159265358979323846;
            // Taylor series sin(x) = x - x³/6 + x⁵/120 - x⁷/5040 + x⁹/362880
            // Reduce x to [-PI, PI] range first
            let x = x - (6.28318530717958647692 * ((x / 6.28318530717958647692 + 0.5) as i64 as f64));
            let x2 = x * x;
            let x3 = x2 * x;
            let x5 = x3 * x2;
            let x7 = x5 * x2;
            let x9 = x7 * x2;
            let x11 = x9 * x2;
            let s = x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0 + x9 / 362880.0 - x11 / 39916800.0;
            table[i] = (s * 0.5 * 2147483647.0) as i32;
            i += 1;
        }
        table
    };

    let mut phase_acc: u32 = 0;
    // Phase increment for 440 Hz at 47917 Hz sample rate
    let phase_inc: u32 = 39_472_883;

    let mut phase_acc: u32 = 0;
    let phase_inc: u32 = 39_472_883; // 440 Hz at 47917 Hz

    loop {
        // Feed SAI FIFO — runs every loop iteration
        while audio::sai_fifo_has_room() {
            let idx = (phase_acc >> 24) as usize;
            let sample = (SINE_TABLE[idx] >> 16) as i16;
            audio::write_sai_data(sample);
            audio::write_sai_data(sample);
            phase_acc = phase_acc.wrapping_add(phase_inc);
        }

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
