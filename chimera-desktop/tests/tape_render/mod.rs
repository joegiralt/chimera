//! A TAPE Sound through the whole instrument, raw levels (ab-method: no
//! normalisation), for the PLAY tests and the listening page.
#![allow(dead_code)]

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::preset::Sound;
use chimera_core::project::Project;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::{MidiNote, Velocity};
use chimera_desktop::factory_tapes::{
    Recipes, build_all, held, originals, read_recipes, tape_sound,
};
use chimera_desktop::import::tape::Built;
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use std::sync::OnceLock;

pub const SR: f32 = SAMPLE_RATE as f32;

/// `(seconds, note, on)`.
pub type Events = Vec<(f32, u8, bool)>;

/// The factory set, built once.
pub fn set() -> &'static (Recipes, Vec<Option<Built>>) {
    static SET: OnceLock<(Recipes, Vec<Option<Built>>)> = OnceLock::new();
    SET.get_or_init(|| {
        let r = read_recipes().unwrap();
        let b = build_all(&r, &originals()).unwrap();
        (r, b)
    })
}

/// A factory source by name.
pub fn source(name: &str) -> Option<&'static Built> {
    let (r, b) = set();
    r.sources
        .iter()
        .zip(b)
        .find(|(x, _)| x.name.as_str() == name)
        .and_then(|(_, b)| b.as_ref())
}

/// A factory Sound by name, and its source.
pub fn factory(name: &str) -> Option<(Sound, &'static Built)> {
    let (r, _) = set();
    let s = r.sounds.iter().find(|s| s.name.as_str() == name)?;
    let b = source(s.source.as_str())?;
    Some((tape_sound(s, &b.head), b))
}

/// `snd` on `b` over `secs`, the events through P1: the DAC's first pair,
/// interleaved L R, after the limiter.
pub fn render(snd: &Sound, b: &Built, events: &[(f32, u8, bool)], secs: f32) -> Vec<f32> {
    let (mut p, _) = Project::boxed();
    let id = p.source_store(b.head.sref, &held(b)).unwrap();
    let mut tok = p.pool().table().reader().unwrap();
    let mut shared = Box::<AudioShared>::default();
    *shared.parts[0].params = snd.params.clone();
    *shared.parts[0].mod_state = snd.mod_state.clone();
    shared.parts[0].source = Some(id);
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    inst.land_knobs();
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let ch = shared.parts[0].mix.channel;
    let mut out = Vec::new();
    let n = (secs * SR) as usize / BLOCK_SIZE;
    let mut next = 0;
    let mut ev = events.to_vec();
    ev.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for blk in 0..n {
        let t = (blk * BLOCK_SIZE) as f32 / SR;
        let view = tok.begin_block();
        while next < ev.len() && ev[next].0 <= t {
            let (_, note, on) = ev[next];
            let kind = if on {
                NoteKind::On(Velocity::DEFAULT)
            } else {
                NoteKind::Off
            };
            inst.handle_with(
                NoteEvent {
                    channel: ch,
                    note: MidiNote::new(note).unwrap(),
                    kind,
                },
                &shared,
                &view,
            );
            next += 1;
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &view);
        drop(view);
        tok.store_sounding(inst.source_mask());
        out.extend_from_slice(&dac.out()[0]);
    }
    out
}

/// `(L + R) / 2` of an interleaved render.
pub fn mono(x: &[f32]) -> Vec<f32> {
    x.chunks(2).map(|f| 0.5 * (f[0] + f[1])).collect()
}

/// FNV-1a over the samples' bits.
pub fn hash(x: &[f32]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for v in x {
        for b in v.to_bits().to_le_bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
    }
    h
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// RMS of each `ms` frame of a mono render.
pub fn frames(x: &[f32], ms: f32) -> Vec<f32> {
    x.chunks((ms * SR / 1000.0) as usize).map(rms).collect()
}

/// The RMS of `[t0, t1)` s of a mono render, dB.
pub fn level_in(x: &[f32], t0: f32, t1: f32) -> f32 {
    let (a, b) = ((t0 * SR) as usize, ((t1 * SR) as usize).min(x.len()));
    db(rms(&x[a.min(b)..b]))
}

/// A chord on at `on`, off at `off`.
pub fn chord(notes: &[u8], on: f32, off: f32) -> Events {
    let mut e: Events = notes.iter().map(|&n| (on, n, true)).collect();
    e.extend(notes.iter().map(|&n| (off, n, false)));
    e
}

/// A short phrase: single notes with key-ups, an overlap, then a chord.
pub fn phrase(r: u8) -> Events {
    let mut e = vec![
        (0.0, r, true),
        (0.45, r, false),
        (0.5, r + 4, true),
        (0.7, r + 4, false),
        (0.8, r + 7, true),
        (1.0, r + 12, true),
        (1.2, r + 12, false),
        (2.0, r + 7, false),
    ];
    e.extend(chord(&[r - 12, r - 5, r], 2.2, 3.2));
    e
}

/// A 32-bit float WAV, stereo when `ch` is 2.
pub fn write_wav(path: &std::path::Path, x: &[f32], ch: u16) {
    let spec = hound::WavSpec {
        channels: ch,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for &v in x {
        w.write_sample(v).unwrap();
    }
    w.finalize().unwrap();
}
