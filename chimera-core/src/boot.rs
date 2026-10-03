//! What a reset leads to: the synth, or ST's ROM DFU loader (USB console
//! spec § Enter DFU from the firmware). Pure, no I/O: the chip shell reads
//! and clears the marker, then acts on `after_reset`.

use core::fmt;

use chimera_hal::Ms;

use crate::reset::ResetCause;

/// "DFU!". 0 and every other value boot the synth.
pub const DFU_MAGIC: u32 = 0x4446_5521;

/// ST AN2606, STM32H74x/75x: the system memory bootloader's vector table.
pub const ROM_DFU_BASE: u32 = 0x1FF0_9800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAction {
    Synth,
    RomDfu,
}

/// `marker`, read after a reset; `readback`, the marker read again after
/// the shell cleared it; `cause`, RCC_RSR's reset. Only `DFU_MAGIC` that
/// cleared to 0, after a software reset, enters the ROM loader. A
/// power-on, brown-out, pin or watchdog reset always boots the synth, so a
/// marker that outlived a power-off (a backup domain on VBAT) can't trap a
/// cold boot in DFU. A read-back that isn't 0 is stuck (DBP never set, or
/// the write didn't land) and boots the synth too.
pub const fn after_reset(marker: u32, readback: u32, cause: ResetCause) -> BootAction {
    if marker == DFU_MAGIC && readback == 0 && matches!(cause, ResetCause::Software) {
        BootAction::RomDfu
    } else {
        BootAction::Synth
    }
}

/// Who wrote the last marker, kept in RTC_BKP2R: "MENU" and "CONS".
pub const FROM_MENU: u32 = u32::from_be_bytes(*b"MENU");
pub const FROM_CONSOLE: u32 = u32::from_be_bytes(*b"CONS");

/// How far a start-up got, kept in RTC_BKP4R as it goes: after a reset,
/// the last one written says where the boot before it stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum BootStage {
    /// The top of `main`.
    Entry = 1,
    /// SYSTEM and the project read from the card.
    Card = 2,
    /// The audio and the controls tick started.
    Audio = 3,
    /// PART 1 drawn and the watchdog armed, its kicks live.
    Armed = 4,
    /// USB up or skipped: the UI loop runs.
    Running = 5,
}

impl BootStage {
    pub const ALL: [BootStage; 5] = [
        BootStage::Entry,
        BootStage::Card,
        BootStage::Audio,
        BootStage::Armed,
        BootStage::Running,
    ];

    pub const fn code(self) -> u32 {
        self as u32
    }

    pub fn from_code(code: u32) -> Option<BootStage> {
        BootStage::ALL.into_iter().find(|s| s.code() == code)
    }

    pub const fn label(self) -> &'static str {
        match self {
            BootStage::Entry => "entry",
            BootStage::Card => "card",
            BootStage::Audio => "audio",
            BootStage::Armed => "armed",
            BootStage::Running => "running",
        }
    }
}

/// Which USB precondition never came true, so the boot left USB off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbOff {
    /// RCC_CR.HSI48RDY: the USB kernel clock.
    Hsi48,
    /// OTG GRSTCTL.AHBIDL after the RCC reset.
    AhbIdle,
    /// OTG GRSTCTL.CSRST, the core soft reset, never cleared.
    CoreReset,
}

/// How the last boot's USB step ended, kept in RTC_BKP5R, with the try
/// it ended on (1-based, `UsbRetry`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbState {
    On { tries: u8 },
    Off { why: UsbOff, tries: u8 },
}

impl UsbOff {
    /// The register field, as the boot toast and line name it.
    pub const fn label(self) -> &'static str {
        match self {
            UsbOff::Hsi48 => "hsi48",
            UsbOff::AhbIdle => "ahbidl",
            UsbOff::CoreReset => "csrst",
        }
    }

    /// The toast after PART 1 comes up.
    pub const fn toast(self) -> &'static str {
        match self {
            UsbOff::Hsi48 => "USB OFF: hsi48",
            UsbOff::AhbIdle => "USB OFF: ahbidl",
            UsbOff::CoreReset => "USB OFF: csrst",
        }
    }
}

impl UsbState {
    /// The state in bits 8.., the try in 0..8; never 0, "not reached".
    pub const fn code(self) -> u32 {
        let (state, tries) = match self {
            UsbState::On { tries } => (1, tries),
            UsbState::Off { why, tries } => (
                match why {
                    UsbOff::Hsi48 => 2,
                    UsbOff::AhbIdle => 3,
                    UsbOff::CoreReset => 4,
                },
                tries,
            ),
        };
        state << 8 | tries as u32
    }

