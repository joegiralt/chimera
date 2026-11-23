// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `flash`: the flash controller's state for bank 2 (RM0433 § 4.9), read
//! only. Raw words in, decoded lines out; the shell does the reads.

use core::fmt::{self, Write};

/// One bank's registers: FLASH_CRx, SRx, PRAR_CURx, SCAR_CURx, WPSN_CURxR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BankRegs {
    pub cr: u32,
    pub sr: u32,
    pub prar_cur: u32,
    pub scar_cur: u32,
    pub wpsn_cur: u32,
}

/// The controller's registers, read once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlashRegs {
    pub acr: u32,
    pub optcr: u32,
    pub optsr_cur: u32,
    pub boot_cur: u32,
    pub banks: [BankRegs; 2],
}

/// Where `FlashPeek`'s words come from: bank 2's start, S0. It never peeks
/// at the slots (0x08180000+); only the install's guard reads them.
pub const PEEK_AT: u32 = 0x0810_0000;

/// The first 32 bytes at `PEEK_AT`, then SR1 and SR2 read after them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlashPeek {
    pub words: [u32; 8],
    pub sr_after: [u32; 2],
}

const NAMES: [[&str; 3]; 2] = [
    ["sr1", "prar_cur1", "scar_cur1"],
    ["sr2", "prar_cur2", "scar_cur2"],
];

const BANK_BASE: [u32; 2] = [0x0800_0000, 0x0810_0000];

fn bit(v: u32, n: u32) -> u32 {
    (v >> n) & 1
}

/// RDP's byte as its level: 0xAA is 0, 0xCC is 2, any other is 1.
pub fn rdp_level(optsr: u32) -> u8 {
    match (optsr >> 8) & 0xff {
        0xaa => 0,
        0xcc => 2,
        _ => 1,
    }
}

/// The sectors whose WRPSn bit is 0 (protected), as a bit set.
pub fn protected_sectors(wpsn: u32) -> u8 {
    !(wpsn as u8)
}

/// A PCROP or secure area: `None` when START > END (no area), else its
/// first and last byte in `bank`.
pub fn area(reg: u32, bank: usize) -> Option<(u32, u32)> {
    let start = reg & 0xfff;
    let end = (reg >> 16) & 0xfff;
    let base = BANK_BASE.get(bank).copied().unwrap_or(0);
    (start <= end).then(|| (base + start * 256, base + end * 256 + 255))
}

const SR_ERRORS: [(u32, &str); 9] = [
    (17, "wrperr"),
    (18, "pgserr"),
    (19, "strberr"),
    (21, "incerr"),
    (22, "operr"),
    (23, "rdperr"),
    (24, "rdserr"),
    (25, "sneccerr"),
    (26, "dbeccerr"),
];

pub(super) fn write_sr(name: &str, sr: u32, w: &mut impl Write) -> fmt::Result {
    write!(
        w,
        "{name} {sr:08x} bsy {} wbne {} qw {} crc_busy {} eop {} crcend {} errors",
        bit(sr, 0),
        bit(sr, 1),
        bit(sr, 2),
        bit(sr, 3),
        bit(sr, 16),
        bit(sr, 27)
    )?;
    let mut any = false;
    for (n, e) in SR_ERRORS {
        if bit(sr, n) == 1 {
            write!(w, " {e}")?;
            any = true;
        }
    }
    if !any {
        w.write_str(" -")?;
    }
    w.write_char('\n')
}

/// One FLASH_CRx line.
pub(super) fn write_cr(n: usize, cr: u32, w: &mut impl Write) -> fmt::Result {
    writeln!(
        w,
        "cr{n} {cr:08x} lock {} pg {} ser {} ber {} psize {} fw {} start {} snb {}",
        bit(cr, 0),
        bit(cr, 1),
        bit(cr, 2),
        bit(cr, 3),
        (cr >> 4) & 3,
        bit(cr, 6),
        bit(cr, 7),
        (cr >> 8) & 7
    )
}

fn write_area(name: &str, what: &str, reg: u32, bank: usize, w: &mut impl Write) -> fmt::Result {
    write!(w, "{name} {reg:08x} {what} ")?;
    match area(reg, bank) {
        None => w.write_str("none")?,
        Some((a, b)) => write!(w, "{a:08x}..{b:08x}")?,
    }
    writeln!(w, " dme {}", bit(reg, 31))
}

/// `flash`'s register lines.
pub fn write_flash_regs(r: &FlashRegs, w: &mut impl Write) -> fmt::Result {
    let (acr, optcr, o, boot) = (r.acr, r.optcr, r.optsr_cur, r.boot_cur);
    writeln!(
        w,
        "acr {acr:08x} latency {} wrhighfreq {}",
        acr & 0xf,
        (acr >> 4) & 3
    )?;
    writeln!(
        w,
        "optcr {optcr:08x} optlock {} optstart {} mer {} swap_bank {}",
        bit(optcr, 0),
        bit(optcr, 1),
        bit(optcr, 4),
        bit(optcr, 31)
    )?;
    let bor = match (o >> 2) & 3 {
        0 => "off",
        1 => "1",
        2 => "2",
        _ => "3",
    };
    let iwdg = if bit(o, 4) == 1 { "sw" } else { "hw" };
    writeln!(
        w,
        "optsr_cur {o:08x} rdp {} swap_bank {} iwdg {iwdg} bor {bor} nrst_stop {} \
         nrst_stby {} fz_iwdg_stop {} fz_iwdg_sdby {} st_ram {} security {} io_hslv {} \
         optchangeerr {} opt_busy {}",
        rdp_level(o),
        bit(o, 31),
        bit(o, 6),
        bit(o, 7),
        bit(o, 16),
        bit(o, 17),
        (o >> 19) & 3,
        bit(o, 21),
        bit(o, 29),
        bit(o, 30),
        bit(o, 0)
    )?;
    writeln!(
        w,
        "boot_cur {boot:08x} add0 {:08x} add1 {:08x}",
        (boot & 0xffff) << 16,
        boot & 0xffff_0000
    )?;
    for (i, (b, [sr, prar, scar])) in r.banks.iter().zip(NAMES).enumerate() {
        let n = i + 1;
        write_cr(n, b.cr, w)?;
        write_sr(sr, b.sr, w)?;
        write!(w, "wpsn_cur{n}r {:08x} protected", b.wpsn_cur)?;
        let p = protected_sectors(b.wpsn_cur);
        if p == 0 {
            w.write_str(" -")?;
        }
        for s in (0..8).filter(|s| bit(u32::from(p), *s) == 1) {
            write!(w, " {s}")?;
        }
        w.write_char('\n')?;
        write_area(prar, "pcrop", b.prar_cur, i, w)?;
        write_area(scar, "secure", b.scar_cur, i, w)?;
    }
    Ok(())
}

/// `flash`'s peek lines: the words, erased or not, then SR1 and SR2 after
/// the reads.
pub fn write_flash_peek(p: &FlashPeek, w: &mut impl Write) -> fmt::Result {
    write!(w, "peek {PEEK_AT:08x}")?;
    p.words.iter().try_for_each(|x| write!(w, " {x:08x}"))?;
    let erased = p.words.iter().all(|&x| x == u32::MAX);
    writeln!(w, " {}", if erased { "erased" } else { "written" })?;
    let [sr1, sr2] = p.sr_after;
    write_sr("sr1_after", sr1, w)?;
    write_sr("sr2_after", sr2, w)
}
