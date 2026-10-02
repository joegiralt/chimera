use chimera_core::boot::{
    BootAction, BootSeen, DFU_MAGIC, FROM_CONSOLE, FROM_MENU, ROM_DFU_BASE, after_reset,
};
use chimera_core::reset::ResetCause;

const SOFT: ResetCause = ResetCause::Software;

#[test]
fn only_the_magic_enters_dfu() {
    assert_eq!(after_reset(DFU_MAGIC, 0, SOFT), BootAction::RomDfu);
    for m in [
        0,
        u32::MAX,
        DFU_MAGIC ^ 1,
        DFU_MAGIC.swap_bytes(),
        DFU_MAGIC.rotate_left(8),
    ] {
        assert_eq!(after_reset(m, 0, SOFT), BootAction::Synth, "{m:#010x}");
    }
}

#[test]
fn a_random_marker_boots_the_synth() {
    let mut x = 0x5eed_cafe_u32; // xorshift32
    for _ in 0..1_000_000 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        if x != DFU_MAGIC {
            assert_eq!(after_reset(x, 0, SOFT), BootAction::Synth, "{x:#010x}");
        }
    }
}

#[test]
fn the_rom_loader_is_an2606s() {
    assert_eq!(ROM_DFU_BASE, 0x1FF0_9800); // STM32H74x/75x system memory
}

#[test]
fn a_marker_that_would_not_clear_boots_the_synth() {
    for readback in [DFU_MAGIC, 1, u32::MAX] {
        assert_eq!(
            after_reset(DFU_MAGIC, readback, SOFT),
            BootAction::Synth,
            "{readback:#010x}"
        );
    }
}

/// A marker that outlived a power-off (VBAT) never traps a cold boot.
#[test]
fn only_a_software_reset_honours_the_marker() {
    for cause in [
        ResetCause::PowerOn,
        ResetCause::Brownout,
        ResetCause::Pin,
        ResetCause::Watchdog,
        ResetCause::Unknown,
    ] {
        assert_eq!(
            after_reset(DFU_MAGIC, 0, cause),
            BootAction::Synth,
            "{cause:?}"
        );
    }
}

/// RCC_RSR as the shell reads it: a power-on's flags never enter DFU, a
/// `sys_reset`'s do, and a watchdog's outrank a software reset's.
#[test]
fn the_reset_flags_decide() {
    const PIN: u32 = 1 << 22;
    const BOR: u32 = 1 << 21;
    const POR: u32 = 1 << 23;
    const SFT: u32 = 1 << 24;
    const IWDG1: u32 = 1 << 26;
    let act = |rsr| after_reset(DFU_MAGIC, 0, ResetCause::from_rsr(rsr));
    assert_eq!(act(POR | BOR | PIN), BootAction::Synth);
    assert_eq!(act(BOR | PIN), BootAction::Synth);
    assert_eq!(act(PIN), BootAction::Synth);
    assert_eq!(act(0), BootAction::Synth);
    assert_eq!(act(IWDG1 | SFT | PIN), BootAction::Synth);
    assert_eq!(act(SFT | PIN), BootAction::RomDfu);
}

#[test]
fn the_boot_line() {
    let seen = BootSeen {
        marker: DFU_MAGIC,
        readback: 0,
        action: BootAction::Synth,
        rsr: 0x00e6_0000,
        dbp: false,
        boots: 7,
        from: FROM_CONSOLE,
        jump_rsr: 0x0140_0000,
    };
    assert_eq!(
        seen.to_string(),
        "boot marker=44465521 readback=00000000 action=Synth rsr=00e60000 dbp=0 boots=7 \
         from=console jump_rsr=01400000"
    );
    let words = |from| BootSeen { from, ..seen }.to_string();
    assert!(words(FROM_MENU).contains(" from=menu "));
    assert!(words(0).contains(" from=none "));
    assert!(words(0x1234).contains(" from=00001234 "));
    let jumped = BootSeen {
        action: BootAction::RomDfu,
        dbp: true,
        ..seen
    }
    .to_string();
    assert!(jumped.contains(" action=RomDfu ") && jumped.contains(" dbp=1 "));
}
