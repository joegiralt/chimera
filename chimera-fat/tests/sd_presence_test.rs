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

use chimera_fat::sd::{BOOT_RETRY_EVERY, BOOT_WINDOW, Presence, R1_IDLE, presence};
use chimera_hal::store::BootCard;
use chimera_hal::units::{Millis, NonZeroMillis};

/// The best of a burst: idle as soon as one CMD0 says so, else the first
/// R1, else none.
#[test]
fn a_burst_stops_at_idle() {
    let mut n = 0;
    let got = presence(|| {
        n += 1;
        [Some(0x00), Some(R1_IDLE), Some(0x05)][n - 1]
    });
    assert_eq!((got, n), (Some(R1_IDLE), 2));
}

#[test]
fn a_burst_without_idle_keeps_the_first_answer() {
    let replies = [None, Some(0x00), Some(0x05)];
    let mut n = 0;
    let got = presence(|| {
        n += 1;
        replies[n - 1]
    });
    assert_eq!((got, n), (Some(0x00), 3));
    assert_eq!(presence(|| None), None);
}

/// A slot whose card replies `reply(try)` to each burst (0 is the first),
/// each try costing `try_ms` on the clock besides the pauses.
struct Slot {
    reply: fn(u32) -> Option<u8>,
    try_ms: u32,
    tries: u32,
    paused: Millis,
    clock: Millis,
}

impl Slot {
    fn new(reply: fn(u32) -> Option<u8>) -> Self {
        Self {
            reply,
            try_ms: 0,
            tries: 0,
            paused: Millis::ZERO,
            clock: Millis::ZERO,
        }
    }

    fn ask(&mut self, p: &mut Presence) -> bool {
        let (reply, try_ms) = (self.reply, self.try_ms);
        let tries = &mut self.tries;
        let clock = core::cell::Cell::new(self.clock);
        let paused = core::cell::Cell::new(self.paused);
        let r = p.ask(
            || {
                let t = *tries;
                *tries += 1;
                clock.set(clock.get().saturating_add(Millis::new(try_ms)));
                reply(t)
            },
            |ms: NonZeroMillis| {
                paused.set(paused.get().saturating_add(ms.get()));
                clock.set(clock.get().saturating_add(ms.get()));
            },
            || clock.get(),
        );
        (self.clock, self.paused) = (clock.get(), paused.get());
        r
    }
}

fn ms(n: u32) -> Millis {
    Millis::new(n)
}

fn idle(_: u32) -> Option<u8> {
    Some(R1_IDLE)
}

fn silent(_: u32) -> Option<u8> {
    None
}

fn idle_from_30(t: u32) -> Option<u8> {
    (t >= 30).then_some(R1_IDLE)
}

/// Powering up: DO held low reads 0x00, an R1 but not idle.
fn low_then_idle(t: u32) -> Option<u8> {
    Some(if t >= 12 { R1_IDLE } else { 0x00 })
}

fn low(_: u32) -> Option<u8> {
    Some(0x00)
}

fn every() -> Millis {
    BOOT_RETRY_EVERY.get()
}

#[test]
fn nothing_asked_no_boot_result() {
    assert_eq!(Presence::new().boot(), None);
}

/// The usual boot: idle on the first burst, nothing waits.
#[test]
fn a_ready_card_costs_no_wait() {
    let (mut p, mut s) = (Presence::new(), Slot::new(idle));
    assert!(s.ask(&mut p));
    assert_eq!((s.tries, s.paused), (1, Millis::ZERO));
    assert_eq!(p.boot(), Some(BootCard::Ready(ms(0))));
}

/// chimera-releases#16: a card still powering up when boot first asks is
/// asked again until it answers, not read as an empty slot.
#[test]
fn boot_waits_for_a_slow_card() {
    let (mut p, mut s) = (Presence::new(), Slot::new(idle_from_30));
    assert!(s.ask(&mut p));
    assert_eq!(s.tries, 31);
    assert_eq!(s.paused, ms(30 * every().get()));
    assert_eq!(p.boot(), Some(BootCard::Ready(s.paused)));
}