    pub fn from_code(code: u32) -> Option<UsbState> {
        let tries = u8::try_from(code & 0xFF).ok()?;
        let why = match code >> 8 {
            1 => return Some(UsbState::On { tries }),
            2 => UsbOff::Hsi48,
            3 => UsbOff::AhbIdle,
            4 => UsbOff::CoreReset,
            _ => return None,
        };
        Some(UsbState::Off { why, tries })
    }

    /// The last try failed: USB stays off this boot.
    pub const fn gave_up(self) -> bool {
        matches!(self, UsbState::Off { tries, .. } if tries >= USB_TRIES)
    }
}

impl fmt::Display for UsbState {
    /// `on(try 2)`, `off(csrst, try 10)`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UsbState::On { tries } => write!(f, "on(try {tries})"),
            UsbState::Off { why, tries } => write!(f, "off({}, try {tries})", why.label()),
        }
    }
}

/// USB tries per boot: one at start-up, then one every `USB_RETRY_MS`
/// from the UI loop. The core reset can fail for a while after a cold
/// power-up; the synth plays between tries.
pub const USB_TRIES: u8 = 10;
pub const USB_RETRY_MS: u32 = 500;

/// When the next USB try is due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbRetry {
    tries: u8,
    last: Ms,
}

impl Default for UsbRetry {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbRetry {
    pub const fn new() -> Self {
        UsbRetry {
            tries: 0,
            last: Ms(0),
        }
    }

    /// The first at once, then `USB_RETRY_MS` after the last, `USB_TRIES`
    /// in all.
    pub fn due(&self, now: Ms) -> bool {
        self.tries == 0 || (self.tries < USB_TRIES && now.since(self.last) >= USB_RETRY_MS)
    }

    /// A try made at `now`: its number, from 1.
    pub fn tried(&mut self, now: Ms) -> u8 {
        self.tries = self.tries.saturating_add(1);
        self.last = now;
        self.tries
    }
}

/// Polls `ready` until it is true or `limit` has passed on `now`'s clock
/// (a wrapping counter, DWT cycles on the chip). True if it came ready.
pub fn wait_until(
    limit: u32,
    mut now: impl FnMut() -> u32,
    mut ready: impl FnMut() -> bool,
) -> bool {
    let start = now();
    loop {
        if ready() {
            return true;
        }
        if now().wrapping_sub(start) > limit {
            return ready();
        }
    }
}

/// How far the USB bring-up got, kept in RTC_BKP6R as it goes, with the
/// try it is on: the step in bits 8.., the try in 0..8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsbStep {
    /// `preflight` started.
    Preflight = 1,
    /// `init` done: the bus and the class built.
    Init = 2,
    /// `connect` called: the core enable and the D+ pull-up.
    Connect = 3,
    /// `connect` returned: on the bus, nothing from the host yet.
    Up = 4,
    /// Start-of-frame packets arriving (OTG DSTS.FNSOF moving): the host
    /// reset the bus and drives it.
    Sof = 5,
    /// The host gave the device an address.
    Addressed = 6,
    /// The host configured it.
    Configured = 7,
    /// The bus went idle (usb-device `Suspend`).
    Suspended = 8,
}

impl UsbStep {
    pub const ALL: [UsbStep; 8] = [
        UsbStep::Preflight,
        UsbStep::Init,
        UsbStep::Connect,
        UsbStep::Up,
        UsbStep::Sof,
        UsbStep::Addressed,
        UsbStep::Configured,
        UsbStep::Suspended,
    ];

    pub const fn code(self, tries: u8) -> u32 {
        (self as u32) << 8 | tries as u32
    }

    /// The step and its try, from `code`.
    pub fn from_code(code: u32) -> Option<(UsbStep, u8)> {
        let tries = u8::try_from(code & 0xFF).ok()?;
        let step = UsbStep::ALL.into_iter().find(|s| *s as u32 == code >> 8)?;
        Some((step, tries))
    }

    pub const fn label(self) -> &'static str {
        match self {
            UsbStep::Preflight => "preflight",
            UsbStep::Init => "init",
            UsbStep::Connect => "connect",
            UsbStep::Up => "up",
            UsbStep::Sof => "sof",
            UsbStep::Addressed => "addressed",
            UsbStep::Configured => "configured",
            UsbStep::Suspended => "suspended",
        }
    }
}

/// The OTG and PWR registers a cold and a warm bring-up are compared on,
/// snapshotted to RTC_BKP7R..12R while the port runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsbRegs {
    pub gotgctl: u32,
    pub gccfg: u32,
    pub dctl: u32,
    pub gintsts: u32,
    pub dsts: u32,
    pub pwr_cr3: u32,
}

