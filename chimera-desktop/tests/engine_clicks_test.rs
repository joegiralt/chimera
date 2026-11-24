// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The showcase QA's engine clicks, each repro played through the real `Instrument` (Part 1, after the limiter, as the
//! factory-sounds QA plays a Sound) and judged by `listen::qa_takes`, the
//! factory-sounds QA's click metric, against its `CLICK_RATIO`.

use chimera_core::addr::{BlockRef, Op, ParamAddr};
use chimera_core::dsp::algo::algorithms::AlgoId;
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::va::VaMode;
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
use chimera_desktop::listen::{CLICK_RATIO, click_exempt, qa_takes};
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
    e.mix.set_mode(mode);
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
    p.algo.try_set_algs(AlgoId::T5, AlgoId::T5).unwrap();
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
/// blocker's tail: RUBBER BASS read 3071×, INIT at RR 10 363×. RR 12 to 15
/// keep their snap: the release's own corner, which INIT's fully open LP4
/// passes (ADR 0327), reads 3.5–10.8× on INIT's bass (owner, 2026-11-23:
/// inaudible, accepted).
#[test]
fn algo_short_release_does_not_click() {
    let r = ratio(&rubber_bass(), PartMode::Poly, &BASS, 1.8);
    assert!(r <= CLICK_RATIO, "RUBBER BASS: {r:.2}×");
    for rr in [8, 10, 11] {
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

/// Up two octaves of a major seventh and back: the factory-sounds QA's
/// `texture`, sixteenths at 120 BPM, velocity 40 rising to 127.
fn texture(home: u8) -> Vec<Hit> {
    const STEPS: [u8; 16] = [0, 4, 7, 11, 12, 16, 19, 23, 24, 23, 19, 16, 12, 11, 7, 4];
    STEPS
        .iter()
        .enumerate()
        .map(|(i, &d)| {
            let on = i as f32 * 0.125;
            hit(
                on,
                on + 0.1,
                home + d,
                40 + (87.0 * i as f32 / 15.0).round() as u8,
            )
        })
        .collect()
}

/// The showcase's FADED HYMN (S9100002) dry: VA sines ENV 2 sweeps toward
/// saws, a 2 s release.
fn faded_hymn(release: f32) -> Sound {
    let mut s = Sound::neutral(EngineType::Va);
    let p = &mut s.params;
    p.out.volume = 0.25;
    let e1 = &mut p.envelopes[0];
    (e1.attack, e1.sustain, e1.release) = (0.65, 1.0, release);
    (p.va.wave, p.va.stability) = ([0.0; 2], 0.5);
    p.filter.set_kind(FilterKind::K2040);
    p.filter.cutoff = 2400.0;
    let e2 = &mut p.envelopes[1];
    (e2.speed, e2.attack, e2.sustain) = (chimera_core::dsp::modulator::EnvSpeed::Slow, 0.55, 1.0);
    p.lfos[2].shape = 4;
    p.lfos[2].rate = 6.0;
    let va = |id| ParamAddr::new(BlockRef::Va, id);
    use chimera_core::dsp::va::VaParams as V;
    let vca = ParamAddr::new(BlockRef::Out, chimera_core::params::OutParams::VCA);
    route(&mut s, ModSource::Env1, vca, 127);
    route(&mut s, ModSource::Env2, va(V::WAVE1), 85);
    route(&mut s, ModSource::Env2, va(V::WAVE2), 85);
    route(&mut s, ModSource::Lfo3, va(V::SHAPE1), 20);
    s
}

/// #483: FADED HYMN's `texture` at REL 0.8 (9.8× in the QA) and 0.5. Its
/// sixteenths re-strike keys still ringing at a new velocity, and AMP's VEL
/// stepped the VCA by their ratio.
#[test]
fn va_restrike_at_a_new_velocity_does_not_click() {
    for rel in [0.5, 0.8] {
        let r = ratio(&faded_hymn(rel), PartMode::Poly, &texture(48), 3.5);
        assert!(r <= CLICK_RATIO, "REL {rel}: {r:.2}×");
    }
}

/// The showcase's GLIDE BASS (S9100006) with SHAPE1 at 0.3: VA saws, SUB
/// 0.2, a MONO GLIDE steal of 80 ms, the 2040 swept by ENV 2, LFO 1 on
/// WAVE1.
fn glide_bass(shape: f32, sub: f32) -> Sound {
    let mut s = Sound::neutral(EngineType::Va);
    let p = &mut s.params;
    p.out.volume = 0.25;
    let e1 = &mut p.envelopes[0];
    (e1.attack, e1.decay, e1.sustain, e1.release) = (0.1, 0.42, 0.7, 0.3);
    (p.va.shape[0], p.va.sub, p.va.stability) = (shape, sub, 0.2);
    (p.pitch.steal, p.pitch.glide_time) = (Steal::Glide, 0.5765);
    p.filter.set_kind(FilterKind::K2040);
    p.filter.cutoff = 1000.0;
    let e2 = &mut p.envelopes[1];
    (e2.decay, e2.sustain) = (0.38, 0.0);
    p.lfos[0].rate = 2.0;
    use chimera_core::dsp::va::VaParams as V;
    let vca = ParamAddr::new(BlockRef::Out, chimera_core::params::OutParams::VCA);
    let cut = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    route(&mut s, ModSource::Env1, vca, 127);
    route(&mut s, ModSource::Env2, cut, 40);
    route(
        &mut s,
        ModSource::Lfo1,
        ParamAddr::new(BlockRef::Va, V::WAVE1),
        25,
    );
    s
}

/// #485: a MONO glide with SHAPE1 above 0 and SUB above 0 read 6.77× (SUB
/// 0: 1.91×). The shaped saw's main edge sits s²/8 of a cycle before the
/// wrap, where the sub flipped: a narrow notch each cycle. The sub now flips
/// at the shaped edge; in SYNC at the louder of the master's edge and the
/// slave's reset (AMOUNT 0: 30× at the wrap; 1: 7× at the edge).
#[test]
fn va_shaped_glide_with_sub_does_not_click() {
    for shape in [0.3, 0.6, 1.0] {
        for sub in [0.2, 0.5] {
            let r = ratio(&glide_bass(shape, sub), PartMode::Mono, &GLIDE, 2.0);
            assert!(r <= CLICK_RATIO, "SHAPE1 {shape} SUB {sub}: {r:.2}×");
        }
    }
    for amount in [0.0, 0.45, 0.55, 1.0] {
        let mut s = glide_bass(0.3, 0.2);
        let va = &mut s.params.va;
        (va.mode, va.amount, va.coarse[1]) = (VaMode::Sync, amount, 7);
        let r = ratio(&s, PartMode::Mono, &GLIDE, 2.0);
        assert!(r <= CLICK_RATIO, "SYNC AMOUNT {amount}: {r:.2}×");
    }
}

/// INIT VA (two saws, FINE2 +6, the 2040) in `mode`, AMOUNT full.
fn ring_saws(mode: VaMode) -> Sound {
    let mut s = Sound::init(EngineType::Va);
    (s.params.va.mode, s.params.va.amount) = (mode, 1.0);
    s
}

/// #488: RING on two saws reads 17× in the showcase QA (8× here, C2's
/// first cycle). The product of two edged waves pulses where both edges
/// meet, and the ideal additive-saw reference pulses the same: it is
/// kept as RING's character (2026-10-13), so the QA's click gate
/// leaves RING to the ear.
#[test]
fn va_ring_is_left_to_the_ear() {
    let ring = ring_saws(VaMode::Ring);
    let r = ratio(&ring, PartMode::Poly, &[hit(0.0, 0.8, 36, 100)], 1.2);
    assert!(
        r > CLICK_RATIO,
        "RING's pulse is gone ({r:.2}×): drop the exemption"
    );
    assert!(click_exempt(&ring).is_some());
    for mode in [VaMode::Mix, VaMode::Sync] {
        assert_eq!(click_exempt(&ring_saws(mode)), None, "{mode:?}");
    }
}
