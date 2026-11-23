// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The USB console's shell (ADR 0068): CDC-ACM on the OTG FS port (PA11/PA12).
//! The device lives in a static cell the UI reaches through `with`.
//! Without `midi-usb` only the UI loop polls it and `pac::Interrupt::OTG_FS`
//! is never unmasked. With it, the `OTG_FS` handler polls it too, at MIDI's
//! level under audio; `with` masks the handler. A packet no class takes
//! parks RXFLVL until the UI's next hold, and a storm guard falls back to
//! the UI's polls (spec §§ 5, 6).

use chimera_core::board::BoardId;
use chimera_core::boot::{BootSeen, UsbOff, UsbRegs, UsbRetry, UsbState, UsbStep, wait_until};
use chimera_core::console::{
    AnswerClock, Bank2Answer, BankRegs, CardAnswer, CardArg, Console, FlashPeek, FlashRegs, Frame,
    Hand, Hands, LoopTimer, Out, PEEK_AT, Refusal, Request, RestartKind, Served, Stalled, Stats,
    SweepAsk, Unit, WallClock, answer, restart_after, serial_hex,
};
use chimera_core::perf::load::AudioStats;
use chimera_core::triple::Reader;
use chimera_core::ui::UiState;
use chimera_hal::Ms;
use chimera_hal::store::SweepRun;
use cortex_m::peripheral::DWT;
use stm32h7xx_hal::gpio::{Alternate, PA11, PA12};
use stm32h7xx_hal::pac;
use stm32h7xx_hal::rcc::rec::{self, UsbClkSel, UsbClkSelGetter};
use stm32h7xx_hal::rcc::{Ccdr, CoreClocks, ResetEnable};
use stm32h7xx_hal::signature::Uid;
use stm32h7xx_hal::usb_hs::UsbBus;
use synopsys_usb_otg::UsbPeripheral;
use usb_device::UsbError;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{UsbDevice, UsbDeviceState};
use usbd_serial::SerialPort;

use crate::sd::SdStore;
#[cfg(feature = "midi-usb")]
use chimera_core::console::{RxHead, RxPark, StormKinds, StormWindow, trip};
use chimera_core::errand::Errand;
use chimera_hal::units::Cycles;
#[cfg(feature = "midi-usb")]
use core::sync::atomic::Ordering;
use core::{cell::UnsafeCell, mem::MaybeUninit};
#[cfg(feature = "midi-usb")]
use cortex_m::peripheral::NVIC;
#[cfg(feature = "midi-usb")]
use stm32h7xx_hal::pac::interrupt;

/// The stock PreenFM3's ID: Ixox/preenfm3 firmware/Src/usbd_desc.c.
pub const VID_PID: (u16, u16) = (0x0483, 0x5740);
// Never the ROM's DFU ID, so `dfu-util -d 0x0483:0xdf11` cannot match the
// running synth.
const _: () = assert!(!(VID_PID.0 == 0x0483 && VID_PID.1 == 0xDF11));

/// After `dfu`'s `OK` is flushed, how long the port keeps polling so the
/// host collects it before the reset. Full speed polls bulk IN every 1 ms
/// frame; 20 frames covers a busy host.
const DRAIN_MS: u32 = 20;

/// Each USB precondition's wait at most. VDD33USB and the core reset
/// settle in well under a millisecond once powered; this leaves a cold
/// supply room. The waits keep interrupts on, so the armed watchdog is
/// kicked through them.
const READY_WAIT_MS: u32 = 50;

/// VDD33USB's detector, from USB33DEN to USB33RDY, at most.
const USB33_WAIT_MS: u32 = 100;

/// Bytes read per loop top at most: a flood without a newline can't hold the UI.
const READ_BUDGET: usize = 256;

pub(crate) type Bus = UsbBus<Otg2>;
pub(crate) type Dev = UsbDevice<'static, Bus>;
pub(crate) type Serial = SerialPort<'static, Bus>;

/// `bcdDevice` of the composite device, MIDI configured or not: a new
/// layout under the same VID:PID, which Windows caches drivers by. 0.2's
/// console-only device was 0x0010; one cable 0x0020; a cable a Part
/// 0x0021 (ADR 0243).
#[cfg(all(feature = "midi-usb", not(feature = "usb-audio")))]
pub(crate) const MIDI_RELEASE: u16 = 0x0021;
/// With USB audio's stream in MIDI's function (ADR 0248), one cable;
/// the fallback build's, written by usb-device.
#[cfg(all(feature = "usb-audio", feature = "usb-config-writer"))]
pub(crate) const MIDI_RELEASE: u16 = 0x0030;
/// With the host's playback too (ADR 0260), served from the class's
/// static.
#[cfg(all(feature = "usb-audio", not(feature = "usb-config-writer")))]
pub(crate) const MIDI_RELEASE: u16 = 0x0040;

/// OTG2, the OTG FS core on PA11/PA12, for `UsbBus`. Its `enable` only
/// sets the clock bit, so `UsbBus::enable`'s masked, unbounded spins
/// (AHBIDL, then PHYSEL and CSRST) repeat exactly what `preflight` just
/// saw complete. The HAL's `USB2::enable` would also pulse the block's
/// RCC reset and set USB33DEN inside that masked section, which
/// `preflight` never checked. From a cold power-on CSRST never completes
/// and the console stays off until a pass through the ROM loader:
/// private issue #331
pub struct Otg2 {
    _global: pac::OTG2_HS_GLOBAL,
    _device: pac::OTG2_HS_DEVICE,
    _pwrclk: pac::OTG2_HS_PWRCLK,
    hclk: u32,
}

// SAFETY: as the HAL's `USB2` (stm32h7xx-hal 0.16 usb_hs.rs): the register
// blocks are owned here; `UsbBus` writes them, through `REGISTERS`, under
// its own critical sections, and `regs()` only reads them.
unsafe impl Sync for Otg2 {}

// SAFETY: REGISTERS is OTG2's global block, which this type owns with its
// device and power-and-clock blocks; the constants are the HAL's for USB2
// (RM0433: 9 endpoints, 4 KB of FIFO RAM).
unsafe impl UsbPeripheral for Otg2 {
    const REGISTERS: *const () = pac::OTG2_HS_GLOBAL::ptr() as *const ();
    const HIGH_SPEED: bool = true;
    const FIFO_DEPTH_WORDS: usize = 1024;
    const ENDPOINT_COUNT: usize = 9;

    fn enable() {
        // SAFETY: AHB1ENR's USB2OTGEN is this port's own clock bit; set
        // in a critical section, as the HAL sets it.
        let rcc = unsafe { &*pac::RCC::ptr() };
        cortex_m::interrupt::free(|_| rcc.ahb1enr.modify(|_, w| w.usb2otgen().set_bit()));
    }

    fn ahb_frequency_hz(&self) -> u32 {
        self.hclk
    }
}

/// Routes HSI48, which the CRS trims once the port is up, to the USB
/// kernel clock. At boot, before `ccdr.peripheral` is split up.
pub fn route_kernel_clock(mut ccdr: Ccdr) -> Ccdr {
    ccdr.peripheral.kernel_usb_clk_mux(UsbClkSel::Hsi48);
    ccdr
}

