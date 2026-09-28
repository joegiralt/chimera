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
