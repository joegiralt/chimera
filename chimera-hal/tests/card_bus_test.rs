use chimera_hal::Ms;
use chimera_hal::card_bus::{CardBus, CardFault, Owner, Quiet, Taker, Took, Wake};
use chimera_hal::units::{Millis, StreamCount, UiWait};

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
    assert!(!b.stuck(Ms(300)));
    assert!(b.stuck(Ms(301)));
    b.give();
    assert!(b.try_take(Taker::Ui, Ms(100)));
    assert!(!b.stuck(Ms(10_000)));
}

#[test]
fn a_take_stamped_after_the_clock_read_is_not_stuck() {
    let b = CardBus::new();
    assert!(b.try_take(Taker::Stream, Ms(1_002)));
    assert!(!b.stuck(Ms(1_000)));
    b.give();
    assert!(b.try_take(Taker::Stream, Ms(1)));
    assert!(!b.stuck(Ms(u32::MAX - 1)));
    b.give();
    assert!(b.try_take(Taker::Stream, Ms(u32::MAX - 1)));
    assert!(!b.stuck(Ms(u32::MAX - 3)));
    assert!(b.stuck(Ms(200)));
    assert!(!b.stuck(Ms(198)));
}

#[test]
fn abort_asked_is_taken_once() {
    let b = CardBus::new();
    assert!(!b.take_abort());
    b.ask_abort();
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
fn ui_wait_round_trips() {
    let b = CardBus::new();
    assert_eq!(b.ui_wait(), UiWait::Idle);
    b.set_ui_wait(UiWait::Waiting);
    assert_eq!(b.ui_wait(), UiWait::Waiting);
    b.set_ui_wait(UiWait::Idle);
    assert_eq!(b.ui_wait(), UiWait::Idle);
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
    b.ask_abort();
    assert_eq!(b.take_or_want(Ms(0)), Took::Taken);
    assert!(!b.take_abort());
    b.ask_abort();
    assert!(b.take_abort());
}

#[test]
fn a_yield_to_a_waiting_ui_is_answered_by_its_give() {
    let b = CardBus::new();
    b.want();
    assert!(b.try_take(Taker::Ui, Ms(0)));
    assert_eq!(b.give(), Wake::Pend);
    assert_eq!(b.give(), Wake::Quiet);
}
