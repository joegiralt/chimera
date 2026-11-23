// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_core::console::{
    BankRegs, FlashPeek, FlashRegs, area, protected_sectors, rdp_level, write_flash_peek,
    write_flash_regs,
};

/// A fresh H753 at RDP 0: no WRP, no PCROP, no secure area, both banks locked.
fn factory() -> FlashRegs {
    let bank = BankRegs {
        cr: 0x0000_0031,
        sr: 0,
        prar_cur: 0x0000_00ff,
        scar_cur: 0x0000_00ff,
        wpsn_cur: 0x0000_00ff,
    };
    FlashRegs {
        acr: 0x0000_0037,
        optcr: 0x0000_0001,
        optsr_cur: 0x1bc6_aaf0,
        boot_cur: 0x1ff0_0800,
        banks: [bank; 2],
    }
}

fn regs(r: &FlashRegs) -> String {
    let mut s = String::new();
    write_flash_regs(r, &mut s).unwrap();
    s
}

fn line<'a>(s: &'a str, key: &str) -> &'a str {
    s.lines()
        .find(|l| l.split(' ').next() == Some(key))
        .unwrap_or_else(|| panic!("no {key} in {s}"))
}

#[test]
fn rdp_levels() {
    assert_eq!(rdp_level(0x0000_aa00), 0);
    assert_eq!(rdp_level(0x0000_cc00), 2);
    assert_eq!(rdp_level(0x0000_bb00), 1);
    assert_eq!(rdp_level(0x0000_0000), 1);
}

#[test]
fn wrps_zero_is_protected() {
    assert_eq!(protected_sectors(0xff), 0);
    assert_eq!(protected_sectors(0xfe), 0b0000_0001);
    assert_eq!(protected_sectors(0x0f), 0xf0);
    assert_eq!(
        protected_sectors(0xffff_ff00),
        0xff,
        "bits above 7 are reserved"
    );
}

#[test]
fn areas_in_256_byte_blocks_per_bank() {
    assert_eq!(area(0x0000_00ff, 0), None, "start > end: no area");
    assert_eq!(area(0x0000_0000, 0), Some((0x0800_0000, 0x0800_00ff)));
    assert_eq!(area(0x0003_0002, 1), Some((0x0810_0200, 0x0810_03ff)));
    assert_eq!(
        area(0x8fff_0000, 1),
        Some((0x0810_0000, 0x081f_ffff)),
        "DMEP ignored"
    );
}

#[test]
fn factory_state_decodes() {
    let s = regs(&factory());
    assert_eq!(line(&s, "acr"), "acr 00000037 latency 7 wrhighfreq 3");
    assert_eq!(
        line(&s, "optcr"),
        "optcr 00000001 optlock 1 optstart 0 mer 0 swap_bank 0"
    );
    assert_eq!(
        line(&s, "optsr_cur"),
        "optsr_cur 1bc6aaf0 rdp 0 swap_bank 0 iwdg sw bor off nrst_stop 1 nrst_stby 1 \
         fz_iwdg_stop 0 fz_iwdg_sdby 1 st_ram 0 security 0 io_hslv 0 optchangeerr 0 opt_busy 0"
    );
    assert_eq!(
        line(&s, "boot_cur"),
        "boot_cur 1ff00800 add0 08000000 add1 1ff00000"
    );
    for n in ["1", "2"] {
        assert_eq!(
            line(&s, &format!("cr{n}")),
            format!("cr{n} 00000031 lock 1 pg 0 ser 0 ber 0 psize 3 fw 0 start 0 snb 0")
        );
        assert_eq!(
            line(&s, &format!("sr{n}")),
            format!("sr{n} 00000000 bsy 0 wbne 0 qw 0 crc_busy 0 eop 0 crcend 0 errors -")
        );
        assert_eq!(
            line(&s, &format!("wpsn_cur{n}r")),
            format!("wpsn_cur{n}r 000000ff protected -")
        );
        assert_eq!(
            line(&s, &format!("prar_cur{n}")),
            format!("prar_cur{n} 000000ff pcrop none dme 0")
        );
        assert_eq!(
            line(&s, &format!("scar_cur{n}")),
            format!("scar_cur{n} 000000ff secure none dme 0")
        );
    }
    assert_eq!(s.lines().count(), 14);
}

#[test]
fn bank2_protected_swapped_and_stuck_erasing() {
    let mut r = factory();
    r.optsr_cur = 0x8000_cc0c | 1 << 30;
    r.optcr = 0x8000_0000;
    r.banks[1] = BankRegs {
        cr: 0x0000_02b4,
        sr: 1 | 1 << 2 | 1 << 17 | 1 << 22 | 1 << 26,
        prar_cur: 0x8001_0000,
        scar_cur: 0x0000_0000,
        wpsn_cur: 0x0000_00f0,
    };
    let s = regs(&r);
    assert!(
        line(&s, "optsr_cur").contains(" rdp 2 swap_bank 1 iwdg hw bor 3 "),
        "{s}"
    );
    assert!(
        line(&s, "optsr_cur").ends_with("optchangeerr 1 opt_busy 0"),
        "{s}"
    );
    assert!(line(&s, "optcr").ends_with("optlock 0 optstart 0 mer 0 swap_bank 1"));
    assert_eq!(
        line(&s, "cr2"),
        "cr2 000002b4 lock 0 pg 0 ser 1 ber 0 psize 3 fw 0 start 1 snb 2"
    );
    assert_eq!(
        line(&s, "sr2"),
        "sr2 04420005 bsy 1 wbne 0 qw 1 crc_busy 0 eop 0 crcend 0 errors wrperr operr dbeccerr"
    );
    assert_eq!(
        line(&s, "wpsn_cur2r"),
        "wpsn_cur2r 000000f0 protected 0 1 2 3"
    );
    assert_eq!(
        line(&s, "prar_cur2"),
        "prar_cur2 80010000 pcrop 08100000..081001ff dme 1"
    );
    assert_eq!(
        line(&s, "scar_cur2"),
        "scar_cur2 00000000 secure 08100000..081000ff dme 0"
    );
    assert_eq!(
        line(&s, "sr1"),
        line(&regs(&factory()), "sr1"),
        "bank 1 untouched"
    );
}

#[test]
fn peek_says_erased_or_written() {
    let mut p = FlashPeek {
        words: [u32::MAX; 8],
        sr_after: [0, 1 << 25],
    };
    let ff = " ffffffff".repeat(8);
    let mut s = String::new();
    write_flash_peek(&p, &mut s).unwrap();
    assert_eq!(
        s,
        format!(
            "peek 08100000{ff} erased\n\
             sr1_after 00000000 bsy 0 wbne 0 qw 0 crc_busy 0 eop 0 crcend 0 errors -\n\
             sr2_after 02000000 bsy 0 wbne 0 qw 0 crc_busy 0 eop 0 crcend 0 errors sneccerr\n"
        )
    );
    p.words[7] = 0xffff_fffe;
    let mut s = String::new();
    write_flash_peek(&p, &mut s).unwrap();
    assert!(s.starts_with(&format!(
        "peek 08100000{} fffffffe written\n",
        " ffffffff".repeat(7)
    )));
}
