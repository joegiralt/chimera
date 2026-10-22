//! A loom model of the card bus's wakeups (spec § 4.3): the audio wants
//! and pends, the UI takes and gives, the card IRQ takes only through
//! `take_or_want`. However they interleave, no want is left with the bus
//! Free and no pend outstanding.
//!
//! `RUSTFLAGS="--cfg loom" cargo test -p chimera-core --features test-support
//! --test loom_card_bus --release` (`just loom`).
#![cfg(loom)]

use loom::sync::Arc;
use loom::sync::atomic::{AtomicBool, Ordering::SeqCst};
use loom::thread;

use chimera_hal::Ms;
use chimera_hal::card_bus::{CardBus, Owner, Taker, Took, Wake};

#[test]
fn no_want_left_stranded() {
    loom::model(|| {
        let bus = Arc::new(CardBus::new());
        let slot_wanted = Arc::new(AtomicBool::new(false));
        // The NVIC's pending bit for DMA2_STR0.
        let pending = Arc::new(AtomicBool::new(false));

        let audio = {
            let (slot_wanted, pending) = (slot_wanted.clone(), pending.clone());
            thread::spawn(move || {
                slot_wanted.store(true, SeqCst);
                pending.store(true, SeqCst);
            })
        };

        let ui = {
            let (bus, pending) = (bus.clone(), pending.clone());
            thread::spawn(move || {
                for _ in 0..2 {
                    if bus.try_take(Taker::Ui, Ms(0)) {
                        if bus.give() == Wake::Pend {
                            pending.store(true, SeqCst);
                        }
                        return;
                    }
                    thread::yield_now();
                }
            })
        };

        let irq = {
            let (bus, slot_wanted, pending) = (bus.clone(), slot_wanted.clone(), pending.clone());
            thread::spawn(move || {
                while pending.swap(false, SeqCst) {
                    if !slot_wanted.load(SeqCst) {
                        continue;
                    }
                    match bus.take_or_want(Ms(0)) {
                        Took::Wanted => return,
                        Took::Taken => {
                            slot_wanted.store(false, SeqCst);
                            if bus.give() == Wake::Pend {
                                pending.store(true, SeqCst);
                            }
                            if slot_wanted.load(SeqCst) {
                                pending.store(true, SeqCst);
                            }
                        }
                    }
                }
            })
        };

        audio.join().unwrap();
        ui.join().unwrap();
        irq.join().unwrap();

        // SAFETY: every thread is joined, so these happen after every
        // store (loom checks it). A plain `load` here may read an older
        // store than the last.
        let (wanted, pended) = unsafe { (slot_wanted.unsync_load(), pending.unsync_load()) };
        assert!(!wanted || pended, "a want stranded with no pend");
        assert_eq!(bus.owner(), Owner::Free);
    });
}
