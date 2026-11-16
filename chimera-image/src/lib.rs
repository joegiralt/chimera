//! Bank 2's image: header, seal, judgement and the one door (ADR 0137).
//! Pure and `no_std`; the board reads flash through `ImageRead`.
#![no_std]

pub mod crc;
mod header;
mod install;
mod judge;
pub mod regs;
pub mod slot;

pub use header::{
    BANK2_BASE, BANK2_LEN, COMMIT, COMMIT_OFF, FLASH_WORD, FORMAT, HEADER_LEN, Header, IMAGE_BODY,
    MAGIC, Seal, UNSEALED_HEADER, Version,
};
pub use install::{
    AutoInstall, BANK2_BAD, BANK2_INSTALLING, Bank2Sector, Bank2Word, BootAction, Flash, FlashAt,
    FlashFail, InstallFail, Mark, Phase, Plan, PlanOp, Progress, SLOT_TORN, auto_install,
    bank2_ecc_fault, boot_action, fault_mark, guard_marks, install_with, pick_staged, plan,
    slot_mark, slot_mark_set, wants_staged,
};
pub use judge::{Bank2Memory, Door, ImageRead, ReadFail, Verdict, judge, judge_uncommitted};
pub use slot::{Slot, SlotSet};
