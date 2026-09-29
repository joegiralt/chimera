//! SD card in SPI mode on SPI2, polled (spec § Card): SCK PA9, MISO PB14,
//! MOSI PB15, CS PE12. Every transaction checks the operation's deadline.

use chimera_fat::deadline::{Deadline, Over};
use cortex_m::peripheral::{DCB, DWT};
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::{self as ehspi, ErrorKind, ErrorType, Operation, SpiDevice};
use embedded_sdmmc::SdCard;
use embedded_sdmmc::sdcard::spi::AcquireOpts;
use stm32h7xx_hal::gpio::{Output, PA9, PB14, PB15, PE12, PushPull, Speed};
use stm32h7xx_hal::hal::blocking::spi::{Transfer, Write};
use stm32h7xx_hal::pac::SPI2;
use stm32h7xx_hal::prelude::*;
use stm32h7xx_hal::rcc::{CoreClocks, rec};
use stm32h7xx_hal::spi::{self, Enabled, Spi};
use stm32h7xx_hal::time::Hertz;

use crate::clocks;

pub const SD_INIT_HZ: u32 = 400_000;
pub const SD_FAST_HZ: u32 = 25_000_000;
pub const SD_MODE: spi::Mode = spi::MODE_3;
pub const SD_ACQUIRE_RETRIES: u32 = 3;
pub const SD_ACQUIRE_MS: u32 = 1_500;
pub const SD_IDLE_MS: u32 = 600;
pub const SD_OP_CAP_MS: u32 = 10_000;

const _: () = assert!(SD_FAST_HZ <= 25_000_000 && SD_INIT_HZ <= 400_000);

/// A transaction moving a data block restarts the idle deadline.
const BLOCK: usize = 512;
/// ≥ 74 clocks with CS high.
const WAKE_BYTES: usize = 10;
/// SCK cycles in the shortest transaction (one byte).
const MIN_TRANSACTION_SCK: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdSpiError {
    Spi,
    Timeout,
}

impl ehspi::Error for SdSpiError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

pub struct SdSpi {
    spi: Spi<SPI2, Enabled>,
    cs: PE12<Output<PushPull>>,
    clocks: CoreClocks,
    cpu_hz: u32,
    counting: bool,
    hz: Hertz,
    mode: spi::Mode,
    deadline: Deadline,
}

impl SdSpi {
    /// `counting`: the DWT cycle counter runs.
    pub fn new(
        spi: Spi<SPI2, Enabled>,
        cs: PE12<Output<PushPull>>,
        clocks: CoreClocks,
        cpu_hz: u32,
        counting: bool,
        hz: Hertz,
        mode: spi::Mode,
    ) -> Self {
        let mut s = Self {
            spi,
            cs,
            clocks,
            cpu_hz,
            counting,
            hz,
            mode,
            deadline: Deadline::arm_transfers(0, 0),
        };
        s.arm(SD_IDLE_MS);
        s
    }

    pub fn set_hz(&mut self, hz: Hertz) {
        self.rebuild(hz, self.mode);
    }

    pub fn set_mode(&mut self, m: spi::Mode) {
        self.rebuild(self.hz, m);
    }

    fn rebuild(&mut self, hz: Hertz, mode: spi::Mode) {
        // SAFETY: the bitwise copy is overwritten below before `self.spi` is
        // used again, and nothing between can unwind (no_std panics halt), so
        // the peripheral never has two live owners. Neither type has `Drop`.
        let old = unsafe { core::ptr::read(&self.spi) };
        let (regs, rec) = old.disable().free();
        let new = regs.spi_unchecked(spi::Config::new(mode), hz, rec, &self.clocks);
        // SAFETY: `self.spi` holds the stale copy read above; writing over it
        // without dropping it is what ends that copy.
        unsafe { core::ptr::write(&mut self.spi, new) };
        (self.hz, self.mode) = (hz, mode);
    }

    /// Clocks with CS high, as the card needs before CMD0.
    pub fn wake(&mut self) {
        self.cs.set_high();
        // Nothing listens yet: a failed wake shows as a failed acquire.
        let _ = self.spi.write(&[0xFF; WAKE_BYTES]);
    }

    /// Starts an operation at the current clock: `idle_ms` without a block
    /// moved, or `SD_OP_CAP_MS` in all, is a timeout.
    pub fn arm(&mut self, idle_ms: u32) {
        let (idle, cap) = (u64::from(idle_ms), u64::from(SD_OP_CAP_MS));
        self.deadline = if self.counting {
            let per_ms = u64::from(self.cpu_hz / 1_000);
            Deadline::arm(DWT::cycle_count(), idle * per_ms, cap * per_ms)
        } else {
            // ponytail: coarse. Each transaction counts as its shortest (one
            // byte at SCK), so this never times out early but runs long by
            // the per-transaction overhead; the DWT path is exact.
            let per_ms = u64::from(self.hz.raw() / MIN_TRANSACTION_SCK / 1_000);
            Deadline::arm_transfers(idle * per_ms, cap * per_ms)
        };
    }

    pub fn timed_out(&self) -> bool {
        self.deadline.is_over()
    }

