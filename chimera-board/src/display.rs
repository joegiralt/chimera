// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ILI9341 display driver via SPI1.
//! 240x320 RGB565, framebuffer in static BSS (too large for stack).

use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use chimera_core::ui::theme_settings::{Gamma, Palette, tables};
use chimera_hal::panel::{self, Health, Link};
use chimera_hal::{ChimeraDisplay, FB_SIZE, SCREEN_HEIGHT, SCREEN_WIDTH};
use embedded_graphics_core::Pixel;
use embedded_graphics_core::draw_target::DrawTarget;
use embedded_graphics_core::geometry::{OriginDimensions, Size};
use embedded_graphics_core::pixelcolor::Rgb565;
use embedded_graphics_core::pixelcolor::raw::{RawData, RawU16};
use stm32h7xx_hal::hal::blocking::spi::Write;
use stm32h7xx_hal::hal::digital::v2::OutputPin;

static mut FRAMEBUFFER: [u16; FB_SIZE] = [0u16; FB_SIZE];
static FRAMEBUFFER_TAKEN: AtomicBool = AtomicBool::new(false);

/// SPI1 transfers that ended in an error, for `stats`.
pub static SPI_ERRORS: AtomicU32 = AtomicU32::new(0);
/// Times the health check re-inited the panel (ADR 0326), for `stats`.
pub static PANEL_REINITS: AtomicU32 = AtomicU32::new(0);
/// The last answer to each of RDDPM, RDDMADCTL, RDDCOLMOD, as `0x100 |
/// byte`; 0 when the read failed (zero-initialised: no flash for the
/// statics).
/// For `stats`' `panel_read` (ADR 0332).
static LAST_READ: [AtomicU32; 3] = [const { AtomicU32::new(0) }; 3];
/// Reads attempted, every register counted.
static READS: AtomicU32 = AtomicU32::new(0);
/// The health check has seen the panel healthy once: it is judging.
pub static PANEL_ARMED: AtomicBool = AtomicBool::new(false);

/// The framebuffer, once: `Stm32Display` owns it from then on.
pub fn take_framebuffer() -> Option<&'static mut [u16; FB_SIZE]> {
    if FRAMEBUFFER_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `FRAMEBUFFER` ever made.
    Some(unsafe { &mut *addr_of_mut!(FRAMEBUFFER) })
}

type Spi1 = stm32h7xx_hal::pac::spi1::RegisterBlock;

/// SPI1's registers, driven here rather than through the HAL's `Spi`.
fn spi1() -> &'static Spi1 {
    // SAFETY: this aliases the HAL's `Spi<SPI1>`, which `Stm32Display`
    // holds (`_spi`) so nothing else can take SPI1 and never calls: every
    // SPI1 access is `put`, `drain`, `link_up` or the read, all from the
    // UI loop that owns the display, and no interrupt touches SPI1.
    unsafe { &*stm32h7xx_hal::pac::SPI1::ptr() }
}

/// Polls of one flag before the link gives up: a byte at 50 MHz is 160 ns,
/// two at a read's 6.25 MHz ~2.6 µs.
const POLLS: u32 = 20_000;

/// SPI1's SR flags waited on (RM0433 SPI_SR).
const TXP: u32 = 1 << 1;
const SUSP: u32 = 1 << 11;
const TXC: u32 = 1 << 12;

/// SR's `flag` set within `POLLS`, else counted for `stats`.
fn wait(flag: u32) -> bool {
    let spi = spi1();
    let mut n = POLLS;
    while spi.sr.read().bits() & flag == 0 {
        if n == 0 {
            SPI_ERRORS.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        n -= 1;
    }
    true
}

/// `bytes` into SPI1's FIFO, transmit-only (`lib.rs`): no RX to drain, so
/// it streams. CSTART once, on the first byte; `drain` before CS rises.
#[inline(never)]
fn put(bytes: &[u8]) {
    let spi = spi1();
    let txdr = core::ptr::addr_of!(spi.txdr).cast::<u8>().cast_mut();
    let mut started = spi.cr1.read().cstart().bit_is_set();
    for &b in bytes {
        if !wait(TXP) {
            return;
        }
        // SAFETY: TXP was just read set, so the FIFO has room; a byte
        // write of TXDR is one 8-bit frame.
        unsafe { core::ptr::write_volatile(txdr, b) };
        if !started {
            spi.cr1.modify(|_, w| w.cstart().set_bit());
            started = true;
        }
    }
}

/// The last bit out (TXC): before CS rises or DC changes.
fn drain() {
    wait(TXC);
}

/// SPI1 off, its endless transfer suspended first (RM0433's disable
/// procedure): MBR and COMM only change with SPE clear. AFCNTR keeps SCK
/// and MOSI driven meanwhile.
#[cfg_attr(
    all(target_os = "none", feature = "bank2"),
    unsafe(link_section = ".bank2_panel.read")
)]
fn off(spi: &Spi1) {
    if spi.cr1.read().cstart().bit_is_set() {
        spi.cr1.modify(|_, w| w.csusp().set_bit());
        wait(SUSP);
        spi.ifcr.write(|w| w.suspc().set_bit());
    }
    spi.cr1.modify(|_, w| w.spe().clear_bit());
}

