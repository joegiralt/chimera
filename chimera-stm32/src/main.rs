#![no_std]
#![no_main]

mod audio;
mod controls;
mod display;
mod midi;

use cortex_m_rt::entry;
use panic_halt as _;
use stm32h7xx_hal::{pac, prelude::*};

use chimera_core::ui::UiState;
use chimera_core::ui::perf::{PerfStats, PerfTracker};
use chimera_hal::ChimeraDisplay;

#[entry]
fn main() -> ! {
    // Take peripherals
    let dp = pac::Peripherals::take().unwrap();
    let cp = cortex_m::Peripherals::take().unwrap();

    // ── Clock Configuration ─────────────────────────────────────────
    // HSE = 25 MHz, PLL1 → 480 MHz system clock
    let pwr = dp.PWR.constrain();
    let pwrcfg = pwr.vos0(&dp.SYSCFG).freeze();

    let rcc = dp.RCC.constrain();
    let ccdr = rcc
        .sys_ck(480.MHz())
        .hclk(240.MHz())
        .pll1_strategy(stm32h7xx_hal::rcc::PllConfigStrategy::Iterative)
        .pll1_p_ck(480.MHz())
        .pll3_p_ck(384.MHz()) // SAI clock source: 384MHz / 8 = 48kHz * 256
        .freeze(pwrcfg, &dp.SYSCFG);

    // Enable DMA clocks
    let _ = &ccdr.peripheral.DMA1;
    let _ = &ccdr.peripheral.DMA2;

    // ── GPIO Setup ──────────────────────────────────────────────────
    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiob = dp.GPIOB.split(ccdr.peripheral.GPIOB);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);

    // LED
    let mut led = gpioe.pe1.into_push_pull_output();
    led.set_high();

    // ── Controls (HC165 shift registers) ────────────────────────────
    let hc165_data = gpioa.pa0.into_pull_up_input();
    let hc165_load = gpioa.pa1.into_push_pull_output();
    let hc165_clk = gpioa.pa2.into_push_pull_output();
    let mut hw_controls = controls::Stm32Controls::new(hc165_data, hc165_load, hc165_clk);

    // ── Display (ILI9341 via SPI1) ──────────────────────────────────
    let spi1_sck = gpioa.pa5.into_alternate::<5>();
    let spi1_miso = gpioa.pa6.into_alternate::<5>();
    let spi1_mosi = gpioa.pa7.into_alternate::<5>();
    let tft_dc = gpiod.pd8.into_push_pull_output();
    let tft_reset = gpiod.pd9.into_push_pull_output();
    let tft_cs = gpiod.pd10.into_push_pull_output();

    let spi1 = dp.SPI1.spi(
        (spi1_sck, spi1_miso, spi1_mosi),
        stm32h7xx_hal::spi::MODE_0,
        25.MHz(),
        ccdr.peripheral.SPI1,
        &ccdr.clocks,
    );

    let mut hw_display = display::Stm32Display::new(spi1, tft_dc, tft_reset, tft_cs);
    hw_display.init();

    // ── MIDI (USART1) ───────────────────────────────────────────────
    let midi_tx = gpiob.pb6.into_alternate::<7>();
    let midi_rx = gpiob.pb7.into_alternate::<7>();

    let serial = dp.USART1.serial(
        (midi_tx, midi_rx),
        31_250.bps(),
        ccdr.peripheral.USART1,
        &ccdr.clocks,
    ).unwrap();

    let mut hw_midi = midi::Stm32Midi::new(serial);

    // ── Audio (SAI1 via DMA) ────────────────────────────────────────
    // SAI setup requires more complex configuration — see audio module
    // For now, audio runs in the main loop (non-DMA placeholder)

    // ── UI State ────────────────────────────────────────────────────
    let mut ui = UiState::new();
    let mut perf = PerfTracker::new();
    let perf_stats = PerfStats::zero();

    // Blink LED to show we're alive
    led.set_low();

    // ── Main Loop ───────────────────────────────────────────────────
    // Audio: eventually runs in DMA half-transfer ISR
    // UI: 30fps render loop
    let mut frame_counter: u32 = 0;

    loop {
        // Poll controls at ~500 Hz (every 2nd iteration at ~1kHz loop)
        if frame_counter % 2 == 0 {
            hw_controls.poll();
        }

        // Read MIDI
        if let Some(msg) = hw_midi.read() {
            // TODO: route MIDI messages to voice
            let _ = msg;
        }

        // UI update at ~30 fps (every 33rd iteration at ~1kHz loop)
        if frame_counter % 33 == 0 {
            ui.handle_input(&hw_controls);
            ui.update();
            ui.render(&mut hw_display, &perf_stats);
            hw_display.flush();
        }

        frame_counter = frame_counter.wrapping_add(1);

        // Simple delay — replace with proper timer later
        cortex_m::asm::delay(480_000); // ~1ms at 480MHz
    }
}