/// What `boot()` hands over, nothing enabled yet.
pub struct UsbParts {
    pub dm: PA11<Alternate<10>>,
    pub dp: PA12<Alternate<10>>,
    pub global: pac::OTG2_HS_GLOBAL,
    pub device: pac::OTG2_HS_DEVICE,
    pub pwrclk: pac::OTG2_HS_PWRCLK,
    pub rec: rec::Usb2Otg,
    pub crs: pac::CRS,
    pub crs_rec: rec::Crs,
    /// What the top of `main` saw: `status`'s `boot` line.
    pub boot: BootSeen,
    /// The boot's card sweep, run or skipped: `status`'s `sweep` line.
    pub sweep: Option<SweepRun>,
    /// The board's strap: `status`'s `board` line.
    pub board: BoardId,
    /// ITCM zeroed: `ed`'s state is built there (ADR 0286).
    pub itcm: crate::shared::ItcmZeroed,
}

/// `dfu` answered and its `OK` flushed: only `Usb::service` makes one, and
/// only `dfu::enter` takes one.
#[must_use]
pub struct DfuAsked(());

/// `sweep` answered, its mark set to `Asked` and its `OK` flushed: the
/// boot it restarts into sweeps (ADR 0094). Only `Usb::service` makes one.
#[must_use]
pub struct SweepAsked(());

/// `bank2 install` opened the door and its `OK` is out: the boot it
/// restarts into lays USB out with MIDI and audio (#1064). Only
/// `Usb::service` makes one.
#[must_use]
pub struct Bank2Opened(());

/// A console answer that ends in a restart.
pub enum Restart {
    Dfu(DfuAsked),
    Sweep(SweepAsked),
    Bank2(Bank2Opened),
}

/// `card`'s measurement, run before its answer starts: `None` without one
/// in this build.
pub type CardRun<'a> = &'a mut dyn FnMut(
    &Errand,
    CardArg,
    Option<&mut Reader<AudioStats>>,
    &mut SdStore,
) -> Option<CardAnswer>;

/// The port, the half-read request line, and the loop's time between tops.
pub struct Usb {
    link: Link,
    line: Console,
    timer: LoopTimer,
    /// Where `stats` last read the card IRQ's cycles.
    irq: crate::card_dma::IrqMark,
    /// The shares' wall time, in DWT cycles.
    wall: WallClock,
    /// The USB interrupt's cycles, and where `stats` last read them.
    #[cfg(feature = "midi-usb")]
    usb_irq: UsbIrq,
    /// DWT at the last loop top, and whether that iteration answered.
    top: u32,
    last: Served,
    cycles_per_us: u32,
    boot: BootSeen,
    sweep: Option<SweepRun>,
    /// `bank2 install` drew over the screen: the loop redraws it whole.
    redraw: bool,
    board: BoardId,
    /// `key` and `turn`'s gestures, played into the controls each lap.
    hands: Hands,
    /// `ed set`'s queue, applied in the UI pass, and `ed since`'s copy. In
    /// ITCM (`shared::take_ed`), not this frame: the copy of the values would
    /// take `main`'s stack. `None`: the cell was taken twice.
    ed: Option<&'static mut chimera_core::console::EdState>,
}

/// The port set up but not on the bus: no pull-up, so the host sees
/// nothing until `connect`. Only `init` makes one.
pub struct Unconnected {
    bus: &'static UsbBusAllocator<UsbBus<Otg2>>,
    serial: Serial,
    #[cfg(feature = "midi-usb")]
    midi: crate::usb_midi::Midi,
    uid: &'static str,
    cpu_hz: u32,
    boot: BootSeen,
    sweep: Option<SweepRun>,
    board: BoardId,
    itcm: crate::shared::ItcmZeroed,
}

/// The port's preconditions held, each checked with a bounded wait: only
/// `preflight` makes one, and `init` takes it with the parts.
#[must_use]
pub struct Ready(UsbParts);

/// The port's bring-up, each wait bounded, interrupts on: HSI48RDY (the
/// kernel clock), USB33DEN then USB33RDY (VDD33USB), the OTG clock, then
/// AHBIDL and a core soft reset (CSRST) on the FS PHY. The last two are
/// what `connect`'s enable (synopsys-usb-otg `UsbBus::enable`) spins on
/// with interrupts masked and no timeout. `Err` names the first that
/// never came true and hands the parts back for the next try.
pub fn preflight(parts: UsbParts, cpu_hz: u32) -> Result<Ready, (UsbParts, UsbOff)> {
    match check(&parts, cpu_hz) {
        Ok(()) => Ok(Ready(parts)),
        Err(why) => Err((parts, why)),
    }
}

fn check(parts: &UsbParts, cpu_hz: u32) -> Result<(), UsbOff> {
    let limit = READY_WAIT_MS * (cpu_hz / 1000);
    let wait = |ready: &mut dyn FnMut() -> bool| wait_until(limit, DWT::cycle_count, ready);
    // SAFETY: RCC and PWR are owned by the HAL after `freeze`; this reads
    // HSI48RDY, sets PWR_CR3.USB33DEN (bit 24), the bit the HAL's own
    // `USB2::enable` sets the same way, reads USB33RDY (bit 26, RM0433
    // PWR_CR3; the PAC's `usb33rdy`), and sets AHB1ENR's USB2OTGEN, this
    // port's own clock bit. Both registers are shared, and these
    // read-modify-writes run outside a critical section: sound because no
    // interrupt handler writes RCC or PWR.
    let (rcc, pwr) = unsafe { (&*pac::RCC::ptr(), &*pac::PWR::ptr()) };
    if !wait(&mut || rcc.cr.read().hsi48rdy().is_ready()) {
        return Err(UsbOff::Hsi48);
    }
    // VDD33USB seen before the core reset, as the ROM loader leaves it.
    pwr.cr3.modify(|_, w| w.usb33den().set_bit());
    let usb33 = USB33_WAIT_MS * (cpu_hz / 1000);
    if !wait_until(usb33, DWT::cycle_count, || {
        pwr.cr3.read().usb33rdy().bit_is_set()
    }) {
        return Err(UsbOff::Usb33);
    }
    rcc.ahb1enr.modify(|_, w| w.usb2otgen().set_bit());
    let global = &parts.global;
    // The HAL's order: PHYSEL (the FS PHY's 48 MHz), ≥ 10 PHY clocks, then
    // AHBIDL and CSRST. Back to back, CSRST sticks from cold (#331 test).
    global.gusbcfg.modify(|_, w| w.physel().set_bit());
    crate::clocks::delay_us(cpu_hz, 10);
    if !wait(&mut || global.grstctl.read().ahbidl().bit_is_set()) {
        return Err(UsbOff::AhbIdle);
    }
    global.grstctl.modify(|_, w| w.csrst().set_bit());
    if !wait(&mut || global.grstctl.read().csrst().bit_is_clear()) {
        return Err(UsbOff::CoreReset);
    }
    Ok(())
}

