//! The USB console's shell (ADR 0068): CDC-ACM on the OTG FS port (PA11/PA12),
//! polled from the UI loop. `pac::Interrupt::OTG_FS` is never unmasked, so
//! USB can never preempt audio.

use chimera_core::boot::{BootSeen, UsbOff, UsbRegs, UsbRetry, UsbState, UsbStep, wait_until};
use chimera_core::console::{
    AnswerClock, Bank2Answer, BankRegs, CardAnswer, CardArg, Console, FlashPeek, FlashRegs, Frame,
    LoopTimer, Out, PEEK_AT, Request, Served, Stalled, Stats, SweepAsk, Unit, answer, serial_hex,
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
use usb_device::device::{
    StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbDeviceState, UsbVidPid,
};
use usbd_serial::{SerialPort, USB_CLASS_CDC};

use crate::sd::SdStore;

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

type Dev = UsbDevice<'static, UsbBus<Otg2>>;
type Port = SerialPort<'static, UsbBus<Otg2>>;

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
}

/// `dfu` answered and its `OK` flushed: only `Usb::service` makes one, and
/// only `dfu::enter` takes one.
#[must_use]
pub struct DfuAsked(());

/// `sweep` answered, its mark set to `Asked` and its `OK` flushed: the
/// boot it restarts into sweeps (ADR 0094). Only `Usb::service` makes one.
#[must_use]
pub struct SweepAsked(());

/// A console answer that ends in a restart.
pub enum Restart {
    Dfu(DfuAsked),
    Sweep(SweepAsked),
}

/// `card`'s measurement, run before its answer starts: `None` without one
/// in this build.
pub type CardRun<'a> =
    &'a mut dyn FnMut(CardArg, Option<&mut Reader<AudioStats>>, &mut SdStore) -> Option<CardAnswer>;

/// The port, the half-read request line, and the loop's time between tops.
pub struct Usb {
    dev: Dev,
    serial: Port,
    line: Console,
    timer: LoopTimer,
    /// DWT at the last loop top, and whether that iteration answered.
    top: u32,
    last: Served,
    cycles_per_us: u32,
    boot: BootSeen,
    sweep: Option<SweepRun>,
    /// `bank2 install` drew over the screen: the loop redraws it whole.
    redraw: bool,
}

