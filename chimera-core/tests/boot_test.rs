use chimera_core::boot::{
    BootAction, BootSeen, BootStage, DFU_MAGIC, FROM_CONSOLE, FROM_MENU, ROM_DFU_BASE,
    USB_RETRY_MS, USB_TRIES, UsbOff, UsbRegs, UsbRetry, UsbState, UsbStep, after_reset, wait_until,
};
use chimera_core::reset::ResetCause;
use chimera_hal::Ms;

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
        last_stage: BootStage::Audio.code(),
        last_usb: UsbState::Off {
            why: UsbOff::CoreReset,
            tries: 3,
        }
        .code(),
        last_usb_step: UsbStep::Connect.code(3),
        last_usb_regs: UsbRegs::from_words([0xC0000, 0x10000, 2, 0x0400_0020, 0x100, 0x0100_0006]),
    };
    assert_eq!(
        seen.to_string(),
        "boot marker=44465521 readback=00000000 action=Synth rsr=00e60000 dbp=0 boots=7 \
         from=console jump_rsr=01400000 last_stage=audio last_usb=off(csrst, try 3) last_usb_step=connect(try 3)\n\
         last_usb_regs gotgctl=000c0000 gccfg=00010000 dctl=00000002 gintsts=04000020 \
         dsts=00000100 pwr_cr3=01000006"
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

#[test]
fn every_stage_reads_back_from_its_code() {
    for s in BootStage::ALL {
        assert_eq!(BootStage::from_code(s.code()), Some(s));
    }
    assert_eq!(BootStage::from_code(0), None);
    let seen = |last_stage| BootSeen {
        marker: 0,
        readback: 0,
        action: BootAction::Synth,
        rsr: 0,
        dbp: false,
        boots: 1,
        from: 0,
        jump_rsr: 0,
        last_stage,
        last_usb: 0,
        last_usb_step: 0,
        last_usb_regs: UsbRegs::default(),
    };
    assert!(
        seen(0)
            .to_string()
            .contains(" last_stage=none last_usb=none last_usb_step=none\n")
    );
    assert!(seen(99).to_string().contains(" last_stage=00000063 "));
    assert!(seen(5).to_string().contains(" last_stage=running "));
}

#[test]
fn every_usb_state_reads_back_from_its_code() {
    for tries in [1, 2, USB_TRIES] {
        let mut all = vec![UsbState::On { tries }];
        for why in [
            UsbOff::Hsi48,
            UsbOff::Usb33,
            UsbOff::AhbIdle,
            UsbOff::CoreReset,
        ] {
            all.push(UsbState::Off { why, tries });
        }
        for s in all {
            assert_ne!(s.code(), 0, "0 is not reached");
            assert_eq!(UsbState::from_code(s.code()), Some(s));
        }
    }
    assert_eq!(UsbState::from_code(0), None);
    assert_eq!(UsbState::On { tries: 2 }.to_string(), "on(try 2)");
}

#[test]
fn usb_retries_every_500_ms_ten_times_then_gives_up() {
    let mut r = UsbRetry::new();
    assert!(r.due(Ms(7)), "the first try is at once");
    assert_eq!(r.tried(Ms(7)), 1);
    assert!(!r.due(Ms(7 + USB_RETRY_MS - 1)));
    let mut now = 7;
    for n in 2..=USB_TRIES {
        now += USB_RETRY_MS;
        assert!(r.due(Ms(now)), "try {n}");
        assert_eq!(r.tried(Ms(now)), n);
    }
    assert!(!r.due(Ms(now + 10 * USB_RETRY_MS)), "no try after the last");
    let wrap = {
        let mut w = UsbRetry::new();
        w.tried(Ms(u32::MAX - 100));
        w
    };
    assert!(wrap.due(Ms(USB_RETRY_MS)), "the clock wraps");
    let off = |tries| UsbState::Off {
        why: UsbOff::CoreReset,
        tries,
    };
    assert!(!off(USB_TRIES - 1).gave_up());
    assert!(off(USB_TRIES).gave_up());
    assert!(!UsbState::On { tries: USB_TRIES }.gave_up());
}

#[test]
fn a_bounded_wait_gives_up_after_its_limit() {
    use std::cell::Cell;
    let t = Cell::new(0u32);
    let tick = || {
        t.set(t.get().wrapping_add(10));
        t.get()
    };
    assert!(!wait_until(100, tick, || false));
    assert!(t.get() <= 130, "stopped near the limit: {}", t.get());
    t.set(u32::MAX - 50); // the counter wraps mid-wait
    assert!(!wait_until(100, tick, || false));
    let polls = Cell::new(0);
    let ready_on_third = || {
        polls.set(polls.get() + 1);
        polls.get() >= 3
    };
    assert!(wait_until(1_000, tick, ready_on_third));
    assert!(wait_until(0, || 0, || true), "ready at once needs no time");
}

#[test]
fn the_usb_toast_names_the_field() {
    for why in [
        UsbOff::Hsi48,
        UsbOff::Usb33,
        UsbOff::AhbIdle,
        UsbOff::CoreReset,
    ] {
        assert_eq!(why.toast(), format!("USB OFF: {}", why.label()));
    }
}

#[test]
fn every_usb_step_reads_back_from_its_code() {
    for s in UsbStep::ALL {
        for tries in [1, USB_TRIES] {
            assert_eq!(UsbStep::from_code(s.code(tries)), Some((s, tries)));
        }
    }
    assert_eq!(UsbStep::from_code(0), None);
    let r = UsbRegs::from_words([1, 2, 3, 4, 5, 6]);
    assert_eq!(UsbRegs::from_words(r.words()), r);
}
