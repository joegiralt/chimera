//! The unit off the audio path (ADR 0108): its drivers, the boot and the UI
//! loop, in a small-code profile. `chimera-stm32`, the binary, keeps the
//! audio interrupt in the fast one and hands this crate its two audio steps
//! as plain function pointers (`Shell`), so nothing here is instantiated
//! there.
#![no_std]

pub mod bank2;
pub mod cache;
#[cfg(not(feature = "sd-probe"))]
pub mod card_dma;
// Only where `CARD_BENCH` is true: nothing else reaches it.
#[cfg(all(
    feature = "bench",
    feature = "usb-console",
    feature = "bank2",
    not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
))]
pub mod card_speed;

/// The console's `card` bench is in this build: bank 2's bench builds
/// only, where `card dma` runs, and not ORBIT's, the master tape's nor
/// TAPE's (FLASH, AXI's margin). Without it nothing sets the stream
/// bench's beds, so its heads never run.
pub const CARD_BENCH: bool = cfg!(all(
    feature = "bench",
    feature = "usb-console",
    feature = "bank2",
    not(any(
        feature = "orbit",
        feature = "master-tape",
        feature = "bench-tape"
    ))
));
pub mod clocks;
#[cfg(not(feature = "sd-probe"))]
pub mod controls;
pub mod dfu;
pub mod display;
#[cfg(feature = "fault-inject")]
pub mod fault;
#[cfg(not(feature = "sd-probe"))]
pub mod orbit;
#[cfg(not(feature = "sd-probe"))]
pub mod priority;
#[cfg(not(feature = "sd-probe"))]
pub mod probe;
pub mod sd;
#[cfg(feature = "sd-probe")]
mod sd_probe;
#[cfg(not(feature = "sd-probe"))]
pub mod shared;
#[cfg(feature = "usb-console")]
pub mod usb;
#[cfg(feature = "usb-audio")]
pub mod usb_audio;
#[cfg(feature = "midi-usb")]
mod usb_midi;
#[cfg(not(feature = "sd-probe"))]
pub mod watchdog;

use chimera_core::board::{BoardId, HC165_PA, HC165_PF, Port};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::project::{LOAD_ACK_TIMEOUT_MS, LOAD_LINK};
#[cfg(not(feature = "sd-probe"))]
use chimera_core::reset::ResetCause;
use chimera_core::ui::theme_settings::{ThemeSettings, duty, palette, tables};
use display::Stm32Display;
use stm32h7xx_hal::gpio::{Output, PD8, PD9, PD10, PushPull, Speed};
use stm32h7xx_hal::rcc::CoreClocks;
use stm32h7xx_hal::{pac, prelude::*, spi};

/// The seal of the bank-2 image this build was linked with: the seal tool
/// writes it into the `.bin` after the link. A build never sealed keeps
/// `UNSEALED` and never opens bank 2.
#[cfg(feature = "bank2")]
#[unsafe(no_mangle)]
#[used]
static BANK2_SEAL: [u8; 8] = *b"UNSEALED";

/// `BANK2_SEAL` as flash holds it, `None` unsealed.
#[cfg(feature = "bank2")]
pub fn bank2_seal() -> Option<chimera_image::Seal> {
    // SAFETY: a static's address, aligned and live. Volatile because the
    // seal tool patches the bytes after the link: the compiler must not fold
    // the placeholder into the code.
    let b = unsafe { core::ptr::read_volatile(&raw const BANK2_SEAL) };
    chimera_image::Seal::new(b)
}

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

/// The strap's pull-ups (about 40 kOhm) into the pins' few pF settle in
/// well under a microsecond; 10 us is margin. Stock reads with none.
const STRAP_SETTLE_US: u32 = 10;
// `boot`'s typed pins are the ones the core's tables name.
const _: () = assert!(HC165_PA.data == 0 && HC165_PA.load == 1 && HC165_PA.clk == 2);
const _: () = assert!(HC165_PF.data == 2 && HC165_PF.load == 1 && HC165_PF.clk == 0);

pub type Display = Stm32Display<
    spi::Spi<pac::SPI1, spi::Enabled>,
    PD8<Output<PushPull>>,
    PD9<Output<PushPull>>,
    PD10<Output<PushPull>>,
>;
#[cfg(not(feature = "sd-probe"))]
type Backlight = stm32h7xx_hal::pwm::Pwm<pac::TIM1, 1, stm32h7xx_hal::pwm::ComplementaryDisabled>;

