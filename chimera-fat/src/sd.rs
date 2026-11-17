//! Is a card in the slot? Asked before an acquire, so an empty slot costs
//! a few CMD0s, not the acquire deadline (#186). Pure: the shell sends
//! `CMD0` with CS low and hands over the `NCR_BYTES` it clocks back.

use crate::sd_addr::CmdArg;
use crate::sd_proto;
use chimera_hal::store::BootCard;
use chimera_hal::units::Millis;

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

/// How long boot keeps asking (chimera-releases#16). The SD spec gives a
/// card 1 ms after VDD is stable, then 74 clocks, before CMD0 (Physical
/// Layer Simplified Spec § 6.4.1); real cards and a slow rail take longer,
/// and nothing says when the rail is stable. ADR 0277.
pub const BOOT_WINDOW: Millis = Millis::new(1_000);
/// The pause between boot's asks; each ask re-sends the wake clocks.
pub const BOOT_RETRY_EVERY: Millis = Millis::new(10);

/// Is a card in the slot, and how did it answer at boot? The first ask
/// after reset waits out `BOOT_WINDOW`; every later one asks once (#186),
/// so a re-read of an empty slot never stalls the UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct Presence {
    boot: Option<BootCard>,
}

impl Presence {
    pub const fn new() -> Self {
        Self { boot: None }
    }

    /// What the first ask found; `None` before it.
    pub const fn boot(&self) -> Option<BootCard> {
        self.boot
    }

    /// `answers`: wake clocks, then the CMD0 check. `pause`: wait that
    /// long. At boot the time is the pauses', a try's own (about 1 ms) on
    /// top.
    pub fn ask(
        &mut self,
        mut answers: impl FnMut() -> bool,
        mut pause: impl FnMut(Millis),
    ) -> bool {
        if self.boot.is_some() {
            return answers();
        }
        let every = BOOT_RETRY_EVERY.get().max(1);
        let mut waited = 0;
        let found = loop {
            if answers() {
                break BootCard::Ready(Millis::new(waited));
            }
            if waited >= BOOT_WINDOW.get() {
                break BootCard::Absent(Millis::new(waited));
            }
            pause(Millis::new(every));
            waited += every;
        };
        self.boot = Some(found);
        matches!(found, BootCard::Ready(_))
    }
}
