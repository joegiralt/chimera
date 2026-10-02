//! The USB console's shell (ADR 0068): CDC-ACM on the OTG FS port (PA11/PA12),
//! polled from the UI loop. `pac::Interrupt::OTG_FS` is never unmasked, so
//! USB can never preempt audio.

use stm32h7xx_hal::gpio::{Alternate, PA11, PA12};
use stm32h7xx_hal::pac;
use stm32h7xx_hal::rcc::{CoreClocks, ResetEnable, rec};
use stm32h7xx_hal::usb_hs::{USB2, UsbBus};
use usb_device::bus::UsbBusAllocator;
use usb_device::device::{StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbVidPid};
use usbd_serial::{SerialPort, USB_CLASS_CDC};

/// The stock PreenFM3's ID: Ixox/preenfm3 firmware/Src/usbd_desc.c.
pub const VID_PID: (u16, u16) = (0x0483, 0x5740);

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

pub struct Usb {
    dev: UsbDevice<'static, UsbBus<USB2>>,
    serial: SerialPort<'static, UsbBus<USB2>>,
}

/// Brings the port up, once: the CRS trimming HSI48 from the host's SOF,
/// the OTG core, then the CDC-ACM device.
pub fn init(parts: UsbParts, clocks: &CoreClocks) -> Usb {
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

    let usb = USB2::new(global, device, pwrclk, dm, dp, rec, clocks);
    // EP OUT packets only (EP0 and the bulk OUT, 64 bytes each); the TX
    // FIFOs live in the core's own RAM.
    let ep_memory = cortex_m::singleton!(: [u32; 256] = [0; 256]).expect("EP memory taken once");
    let bus = UsbBus::new(usb, ep_memory);
    // Here, so the device and the class can both borrow it for 'static.
    let bus: &'static UsbBusAllocator<UsbBus<USB2>> =
        cortex_m::singleton!(: UsbBusAllocator<UsbBus<USB2>> = bus).expect("USB bus taken once");

    let serial = SerialPort::new(bus);
    let strings = StringDescriptors::default()
        .manufacturer("Chimera")
        .product("Chimera console")
        // The chip's unique ID from Task 8 on.
        .serial_number("000000000000000000000000");
    let dev = UsbDeviceBuilder::new(bus, UsbVidPid(VID_PID.0, VID_PID.1))
        .strings(&[strings])
        .expect("one language")
        .self_powered(true)
        .max_power(100)
        .expect("100 mA is within 500")
        .device_class(USB_CLASS_CDC)
        .build();
    Usb { dev, serial }
}

impl Usb {
    /// Once per UI loop iteration. Until the console answers, what the host
    /// sends is read and dropped.
    pub fn poll(&mut self) {
        if self.dev.poll(&mut [&mut self.serial]) {
            let mut buf = [0u8; 64];
            while matches!(self.serial.read(&mut buf), Ok(n) if n > 0) {}
        }
    }
}