/// The unit with the display up, as both builds start.
pub struct Board {
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
    /// Bank 2 as the boot judged it: ABOUT and `status`, and the boot's
    /// install.
    bank2: bank2::Bank2Boot,
    board_id: BoardId,
    led: stm32h7xx_hal::gpio::PE1<Output<PushPull>>,
    beat_led: orbit::BeatLed,
    backlight: Backlight,
    theme: ThemeSettings,
    iwdg: pac::IWDG,
    dbgmcu: pac::DBGMCU,
    /// The DFU marker: `dfu::enter` writes it.
    marker: dfu::Marker,
    #[cfg(feature = "usb-console")]
    usb: usb::UsbParts,
}

/// The SD bring-up probe: the card's report on the display, then a halt.
#[cfg(feature = "sd-probe")]
pub fn probe_card(mut b: Board) -> ! {
    let sd = sd::init(b.sd, &mut b.cp.DCB, &mut b.cp.DWT, &b.clocks, b.clk.cpu_hz);
    let store = sd::take_store(sd).expect("store taken once");
    sd_probe::run(&mut b.display, b.clk, store)
}

pub fn boot(mut cp: cortex_m::Peripherals, dp: pac::Peripherals, checked: dfu::Checked) -> Board {
    #[cfg(not(feature = "sd-probe"))]
    probe::paint_stack();
    fp_flush_to_zero(&mut cp.FPU);
    // The probe never enters DFU: the check ran, nothing writes the marker.
    #[cfg(feature = "sd-probe")]
    let _ = checked.seen();

    // `dfu::after_reset` read RCC_RSR and cleared it for the next reset.
    #[cfg(all(not(feature = "sd-probe"), feature = "usb-console"))]
    let seen = checked.seen();
    #[cfg(not(feature = "sd-probe"))]
    let reset_cause = checked.cause();

    cache::enable_d2_sram();
    let rev = clocks::read_rev(&dp.DBGMCU);
    let (ccdr, clk) = clocks::freeze(dp.PWR, dp.RCC, &dp.SYSCFG, rev);
    #[cfg(feature = "usb-console")]
    let ccdr = usb::route_kernel_clock(ccdr);
    cache::init(&mut cp.MPU, &mut cp.SCB, &mut cp.CPUID);
    // SAFETY: once, before audio and the rings' owner exist, so the eight
    // words (8 KiB apart, so in different lines) are nothing else's yet.
    #[cfg(not(feature = "sd-probe"))]
    chimera_core::console::SRAM4
        .store(unsafe { cache::sram4_self_test(&mut cp.SCB, &shared::sram4_words()) });
    #[cfg(not(feature = "sd-probe"))]
    shared::zero_rings();
    // Bank 2 judged before anything could call into it (ADR 0137), a
    // fault's mark first; the cycle counter bounds its erase.
    #[cfg(not(feature = "sd-probe"))]
    let _ = clocks::enable_cycle_counter(&mut cp.DCB, &mut cp.DWT);
    #[cfg(not(feature = "sd-probe"))]
    let bank2 = bank2::boot_judge(&dp.RTC, clk.cpu_hz);
    #[cfg(not(feature = "sd-probe"))]
    shared::build_waves();

    let gpioa = dp.GPIOA.split(ccdr.peripheral.GPIOA);
    let gpiod = dp.GPIOD.split(ccdr.peripheral.GPIOD);
    let gpioe = dp.GPIOE.split(ccdr.peripheral.GPIOE);
    let gpiob = dp.GPIOB.split(ccdr.peripheral.GPIOB);
    #[cfg(feature = "midi-din")]
    let _midi_rx = gpiob.pb7.into_alternate::<7>();
    // Open drain: DIN OUT's loop runs from +5 V (220 R to pin 4, PB6 by
    // 220 R to pin 5), so a 3.3 V high still lit the receiver's opto.
    // PB6 is 5 V tolerant; released, the loop floats to 5 V.
    #[cfg(all(feature = "midi-din", feature = "midi-map"))]
    let _midi_tx = gpiob.pb6.into_alternate_open_drain::<7>();
    // The board's strap before its HC165 pins (ADR 0140): PD0..PD3 pulled
    // up, left to settle, read once. They stay pulled-up inputs.
    let straps = (
        gpiod.pd0.into_pull_up_input(),
        gpiod.pd1.into_pull_up_input(),
        gpiod.pd2.into_pull_up_input(),
        gpiod.pd3.into_pull_up_input(),
    );
    clocks::delay_us(clk.cpu_hz, STRAP_SETTLE_US);
    let board_id = BoardId::from_pins([
        straps.0.is_high(),
        straps.1.is_high(),
        straps.2.is_high(),
        straps.3.is_high(),
    ]);
    // The pins `board_id.hc165()` names; the LQFP100 has no port F.
    match board_id.hc165().port {
        Port::A => {
            let _hc_data = gpioa.pa0.into_floating_input();
            let _hc_load = gpioa.pa1.into_push_pull_output();
            let _hc_clk = gpioa.pa2.into_push_pull_output();
        }
        Port::F => {
            let gpiof = dp.GPIOF.split(ccdr.peripheral.GPIOF);
            let _hc_data = gpiof.pf2.into_floating_input();
            let _hc_load = gpiof.pf1.into_push_pull_output();
            let _hc_clk = gpiof.pf0.into_push_pull_output();
        }
    }

    // The front-panel LED between D and E: low from boot, never floating.
    // A plain GPIO; TIM3_CH4 is its pin for a later fade (#307). Without
    // ORBIT it just stays low.
    let mut beat_led = gpiob.pb1.into_push_pull_output();
    beat_led.set_low();
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
    backlight.set_duty(duty(theme.bright, backlight.get_max_duty()));
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
    display.set_gamma(tables(theme.gamma));
    display.set_palette(palette(&theme));
    Board {
        cp,
        clk,
        display,
        clocks: ccdr.clocks,
        #[cfg(not(feature = "sd-probe"))]
        synth: SynthParts {
            reset_cause,
            bank2,
            board_id,
            led,
            beat_led,
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
                sweep: None,
                board: board_id,
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

/// The boot sweep's mode: a dry run until `sweep-free`, which the owner's
/// go enables.
#[cfg(not(feature = "sd-probe"))]
#[cfg(not(feature = "sweep-free"))]
const SWEEP: chimera_hal::store::Sweep = chimera_hal::store::Sweep::DryRun;
#[cfg(not(feature = "sd-probe"))]
#[cfg(feature = "sweep-free")]
const SWEEP: chimera_hal::store::Sweep = chimera_hal::store::Sweep::Free;

/// The load ack's timeout in control ticks.
#[cfg(not(feature = "sd-probe"))]
const LOAD_ACK_TICKS: u32 = LOAD_ACK_TIMEOUT_MS * controls::CONTROLS_HZ / 1000;
#[cfg(not(feature = "sd-probe"))]
const _: () = assert!(LOAD_ACK_TICKS >= 1, "the ack timeout is under a tick");

/// What step 2 needs to start the audio: the engine's inputs, the
/// interrupt controller and the clocks.
#[cfg(not(feature = "sd-probe"))]
pub struct AudioStart<'a> {
    pub nvic: &'a mut cortex_m::peripheral::NVIC,
    pub clk: clocks::Clocks,
    pub pclk2_hz: u32,
    pub shared: chimera_core::triple::Reader<chimera_core::instrument::AudioShared>,
    pub notes: chimera_core::note_queue::NoteDrain<'static, { shared::NOTE_SOURCES }>,
    pub scope: chimera_core::triple::Writer<chimera_core::scope::ScopeFrame>,
    /// The voices' formant views, once a block, for the OP pages.
    pub formant: chimera_core::triple::Writer<chimera_core::dsp::algo::engine::FormantViews>,
    pub orbit: orbit::Runner,
    pub sources: chimera_core::source_table::AudioToken,
    /// The stream rings' audio side.
    pub streams: chimera_core::stream::ring::AudioSide<'static>,
    #[cfg(feature = "midi-din")]
    pub din: chimera_core::note_queue::NoteProducer<'static>,
    /// MIDI DIN's controllers: the ISR pushes, the UI drains (MIDI map
    /// spec § 1).
    #[cfg(feature = "midi-din")]
    pub din_ctrl: &'static chimera_core::midi_in::CtrlRing,
    /// DIN OUT's bytes: the UI pushes, the ISR pops into the TX FIFO.
    #[cfg(all(feature = "midi-din", feature = "midi-map"))]
    pub din_out: &'static chimera_core::out_ring::OutRing,
}

/// MIDI out's pass: what the UI queued, then each port's handler pended to
/// send it: USART1's disarms itself once DIN's ring is empty, `OTG_FS`'s
/// starts USB's IN packets. Out of line, so the loop around it keeps its
/// shape.
#[cfg(all(feature = "midi-din", feature = "midi-map"))]
#[inline(never)]
fn midi_out(ui: &mut chimera_core::ui::UiState, out: &chimera_core::out_ring::OutPorts) {
    ui.send(out);
    // USB IN's step for the audio side (ADR 0260).
    #[cfg(all(feature = "usb-audio", not(feature = "usb-config-writer")))]
    usb_audio::PLAY_LEVEL.store(ui.midi().usb_in(), core::sync::atomic::Ordering::Relaxed);
    if !out.din.is_empty() {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::USART1);
    }
    #[cfg(feature = "midi-usb")]
    if out.usb.is_some_and(|u| !u.is_empty()) {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::OTG_FS);
    }
}