/// Sets the port up, once: the CRS trimming HSI48 from the host's SOF, the
/// OTG core and the CDC-ACM class. Nothing connects yet (`connect`).
pub fn init(ready: Ready, midi: MidiSinks, clocks: &CoreClocks, cpu_hz: u32) -> Unconnected {
    let Ready(parts) = ready;
    let UsbParts {
        dm,
        dp,
        global,
        device,
        pwrclk,
        rec,
        crs,
        crs_rec,
        boot,
        sweep,
        board,
        itcm,
    } = parts;
    crs_rec.enable();
    // SAFETY: SYNCSRC is a 2-bit field and 0b11 is a defined value, USB2
    // OTG FS SOF (RM0433 CRS_CFGR); CEN is still clear, so it may be written.
    crs.cfgr.modify(|_, w| unsafe { w.syncsrc().bits(0b11) });
    crs.cr
        .modify(|_, w| w.autotrimen().set_bit().cen().set_bit());

    // Without its 48 MHz the core never leaves CSRST and `connect` spins
    // there, interrupts off.
    assert_eq!(rec.get_kernel_clk_mux(), UsbClkSel::Hsi48);
    // PA11/PA12 are AF10 already; holding them keeps them so.
    let _pins = (dm, dp);
    let usb = Otg2 {
        _global: global,
        _device: device,
        _pwrclk: pwrclk,
        hclk: clocks.hclk().raw(),
    };
    // EP OUT packets only (EP0's, the console's bulk OUT and MIDI's, 64
    // bytes each); the TX FIFOs live in the core's own RAM.
    let ep_memory = cortex_m::singleton!(: [u32; 256] = [0; 256]).expect("EP memory taken once");
    let bus = UsbBus::new(usb, ep_memory);
    // Here, so the device and the class can both borrow it for 'static.
    let bus: &'static UsbBusAllocator<UsbBus<Otg2>> =
        cortex_m::singleton!(: UsbBusAllocator<UsbBus<Otg2>> = bus).expect("USB bus taken once");

    let serial = SerialPort::new(bus);
    // The chip's 96-bit UID: a stable /dev/serial/by-id name per unit.
    let uid = cortex_m::singleton!(: chimera_core::console::SerialNumber = serial_hex(Uid::read()))
        .expect("serial number taken once");
    // USB MIDI only if bank 2's door is open now: this boot's layout.
    #[cfg(feature = "midi-usb")]
    let midi = crate::usb_midi::Midi::alloc(bus, midi, uid.as_str());
    #[cfg(not(feature = "midi-usb"))]
    let () = midi;
    Unconnected {
        bus,
        serial,
        #[cfg(feature = "midi-usb")]
        midi,
        uid: uid.as_str(),
        cpu_hz,
        boot,
        sweep,
        board,
        itcm,
    }
}

impl Unconnected {
    /// Builds the device, which enables the core and its D+ pull-up: the
    /// host starts enumerating now, so only the UI loop that polls it may
    /// call this, at its top (the spec: the port appears once the UI loop
    /// starts). A bench build's screens hold for minutes before that, and
    /// an unpolled device that long is one the host gives up on.
    pub fn connect(self) -> Usb {
        // MIDI or not (the slot filled at `init`), fixed for the boot: a
        // host never sees the layout change under it.
        #[cfg(feature = "midi-usb")]
        let midi = Some(MIDI_RELEASE);
        #[cfg(not(feature = "midi-usb"))]
        let midi = None;
        let core = UsbCore {
            dev: chimera_midi::usb_class::device(self.bus, VID_PID, self.uid, midi),
            serial: self.serial,
            #[cfg(feature = "midi-usb")]
            midi: self.midi,
            #[cfg(feature = "midi-usb")]
            irq: Irq {
                window: StormWindow::new(),
                // SysTick runs from boot step 1: the tick test holds from here.
                tick: crate::controls::now_ms().0,
                tick_at: crate::shared::dwt(),
                alive: None,
                per_ms: Cycles::new((self.cpu_hz / 1_000).max(1)),
                held: false,
                park: RxPark::new(),
                counts: Counts {
                    cycles: 0,
                    us_max: 0,
                    storms: StormKinds::new(),
                    rx_parks: 0,
                    in_drops: 0,
                    out_drops: 0,
                    pipe_drops: 0,
                },
            },
        };
        #[cfg(feature = "midi-usb")]
        let usb_irq = UsbIrq::new(core.irq.counts);
        let link = install(core);
        Usb {
            link,
            line: Console::new(),
            timer: LoopTimer::new(),
            irq: crate::card_dma::IrqMark::new(0),
            wall: WallClock::new(crate::shared::dwt(), crate::controls::now_ms()),
            #[cfg(feature = "midi-usb")]
            usb_irq,
            top: DWT::cycle_count(),
            last: Served::Idle,
            cycles_per_us: (self.cpu_hz / 1_000_000).max(1),
            boot: self.boot,
            sweep: self.sweep,
            redraw: false,
            board: self.board,
            hands: Hands::new(),
            ed: crate::shared::take_ed(self.itcm),
        }
    }
}

