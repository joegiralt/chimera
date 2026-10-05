//! BANK re-tunes its low modes every block and the rest in turn (ADR 0096):
//! a fresh note is the unstaggered bank's from its first sample, a held
//! one strays from it by little, and a swept macro never clicks.

mod common;

use chimera_core::addr::BlockRef;
use chimera_core::dsp::modal::{
    BankModes, ModalEngine, ModalParams, ResonatorMode, SymPool, stagger,
};
use chimera_core::params::PitchParams;
use chimera_hal::BLOCK_SIZE;
use common::SR;
use common::sweep::{Bench, CLICK_FLOOR, CLICK_RATIO, Param, Patch, Tape, registry};

fn bank(modes: BankModes) -> ModalParams {
    ModalParams {
        mode: ResonatorMode::Modal,
        modes,
        ..Default::default()
    }
}

/// `notes` struck at once and summed, `blocks` long, each block's
/// parameters and pitch ratio from `at`; unstaggered when `flat`.
fn render(
    notes: &[u8],
    on: &ModalParams,
    blocks: usize,
    flat: bool,
    at: &dyn Fn(usize) -> (ModalParams, f32),
) -> Vec<f32> {
    stagger::set_off(flat);
    let mut pool = SymPool::boxed();
    let mut engines: Vec<Box<ModalEngine>> = notes
        .iter()
        .map(|&n| {
            let mut e = Box::new(ModalEngine::new_in(&mut pool, ResonatorMode::Modal));
            e.note_on(n, 100, on, SR, &mut pool);
            e
        })
        .collect();
    let mut out = vec![0.0f32; blocks * BLOCK_SIZE];
    for (b, chunk) in out.as_chunks_mut::<BLOCK_SIZE>().0.iter_mut().enumerate() {
        let (p, ratio) = at(b);
        for e in &mut engines {
            let mut buf = [0.0; BLOCK_SIZE];
            e.set_pitch(ratio);
            e.render(&mut buf, &p, SR, &mut pool);
            for (o, x) in chunk.iter_mut().zip(buf) {
                *o += x;
            }
        }
    }
    stagger::set_off(false);
    out
}

/// A parameter swept from 1 to 0.2 and back over `blocks`, the rest at
/// `base`.
fn swept(base: ModalParams, which: usize, b: usize, blocks: usize) -> (ModalParams, f32) {
    let t = (b as f32 / blocks as f32).min(1.0);
    let v = 0.2 + 0.8 * (2.0 * t - 1.0).abs();
    let mut p = base;
    let mut ratio = 1.0;
    match which {
        0 => p.structure = v,
        1 => p.bright = v,
        2 => p.damp = v,
        3 => p.pos = v,
        // Up an octave and back: the modes' frequencies.
        _ => ratio = libm::exp2f(1.0 - (2.0 * t - 1.0).abs()),
    }
    (p, ratio)
}

/// C1 to G2, and C3 to C4.
const LOW: [u8; 4] = [24, 31, 36, 43];
const MID: [u8; 4] = [48, 52, 55, 60];

const SWEPT: [&str; 5] = ["STRUCTURE", "BRIGHT", "DAMP", "POS", "PITCH"];

/// A note's first block takes every mode's coefficients from its own
/// macros, as the unstaggered bank does: bit for bit, at every MODES, with
/// the first block's macros moved off the note-on's (a route) and after an
/// earlier note has turned the rotation.
#[test]
fn a_fresh_note_is_unstaggered_from_its_first_sample() {
    for modes in [
        BankModes::M16,
        BankModes::M24,
        BankModes::M32,
        BankModes::M48,
    ] {
        let on = bank(modes);
        let moved = ModalParams {
            structure: 0.8,
            bright: 0.9,
            damp: 0.3,
            ..on
        };
        for at in [on, moved] {
            let first = |flat| render(&[48], &on, 1, flat, &|_| (at, 1.0));
            assert_eq!(first(false), first(true), "{modes:?}");
        }
        // A second note, a fifth up, on the same engine: the rotation is
        // three blocks on; both runs ring the first note alike.
        let twice = |flat: bool| {
            stagger::set_off(true);
            let mut pool = SymPool::boxed();
            let mut e = Box::new(ModalEngine::new_in(&mut pool, ResonatorMode::Modal));
            e.note_on(48, 100, &on, SR, &mut pool);
            let mut buf = [0.0; BLOCK_SIZE];
            for _ in 0..3 {
                e.render(&mut buf, &on, SR, &mut pool);
            }
            stagger::set_off(flat);
            e.note_on(55, 100, &on, SR, &mut pool);
            e.render(&mut buf, &moved, SR, &mut pool);
            stagger::set_off(false);
            buf
        };
        assert_eq!(twice(false), twice(true), "{modes:?}, a second note");
    }
}

