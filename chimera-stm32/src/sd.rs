//! SD card in SPI mode on SPI2, polled (spec § Card): SCK PA9, MISO PB14,
//! MOSI PB15, CS PE12. Every transaction checks the operation's deadline.

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

use crate::clocks;

pub const SD_INIT_HZ: u32 = 400_000;
pub const SD_FAST_HZ: u32 = 12_500_000;
pub const SD_MODE: spi::Mode = spi::MODE_0;
pub const SD_ACQUIRE_RETRIES: u32 = 3;
pub const SD_ACQUIRE_MS: u32 = 250;
pub const SD_IDLE_MS: u32 = 600;
pub const SD_OP_CAP_MS: u32 = 10_000;

/// A transaction moving a data block restarts the idle deadline.
const BLOCK: usize = 512;
/// Without the cycle counter, transactions stand in for time: none takes
/// under 1 µs, so a count of these per ms never times out early.
const TRANSACTIONS_PER_MS: u32 = 1_000;
/// ≥ 74 clocks with CS high.
const WAKE_BYTES: usize = 10;

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

/// The running operation's budget.
enum Deadline {
    /// Cycles since the last block and since `arm`, summed from wrapping DWT
    /// reads (the counter wraps every ~9 s, under `SD_OP_CAP_MS`).
    Cycles { last: u32, idle: u32, op: u64 },
    /// Transactions left before the idle and the cap deadlines.
    Transfers { left: u32, cap: u32 },
}

pub struct SdSpi {
    spi: Spi<SPI2, Enabled>,
    cs: PE12<Output<PushPull>>,
    clocks: CoreClocks,
    cpu_hz: u32,
    hz: u32,
    mode: spi::Mode,
    deadline: Deadline,
    idle_ms: u32,
    timed_out: bool,
    #[cfg(feature = "sd-probe")]
    max_gap: u32,
}

impl SdSpi {
    /// `counting`: the DWT cycle counter runs.
    pub fn new(
        spi: Spi<SPI2, Enabled>,
        cs: PE12<Output<PushPull>>,
        clocks: CoreClocks,
        cpu_hz: u32,
        counting: bool,
        hz: u32,
        mode: spi::Mode,
    ) -> Self {
        let deadline = if counting {
            Deadline::Cycles {
                last: 0,
                idle: 0,
                op: 0,
            }
        } else {
            Deadline::Transfers { left: 0, cap: 0 }
        };
        let mut s = Self {
            spi,
            cs,
            clocks,
            cpu_hz,
            hz,
            mode,
            deadline,
            idle_ms: SD_IDLE_MS,
            timed_out: false,
            #[cfg(feature = "sd-probe")]
            max_gap: 0,
        };
        s.arm(SD_IDLE_MS);
        s
    }

    pub fn set_hz(&mut self, hz: u32) {
        self.rebuild(hz, self.mode);
    }

    pub fn set_mode(&mut self, m: spi::Mode) {
        self.rebuild(self.hz, m);
    }

    fn rebuild(&mut self, hz: u32, mode: spi::Mode) {
        // SAFETY: the bitwise copy is overwritten below before `self.spi` is
        // used again, and nothing between can unwind (no_std panics halt), so
        // the peripheral never has two live owners. Neither type has `Drop`.
        let old = unsafe { core::ptr::read(&self.spi) };
        let (regs, rec) = old.disable().free();
        let new = regs.spi_unchecked(spi::Config::new(mode), hz.Hz(), rec, &self.clocks);
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

    /// Starts an operation: `idle_ms` without a block moved, or
    /// `SD_OP_CAP_MS` in all, is a timeout.
    pub fn arm(&mut self, idle_ms: u32) {
        self.timed_out = false;
        self.idle_ms = idle_ms;
        self.deadline = match self.deadline {
            Deadline::Cycles { .. } => Deadline::Cycles {
                last: DWT::cycle_count(),
                idle: 0,
                op: 0,
            },
            Deadline::Transfers { .. } => Deadline::Transfers {
                left: idle_ms.saturating_mul(TRANSACTIONS_PER_MS),
                cap: SD_OP_CAP_MS * TRANSACTIONS_PER_MS,
            },
        };
        #[cfg(feature = "sd-probe")]
        {
            self.max_gap = 0;
        }
    }

    pub fn timed_out(&self) -> bool {
        self.timed_out
    }

    /// The longest time between blocks since `arm`, in µs (0 without the
    /// cycle counter).
    #[cfg(feature = "sd-probe")]
    pub fn max_gap_us(&self) -> u32 {
        (u64::from(self.max_gap) * 1_000_000 / u64::from(self.cpu_hz)) as u32
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
        matches!(self.deadline, Deadline::Cycles { .. })
    }

    /// Spends one transaction's worth of the deadline.
    fn check(&mut self) -> Result<(), SdSpiError> {
        let over = match &mut self.deadline {
            Deadline::Cycles { last, idle, op } => {
                let now = DWT::cycle_count();
                let d = now.wrapping_sub(*last);
                *last = now;
                *idle = idle.saturating_add(d);
                *op += u64::from(d);
                let ms = |c: u64| c * 1_000 / u64::from(self.cpu_hz);
                ms(u64::from(*idle)) >= u64::from(self.idle_ms)
                    || ms(*op) >= u64::from(SD_OP_CAP_MS)
            }
            Deadline::Transfers { left, cap } => {
                *left = left.saturating_sub(1);
                *cap = cap.saturating_sub(1);
                *left == 0 || *cap == 0
            }
        };
        self.timed_out |= over;
        if self.timed_out {
            Err(SdSpiError::Timeout)
        } else {
            Ok(())
        }
    }

    fn block_moved(&mut self) {
        match &mut self.deadline {
            Deadline::Cycles { idle, .. } => {
                #[cfg(feature = "sd-probe")]
                {
                    self.max_gap = self.max_gap.max(*idle);
                }
                *idle = 0;
            }
            Deadline::Transfers { left, .. } => {
                *left = self.idle_ms.saturating_mul(TRANSACTIONS_PER_MS);
            }
        }
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
        self.check()?;
        self.cs.set_low();
        let moved = self.run(ops);
        self.cs.set_high();
        if moved? >= BLOCK {
            self.block_moved();
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

/// The card on SPI2, not yet acquired: MISO pulled up, CS high, `SD_MODE`
/// at `SD_INIT_HZ`, then the wake clocks.
#[allow(clippy::too_many_arguments)]
pub fn init(
    spi2: SPI2,
    rec: rec::Spi2,
    pa9: PA9,
    pb14: PB14,
    pb15: PB15,
    pe12: PE12,
    dcb: &mut DCB,
    dwt: &mut DWT,
    clocks: &CoreClocks,
    cpu_hz: u32,
) -> SdDevice {
    let counting = clocks::enable_cycle_counter(dcb, dwt);
    let mut cs = pe12.into_push_pull_output();
    cs.set_high();
    cs.set_speed(Speed::High);
    // SD drives DO open-drain until selected, and a floating MISO makes the
    // no-card path random.
    let _sck = pa9.into_alternate::<5>().speed(Speed::High);
    let _miso = pb14
        .into_alternate::<5>()
        .speed(Speed::High)
        .internal_pull_up(true);
    let _mosi = pb15.into_alternate::<5>().speed(Speed::High);
    let spi = spi2.spi_unchecked(spi::Config::new(SD_MODE), SD_INIT_HZ.Hz(), rec, clocks);
    let mut sd = SdSpi::new(spi, cs, *clocks, cpu_hz, counting, SD_INIT_HZ, SD_MODE);
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
