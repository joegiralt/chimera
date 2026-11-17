use chimera_fat::sd::{CMD0, NCR_BYTES, r1_within_ncr};

#[test]
fn cmd0_is_go_idle_with_its_crc() {
    assert_eq!(CMD0, [0x40, 0, 0, 0, 0, 0x95]);
    assert_eq!(NCR_BYTES, 8);
}

/// An empty slot: MISO is pulled up, every byte reads 0xFF.
#[test]
fn all_ones_is_no_answer() {
    assert_eq!(r1_within_ncr(&[0xFF; NCR_BYTES]), None);
    assert_eq!(r1_within_ncr(&[]), None);
}

/// The first byte with bit 7 clear is R1, at any position within NCR.
#[test]
fn r1_is_the_first_byte_with_bit_7_clear() {
    for at in 0..NCR_BYTES {
        let mut b = [0xFF; NCR_BYTES];
        b[at] = 0x01;
        assert_eq!(r1_within_ncr(&b), Some(0x01), "R1 at byte {at}");
    }
    assert_eq!(r1_within_ncr(&[0xFF, 0x05, 0x01]), Some(0x05));
}

/// A busy card holds DO low: that is a card, not an empty slot.
#[test]
fn busy_low_is_an_answer() {
    assert_eq!(r1_within_ncr(&[0x00; NCR_BYTES]), Some(0x00));
}

/// Bus noise with bit 7 set is not R1.
#[test]
fn high_bytes_are_not_r1() {
    assert_eq!(r1_within_ncr(&[0xFE, 0x80, 0xC1, 0xFF]), None);
}

/// Only NCR's bytes count: an answer past them is too late.
#[test]
fn answer_past_ncr_is_none() {
    let mut b = [0xFF; NCR_BYTES + 1];
    b[NCR_BYTES] = 0x01;
    assert_eq!(r1_within_ncr(&b), None);
}

use chimera_fat::sd::{BOOT_RETRY_EVERY, BOOT_WINDOW, Presence};
use chimera_hal::store::BootCard;
use chimera_hal::units::Millis;

/// A slot whose card first answers on try `answers_on` (0 is the first),
/// or never.
struct Slot {
    answers_on: Option<u32>,
    tries: u32,
    paused: u32,
}

impl Slot {
    fn new(answers_on: Option<u32>) -> Self {
        Self {
            answers_on,
            tries: 0,
            paused: 0,
        }
    }

    fn ask(&mut self, p: &mut Presence) -> bool {
        p.ask(
            || {
                let t = self.tries;
                self.tries += 1;
                self.answers_on.is_some_and(|n| t >= n)
            },
            |ms: Millis| self.paused += ms.get(),
        )
    }
}

fn ms(n: u32) -> Millis {
    Millis::new(n)
}

#[test]
fn nothing_asked_no_boot_result() {
    assert_eq!(Presence::new().boot(), None);
}

/// The usual boot: the card answers the first CMD0s, nothing waits.
#[test]
fn a_ready_card_costs_no_wait() {
    let (mut p, mut s) = (Presence::new(), Slot::new(Some(0)));
    assert!(s.ask(&mut p));
    assert_eq!((s.tries, s.paused), (1, 0));
    assert_eq!(p.boot(), Some(BootCard::Ready(ms(0))));
}

/// chimera-releases#16: a card still powering up when boot first asks is
/// asked again until it answers, not read as an empty slot.
#[test]
fn boot_waits_for_a_slow_card() {
    let (mut p, mut s) = (Presence::new(), Slot::new(Some(30)));
    assert!(s.ask(&mut p));
    assert_eq!(s.tries, 31);
    assert_eq!(s.paused, 30 * BOOT_RETRY_EVERY.get());
    assert_eq!(p.boot(), Some(BootCard::Ready(ms(s.paused))));
}

/// An empty slot at boot costs the window, and no more.
#[test]
fn boot_gives_up_after_the_window() {
    let (mut p, mut s) = (Presence::new(), Slot::new(None));
    assert!(!s.ask(&mut p));
    assert_eq!(s.paused, BOOT_WINDOW.get());
    assert_eq!(s.tries, BOOT_WINDOW.get() / BOOT_RETRY_EVERY.get() + 1);
    assert_eq!(p.boot(), Some(BootCard::Absent(BOOT_WINDOW)));
}

/// The window is the spec's power-up time with room for slow cards.
#[test]
fn the_window_covers_power_up() {
    assert!(BOOT_WINDOW.get() >= 1_000);
    assert!(BOOT_RETRY_EVERY.get() > 0 && BOOT_RETRY_EVERY.get() <= 20);
}

/// #186 after boot: a re-read of an empty slot asks once and never
/// waits, so the UI doesn't stall; the boot's result stays.
#[test]
fn after_boot_an_ask_is_one_try() {
    for first in [Some(0), Some(5), None] {
        let mut p = Presence::new();
        Slot::new(first).ask(&mut p);
        let boot = p.boot();
        let mut empty = Slot::new(None);
        assert!(!empty.ask(&mut p));
        assert_eq!((empty.tries, empty.paused), (1, 0), "after {first:?}");
        let mut there = Slot::new(Some(0));
        assert!(there.ask(&mut p));
        assert_eq!((there.tries, there.paused), (1, 0));
        assert_eq!(p.boot(), boot);
    }
}