/// The port set up but not on the bus: no pull-up, so the host sees
/// nothing until `connect`. Only `init` makes one.
pub struct Unconnected {
    bus: &'static UsbBusAllocator<UsbBus<Otg2>>,
    serial: Port,
    uid: &'static str,
    cpu_hz: u32,
    boot: BootSeen,
    sweep: Option<SweepRun>,
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
pub fn init(ready: Ready, clocks: &CoreClocks, cpu_hz: u32) -> Unconnected {
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
    // EP OUT packets only (EP0 and the bulk OUT, 64 bytes each); the TX
    // FIFOs live in the core's own RAM.
    let ep_memory = cortex_m::singleton!(: [u32; 256] = [0; 256]).expect("EP memory taken once");
    let bus = UsbBus::new(usb, ep_memory);
    // Here, so the device and the class can both borrow it for 'static.
    let bus: &'static UsbBusAllocator<UsbBus<Otg2>> =
        cortex_m::singleton!(: UsbBusAllocator<UsbBus<Otg2>> = bus).expect("USB bus taken once");

    let serial = SerialPort::new(bus);
    // The chip's 96-bit UID: a stable /dev/serial/by-id name per unit.
    let uid = cortex_m::singleton!(: chimera_core::console::SerialNumber = serial_hex(Uid::read()))
        .expect("serial number taken once");
    Unconnected {
        bus,
        serial,
        uid: uid.as_str(),
        cpu_hz,
        boot,
        sweep,
    }
}

impl Unconnected {
    /// Builds the device, which enables the core and its D+ pull-up: the
    /// host starts enumerating now, so only the UI loop that polls it may
    /// call this, at its top (the spec: the port appears once the UI loop
    /// starts). A bench build's screens hold for minutes before that, and
    /// an unpolled device that long is one the host gives up on.
    pub fn connect(self) -> Usb {
        let strings = StringDescriptors::default()
            .manufacturer("Chimera")
            .product("Chimera console")
            .serial_number(self.uid);
        let dev = UsbDeviceBuilder::new(self.bus, UsbVidPid(VID_PID.0, VID_PID.1))
            .strings(&[strings])
            .expect("one language")
            .self_powered(true)
            .max_power(100)
            .expect("100 mA is within 500")
            .device_class(USB_CLASS_CDC)
            .build();
        Usb {
            dev,
            serial: self.serial,
            line: Console::new(),
            timer: LoopTimer::new(),
            top: DWT::cycle_count(),
            last: Served::Idle,
            cycles_per_us: (self.cpu_hz / 1_000_000).max(1),
            boot: self.boot,
            sweep: self.sweep,
            redraw: false,
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
        #[cfg(feature = "bank2")]
        let cpu_hz = self.cycles_per_us * 1_000_000;
        let Usb {
            dev,
            serial,
            line,
            timer,
            boot,
            sweep,
            #[cfg(feature = "bank2")]
            redraw,
            ..
        } = self;
        dev.poll(&mut [&mut *serial]);
        self.last = Served::Idle;
        // A stalled answer's tail still waiting: no new request until the
        // host reads it, so a host that writes but never reads NAKs itself
        // instead of costing a stall every lap.
        if let Err(UsbError::WouldBlock) = serial.flush() {
            return None;
        }
        // Read whatever poll said or not: bytes left from the last request
        // already wait in the port's buffer.
        let mut byte = [0u8; 1];
        for _ in 0..READ_BUDGET {
            if !matches!(serial.read(&mut byte), Ok(1)) {
                break;
            }
            let Some(req) = line.push(byte[0]) else {
                continue;
            };
            // Outside a bench build nothing measures, and none of it links.
            let card = match req {
                Ok(Request::Card(arg)) if cfg!(feature = "bench") => {
                    card(arg, stats.as_deref_mut(), store)
                }
                _ => None,
            };
            let sweep_ask = match req {
                Ok(Request::Sweep(_)) => Some(chimera_core::console::ask_sweep(
                    ui.project_status() == chimera_core::project::ProjectStatus::Modified,
                    store,
                )),
                _ => None,
            };
            // Bank 2's write, seconds at most, before the answer's clock
            // starts; the screen it drew is redrawn whole after.
            let bank2 = match req {
                #[cfg(feature = "bank2")]
                Ok(Request::Bank2(chimera_core::console::Bank2Arg::Install)) => {
                    *redraw = true;
                    Some(crate::bank2::run_install(store, ui, display, cpu_hz))
                }
                _ => None,
            };
            let unit = &mut ChipUnit {
                ui: &*ui,
                stats,
                card,
                timer,
                bench,
                frame: display.frame(),
                dfu: false,
                boot: *boot,
                sweep: *sweep,
                sweep_ask,
                files: store,
                forget: false,
                bank2,
            };
            let out = &mut UsbOut {
                dev,
                serial,
                clock: AnswerClock::start(crate::controls::now_ms()),
            };
            // A stalled answer just stops: the host tool resyncs on its next request.
            let sent = answer(req, unit, out).and_then(|()| out.flush());
            self.last = Served::Answered;
            let dfu = unit.dfu;
            if unit.forget {
                ui.forget_library();
            }
            let restart = match (dfu, sweep_ask) {
                (true, _) => Restart::Dfu(DfuAsked(())),
                (false, Some(SweepAsk::Requested)) => Restart::Sweep(SweepAsked(())),
                _ => return None,
            };
            if sent.is_err() {
                return None;
            }
            // `flush` empties the class's buffer, not the endpoint: polls
            // until the host has had time to collect the last IN packet.
            let start = crate::controls::now_ms();
            while crate::controls::now_ms().since(start) < DRAIN_MS {
                dev.poll(&mut [&mut *serial]);
            }
            return Some(restart);
        }
        None
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
    bench: Option<&'a str>,
    frame: Frame<'a>,
    /// `dfu` asked: the restart waits for the `OK` to be out.
    dfu: bool,
    boot: BootSeen,
    sweep: Option<SweepRun>,
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
}

impl Unit for ChipUnit<'_> {
    fn ui(&self) -> &UiState {
        self.ui
    }

    /// `None` without `perf-probe`: no reader.
    fn stats(&mut self) -> Option<Stats> {
        let audio = crate::audio_stats(self.stats.as_deref_mut())?;
        crate::probe::restart_voice_peak();
        let (loop_avg_us, loop_peak_us) = self.timer.take();
        Some(Stats {
            audio,
            loop_avg_us,
            loop_peak_us,
        })
    }

    fn bench(&self) -> Option<&str> {
        self.bench
    }

    fn card(&mut self) -> Option<CardAnswer> {
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
        }
    }