/// Once at boot, before any transfer (no CSTART yet): SPI1 keeps its pins
/// driven while disabled (CFG2 AFCNTR), so a clock or duplex change never
/// floats SCK or MOSI.
fn link_up() {
    let spi = spi1();
    spi.cr1.modify(|_, w| w.spe().clear_bit());
    spi.cfg2.modify(|_, w| w.afcntr().set_bit());
    spi.cr1.modify(|_, w| w.spe().set_bit());
}

pub struct Stm32Display<SPI, DC, RST, CS> {
    /// SPI1, held so nothing else takes it; `put` and `slow_read` drive its
    /// registers.
    _spi: SPI,
    dc: DC,
    reset: RST,
    cs: CS,
    fb: &'static mut [u16; FB_SIZE],
    /// SETTINGS › PERSONALIZE › THEME's colours, swapped in as pixels go
    /// out; the framebuffer keeps the canonical palette.
    palette: Palette,
    /// The panel's mode as read back (ADR 0326); no check in `sd-probe`.
    #[cfg_attr(feature = "sd-probe", allow(dead_code))]
    health: Health,
    cpu_hz: u32,
    /// THEME's GAMMA as last pushed, for a re-init to push again.
    #[cfg_attr(feature = "sd-probe", allow(dead_code))]
    gamma: Gamma,
}

impl<SPI, DC, RST, CS> Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
    pub fn new(spi: SPI, dc: DC, reset: RST, cs: CS, fb: &'static mut [u16; FB_SIZE]) -> Self {
        link_up();
        Self {
            _spi: spi,
            dc,
            reset,
            cs,
            fb,
            palette: Palette::IDENTITY,
            health: Health::new(),
            cpu_hz: 0,
            gamma: Gamma::Panel,
        }
    }

    /// The framebuffer and the palette it is flushed through, for the console.
    #[cfg(feature = "usb-console")]
    pub fn frame(&self) -> chimera_core::console::Frame<'_> {
        chimera_core::console::Frame {
            fb: self.fb,
            palette: self.palette,
        }
    }

    /// Show the framebuffer through `palette` from the next flush on.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Positive (E0h) and negative (E1h) gamma correction, live.
    pub fn set_gamma(&mut self, g: Gamma) {
        self.gamma = g;
        let t = tables(g);
        self.cmd_data(panel::cmd::PGAMCTRL, &t.positive);
        self.cmd_data(panel::cmd::NGAMCTRL, &t.negative);
    }

    /// Push framebuffer pixels `start..end` (already windowed) through the palette.
    fn write_pixels(&mut self, start: usize, end: usize) {
        let _ = self.dc.set_high();
        let _ = self.cs.set_low();
        let mut bytes = [0u8; 512];
        for chunk in self.fb[start..end].chunks(256) {
            self.palette.map_into(chunk, &mut bytes);
            put(&bytes[..chunk.len() * 2]);
        }
        drain();
        let _ = self.cs.set_high();
    }

    /// One CS frame of `bytes`, DC high for data.
    fn send(&mut self, data: bool, bytes: &[u8]) {
        let _ = if data {
            self.dc.set_high()
        } else {
            self.dc.set_low()
        };
        let _ = self.cs.set_low();
        put(bytes);
        drain();
        let _ = self.cs.set_high();
    }

    fn cmd(&mut self, cmd: u8) {
        self.send(false, &[cmd]);
    }

    fn data_bytes(&mut self, data: &[u8]) {
        self.send(true, data);
    }

    fn cmd_data(&mut self, cmd: u8, data: &[u8]) {
        self.cmd(cmd);
        self.data_bytes(data);
    }

    /// ILI9341 init sequence (`chimera_hal::panel::init`). Leaves gamma at
    /// the panel's reset tables (THEME's PANEL); the caller pushes the boot
    /// theme's actual gamma right after.
    pub fn init(&mut self, cpu_hz: u32) {
        self.cpu_hz = cpu_hz;
        panel::init(self);
    }

    /// The health check, when due (ADR 0326): a panel re-inited takes
    /// THEME's gamma and the whole framebuffer again. Bank 2's, through
    /// the board's door.
    #[cfg(not(feature = "sd-probe"))]
    #[cfg_attr(
        all(target_os = "none", feature = "bank2"),
        unsafe(link_section = ".bank2_panel.tend")
    )]
    pub fn tend(&mut self) {
        let mut h = self.health;
        let reinit = panel::tend(&mut h, crate::controls::now_ms(), self);
        self.health = h;
        PANEL_ARMED.store(h.armed(), Ordering::Relaxed);
        if reinit {
            PANEL_REINITS.fetch_add(1, Ordering::Relaxed);
            self.set_gamma(self.gamma);
            self.flush();
        }
    }
}