/// A card answering 0x00 isn't ready: boot waits for idle (0x01).
#[test]
fn boot_waits_past_a_low_reply_for_idle() {
    let (mut p, mut s) = (Presence::new(), Slot::new(low_then_idle));
    assert!(s.ask(&mut p));
    assert_eq!(s.tries, 13);
    assert_eq!(p.boot(), Some(BootCard::Ready(ms(12 * every().get()))));
}

/// The boot result is the measured time, tries included, not the pauses.
#[test]
fn boot_reports_measured_time() {
    let (mut p, mut s) = (Presence::new(), Slot::new(idle_from_30));
    s.try_ms = 1;
    assert!(s.ask(&mut p));
    assert_eq!(p.boot(), Some(BootCard::Ready(s.clock)));
    assert_eq!(s.clock, ms(31 + 30 * every().get()));
}

/// The window is measured: slow tries end it sooner than the pauses would.
#[test]
fn slow_tries_count_toward_the_window() {
    let (mut p, mut s) = (Presence::new(), Slot::new(silent));
    s.try_ms = 10;
    assert!(!s.ask(&mut p));
    assert!(s.clock >= BOOT_WINDOW && s.paused < BOOT_WINDOW);
    assert_eq!(p.boot(), Some(BootCard::Absent(s.clock)));
}

/// An empty slot at boot costs the window, and no more.
#[test]
fn boot_gives_up_after_the_window() {
    let (mut p, mut s) = (Presence::new(), Slot::new(silent));
    assert!(!s.ask(&mut p));
    assert_eq!(s.paused, BOOT_WINDOW);
    assert_eq!(s.tries, BOOT_WINDOW.get() / every().get() + 1);
    assert_eq!(p.boot(), Some(BootCard::Absent(BOOT_WINDOW)));
}

/// A card that answers but never goes idle in the window is still a card
/// (#186): the acquire tries it, and status says it was busy.
#[test]
fn a_card_never_idle_is_busy_not_absent() {
    let (mut p, mut s) = (Presence::new(), Slot::new(low));
    assert!(s.ask(&mut p));
    assert_eq!(p.boot(), Some(BootCard::Busy(BOOT_WINDOW)));
}

/// A card that answers after the window (mount's re-init retry, or a
/// later re-read) is recorded as late, keeping the boot's wait.
#[test]
fn an_answer_after_the_window_is_late() {
    let mut p = Presence::new();
    Slot::new(silent).ask(&mut p);
    assert!(Slot::new(low).ask(&mut p));
    assert_eq!(p.boot(), Some(BootCard::Late(BOOT_WINDOW)));
    assert!(!Slot::new(silent).ask(&mut p));
    assert_eq!(p.boot(), Some(BootCard::Late(BOOT_WINDOW)));
}

/// #186 after boot: a re-read asks one burst and never waits, so the UI
/// doesn't stall; any R1 is a card.
#[test]
fn after_boot_an_ask_is_one_try() {
    for first in [idle as fn(u32) -> Option<u8>, idle_from_30, low, silent] {
        let mut p = Presence::new();
        Slot::new(first).ask(&mut p);
        let mut empty = Slot::new(silent);
        assert!(!empty.ask(&mut p));
        assert_eq!((empty.tries, empty.paused), (1, Millis::ZERO));
        let mut busy = Slot::new(low);
        assert!(busy.ask(&mut p));
        assert_eq!((busy.tries, busy.paused), (1, Millis::ZERO));
    }
}

/// Once the boot found the card, later asks never rewrite that.
#[test]
fn a_boot_result_found_stays() {
    for (first, want) in [
        (idle as fn(u32) -> Option<u8>, BootCard::Ready(ms(0))),
        (low, BootCard::Busy(BOOT_WINDOW)),
    ] {
        let mut p = Presence::new();
        Slot::new(first).ask(&mut p);
        Slot::new(silent).ask(&mut p);
        Slot::new(idle).ask(&mut p);
        assert_eq!(p.boot(), Some(want));
    }
}
