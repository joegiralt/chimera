#![no_std]
#![no_main]

#[cfg(all(feature = "bench", feature = "sd-probe"))]
compile_error!("bench and sd-probe both take over after boot: pick one");
#[cfg(all(
    feature = "bench",
    not(any(
        feature = "bench-algo",
        feature = "bench-routing",
        feature = "bench-memory",
        feature = "bench-stress",
        feature = "bench-orbit",
        feature = "bench-tape"
    ))
))]
compile_error!("the bench needs an area: bench-algo, -routing, -memory, -stress, -orbit or -tape");
// The lost-cluster sweep frees only on the owner's go (#360, ADR 0094):
// until then the boot runs it dry. Only the commit that enables freeing
// removes this line.
#[cfg(feature = "sweep-free")]
compile_error!("owner's go required");
#[cfg(all(feature = "usb-console", feature = "sd-probe"))]
compile_error!(
    "the SD probe halts after boot, so nothing would poll USB: build it with --no-default-features"
);

// The SD probe build halts after `boot`: nothing of the synth is built.
#[cfg(not(feature = "sd-probe"))]
mod audio;
#[cfg(all(feature = "bench", not(feature = "sd-probe")))]
mod bench;
#[cfg(feature = "filter-probes")]
mod filter_probes;
#[cfg(all(feature = "midi-din", not(feature = "sd-probe")))]
mod midi_din;
mod panic;

use chimera_board::dfu;
use cortex_m_rt::entry;
use stm32h7xx_hal::pac;

#[cfg(not(feature = "sd-probe"))]
#[cortex_m_rt::exception]
fn SysTick() {
    static mut HEARTBEAT: chimera_core::audio_out::Heartbeat =
        chimera_core::audio_out::Heartbeat::new();
    chimera_board::controls::isr_tick();
    chimera_board::watchdog::kick_if_audio_alive(HEARTBEAT);
}

#[entry]
fn main() -> ! {
    let mut cp = cortex_m::Peripherals::take().unwrap();
    let dp = pac::Peripherals::take().unwrap();
    // Before `boot` sets up anything: a DFU request is honoured whatever
    // build is flashed.
    let checked = dfu::after_reset(&mut cp, &dp.RCC, &dp.PWR, &dp.RTC);
    let board = chimera_board::boot(cp, dp, checked);
    #[cfg(feature = "sd-probe")]
    chimera_board::probe_card(board);
    #[cfg(not(feature = "sd-probe"))]
    chimera_board::synth(
        board,
        chimera_board::Shell {
            start_audio: audio::start,
            #[cfg(feature = "bench")]
            bench,
        },
    );
}

/// The bench's screens before the audio starts; their text, for the
/// console's `bench`.
#[cfg(feature = "bench")]
fn bench(
    display: &mut chimera_board::Display,
    clk: chimera_board::clocks::Clocks,
    project: &mut chimera_core::project::Project,
    sources: &mut chimera_core::source_table::AudioToken,
) -> &'static str {
    let report = bench::take_report().expect("bench report taken once");
    bench::run(display, clk, (project, sources), report);
    let report: &'static chimera_core::console::Report<{ bench::BENCH_TEXT_LEN }> = report;
    report.as_str()
}