impl<SPI, DC, RST, CS> Link for Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
    fn cmd(&mut self, c: u8) {
        Stm32Display::cmd(self, c);
    }

    fn data(&mut self, d: &[u8]) {
        self.data_bytes(d);
    }

    #[cfg_attr(
        all(target_os = "none", feature = "bank2"),
        unsafe(link_section = ".bank2_panel.read")
    )]
    fn read(&mut self, c: u8) -> Option<u8> {
        // CS is high: every frame before this one drained and raised it.
        // The clock and duplex change only here, between frames.
        reading(true);
        let _ = self.dc.set_low();
        let _ = self.cs.set_low();
        let mut cmd = [0u8; 1];
        let mut ans = [0u8; 1];
        // The command with DC low; the answer clocked in with DC high, as
        // a data read (the panel samples D/CX on each byte's 8th bit: low,
        // our dummy 0xFF would be taken as a command). Measured on the
        // owner's unit 2026-11-23: `9c 48 05`, no dummy bit (datasheet
        // V1.11 p. 38, 8-bit read; ADR 0332).
        let mut ok = exchange(&[c], &mut cmd);
        if ok {
            let _ = self.dc.set_high();
            ok = exchange(&[0xFF], &mut ans);
        }
        let _ = self.cs.set_high();
        reading(false);
        READS.fetch_add(1, Ordering::Relaxed);
        let raw = if ok { 0x100 | u32::from(ans[0]) } else { 0 };
        if let Some(slot) = LAST_READ.get(usize::from(c.wrapping_sub(panel::cmd::RDDPM))) {
            slot.store(raw, Ordering::Relaxed);
        }
        if ok { Some(ans[0]) } else { None }
    }

    fn reset(&mut self, low: bool) {
        let _ = if low {
            self.reset.set_low()
        } else {
            self.reset.set_high()
        };
    }

    fn delay_us(&mut self, us: u32) {
        crate::clocks::delay_us(self.cpu_hz, us);
    }
}

/// `stats`' lines for the link (ADR 0326), written in bank 2.
#[cfg(not(feature = "sd-probe"))]
#[cfg_attr(
    all(target_os = "none", feature = "bank2"),
    unsafe(link_section = ".bank2_panel.stats")
)]
pub(crate) fn stats_lines(t: &mut dyn core::fmt::Write) -> core::fmt::Result {
    writeln!(
        t,
        "panel_armed {}\npanel_reinits {}\nspi_errors {}",
        u8::from(PANEL_ARMED.load(Ordering::Relaxed)),
        PANEL_REINITS.load(Ordering::Relaxed),
        SPI_ERRORS.load(Ordering::Relaxed)
    )?;
    // `panel_read 9c 48 05 reads 12`: each register's answer, hex; `none`
    // for one whose read failed. Built by
    // hand from `READ_WORDS`, so its text and digits stay in bank 2.
    /// `panel_read`'s words, in bank 2 with the code that writes them.
    #[cfg_attr(
        all(target_os = "none", feature = "bank2"),
        unsafe(link_section = ".bank2_panel.rodata")
    )]
    static READ_WORDS: [u8; 22] = *b"panel_read none reads ";
    let w = &READ_WORDS;
    let mut line = [0u8; 48];
    let mut at = 0;
    // A closure would be its own function, outside this one's section.
    macro_rules! put {
        ($b:expr) => {{
            let b: &[u8] = $b;
            for &c in b {
                if let Some(d) = line.get_mut(at) {
                    *d = c;
                    at += 1;
                }
            }
        }};
    }
    put!(&w[..10]);
    for r in &LAST_READ {
        match r.load(Ordering::Relaxed) {
            0 => put!(&w[10..15]),
            v => {
                let mut d = [b' '; 3];
                for (i, c) in d[1..].iter_mut().enumerate() {
                    let n = (v >> (4 - 4 * i)) as u8 & 0xF;
                    *c = if n < 10 { b'0' + n } else { b'a' + n - 10 };
                }
                put!(&d);
            }
        }
    }
    put!(&w[15..22]);
    let mut n = READS.load(Ordering::Relaxed);
    let mut digits = [0u8; 10];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    put!(&digits[i..]);
    put!(b"\n");
    // SAFETY: every byte written above is ASCII.
    t.write_str(unsafe { core::str::from_utf8_unchecked(&line[..at]) })
}