impl Usb {
    /// The UI loop's first line, its snapshot point: the last iteration has
    /// flushed, so `frame` is what the panel shows. Times the last lap (an
    /// answered one is not counted), polls, reads into the line, and
    /// answers at most one request: `Some` for a `dfu` the host was told
    /// `OK` to. A `card` runs `card` first, so its seconds come before the
    /// answer's clock starts; so does `sweep`'s request, whose `OK` is
    /// a restart into the sweep; so does `bank2 install`, its seconds of
    /// writing behind its own screen (`take_redraw`). `ls`, `put`, `rm` and
    /// `get` work on `store`; a Sound `put` or `rm` drops the library's
    /// page after its answer. `shot` reads `display` as the last flush
    /// left it. Out of line: inlined, its buffers took `synth`'s frame past
    /// the 8 KB check.
    #[inline(never)]
    pub fn service(
        &mut self,
        ui: &mut UiState,
        mut stats: Option<&mut Reader<AudioStats>>,
        bench: Option<&str>,
        display: &mut crate::Display,
        card: CardRun<'_>,
        store: &mut SdStore,
    ) -> Option<Restart> {
        let now = DWT::cycle_count();
        let us = now.wrapping_sub(self.top) / self.cycles_per_us;
        self.top = now;
        self.timer.lap(us, self.last);
        // Every pass, inside the IRQ counter's wrap and the wall clock's.
        crate::card_dma::irq_cycles();
        let per_ms = Cycles::new(self.cycles_per_us * 1_000);
        self.wall
            .fold(Cycles::new(now), crate::controls::now_ms(), per_ms);
        #[cfg(feature = "bank2")]
        let cpu_hz = self.cycles_per_us * 1_000_000;
        #[cfg(feature = "usb-audio")]
        let mut audio_text = crate::usb_audio::Line::new();
        let Usb {
            link,
            line,
            timer,
            irq,
            wall,
            #[cfg(feature = "midi-usb")]
            usb_irq,
            cycles_per_us,
            boot,
            sweep,
            #[cfg(feature = "bank2")]
            redraw,
            board,
            hands,
            ed,
            ..
        } = self;
        // One hold a lap: a storm's mask lifted (`unlock` then unmasks), the
        // counters read, the device polled, the console flushed and read up
        // to one request. The line parses under the hold: what follows a
        // request's newline must wait in the port for the next lap.
        let req = link.with(|c| {
            #[cfg(feature = "midi-usb")]
            {
                c.irq.held = false;
                usb_irq.fold(c.irq.counts);
            }
            c.poll_all();
            // A stalled answer's tail still waiting: no new request until
            // the host reads it, so a host that writes but never reads NAKs
            // itself instead of costing a stall every lap.
            let req = match c.serial.flush() {
                Err(UsbError::WouldBlock) => None,
                _ => read(&mut c.serial, line),
            };
            // `bench` answers USB audio's counters where nothing else does.
            #[cfg(feature = "usb-audio")]
            if req.is_some() {
                let _ = c.midi.audio_report(&mut audio_text);
            }
            req
        });
        #[cfg(feature = "usb-audio")]
        let bench = bench.or(Some(audio_text.as_str()));
        self.last = Served::Idle;
        if let Some(req) = req {
            // A bench `card`, `sweep`'s scan, `bank2`'s write: seconds with
            // no hold, so an errand. Outside a bench build nothing
            // measures, and none of it links.
            let (card, sweep_ask, bank2) = link.errand(|e| {
                let card = match req {
                    Ok(Request::Card(arg)) if cfg!(feature = "bench") => {
                        card(e, arg, stats.as_deref_mut(), store)
                    }
                    _ => None,
                };
                let sweep_ask = match req {
                    Ok(Request::Sweep(_)) => Some(chimera_core::console::ask_sweep(
                        e,
                        ui.project_status() == chimera_core::project::ProjectStatus::Modified,
                        store,
                    )),
                    _ => None,
                };
                // Bank 2's write, seconds at most, before the answer's
                // clock starts; the screen it drew is redrawn whole after.
                let bank2 = match req {
                    #[cfg(feature = "bank2")]
                    Ok(Request::Bank2(chimera_core::console::Bank2Arg::Install)) => {
                        *redraw = true;
                        Some(crate::bank2::run_install(e, store, ui, display, cpu_hz))
                    }
                    #[cfg(feature = "bank2-gate")]
                    Ok(Request::Bank2(chimera_core::console::Bank2Arg::Erase)) => {
                        Some(crate::bank2::run_erase(e, ui, cpu_hz))
                    }
                    #[cfg(feature = "bank2-gate")]
                    Ok(Request::Bank2(chimera_core::console::Bank2Arg::SlotPoke)) => {
                        Some(crate::bank2::run_slot_poke(e, ui, cpu_hz))
                    }
                    #[cfg(feature = "bank2-gate")]
                    Ok(Request::Bank2(chimera_core::console::Bank2Arg::EccTear)) => {
                        Some(crate::bank2::run_ecc_tear(e, ui, cpu_hz))
                    }
                    #[cfg(feature = "bank2-gate")]
                    Ok(Request::Bank2(chimera_core::console::Bank2Arg::EraseSlots)) => {
                        Some(crate::bank2::run_erase_slots(e, ui, cpu_hz))
                    }
                    #[cfg(feature = "bank2-gate")]
                    Ok(Request::Bank2(chimera_core::console::Bank2Arg::EccRead(at))) => {
                        Some(crate::bank2::run_ecc_read(e, ui, at, cpu_hz))
                    }
                    #[cfg(feature = "bank2-gate")]
                    Ok(Request::Bank2(chimera_core::console::Bank2Arg::Pull(p))) => {
                        Some(crate::bank2::run_pull(p))
                    }
                    _ => None,
                };
                (card, sweep_ask, bank2)
            });
            let unit = &mut ChipUnit {
                ui: &*ui,
                stats,
                card,
                timer,
                irq,
                wall,
                #[cfg(feature = "midi-usb")]
                usb_irq,
                cycles_per_us: *cycles_per_us,
                bench,
                frame: display.frame(),
                dfu: false,
                boot: *boot,
                sweep: *sweep,
                board: *board,
                sweep_ask,
                files: store,
                forget: false,
                bank2,
                hands,
                ed: ed.as_deref_mut(),
            };
            let out = &mut UsbOut {
                link,
                clock: AnswerClock::start(crate::controls::now_ms()),
                rx: chimera_core::console::Rx::new(),
            };
            // A stalled answer just stops: the host tool resyncs on its next request.
            let sent = answer(req, unit, out).and_then(|()| out.flush());
            self.last = Served::Answered;
            let dfu = unit.dfu;
            if unit.forget {
                ui.forget_library();
            }
            let restart = match restart_after(dfu, sweep_ask, bank2.as_ref(), sent.is_ok())? {
                RestartKind::Dfu => Restart::Dfu(DfuAsked(())),
                RestartKind::Sweep => Restart::Sweep(SweepAsked(())),
                RestartKind::Bank2 => Restart::Bank2(Bank2Opened(())),
            };
            // `flush` empties the class's buffer, not the endpoint: polls
            // until the host has had time to collect the last IN packet.
            let start = crate::controls::now_ms();
            while crate::controls::now_ms().since(start) < DRAIN_MS {
                link.with(UsbCore::poll_all);
            }
            return Some(restart);
        }
        None
    }

    /// The console's hand at `now`: what `key` and `turn` hold and turn.
    pub fn hand(&mut self, now: Ms) -> Hand {
        self.hands.step(now)
    }

    /// `ed set`'s queue, applied to `ui` as knob turns.
    pub fn apply_ed(&mut self, ui: &mut UiState) {
        if let Some(ed) = self.ed.as_deref_mut() {
            chimera_core::bank2::ed_apply(ed, ui);
        }
    }

    /// `bank2 install` drew over the screen since the last call.
    pub fn take_redraw(&mut self) -> bool {
        core::mem::take(&mut self.redraw)
    }
}

/// What the console reads, borrowed for one answer.
struct ChipUnit<'a> {
    ui: &'a UiState,
    stats: Option<&'a mut Reader<AudioStats>>,
    card: Option<CardAnswer>,
    timer: &'a mut LoopTimer,
    irq: &'a mut crate::card_dma::IrqMark,
    wall: &'a mut WallClock,
    #[cfg(feature = "midi-usb")]
    usb_irq: &'a mut UsbIrq,
    cycles_per_us: u32,
    bench: Option<&'a str>,
    frame: Frame<'a>,
    /// `dfu` asked: the restart waits for the `OK` to be out.
    dfu: bool,
    boot: BootSeen,
    sweep: Option<SweepRun>,
    board: BoardId,
    /// `sweep`'s request, before the answer.
    sweep_ask: Option<SweepAsk>,
    /// `ls` and `put`'s card: unread without `card-put`, and in bench
    /// builds, which measure and take no files (their FLASH is full).
    #[cfg_attr(
        not(all(feature = "card-put", not(feature = "bench"))),
        allow(dead_code)
    )]
    files: &'a mut SdStore,
    /// A Sound `put` or `rm` reached the card: the library's page goes
    /// once the answer is out.
    forget: bool,
    /// `bank2 install`'s outcome, before the answer.
    bank2: Option<Bank2Answer>,
    hands: &'a mut Hands,
    ed: Option<&'a mut chimera_core::console::EdState>,
}

