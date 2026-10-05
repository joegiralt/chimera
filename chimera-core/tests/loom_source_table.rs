//! A loom model of `SourceTable` (ADR 0087's gate): one audio thread
//! running blocks through its `AudioToken`, one UI thread closing the gate,
//! waiting for the answer, moving on an ack and opening. A shadow cell
//! stands for the moving source's bytes: the audio reads it whenever
//! `GateView::bytes` hands it the source, the UI writes it as it moves
//! them, and loom fails the model if any read and write aren't ordered.
//!
//! `RUSTFLAGS="--cfg loom" cargo test -p chimera-core --features test-support
//! --test loom_source_table --release` (`just loom`).
#![cfg(loom)]

use loom::cell::UnsafeCell;
use loom::sync::Arc;
use loom::thread;

use chimera_core::project::SourceId;
use chimera_core::source_table::{Answer, SourceTable};

/// The model: `voice` says whether the audio's first block starts a note
/// on the moving source (so the next block nacks).
fn model(voice: bool) {
    loom::model(move || {
        let t: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
        let bytes: &'static mut [u8] = Box::leak(vec![7u8; 64].into_boxed_slice());
        t.attach_for_test(bytes.as_mut_ptr());
        let (a, b) = (SourceId::ALL[0], SourceId::ALL[1]);
        t.publish_for_test(a, 32, 16, 24_000);
        t.publish_for_test(b, 0, 16, 24_000);
        let shadow = Arc::new(UnsafeCell::new(0u32));
        let mut tok = t.reader().unwrap();

        let audio = {
            let shadow = shadow.clone();
            thread::spawn(move || {
                let mut sounding = 0;
                for n in 0..2 {
                    {
                        let v = tok.begin_block();
                        if v.bytes(a).is_some() {
                            // SAFETY: loom's cell, read through its checker.
                            shadow.with(|p| unsafe { *p });
                            if voice && n == 0 {
                                sounding = a.bit();
                            }
                        }
                        assert!(v.bytes(b).is_some(), "an unmoved source always reads");
                    }
                    tok.store_sounding(sounding);
                }
            })
        };

        let e = t.close_gate_for_test(a.bit());
        let mut tries = 0;
        let answer = t.wait_for_test(e, || {
            thread::yield_now();
            tries += 1;
            tries < 3
        });
        if answer == Answer::Acked {
            // SAFETY: loom's cell, written through its checker.
            shadow.with_mut(|p| unsafe { *p += 1 });
            t.rebase_for_test(a, 16);
        }
        t.open_gate_for_test();
        audio.join().unwrap();
    });
}

#[test]
fn no_read_overlaps_a_move() {
    model(false);
}

#[test]
fn a_nack_never_moves_under_a_voice() {
    model(true);
}
