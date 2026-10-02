//! The USB console's shell (ADR 0068): CDC-ACM on the OTG FS port (PA11/PA12),
//! polled from the UI loop. `pac::Interrupt::OTG_FS` is never unmasked, so
//! USB can never preempt audio.

use chimera_core::console::{
    AnswerClock, Console, Frame, LoopTimer, Out, Served, Stalled, Stats, Unit, answer, serial_hex,
};
use chimera_core::perf::load::AudioStats;
use chimera_core::triple::Reader;
use chimera_core::ui::UiState;
use cortex_m::peripheral::DWT;
use stm32h7xx_hal::gpio::{Alternate, PA11, PA12};
use stm32h7xx_hal::pac;
use stm32h7xx_hal::rcc::rec::{self, UsbClkSel, UsbClkSelGetter};
use stm32h7xx_hal::rcc::{Ccdr, CoreClocks, ResetEnable};
use stm32h7xx_hal::signature::Uid;
use stm32h7xx_hal::usb_hs::{USB2, UsbBus};
use usb_device::UsbError;
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbVidPid};
use usbd_serial::{SerialPort, USB_CLASS_CDC};

/// The stock PreenFM3's ID: Ixox/preenfm3 firmware/Src/usbd_desc.c.
pub const VID_PID: (u16, u16) = (0x0483, 0x5740);
// Never the ROM's DFU ID, so `dfu-util -d 0x0483:0xdf11` cannot match the
// running synth.
const _: () = assert!(!(VID_PID.0 == 0x0483 && VID_PID.1 == 0xDF11));

/// Bytes read per loop top at most: a flood without a newline can't hold the UI.
const READ_BUDGET: usize = 256;

type Dev = UsbDevice<'static, UsbBus<USB2>>;
type Port = SerialPort<'static, UsbBus<USB2>>;

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
}

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
}

/// Brings the port up, once: the CRS trimming HSI48 from the host's SOF,
/// the OTG core, then the CDC-ACM device.
pub fn init(parts: UsbParts, clocks: &CoreClocks, cpu_hz: u32) -> Usb {
    let UsbParts {
        dm,
        dp,
        global,
        device,
        pwrclk,
        rec,
        crs,
        crs_rec,
    } = parts;
    crs_rec.enable();
    // SAFETY: SYNCSRC is a 2-bit field and 0b11 is a defined value, USB2
    // OTG FS SOF (RM0433 CRS_CFGR); CEN is still clear, so it may be written.
    crs.cfgr.modify(|_, w| unsafe { w.syncsrc().bits(0b11) });
    crs.cr
        .modify(|_, w| w.autotrimen().set_bit().cen().set_bit());

    // Without its 48 MHz the core never leaves CSRST and `USB2::new` spins
    // there, interrupts off.
    assert_eq!(rec.get_kernel_clk_mux(), UsbClkSel::Hsi48);
    let usb = USB2::new(global, device, pwrclk, dm, dp, rec, clocks);
    // EP OUT packets only (EP0 and the bulk OUT, 64 bytes each); the TX
    // FIFOs live in the core's own RAM.
    let ep_memory = cortex_m::singleton!(: [u32; 256] = [0; 256]).expect("EP memory taken once");
    let bus = UsbBus::new(usb, ep_memory);
    // Here, so the device and the class can both borrow it for 'static.
    let bus: &'static UsbBusAllocator<UsbBus<USB2>> =
        cortex_m::singleton!(: UsbBusAllocator<UsbBus<USB2>> = bus).expect("USB bus taken once");

    let serial = SerialPort::new(bus);
    // The chip's 96-bit UID: a stable /dev/serial/by-id name per unit.
    let uid = cortex_m::singleton!(: chimera_core::console::SerialNumber = serial_hex(Uid::read()))
        .expect("serial number taken once");
    let strings = StringDescriptors::default()
        .manufacturer("Chimera")
        .product("Chimera console")
        .serial_number(uid.as_str());
    let dev = UsbDeviceBuilder::new(bus, UsbVidPid(VID_PID.0, VID_PID.1))
        .strings(&[strings])
        .expect("one language")
        .self_powered(true)
        .max_power(100)
        .expect("100 mA is within 500")
        .device_class(USB_CLASS_CDC)
        .build();
    Usb {
        dev,
        serial,
        line: Console::new(),
        timer: LoopTimer::new(),
        top: DWT::cycle_count(),
        last: Served::Idle,
        cycles_per_us: (cpu_hz / 1_000_000).max(1),
    }
}

impl Usb {
    /// The UI loop's first line, its snapshot point: the last iteration has
    /// flushed, so `frame` is what the panel shows. Times the last lap (an
    /// answered one is not counted), polls, reads into the line, and
    /// answers at most one request.
    pub fn service(
        &mut self,
        ui: &UiState,
        stats: Option<&mut Reader<AudioStats>>,
        bench: Option<&str>,
        frame: Frame<'_>,
    ) {
        let now = DWT::cycle_count();
        let us = now.wrapping_sub(self.top) / self.cycles_per_us;
        self.top = now;
        self.timer.lap(us, self.last);
        let Usb {
            dev,
            serial,
            line,
            timer,
            ..
        } = self;
        dev.poll(&mut [&mut *serial]);
        self.last = Served::Idle;
        // A stalled answer's tail still waiting: no new request until the
        // host reads it, so a host that writes but never reads NAKs itself
        // instead of costing a stall every lap.
        if let Err(UsbError::WouldBlock) = serial.flush() {
            return;
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
            };
            let out = &mut UsbOut {
                dev,
                serial,
                clock: AnswerClock::start(crate::controls::now_ms()),
            };
            // A stalled answer just stops: the host tool resyncs on its next request.
            let _ = answer(req, unit, out).and_then(|()| out.flush());
            self.last = Served::Answered;
            break;
        }
    }
}

/// What the console reads, borrowed for one answer.
struct ChipUnit<'a> {
    ui: &'a UiState,
    stats: Option<&'a mut Reader<AudioStats>>,
    timer: &'a mut LoopTimer,
    bench: Option<&'a str>,
    frame: Frame<'a>,
}

impl Unit for ChipUnit<'_> {
    fn ui(&self) -> &UiState {
        self.ui
    }

    /// `None` without `perf-probe`: no reader.
    fn stats(&mut self) -> Option<Stats> {
        let audio = crate::audio_stats(self.stats.as_deref_mut())?;
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