/// DIN owes the note queue with no byte to come (`MIDI_OWES`): its
/// handler retries (#884). Out of line, as `midi_out`.
#[cfg(feature = "midi-din")]
#[inline(never)]
fn din_retry() {
    if shared::MIDI_OWES.load(core::sync::atomic::Ordering::Relaxed) {
        cortex_m::peripheral::NVIC::pend(pac::Interrupt::USART1);
    }
}

/// The binary's audio side, which the start-up calls at its steps. Plain
/// function pointers: `synth` is not generic over them, so it and the UI
/// it drives are instantiated in this crate (ADR 0108).
#[cfg(not(feature = "sd-probe"))]
pub struct Shell {
    /// Step 2: the engine built, the audio and MIDI DIN started.
    pub start_audio: fn(AudioStart<'_>),
    /// The bench's screens, after step 1; their text, for the console.
    #[cfg(feature = "bench")]
    pub bench: fn(
        &mut Display,
        clocks::Clocks,
        &mut chimera_core::project::Project,
        &mut chimera_core::source_table::AudioToken,
        BenchRings<'_>,
    ) -> &'static str,
}

/// The stream rings' sides, lent to TAPE's bench before the card IRQ takes
/// its own (`TAPE STREAM`); `None` in the other areas, where it has.
#[cfg(feature = "bench")]
pub type BenchRings<'a> = Option<(
    &'a mut chimera_core::stream::ring::AudioSide<'static>,
    &'a mut chimera_core::stream::ring::CardSide<'static>,
)>;

