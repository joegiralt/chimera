#![no_std]
#![no_main]

#[cfg(all(feature = "bench", feature = "sd-probe"))]
compile_error!("bench and sd-probe both take over after boot: pick one");
#[cfg(all(feature = "usb-console", feature = "sd-probe"))]
compile_error!(
    "the SD probe halts after boot, so nothing would poll USB: build it with --no-default-features"
);

// The SD probe build halts after `boot`: nothing of the synth is built.
#[cfg(not(feature = "sd-probe"))]
mod audio;
#[cfg(all(feature = "bench", not(feature = "sd-probe")))]
mod bench;
mod cache;
mod clocks;
#[cfg(not(feature = "sd-probe"))]
mod controls;
mod dfu;
mod display;
#[cfg(all(feature = "midi-din", not(feature = "sd-probe")))]
mod midi_din;
mod panic;
#[cfg(not(feature = "sd-probe"))]
mod priority;
#[cfg(not(feature = "sd-probe"))]
mod probe;
mod sd;
#[cfg(feature = "sd-probe")]
mod sd_probe;
#[cfg(not(feature = "sd-probe"))]
mod shared;
#[cfg(feature = "usb-console")]
mod usb;
#[cfg(not(feature = "sd-probe"))]
mod watchdog;

#[cfg(not(feature = "sd-probe"))]
use chimera_core::project::{LOAD_ACK_TIMEOUT_MS, LOAD_LINK};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::reset::ResetCause;
use chimera_core::ui::theme_settings::ThemeSettings;
use cortex_m_rt::entry;
use display::Stm32Display;
use stm32h7xx_hal::gpio::{Output, PD8, PD9, PD10, PushPull, Speed};
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
    clocks: CoreClocks,
    #[cfg(not(feature = "sd-probe"))]
    synth: SynthParts,
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
    /// The DFU marker: `dfu::enter` writes it.
    marker: dfu::Marker,
    #[cfg(feature = "usb-console")]
    usb: usb::UsbParts,
}

#[entry]
fn main() -> ! {
    let mut cp = cortex_m::Peripherals::take().unwrap();
    let dp = pac::Peripherals::take().unwrap();
    // Before `boot` sets up anything: a DFU request is honoured whatever
    // build is flashed.
    let checked = dfu::after_reset(&mut cp, &dp.RCC, &dp.PWR, &dp.RTC);
    let board = boot(cp, dp, checked);
    #[cfg(feature = "sd-probe")]
    probe_card(board);
    #[cfg(not(feature = "sd-probe"))]
    synth(board);
}

#[cfg(feature = "sd-probe")]
fn probe_card(mut b: Board) -> ! {
    let sd = sd::init(b.sd, &mut b.cp.DCB, &mut b.cp.DWT, &b.clocks, b.clk.cpu_hz);
    let store = sd::take_store(sd).expect("store taken once");
    sd_probe::run(&mut b.display, b.clk, store)
}

