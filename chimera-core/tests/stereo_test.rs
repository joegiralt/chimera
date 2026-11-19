//! A Sound's stereo (ADR 0294): OUT PAN places each note of ALGO, MODAL,
//! VA and TAPE, set or routed, ramped across each block; WIDTH scales the
//! Part's side, eased. Centre and unity are the mix as it was, bit for bit
//! (the goldens hold every factory Sound to it). DRUM's are in
//! `drum_hits_test` (its kit writer is once a process).

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::hw::SampleBudget;
use chimera_core::modulation::{ModSource, ModState};
use chimera_core::params::{EngineType, OutParams, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::stereo::{VoicePan, Width};
use common::{InstRig, mods};

const PAN: ParamAddr = ParamAddr::new(BlockRef::Out, OutParams::PAN);

fn rig(engine: EngineType, pan: f32, width: f32, m: Option<ModState>) -> InstRig {
    let mut r = InstRig::with_budget(SampleBudget::for_cpu(u32::MAX));
    let s = Sound::init(engine);
    let mut p: ParamSnapshot = s.params;
    p.out.pan = VoicePan::new(pan);
    p.out.width = Width::new(width);
    *r.shared.parts[0].params = p;
    *r.shared.parts[0].mod_state = m.unwrap_or(s.mod_state);
    r
}

/// Pair 1's left and right over `blocks` blocks of a held C4.
fn play(r: &mut InstRig, blocks: usize) -> (Vec<f32>, Vec<f32>) {
    r.note_on(0, 60);
    let (mut l, mut rr) = (Vec::new(), Vec::new());
    for _ in 0..blocks {
        r.render(1);
        for f in r.out()[0].chunks(2) {
            l.push(f[0]);
            rr.push(f[1]);
        }
    }
    (l, rr)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// The engines the Instrument plays here: TAPE's notes need a source
/// pool, so TAPE is placed on a voice of its own (`tape_is_placed`).
const ENGINES: [EngineType; 3] = [EngineType::Algo, EngineType::Modal, EngineType::Va];

#[test]
fn centre_and_unity_are_the_mix_as_it_was() {
    for e in ENGINES {
        let (l, r) = play(&mut rig(e, 0.0, 1.0, None), 30);
        let (l2, r2) = play(&mut rig(e, 0.0, 2.0, None), 30);
        assert!(rms(&l) > 1e-4, "{e:?} sounds");
        assert_eq!(l, r, "{e:?}: centred, both sides alike");
        // A centred note writes no side: WIDTH has nothing to scale.
        assert_eq!((l, r), (l2, r2), "{e:?}");
    }
}

#[test]
fn out_pan_places_every_melodic_engine() {
    for e in ENGINES {
        let (l, r) = play(&mut rig(e, 1.0, 1.0, None), 30);
        assert!(
            rms(&l) < 1e-4 * rms(&r),
            "{e:?} hard right: {} {}",
            rms(&l),
            rms(&r)
        );
        let (l, r) = play(&mut rig(e, -1.0, 1.0, None), 30);
        assert!(rms(&r) < 1e-4 * rms(&l), "{e:?} hard left");
        // Constant power: half right is −3 dB short of hard right's side.
        let (l, r) = play(&mut rig(e, 0.5, 1.0, None), 30);
        assert!(rms(&r) > rms(&l), "{e:?} half right leans right");
    }
}

/// WIDTH 0 folds a panned note to the middle; the mono sum never moves.
#[test]
fn width_zero_is_mono_and_keeps_the_sum() {
    let warm = |w: f32| {
        let mut r = rig(EngineType::Algo, 0.6, w, None);
        // A block first: the Part's WIDTH lands while it has no side.
        r.render(1);
        r
    };
    let (l0, r0) = play(&mut warm(0.0), 40);
    let (l1, r1) = play(&mut warm(1.0), 40);
    let tail = 10 * 64..l0.len();
    for (a, b) in l0[tail.clone()].iter().zip(&r0[tail.clone()]) {
        assert!((a - b).abs() < 1e-6, "mono {a} {b}");
    }
    for i in tail {
        let (s0, s1) = (l0[i] + r0[i], l1[i] + r1[i]);
        assert!((s0 - s1).abs() < 1e-5, "sum at {i}: {s0} {s1}");
    }
}

/// An LFO routed into PAN moves the note left and right.
#[test]
fn a_route_moves_the_pan() {
    let m = mods(&[(ModSource::Lfo1, PAN, 127)]);
    let mut r = rig(EngineType::Algo, 0.0, 1.0, Some(m));
    r.shared.parts[0].params.lfos[0].rate = 2.0;
    r.shared.parts[0].params.lfos[0].depth = 1.0;
    let (l, rr) = play(&mut r, 400);
    let bal: Vec<f32> = l
        .chunks(64)
        .zip(rr.chunks(64))
        .skip(4)
        .map(|(a, b)| (rms(b) - rms(a)) / (rms(a) + rms(b) + 1e-9))
        .collect();
    assert!(rms(&l) > 1e-4, "sounds");
    let (lo, hi) = bal
        .iter()
        .fold((1.0f32, -1.0f32), |(lo, hi), &b| (lo.min(b), hi.max(b)));
    assert!(lo < -0.3 && hi > 0.3, "swings both ways: {lo} {hi}");
}

/// A WIDTH turned mid-note eases over the stage's 20 ms, never in a step.
#[test]
fn a_width_change_eases() {
    let mut r = rig(EngineType::Algo, 1.0, 1.0, None);
    let (l, _) = play(&mut r, 20);
    assert!(rms(&l[19 * 64..]) < 1e-5, "hard right");
    r.shared.parts[0].params.out.width = Width::MONO;
    let (mut first, mut later) = (Vec::new(), Vec::new());
    for b in 0..120 {
        r.render(1);
        let l: Vec<f32> = r.out()[0].chunks(2).map(|f| f[0]).collect();
        let rr: Vec<f32> = r.out()[0].chunks(2).map(|f| f[1]).collect();
        if b == 0 {
            first = vec![rms(&l), rms(&rr)];
        }
        later = vec![rms(&l), rms(&rr)];
    }
    assert!(first[0] < 0.5 * first[1], "eased, not snapped: {first:?}");
    assert!(
        (later[0] - later[1]).abs() < 1e-3 * later[1],
        "landed mono: {later:?}"
    );
}

/// AMP › STEREO: PAN · WIDTH under AMP on every melodic chain, before
/// SENDS; WIDTH on DRUM's AMP › GATE's F; each cell's words fit the band.
#[test]
fn stereo_is_amps_page_and_gates_f() {
    use chimera_core::ui::block_registry::{DRUM_GATE, SENDS, STEREO};
    use chimera_core::ui::nav::chain_def_for;
    use chimera_core::ui::{notes, theme};
    assert_eq!((STEREO.name, STEREO.short), ("Stereo", "STER"));
    let got: Vec<_> = (0..6).map(|i| STEREO.params[i].label()).collect();
    assert_eq!(got, ["PAN", "WIDTH", "--", "--", "--", "--"]);
    assert_eq!(DRUM_GATE.params[5].label(), "WIDTH");
    for e in [
        EngineType::Algo,
        EngineType::Modal,
        EngineType::Va,
        EngineType::Tape,
    ] {
        let amp = chain_def_for(e)
            .blocks
            .iter()
            .find(|b| b.def.short == "AMP")
            .expect("AMP");
        let subs: Vec<u16> = amp.sub_pages.iter().map(|d| d.id).collect();
        assert_eq!(subs, [STEREO.id, SENDS.id], "{e:?}");
    }
    let s = Sound::init(EngineType::Algo);
    let out = |id| ParamAddr::new(BlockRef::Out, id);
    assert_eq!(
        notes::cell(out(OutParams::PAN), &s),
        Some(notes::STEREO_PAN)
    );
    assert_eq!(
        notes::cell(out(OutParams::WIDTH), &s),
        Some(notes::STEREO_WIDTH)
    );
    for w in [notes::STEREO_PAN, notes::STEREO_WIDTH] {
        let px = chimera_core::ui::draw::text_width(&theme::FONT_LABEL, w, 1);
        assert!(px <= theme::CONTENT_W, "{w}: {px}");
    }
    // PAN is a knob and a destination; WIDTH the Part's, eased by its stage.
    assert!(PAN.modulatable());
    assert!(!out(OutParams::WIDTH).modulatable());
}

/// TAPE (on a voice with its own source, as the Instrument's has none
/// here): hard right, its side is its mid, every sample.
#[test]
fn tape_is_placed() {
    use chimera_core::{MidiNote, Velocity};
    use chimera_hal::BLOCK_SIZE;
    let s = Sound::init(EngineType::Tape);
    let mut p = s.params;
    p.out.pan = VoicePan::new(1.0);
    let mut v = common::rig::Rig::new(chimera_hal::SAMPLE_RATE);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::new(100).unwrap(), &p);
    let (mut heard, mut out) = (0.0f32, [0.0f32; BLOCK_SIZE]);
    for _ in 0..20 {
        let mut side = None;
        v.render_sided(&mut out, &p, &s.mod_state, &mut side);
        let side = side.expect("placed: a side");
        for (m, s) in out.iter().zip(&side) {
            assert!((m - s).abs() <= 1e-6 * m.abs().max(1e-9) + 1e-9, "{m} {s}");
            heard = heard.max(m.abs());
        }
    }
    assert!(heard > 1e-4, "TAPE sounds");
}

/// Hard right by a route (VEL → PAN, a full velocity), through `act`
/// after the first note: no block may flash into the left, as a copy of
/// the stored, unrouted PAN would (a retrigger, a mono steal, legato).
fn stays_right(act: impl Fn(&mut InstRig), setup: impl Fn(&mut InstRig)) {
    let m = mods(&[(ModSource::Vel, PAN, 127)]);
    let mut r = rig(EngineType::Algo, 0.0, 1.0, Some(m));
    setup(&mut r);
    r.note_on_vel(0, 60, 127);
    let mut blocks = Vec::new();
    for b in 0..60 {
        if b == 20 {
            act(&mut r);
        }
        r.render(1);
        let (l, rr): (Vec<f32>, Vec<f32>) = r.out()[0].chunks(2).map(|f| (f[0], f[1])).unzip();
        blocks.push((rms(&l), rms(&rr)));
    }
    assert!(blocks[30].1 > 1e-3, "sounds: {:?}", blocks[30]);
    for (b, &(l, rr)) in blocks.iter().enumerate().skip(1) {
        assert!(l <= 1e-4 * rr + 1e-9, "block {b}: left {l}, right {rr}");
    }
}

#[test]
fn a_retriggered_note_keeps_its_routed_place() {
    stays_right(|r| r.note_on_vel(0, 60, 127), |_| {});
}

#[test]
fn a_mono_steal_keeps_its_routed_place() {
    use chimera_core::part::PartMode;
    stays_right(
        |r| r.note_on_vel(0, 64, 127),
        |r| r.shared.parts[0].mix.set_mode(PartMode::Mono),
    );
}

#[test]
fn a_legato_note_keeps_its_routed_place() {
    use chimera_core::params::Steal;
    use chimera_core::part::PartMode;
    stays_right(
        |r| r.note_on_vel(0, 64, 127),
        |r| {
            r.shared.parts[0].mix.set_mode(PartMode::Mono);
            r.shared.parts[0].params.pitch.steal = Steal::Glide;
        },
    );
}

/// A released note is placed to its last block: its tail never ends
/// centred.
#[test]
fn a_note_is_placed_to_its_end() {
    let mut r = rig(EngineType::Algo, 1.0, 1.0, None);
    r.note_on(0, 60);
    r.render(10);
    r.note_off(0, 60);
    let mut heard = false;
    for b in 0..2000 {
        r.render(1);
        let (l, rr): (Vec<f32>, Vec<f32>) = r.out()[0].chunks(2).map(|f| (f[0], f[1])).unzip();
        let (l, rr) = (rms(&l), rms(&rr));
        heard |= rr > 1e-6;
        assert!(l <= 1e-4 * rr + 1e-12, "block {b}: left {l}, right {rr}");
    }
    assert!(heard);
}

/// Under MPE each note is placed on its own: TIMBRE → PAN, one note's
/// timbre up, the other's at rest.
#[test]
fn mpe_places_each_note_by_its_own_timbre() {
    use chimera_core::MidiChannel;
    use chimera_core::mpe::Zones;
    use chimera_core::note_queue::{Expr, ExprEvent};
    use chimera_midi::value::U7;
    let m = mods(&[(ModSource::Timbre, PAN, 127)]);
    let mut r = rig(EngineType::Algo, 0.0, 1.0, Some(m));
    r.shared.mpe = Zones::OFF.with_lower(15);
    r.note_on(1, 60);
    r.note_on(2, 64);
    r.inst.expr(ExprEvent {
        channel: MidiChannel::new(2).unwrap(),
        expr: Expr::Timbre(U7::new(127).unwrap()),
    });
    r.render(400);
    let played = r.inst.played(PAN);
    let slots = r.inst.allocator().slots();
    let at = |n: u8| {
        (0..slots.len())
            .find(|&v| slots[v].held() && slots[v].note().map(|k| k.get()) == Some(n))
            .map(|v| played[v])
            .unwrap()
    };
    assert_eq!(at(60), 0.0, "at rest: centred");
    assert_eq!(at(64), 1.0, "its timbre: hard right");
    let (l, rr): (Vec<f32>, Vec<f32>) = r.out()[0].chunks(2).map(|f| (f[0], f[1])).unzip();
    assert!(rms(&rr) > 1.2 * rms(&l), "{} {}", rms(&l), rms(&rr));
}

/// With the Part's own PAN off centre, panning after WIDTH moves left +
/// right with WIDTH: by `(gR − gL)·side·WIDTH`, linear in it. (Centred, it
/// never moves: `width_zero_is_mono_and_keeps_the_sum`.)
#[test]
fn an_off_centre_part_moves_the_sum_linearly_with_width() {
    let sum = |w: f32| {
        let mut r = rig(EngineType::Algo, 0.6, w, None);
        r.shared.parts[0].mix.pan = 0.5;
        // Quiet, under the output stage's limiter: the mix alone.
        r.shared.parts[0].mix.level = 0.1;
        r.render(1);
        // Past WIDTH's 20 ms ease from unity.
        let (l, rr) = play(&mut r, 160);
        l.iter()
            .zip(&rr)
            .skip(120 * 64)
            .map(|(a, b)| a + b)
            .collect::<Vec<f32>>()
    };
    let (s0, s1, s2) = (sum(0.0), sum(1.0), sum(2.0));
    let d1: Vec<f32> = s1.iter().zip(&s0).map(|(a, b)| a - b).collect();
    let d2: Vec<f32> = s2.iter().zip(&s1).map(|(a, b)| a - b).collect();
    assert!(rms(&d1) > 1e-3 * rms(&s1), "the sum moves: {}", rms(&d1));
    for (a, b) in d1.iter().zip(&d2) {
        assert!((a - b).abs() <= 1e-4 * rms(&s1), "linear: {a} {b}");
    }
}

/// Host timings for ADR 0294 (release, `--ignored --nocapture`): best of
/// five runs of 20,000 blocks of the whole instrument, one VA note held,
/// centred, at a still PAN off centre, with PAN moving every block, and
/// off centre with WIDTH 0.5.
#[test]
#[ignore]
fn host_timing() {
    let time = |pan: f32, width: f32, moving: bool| {
        let mut best = f64::MAX;
        for _ in 0..5 {
            let mut r = rig(EngineType::Va, pan, width, None);
            r.note_on(0, 60);
            r.render(10);
            let t = std::time::Instant::now();
            for b in 0..20_000 {
                if moving {
                    r.shared.parts[0].params.out.pan =
                        VoicePan::new(if b % 2 == 0 { 0.3 } else { 0.6 });
                }
                r.render(1);
            }
            best = best.min(t.elapsed().as_nanos() as f64 / 20_000.0);
        }
        best
    };
    println!("centre {:.0} ns", time(0.0, 1.0, false));
    println!("still 0.5 {:.0} ns", time(0.5, 1.0, false));
    println!("moving {:.0} ns", time(0.3, 1.0, true));
    println!("0.5 width 0.5 {:.0} ns", time(0.5, 0.5, false));
}
