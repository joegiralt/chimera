#![no_std]
#![no_main]

mod audio;
mod cache;
mod clocks;
mod controls;
mod display;
#[cfg(feature = "midi-din")]
mod midi_din;
mod panic;
mod priority;
mod shared;

use chimera_core::clock_plan::{SiliconRev, pll3_for};
use chimera_core::hw::SampleBudget;
use chimera_core::ui::fmt::FmtBuf;
use chimera_core::ui::perf::PerfTracker;
use chimera_core::ui::{draw, theme};
use chimera_hal::ChimeraDisplay;
use controls::Stm32Controls;
use cortex_m_rt::{entry, exception, pre_init};
use display::Stm32Display;
use priority::Priority;
use stm32h7xx_hal::gpio::Speed;
use stm32h7xx_hal::{pac, prelude::*, spi};

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
    let mut cp = cortex_m::Peripherals::take().unwrap();
    let dp = pac::Peripherals::take().unwrap();

    cache::enable_d2_sram();
    let rev = clocks::read_rev(&dp.DBGMCU);
    let (ccdr, clk) = clocks::freeze(dp.PWR, dp.RCC, &dp.SYSCFG, rev);
    cache::init(&mut cp.MPU, &mut cp.SCB, &mut cp.CPUID);

    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);
    let gpiof = dp.GPIOF.split(ccdr.peripheral.GPIOF);
    #[cfg(feature = "midi-din")]
    let _midi_rx = dp
        .GPIOB
        .split(ccdr.peripheral.GPIOB)
        .pb7
        .into_alternate::<7>();
    let _hc_data = gpiof.pf2.into_floating_input();
    let _hc_load = gpiof.pf1.into_push_pull_output();
    let _hc_clk = gpiof.pf0.into_push_pull_output();

    let mut led = gpioe.pe1.into_push_pull_output();
    let mut backlight = gpioe.pe11.into_push_pull_output();
    backlight.set_high();

    let mut sai_mclk = gpioe.pe2.into_alternate::<6>();
    let mut sai_fs = gpioe.pe4.into_alternate::<6>();
    let mut sai_sck = gpioe.pe5.into_alternate::<6>();
    let mut sai_sd_a1 = gpioe.pe6.into_alternate::<6>();
    // MCLK is 12.288 MHz, the edge of the low-speed GPIO range.
    sai_mclk.set_speed(Speed::Medium);
    sai_fs.set_speed(Speed::Medium);
    sai_sck.set_speed(Speed::Medium);
    sai_sd_a1.set_speed(Speed::Medium);
    let mut sai_sd_b1 = gpioe.pe3.into_alternate::<6>();
    let mut sai_sd_a2 = gpiod.pd11.into_alternate::<10>();
    sai_sd_b1.set_speed(Speed::Medium);
    sai_sd_a2.set_speed(Speed::Medium);
    led.set_high();

    let mut sck = gpioa.pa5.into_alternate::<5>();
    let mut mosi = gpioa.pa7.into_alternate::<5>();
    sck.set_speed(Speed::High);
    mosi.set_speed(Speed::High);
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
    clocks::delay_us(clk.cpu_hz, 250_000);
    display.init(clk.cpu_hz);
    boot_splash(&mut display, &clk);

    let mut controls = Stm32Controls::new();
    let ui = shared::take_ui().expect("UI state taken once");
    let perf = PerfTracker::new();

    controls::start_systick(clk.cpu_hz);
    priority::set_systick(&mut cp.SCB, Priority::SYSTICK);
    controls::enable();

    let (scope_w, mut scope_r) = shared::take_scope().expect("scope buffer taken once");
    let (mut shared_w, shared_r) =
        shared::take_audio(&ui.performance).expect("audio buffer taken once");
    audio::engine::init(SampleBudget::for_cpu(clk.cpu_hz), shared_r, scope_w);

    let pll3 = pll3_for(clocks::HSE_HZ, chimera_hal::SAMPLE_RATE, clk.rev);
    clocks::init_pll3(&pll3);
    audio::sai::init(clk.rev.new_sai(), pll3.mckdiv);
    audio::dma::clear();
    audio::prefill();
    audio::dma::init(&mut cp.NVIC);
    audio::dma::start();
    audio::sai::start();

    #[cfg(feature = "midi-din")]
    midi_din::init(&mut cp.NVIC, ccdr.clocks.pclk2().raw());

    ui.update();
    ui.render_with_scope(&mut display, &perf.stats, scope_r.read());
    display.flush();
    ui.prime_regions(&perf.stats, scope_r.read());
    led.set_low();

    loop {
        controls.snapshot();
        if controls.has_activity() {
            ui.handle_input(&controls);
        }
        ui.update();
        shared_w.publish(|b| b.update_from(&ui.performance));
        let flush_list = ui.render_dirty_with_scope(&mut display, &perf.stats, scope_r.read());
        for &(ys, ye) in &flush_list {
            if ys != ye {
                display.flush_region(ys, ye);
            }
        }
    }
}

// Temporary (bring-up step 1): which revision and clock this board runs;
// the AUDIO page replaces it in step 6.
fn boot_splash(display: &mut impl ChimeraDisplay, clk: &clocks::Clocks) {
    use core::fmt::Write;
    draw::fill_rect(display, 0, 0, theme::SCREEN_W, theme::SCREEN_H, theme::BG);
    let mut line = FmtBuf::new();
    let _ = write!(
        line,
        "REV {}  {} MHZ",
        clk.rev.label(),
        clk.cpu_hz / 1_000_000
    );
    draw::text(
        display,
        &theme::FONT_VALUE,
        "CHIMERA",
        theme::MARGIN_X,
        140,
        theme::INK,
    );
    draw::text(
        display,
        &theme::FONT_VALUE,
        line.as_str(),
        theme::MARGIN_X,
        162,
        theme::INK2,
    );
    if let SiliconRev::Unknown(id) = clk.rev {
        line.clear();
        let _ = write!(line, "REV_ID 0x{id:04X}");
        draw::text(
            display,
            &theme::FONT_LABEL,
            line.as_str(),
            theme::MARGIN_X,
            180,
            theme::MID,
        );
    }
    display.flush();
    clocks::delay_us(clk.cpu_hz, 1_500_000);
}
