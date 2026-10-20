//! The STM32H7 SPI2 and DMA2 register sequences as data. The shell runs
//! each `Op` as one register access (or one bounded wait); host tests
//! check the order.

const SSI: u32 = 1 << 12;
const SPE: u32 = 1;

/// A CR1 value. Both carry SSI: a write without it is MODF (SSM = 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cr1(u32);

impl Cr1 {
    pub const ON: Cr1 = Cr1(SSI | SPE);
    pub const OFF: Cr1 = Cr1(SSI);

    pub const fn bits(self) -> u32 {
        self.0
    }
}

const _: () = assert!(Cr1::ON.bits() & SSI != 0 && Cr1::OFF.bits() & SSI != 0);
const _: () = assert!(Cr1::ON.bits() & SPE != 0 && Cr1::OFF.bits() & SPE == 0);

/// CR2.TSIZE's range, 1 to 0xFFFF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BurstLen(u16);

impl BurstLen {
    pub const fn new(n: usize) -> Option<Self> {
        if n == 0 || n > 0xFFFF {
            None
        } else {
            Some(Self(n as u16))
        }
    }

    pub const fn get(self) -> usize {
        self.0 as usize
    }

    /// TSIZE's and NDTR's 16 bits, never 0.
    pub const fn as_u16(self) -> u16 {
        self.0
    }
}

/// CFG1.MBR, 0 to 7: SCK = kernel >> (mbr + 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mbr(u8);

impl Mbr {
    /// The smallest divider whose SCK is at most `max_hz`.
    pub const fn fastest(kernel_hz: u32, max_hz: u32) -> Option<Self> {
        let mut m = 0;
        while m < 8 {
            if kernel_hz >> (m + 1) <= max_hz {
                return Some(Self(m));
            }
            m += 1;
        }
        None
    }

    pub const fn sck_hz(self, kernel_hz: u32) -> u32 {
        kernel_hz >> (self.0 + 1)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// If CR1.CSTART reads started: CSUSP, then wait for SR.SUSP.
    Suspend,
    WaitEot,
    /// SR.OVR or SR.MODF gives `Err`.
    CheckErrors,
    /// IFCR: EOT, TXTF, SUSP, OVR, MODF, UDR, TIFRE.
    ClearFlags,
    Cr1(Cr1),
    /// CR1.CSTART modify.
    Start,
    /// CFG2.AFCNTR modify.
    AfcntrOn,
    /// CFG2.MASTER modify, with SPE off and MODF cleared: a MODF clears it
    /// with SPE, and a slave never clocks.
    MasterOn,
    /// CFG1, saved at take and put back at give.
    SaveCfg1,
    SetMbr,
    RestoreCfg1,
    Tsize(Option<BurstLen>),
    RxDma(bool),
    TxDma(bool),
    /// LIFCR: every flag of streams 0 and 1.
    ClearStreams,
    /// RX's NDTR, M0AR and EN, then TX's; `StreamOn` if either still
    /// reads EN.
    StreamsOn(BurstLen),
    /// EN = 0 on both, then wait until both read 0.
    StreamsOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegFault {
    Timeout(Op),
    Ovr,
    Modf,
    /// `StreamsOn` found a stream still enabled: its NDTR and M0AR would
    /// not take.
    StreamOn,
}

/// Each op in turn, stopping at the first fault: a take, a burst, its end.
pub fn run(ops: &[Op], f: impl FnMut(Op) -> Result<(), RegFault>) -> Result<(), RegFault> {
    ops.iter().copied().try_for_each(f)
}

/// Every op, past any fault: a give or an abort leaves nothing half
/// undone. The first fault is returned.
pub fn run_all(ops: &[Op], mut f: impl FnMut(Op) -> Result<(), RegFault>) -> Result<(), RegFault> {
    let mut first = Ok(());
    for &o in ops {
        let r = f(o);
        if first.is_ok() {
            first = r;
        }
    }
    first
}

/// CFG1 is saved first, before the one op that can fail: a take given
/// straight back restores the HAL's CFG1, never one it never read.
pub const TAKE: [Op; 7] = [
    Op::SaveCfg1,
    Op::Suspend,
    Op::ClearFlags,
    Op::Cr1(Cr1::OFF),
    Op::SetMbr,
    Op::AfcntrOn,
    Op::MasterOn,
];

/// Polled bytes follow (CMD18, CMD12, R1, busy), each bounded.
pub const POLLED: [Op; 3] = [Op::Tsize(None), Op::Cr1(Cr1::ON), Op::Start];

/// AFCNTR and TSIZE are set while SPE = 0; RXDMAEN before the streams and
/// TXDMAEN after (RM0433's SPI DMA sequence).
pub const fn burst(n: BurstLen) -> [Op; 11] {
    [
        Op::Suspend,
        Op::ClearFlags,
        Op::Cr1(Cr1::OFF),
        Op::AfcntrOn,
        Op::Tsize(Some(n)),
        Op::RxDma(true),
        Op::ClearStreams,
        Op::StreamsOn(n),
        Op::TxDma(true),
        Op::Cr1(Cr1::ON),
        Op::Start,
    ]
}

pub const BURST_END: [Op; 6] = [
    Op::WaitEot,
    Op::CheckErrors,
    Op::ClearFlags,
    Op::Cr1(Cr1::OFF),
    Op::RxDma(false),
    Op::TxDma(false),
];

/// Leaves the peripheral as the HAL left it; AFCNTR stays on (harmless to
/// the HAL, and SCK stays driven).
pub const GIVE: [Op; 9] = [
    Op::Suspend,
    Op::ClearFlags,
    Op::Cr1(Cr1::OFF),
    Op::MasterOn,
    Op::RxDma(false),
    Op::TxDma(false),
    Op::RestoreCfg1,
    Op::Tsize(None),
    Op::Cr1(Cr1::ON),
];

/// The shell then runs `POLLED`, CMD12, `stop_response`, the busy wait, CS
/// high, one 0xFF, then `GIVE`.
pub const ABORT: [Op; 8] = [
    Op::StreamsOff,
    Op::Suspend,
    Op::Cr1(Cr1::OFF),
    Op::ClearStreams,
    Op::ClearFlags,
    Op::MasterOn,
    Op::RxDma(false),
    Op::TxDma(false),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaEvent {
    Complete,
    Error,
    Spurious,
}

impl DmaEvent {
    /// TE wins over TC.
    pub const fn of(tcif: bool, teif: bool) -> Self {
        if teif {
            DmaEvent::Error
        } else if tcif {
            DmaEvent::Complete
        } else {
            DmaEvent::Spurious
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    Advance,
    Abort,
    Ignore,
    Idle,
}

/// A spurious entry mid-transfer is a software pend (I6): ignored.
pub const fn gate(in_flight: bool, ev: DmaEvent) -> Gate {
    match (in_flight, ev) {
        (_, DmaEvent::Error) => Gate::Abort,
        (true, DmaEvent::Complete) => Gate::Advance,
        (true, DmaEvent::Spurious) => Gate::Ignore,
        (false, _) => Gate::Idle,
    }
}
