//! The formant demo Sounds (spec § Demo clips): a phrase of each through
//! the sweep's `Bench`, checked clean, and rendered to WAVs for the owner.

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::kinds::OpKind;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::algo::plan::OPS;
use chimera_core::dsp::algo::tx::{detune_at, ratio_at};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::note_to_freq;
use chimera_core::factory::{Demo, demo_sound, factory_sound};
use chimera_core::modulation::MAX_MOD_SOURCES;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use common::formant::{off_grid_db, render_algo};
use common::sweep::{BPS, Bench, Patch, Tape, lufs_gated};

const DEMOS: [(Demo, &str); 4] = [
    (Demo::Choir, "choir"),
    (Demo::TalkBass, "talk_bass"),
    (Demo::Breath, "breath"),
    (Demo::Vox, "vox"),
];

/// Blocks in `ms`.
fn blocks(ms: usize) -> usize {
    ms * BPS / 1000
}

/// A note, held from block `on` to `off`.
type Note = (u8, usize, usize);

/// The bank's Sound for `d` (factory 8–11), at its tuned VOL.
fn bank(d: Demo) -> Sound {
    factory_sound(8 + d as usize).unwrap()
}

/// CHOIR's progression, 2 s a chord, each change legato: C, F, G, C.
fn progression() -> (Vec<Note>, usize) {
    let each = blocks(2000);
    let chords: [[u8; 3]; 4] = [[48, 55, 60], [53, 57, 60], [50, 55, 59], [48, 55, 64]];
    let notes = chords
        .iter()
        .enumerate()
        .flat_map(|(i, c)| c.map(|n| (n, i * each, (i + 1) * each)))
        .collect();
    (notes, 4 * each + blocks(1500))
}

/// The phrase's notes and its length in blocks (a tail past the last
/// release).
fn phrase(d: Demo) -> (Vec<Note>, usize) {
    let tail = blocks(1500);
    match d {
        Demo::Choir => progression(),
        Demo::TalkBass => {
            // Two bars at 100 BPM, a quarter 0.6 s: repeats, a rest, and two
            // legato joins (the next note on before the last lets go).
            let q = blocks(600);
            let notes = vec![
                (36, 0, q * 6 / 10),
                (36, q, q + q * 6 / 10),
                (39, 2 * q, 3 * q + 20),
                (43, 3 * q, 4 * q - 60),
                (41, 4 * q, 4 * q + q / 2),
                (41, 5 * q - q / 2, 5 * q + q / 3),
                (39, 6 * q, 7 * q + 20),
                (36, 7 * q, 8 * q - 60),
            ];
            (notes, 8 * q + tail)
        }
        Demo::Breath => {
            let (hold, step) = (blocks(1500), blocks(2000));
            let notes = [60, 62, 64, 67]
                .into_iter()
                .enumerate()
                .map(|(i, n)| (n, i * step, i * step + hold))
                .collect();
            (notes, 3 * step + hold + tail)
        }
        Demo::Vox => {
            // Four bars at 120 BPM in sixteenths: staccato repeats, held
            // notes and a legato join, (note, start, length).
            const LEAD: [(u8, usize, usize); 19] = [
                (60, 0, 2),
                (60, 3, 1),
                (67, 4, 4),
                (70, 8, 2),
                (72, 10, 6),
                (70, 16, 1),
                (67, 18, 1),
                (63, 20, 4),
                (65, 23, 5),
                (63, 28, 2),
                (62, 30, 2),
                (72, 32, 2),
                (75, 35, 1),
                (79, 36, 4),
                (77, 40, 2),
                (75, 42, 2),
                (74, 44, 2),
                (72, 46, 2),
                (67, 48, 8),
            ];
            let s = blocks(125);
            let notes = LEAD.map(|(n, a, l)| (n, a * s, (a + l) * s)).to_vec();
            (notes, 56 * s + tail)
        }
    }
}

/// `d` played through the sweep's `Bench` (the real `Instrument`, FX and
/// limiter), with the block of every note event.
fn render_phrase(d: Demo) -> (Tape, Vec<(usize, bool)>) {
    render_sound(d, bank(d))
}

/// As `render_phrase`, playing `sound` on `d`'s phrase.
fn render_sound(d: Demo, sound: Sound) -> (Tape, Vec<(usize, bool)>) {
    let (notes, end) = phrase(d);
    render_notes(sound, &notes, end, FxParams::default(), |_| None)
}