impl Unit for ChipUnit<'_> {
    fn ui(&self) -> &UiState {
        self.ui
    }

    /// `None` without `perf-probe`: no reader.
    fn stats(&mut self) -> Option<Stats> {
        let audio = crate::audio_stats(self.stats.as_deref_mut())?;
        crate::probe::restart_peaks();
        let (loop_avg_us, loop_peak_us) = self.timer.take();
        let wall = self.wall.fold(
            crate::shared::dwt(),
            crate::controls::now_ms(),
            Cycles::new(self.cycles_per_us * 1_000),
        );
        let streams = chimera_core::stream::STREAMS.then(|| crate::card_dma::tally(self.irq, wall));
        Some(Stats {
            audio,
            loop_avg_us,
            loop_peak_us,
            streams,
            #[cfg(feature = "midi-usb")]
            usb: Some(self.usb_irq.tally(wall)),
            #[cfg(not(feature = "midi-usb"))]
            usb: None,
            #[cfg(feature = "bank2")]
            panel: crate::bank2::board_door(|d| d.panel_stats),
            #[cfg(not(feature = "bank2"))]
            panel: None,
        })
    }

    fn bench(&self) -> Option<&str> {
        self.bench
    }

    /// `None` outright in the builds without the card bench (`lib.rs`), so
    /// their answer links none of `card`'s text.
    fn card(&mut self) -> Option<CardAnswer> {
        if !crate::CARD_BENCH {
            return None;
        }
        self.card
    }

    fn frame(&self) -> Frame<'_> {
        self.frame
    }

    fn dfu(&mut self) -> Option<()> {
        self.dfu = true;
        Some(())
    }

    #[cfg(feature = "fault-inject")]
    fn fault_dump(&mut self) -> Option<[chimera_core::console::StreamSnap; 3]> {
        Some(crate::fault::dump())
    }

    #[cfg(feature = "fault-inject")]
    fn fault(&mut self, arg: chimera_core::console::FaultArg) -> Option<()> {
        match arg {
            chimera_core::console::FaultArg::Dma(n) => crate::fault::dma_transfer_error(n),
            chimera_core::console::FaultArg::Ndtr => None,
            chimera_core::console::FaultArg::Card => {
                crate::card_dma::fail_next(2);
                Some(())
            }
        }
    }

    fn boot(&self) -> Option<BootSeen> {
        Some(self.boot)
    }

    fn sweep(&self) -> Option<SweepRun> {
        self.sweep
    }

    fn board(&self) -> Option<BoardId> {
        Some(self.board)
    }

    fn request_sweep(&mut self) -> Option<SweepAsk> {
        self.sweep_ask
    }

    #[cfg(feature = "midi-din")]
    fn midi(&self) -> Option<chimera_core::console::MidiStats> {
        use crate::shared::{MIDI_CTRL_DROPS, MIDI_DROPS, MIDI_FAULTS};
        use core::sync::atomic::Ordering;
        Some(chimera_core::console::MidiStats {
            faults: MIDI_FAULTS.load(Ordering::Relaxed),
            drops: MIDI_DROPS.load(Ordering::Relaxed),
            ctrl_drops: MIDI_CTRL_DROPS.load(Ordering::Relaxed),
            #[cfg(feature = "midi-usb")]
            usb_drops: self.usb_irq.seen.pipe_drops,
            #[cfg(not(feature = "midi-usb"))]
            usb_drops: 0,
        })
    }

    fn usb_regs(&self) -> Option<UsbRegs> {
        Some(regs())
    }

    fn flash_regs(&self) -> Option<FlashRegs> {
        Some(flash_regs())
    }

    /// `None` while region 2 is `Closed`: the load would fault.
    fn flash_peek(&self) -> Option<FlashPeek> {
        crate::bank2::mpu().readable().then(flash_peek)
    }

    fn forget_library(&mut self) {
        self.forget = true;
    }

    fn ed(&mut self) -> (&UiState, Option<&mut chimera_core::console::EdState>) {
        (self.ui, self.ed.as_deref_mut())
    }

    fn hands(&mut self) -> Option<&mut Hands> {
        Some(&mut *self.hands)
    }

    #[cfg(feature = "bank2")]
    fn bank2_seal(&self) -> Option<chimera_core::bank2::Seal> {
        crate::bank2_seal()
    }

    fn bank2_install(&mut self) -> Option<Bank2Answer> {
        self.bank2
    }

    #[cfg(all(feature = "card-put", not(feature = "bench")))]
    type Files = SdStore;
    #[cfg(not(all(feature = "card-put", not(feature = "bench"))))]
    type Files = chimera_core::console::NoCard;

    #[cfg(all(feature = "card-put", not(feature = "bench")))]
    fn files(&mut self) -> Option<&mut SdStore> {
        Some(&mut *self.files)
    }

    /// `ls` and `put` are not in this build: no `card-put`, or a bench
    /// build.
    #[cfg(not(all(feature = "card-put", not(feature = "bench"))))]
    fn files(&mut self) -> Option<&mut Self::Files> {
        None
    }
}

/// The port's bytes into `line`, up to `READ_BUDGET` (a flood without a
/// newline can't hold the UI): `Some` once one ends a request.
fn read(serial: &mut Serial, line: &mut Console) -> Option<Result<Request, Refusal>> {
    let mut byte = [0u8; 1];
    for _ in 0..READ_BUDGET {
        if !matches!(serial.read(&mut byte), Ok(1)) {
            return None;
        }
        if let Some(req) = line.push(byte[0]) {
            return Some(req);
        }
    }
    None
}

/// The port as `Out`, for one answer: it polls the device while the port
/// is full, until its `AnswerClock` expires.
struct UsbOut<'a> {
    link: &'a mut Link,
    clock: AnswerClock,
    rx: chimera_core::console::Rx,
}

impl UsbOut<'_> {
    /// One poll, unless the answer is out of time.
    fn pump(&mut self) -> Result<(), Stalled> {
        if self.clock.expired(crate::controls::now_ms()) {
            return Err(Stalled);
        }
        self.link.with(UsbCore::poll_all);
        Ok(())
    }

    /// The port's buffer out to the host, after the whole answer.
    fn flush(&mut self) -> Result<(), Stalled> {
        loop {
            match self.link.with(|c| c.serial.flush()) {
                Ok(()) => return Ok(()),
                Err(UsbError::WouldBlock) => self.pump()?,
                Err(_) => return Err(Stalled),
            }
        }
    }
}