/// SPI1 for a read: 100 MHz / 16 = 6.25 MHz, inside the ILI9341's 150 ns
/// read cycle (datasheet V1.11 § 19.3), and full duplex to hear the panel;
/// or back to the writes' /2 (50 MHz), transmit-only. With CS high.
#[cfg_attr(
    all(target_os = "none", feature = "bank2"),
    unsafe(link_section = ".bank2_panel.read")
)]
fn reading(on: bool) {
    let spi = spi1();
    off(spi);
    spi.ifcr.write(|w| w.ovrc().set_bit().modfc().set_bit());
    if on {
        spi.cfg1.modify(|_, w| w.mbr().div16());
        spi.cfg2.modify(|_, w| w.comm().full_duplex());
    } else {
        spi.cfg1.modify(|_, w| w.mbr().div2());
        spi.cfg2.modify(|_, w| w.comm().transmitter());
    }
    spi.cr1.modify(|_, w| w.spe().set_bit());
}

/// `tx` out (at most the 16-byte FIFO) and as many bytes in, into `rx`.
/// False, counted: the SPI flagged an error or never answered.
#[cfg_attr(
    all(target_os = "none", feature = "bank2"),
    unsafe(link_section = ".bank2_panel.read")
)]
fn exchange(tx: &[u8], rx: &mut [u8]) -> bool {
    let spi = spi1();
    let txdr = core::ptr::addr_of!(spi.txdr).cast::<u8>().cast_mut();
    let rxdr = core::ptr::addr_of!(spi.rxdr).cast::<u8>();
    for &b in tx {
        // SAFETY: SPI1 was enabled with empty FIFOs and every earlier
        // exchange read back all it sent, so `tx` (at most 16 bytes) fits;
        // byte writes of TXDR are 8-bit frames.
        unsafe { core::ptr::write_volatile(txdr, b) };
    }
    if spi.cr1.read().cstart().bit_is_clear() {
        spi.cr1.modify(|_, w| w.cstart().set_bit());
    }
    for g in rx.iter_mut() {
        let mut n = POLLS;
        loop {
            let sr = spi.sr.read();
            if sr.ovr().bit_is_set() || sr.modf().bit_is_set() || n == 0 {
                SPI_ERRORS.fetch_add(1, Ordering::Relaxed);
                return false;
            }
            if sr.rxp().bit_is_set() {
                // SAFETY: RXP was just read set: a byte waits in RXDR.
                *g = unsafe { core::ptr::read_volatile(rxdr) };
                break;
            }
            n -= 1;
        }
    }
    true
}

impl<SPI, DC, RST, CS> DrawTarget for Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    /// A fill of the framebuffer, not 76,800 `draw_iter` pixels.
    fn clear(&mut self, color: Rgb565) -> Result<(), Self::Error> {
        self.fb.fill(RawU16::from(color).into_inner());
        Ok(())
    }

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        chimera_hal::draw_into_fb(self.fb, pixels);
        Ok(())
    }
}

impl<SPI, DC, RST, CS> OriginDimensions for Stm32Display<SPI, DC, RST, CS> {
    fn size(&self) -> Size {
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    }
}

impl<SPI, DC, RST, CS> ChimeraDisplay for Stm32Display<SPI, DC, RST, CS>
where
    SPI: Write<u8>,
    DC: OutputPin,
    RST: OutputPin,
    CS: OutputPin,
{
    fn flush_region(&mut self, y_start: u16, y_end: u16) {
        panel::window(self, y_start, y_end);
        self.write_pixels(
            y_start as usize * SCREEN_WIDTH as usize,
            y_end as usize * SCREEN_WIDTH as usize,
        );
    }

    fn pixel_buffer(&mut self) -> &mut [u16] {
        self.fb
    }
}
