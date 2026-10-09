//! The last reset's cause, from RCC_RSR.

use chimera_core::reset::ResetCause;

/// RCC_RSR after each kind of reset (RM0433): internal resets drive NRST,
/// so PINRSTF rides along; a power-on sets BORRSTF and PINRSTF too.
#[test]
fn reset_cause_takes_the_most_specific_flag() {
    const PIN: u32 = 1 << 22;
    const BOR: u32 = 1 << 21;
    const POR: u32 = 1 << 23;
    const SFT: u32 = 1 << 24;
    const IWDG1: u32 = 1 << 26;
    assert_eq!(ResetCause::from_rsr(IWDG1 | PIN), ResetCause::Watchdog);
    assert_eq!(ResetCause::from_rsr(SFT | PIN), ResetCause::Software);
    assert_eq!(ResetCause::from_rsr(POR | BOR | PIN), ResetCause::PowerOn);
    assert_eq!(ResetCause::from_rsr(BOR | PIN), ResetCause::Brownout);
    assert_eq!(ResetCause::from_rsr(PIN), ResetCause::Pin);
    assert_eq!(ResetCause::from_rsr(0), ResetCause::Unknown);
}

/// Behind bootloader 1.09 RCC_RSR reads 0: the backup domain tells.
#[test]
fn an_empty_rsr_is_told_from_the_backup_domain() {
    use chimera_core::reset::SOFT_INTENT;
    const POR: u32 = 1 << 23;
    const SFT: u32 = 1 << 24;
    let infer = ResetCause::infer;
    assert_eq!(infer(0, 0, 0), ResetCause::PowerOn);
    assert_eq!(infer(0, 0, SOFT_INTENT), ResetCause::PowerOn);
    assert_eq!(infer(0, 3, SOFT_INTENT), ResetCause::Software);
    assert_eq!(infer(0, 3, 0), ResetCause::Watchdog);
    assert_eq!(infer(0, 3, 0xdead_beef), ResetCause::Watchdog);
    // A flag the bootloader left is read as it is.
    assert_eq!(infer(POR, 3, 0), ResetCause::PowerOn);
    assert_eq!(infer(SFT, 0, 0), ResetCause::Software);
}