/// A held chord whose macros and pitch stay put, its first block's moved
/// off the note-on's: the staggered bank is the unstaggered one bit for
/// bit, a second long. Only a moving macro or pitch can tell them apart.
#[test]
fn a_still_chord_is_the_unstaggered_bank_exactly() {
    let chord = [48, 52, 55, 60];
    for modes in [BankModes::M32, BankModes::M48] {
        let on = bank(modes);
        let at = |_| {
            (
                ModalParams {
                    structure: 0.9,
                    ..on
                },
                1.0,
            )
        };
        let (a, f) = (
            render(&chord, &on, 750, false, &at),
            render(&chord, &on, 750, true, &at),
        );
        assert!(a == f, "{modes:?}");
    }
}

/// The DFT's length: 21 ms at 48 kHz, 47 Hz a bin.
const N: usize = 1024;

/// `x`'s magnitude spectrum under a Hann window, `N` long.
fn spectrum(x: &[f32], (cos, sin): (&[f32], &[f32])) -> Vec<f32> {
    let w: Vec<f32> = (0..N).map(|i| x[i] * (0.5 - 0.5 * cos[i % N])).collect();
    (0..N / 2)
        .map(|k| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (i, v) in w.iter().enumerate() {
                let j = (i * k) % N;
                re += v * cos[j];
                im -= v * sin[j];
            }
            re.hypot(im)
        })
        .collect()
}

/// The octave bands' levels, dB: bins 2..4 (94 to 188 Hz), 4..8, … 256..512.
fn bands(s: &[f32]) -> Vec<f32> {
    (1..9)
        .map(|k| {
            let e: f32 = s[1 << k..2 << k].iter().map(|x| x * x).sum();
            10.0 * e.max(1e-30).log10()
        })
        .collect()
}