    fn boot(&self) -> Option<BootSeen> {
        Some(self.boot)
    }

    fn sweep(&self) -> Option<SweepRun> {
        self.sweep
    }

    fn request_sweep(&mut self) -> Option<SweepAsk> {
        self.sweep_ask
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

/// The port as `Out`, for one answer: it polls the device while the port
/// is full, until its `AnswerClock` expires.
struct UsbOut<'a> {
    dev: &'a mut Dev,
    serial: &'a mut Port,
    clock: AnswerClock,
}

impl UsbOut<'_> {
    /// One poll, unless the answer is out of time.
    fn pump(&mut self) -> Result<(), Stalled> {
        if self.clock.expired(crate::controls::now_ms()) {
            return Err(Stalled);
        }
        self.dev.poll(&mut [&mut *self.serial]);
        Ok(())
    }

    /// The port's buffer out to the host, after the whole answer.
    fn flush(&mut self) -> Result<(), Stalled> {
        loop {
            match self.serial.flush() {
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
            match self.serial.write(bytes) {
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
    /// the UI loop while audio and MIDI run above it.
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let start = crate::controls::now_ms();
        let mut byte = [0u8; 1];
        loop {
            if let Ok(1) = self.serial.read(&mut byte) {
                return Some(byte[0]);
            }
            if crate::controls::now_ms().since(start) >= wait_ms {
                return None;
            }
            self.dev.poll(&mut [&mut *self.serial]);
        }
    }

    fn now(&self) -> Ms {
        crate::controls::now_ms()
    }
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

/// The port through the boot: waiting for a try that passes `preflight`,
/// up, or given up after `USB_TRIES`. One lives on `synth`'s frame for
/// the whole run, and there's no heap to box `Usb` into.
#[allow(clippy::large_enum_variant)]
pub enum PortState {
    Waiting(UsbParts, UsbRetry),
    Up {
        usb: Usb,
        tries: u8,
        step: UsbStep,
        sof: u32,
    },
    Off,
}

impl PortState {
    pub fn new(parts: UsbParts) -> PortState {
        PortState::Waiting(parts, UsbRetry::new())
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
        let PortState::Waiting(_, retry) = self else {
            return None;
        };
        if !retry.due(now) {
            return None;
        }
        let PortState::Waiting(parts, mut retry) = core::mem::replace(self, PortState::Off) else {
            return None;
        };
        let tries = retry.tried(now);
        step(UsbStep::Preflight, tries);
        let state = match preflight(parts, cpu_hz) {
            Ok(ready) => {
                let unconnected = init(ready, clocks, cpu_hz);
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
                    *self = PortState::Waiting(parts, retry);
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
        let next = bus_step(usb.dev.state(), frame != *sof, *step);
        *sof = frame;
        (next != *step).then(|| {
            *step = next;
            (next, *tries)
        })
    }

    pub fn up(&mut self) -> Option<&mut Usb> {
        match self {
            PortState::Up { usb, .. } => Some(usb),
            _ => None,
        }
    }
}
