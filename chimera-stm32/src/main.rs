#![no_std]
#![no_main]

mod audio;
mod controls;
mod display;
mod midi;

use cortex_m_rt::entry;
use panic_halt as _;
use stm32h7xx_hal::{pac, prelude::*, spi};

use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::UiState;
use chimera_hal::ChimeraDisplay;

#[entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();

    // ── Clock Configuration ─────────────────────────────────────────
    let pwr = dp.PWR.constrain();
    let pwrcfg = pwr.freeze();

    let rcc = dp.RCC.constrain();
    let ccdr = rcc
        .sys_ck(480.MHz())
        .pll3_p_ck(384.MHz()) // SAI audio clock
        .freeze(pwrcfg, &dp.SYSCFG);

    // ── GPIO ────────────────────────────────────────────────────────
    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiob = dp.GPIOB.split(ccdr.peripheral.GPIOB);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);

    // LED
    let mut led = gpioe.pe1.into_push_pull_output();
    led.set_high();

    // ── Controls (HC165) ────────────────────────────────────────────
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
        spi::MODE_0,
        25.MHz(),
        ccdr.peripheral.SPI1,
        &ccdr.clocks,
    );

    let mut hw_display = display::Stm32Display::new(spi1, tft_dc, tft_reset, tft_cs);
    hw_display.init();

    // ── MIDI (USART1) ───────────────────────────────────────────────
    let midi_tx = gpiob.pb6.into_alternate::<7>();
    let midi_rx = gpiob.pb7.into_alternate::<7>();

    let serial = dp
        .USART1
        .serial(
            (midi_tx, midi_rx),
            31_250.bps(),
            ccdr.peripheral.USART1,
            &ccdr.clocks,
        )
        .unwrap();

    let (_midi_tx, mut midi_rx) = serial.split();

    // ── UI ───────────────────────────────────────────────────────────
    let mut ui = UiState::new();
    let perf_stats = PerfStats::zero();

    led.set_low(); // alive

    // ── Main Loop ───────────────────────────────────────────────────
    let mut frame_counter: u32 = 0;

    loop {
        // Poll controls ~500Hz
        if frame_counter % 2 == 0 {
            hw_controls.poll();
        }

        // Read MIDI
        if let Ok(byte) = midi_rx.read() {
            // TODO: feed to MIDI parser → voice
            let _ = byte;
        }

        // UI at ~30fps
        if frame_counter % 33 == 0 {
            ui.handle_input(&hw_controls);
            ui.update();
            ui.render(&mut hw_display, &perf_stats);
            hw_display.flush();
        }

        frame_counter = frame_counter.wrapping_add(1);
        cortex_m::asm::delay(480_000); // ~1ms
    }
}
