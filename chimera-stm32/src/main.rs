// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

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
        feature = "bench-tape",
        feature = "pc-sample"
    ))
))]
compile_error!(
    "the bench needs an area: bench-algo, -routing, -memory, -stress, -orbit, -tape or pc-sample"
);
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
#[cfg(feature = "pc-sample")]
mod pc_sample;

use chimera_board::dfu;
use cortex_m_rt::entry;
use stm32h7xx_hal::pac;

#[cfg(not(feature = "sd-probe"))]
#[cortex_m_rt::exception]
fn SysTick() {
    static mut HEARTBEAT: chimera_engine::audio_out::Heartbeat =
        chimera_engine::audio_out::Heartbeat::new();
    // SAFETY: this is SysTick, `isr_tick`'s one caller.
    unsafe { chimera_board::controls::isr_tick() };
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
/// console's `bench`. The bench is bank 2's (ADR 0194): closed, it can't run.
#[cfg(feature = "bench")]
fn bench(
    display: &mut chimera_board::Display,
    clk: chimera_board::clocks::Clocks,
    project: &mut chimera_engine::project::Project,
    sources: &mut chimera_engine::source_table::AudioToken,
    itcm: chimera_board::shared::ItcmZeroed,
    rings: chimera_board::BenchRings<'_>,
) -> &'static str {
    use chimera_core::bank2::{BENCH_DOOR_ADDR, DOOR_CELL, DoorSlot};
    // SAFETY: `bank2.x` links `bench::BENCH_DOOR`, a `BenchDoor`, at
    // `BENCH_DOOR_ADDR` in every bench build (its assert, `__bank2_bench_door`).
    const SLOT: DoorSlot<bench::BenchDoor> = unsafe { DoorSlot::new(BENCH_DOOR_ADDR) };
    DOOR_CELL
        .with_slot(SLOT, |d| {
            assert_eq!(
                d.magic(),
                bench::BENCH_MAGIC,
                "the bench's table at its slot"
            );
            (d.enter)(display, clk, project, sources, itcm, rings)
        })
        .unwrap_or("# bench: bank 2 closed: install this build's .bk2\n")
}

/// A bench `StreamCard`'s frames, `n` for each of `streamed`: bank 1's, as
/// the `dyn Fn` vtable bank 1 holds of it is (ADR 0194).
#[cfg(feature = "bench")]
#[cfg_attr(not(feature = "bench-tape"), allow(dead_code))]
fn streamed_frames(
    streamed: &[chimera_engine::project::SourceId],
    n: u32,
) -> impl Fn(chimera_engine::project::SourceId) -> Option<chimera_engine::stream::units::Frames> + '_
{
    move |id| {
        streamed
            .contains(&id)
            .then_some(chimera_engine::stream::units::Frames::new(n))
    }
}
