#![no_std]
#![no_main]

#[cfg(all(feature = "bench", feature = "sd-probe"))]
compile_error!("bench and sd-probe both take over after boot: pick one");

// The SD probe build halts after `boot`: nothing of the synth is built.
#[cfg(not(feature = "sd-probe"))]
mod audio;
#[cfg(all(feature = "bench", not(feature = "sd-probe")))]
mod bench;
mod cache;
mod clocks;
#[cfg(not(feature = "sd-probe"))]
mod controls;
mod display;
#[cfg(all(feature = "midi-din", not(feature = "sd-probe")))]
mod midi_din;
mod panic;
#[cfg(not(feature = "sd-probe"))]
mod priority;
#[cfg(not(feature = "sd-probe"))]
mod probe;
#[cfg(feature = "sd-probe")]
mod sd;
#[cfg(feature = "sd-probe")]
mod sd_probe;
#[cfg(not(feature = "sd-probe"))]
mod shared;
#[cfg(not(feature = "sd-probe"))]
mod watchdog;

#[cfg(not(feature = "sd-probe"))]
use chimera_core::reset::ResetCause;
use chimera_core::ui::theme_settings::ThemeSettings;
use cortex_m_rt::entry;
use display::Stm32Display;
use stm32h7xx_hal::gpio::{Output, PD8, PD9, PD10, PushPull, Speed};
#[cfg(any(feature = "midi-din", feature = "sd-probe"))]
use stm32h7xx_hal::rcc::CoreClocks;
use stm32h7xx_hal::{pac, prelude::*, spi};

/// Flush-to-zero and default NaN, in this context (FPSCR) and in every
/// exception's (FPDSCR, which the audio ISR starts from; it resets to
/// 0). Tails decaying toward silence then never go denormal, whose
/// arithmetic the host would not match anyway.
fn fp_flush_to_zero(fpu: &mut cortex_m::peripheral::FPU) {
    const FZ_DN: u32 = 1 << 24 | 1 << 25;
    // SAFETY: sets only FZ and DN; nothing here depends on denormals or on
    // NaN payloads, and no interrupt has started yet.
    unsafe {
        fpu.fpdscr.modify(|v| v | FZ_DN);
        cortex_m::register::fpscr::write(cortex_m::register::fpscr::Fpscr::from_bits(
            cortex_m::register::fpscr::read().bits() | FZ_DN,
        ));
    }
}

#[cfg(not(feature = "sd-probe"))]
#[cortex_m_rt::exception]
fn SysTick() {
    static mut HEARTBEAT: chimera_core::audio_out::Heartbeat =
        chimera_core::audio_out::Heartbeat::new();
    controls::isr_tick();
    watchdog::kick_if_audio_alive(HEARTBEAT);
}

type Display = Stm32Display<
    spi::Spi<pac::SPI1, spi::Enabled>,
    PD8<Output<PushPull>>,
    PD9<Output<PushPull>>,
    PD10<Output<PushPull>>,
>;
#[cfg(not(feature = "sd-probe"))]
type Backlight = stm32h7xx_hal::pwm::Pwm<pac::TIM1, 1, stm32h7xx_hal::pwm::ComplementaryDisabled>;

/// The unit with the display up, as both builds start.
struct Board {
    cp: cortex_m::Peripherals,
    clk: clocks::Clocks,
    display: Display,
    #[cfg(any(feature = "midi-din", feature = "sd-probe"))]
    clocks: CoreClocks,
    #[cfg(not(feature = "sd-probe"))]
    synth: SynthParts,
    #[cfg(feature = "sd-probe")]
    sd: sd::SdParts,
}

#[cfg(not(feature = "sd-probe"))]
struct SynthParts {
    reset_cause: ResetCause,
    led: stm32h7xx_hal::gpio::PE1<Output<PushPull>>,
    backlight: Backlight,
    theme: ThemeSettings,
    iwdg: pac::IWDG,
    dbgmcu: pac::DBGMCU,
}