fn boot(mut cp: cortex_m::Peripherals, dp: pac::Peripherals, checked: dfu::Checked) -> Board {
    #[cfg(not(feature = "sd-probe"))]
    probe::paint_stack();
    fp_flush_to_zero(&mut cp.FPU);
    // The probe never enters DFU: the check ran, nothing writes the marker.
    #[cfg(feature = "sd-probe")]
    let _ = checked.seen();

    // `dfu::after_reset` read RCC_RSR and cleared it for the next reset.
    #[cfg(not(feature = "sd-probe"))]
    let seen = checked.seen();
    #[cfg(not(feature = "sd-probe"))]
    let reset_cause = ResetCause::from_rsr(seen.rsr);

    cache::enable_d2_sram();
    let rev = clocks::read_rev(&dp.DBGMCU);
    let (ccdr, clk) = clocks::freeze(dp.PWR, dp.RCC, &dp.SYSCFG, rev);
    #[cfg(feature = "usb-console")]
    let ccdr = usb::route_kernel_clock(ccdr);
    cache::init(&mut cp.MPU, &mut cp.SCB, &mut cp.CPUID);
    #[cfg(not(feature = "sd-probe"))]
    shared::copy_waves();

    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);
    let gpiof = dp.GPIOF.split(ccdr.peripheral.GPIOF);
    let gpiob = dp.GPIOB.split(ccdr.peripheral.GPIOB);
    #[cfg(feature = "midi-din")]
    let _midi_rx = gpiob.pb7.into_alternate::<7>();
    let _hc_data = gpiof.pf2.into_floating_input();
    let _hc_load = gpiof.pf1.into_push_pull_output();
    let _hc_clk = gpiof.pf0.into_push_pull_output();

    let mut led = gpioe.pe1.into_push_pull_output();
    // TIM1 CH2 PWM, above hearing so the backlight driver cannot whine.
    // Full brightness lifts a TN panel's blacks; SETTINGS › THEME's BRIGHT
    // sets the duty: the default until SYSTEM is read.
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
        clocks: ccdr.clocks,
        #[cfg(not(feature = "sd-probe"))]
        synth: SynthParts {
            reset_cause,
            led,
            backlight,
            theme,
            iwdg: dp.IWDG,
            dbgmcu: dp.DBGMCU,
            marker: dfu::Marker::new(checked, dp.RTC),
            #[cfg(feature = "usb-console")]
            usb: usb::UsbParts {
                dm: gpioa.pa11.into_alternate(),
                dp: gpioa.pa12.into_alternate(),
                global: dp.OTG2_HS_GLOBAL,
                device: dp.OTG2_HS_DEVICE,
                pwrclk: dp.OTG2_HS_PWRCLK,
                rec: ccdr.peripheral.USB2OTG,
                crs: dp.CRS,
                crs_rec: ccdr.peripheral.CRS,
                boot: seen,
            },
        },
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

/// The load ack's timeout in control ticks.
#[cfg(not(feature = "sd-probe"))]
const LOAD_ACK_TICKS: u32 = LOAD_ACK_TIMEOUT_MS * controls::CONTROLS_HZ / 1000;
#[cfg(not(feature = "sd-probe"))]
const _: () = assert!(LOAD_ACK_TICKS >= 1, "the ack timeout is under a tick");

