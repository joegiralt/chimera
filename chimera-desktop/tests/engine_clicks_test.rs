//! The showcase QA's engine clicks, each repro played through the real `Instrument` (Part 1, after the limiter, as the
//! factory-sounds QA plays a Sound) and judged by `listen::qa_takes`, the
//! factory-sounds QA's click metric, against its `CLICK_RATIO`.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FilterParams, PitchParams, Steal};
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::{MidiNote, Velocity};
use chimera_desktop::listen::{CLICK_RATIO, qa_takes};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

const SR: f32 = SAMPLE_RATE as f32;
/// The limiter's lookahead: the output is a block late.
const LATE: usize = BLOCK_SIZE;

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
/// after the limiter, and the samples its note-ons start at there (the
/// QA's onsets: every note-on, a block late).
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
            takes.push(b * BLOCK_SIZE + LATE);
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

/// The factory-sounds QA's `glide`: C2, G2, C3, each overlapping the next.
const GLIDE: [Hit; 3] = [
    hit(0.0, 0.5, 36, 100),
    hit(0.4, 0.9, 43, 100),
    hit(0.8, 1.4, 48, 100),
];

/// Its `bass`: the home note held 0.3 s.
const BASS: [Hit; 1] = [hit(0.0, 0.3, 36, 100)];

/// `src` into `a` at `amt`, `a` registered unless a fixed column.
fn route(s: &mut Sound, src: ModSource, a: ParamAddr, amt: i8) {
    if FixedCol::of(a).is_none() {
        s.dest_registry.add(a, *b"ROUTE   ").unwrap();
    }
    assert!(s.mod_state.set_route_addr(src.index(), a, amt));
}

/// The showcase's RUBBER BASS (S9100039, task 12b) as it clicked: ALGO T5,
/// every RR 10, op 2's fast decay, a GLIDE steal, the 2040 driven.
fn rubber_bass() -> Sound {
    let mut s = Sound::neutral(EngineType::Algo);
    let p = &mut s.params;
    (p.algo.alg_a, p.algo.alg_b) = (4, 4);
    let o = &mut p.algo.ops;
    (o[0].level, o[0].velocity, o[0].d1r, o[0].d1l, o[0].d2r) = (99, 3, 4, 14, 2);
    (o[1].level, o[1].velocity, o[1].d1r, o[1].d1l, o[1].d2r) = (82, 6, 16, 6, 2);
    (o[2].coarse, o[2].level, o[2].d1r, o[2].d1l) = (0, 99, 4, 12);
    (o[3].level, o[4].level, o[4].d1r, o[4].d1l) = (0, 80, 4, 13);
    for i in [0, 1, 2, 4] {
        o[i].rr = 10;
    }
    (p.pitch.steal, p.pitch.glide_time) = (Steal::Glide, 0.6);
    p.filter.set_kind(FilterKind::K2040);
    (p.filter.cutoff, p.filter.level, p.filter.resonance) = (1400.0, 0.9, 0.45);
    let e2 = &mut p.envelopes[1];
    (e2.attack, e2.decay, e2.sustain) = (0.1, 0.4, 0.0);
    (p.lfos[0].rate, p.lfos[0].sync, p.lfos[1].rate) = (5.0, 1, 0.4);
    p.out.volume = 10f32.powf(-0.3 / 20.0);
    let op2 = ParamAddr::new(BlockRef::AlgoOp(Op::B), AlgoOpParams::LEVEL);
    let cut = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let fine = ParamAddr::new(BlockRef::Pitch, PitchParams::FINE);
    for (src, a, amt) in [
        (ModSource::Env2, op2, 30),
        (ModSource::Env2, cut, 45),
        (ModSource::Env2, fine, 90),
        (ModSource::Lfo1, op2, 18),
        (ModSource::Lfo2, cut, 15),
        (ModSource::Vel, op2, 25),
        (ModSource::Vel, cut, 20),
    ] {
        route(&mut s, src, a, amt);
    }
    s
}

/// #487: a fast release ends without a click. The voice ended on its DC
/// blocker's tail: RUBBER BASS read 3071×, INIT at RR 10 363×. RR 13 to 15
/// keep their snap: the release's own corner reads 3.5–4× on INIT's bass,
/// and only a release of 13 ms or more reads under the bar.
#[test]
fn algo_short_release_does_not_click() {
    let r = ratio(&rubber_bass(), PartMode::Poly, &BASS, 1.8);
    assert!(r <= CLICK_RATIO, "RUBBER BASS: {r:.2}×");
    for rr in [8, 10, 12] {
        let mut s = Sound::init(EngineType::Algo);
        for o in &mut s.params.algo.ops {
            o.rr = rr;
        }
        let r = ratio(&s, PartMode::Poly, &BASS, 1.8);
        assert!(r <= CLICK_RATIO, "RR {rr}: {r:.2}×");
    }
}

/// #486: ALGO on a MONO Part, legato, CUT and GLIDE at three glide times:
/// a sounding operator's AR 31 re-attack read 3.4–5.5×.
#[test]
fn algo_mono_legato_does_not_click() {
    for steal in [Steal::Cut, Steal::Glide] {
        for glide in [0.3, 0.6, 0.8] {
            let mut s = rubber_bass();
            (s.params.pitch.steal, s.params.pitch.glide_time) = (steal, glide);
            let r = ratio(&s, PartMode::Mono, &GLIDE, 2.9);
            assert!(r <= CLICK_RATIO, "{steal:?} {glide}: {r:.2}×");
        }
    }
}