/// The card IRQ's start, with the rings' card side: false if it can't run
/// (`card_dma::init`).
#[cfg(not(feature = "sd-probe"))]
fn start_card(
    nvic: &mut cortex_m::peripheral::NVIC,
    clocks: &CoreClocks,
    cpu_hz: u32,
    side: chimera_core::stream::ring::CardSide<'static>,
) -> bool {
    use stm32h7xx_hal::spi::{Enabled, Spi};
    let ker = Spi::<pac::SPI2, Enabled>::kernel_clk_unwrap(clocks).raw();
    card_dma::init(
        nvic,
        side,
        card_dma::Hz::new(ker),
        card_dma::Hz::new(cpu_hz),
    )
}

/// The bench's text and flush, built here with the UI's (ADR 0108):
/// called from chimera-stm32, `draw::text` and `flush` would be built
/// again there, at its opt-level.
#[cfg(feature = "bench")]
pub mod bench_screen {
    use super::Display;
    use chimera_core::ui::{draw, theme};
    use chimera_hal::ChimeraDisplay;
    use embedded_graphics::pixelcolor::Rgb565;

    /// `s` in the value face in INK, or with `label` in the label face.
    pub fn text(d: &mut Display, label: Option<Rgb565>, s: &str, (x, y): (i32, i32)) {
        match label {
            None => draw::text(d, &theme::FONT_VALUE, s, x, y, theme::INK),
            Some(c) => draw::text(d, &theme::FONT_LABEL, s, x, y, c),
        };
    }

    pub fn flush(d: &mut Display) {
        d.flush();
    }
}