/// `notes` on `sound` for `end` blocks; `vowel(b)`, if any, is VOWEL turned
/// by hand before block `b`, published as a knob move so it glides as on
/// the device. Each block's releases go before its strikes.
fn render_notes(
    sound: Sound,
    notes: &[Note],
    end: usize,
    fx: FxParams,
    vowel: impl Fn(usize) -> Option<u8>,
) -> (Tape, Vec<(usize, bool)>) {
    let mut bench = Bench::new(Patch::AlgoInit, PartParams::default(), fx);
    bench.edit_sound(|s| *s = sound);
    bench.turn_sound(|_| {});
    let mut events: Vec<(usize, bool)> = notes
        .iter()
        .flat_map(|&(_, on, off)| [(on, true), (off, false)])
        .collect();
    events.sort();
    events.dedup();
    let mut tape = Tape::default();
    for b in 0..end {
        if let Some(v) = vowel(b).filter(|&v| v != bench.sound().params.algo.vowel) {
            bench.turn_sound(|s| s.params.algo.vowel = v);
        }
        for &(n, _, off) in notes {
            if b == off {
                bench.note(n, None);
            }
        }
        for &(n, on, _) in notes {
            if b == on {
                bench.note(n, Some(100));
            }
        }
        bench.render(&mut tape);
    }
    (tape, events)
}

/// Clicks at each note event (`Tape::click`), on pair 1.
fn event_clicks(tape: &Tape, events: &[(usize, bool)]) -> Vec<usize> {
    events
        .iter()
        .enumerate()
        .filter(|&(i, &(e, on))| {
            let prev = if i == 0 { 0 } else { events[i - 1].0 };
            let next = events.get(i + 1).map_or(usize::MAX, |n| n.0);
            tape.click(0, e, (prev, next), on).is_some()
        })
        .map(|(_, &(e, _))| e)
        .collect()
}

/// Each phrase's gated BS.1770 loudness as measured at the bank's VOLs,
/// LUFS (Ruling 9's resolution: pinned, not held to the reference).
const PHRASE_LUFS: [f32; 4] = [-8.18, -13.69, -9.38, -9.44];

#[test]
fn demo_clips_are_clean() {
    for ((d, name), want) in DEMOS.into_iter().zip(PHRASE_LUFS) {
        let (tape, events) = render_phrase(d);
        assert!(tape.finite(), "{name}: not finite");
        let peak = tape.peak(0);
        assert!(peak <= 1.0, "{name}: peak {peak}");
        let c = event_clicks(&tape, &events);
        assert!(c.is_empty(), "{name}: clicks at blocks {c:?}");
        let l = lufs_gated(&tape.out[0]);
        println!("{name}: {l:.2} LUFS gated");
        assert!((l - want).abs() <= 1.0, "{name}: {l:.2} LUFS gated");
    }
}

