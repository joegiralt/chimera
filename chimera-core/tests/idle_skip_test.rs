// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bus's idle skips (#793) change no bit: a project played, let ring
//! out until every effect is idle, then woken, renders the same DAC pairs
//! with the skips as with `NEVER_SKIP`, through the whole engine. Run with
//! the host's floats and with flush-to-zero (the unit's FPSCR FZ, which
//! lets the reverb's tail gate reach exact zero), each skip counted so
//! none of the proofs is empty. The skips and their counts are process
//! wide: the tests take turns.

mod common;

use std::sync::Mutex;
use std::sync::atomic::Ordering;

use chimera_core::audio_out::{DacSample, interleave};
use chimera_core::dsp::SendLevel;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::{NEVER_SKIP, SKIPS, Skip};
use chimera_core::hw::{BLOCK_SIZE, SampleBudget};
use chimera_core::instrument::AudioShared;
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::part::DacPair;
use chimera_core::{MidiNote, Velocity};
use common::InstRig;

static TURN: Mutex<()> = Mutex::new(());

/// What a run plays each block: a note on or off on Part 1, or nothing.
#[derive(Clone, Copy)]
enum Step {
    On(u8),
    Off(u8),
    Rest,
}

/// Two bursts of chords with a long silence between: the first rings out
/// for 10 s (500 ms of delay line, a QUAD tail and the chorus all idle by
/// then), the second wakes everything, then 2 s of its tail.
fn score() -> Vec<Step> {
    let mut s = Vec::new();
    for burst in 0..2 {
        let notes: &[u8] = if burst == 0 {
            &[48, 55, 64]
        } else {
            &[50, 57, 65]
        };
        s.extend(notes.iter().map(|&n| Step::On(n)));
        s.extend(std::iter::repeat_n(Step::Rest, 200));
        s.extend(notes.iter().map(|&n| Step::Off(n)));
        s.extend(std::iter::repeat_n(
            Step::Rest,
            if burst == 0 { 7_500 } else { 1_500 },
        ));
    }
    s
}

/// One pair's block out: its DAC words and its floats' bits.
type Out = (Vec<DacSample>, Vec<u32>);

/// Every pair's block out, every block, through the engine: `fx`, Part 1
/// sending `send` to every effect, on `pair`. A pair nothing is routed to
/// (pair 1 takes the FX return) is exactly +0 throughout.
/// From step `back`, if given, the reverb's MIX is back at its default.
fn render(fx: FxParams, send: f32, pair: DacPair, skip: bool, back: Option<usize>) -> Vec<Out> {
    NEVER_SKIP.store(!skip, Ordering::Relaxed);
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    let mut shared = Box::new(AudioShared::default());
    shared.fx = fx;
    shared.parts[0].params.out.sends = [SendLevel::new(send); 3];
    shared.parts[0].mix.output = pair;
    let ch = shared.parts[0].mix.channel;
    let mut out = Vec::new();
    for (i, step) in score().into_iter().enumerate() {
        if back == Some(i) {
            shared.fx.reverb.mix = FxParams::DEFAULT.reverb.mix;
        }
        let ev = |n: u8, kind| NoteEvent {
            channel: ch,
            note: MidiNote::new(n).unwrap(),
            kind,
        };
        match step {
            Step::On(n) => rig
                .inst
                .handle(ev(n, NoteKind::On(Velocity::DEFAULT)), &shared),
            Step::Off(n) => rig.inst.handle(ev(n, NoteKind::Off), &shared),
            Step::Rest => {
                let dac = rig.block(&shared);
                for p in DacPair::ALL {
                    let mut words = vec![DacSample::ZERO; 2 * BLOCK_SIZE];
                    interleave(dac, p, (&mut words[..]).try_into().unwrap());
                    let bits: Vec<u32> = dac[p.index()].iter().map(|s| s.to_bits()).collect();
                    if p != DacPair::P1 && p != pair {
                        assert!(bits.iter().all(|&b| b == 0), "{p:?} unrouted, not +0");
                    }
                    out.push((words, bits));
                }
            }
        }
    }
    NEVER_SKIP.store(false, Ordering::Relaxed);
    out
}

fn counts() -> [u32; 8] {
    core::array::from_fn(|k| SKIPS[k].swap(0, Ordering::Relaxed))
}

/// The unit's FZ: x86's flush-to-zero and denormals-are-zero, for `f`.
fn flushed<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_arch = "x86_64")]
    {
        #[allow(deprecated)]
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        /// Puts the thread's MXCSR back, a panic in `f` too.
        struct Restore(u32);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: writes back the value this thread's MXCSR held
                // before `flushed` set FTZ and DAZ.
                #[allow(deprecated)]
                unsafe {
                    _mm_setcsr(self.0)
                };
            }
        }
        // SAFETY: reading MXCSR has no effect.
        #[allow(deprecated)]
        let _restore = Restore(unsafe { _mm_getcsr() });
        // SAFETY: sets only MXCSR's FTZ (bit 15) and DAZ (bit 6) on this
        // thread, until `_restore` drops; no float code here depends on
        // denormals.
        #[allow(deprecated)]
        unsafe {
            _mm_setcsr(_restore.0 | 0x8040)
        };
        f()
    }
    #[cfg(not(target_arch = "x86_64"))]
    f()
}