#[cfg(not(feature = "sd-probe"))]
pub fn synth(board: Board, shell: Shell) -> ! {
    use chimera_core::boot::BootStage;
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
                bank2,
                board_id,
                mut led,
                beat_led,
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
    let mut stats_r = probe::init(&mut cp.DCB, &mut cp.DWT, clk, reset_cause, board_id);

    let mut controls = Stm32Controls::new();
    // Start-up, in order, behind the splash until PART 1 can be drawn:
    // 1. SYSTEM and the last project from the card, however slow;
    // 2. the controls tick, the audio and MIDI DIN;
    // 3. PART 1 drawn, then the watchdog, armed once its kicks are live
    //    (`await_live`), so no earlier step runs on its clock;
    // 4. USB under the watchdog: `preflight` checks with bounded waits what
    //    `connect` spins on with interrupts masked. A port that isn't ready
    //    is tried again from the loop every 500 ms, 3 tries in all; the
    //    last failure is toasted, and `last_usb` keeps how it ended. From
    //    a cold power-on it fails (#331): the synth plays without USB;
    // 5. the UI loop.
    // `last_stage` on the next boot says how far this one got.
    // Step 1: SYSTEM behind the splash, then its theme. Card work runs only
    // here and in the UI loop, never on the audio path.
    let _ = chimera_core::ui::splash::draw(&mut display);
    display.flush();
    let sd = sd::init(sd, &mut cp.DCB, &mut cp.DWT, &clocks, clk.cpu_hz);
    let store = sd::take_store(sd).expect("store taken once");
    // The rings' card side to the card IRQ, its audio side to the audio.
    // TAPE's bench lends both sides to its STREAM rows first.
    #[cfg_attr(not(all(feature = "bench", feature = "bench-tape")), allow(unused_mut))]
    let (mut streams, mut card_side) = shared::ring_sides().expect("ring sides taken once");
    #[cfg(not(all(feature = "bench", feature = "bench-tape")))]
    let _card_irq = start_card(&mut cp.NVIC, &clocks, clk.cpu_hz, card_side);
    let mut card = Card::new();
    // No card or a card fault shows at the project's boot; a SYSTEM
    // file that can't be read still applies the defaults silently:
    // private issue #197
    let (mut sync, mut settings, _) = SystemSync::boot(&mut card, store);
    // A staged bank 2 this build can open, installed behind DON'T POWER
    // OFF before the project, which may play from it.
    #[cfg_attr(not(feature = "bank2"), allow(unused_mut))]
    let mut status = bank2.status;
    #[cfg(feature = "bank2")]
    if let Some(flash) = &bank2.install_over
        && let Some(now) = bank2::boot_install(store, &mut display, &marker, flash, clk.cpu_hz)
    {
        status = now;
    }
    // Then the UI and NEW, so an install's door is open for NEW's maps:
    // built closed, they decode empty and NEW's template reads Modified.
    let ui = shared::take_ui().expect("UI state taken once");
    // A build without bank 2 plays TAPE and the reverb from bank 1: no
    // status, so nothing reads NEEDS UPDATE (ADR 0141).
    if cfg!(feature = "bank2") {
        ui.set_bank2(status);
    }
    ui.project_mut().set_gate_clock(gate_ms);
    // Streamed sources' files go in the registry for the card (slice 1
    // § 5.5), if its interrupt runs: else no row is lent, and a source too
    // big for the arena reads STREAMS FULL, not a silent stream. A bench
    // build's beds keep its rows to themselves.
    #[cfg(not(feature = "bench"))]
    if let Some(r) = shared::registry().filter(|_| _card_irq) {
        ui.project_mut().set_streams(r);
    }
    ui.set_theme(settings.theme);
    ui.set_midi(settings.midi);
    apply_theme(settings.theme, &mut theme, &mut backlight, &mut display);
    // Still behind the splash: the lost-cluster sweep (#360, ADR 0094),
    // only when the card's mark owes one (an unclean change), its bitmap
    // in the arena's gap before the project fills it. A clean mark costs
    // a few block reads. A doubt is reported and the boot goes on.
    let swept = chimera_core::storage::boot_sweep(store, SWEEP, ui.project_mut().sweep_scratch());
    #[cfg(feature = "usb-console")]
    let usb_parts = usb::UsbParts {
        sweep: Some(swept),
        ..usb_parts
    };
    #[cfg(not(feature = "usb-console"))]
    let _ = swept;
    // Then the last project (about 130 KB read), or NEW and why; then a
    // sweep that stopped for doubts says so.
    ui.boot_project(&mut card, store, settings.last_project);
    ui.note_sweep(&swept);
    // The splash again in the card's theme; it stays up until PART 1.
    // The Yellow Sign ignores INVERT (`splash_palette`).
    display.set_palette(chimera_core::ui::theme_settings::splash_palette(&theme));
    let _ = chimera_core::ui::splash::draw(&mut display);
    display.flush();
    display.set_palette(palette(&theme));
    marker.stage(BootStage::Card);
    let perf = PerfTracker::new();
    // The audio's one source reader, taken once the boot's source passes
    // are done (they move without waiting while none is out); the bench
    // reads the pool through it before the engine does.
    #[cfg_attr(not(feature = "bench"), allow(unused_mut))]
    let mut sources = chimera_core::source_table::SOURCE_TABLE
        .reader()
        .expect("the source reader, once");
    // The bench's screens as text, kept for the console's `bench`.
    #[cfg(feature = "bench")]
    #[cfg_attr(not(feature = "usb-console"), allow(unused_variables))]
    let bench_text = Some((shell.bench)(
        &mut display,
        clk,
        ui.project_mut(),
        &mut sources,
        #[cfg(feature = "bench-tape")]
        Some((&mut streams, &mut card_side)),
        #[cfg(not(feature = "bench-tape"))]
        None,
    ));
    #[cfg(all(feature = "bench", feature = "bench-tape"))]
    start_card(&mut cp.NVIC, &clocks, clk.cpu_hz, card_side);
    #[cfg(all(feature = "usb-console", not(feature = "bench")))]
    let bench_text: Option<&str> = None;

    // Step 2.
    controls::start_systick(cp.SYST, &mut cp.SCB, clk.cpu_hz);
    controls::wire(board_id.hc165());
    controls::enable();

    let (scope_w, mut scope_r) = shared::take_scope().expect("scope buffer taken once");
    let (formant_w, mut formant_r) = shared::take_formant().expect("formant views taken once");
    let (mut shared_w, shared_r, knobs) =
        shared::take_audio(ui.project().perf()).expect("audio buffer taken once");
    // The kits, published before the audio starts (ADR 0265).
    let mut kits = chimera_core::dsp::drum::view::KitWriter::take().expect("kit writer once");
    kits.publish_all(ui.project().kits());
    // Without MIDI DIN, USB or `card` nothing takes a producer.
    #[cfg_attr(
        not(any(
            feature = "midi-din",
            feature = "midi-usb",
            all(
                feature = "bench",
                feature = "usb-console",
                not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
            )
        )),
        allow(unused_mut, unused_variables)
    )]
    let (mut producers, notes) = shared::NOTES.split().expect("note sources split once");
    #[cfg(any(feature = "midi-din", feature = "midi-usb"))]
    let ctrl = shared::take_ctrl().expect("control rings taken once");
    #[cfg(all(feature = "midi-din", feature = "midi-map"))]
    let out = shared::take_out().expect("out rings taken once");
    // USB MIDI's, for the port's poll once it connects (step 4).
    #[cfg(feature = "midi-usb")]
    let usb_midi = usb_midi::Sinks {
        notes: producers.take(const { shared::source(shared::Port::Usb) }),
        ctrl: &ctrl.usb,
        #[cfg(all(feature = "midi-din", feature = "midi-map"))]
        out: out.usb,
        #[cfg(not(all(feature = "midi-din", feature = "midi-map")))]
        out: None,
    };
    #[cfg(all(feature = "usb-console", not(feature = "midi-usb")))]
    let usb_midi = ();
    let (orbit_runner, mut orbit) = orbit::take(beat_led);
    (shell.start_audio)(AudioStart {
        nvic: &mut cp.NVIC,
        clk,
        pclk2_hz: clocks.pclk2().raw(),
        shared: shared_r,
        notes,
        scope: scope_w,
        formant: formant_w,
        orbit: orbit_runner,
        sources,
        streams,
        #[cfg(feature = "midi-din")]
        din: producers
            .take(const { shared::source(shared::Port::Din) })
            .expect("DIN producer taken once"),
        #[cfg(feature = "midi-din")]
        din_ctrl: &ctrl.din,
        #[cfg(all(feature = "midi-din", feature = "midi-map"))]
        din_out: out.din,
    });

    marker.stage(BootStage::Audio);

    // The console's `card`: the measurement over the card, its chord on
    // the `CARD` note source. Only in bank 2's bench builds, where `card
    // dma` runs (#568); not ORBIT's, the master tape's nor TAPE's: with it
    // they no longer fit FLASH, or AXI's margin (TAPE's comes back with
    // streaming: #539).
    #[cfg(all(
        feature = "bench",
        feature = "usb-console",
        feature = "bank2",
        not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
    ))]
    let mut card_bench = card_speed::take(
        producers
            .take(shared::CARD)
            .expect("card producer taken once"),
        clk.cpu_hz,
    )
    .expect("card bench taken once");

    // Step 3: PART 1 replaces the splash.
    let (mut pacer, first) = chimera_core::ui::animation::Pacer::start(controls::now_ms());
    ui.update(first);
    ui.render_with_audio(&mut display, &perf.stats, None, scope_r.read());
    display.flush();
    ui.prime_regions(&perf.stats, None, scope_r.read());
    led.set_low();

    watchdog::start(iwdg, &dbgmcu, watchdog::await_live(clk.cpu_hz));
    marker.stage(BootStage::Armed);

    // Step 4. On the bus only now: the loop below polls it from here on.
    #[cfg(feature = "usb-console")]
    let mut usb = usb::PortState::new(usb_parts, usb_midi);
    #[cfg(feature = "usb-console")]
    let mut usb_snap = controls::now_ms();
    #[cfg(feature = "usb-console")]
    tend_usb(&mut usb, ui, &marker, &clocks, clk.cpu_hz, &mut usb_snap);
    marker.stage(BootStage::Running);

    let mut last_tick = controls::ticks();
    loop {
        // A write's quiet released 20 ms after its last write; then a
        // stuck card transfer aborted, a stranded want woken.
        if let Some(q) = sd::BUS.settle(controls::now_ms()) {
            shared::STREAM_STATS.note_quiet(q);
        }
        card_dma::watch(controls::now_ms());
        // A `card dma` run moved on: B6 redraws every row, and saves the
        // project once a minute.
        #[cfg(all(
            feature = "usb-console",
            feature = "bench",
            feature = "bank2",
            not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
        ))]
        // A save is card work: an errand.
        let bench_redraw = usb.errand(|e| {
            let ask = card_bench.tick(controls::now_ms(), store, stats_r.as_mut());
            if ask.save {
                card_bench.saved(bench_save(e, &mut card, store, ui));
            }
            ask.redraw
        });
        #[cfg(not(all(
            feature = "usb-console",
            feature = "bench",
            feature = "bank2",
            not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
        )))]
        let bench_redraw = false;
        // The snapshot point: every path through the last iteration flushed.
        #[cfg(feature = "usb-console")]
        tend_usb(&mut usb, ui, &marker, &clocks, clk.cpu_hz, &mut usb_snap);
        #[cfg(all(
            feature = "usb-console",
            feature = "bench",
            feature = "bank2",
            not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
        ))]
        let card_run: usb::CardRun = &mut |_, arg, s, store| Some(card_bench.answer(arg, store, s));
        #[cfg(all(
            feature = "usb-console",
            not(all(
                feature = "bench",
                feature = "bank2",
                not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
            ))
        ))]
        let card_run: usb::CardRun = &mut |_, _, _, _| None;
        #[cfg(feature = "usb-console")]
        if let Some(restart) = usb.up().and_then(|u| {
            u.service(
                ui,
                stats_r.as_mut(),
                bench_text,
                &mut display,
                card_run,
                store,
            )
        }) {
            // As the menu's path: a THEME change in this visit is kept.
            usb.errand(|e| ui.sync_system_now(e, &mut sync, &mut card, store, &mut settings));
            match restart {
                usb::Restart::Dfu(asked) => dfu::enter(&marker, dfu::DfuFrom::Console(asked)),
                // The boot it restarts into sweeps, the arena's gap free.
                usb::Restart::Sweep(_) => marker.reset(),
            }
        }
        // `bank2 install` held the screen: everything redraws below.
        #[cfg(feature = "usb-console")]
        let installed = usb.up().is_some_and(|u| u.take_redraw());
        #[cfg(not(feature = "usb-console"))]
        let installed = false;
        // SYSTEM's PUSH ENCODERS as last edited: off, the read stays 24 bits.
        controls::set_push(ui.push_encoders());
        // The console's `key` and `turn`, at the scan's level.
        #[cfg(feature = "usb-console")]
        controls::hand(
            &usb.up()
                .map_or_else(Default::default, |u| u.hand(controls::now_ms())),
        );
        controls.snapshot();
        // ORBIT's latest view: the pages draw it, the drift is absorbed.
        orbit.see(ui);
        // The OP pages' band: the edited Part's newest voice.
        ui.set_formant_views(formant_r.read());
        // Every frame, even idle: a held key must age.
        ui.handle_input(&controls);
        // Mapped CCs, in the room the hand's moves left: DIN's then USB's,
        // one pass, one budget. No bench run plays a controller: bench
        // firmware leaves the MIDI map out (FLASH, #507) and drops the rings
        // unread, so they never fill.
        #[cfg(all(any(feature = "midi-din", feature = "midi-usb"), feature = "midi-map"))]
        ui.hear_ccs(ctrl.drain(), controls::now_ms(), shared_w.taken());
        #[cfg(all(
            any(feature = "midi-din", feature = "midi-usb"),
            not(feature = "midi-map")
        ))]
        ctrl.clear();
        #[cfg(all(feature = "midi-din", feature = "midi-map"))]
        midi_out(ui, &out);
        #[cfg(feature = "midi-din")]
        din_retry();
        // Card work and SYSTEM's sync can hold the UI for seconds: an
        // errand, so no false storm from the guard's service test.
        let errand = |e: &chimera_core::errand::Errand| {
            // SYSTEM synced first: a THEME change made in this visit is kept.
            if let Some(yes) = ui.take_dfu_synced(e, &mut sync, &mut card, store, &mut settings) {
                dfu::enter(&marker, dfu::DfuFrom::Menu(yes));
            }
            // Card work the keys asked for, under BUSY. A load settles
            // before it publishes: the ack, or `LOAD_ACK_TICKS`.
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
            ui.card_work(e, cx, &LOAD_LINK, |swap, u| {
                let t0 = controls::ticks();
                let _ = swap.settle(&LOAD_LINK, || {
                    controls::ticks().wrapping_sub(t0) < LOAD_ACK_TICKS
                });
                shared_w.publish(|b| publish(b, u, knobs));
                kits.publish_all(u.project().kits());
            });
            // Leaving SETTINGS syncs SYSTEM, with no overlay first: a save
            // is quicker than BUSY can be read. A toast says how it went.
            ui.sync_system(e, &mut sync, &mut card, store, &mut settings);
            busy
        };
        #[cfg(feature = "usb-console")]
        let busy = usb.errand(errand);
        // SAFETY: no USB guard in this build.
        #[cfg(not(feature = "usb-console"))]
        let busy = unsafe { chimera_core::errand::Errand::run(errand) };
        // SETTINGS › THEME: the UI loop owns the display and the backlight.
        let recolour = apply_theme(ui.theme(), &mut theme, &mut backlight, &mut display);
        // The loop spins as fast as it can; animation runs at UI_FPS.
        if let Some(t) = pacer.due(controls::now_ms()) {
            ui.update(t);
            // ORBIT's beat on the front-panel LED; off when it is stopped.
            orbit.beat(ui);
        }
        shared_w.publish(|b| publish(b, ui, knobs));
        kits.publish_all(ui.project().kits());
        ui.take_hits(&mut kits);
        let stats = audio_stats(stats_r.as_mut());
        // Read after the card work; the toast's first step ignores it.
        let now = controls::ticks();
        let elapsed_ms = now.wrapping_sub(last_tick) * 1_000 / controls::CONTROLS_HZ;
        last_tick = now;
        ui.step_idle(elapsed_ms, busy);
        let toast = ui.step_toast(elapsed_ms);
        if toast == ToastStep::Ended || busy || installed || bench_redraw {
            // The toast, BUSY or the install covered rows the dirty regions
            // don't know about; B6 redraws them all regardless.
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

/// B6's save of the open project over its own file: true if it landed.
#[cfg(all(
    feature = "usb-console",
    feature = "bench",
    feature = "bank2",
    not(any(feature = "orbit", feature = "master-tape", feature = "bench-tape"))
))]
#[inline(never)]
fn bench_save(
    _: &chimera_core::errand::Errand,
    card: &mut chimera_core::storage::Card,
    store: &mut sd::SdStore,
    ui: &mut chimera_core::ui::UiState,
) -> bool {
    use chimera_core::project::{ProjectNote, SaveTo, save_project_as};
    let name = ui.project().meta().name();
    let saved = save_project_as(card, store, ui.project_mut(), SaveTo::Own, name);
    matches!(saved.out, ProjectNote::Saved(_))
}