#[entry]
fn main() -> ! {
    let board = boot();
    #[cfg(feature = "sd-probe")]
    probe_card(board);
    #[cfg(not(feature = "sd-probe"))]
    synth(board);
}

#[cfg(feature = "sd-probe")]
fn probe_card(mut b: Board) -> ! {
    let sd = sd::init(b.sd, &mut b.cp.DCB, &mut b.cp.DWT, &b.clocks, b.clk.cpu_hz);
    sd_probe::run(&mut b.display, b.clk, sd)
}

fn boot() -> Board {
    #[cfg(not(feature = "sd-probe"))]
    probe::paint_stack();
    let mut cp = cortex_m::Peripherals::take().unwrap();
    fp_flush_to_zero(&mut cp.FPU);
    let dp = pac::Peripherals::take().unwrap();

    // RCC_RSR survives the reset it records; clear it for the next one.
    #[cfg(not(feature = "sd-probe"))]
    let reset_cause = ResetCause::from_rsr(dp.RCC.rsr.read().bits());
    dp.RCC.rsr.modify(|_, w| w.rmvf().set_bit());

    cache::enable_d2_sram();
    let rev = clocks::read_rev(&dp.DBGMCU);
    let (ccdr, clk) = clocks::freeze(dp.PWR, dp.RCC, &dp.SYSCFG, rev);
    cache::init(&mut cp.MPU, &mut cp.SCB, &mut cp.CPUID);
    #[cfg(not(feature = "sd-probe"))]
    shared::copy_waves();

    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);
    let gpiof = dp.GPIOF.split(ccdr.peripheral.GPIOF);
    #[cfg(any(feature = "midi-din", feature = "sd-probe"))]
    let gpiob = dp.GPIOB.split(ccdr.peripheral.GPIOB);
    #[cfg(feature = "midi-din")]
    let _midi_rx = gpiob.pb7.into_alternate::<7>();
    let _hc_data = gpiof.pf2.into_floating_input();
    let _hc_load = gpiof.pf1.into_push_pull_output();
    let _hc_clk = gpiof.pf0.into_push_pull_output();

    let mut led = gpioe.pe1.into_push_pull_output();
    // TIM1 CH2 PWM, above hearing so the backlight driver cannot whine.
    // Full brightness lifts a TN panel's blacks; System › Theme's BRIGHT
    // sets the duty (70 % at boot: no storage yet).
    let mut backlight = dp.TIM1.pwm(
        gpioe.pe11.into_alternate::<1>(),
        20.kHz(),
        ccdr.peripheral.TIM1,
        &ccdr.clocks,
    );
    let theme = ThemeSettings::DEFAULT;
    backlight.set_duty(theme.bright.duty(backlight.get_max_duty()));
    backlight.enable();

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

    let fb = display::take_framebuffer().expect("framebuffer taken once");
    let mut display = Stm32Display::new(spi, dc, reset, cs, fb);
    clocks::delay_us(clk.cpu_hz, 250_000);
    display.init(clk.cpu_hz);
    // The boot theme is no longer the panel's own reset state (PUNCH, not
    // PANEL; ground −2, not 0), so push it explicitly instead of waiting for
    // the first change the main loop notices.
    display.set_gamma(theme.gamma.tables());
    display.set_palette(theme.palette());
    Board {
        cp,
        clk,
        display,
        #[cfg(any(feature = "midi-din", feature = "sd-probe"))]
        clocks: ccdr.clocks,
        #[cfg(not(feature = "sd-probe"))]
        synth: SynthParts {
            reset_cause,
            led,
            backlight,
            theme,
            iwdg: dp.IWDG,
            dbgmcu: dp.DBGMCU,
        },
        #[cfg(feature = "sd-probe")]
        sd: sd::SdParts {
            spi2: dp.SPI2,
            rec: ccdr.peripheral.SPI2,
            sck: gpioa.pa9,
            miso: gpiob.pb14,
            mosi: gpiob.pb15,
            cs: gpioe.pe12,
        },
    }
}

