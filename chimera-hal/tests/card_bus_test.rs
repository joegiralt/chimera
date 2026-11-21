use chimera_hal::Ms;
use chimera_hal::card_bus::{CardBus, CardFault, Owner, Quiet, Taker, Took, Wake};
use chimera_hal::units::{Millis, StreamCount};

#[test]
fn take_and_give() {
    let b = CardBus::new();
    assert_eq!(b.owner(), Owner::Free);
    assert!(b.try_take(Taker::Ui, Ms(0)));
    assert_eq!(b.owner(), Owner::Ui);
    assert_eq!(b.give(), Wake::Quiet);
    assert_eq!(b.owner(), Owner::Free);
    assert!(b.try_take(Taker::Stream, Ms(0)));
    assert_eq!(b.owner(), Owner::Stream);
}

#[test]
fn second_taker_fails() {
    let b = CardBus::new();
    assert!(b.try_take(Taker::Stream, Ms(0)));
    assert!(!b.try_take(Taker::Ui, Ms(0)));
    assert!(!b.try_take(Taker::Stream, Ms(0)));
    assert_eq!(b.owner(), Owner::Stream);
}

#[test]
fn take_or_want_marks_wanted_while_ui_holds() {
    let b = CardBus::new();
    assert!(b.try_take(Taker::Ui, Ms(0)));
    assert_eq!(b.take_or_want(Ms(0)), Took::Wanted);
    assert_eq!(b.owner(), Owner::Ui);
    assert_eq!(b.give(), Wake::Pend);
    assert_eq!(b.give(), Wake::Quiet);
}

#[test]
fn take_or_want_takes_a_free_bus() {
    let b = CardBus::new();
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    assert_eq!(b.owner(), Owner::Stream);
    assert_eq!(b.give(), Wake::Quiet);
}

#[test]
fn stream_stuck_after_watchdog() {
    let b = CardBus::new();
    assert!(b.try_take(Taker::Stream, Ms(100)));
    assert!(b.stuck(Ms(300)).is_none());
    assert!(b.stuck(Ms(301)).is_some());
    b.give();
    assert!(b.try_take(Taker::Ui, Ms(100)));
    assert!(b.stuck(Ms(10_000)).is_none());
}

/// M2 (#761): a slow card within the SD spec waits up to 100 ms (Nac)
/// for each block's token. A 9-block chunk at 90 ms a block holds the bus
/// 810 ms; renewed at each burst, the watchdog never calls it stuck, and
/// a burst that then stalls is still caught 200 ms after it began.
#[test]
fn a_slow_read_renewed_each_burst_is_never_stuck() {
    let b = CardBus::new();
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    for block in 0..9 {
        let at = block * 90;
        b.renew(Ms(at));
        for t in at..at + 90 {
            assert!(b.stuck(Ms(t)).is_none(), "stuck at {t} ms");
        }
    }
    assert!(b.stuck(Ms(720 + 200)).is_none());
    assert!(
        b.stuck(Ms(720 + 201)).is_some(),
        "a stalled burst is caught"
    );
}

/// Only a `Stream` hold is renewed: the UI's is never stamped.
#[test]
fn renew_leaves_another_owners_bus_alone() {
    let b = CardBus::new();
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    b.give();
    assert!(b.try_take(Taker::Ui, Ms(0)));
    b.renew(Ms(500));
    b.give();
    assert_eq!(b.take_or_want(Ms(1_000)), Took::Taken);
    assert!(b.stuck(Ms(1_200)).is_none());
}

#[test]
fn a_take_stamped_after_the_clock_read_is_not_stuck() {
    let b = CardBus::new();
    assert!(b.try_take(Taker::Stream, Ms(1_002)));
    assert!(b.stuck(Ms(1_000)).is_none());
    b.give();
    assert!(b.try_take(Taker::Stream, Ms(1)));
    assert!(b.stuck(Ms(u32::MAX - 1)).is_none());
    b.give();
    assert!(b.try_take(Taker::Stream, Ms(u32::MAX - 1)));
    assert!(b.stuck(Ms(u32::MAX - 3)).is_none());
    assert!(b.stuck(Ms(200)).is_some());
    assert!(b.stuck(Ms(198)).is_none());
}

#[test]
fn abort_asked_is_taken_once() {
    let b = CardBus::new();
    assert!(!b.take_abort());
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    assert!(b.ask_abort(b.held().unwrap()));
    assert!(b.take_abort());
    assert!(!b.take_abort());
}

#[test]
fn settle_waits_quiet_hold() {
    let b = CardBus::new();
    b.ask_quiet(Ms(0));
    b.answer_quiet(Ms(0));
    b.note_write(Ms(100));
    assert_eq!(None, b.settle(Ms(119)));
    assert_eq!(b.quiet(), Quiet::Held);
    assert_eq!(b.settle(Ms(120)), Some(Millis::new(120)));
    assert_eq!(b.quiet(), Quiet::Off);
}

#[test]
fn settle_across_the_ms_wrap() {
    let b = CardBus::new();
    b.ask_quiet(Ms(0));
    b.answer_quiet(Ms(0));
    b.note_write(Ms(u32::MAX - 5));
    assert_eq!(b.settle(Ms(14)), Some(Millis::new(14)));
}

#[test]
fn quiet_walks_off_asked_held() {
    let b = CardBus::new();
    assert_eq!(b.quiet(), Quiet::Off);
    b.ask_quiet(Ms(0));
    assert_eq!(b.quiet(), Quiet::Asked);
    b.answer_quiet(Ms(0));
    assert_eq!(b.quiet(), Quiet::Held);
    b.note_write(Ms(0));
    assert_eq!(b.settle(Ms(20)), Some(Millis::new(20)));
    assert_eq!(b.quiet(), Quiet::Off);
}