impl Out for UsbOut<'_> {
    fn put(&mut self, mut bytes: &[u8]) -> Result<(), Stalled> {
        while !bytes.is_empty() {
            match self.link.with(|c| c.serial.write(bytes)) {
                Ok(n) if n > 0 => {
                    bytes = bytes.get(n..).unwrap_or(&[]);
                    self.clock.progress(crate::controls::now_ms());
                }
                Ok(_) | Err(UsbError::WouldBlock) => self.pump()?,
                // Unconfigured or gone: nothing will take the rest.
                Err(_) => return Err(Stalled),
            }
        }
        Ok(())
    }

    fn fresh(&mut self) {
        self.clock = AnswerClock::start(crate::controls::now_ms());
    }

    /// Polls until a byte comes or `wait_ms` pass: `put`'s data, read in
    /// the UI loop while audio and MIDI run above it, a poll and a packet
    /// a hold, so a parked RX head waits one hold a packet, not one a byte.
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let start = crate::controls::now_ms();
        loop {
            if let Some(b) = self.rx.pop() {
                return Some(b);
            }
            let rx = &mut self.rx;
            if self.link.with(|c| {
                c.poll_all();
                rx.fill(|buf| c.serial.read(buf).ok())
            }) {
                continue;
            }
            if crate::controls::now_ms().since(start) >= wait_ms {
                return None;
            }
        }
    }

    fn now(&self) -> Ms {
        crate::controls::now_ms()
    }
}

/// The device and its classes.
pub(crate) struct UsbCore {
    pub(crate) dev: Dev,
    pub(crate) serial: Serial,
    /// USB MIDI, on or off this boot.
    #[cfg(feature = "midi-usb")]
    pub(crate) midi: crate::usb_midi::Midi,
    /// The handler's own counters, read by the UI under `with`.
    #[cfg(feature = "midi-usb")]
    pub(crate) irq: Irq,
}

/// The handler's storm guard and counters, kept in the cell beside the
/// device: the handler writes them, the UI under `with`.
#[cfg(feature = "midi-usb")]
pub(crate) struct Irq {
    /// Its entries this millisecond: the storm guard's.
    window: StormWindow,
    /// SysTick's tick as the handler last saw it, and the DWT cycle it
    /// saw it change.
    tick: u32,
    tick_at: Cycles,
    /// The UI's last sign of life (`with`); `None` before the first, and
    /// on an errand.
    alive: Option<Cycles>,
    /// DWT cycles in a millisecond.
    per_ms: Cycles,
    /// The guard masked the handler: `with` leaves it masked until the top
    /// of the next `Usb::service` clears it.
    held: bool,
    /// RXFLVLM cleared: the RX FIFO's head waits for the UI; every `with`
    /// sets it again.
    park: RxPark,
    counts: Counts,
}

/// The handler's counters, what `stats` reads.
#[cfg(feature = "midi-usb")]
#[derive(Clone, Copy)]
pub(crate) struct Counts {
    /// Its own cycles less the audio's inside it, wrapping.
    cycles: u32,
    /// Its longest entry, in µs.
    us_max: u32,
    /// The guard's trips, by test.
    storms: StormKinds,
    /// RXFLVL parks.
    rx_parks: u32,
    /// USB MIDI event packets read and dropped.
    pub(crate) in_drops: u32,
    /// Echo messages USB's out ring lost and IN packets the endpoint
    /// refused.
    pub(crate) out_drops: u32,
    /// Events the note queue refused USB's pipe (`PipeStats::refused`),
    /// as of the last poll that fed it.
    pub(crate) pipe_drops: u32,
}

impl UsbCore {
    /// The one poll: the console's.
    #[cfg(not(feature = "midi-usb"))]
    #[inline(always)]
    fn poll_all(&mut self) {
        self.dev.poll(&mut [&mut self.serial]);
    }

    /// The one poll. MIDI on and the door open: bank 2 polls console and
    /// MIDI. Else the console here, and MIDI's OUT drained if it is on
    /// (`Closed`).
    #[cfg(feature = "midi-usb")]
    #[inline(never)]
    fn poll_all(&mut self) {
        let UsbCore {
            dev,
            serial,
            midi,
            irq,
        } = self;
        let counts = &mut irq.counts;
        if let crate::usb_midi::Polled::Console(closed) = midi.poll(dev, serial, counts) {
            dev.poll(&mut [serial]);
            if let Some(c) = closed {
                c.drain(dev, &mut counts.in_drops);
            }
        }
    }
}

/// The handler's counters as `service` last read them, its cycles folded
/// into 64 bits there, far inside their 8.9 s wrap, and the total and wall
/// cycles `stats` last read.
#[cfg(feature = "midi-usb")]
struct UsbIrq {
    seen: Counts,
    cycles: chimera_core::stream::stats::IrqCycles,
    read: u64,
    at: u64,
}

#[cfg(feature = "midi-usb")]
impl UsbIrq {
    fn new(seen: Counts) -> UsbIrq {
        UsbIrq {
            seen,
            cycles: chimera_core::stream::stats::IrqCycles::new(seen.cycles),
            read: 0,
            at: 0,
        }
    }

    fn fold(&mut self, seen: Counts) {
        self.seen = seen;
        self.cycles.fold(seen.cycles);
    }

    /// `stats`' USB lines at `wall` (`WallClock`'s cycles): the share
    /// since the last read.
    fn tally(&mut self, wall: u64) -> chimera_core::console::UsbTally {
        let total = self.cycles.total();
        let own = total.saturating_sub(core::mem::replace(&mut self.read, total));
        let wall = wall.saturating_sub(core::mem::replace(&mut self.at, wall));
        chimera_core::console::UsbTally {
            irq_share: chimera_core::console::PerMille::of(own, wall),
            irq_us_max: chimera_hal::units::Micros::new(self.seen.us_max),
            storms: self.seen.storms,
            rx_parks: self.seen.rx_parks,
            midi_in_drops: self.seen.in_drops,
            midi_out_drops: self.seen.out_drops,
        }
    }
}

/// The UI's hold on the device: the proof `connect` filled the cell,
/// reached through `with`. One per `Usb`, borrowed `&mut` for each
/// closure, so a `with` can't nest.
struct Link(());

impl Link {
    #[inline(always)]
    fn with<R>(&mut self, f: impl FnOnce(&mut UsbCore) -> R) -> R {
        with(f)
    }

    /// `f`, the UI off on an errand that can outlast `SERVICE_STALL_MS`
    /// with no hold (card work, SYSTEM's sync, an install): the guard's
    /// service test sleeps through it, its tick test still holds. Back, the
    /// UI is alive from then. The one way long UI work goes: a scope, so
    /// the guard can't be left asleep or forgotten awake.
    #[inline(always)]
    fn errand<R>(&mut self, f: impl FnOnce(&Errand) -> R) -> R {
        #[cfg(feature = "midi-usb")]
        self.with(|c| c.irq.alive = None);
        // SAFETY: the service test sleeps until the hold below gives the
        // stamp back.
        let r = unsafe { Errand::run(f) };
        #[cfg(feature = "midi-usb")]
        self.with(|_| ());
        r
    }
}