/// Every demo's routes move it: muting VOWEL's column changes the phrase.
#[test]
fn demo_vowels_move() {
    let vowel = ParamAddr::new(BlockRef::Algo, AlgoParams::VOWEL);
    for d in Demo::ALL {
        let mut still = bank(d);
        let col = still.mod_state.find(vowel).expect("VOWEL is routed");
        for src in 0..MAX_MOD_SOURCES {
            still.mod_state.set_amount(src, col, 0);
        }
        let (a, _) = render_phrase(d);
        let (b, _) = render_sound(d, still);
        let diff = a.out[0]
            .iter()
            .zip(&b.out[0])
            .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        assert!(diff > 1e-3, "{d:?}: VOWEL's route changes nothing ({diff})");
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

/// The owner's auditions (spec § Demo clips, UAT items 4 and 5), each
/// with its peak, gated loudness, clicks and held-note alias.
struct Clip {
    file: &'static str,
    sound: Sound,
    notes: Vec<Note>,
    end: usize,
    /// VOWEL by hand, per block.
    vowel: fn(usize) -> Option<u8>,
    /// The block a steady 2^14-sample window starts, if one is meaningful.
    alias_at: Option<usize>,
}

fn clips() -> Vec<Clip> {
    let none: fn(usize) -> Option<u8> = |_| None;
    let at = |d: Demo, alias_at| {
        let (notes, end) = phrase(d);
        (notes, end, alias_at)
    };
    let mut v: Vec<Clip> = DEMOS
        .iter()
        .map(|&(d, name)| {
            let alias = match d {
                Demo::Choir => Some(blocks(500)),
                Demo::TalkBass => Some(blocks(1250)),
                Demo::Breath | Demo::Vox => None,
            };
            let (notes, end, alias_at) = at(d, alias);
            Clip {
                file: name,
                sound: bank(d),
                notes,
                end,
                vowel: none,
                alias_at,
            }
        })
        .collect();
    // UAT 4: CHOIR's chord held, LFO 1's route muted, VOWEL by hand from U
    // to I (0 to 100) over 5 s, after half a second on U.
    let mut still = bank(Demo::Choir);
    for src in 0..MAX_MOD_SOURCES {
        still.mod_state.set_amount(src, 0, 0);
    }
    let (start, ramp) = (blocks(500), blocks(5000));
    v.push(Clip {
        file: "choir_vowel_sweep",
        sound: still,
        notes: [48, 55, 60]
            .map(|n| (n, 0, start + ramp + blocks(500)))
            .to_vec(),
        end: start + ramp + blocks(2000),
        vowel: |b| {
            let (start, ramp) = (blocks(500), blocks(5000));
            Some((100 * b.saturating_sub(start).min(ramp) / ramp) as u8)
        },
        alias_at: Some(blocks(3000)),
    });
    // UAT 5: an FM factory Sound on CHOIR's progression, at its own VOL.
    let (notes, end) = progression();
    v.push(Clip {
        file: "reference_tx_bass_progression",
        sound: factory_sound(0).unwrap(),
        notes,
        end,
        vowel: none,
        alias_at: Some(blocks(500)),
    });
    v
}

/// The energy off every sounding operator's harmonics of the notes held
/// over the window at `at`, dB: rendered dry, NOIS operators muted, the
/// routes into pitch (vibrato, drift) muted, as they move the grid, and
/// DRIVE and the filter's LEVEL at rest, as their intermodulation of a
/// chord's notes lands off it.
fn alias_db(c: &Clip, at: usize) -> f32 {
    let mut s = c.sound.clone();
    s.params.drive.drive = 0.0;
    s.params.filter.level = 0.5;
    for d in 0..s.mod_state.num_dests() {
        if s.mod_state.dest(d).block == BlockRef::Pitch {
            for src in 0..MAX_MOD_SOURCES {
                s.mod_state.set_amount(src, d, 0);
            }
        }
    }
    let mut ratios = Vec::new();
    for i in 0..OPS {
        let kind = s.params.algo.kinds().get(i);
        let o = &mut s.params.algo.ops[i];
        if kind == OpKind::Noise {
            o.level = 0;
        } else if o.level > 0 {
            ratios.push(ratio_at(o.coarse, f32::from(o.fine)) * detune_at(f32::from(o.detune)));
        }
    }
    let held: Vec<u8> = c
        .notes
        .iter()
        .filter(|&&(_, on, off)| on <= at && off >= at + (1 << 14) / BLOCK_SIZE)
        .map(|&(n, _, _)| n)
        .collect();
    assert!(!held.is_empty(), "{}: a note is held at {at}", c.file);
    let grid: Vec<f32> = held
        .iter()
        .flat_map(|&n| ratios.iter().map(move |r| note_to_freq(n) * r))
        .flat_map(|f0| {
            (1..)
                .map(move |k| k as f32 * f0)
                .take_while(|&h| h < 24_000.0)
        })
        .collect();
    let (tape, _) = render_notes(s, &c.notes, at + 300, FxParams::OFF, c.vowel);
    let left: Vec<f32> = tape.out[0].iter().step_by(2).copied().collect();
    // `out` is a block behind the strike (the limiter's lookahead).
    off_grid_db(&left[(at + 1) * BLOCK_SIZE..], &grid)
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
    println!("| file | s | peak | gated LUFS | event clicks | step clicks | alias dB |");
    for c in clips() {
        let (tape, events) = render_notes(
            c.sound.clone(),
            &c.notes,
            c.end,
            FxParams::default(),
            c.vowel,
        );
        let path = dir.join(format!("{}.wav", c.file));
        std::fs::write(&path, wav(&tape.out[0])).unwrap();
        let alias = c
            .alias_at
            .map_or("n/a (noise)".into(), |a| format!("{:.1}", alias_db(&c, a)));
        println!(
            "| {} | {:.2} | {:.3} | {:.2} | {:?} | {} | {} |",
            path.display(),
            c.end as f32 / BPS as f32,
            tape.peak(0),
            lufs_gated(&tape.out[0]),
            event_clicks(&tape, &events),
            common::clicks(&tape.out[0].iter().step_by(2).copied().collect::<Vec<_>>()).len(),
            alias
        );
    }
}
