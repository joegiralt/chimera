//! The USB console's shell (ADR 0068): CDC-ACM on the OTG FS port (PA11/PA12),
//! polled from the UI loop. `pac::Interrupt::OTG_FS` is never unmasked, so
//! USB can never preempt audio.

use chimera_core::boot::{BootSeen, UsbOff, UsbRegs, UsbRetry, UsbState, UsbStep, wait_until};
use chimera_core::console::{
    AnswerClock, Console, Frame, LoopTimer, Out, Served, Stalled, Stats, Unit, answer, serial_hex,
};
use chimera_core::perf::load::AudioStats;
use chimera_core::triple::Reader;
use chimera_core::ui::UiState;
use chimera_hal::Ms;
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

/// OTG2, the OTG FS core on PA11/PA12, brought up in the stock PreenFM3
/// firmware's order (Ixox/preenfm3 `firmware/Src/usbd_conf.c`
/// `HAL_PCD_MspInit`): its AHB clock enabled and nothing else, no RCC
/// reset; `preflight` enables VDD33USB's detector first. From a cold
/// power-on the core reset still never completes (CSRST stays set), and
/// the console stays off until a pass through the ROM loader:
/// https://github.com/joegiralt/chimera/issues/331
pub struct Otg2 {
    _global: pac::OTG2_HS_GLOBAL,
    _device: pac::OTG2_HS_DEVICE,
    _pwrclk: pac::OTG2_HS_PWRCLK,
    hclk: u32,
}

// SAFETY: as the HAL's `USB2` (stm32h7xx-hal 0.16 usb_hs.rs): the register
// blocks are owned here, and only `UsbBus` touches them, through
// `REGISTERS`, under its own critical sections.
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
}

/// `dfu` answered and its `OK` flushed: only `Usb::service` makes one, and
/// only `dfu::enter` takes one.
#[must_use]
pub struct DfuAsked(());

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
}

/// The port set up but not on the bus: no pull-up, so the host sees
/// nothing until `connect`. Only `init` makes one.
pub struct Unconnected {
    bus: &'static UsbBusAllocator<UsbBus<Otg2>>,
    serial: Port,
    uid: &'static str,
    cpu_hz: u32,
    boot: BootSeen,
}

/// The port's preconditions held, each checked with a bounded wait: only
/// `preflight` makes one, and `init` takes it with the parts.
#[must_use]
pub struct Ready(UsbParts);

/// What `connect`'s enable (synopsys-usb-otg `UsbBus::enable`) spins on
/// with interrupts masked and no timeout, checked first with interrupts
/// on and a timeout: HSI48RDY, then the OTG core's AHBIDL and a core soft
/// reset (CSRST) on the FS PHY. `Err` names the first that never came
/// true: `connect` would spin on it forever with interrupts masked, so the
/// boot goes on without USB, and the shell toasts why.
pub fn preflight(parts: UsbParts, cpu_hz: u32, tries: u8) -> Result<Ready, (UsbParts, UsbOff)> {
    match check(&parts, cpu_hz, tries) {
        Ok(()) => Ok(Ready(parts)),
        Err(why) => Err((parts, why)),
    }
}

/// How long the OTG block's RCC reset is left to settle.
const RESET_SETTLE_MS: u32 = 2;

fn check(parts: &UsbParts, cpu_hz: u32, tries: u8) -> Result<(), UsbOff> {
    let limit = READY_WAIT_MS * (cpu_hz / 1000);
    let wait = |ready: &mut dyn FnMut() -> bool| wait_until(limit, DWT::cycle_count, ready);
    // SAFETY: RCC and PWR are owned by the HAL after `freeze`; this reads
    // HSI48RDY, sets PWR_CR3.USB33DEN (bit 24), the bit the HAL's own
    // `USB2::enable` sets the same way, reads USB33RDY (bit 26, RM0433
    // PWR_CR3; the PAC's `usb33rdy`), and sets AHB1ENR's USB2OTGEN and
    // pulses AHB1RSTR's USB2OTGRST, this port's own clock and reset bits.
    // Nothing else writes these after `freeze`, no interrupt included.
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
    // From try 2: the block's RCC reset and a moment to settle, in case a
    // CSRST that never cleared wedged the core. It doesn't free the cold
    // case (#331), but costs nothing.
    if tries >= 2 {
        rcc.ahb1rstr.modify(|_, w| w.usb2otgrst().set_bit());
        rcc.ahb1rstr.modify(|_, w| w.usb2otgrst().clear_bit());
        let _ = wait_until(RESET_SETTLE_MS * (cpu_hz / 1000), DWT::cycle_count, || {
            false
        });
    }
    let global = &parts.global;
    if !wait(&mut || global.grstctl.read().ahbidl().bit_is_set()) {
        return Err(UsbOff::AhbIdle);
    }
    // The FS PHY, as `UsbBus::enable` selects it before its reset.
    global.gusbcfg.modify(|_, w| w.physel().set_bit());
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
        }
    }
}

impl Usb {
    /// The UI loop's first line, its snapshot point: the last iteration has
    /// flushed, so `frame` is what the panel shows. Times the last lap (an
    /// answered one is not counted), polls, reads into the line, and
    /// answers at most one request: `Some` for a `dfu` the host was told
    /// `OK` to.
    pub fn service(
        &mut self,
        ui: &UiState,
        stats: Option<&mut Reader<AudioStats>>,
        bench: Option<&str>,
        frame: Frame<'_>,
    ) -> Option<DfuAsked> {
        let now = DWT::cycle_count();
        let us = now.wrapping_sub(self.top) / self.cycles_per_us;
        self.top = now;
        self.timer.lap(us, self.last);
        let Usb {
            dev,
            serial,
            line,
            timer,
            boot,
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
            let unit = &mut ChipUnit {
                ui,
                stats,
                timer,
                bench,
                frame,
                dfu: false,
                boot: *boot,
            };
            let out = &mut UsbOut {
                dev,
                serial,
                clock: AnswerClock::start(crate::controls::now_ms()),
            };
            // A stalled answer just stops: the host tool resyncs on its next request.
            let sent = answer(req, unit, out).and_then(|()| out.flush());
            self.last = Served::Answered;
            if !(unit.dfu && sent.is_ok()) {
                return None;
            }
            // `flush` empties the class's buffer, not the endpoint: polls
            // until the host has had time to collect the last IN packet.
            let start = crate::controls::now_ms();
            while crate::controls::now_ms().since(start) < DRAIN_MS {
                dev.poll(&mut [&mut *serial]);
            }
            return Some(DfuAsked(()));
        }
        None
    }
}

/// What the console reads, borrowed for one answer.
struct ChipUnit<'a> {
    ui: &'a UiState,
    stats: Option<&'a mut Reader<AudioStats>>,
    timer: &'a mut LoopTimer,
    bench: Option<&'a str>,
    frame: Frame<'a>,
    /// `dfu` asked: the restart waits for the `OK` to be out.
    dfu: bool,
    boot: BootSeen,
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

    fn frame(&self) -> Frame<'_> {
        self.frame
    }

    fn dfu(&mut self) -> Option<()> {
        self.dfu = true;
        Some(())
    }

    fn boot(&self) -> Option<BootSeen> {
        Some(self.boot)
    }

    fn usb_regs(&self) -> Option<UsbRegs> {
        Some(regs())
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
        let state = match preflight(parts, cpu_hz, tries) {
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
                    sof: regs().dsts,
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
        // DSTS.FNSOF, bits 8..22.
        let frame = regs().dsts & 0x3F_FF00;
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