/// The device and its classes, in every build: with `midi-usb` the
/// `OTG_FS` handler's too.
struct Cell(UnsafeCell<MaybeUninit<UsbCore>>);
// SAFETY: written once by `install` before `OTG_FS` is unmasked; after that
// touched only by the handler and by `with`, which masks it first (the
// thread can't preempt the handler). Without `midi-usb` no handler runs.
unsafe impl Sync for Cell {}
static CORE: Cell = Cell(UnsafeCell::new(MaybeUninit::uninit()));

/// The core into the cell, then, with `midi-usb`, `OTG_FS` unmasked at
/// MIDI's level.
fn install(core: UsbCore) -> Link {
    // SAFETY: the one write: only `connect` calls this, on the one
    // `Unconnected` `init`'s singletons allow, and the handler, the cell's
    // other user, is still masked.
    unsafe { (*CORE.0.get()).write(core) };
    #[cfg(feature = "midi-usb")]
    {
        // The write must not sink past the unmask's volatile write.
        core::sync::atomic::compiler_fence(Ordering::SeqCst);
        // SAFETY: only the NVIC is used, for this interrupt's priority and
        // unmask; every other priority is set at boot, and nothing else
        // writes one after.
        let mut nvic = unsafe { cortex_m::Peripherals::steal() }.NVIC;
        crate::priority::set_irq(
            &mut nvic,
            pac::Interrupt::OTG_FS,
            crate::priority::Priority::MIDI,
        );
        // SAFETY: the handler touches only the cell, whose user here masks
        // it first, and the counters it alone adds to.
        unsafe { NVIC::unmask(pac::Interrupt::OTG_FS) };
    }
    Link(())
}

/// The UI's way in: `OTG_FS` masked for `f`. Never nested: only `Link`
/// calls it, and only `install` makes one, after the cell is written.
fn with<R>(f: impl FnOnce(&mut UsbCore) -> R) -> R {
    lock();
    // SAFETY: the cell is written (a `Link` exists), and the handler, its
    // one other user, is masked and not running (or, without `midi-usb`,
    // doesn't exist).
    let core = unsafe { (*CORE.0.get()).assume_init_mut() };
    // Any UI hold is the UI alive: a long answer pumps inside one `service`.
    // Before `f`, so an errand's `None` stands.
    #[cfg(feature = "midi-usb")]
    {
        core.irq.alive = Some(crate::shared::dwt());
    }
    let r = f(core);
    #[cfg(feature = "midi-usb")]
    unlock(&mut core.irq);
    r
}

/// `with`'s mask, out of line: one call at each of its sites. Nothing to
/// mask without `midi-usb`.
#[cfg_attr(feature = "midi-usb", inline(never))]
#[cfg_attr(not(feature = "midi-usb"), inline(always))]
fn lock() {
    #[cfg(feature = "midi-usb")]
    {
        NVIC::mask(pac::Interrupt::OTG_FS);
        // The mask lands before the cell is touched.
        cortex_m::asm::dsb();
        cortex_m::asm::isb();
    }
}

/// `with`'s unmask, unless a storm holds the handler off. A parked head
/// is let back first: the hold may have drained its class, and a console
/// still full parks it again in one entry, so RXFLVL never stays masked
/// past a hold.
#[cfg(feature = "midi-usb")]
#[inline(never)]
fn unlock(irq: &mut Irq) {
    if let Some(p) = irq.park.unlock() {
        rx_park(p);
    }
    if !irq.held {
        // SAFETY: as `install`'s unmask.
        unsafe { NVIC::unmask(pac::Interrupt::OTG_FS) };
    }
}

/// Polls the device; a RX FIFO head the poll left (a full console's
/// packet, which keeps RXFLVL up) parks RXFLVL until the UI's next hold,
/// and a head gone (read, or flushed by a bus reset) lets it back. Over
/// `STORM_LIMIT` entries in a millisecond, or while it starves SysTick
/// (the watchdog's kick) or the UI's `service`, it masks itself until the
/// UI's next `service`, whose polls carry the console meanwhile.
#[cfg(feature = "midi-usb")]
#[interrupt]
fn OTG_FS() {
    use crate::shared::{MIDI_CYCLES, audio_cycles, own_cycles, stamp};
    // Its own time, for `stats` and the card IRQ's.
    let enter = stamp(audio_cycles);
    // SAFETY: `install` wrote the cell before unmasking this handler, and
    // `with` masks it while the UI holds the cell: this reference is the
    // only one. The handler can't preempt itself.
    let core = unsafe { (*CORE.0.get()).assume_init_mut() };
    let n = &mut core.irq;
    let now = enter.at;
    let tick = crate::controls::now_ms().0;
    if tick != n.tick {
        (n.tick, n.tick_at) = (tick, now);
    }
    let kind = trip(&mut n.window, now, n.tick_at, n.alive, n.per_ms);
    if let Some(kind) = kind {
        n.held = true;
        n.counts.storms.add(kind);
        NVIC::mask(pac::Interrupt::OTG_FS);
    } else {
        let before = rx_head(true);
        core.poll_all();
        let n = &mut core.irq;
        if let Some(p) = n.park.polled(before, rx_head(false)) {
            n.counts.rx_parks = n.counts.rx_parks.wrapping_add(u32::from(p));
            rx_park(p);
        }
    }
    let own = own_cycles(enter, stamp(audio_cycles)).get();
    // The card IRQ's own time leaves out the MIDI level's (`card_dma::clocks`).
    MIDI_CYCLES.fetch_add(own, Ordering::Relaxed);
    let n = &mut core.irq;
    let us = own / (n.per_ms.get() / 1_000).max(1);
    n.counts.cycles = n.counts.cycles.wrapping_add(own);
    n.counts.us_max = n.counts.us_max.max(us);
}

/// OTG2's global block, for the RXFLVL park.
#[cfg(feature = "midi-usb")]
#[inline(always)]
fn otg() -> &'static pac::otg1_hs_global::RegisterBlock {
    // SAFETY: OTG2's fixed global block. Its reads here have no side
    // effect (GRXSTSR peeks; GRXSTSP would pop), and its one write is
    // `rx_park`'s.
    unsafe { &*pac::OTG2_HS_GLOBAL::ptr() }
}

/// The RX FIFO's head, with RXFLVL up. `polled`: only if the crate's poll
/// will read it, which ENUMDNE, WKUINT and USBSUSP each take first.
#[cfg(feature = "midi-usb")]
#[inline(always)]
fn rx_head(polled: bool) -> Option<RxHead> {
    let g = otg();
    let s = g.gintsts.read();
    let first = polled && (s.enumdne().bit() || s.wkuint().bit() || s.usbsusp().bit());
    (s.rxflvl().bit() && !first).then(|| RxHead(g.grxstsr_device().read().bits()))
}

/// GINTMSK's RXFLVLM cleared (`park`) or set. A read-modify-write that
/// can't race: synopsys-usb-otg writes GINTMSK only in `enable`, inside
/// `connect`'s `build`, before `install` unmasks the handler; after that
/// only the handler and `unlock` write it, and `with` masks the one while
/// the other runs. USBRST stays unmasked, so a bus reset still enters the
/// handler, whose poll flushes the FIFO and lets RXFLVL back.
#[cfg(feature = "midi-usb")]
#[inline(always)]
fn rx_park(park: bool) {
    otg().gintmsk.modify(|_, w| w.rxflvlm().bit(!park));
}