#[cfg(not(feature = "sd-probe"))]
fn synth(board: Board) -> ! {
    use chimera_core::clock_plan::pll3_for;
    use chimera_core::hw::SampleBudget;
    use chimera_core::ui::perf::PerfTracker;
    use chimera_hal::ChimeraDisplay;
    use controls::Stm32Controls;

    let Board {
        mut cp,
        clk,
        mut display,
        #[cfg(feature = "midi-din")]
        clocks,
        synth:
            SynthParts {
                reset_cause,
                mut led,
                mut backlight,
                mut theme,
                iwdg,
                dbgmcu,
            },
    } = board;
    let mut stats_r = probe::init(&mut cp.DCB, &mut cp.DWT, clk, reset_cause);

    let mut controls = Stm32Controls::new();
    let ui = shared::take_ui().expect("UI state taken once");
    let perf = PerfTracker::new();
    #[cfg(feature = "bench")]
    bench::run(&mut display, clk, &ui.performance);

    controls::start_systick(cp.SYST, &mut cp.SCB, clk.cpu_hz);
    controls::enable();

    let (scope_w, mut scope_r) = shared::take_scope().expect("scope buffer taken once");
    let (mut shared_w, shared_r) =
        shared::take_audio(&ui.performance).expect("audio buffer taken once");
    // Without MIDI DIN nothing takes a producer.
    #[cfg_attr(not(feature = "midi-din"), allow(unused_mut, unused_variables))]
    let (mut producers, notes) = audio::engine::NOTES
        .split()
        .expect("note sources split once");
    audio::engine::init(SampleBudget::for_cpu(clk.cpu_hz), shared_r, notes, scope_w);

    let pll3 = pll3_for(clocks::HSE_HZ, chimera_hal::SAMPLE_RATE, clk.rev);
    clocks::init_pll3(&pll3);
    audio::sai::init(clk.rev.new_sai(), pll3.mckdiv);
    audio::dma::clear();
    audio::prefill();
    watchdog::start(iwdg, &dbgmcu);
    audio::dma::init(&mut cp.NVIC);
    audio::dma::start();
    audio::sai::start();

    #[cfg(feature = "midi-din")]
    midi_din::init(
        &mut cp.NVIC,
        clocks.pclk2().raw(),
        producers
            .take(audio::engine::DIN)
            .expect("DIN producer taken once"),
    );

    ui.update();
    ui.render_with_audio(&mut display, &perf.stats, None, scope_r.read());
    display.flush();
    ui.prime_regions(&perf.stats, None, scope_r.read());
    led.set_low();

    loop {
        controls.snapshot();
        if controls.has_activity() {
            ui.handle_input(&controls);
        }
        // System › Theme: the UI loop owns the display and the backlight.
        let new_theme = ui.theme();
        let recolour = new_theme != theme && new_theme.palette() != theme.palette();
        if new_theme != theme {
            backlight.set_duty(new_theme.bright.duty(backlight.get_max_duty()));
            if new_theme.gamma != theme.gamma {
                display.set_gamma(new_theme.gamma.tables());
            }
            display.set_palette(new_theme.palette());
            theme = new_theme;
        }
        ui.update();
        shared_w.publish(|b| b.update_from(&ui.performance));
        let stats = stats_r.as_mut().map(|r| {
            let mut s = *r.read();
            s.stack_used = probe::stack_used();
            s
        });
        let flush_list =
            ui.render_dirty_with_audio(&mut display, &perf.stats, stats.as_ref(), scope_r.read());
        if recolour {
            // A new palette recolours rows that did not redraw.
            display.flush();
        } else {
            for &(ys, ye) in &flush_list {
                if ys != ye {
                    display.flush_region(ys, ye);
                }
            }
        }
    }
}
