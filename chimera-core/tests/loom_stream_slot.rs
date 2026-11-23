// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A loom model of a stream slot: one audio thread wants a chunk, cancels
//! and wants another; one card thread claims, fills and publishes. A
//! shadow cell stands for the slot's bytes, holding the chunk the card
//! filled them with: the audio reads it whenever `ready` hands it a slot
//! under its own generation, and loom fails the model if it ever reads
//! another chunk's bytes, or a read and a write aren't ordered.
//!
//! `RUSTFLAGS="--cfg loom" cargo test -p chimera-core --features test-support
//! --test loom_stream_slot --release` (`just loom`).
#![cfg(loom)]

use loom::cell::UnsafeCell;
use loom::sync::Arc;
use loom::thread;

use chimera_engine::stream::layout::ChunkIdx;
use chimera_engine::stream::ring::{AudioTurn, Rings, ThirdSlots};
use chimera_engine::stream::units::{AudioBlocks, Generation, SlotIdx};
use chimera_engine::voice_alloc::VoiceIdx;

const V: VoiceIdx = VoiceIdx::ALL[0];

/// If slot `s` is Ready under `g`, its bytes must be `chunk`'s.
fn read(turn: &AudioTurn, s: SlotIdx, g: Generation, chunk: u32, shadow: &UnsafeCell<u32>) {
    if let Some(r) = turn.ready(V, s, g) {
        assert_eq!(r.chunk().get(), chunk, "Ready under another chunk");
        // SAFETY: loom's cell, read through its checker.
        let filled = shadow.with(|p| unsafe { *p });
        assert_eq!(filled, chunk, "the audio read another fill's bytes");
    }
}

/// Slot A's bytes lie in `Rings`, slot C's in `ThirdSlots`.
#[test]
fn a_cancelled_fill_never_reads_as_the_new_one() {
    model(SlotIdx::A);
}

#[test]
fn a_cancelled_third_slot_fill_never_reads_as_the_new_one() {
    model(SlotIdx::C);
}

fn model(s: SlotIdx) {
    loom::model(move || {
        // `Rings::new` builds its 64 KB on the stack: more than a loom
        // thread's default.
        let (r, third): (&'static Rings, &'static ThirdSlots) = thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                (
                    &*Box::leak(Box::new(Rings::new())),
                    &*Box::leak(Box::new(ThirdSlots::new())),
                )
            })
            .unwrap()
            .join()
            .unwrap();
        let (mut audio, mut card) = r.sides(third).unwrap();
        let shadow = Arc::new(UnsafeCell::new(0u32));
        let at = AudioBlocks::new(0);

        let filler = {
            let shadow = shadow.clone();
            thread::spawn(move || {
                for _ in 0..2 {
                    let Some(c) = card.claim(V, s) else {
                        thread::yield_now();
                        continue;
                    };
                    if let Some(buf) = card.fill_buf(&c) {
                        buf[0] = 1;
                        // SAFETY: loom's cell, written through its checker.
                        shadow.with_mut(|p| unsafe { *p = c.chunk().get() });
                    }
                    card.fill_done(c);
                }
            })
        };

        let mut turn = audio.turn();
        let g1 = turn.want(V, s, ChunkIdx::new(1).unwrap(), at).unwrap();
        read(&turn, s, g1, 1, &shadow);
        turn.cancel(V, s);
        let g2 = turn.want(V, s, ChunkIdx::new(2).unwrap(), at).unwrap();
        assert_ne!(g1, g2);
        thread::yield_now();
        read(&turn, s, g2, 2, &shadow);
        assert!(
            turn.ready(V, s, g1).is_none(),
            "a cancelled generation never reads"
        );
        drop(turn);
        filler.join().unwrap();
    });
}
