//! The formant demo Sounds (spec § Demo clips): a phrase of each through
//! the sweep's `Bench`, checked clean, and rendered to WAVs for the owner.

mod common;

use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::tx::{detune_at, ratio_at};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::factory::{Demo, demo_sound};
use chimera_core::part::PartParams;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use common::formant::{off_grid_db, render_algo};
use common::sweep::{BPS, Bench, Patch, Tape};

const DEMOS: [(Demo, &str); 3] = [
    (Demo::Choir, "choir"),
    (Demo::TalkBass, "talk_bass"),
    (Demo::Breath, "breath"),
];

/// Blocks in `ms`.
fn blocks(ms: usize) -> usize {
    ms * BPS / 1000
}

/// A note, held from block `on` to `off`.
type Note = (u8, usize, usize);

/// The phrase's notes and its length in blocks (a tail past the last
/// release).
fn phrase(d: Demo) -> (Vec<Note>, usize) {
    let tail = blocks(1500);
    match d {
        Demo::Choir => {
            let hold = blocks(3000);
            ([48, 55, 60].map(|n| (n, 0, hold)).to_vec(), hold + tail)
        }
        Demo::TalkBass => {
            let each = blocks(400);
            let notes = [36, 39, 43]
                .into_iter()
                .enumerate()
                .map(|(i, n)| (n, i * each, (i + 1) * each))
                .collect();
            (notes, 3 * each + tail)
        }
        Demo::Breath => {
            let hold = blocks(2000);
            (vec![(60, 0, hold)], hold + tail)
        }
    }
}

/// `d` played through the sweep's `Bench` (the real `Instrument`, FX and
/// limiter), with the block of every note event.
fn render_phrase(d: Demo) -> (Tape, Vec<(usize, bool)>) {
    let mut bench = Bench::new(Patch::AlgoInit, PartParams::default(), FxParams::default());
    bench.edit_sound(|s| *s = demo_sound(d));
    let (notes, end) = phrase(d);
    let mut events: Vec<(usize, bool)> = notes
        .iter()
        .flat_map(|&(_, on, off)| [(on, true), (off, false)])
        .collect();
    events.sort();
    events.dedup();
    let mut tape = Tape::default();
    for b in 0..end {
        for &(n, on, off) in &notes {
            if b == on {
                bench.note(n, Some(100));
            }
            if b == off {
                bench.note(n, None);
            }
        }
        bench.render(&mut tape);
    }
    (tape, events)
}

#[test]
fn demo_clips_are_clean() {
    for (d, name) in DEMOS {
        let (tape, events) = render_phrase(d);
        assert!(tape.finite(), "{name}: not finite");
        let peak = tape.peak(0);
        assert!(peak <= 1.0, "{name}: peak {peak}");
        for (i, &(e, on)) in events.iter().enumerate() {
            let prev = if i == 0 { 0 } else { events[i - 1].0 };
            let next = events.get(i + 1).map_or(usize::MAX, |n| n.0);
            assert!(
                tape.click(0, e, (prev, next), on).is_none(),
                "{name}: click at block {e}"
            );
        }
    }
}

/// CHOIR's chord root, A3, held on the engine: its FRMT operators' energy
/// off their own harmonics is under −60 dB. A detuned operator brings its
/// own grid; the NOIS operators are muted, as noise is on no grid.
#[test]
fn demo_clips_do_not_alias() {
    let mut p = demo_sound(Demo::Choir).params.algo;
    let mut grid = Vec::new();
    for i in 0..OPS {
        let kind = p.kinds().get(i);
        let o = &mut p.ops[i];
        if kind == OpKind::Formant && o.level > 0 {
            let f0 = 220.0 * ratio_at(o.coarse, f32::from(o.fine)) * detune_at(f32::from(o.detune));
            grid.extend((1..).map(|k| k as f32 * f0).take_while(|&h| h < 24_000.0));
        } else {
            o.level = 0;
        }
    }
    assert!(!grid.is_empty(), "CHOIR has a FRMT operator");
    let out = render_algo(&p, 57, 800);
    let off = off_grid_db(&out[BLOCK_SIZE * 400..], &grid);
    assert!(off < -60.0, "{off} dB off the grid");
}

/// A 48 kHz 16-bit stereo WAV: the 44-byte header, written by hand.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&2u16.to_le_bytes()); // stereo
    v.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    v.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    v.extend_from_slice(&4u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for &x in samples {
        let q = (chimera_core::dsp::clampf(x, -1.0, 1.0) * 32767.0).round() as i16;
        v.extend_from_slice(&q.to_le_bytes());
    }
    v
}

#[test]
fn the_wav_header_is_well_formed() {
    let w = wav(&[0.0, 1.0, -1.0, 2.0]);
    assert_eq!(w.len(), 44 + 8);
    assert_eq!(&w[..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 8);
    assert_eq!(&w[44..], [0, 0, 255, 127, 1, 128, 255, 127]);
}

/// `FORMANT_CLIPS_DIR` if set, else `target/demo/`.
#[test]
#[ignore]
fn render_formant_demo_clips() {
    let dir = std::env::var_os("FORMANT_CLIPS_DIR").map_or_else(
        || std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/demo"),
        Into::into,
    );
    std::fs::create_dir_all(&dir).unwrap();
    for (d, name) in DEMOS {
        let (tape, _) = render_phrase(d);
        let path = dir.join(format!("{name}.wav"));
        std::fs::write(&path, wav(&tape.out[0])).unwrap();
        println!("wrote {}", path.display());
    }
}