impl UsbRegs {
    pub const WORDS: usize = 6;

    pub const fn words(self) -> [u32; UsbRegs::WORDS] {
        [
            self.gotgctl,
            self.gccfg,
            self.dctl,
            self.gintsts,
            self.dsts,
            self.pwr_cr3,
        ]
    }

    pub const fn from_words(w: [u32; UsbRegs::WORDS]) -> UsbRegs {
        UsbRegs {
            gotgctl: w[0],
            gccfg: w[1],
            dctl: w[2],
            gintsts: w[3],
            dsts: w[4],
            pwr_cr3: w[5],
        }
    }
}

impl fmt::Display for UsbRegs {
    /// `gotgctl=… gccfg=… dctl=… gintsts=… dsts=… pwr_cr3=…`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "gotgctl={:08x} gccfg={:08x} dctl={:08x} gintsts={:08x} dsts={:08x} pwr_cr3={:08x}",
            self.gotgctl, self.gccfg, self.dctl, self.gintsts, self.dsts, self.pwr_cr3
        )
    }
}

/// What the shell saw at the top of `main`, for `status`'s `boot` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSeen {
    /// RTC_BKP0R on entry.
    pub marker: u32,
    /// RTC_BKP0R after the clear.
    pub readback: u32,
    pub action: BootAction,
    /// RCC_RSR on entry, before RMVF.
    pub rsr: u32,
    /// PWR_CR1.DBP on entry, before the shell set it.
    pub dbp: bool,
    /// RTC_BKP1R, counted up every boot: it counts on through a power-off
    /// only if the backup domain kept power.
    pub boots: u32,
    /// RTC_BKP2R: `FROM_MENU`, `FROM_CONSOLE`, or 0 if nothing wrote one.
    pub from: u32,
    /// RTC_BKP3R: RCC_RSR at the last jump to the ROM loader, 0 if none.
    pub jump_rsr: u32,
    /// RTC_BKP4R on entry: the `BootStage` the boot before this one
    /// reached, 0 after the backup domain reset.
    pub last_stage: u32,
    /// RTC_BKP5R on entry: the boot before this one's `UsbState`, 0 if it
    /// never got that far.
    pub last_usb: u32,
    /// RTC_BKP6R on entry: the boot before this one's last `UsbStep`.
    pub last_usb_step: u32,
    /// RTC_BKP7R..12R on entry: the boot before this one's last
    /// `UsbRegs` snapshot.
    pub last_usb_regs: UsbRegs,
}

impl fmt::Display for BootSeen {
    /// `boot marker=… … last_usb=… last_usb_step=…`, then a
    /// `last_usb_regs …` line.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let action = match self.action {
            BootAction::Synth => "Synth",
            BootAction::RomDfu => "RomDfu",
        };
        write!(
            f,
            "boot marker={:08x} readback={:08x} action={action} rsr={:08x} dbp={} boots={} from=",
            self.marker,
            self.readback,
            self.rsr,
            u8::from(self.dbp),
            self.boots,
        )?;
        match self.from {
            0 => f.write_str("none")?,
            FROM_MENU => f.write_str("menu")?,
            FROM_CONSOLE => f.write_str("console")?,
            x => write!(f, "{x:08x}")?,
        }
        write!(f, " jump_rsr={:08x} last_stage=", self.jump_rsr)?;
        match (self.last_stage, BootStage::from_code(self.last_stage)) {
            (_, Some(s)) => f.write_str(s.label())?,
            (0, None) => f.write_str("none")?,
            (x, None) => write!(f, "{x:08x}")?,
        }
        f.write_str(" last_usb=")?;
        match (self.last_usb, UsbState::from_code(self.last_usb)) {
            (_, Some(s)) => write!(f, "{s}")?,
            (0, None) => f.write_str("none")?,
            (x, None) => write!(f, "{x:08x}")?,
        }
        f.write_str(" last_usb_step=")?;
        match (self.last_usb_step, UsbStep::from_code(self.last_usb_step)) {
            (_, Some((s, n))) => write!(f, "{}(try {n})", s.label())?,
            (0, None) => f.write_str("none")?,
            (x, None) => write!(f, "{x:08x}")?,
        }
        write!(f, "\nlast_usb_regs {}", self.last_usb_regs)
    }
}

/// What the DFU prompt's yes confirms. `Witnessed<Witness = ()>`: nothing
/// on the card is at stake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomDfu;