#[cfg(not(feature = "sd-probe"))]
fn synth(board: Board) -> ! {
    use chimera_core::boot::BootStage;
    use chimera_core::clock_plan::pll3_for;
    use chimera_core::hw::SampleBudget;
    use chimera_core::storage::{Card, SystemSync};
    use chimera_core::ui::busy::{ToastStep, draw_busy, draw_toast};
    use chimera_core::ui::perf::PerfTracker;
    use chimera_core::ui::settings::CardCx;
    use chimera_hal::ChimeraDisplay;
    use controls::Stm32Controls;

    let Board {
        mut cp,
        clk,
        mut display,
        clocks,
        synth:
            SynthParts {
                reset_cause,
                mut led,
                mut backlight,
                mut theme,
                iwdg,
                dbgmcu,
                marker,
                #[cfg(feature = "usb-console")]
                    usb: usb_parts,
            },
        sd,
    } = board;
    let mut stats_r = probe::init(&mut cp.DCB, &mut cp.DWT, clk, reset_cause);

    let mut controls = Stm32Controls::new();
    let ui = shared::take_ui().expect("UI state taken once");

    // Start-up, in order, behind the splash until PART 1 can be drawn:
    // 1. SYSTEM and the last project from the card, however slow;
    // 2. the controls tick, the audio and MIDI DIN;
    // 3. PART 1 drawn, and USB set up and on the bus;
    // 4. the watchdog, armed last, once its kicks are live (`await_live`),
    //    so no start-up step can run on its clock;
    // 5. the UI loop.
    // `last_stage` on the next boot says how far this one got.
    // Step 1: SYSTEM behind the splash, then its theme. Card work runs only
    // here and in the UI loop, never on the audio path.
    let _ = chimera_core::ui::splash::draw(&mut display);
    display.flush();
    let sd = sd::init(sd, &mut cp.DCB, &mut cp.DWT, &clocks, clk.cpu_hz);
    let store = sd::take_store(sd).expect("store taken once");
    let mut card = Card::new();
    // No card or a card fault shows at the project's boot; a SYSTEM
    // file that can't be read still applies the defaults silently:
    // https://github.com/joegiralt/chimera/issues/197
    let (mut sync, mut settings, _) = SystemSync::boot(&mut card, store);
    ui.set_theme(settings.theme);
    apply_theme(settings.theme, &mut theme, &mut backlight, &mut display);
    // Still behind the splash: the last project (about 130 KB read), or
    // NEW and why.
    ui.boot_project(&mut card, store, settings.last_project);
    // The splash again in the card's theme; it stays up until PART 1.
    let _ = chimera_core::ui::splash::draw(&mut display);
    display.flush();
    marker.stage(BootStage::Card);
    let perf = PerfTracker::new();
    // The bench's screens as text, kept for the console's `bench`.
    #[cfg(feature = "bench")]
    #[cfg_attr(not(feature = "usb-console"), allow(unused_variables))]
    let bench_text = {
        let report = bench::take_report().expect("bench report taken once");
        bench::run(&mut display, clk, ui.project_mut(), report);
        let report: &'static chimera_core::console::Report<{ bench::BENCH_TEXT_LEN }> = report;
        Some(report.as_str())
    };
    #[cfg(all(feature = "usb-console", not(feature = "bench")))]
    let bench_text: Option<&str> = None;

    // Step 2.
    controls::start_systick(cp.SYST, &mut cp.SCB, clk.cpu_hz);
    controls::enable();

    let (scope_w, mut scope_r) = shared::take_scope().expect("scope buffer taken once");
    let (mut shared_w, shared_r) =
        shared::take_audio(ui.project().perf()).expect("audio buffer taken once");
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

    marker.stage(BootStage::Audio);

    // Step 3. USB set up, not yet on the bus.
    #[cfg(feature = "usb-console")]
    let usb = usb::init(usb_parts, &clocks, clk.cpu_hz);

    let (mut pacer, first) = chimera_core::ui::animation::Pacer::start(controls::now_ms());
    ui.update(first);
    ui.render_with_audio(&mut display, &perf.stats, None, scope_r.read());
    display.flush();
    ui.prime_regions(&perf.stats, None, scope_r.read());
    led.set_low();

    // On the bus only now: the loop below polls it from this point on.
    // Its core reset spins with interrupts masked, so before the watchdog.
    #[cfg(feature = "usb-console")]
    let mut usb = usb.connect();
    marker.stage(BootStage::Usb);

    // Step 4.
    watchdog::start(iwdg, &dbgmcu, watchdog::await_live(clk.cpu_hz));
    marker.stage(BootStage::Running);

    let mut last_tick = controls::ticks();
    loop {
        // The snapshot point: every path through the last iteration flushed.
        #[cfg(feature = "usb-console")]
        if let Some(asked) = usb.service(ui, stats_r.as_mut(), bench_text, display.frame()) {
            // As the menu's path: a THEME change in this visit is kept.
            ui.sync_system_now(&mut sync, &mut card, store, &mut settings);
            dfu::enter(&marker, dfu::DfuFrom::Console(asked));
        }
        controls.snapshot();
        // Every frame, even idle: a held key must age.
        ui.handle_input(&controls);
        // SYSTEM synced first: a THEME change made in this visit is kept.
        if let Some(yes) = ui.take_dfu_synced(&mut sync, &mut card, store, &mut settings) {
            dfu::enter(&marker, dfu::DfuFrom::Menu(yes));
        }
        // Card work the keys asked for, under BUSY. A load settles before
        // it publishes: the ack, or `LOAD_ACK_TICKS`.
        let busy = ui.card_pending();
        if busy {
            let (y0, y1) = draw_busy(&mut display);
            display.flush_region(y0, y1);
        }
        let cx = CardCx {
            card: &mut card,
            store: &mut *store,
            sync: &mut sync,
            settings: &mut settings,
        };
        ui.card_work(cx, &LOAD_LINK, |swap, p| {
            let t0 = controls::ticks();
            let _ = swap.settle(&LOAD_LINK, || {
                controls::ticks().wrapping_sub(t0) < LOAD_ACK_TICKS
            });
            shared_w.publish(|b| b.update_from(p.perf(), LOAD_LINK.epoch()));
        });
        // Leaving SETTINGS syncs SYSTEM, with no overlay first: a save is
        // quicker than BUSY can be read. A toast says how it went.
        ui.sync_system(&mut sync, &mut card, store, &mut settings);
        // SETTINGS › THEME: the UI loop owns the display and the backlight.
        let recolour = apply_theme(ui.theme(), &mut theme, &mut backlight, &mut display);
        // The loop spins as fast as it can; animation runs at UI_FPS.
        if let Some(t) = pacer.due(controls::now_ms()) {
            ui.update(t);
        }
        shared_w.publish(|b| b.update_from(ui.project().perf(), LOAD_LINK.epoch()));
        let stats = audio_stats(stats_r.as_mut());
        // Read after the card work; the toast's first step ignores it.
        let now = controls::ticks();
        let elapsed_ms = now.wrapping_sub(last_tick) * 1_000 / controls::CONTROLS_HZ;
        last_tick = now;
        let toast = ui.step_toast(elapsed_ms);
        if toast == ToastStep::Ended || busy {
            // The toast or BUSY covered rows the dirty regions don't know about.
            ui.render_with_audio(&mut display, &perf.stats, stats.as_ref(), scope_r.read());
            ui.prime_regions(&perf.stats, stats.as_ref(), scope_r.read());
            if let ToastStep::Show(text) = toast {
                draw_toast(&mut display, text.as_str());
            }
            display.flush();
            continue;
        }
        let flush_list =
            ui.render_dirty_with_audio(&mut display, &perf.stats, stats.as_ref(), scope_r.read());
        // Over whatever redrew beneath it, before anything is flushed.
        let band = match toast {
            ToastStep::Show(text) => Some(draw_toast(&mut display, text.as_str())),
            _ => None,
        };
        if recolour {
            // A new palette recolours rows that did not redraw.
            display.flush();
        } else {
            for &(ys, ye) in flush_list.iter().chain(&band) {
                if ys != ye {
                    display.flush_region(ys, ye);
                }
            }
        }
    }
}

/// The audio's latest stats with the stack's high-water mark, as the
/// AUDIO LOAD page and the console's `stats` both read them.
#[cfg(not(feature = "sd-probe"))]
fn audio_stats(
    r: Option<&mut chimera_core::triple::Reader<chimera_core::perf::load::AudioStats>>,
) -> Option<chimera_core::perf::load::AudioStats> {
    r.map(|r| {
        let mut s = *r.read();
        s.stack_used = probe::stack_used();
        s
    })
}

/// Pushes `new` to the backlight and the panel. True when the palette
/// changed, which recolours rows that did not redraw.
#[cfg(not(feature = "sd-probe"))]
fn apply_theme(
    new: ThemeSettings,
    theme: &mut ThemeSettings,
    backlight: &mut Backlight,
    display: &mut Display,
) -> bool {
    if new == *theme {
        return false;
    }
    let recolour = new.palette() != theme.palette();
    backlight.set_duty(new.bright.duty(backlight.get_max_duty()));
    if new.gamma != theme.gamma {
        display.set_gamma(new.gamma.tables());
    }
    display.set_palette(new.palette());
    *theme = new;
    recolour
}
