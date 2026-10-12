//! The showcase QA's engine clicks, each repro played
//! through the real `Instrument` (Part 1, after the limiter, as the
//! factory-sounds QA plays a Sound) and judged by `listen::qa_takes`, the
//! factory-sounds QA's click metric, against its `CLICK_RATIO`.

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::{MidiNote, Velocity};
use chimera_desktop::listen::{CLICK_RATIO, qa_takes};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

const SR: f32 = SAMPLE_RATE as f32;

/// A note: seconds on and off, the note, its velocity.
#[derive(Clone, Copy)]
struct Hit {
    on: f32,
    off: f32,
    note: u8,
    vel: u8,
}

const fn hit(on: f32, off: f32, note: u8, vel: u8) -> Hit {
    Hit { on, off, note, vel }
}

fn block(t: f32) -> usize {
    (t * SR / BLOCK_SIZE as f32).round() as usize
}

/// `hits` played by `snd` on Part 1 in `mode` for `secs`, mono `(L+R)/2`
/// after the limiter, and the samples its note-ons from silence start at
/// (the QA's takes; a legato note-on is not one).
fn play(snd: &Sound, mode: PartMode, hits: &[Hit], secs: f32) -> (Vec<f32>, Vec<usize>) {
    let mut perf = Performance::new();
    let e = perf.edit(PartId::ALL[0]);
    **e.sound = snd.clone();
    e.mix.mode = mode;
    let shared = Box::new(AudioShared::from_performance(&perf));
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let channel = shared.parts[0].mix.channel;
    let ev = |note: u8, kind| NoteEvent {
        channel,
        note: MidiNote::new(note).unwrap(),
        kind,
    };
    let (mut out, mut takes) = (Vec::new(), Vec::new());
    for b in 0..block(secs) {
        for h in hits.iter().filter(|h| block(h.off) == b) {
            inst.handle_with(ev(h.note, NoteKind::Off), &shared, &GateView::empty());
        }
        for h in hits.iter().filter(|h| block(h.on) == b) {
            if !hits.iter().any(|o| o.on < h.on && h.on < o.off) {
                takes.push(b * BLOCK_SIZE);
            }
            let v = Velocity::new(h.vel).unwrap();
            inst.handle_with(ev(h.note, NoteKind::On(v)), &shared, &GateView::empty());
        }
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &GateView::empty());
        out.extend(dac.out()[0].chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    (out, takes)
}

/// The QA's click ratio of `hits` on `snd`.
fn ratio(snd: &Sound, mode: PartMode, hits: &[Hit], secs: f32) -> f32 {
    let (y, takes) = play(snd, mode, hits, secs);
    let q = qa_takes(&y, &takes);
    println!(
        "ratio {:.2} at {} (peak {:.1} dBFS)",
        q.click_ratio, q.click_at, q.peak_dbfs
    );
    assert!(q.peak_dbfs > -60.0, "silent render");
    q.click_ratio
}

/// Its `bass`: the home note held 0.3 s.
const BASS: [Hit; 1] = [hit(0.0, 0.3, 36, 100)];

fn algo() -> Sound {
    Sound::init(EngineType::Algo)
}

/// #487: a fast release ends without a click: the voice used to end on its
/// DC blocker's tail (363× at RR 10). RR 13 to 15 keep their snap: the
/// release's own corner reads 3.5–4× on this bass, and only a release of
/// 13 ms or more reads under the bar.
#[test]
fn algo_short_release_does_not_click() {
    for rr in [8, 10, 12] {
        let mut s = algo();
        for o in &mut s.params.algo.ops {
            o.rr = rr;
        }
        let r = ratio(&s, PartMode::Poly, &BASS, 1.8);
        assert!(r <= CLICK_RATIO, "RR {rr}: {r:.2}×");
    }
}