/// DSTS.FNSOF (bits 8..22): moving while the host sends start-of-frame.
fn frame_number() -> u32 {
    // SAFETY: a plain read of OTG2's DSTS, which has no read side effect.
    let d = unsafe { &*pac::OTG2_HS_DEVICE::ptr() };
    d.dsts.read().bits() & 0x3F_FF00
}

/// The OTG and PWR registers a cold and a warm bring-up are compared on.
pub fn regs() -> UsbRegs {
    // SAFETY: plain reads of OTG2's and PWR's fixed registers; none of
    // these has a read side effect (GINTSTS clears on a write of 1).
    let (g, d, pwr) = unsafe {
        (
            &*pac::OTG2_HS_GLOBAL::ptr(),
            &*pac::OTG2_HS_DEVICE::ptr(),
            &*pac::PWR::ptr(),
        )
    };
    UsbRegs {
        gotgctl: g.gotgctl.read().bits(),
        gccfg: g.gccfg.read().bits(),
        dctl: d.dctl.read().bits(),
        gintsts: g.gintsts.read().bits(),
        dsts: d.dsts.read().bits(),
        pwr_cr3: pwr.cr3.read().bits(),
    }
}

/// The flash controller's registers, read only: nothing here writes,
/// unlocks or touches the option bytes.
fn flash_regs() -> FlashRegs {
    // SAFETY: plain reads of FLASH's fixed registers; none has a read side
    // effect (the SR flags clear only through CCR writes).
    let f = unsafe { &*pac::FLASH::ptr() };
    let bank = |b: &pac::flash::BANK| BankRegs {
        cr: b.cr.read().bits(),
        sr: b.sr.read().bits(),
        prar_cur: b.prar_cur.read().bits(),
        scar_cur: b.scar_cur.read().bits(),
        wpsn_cur: b.wpsn_curr.read().bits(),
    };
    FlashRegs {
        acr: f.acr.read().bits(),
        optcr: f.optcr().read().bits(),
        optsr_cur: f.optsr_cur().read().bits(),
        boot_cur: f.boot_curr().read().bits(),
        banks: [bank(f.bank1()), bank(f.bank2())],
    }
}

/// `PEEK_AT`'s first 32 bytes, then SR1 and SR2: an ECC error the reads
/// raised shows there.
fn flash_peek() -> FlashPeek {
    let mut words = [0u32; 8];
    for (k, w) in (0u32..).zip(words.iter_mut()) {
        // SAFETY: an aligned word of bank-2 sector 0, which MPU region 2
        // maps readable (the caller checked `bank2::mpu()`); a load never
        // programs or erases it.
        *w = unsafe { core::ptr::read_volatile((PEEK_AT + 4 * k) as *const u32) };
    }
    let r = flash_regs();
    FlashPeek {
        words,
        sr_after: r.banks.map(|b| b.sr),
    }
}

/// The furthest step the host has taken the device to, from its state
/// and whether start-of-frame packets arrive (DSTS.FNSOF moving).
fn bus_step(state: UsbDeviceState, sof_moved: bool, was: UsbStep) -> UsbStep {
    match state {
        UsbDeviceState::Configured => UsbStep::Configured,
        UsbDeviceState::Addressed => UsbStep::Addressed,
        UsbDeviceState::Suspend => UsbStep::Suspended,
        UsbDeviceState::Default if sof_moved => UsbStep::Sof,
        UsbDeviceState::Default => was,
    }
}

/// What USB MIDI feeds and empties, kept for the try that connects; none
/// without USB MIDI.
#[cfg(feature = "midi-usb")]
pub type MidiSinks = crate::usb_midi::Sinks;
#[cfg(not(feature = "midi-usb"))]
pub type MidiSinks = ();

/// The port through the boot: waiting for a try that passes `preflight`,
/// up, or given up after `USB_TRIES`. One lives on `synth`'s frame for
/// the whole run, and there's no heap to box `Usb` into.
#[allow(clippy::large_enum_variant)]
pub enum PortState {
    Waiting(UsbParts, UsbRetry, MidiSinks),
    Up {
        usb: Usb,
        tries: u8,
        step: UsbStep,
        sof: u32,
    },
    Off,
}

impl PortState {
    pub fn new(parts: UsbParts, midi: MidiSinks) -> PortState {
        PortState::Waiting(parts, UsbRetry::new(), midi)
    }

    /// A try if one is due (`UsbRetry`): `Some` with how it ended. Each
    /// try's waits are bounded, interrupts on, so the synth plays on.
    /// `step` hears each `UsbStep` as the try reaches it.
    pub fn try_up(
        &mut self,
        now: Ms,
        clocks: &CoreClocks,
        cpu_hz: u32,
        step: &mut dyn FnMut(UsbStep, u8),
    ) -> Option<UsbState> {
        let PortState::Waiting(_, retry, _) = self else {
            return None;
        };
        if !retry.due(now) {
            return None;
        }
        let PortState::Waiting(parts, mut retry, midi) = core::mem::replace(self, PortState::Off)
        else {
            return None;
        };
        let tries = retry.tried(now);
        step(UsbStep::Preflight, tries);
        let state = match preflight(parts, cpu_hz) {
            Ok(ready) => {
                let unconnected = init(ready, midi, clocks, cpu_hz);
                step(UsbStep::Init, tries);
                step(UsbStep::Connect, tries);
                let usb = unconnected.connect();
                step(UsbStep::Up, tries);
                *self = PortState::Up {
                    usb,
                    tries,
                    step: UsbStep::Up,
                    sof: frame_number(),
                };
                UsbState::On { tries }
            }
            Err((parts, why)) => {
                let state = UsbState::Off { why, tries };
                if !state.gave_up() {
                    *self = PortState::Waiting(parts, retry, midi);
                }
                state
            }
        };
        Some(state)
    }

    /// The host's progress since the last look, if it moved: the step and
    /// the try that brought the port up.
    pub fn host_step(&mut self) -> Option<(UsbStep, u8)> {
        let PortState::Up {
            usb,
            tries,
            step,
            sof,
        } = self
        else {
            return None;
        };
        let frame = frame_number();
        let next = bus_step(usb.link.with(|c| c.dev.state()), frame != *sof, *step);
        *sof = frame;
        (next != *step).then(|| {
            *step = next;
            (next, *tries)
        })
    }

    /// `f` as an errand (`Link::errand`) while the port is up; else just
    /// `f`, with the proof. Long UI work needs the `&Errand` this hands
    /// out, so it can't run outside one.
    pub fn errand<R>(&mut self, f: impl FnOnce(&Errand) -> R) -> R {
        match self.up() {
            Some(u) => u.link.errand(f),
            // SAFETY: no port up: `OTG_FS` masked, no guard runs.
            None => unsafe { Errand::run(f) },
        }
    }

    pub fn up(&mut self) -> Option<&mut Usb> {
        match self {
            PortState::Up { usb, .. } => Some(usb),
            _ => None,
        }
    }
}