    /// The longest time between blocks since `arm`, in µs (0 without the
    /// cycle counter).
    #[cfg(feature = "sd-probe")]
    pub fn max_gap_us(&self) -> u32 {
        (self.deadline.max_gap() * 1_000_000 / u64::from(self.cpu_hz)) as u32
    }

    /// The kernel clock and the SCK it divides down to, in Hz.
    #[cfg(feature = "sd-probe")]
    pub fn sck_hz(&self) -> (u32, u32) {
        use stm32h7xx_hal::spi::HalSpi;
        let ker = Spi::<SPI2, Enabled>::kernel_clk_unwrap(&self.clocks).raw();
        let mbr = self.spi.inner().cfg1.read().mbr().bits();
        (ker, ker >> (mbr + 1))
    }

    #[cfg(feature = "sd-probe")]
    pub fn counts_cycles(&self) -> bool {
        self.counting
    }

    fn tick(&mut self, moved_block: bool) -> Result<(), SdSpiError> {
        self.deadline
            .tick(DWT::cycle_count(), moved_block)
            .map_err(|Over| SdSpiError::Timeout)
    }

    /// The bytes moved.
    fn run(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<usize, SdSpiError> {
        let mut moved = 0;
        for op in ops {
            moved += match op {
                Operation::Read(buf) => {
                    buf.fill(0xFF);
                    self.spi.transfer(buf).map_err(|_| SdSpiError::Spi)?;
                    buf.len()
                }
                Operation::Write(buf) => {
                    self.spi.write(buf).map_err(|_| SdSpiError::Spi)?;
                    buf.len()
                }
                Operation::Transfer(read, write) => {
                    let n = read.len().min(write.len());
                    read[..n].copy_from_slice(&write[..n]);
                    read[n..].fill(0xFF);
                    self.spi.transfer(read).map_err(|_| SdSpiError::Spi)?;
                    self.spi.write(&write[n..]).map_err(|_| SdSpiError::Spi)?;
                    read.len().max(write.len())
                }
                Operation::TransferInPlace(buf) => {
                    self.spi.transfer(buf).map_err(|_| SdSpiError::Spi)?;
                    buf.len()
                }
                Operation::DelayNs(ns) => {
                    CycleDelay::new(self.cpu_hz).delay_ns(*ns);
                    0
                }
            };
        }
        Ok(moved)
    }
}

impl ErrorType for SdSpi {
    type Error = SdSpiError;
}

impl SpiDevice<u8> for SdSpi {
    fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), SdSpiError> {
        self.tick(false)?;
        self.cs.set_low();
        let moved = self.run(ops);
        self.cs.set_high();
        if moved? >= BLOCK {
            self.tick(true)?;
        }
        Ok(())
    }
}

pub struct CycleDelay {
    cpu_hz: u32,
}

impl CycleDelay {
    pub const fn new(cpu_hz: u32) -> Self {
        Self { cpu_hz }
    }
}

impl DelayNs for CycleDelay {
    fn delay_ns(&mut self, ns: u32) {
        clocks::delay_us(self.cpu_hz, ns.div_ceil(1_000));
    }

    fn delay_us(&mut self, us: u32) {
        clocks::delay_us(self.cpu_hz, us);
    }

    fn delay_ms(&mut self, ms: u32) {
        // One ms at a time: `delay_us` holds ~8.9 s of cycles at 480 MHz.
        for _ in 0..ms {
            clocks::delay_us(self.cpu_hz, 1_000);
        }
    }
}

pub type SdDevice = SdCard<SdSpi, CycleDelay>;

/// What the card needs from `boot`.
pub struct SdParts {
    pub spi2: SPI2,
    pub rec: rec::Spi2,
    pub sck: PA9,
    pub miso: PB14,
    pub mosi: PB15,
    pub cs: PE12,
}

/// The card on SPI2, not yet acquired: MISO pulled up, CS high, `SD_MODE`
/// at `SD_INIT_HZ`, then the wake clocks.
pub fn init(
    parts: SdParts,
    dcb: &mut DCB,
    dwt: &mut DWT,
    clocks: &CoreClocks,
    cpu_hz: u32,
) -> SdDevice {
    let counting = clocks::enable_cycle_counter(dcb, dwt);
    let mut cs = parts.cs.into_push_pull_output();
    cs.set_high();
    cs.set_speed(Speed::High);
    // SD drives DO open-drain until selected, and a floating MISO makes the
    // no-card path random.
    let _sck = parts.sck.into_alternate::<5>().speed(Speed::High);
    let _miso = parts
        .miso
        .into_alternate::<5>()
        .speed(Speed::High)
        .internal_pull_up(true);
    let _mosi = parts.mosi.into_alternate::<5>().speed(Speed::High);
    let hz = SD_INIT_HZ.Hz();
    let spi = parts
        .spi2
        .spi_unchecked(spi::Config::new(SD_MODE), hz, parts.rec, clocks);
    let mut sd = SdSpi::new(spi, cs, *clocks, cpu_hz, counting, hz, SD_MODE);
    sd.wake();
    SdCard::new_with_options(
        sd,
        CycleDelay::new(cpu_hz),
        AcquireOpts {
            use_crc: true,
            acquire_retries: SD_ACQUIRE_RETRIES,
        },
    )
}
