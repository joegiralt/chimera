//! How a card addresses a block: SDHC and SDXC by block, SDSC by byte.
//! A command argument is built only here, from the card type, so a block
//! number is never sent as a byte address or the reverse.

use core::sync::atomic::{AtomicU8, Ordering};

use chimera_hal::units::Lba;
use embedded_sdmmc::embedded_sdmmc_types::sdcard::CardType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddrMode {
    Block,
    Byte,
}

impl AddrMode {
    pub const fn of(card: CardType) -> Self {
        match card {
            CardType::SD1 | CardType::SD2 => Self::Byte,
            CardType::SdhcSdxc => Self::Block,
        }
    }

    /// The read/write argument for `lba`; `None` past an SDSC card's 4 GiB.
    pub const fn arg(self, lba: Lba) -> Option<CmdArg> {
        match self {
            Self::Block => Some(CmdArg(lba.get())),
            Self::Byte => match lba.get().checked_mul(crate::blocks::BLOCK as u32) {
                Some(a) => Some(CmdArg(a)),
                None => None,
            },
        }
    }
}

/// A command argument: built by `AddrMode::arg` or one of the fixed ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CmdArg(u32);

impl CmdArg {
    pub const ZERO: CmdArg = CmdArg(0);
    /// CMD8: 2.7-3.6 V, check pattern 0xAA.
    pub const IF_COND: CmdArg = CmdArg(0x1AA);

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// The mounted card's address mode, shared with the card IRQ; unknown
/// until a mount sets it.
pub struct CardAddr(AtomicU8);

impl CardAddr {
    const NONE: u8 = 0;
    const BLOCK: u8 = 1;
    const BYTE: u8 = 2;

    pub const fn new() -> Self {
        Self(AtomicU8::new(Self::NONE))
    }

    pub fn set(&self, m: AddrMode) {
        let b = match m {
            AddrMode::Block => Self::BLOCK,
            AddrMode::Byte => Self::BYTE,
        };
        self.0.store(b, Ordering::Release);
    }

    pub fn clear(&self) {
        self.0.store(Self::NONE, Ordering::Release);
    }

    pub fn get(&self) -> Option<AddrMode> {
        match self.0.load(Ordering::Acquire) {
            Self::BLOCK => Some(AddrMode::Block),
            Self::BYTE => Some(AddrMode::Byte),
            _ => None,
        }
    }
}

impl Default for CardAddr {
    fn default() -> Self {
        Self::new()
    }
}
