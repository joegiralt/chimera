//! Bank 2's image: header, seal, judgement and the one door (ADR 0137).
//! Pure and `no_std`; the board reads flash through `ImageRead`.
#![no_std]

pub mod crc;
mod header;
mod install;
mod judge;
pub mod regs;

pub use header::{
    BANK2_BASE, BANK2_LEN, COMMIT, COMMIT_OFF, FLASH_WORD, FORMAT, HEADER_LEN, Header, MAGIC, Seal,
    UNSEALED_HEADER, Version,
};
pub use install::{
    AutoInstall, BANK2_BAD, Bank2Sector, Bank2Word, BootAction, Flash, FlashAt, FlashFail,
    InstallFail, Plan, auto_install, boot_action, install, plan,
};
pub use judge::{Bank2Memory, Door, ImageRead, ReadFail, Verdict, judge, judge_uncommitted};
