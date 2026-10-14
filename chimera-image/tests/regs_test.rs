use chimera_image::regs::*;
use chimera_image::{Bank2Sector, FlashFail};

const SECTORS: [Bank2Sector; 4] = [
    Bank2Sector::S0,
    Bank2Sector::S1,
    Bank2Sector::S2,
    Bank2Sector::S3,
];
const BER: u32 = 1 << 3;
const SNB: u32 = 7 << 8;
const PSIZE: u32 = 3 << 4;

#[test]
fn erase_cr_sets_sector() {
    let v = erase_cr(0, Bank2Sector::S3);
    assert_eq!((v & SNB) >> 8, 3, "SNB");
    assert_ne!(v & CR_SER, 0, "SER");
    assert_eq!((v & PSIZE) >> 4, 2, "PSIZE x32");
    assert_eq!(v & BER, 0, "no BER");
    assert_eq!(v & CR_START, 0, "no START");
    assert_eq!(v, 0x0000_0324);
}

#[test]
fn erase_cr_never_ber() {
    for cr in [0, u32::MAX, 0x31, 0x0000_0030, 0x0001_07ca] {
        for s in SECTORS {
            let v = erase_cr(cr, s);
            assert_eq!(v & BER, 0, "BER from {cr:08x} {s:?}");
            assert_eq!(v & (CR_START | CR_PG), 0, "{cr:08x} {s:?}");
            assert_eq!((v & SNB) >> 8, u32::from(s.index()), "{cr:08x} {s:?}");
            assert_eq!((v & PSIZE) >> 4, PSIZE_X32);
            assert_ne!(v & CR_SER, 0);
        }
    }
}

#[test]
fn erase_cr_keeps_the_rest_as_the_spike_did() {
    // The unit's CR2 after the 2026-10-07 ROM DFU, unlocked: PSIZE 3.
    assert_eq!(erase_cr(0x0000_0030, Bank2Sector::S0), 0x0000_0024);
    // PG, BER, FW, START and SNB 7 cleared; interrupt enables kept.
    assert_eq!(erase_cr(0x0001_07ca, Bank2Sector::S0), 0x0001_0024);
    assert_eq!(erase_cr(0x0001_07ca, Bank2Sector::S2), 0x0001_0224);
}

#[test]
fn program_cr_sets_pg_and_x32_only() {
    assert_eq!(program_cr(0x0000_0030), 0x0000_0022);
    assert_eq!(program_cr(0x0001_07ec), 0x0001_0022);
    for cr in [0, u32::MAX, 0x31] {
        let v = program_cr(cr);
        assert_eq!(v & (BER | CR_SER | CR_START | SNB), 0, "{cr:08x}");
        assert_ne!(v & CR_PG, 0);
    }
}

#[test]
fn relock_sets_lock_and_ends_the_op_only_when_idle() {
    assert_eq!(relock_cr(0x0000_0324, 0), 0x0000_0321);
    assert_eq!(relock_cr(0x0000_03a4, 0), 0x0000_0321);
    assert_eq!(relock_cr(0x0000_0022, 0), 0x0000_0021);
    // Still running: SER or PG stays, only LOCK is set.
    assert_eq!(relock_cr(0x0000_03a4, SR_QW), 0x0000_03a5);
    assert_eq!(relock_cr(0x0000_0022, SR_BSY), 0x0000_0023);
    for cr in [0, u32::MAX, 0x31] {
        for sr in [0, SR_BSY, u32::MAX] {
            let v = relock_cr(cr, sr);
            assert_ne!(v & CR_LOCK, 0);
            assert_eq!(v & BER, cr & BER, "relock never sets BER");
        }
    }
}

#[test]
fn ccr_clears_every_error_and_eop() {
    assert_eq!(CCR_CLEAR, 0x07ef_0000);
    assert_eq!(SR_ERRORS | (1 << 16), CCR_CLEAR);
    assert_ne!(SR_ERRORS & SR_DBECCERR, 0);
    assert_eq!(SR_DBECCERR, 1 << 26);
}

#[test]
fn settle_puts_errors_before_the_deadline() {
    assert_eq!(settle(0, false), None);
    assert_eq!(settle(0, true), None, "done just as the deadline passed");
    assert_eq!(settle(1 << 2, false), None, "QW: still going");
    assert_eq!(settle(1 << 0, true), Some(FlashFail::Timeout));
    assert_eq!(settle(1 << 1, true), Some(FlashFail::Timeout), "WBNE");
    assert_eq!(
        settle(1 << 17, false),
        Some(FlashFail::Errors(1 << 17)),
        "WRPERR"
    );
    assert_eq!(
        settle((1 << 0) | (1 << 22) | (1 << 16), true),
        Some(FlashFail::Errors(1 << 22)),
        "the error bits only"
    );
    assert_eq!(settle(1 << 16, false), None, "EOP is no error");
}

#[test]
fn busy_is_bsy_qw_or_wbne() {
    assert!(!busy(0));
    assert!(!busy(!(SR_BSY | SR_QW | SR_WBNE)));
    for b in [SR_BSY, SR_QW, SR_WBNE] {
        assert!(busy(b));
    }
}

#[test]
fn the_erase_deadline_fits_one_counter_wrap() {
    assert_eq!(ms_cycles(ERASE_TIMEOUT_MS, 480_000_000), 2_880_000_000);
    assert_eq!(ms_cycles(WORD_TIMEOUT_MS, 480_000_000), 2_400_000);
    assert_eq!(ERASE_TIMEOUT_MS, 6_000);
    assert_eq!(WORD_TIMEOUT_MS, 5);
}

#[test]
fn the_keys_are_rm0433s() {
    assert_eq!(KEYS, [0x4567_0123, 0xcdef_89ab]);
}
