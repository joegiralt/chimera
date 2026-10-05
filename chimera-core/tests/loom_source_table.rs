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

/// Two rounds and a held voice: the UI closes, waits, and on a nack or a
/// timeout opens and closes again; on an ack it moves source `a` out (its
/// row emptied, as a retired source's compaction leaves it), opens, then
/// reuses `a`'s id for a new source once two blocks have begun since and
/// `a`'s bit is clear (`Pool`'s `Draining`). The voice latches `a` at block
/// `on`: it reads, waits at the gate (keeping its bit), or ends on an empty
/// row (dropping it). The shadow holds 0 for `a`'s bytes, 1 once moved, 2
/// once reused: the voice must read only 0, and never race a write.
fn held_model(on: usize, drain_blocks: u32) {
    loom::model(move || {
        let t: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
        let bytes: &'static mut [u8] = Box::leak(vec![7u8; 64].into_boxed_slice());
        t.attach_for_test(bytes.as_mut_ptr());
        let a = SourceId::ALL[0];
        t.publish_for_test(a, 32, 16, 24_000);
        let shadow = Arc::new(UnsafeCell::new(0u32));
        let mut tok = t.reader().unwrap();

        let audio = {
            let shadow = shadow.clone();
            thread::spawn(move || {
                let (mut latched, mut ended) = (false, false);
                for n in 0..3 {
                    latched |= n == on;
                    {
                        let v = tok.begin_block();
                        if latched && !ended {
                            match v.bytes(a) {
                                Some(_) => {
                                    // SAFETY: loom's cell, read through its checker.
                                    let seen = shadow.with(|p| unsafe { *p });
                                    assert_eq!(seen, 0, "the voice read another source");
                                }
                                None if v.moving(a) => {}
                                None => ended = true,
                            }
                        }
                    }
                    tok.store_sounding(if latched && !ended { a.bit() } else { 0 });
                }
            })
        };

        let mut moved_at = None;
        for _ in 0..2 {
            let e = t.close_gate_for_test(a.bit());
            let mut tries = 0;
            let answer = t.wait_for_test(e, || {
                thread::yield_now();
                tries += 1;
                tries < 2
            });
            if answer == Answer::Acked {
                // SAFETY: loom's cell, written through its checker.
                shadow.with_mut(|p| unsafe { *p = 1 });
                t.clear_for_test(a);
                moved_at = Some(t.blocks());
            }
            t.open_gate_for_test();
            if moved_at.is_some() {
                break;
            }
        }
        if let Some(since) = moved_at {
            for _ in 0..2 {
                let blocks = t.blocks();
                if blocks.wrapping_sub(since) >= drain_blocks && t.sounding() & a.bit() == 0 {
                    // SAFETY: loom's cell, written through its checker.
                    shadow.with_mut(|p| unsafe { *p = 2 });
                    t.publish_for_test(a, 0, 16, 24_000);
                    break;
                }
                thread::yield_now();
            }
        }
        audio.join().unwrap();
    });
}

#[test]
fn a_held_voice_never_reads_a_moved_or_reused_source() {
    for on in 0..2 {
        held_model(on, 2);
    }
}
