//! Is a card in the slot? Asked before an acquire, so an empty slot costs
//! a few CMD0s, not the acquire deadline (#186). Pure: the shell sends
//! `CMD0` with CS low and hands over the `NCR_BYTES` it clocks back.

use crate::sd_addr::CmdArg;
use crate::sd_proto;
use chimera_hal::store::BootCard;
use chimera_hal::units::{Millis, NonZeroMillis};

/// GO_IDLE_STATE with its CRC, which the card checks before SPI mode.
pub const CMD0: [u8; 6] = sd_proto::command(sd_proto::CMD0, CmdArg::ZERO);

/// NCR: a card answers within 8 bytes of a command.
pub const NCR_BYTES: usize = 8;

/// CMD0 tries before the slot counts as empty.
pub const PRESENCE_TRIES: usize = 3;

/// R1, the first byte within NCR with bit 7 clear. None when nothing
/// answered: MISO is pulled up, so an empty slot reads all 0xFF. A busy
/// card holds DO low, which reads 0x00: an answer.
pub fn r1_within_ncr(bytes: &[u8]) -> Option<u8> {
    bytes
        .iter()
        .take(NCR_BYTES)
        .copied()
        .find(|b| b & 0x80 == 0)
}

/// R1 in idle: the card took CMD0 and entered SPI mode.
pub const R1_IDLE: u8 = 0x01;

/// How long boot keeps asking, measured from its first CMD0
/// (chimera-releases#16, ADR 0277). The SD spec gives a card 1 ms after
/// VDD is stable, then 74 clocks, before CMD0 (Physical Layer Simplified
/// Spec § 6.4.1); nothing on the board says when the rail is stable, and
/// a card held in power-up reads 0x00, not idle.
pub const BOOT_WINDOW: Millis = Millis::new(1_000);
/// The pause between boot's asks; each ask re-sends the wake clocks.
pub const BOOT_RETRY_EVERY: NonZeroMillis = NonZeroMillis::new(10).unwrap();

const _: () = assert!(BOOT_WINDOW.get() >= 1_000);
const _: () = assert!(BOOT_RETRY_EVERY.get().get() <= BOOT_WINDOW.get() / 50);

/// One burst of `PRESENCE_TRIES` CMD0s: idle as soon as a try says so,
/// else the first R1 (busy), else none. `try_cmd0` sends one and reads
/// its R1 (`r1_within_ncr`).
pub fn presence(mut try_cmd0: impl FnMut() -> Option<u8>) -> Option<u8> {
    let mut first = None;
    for _ in 0..PRESENCE_TRIES {
        match try_cmd0() {
            Some(R1_IDLE) => return Some(R1_IDLE),
            r => first = first.or(r),
        }
    }
    first
}

/// Is a card in the slot, and how did it answer at boot? The first ask
/// after reset waits, up to `BOOT_WINDOW`, for idle; every later one asks
/// once, any R1 a card (#186), so a re-read of an empty slot never stalls
/// the UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct Presence {
    boot: Option<BootCard>,
}

impl Presence {
    pub const fn new() -> Self {
        Self { boot: None }
    }

    /// What boot found; `None` before the first ask.
    pub const fn boot(&self) -> Option<BootCard> {
        self.boot
    }

    /// `answers`: wake clocks, then a `presence` burst. `pause`: wait that
    /// long. `elapsed`: time since this ask began, as measured (`ZERO`
    /// where nothing counts: the pauses are then the clock).
    pub fn ask(
        &mut self,
        mut answers: impl FnMut() -> Option<u8>,
        mut pause: impl FnMut(NonZeroMillis),
        mut elapsed: impl FnMut() -> Millis,
    ) -> bool {
        match self.boot {
            None => {
                let found = Self::wait(&mut answers, &mut pause, &mut elapsed);
                self.boot = Some(found);
                !matches!(found, BootCard::Absent(_))
            }
            Some(BootCard::Absent(ms)) => {
                let there = answers().is_some();
                if there {
                    self.boot = Some(BootCard::Late(ms));
                }
                there
            }
            Some(_) => answers().is_some(),
        }
    }

    fn wait(
        answers: &mut impl FnMut() -> Option<u8>,
        pause: &mut impl FnMut(NonZeroMillis),
        elapsed: &mut impl FnMut() -> Millis,
    ) -> BootCard {
        let mut paused = Millis::ZERO;
        let mut busy = false;
        loop {
            let r = answers();
            let t = elapsed().max(paused);
            match r {
                Some(R1_IDLE) => return BootCard::Ready(t),
                Some(_) => busy = true,
                None => {}
            }
            if t >= BOOT_WINDOW {
                return if busy {
                    BootCard::Busy(t)
                } else {
                    BootCard::Absent(t)
                };
            }
            pause(BOOT_RETRY_EVERY);
            paused = paused.saturating_add(BOOT_RETRY_EVERY.get());
        }
    }
}