/// A USB try if one is due, and the host's progress: each step, the try's
/// end and a register snapshot (every `USB_RETRY_MS`) kept for the next
/// boot's line; the last failed try toasted.
#[cfg(feature = "usb-console")]
fn tend_usb(
    port: &mut usb::PortState,
    ui: &mut chimera_core::ui::UiState,
    marker: &dfu::Marker,
    clocks: &CoreClocks,
    cpu_hz: u32,
    last_snap: &mut chimera_hal::Ms,
) {
    use chimera_core::boot::{USB_RETRY_MS, UsbState};
    let now = controls::now_ms();
    let tried = port.try_up(now, clocks, cpu_hz, &mut |s, n| marker.usb_step(s, n));
    if let Some(state) = tried {
        marker.usb(state);
        if let UsbState::Off { why, .. } = state
            && state.gave_up()
        {
            ui.show_boot_fault(why.toast());
        }
    }
    if let Some((s, n)) = port.host_step() {
        marker.usb_step(s, n);
    }
    if tried.is_some() || now.since(*last_snap) >= USB_RETRY_MS {
        *last_snap = now;
        marker.usb_regs(usb::regs());
    }
}

/// Every Part, the FX and ORBIT into an audio publish: the loop's and a
/// load's, one copy in flash.
#[cfg(not(feature = "sd-probe"))]
#[inline(never)]
fn publish(
    b: &mut chimera_core::instrument::AudioShared,
    ui: &chimera_core::ui::UiState,
    knobs: &mut chimera_core::dsp::knobs::KnobPublisher,
) {
    b.update_from_project(ui.project(), LOAD_LINK.epoch(), knobs);
    orbit::publish(ui, b);
    ui.publish_ui(b);
}

/// The audio's latest stats with the stack's high-water mark, as the
/// AUDIO LOAD page and the console's `stats` both read them.
#[cfg(not(feature = "sd-probe"))]
pub fn audio_stats(
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
    let recolour = palette(&new) != palette(theme);
    backlight.set_duty(duty(new.bright, backlight.get_max_duty()));
    if new.gamma != theme.gamma {
        display.set_gamma(tables(new.gamma));
    }
    display.set_palette(palette(&new));
    *theme = new;
    recolour
}

/// The source gate's clock (ADR 0087): ms from SysTick, at 2 ms steps.
#[cfg(not(feature = "sd-probe"))]
fn gate_ms() -> u32 {
    controls::ticks().wrapping_mul(1_000 / controls::CONTROLS_HZ)
}