/// A quiet's length runs from its ask; a second ask while one runs moves
/// nothing.
#[test]
fn settle_answers_the_quiet_length_from_its_ask() {
    let b = CardBus::new();
    b.ask_quiet(Ms(10));
    b.ask_quiet(Ms(30));
    b.answer_quiet(Ms(40));
    b.note_write(Ms(50));
    assert_eq!(b.settle(Ms(70)), Some(Millis::new(60)));
    assert_eq!(b.settle(Ms(200)), None, "once");
}

#[test]
fn answer_without_ask_stays_off() {
    let b = CardBus::new();
    b.answer_quiet(Ms(0));
    assert_eq!(b.quiet(), Quiet::Off);
    assert_eq!(None, b.settle(Ms(1_000)));
}

#[test]
fn streams_active_round_trips() {
    let b = CardBus::new();
    assert_eq!(b.streams_active(), None);
    for n in 1..=8 {
        let c = StreamCount::new(n);
        b.set_streams_active(c);
        assert_eq!(b.streams_active(), c);
    }
    b.set_streams_active(None);
    assert_eq!(b.streams_active(), None);
}

#[test]
fn fault_word_round_trip() {
    let b = CardBus::new();
    for f in [CardFault::Clear, CardFault::Read, CardFault::Clear] {
        b.set_fault(f);
        assert_eq!(b.fault(), f);
    }
}

#[test]
fn a_hold_counts_from_its_answer_not_an_old_write() {
    let b = CardBus::new();
    b.note_write(Ms(0));
    b.ask_quiet(Ms(0));
    b.answer_quiet(Ms(5000));
    assert_eq!(None, b.settle(Ms(5001)));
    assert_eq!(b.settle(Ms(5020)), Some(Millis::new(5020)));
}

#[test]
fn answering_a_held_quiet_again_keeps_its_stamp() {
    let b = CardBus::new();
    b.ask_quiet(Ms(0));
    b.answer_quiet(Ms(100));
    b.answer_quiet(Ms(115));
    assert_eq!(b.settle(Ms(120)), Some(Millis::new(120)));
}

#[test]
fn a_stream_take_drops_a_stale_abort() {
    let b = CardBus::new();
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    let h = b.held().unwrap();
    b.give();
    assert!(!b.ask_abort(h), "given back since");
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    assert!(!b.take_abort());
    assert!(b.ask_abort(b.held().unwrap()));
    assert!(b.take_abort());
}

/// #766: the watchdog's abort carries the hold it judged; the IRQ drops
/// one whose hold isn't its transfer's (a give and retake between the
/// judge and the ask), or that a burst renewed since.
#[test]
fn a_stale_abort_never_stops_a_fresh_transfer() {
    let b = CardBus::new();
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    let judged = b.stuck(Ms(201)).expect("stuck");
    b.give();
    assert_eq!(b.take_or_want(Ms(202)), Took::Taken);
    assert!(!b.ask_abort(judged), "the next transfer's: not asked");
    assert!(!b.take_abort());
    // Asked, then the hold moves before the IRQ reads it: dropped there.
    let judged = b.stuck(Ms(500)).expect("stuck");
    assert!(b.ask_abort(judged));
    b.renew(Ms(501));
    assert!(!b.take_abort(), "renewed since");
    let judged = b.stuck(Ms(800)).expect("stuck");
    assert!(b.ask_abort(judged));
    b.give();
    assert_eq!(b.take_or_want(Ms(801)), Took::Taken);
    assert!(!b.take_abort(), "re-taken since");
    let judged = b.stuck(Ms(1_100)).expect("stuck");
    assert!(b.ask_abort(judged));
    assert!(b.take_abort(), "its own transfer's");
}

/// From thread context an ask re-reads the hold: one already renewed or
/// re-taken since the judge is not asked at all, so the asker may judge
/// again (#766's review).
#[test]
fn an_ask_on_a_hold_already_moved_is_not_made() {
    let b = CardBus::new();
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    let judged = b.stuck(Ms(201)).expect("stuck");
    b.renew(Ms(202));
    assert!(!b.ask_abort(judged));
    assert!(!b.take_abort());
    assert!(b.ask_abort(b.held().unwrap()));
    assert!(b.take_abort());
}

/// #569: a UI that has waited `UI_TURN_MS` asks a turn; the card IRQ then
/// parks (`Wanted`) instead of taking the bus, the UI takes it, and its
/// give wakes the IRQ. Taking ends the turn; a UI that gives up cancels it.
#[test]
fn a_ui_turn_parks_the_streams_until_the_ui_has_had_the_bus() {
    use chimera_hal::card_bus::{UI_TURN_MS, Wake};
    assert_eq!(UI_TURN_MS.get(), 100);
    let b = CardBus::new();
    assert!(!b.turn_asked());
    b.ask_turn();
    assert!(b.turn_asked());
    assert_eq!(b.take_or_want(Ms(0)), Took::Wanted, "the streams wait");
    assert!(b.try_take(Taker::Ui, Ms(1)));
    assert!(!b.turn_asked(), "the turn is the UI's take");
    assert_eq!(b.give(), Wake::Pend, "its give wakes the IRQ");
    assert_eq!(b.take_or_want(Ms(2)), Took::Taken);
    b.give();
    b.ask_turn();
    b.cancel_turn();
    assert_eq!(
        b.take_or_want(Ms(3)),
        Took::Taken,
        "cancelled: the streams go on"
    );
}