/// Both paths, the words compared block by block; the skips' counts.
fn same(fx: FxParams, send: f32, pair: DacPair, flush: bool) -> [u32; 8] {
    same_from(fx, send, pair, flush, None)
}

/// `same`, the reverb's MIX back at its default from step `back`.
fn same_from(fx: FxParams, send: f32, pair: DacPair, flush: bool, back: Option<usize>) -> [u32; 8] {
    let run = |skip| {
        if flush {
            flushed(|| render(fx, send, pair, skip, back))
        } else {
            render(fx, send, pair, skip, back)
        }
    };
    let full = run(false);
    assert_eq!(counts(), [0; 8], "a skip ran under NEVER_SKIP");
    let skipped = run(true);
    let n = counts();
    eprintln!("skips {n:?} over {} blocks", full.len() / 3);
    assert_eq!(full.len(), skipped.len());
    for (i, (a, b)) in full.iter().zip(&skipped).enumerate() {
        assert!(
            a == b,
            "block {} pair {}: the skips changed the words or the floats",
            i / 3,
            i % 3 + 1
        );
    }
    assert!(
        full.iter().any(|(w, _)| w.iter().any(|s| s.get() != 0)),
        "silent"
    );
    n
}

fn ran(n: [u32; 8], k: Skip) -> bool {
    n[k as usize] > 0
}

/// The default effects, QUAD at DAMP 0 (higher DAMP can hold the ring's
/// DAMP state at ±1 LSB for good, so QUAD never idles there: correct, and
/// not this test's), every send live.
fn fx() -> FxParams {
    let mut f = FxParams::DEFAULT;
    f.reverb.damping = 0.0;
    f
}

#[test]
fn idle_then_woken_is_bit_for_bit_the_full_path() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    counts();
    let n = same(fx(), 0.5, DacPair::P1, false);
    // `Unheard` too: the tail gate's gain reaches exact 0 before the tail.
    for k in [
        Skip::Delay,
        Skip::Chorus,
        Skip::Ring,
        Skip::Decimator,
        Skip::Unheard,
        Skip::Pairs,
    ] {
        assert!(ran(n, k), "{k:?} never ran: {n:?}");
    }
}

#[test]
fn idle_then_woken_flushed_is_bit_for_bit_the_full_path() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    counts();
    let n = same(fx(), 0.5, DacPair::P1, true);
    for k in [
        Skip::Delay,
        Skip::Chorus,
        Skip::Ring,
        Skip::Decimator,
        Skip::Unheard,
        Skip::TailGate,
        Skip::Interpolators,
    ] {
        assert!(ran(n, k), "{k:?} never ran: {n:?}");
    }
}

/// The reverb at MIX 0 on a live send: its interpolators skipped, their
/// histories moved on, so a MIX brought back plays the same: back while
/// the first chord sounds, just after it is let go, and in the second's
/// tail.
#[test]
fn reverb_at_mix_0_is_bit_for_bit_the_full_path() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    counts();
    let mut f = fx();
    f.reverb.mix = 0.0;
    let n = same(f, 0.5, DacPair::P1, false);
    assert!(ran(n, Skip::Unheard), "{n:?}");
    for back in [100, 203, 7_850] {
        let n = same_from(f, 0.5, DacPair::P1, false, Some(back));
        assert!(ran(n, Skip::Unheard), "back at {back}: {n:?}");
    }
}

/// Part 1 on pair 2 and nothing sending: pair 1 holds only the idle
/// returns, pair 3 nothing.
#[test]
fn a_part_on_another_pair_is_bit_for_bit_the_full_path() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    counts();
    for flush in [false, true] {
        let n = same(fx(), 0.0, DacPair::P2, flush);
        for k in [
            Skip::Delay,
            Skip::Chorus,
            Skip::Ring,
            Skip::Decimator,
            Skip::TailGate,
            Skip::Interpolators,
        ] {
            assert!(ran(n, k), "{k:?} never ran: {n:?}");
        }
    }
}

/// The compressor on and the master VOL and PAN off unity: each scales
/// every pair, so no pair stays known +0 through them, and the paths still
/// match.
#[test]
fn the_master_stages_scaling_is_bit_for_bit_the_full_path() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    counts();
    for (ratio, vol, pan) in [(3, 1.0, 0.0), (0, 0.6, 0.3)] {
        let mut f = fx();
        f.comp.ratio = chimera_core::dsp::comp::Ratio::ALL[ratio];
        (f.comp.vol, f.comp.pan) = (vol, pan);
        same(f, 0.5, DacPair::P2, false);
    }
}