/// The most any octave band within 20 dB of the frame's loudest differs
/// between the two spectra, dB.
fn band_error(a: &[f32], f: &[f32]) -> f32 {
    let (ba, bf) = (bands(a), bands(f));
    let top = bf.iter().copied().fold(f32::MIN, f32::max);
    ba.iter()
        .zip(&bf)
        .filter(|(_, y)| **y > top - 20.0)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// A band may move this much, dB, whatever the sweep: about a level's
/// just-noticeable difference.
const MAX_BAND_DB: f32 = 1.0;

/// A render's spectra, frame by frame from 5 ms on.
fn frames(x: &[f32], tables: (&[f32], &[f32])) -> Vec<Vec<f32>> {
    (BLOCK_SIZE * 4..x.len() - N)
        .step_by(N)
        .map(|at| spectrum(&x[at..], tables))
        .collect()
}

/// The most any frame's octave bands differ, dB.
fn worst_band(a: &[Vec<f32>], f: &[Vec<f32>]) -> f32 {
    a.iter()
        .zip(f)
        .map(|(a, f)| band_error(a, f))
        .fold(0.0, f32::max)
}

/// A held chord, low and mid, each macro and the pitch swept under it, 1 to
/// 0.2 and back over half a second (an octave up and back for the pitch):
/// through the sweep and a sixth of a second after, the stagger moves no octave
/// band within 20 dB of a frame's loudest further than `MAX_BAND_DB`, or
/// than the same sweep a block (1.3 ms) late moves it. Levels alone: a
/// mode a block or three late shifts its phase, which no one hears.
#[test]
fn a_swept_chord_keeps_the_unstaggered_spectrum() {
    let table = |f: fn(f32) -> f32| -> Vec<f32> {
        (0..N)
            .map(|i| f(core::f32::consts::TAU * i as f32 / N as f32))
            .collect()
    };
    let (cos, sin) = (table(f32::cos), table(f32::sin));
    let (sweep, blocks) = (375, 500);
    let t = (&cos[..], &sin[..]);
    for (chord, modes) in [LOW, MID]
        .into_iter()
        .flat_map(|c| [BankModes::M32, BankModes::M48].map(|m| (c, m)))
    {
        let on = bank(modes);
        for (which, name) in SWEPT.iter().enumerate() {
            let at = |b| swept(on, which, b, sweep);
            let late = |b: usize| swept(on, which, b.saturating_sub(1), sweep);
            let spectra = |flat, at: &dyn Fn(usize) -> (ModalParams, f32)| {
                frames(&render(&chord, &on, blocks, flat, at), t)
            };
            let flat = spectra(true, &at);
            let staggered = spectra(false, &at);
            // The high modes do lag: a moving STRUCTURE is heard.
            if which == 0 {
                assert!(staggered != flat, "{chord:?} {modes:?}: never staggered");
            }
            let staggered = worst_band(&staggered, &flat);
            let a_block_late = worst_band(&spectra(true, &late), &flat);
            println!(
                "{chord:?} {modes:?} {name}: a band moves {staggered:.1} dB, a block late {a_block_late:.1}"
            );
            assert!(
                staggered <= MAX_BAND_DB.max(a_block_late),
                "{chord:?} {modes:?} {name}: {staggered:.1} dB, a block late {a_block_late:.1}"
            );
        }
    }
}

/// Block `b`'s second difference, at most, on pair 1.
fn d2s(tape: &Tape) -> Vec<f32> {
    (1..tape.blocks()).map(|b| tape.d2(0, b, b + 1)).collect()
}

fn param(block: BlockRef, id: chimera_core::block::ParamId) -> Param {
    registry(false)
        .into_iter()
        .find(|p| p.is(block, id))
        .unwrap()
}

/// Each macro and FINE swept from its top to its bottom under a held chord on the real
/// mix path, at the UI's rate: no block's step is a click by the sweep's
/// own measure (`CLICK_RATIO` over the blocks around it, over
/// `CLICK_FLOOR`).
#[test]
fn a_swept_bank_never_clicks() {
    let swept = [
        param(BlockRef::Modal, ModalParams::STRUCTURE),
        param(BlockRef::Modal, ModalParams::BRIGHT),
        param(BlockRef::Modal, ModalParams::DAMP),
        param(BlockRef::Modal, ModalParams::POS),
        param(BlockRef::Pitch, PitchParams::FINE),
    ];
    for p in swept {
        let mut bench = Bench::new(
            Patch::ModalInit(ResonatorMode::Modal),
            Default::default(),
            Default::default(),
        );
        // Down from the top: DAMP 0 would silence the chord first.
        let (from, to) = (p.spec.max, p.spec.min);
        bench.set(&p, from);
        let mut tape = Tape::default();
        for n in [48, 52, 55, 60] {
            bench.note(n, Some(100));
        }
        let (start, len) = (150, 300);
        for b in 0..start + len + 100 {
            if (start..start + len).contains(&b) {
                let t = (b - start) as f32 / len as f32;
                bench.set(&p, from + t * (to - from));
            }
            bench.render(&mut tape);
        }
        let d2 = d2s(&tape);
        let mut worst = 0.0f32;
        for b in start..start + len {
            let around = |r: std::ops::Range<usize>| d2[r].iter().copied().fold(0.0, f32::max);
            let steady = around(b - 24..b - 1).max(around(b + 5..b + 28));
            let trans = around(b..b + 4);
            worst = worst.max(trans / steady.max(1e-9));
            assert!(
                !(trans > CLICK_FLOOR && trans > CLICK_RATIO * steady),
                "{}: a click at block {b}, {trans} over {steady}",
                p.name()
            );
        }
        println!("{}: worst step {worst:.2} × its surround", p.name());
        assert!(
            worst > 0.5,
            "{}: the chord rings through the sweep",
            p.name()
        );
    }
}
