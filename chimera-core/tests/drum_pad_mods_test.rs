//! DRUM slice 2 (ADR 0269): each pad's mod envelope, LFO and four routes
//! move its hit's CUTOFF, RES, VOLUME and its own LFO, latched at the hit.
//!
//! The kits reach the audio through one global view (`KIT_VIEW`): every
//! test holds its writer for its whole run, so they play one at a time.

mod common;

use std::sync::{LazyLock, Mutex, MutexGuard};

use chimera_core::dsp::Unit;
use chimera_core::dsp::drum::sound::{
    Amount, EnvDecay, EnvPos, PadEnv, PadRoute, PadRoutes, PadSound, RouteDest, RouteOffsets,
    RouteSource, RouteValues,
};
use chimera_core::dsp::drum::types::{Hz, Kit, Knob, KnobValue, Machine, PadId};
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::dsp::engines::EngineSlot;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::modulator::EnvSpeed;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
use chimera_core::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource, ModState};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::{EngineType, FILTER_SPECS, FilterParams, ParamSnapshot};
use chimera_core::project::PartId;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use chimera_midi::value::U7;
use common::{InstRig, SR};

static WRITER: LazyLock<Mutex<KitWriter>> =
    LazyLock::new(|| Mutex::new(KitWriter::take().expect("the kit writer, once")));

const P1: PartId = PartId::ALL[0];

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).expect("pads 1–12")
}

fn u7(v: u8) -> U7 {
    U7::new(v).expect("0..=127")
}

fn amt(a: i8) -> Amount {
    Amount::new(a).expect("−127..=127")
}

fn route(s: RouteSource, d: RouteDest, a: i8) -> PadRoute {
    PadRoute::new(s, d, amt(a))
}

fn routes(r: [PadRoute; 4]) -> PadRoutes {
    PadRoutes::new(r)
}

/// Part 1's Sound: DRUM INIT, CLEAN at 1000 Hz.
fn sound() -> ParamSnapshot {
    let mut p = ParamSnapshot::for_engine(EngineType::Drum);
    p.filter.cutoff = 1000.0;
    p
}

/// Every pad on Part 1's Sound's FILTER and AMP; pad 1 with `r`.
fn kit(r: PadRoutes) -> Kit {
    let mut k = Kit::DEFAULT;
    let s = PadSound::from_sound(&sound());
    for p in PadId::ALL {
        *k.pad_mut(p).sound_mut() = s;
    }
    *k.pad_mut(pad(1)).sound_mut().routes_mut() = r;
    k
}

fn rig(kit: &Kit, part: &ParamSnapshot) -> (MutexGuard<'static, KitWriter>, InstRig) {
    let mut w = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    for p in PartId::ALL {
        w.publish(p, &Kit::DEFAULT);
        let _ = w.take_hit(p);
    }
    w.publish(P1, kit);
    let mut r = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    for (i, p) in r.shared.parts.iter_mut().enumerate() {
        p.mix.channel = MidiChannel::new(if i == 0 { 0 } else { 15 }).unwrap();
    }
    *r.shared.parts[0].params = part.clone();
    (w, r)
}

/// Pad 1 hit at `vel`.
fn hit(r: &mut InstRig, vel: u8) {
    let ev = NoteEvent {
        channel: MidiChannel::new(0).unwrap(),
        note: MidiNote::new(0).unwrap(),
        kind: NoteKind::On(Velocity::new(vel).unwrap()),
    };
    r.inst.handle(ev, &r.shared);
}

/// The active voice sounding pad 1's hit.
fn voice(r: &InstRig) -> Option<usize> {
    (0..MAX_VOICES).find(|&v| {
        let voice = r.inst.voice(v);
        voice.is_active()
            && matches!(voice.slot(), EngineSlot::Drum(d) if d.hit().is_some_and(|h| h.pad() == pad(1)))
    })
}

fn cutoff(r: &InstRig) -> f32 {
    r.inst
        .voice(voice(r).expect("pad 1 sounds"))
        .played_filter()
        .cutoff
}

fn blocks(ms: usize) -> usize {
    ms * SR as usize / 1000 / BLOCK_SIZE
}

#[test]
fn offsets_sum_by_dest() {
    use RouteDest::*;
    use RouteSource::*;
    let r = routes([
        route(ModEnv, Cutoff, 127),
        route(Lfo, Cutoff, -64),
        route(Vel, Reso, 32),
        PadRoute::OFF,
    ]);
    let v = RouteValues::new([1.0, 0.5, 1.0, 0.0, 0.0, 0.0]);
    let o = r.offsets(&v);
    assert!(
        (o.cutoff() - (1.0 - 64.0 / 127.0 * 0.5)).abs() < 1e-6,
        "{o:?}"
    );
    assert!((o.reso() - 32.0 / 127.0).abs() < 1e-6, "{o:?}");
    assert_eq!(o.volume(), Unit::ONE, "{o:?}");
    assert_eq!((o.lfo_speed(), o.lfo_depth()), (0.0, 0.0));
}

#[test]
fn no_routes_no_offsets() {
    let r = *PadSound::DEFAULT.routes();
    for v in [[0.0; 6], [1.0; 6], [-1.0, 0.3, 0.7, -0.2, 1.0, 0.5]] {
        assert_eq!(r.offsets(&RouteValues::new(v)), RouteOffsets::NONE);
    }
}

/// The values a route reads, each by its source.
#[test]
fn values_read_by_source() {
    let v = RouteValues::new([0.1, 0.2, 0.3, 0.4, 0.5, 0.6]);
    for (i, s) in RouteSource::ALL.into_iter().enumerate() {
        let r = routes([
            route(s, RouteDest::LfoDepth, 127),
            PadRoute::OFF,
            PadRoute::OFF,
            PadRoute::OFF,
        ]);
        assert_eq!(r.offsets(&v).lfo_depth(), (i + 1) as f32 / 10.0);
    }
}

#[test]
fn mod_env_opens_the_filter() {
    let r = routes([
        route(RouteSource::ModEnv, RouteDest::Cutoff, 127),
        PadRoute::OFF,
        PadRoute::OFF,
        PadRoute::OFF,
    ]);
    let mut k = kit(r);
    let s = k.pad_mut(pad(1)).sound_mut();
    s.filter_mut().set_kind(FilterKind::Ladder);
    s.filter_mut().set_cutoff(Hz::new(200.0));
    *s.env_mut() = PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Time(EnvPos::new(u7(40))),
        EnvSpeed::Med,
    );
    // The kernel rings past 1 s.
    let p = k.pad_mut(pad(1));
    p.set_machine(Machine::PlainSine);
    p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
    let (_w, mut r) = rig(&k, &sound());
    hit(&mut r, 100);
    r.render(1);
    let first = cutoff(&r);
    assert!(first > 800.0, "block 1: {first}");
    r.render(blocks(1000) - 1);
    let late = cutoff(&r);
    assert!((late - 200.0).abs() < 2.0, "1 s: {late}");
}

#[test]
fn part_lfos_reach_pads() {
    let r = routes([
        route(RouteSource::Lfo1, RouteDest::Cutoff, 64),
        PadRoute::OFF,
        PadRoute::OFF,
        PadRoute::OFF,
    ]);
    let mut part = sound();
    part.lfos[0].rate = 5.0;
    let (_w, mut rg) = rig(&kit(r), &part);
    hit(&mut rg, 100);
    let mut seen: Vec<f32> = Vec::new();
    for _ in 0..blocks(100) {
        rg.render(1);
        let c = cutoff(&rg);
        if !seen.contains(&c) {
            seen.push(c);
        }
    }
    let (lo, hi) = seen
        .iter()
        .fold((f32::MAX, 0.0f32), |(l, h), &c| (l.min(c), h.max(c)));
    assert!(seen.len() >= 2 && hi > lo * 1.5, "{seen:?}");
}

/// The pad's LFO moves the filter, and a route on its SPEED speeds it up.
#[test]
fn pad_lfo_and_its_speed() {
    let crossings = |r: PadRoutes| {
        let mut k = kit(r);
        k.pad_mut(pad(1)).sound_mut().lfo_mut().set_speed(u7(40));
        let p = k.pad_mut(pad(1));
        p.set_machine(Machine::PlainSine);
        p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
        let (_w, mut rg) = rig(&k, &sound());
        hit(&mut rg, 100);
        let mut n = 0;
        let mut last = None;
        for _ in 0..blocks(500) {
            rg.render(1);
            let up = cutoff(&rg) > 1000.0;
            n += usize::from(last.is_some_and(|l| l != up));
            last = Some(up);
        }
        n
    };
    use RouteDest::*;
    use RouteSource::*;
    let slow = crossings(routes([
        route(Lfo, Cutoff, 64),
        PadRoute::OFF,
        PadRoute::OFF,
        PadRoute::OFF,
    ]));
    let fast = crossings(routes([
        route(Lfo, Cutoff, 64),
        route(Vel, LfoSpeed, 64),
        PadRoute::OFF,
        PadRoute::OFF,
    ]));
    assert!(slow >= 1, "the pad LFO never moved the filter");
    assert!(fast > slow * 2, "SPEED: {fast} vs {slow}");
}

/// Pad 1's hit on a long PLAIN SINE at `vel`, its mod envelope `env`
/// and routes `r`: Part 1's bus over `ms`.
fn play_vol(r: PadRoutes, env: PadEnv, vel: u8, ms: usize) -> Vec<f32> {
    let mut k = kit(r);
    let p = k.pad_mut(pad(1));
    p.set_machine(Machine::PlainSine);
    p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
    *p.sound_mut().env_mut() = env;
    let (_w, mut rg) = rig(&k, &sound());
    hit(&mut rg, vel);
    let mut out = Vec::new();
    for _ in 0..blocks(ms) {
        rg.render(1);
        out.extend_from_slice(rg.inst.part_bus(0));
    }
    out
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, s| m.max(s.abs()))
}

fn one(r: PadRoute) -> PadRoutes {
    routes([r, PadRoute::OFF, PadRoute::OFF, PadRoute::OFF])
}

/// The rise of a mod envelope, A 60 MED, held at 1.
fn rise() -> PadEnv {
    PadEnv::new(
        EnvPos::new(u7(60)),
        EnvPos::ZERO,
        EnvDecay::Infinite,
        EnvSpeed::Med,
    )
}

/// VOLUME from the mod envelope: it ramps, never steps.
#[test]
fn volume_ramps() {
    let r = one(route(RouteSource::ModEnv, RouteDest::Volume, -127));
    let (dry, wet) = (
        play_vol(PadRoutes::OFF, rise(), 100, 300),
        play_vol(r, rise(), 100, 300),
    );
    // The mod envelope rises to 1 and holds: VOLUME ducks to 0.
    let tail = 250 * SR as usize / 1000;
    assert!(peak(&dry[tail..]) > 1e-2);
    assert!(peak(&wet[tail..]) < 1e-3 * peak(&dry[tail..]), "not closed");
    // The gain the route applies, sample by sample, never jumps: its
    // ratio to the dry signal moves by a ramp's step at most.
    let g: Vec<f32> = dry
        .iter()
        .zip(&wet)
        .filter(|(d, _)| d.abs() > 1e-2)
        .map(|(d, w)| w / d)
        .collect();
    let jump = g.windows(2).fold(0.0f32, |m, w| m.max((w[1] - w[0]).abs()));
    assert!(jump < 0.01, "VOLUME stepped by {jump}");
}

/// The VOLUME law (ADR 0269): each route's gain from unity, a > 0 opening
/// with its source, a < 0 ducking, a bipolar LFO as (v + 1) / 2.
#[test]
fn volume_law_by_route() {
    use RouteSource::*;
    let gain = |s: RouteSource, a: i8, x: f32| {
        let mut v = [0.0; 6];
        v[s.index()] = x;
        one(route(s, RouteDest::Volume, a))
            .offsets(&RouteValues::new(v))
            .volume()
            .get()
    };
    for x in [0.0, 0.25, 0.5, 1.0] {
        // +127 follows the source; −127 is its inverse.
        assert!((gain(ModEnv, 127, x) - x).abs() < 1e-6, "{x}");
        assert!((gain(Vel, -127, x) - (1.0 - x)).abs() < 1e-6, "{x}");
        // +64 opens from 1 − 64/127 to 1; −64 ducks from 1 to 1 − 64/127.
        let a = 64.0 / 127.0;
        assert!((gain(ModEnv, 64, x) - (1.0 - a * (1.0 - x))).abs() < 1e-6);
        assert!((gain(ModEnv, -64, x) - (1.0 - a * x)).abs() < 1e-6);
    }
    // A bipolar LFO: −1 closes, +1 opens fully, 0 halfway, at +127.
    for (x, want) in [(-1.0, 0.0), (0.0, 0.5), (1.0, 1.0)] {
        assert!((gain(Lfo, 127, x) - want).abs() < 1e-6, "LFO {x}");
        assert!(
            (gain(Lfo2, -127, x) - (1.0 - want)).abs() < 1e-6,
            "LFO 2 {x}"
        );
    }
    // Every amount is heard: +1 at a source of 0 already dips.
    assert!(gain(ModEnv, 1, 0.0) < 1.0);
}

#[test]
fn volume_routes_multiply() {
    use RouteSource::*;
    let r = routes([
        route(ModEnv, RouteDest::Volume, 127),
        route(Vel, RouteDest::Volume, -64),
        route(ModEnv, RouteDest::Cutoff, 30),
        PadRoute::OFF,
    ]);
    let (env, vel) = (0.6, 0.8);
    let g = r
        .offsets(&RouteValues::new([env, 0.0, vel, 0.0, 0.0, 0.0]))
        .volume()
        .get();
    let want = env * (1.0 - 64.0 / 127.0 * vel);
    assert!((g - want).abs() < 1e-6, "{g} vs {want}");
    // Never over unity, never under 0.
    let all = routes([ModEnv, Lfo, Vel, Lfo1].map(|s| route(s, RouteDest::Volume, -127)));
    for x in [-1.0, 0.0, 1.0] {
        let g = all.offsets(&RouteValues::new([x; 6])).volume().get();
        assert!((0.0..=1.0).contains(&g), "{g}");
    }
}

/// MOD ENV → VOLUME +127 plays the hit through the envelope's shape: up
/// with its attack, down with its decay.
#[test]
fn volume_follows_the_mod_env() {
    let r = one(route(RouteSource::ModEnv, RouteDest::Volume, 127));
    let decay = PadEnv::new(
        EnvPos::new(u7(50)),
        EnvPos::ZERO,
        EnvDecay::Time(EnvPos::new(u7(60))),
        EnvSpeed::Med,
    );
    let (dry, wet) = (
        play_vol(PadRoutes::OFF, decay, 100, 600),
        play_vol(r, decay, 100, 600),
    );
    let ratio = |ms: usize| {
        let b = ms * SR as usize / 1000;
        peak(&wet[b..b + 480]) / peak(&dry[b..b + 480])
    };
    let (start, top, end) = (
        // The first block: the ramp from its first gain.
        peak(&wet[..BLOCK_SIZE]) / peak(&dry[..BLOCK_SIZE]),
        (5..120).map(ratio).fold(0.0, f32::max),
        ratio(580),
    );
    assert!(start < 0.2, "attack starts closed: {start}");
    assert!(top > 0.8, "opens: {top}");
    assert!(end < 0.05, "decays shut: {end}");
}

/// VEL → VOLUME −127: a hard hit ducks itself near silent, a soft one
/// plays near full (each against its own unrouted hit).
#[test]
fn vel_volume_ducks_hard_hits() {
    let r = one(route(RouteSource::Vel, RouteDest::Volume, -127));
    for (vel, lo, hi) in [(127u8, 0.0, 0.02), (1, 0.97, 1.0001)] {
        let dry = play_vol(PadRoutes::OFF, rise(), vel, 100);
        let wet = play_vol(r, rise(), vel, 100);
        let g = peak(&wet) / peak(&dry);
        assert!((lo..=hi).contains(&g), "vel {vel}: {g}");
    }
}

/// Review Focus 5: every route at its extreme stays finite and bounded.
#[test]
fn routes_at_extremes_stay_bounded() {
    use RouteSource::*;
    let spec = &FILTER_SPECS[FilterParams::CUTOFF.0 as usize];
    let all = |d: RouteDest, a: i8| routes([ModEnv, Lfo, Vel, Lfo1].map(|s| route(s, d, a)));
    for r in [
        all(RouteDest::Cutoff, 127),
        all(RouteDest::Reso, 127),
        all(RouteDest::Reso, -127),
    ] {
        for m in Machine::ALL {
            for vel in [1, 127] {
                let mut k = kit(r);
                let p = k.pad_mut(pad(1));
                p.set_machine(m);
                let l = p.sound_mut().lfo_mut();
                l.set_speed(U7::MAX);
                l.set_depth(U7::MAX);
                let (_w, mut rg) = rig(&k, &sound());
                hit(&mut rg, vel);
                for _ in 0..blocks(2000) {
                    rg.render(1);
                    let out = rg.inst.part_bus(0);
                    for &s in out {
                        assert!(s.is_finite() && s.abs() <= 2.0, "{m:?} vel {vel}: {s}");
                    }
                    if let Some(v) = voice(&rg) {
                        let c = rg.inst.voice(v).played_filter().cutoff;
                        assert!(
                            (spec.min..=spec.max).contains(&c),
                            "{m:?} vel {vel}: cutoff {c}"
                        );
                    }
                }
            }
        }
    }
}

/// A routed pad's filter moves per sample, never in steps: the glide
/// rule hands it the moving path, as a Part route does. A 10-octave
/// sweep in ~50 ms leaves nothing above ~6 kHz near the held sine's own
/// (−64 dB here; a knob glide's sub-block steps measure only ~1.5 dB more,
/// so the rule itself is pinned by `moves_filter`).
#[test]
fn routed_pad_sweep_is_smooth() {
    let r = routes([
        route(RouteSource::ModEnv, RouteDest::Cutoff, 127),
        PadRoute::OFF,
        PadRoute::OFF,
        PadRoute::OFF,
    ]);
    let mut k = kit(r);
    let p = k.pad_mut(pad(1));
    p.set_machine(Machine::PlainSine);
    p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
    let s = p.sound_mut();
    s.filter_mut().set_cutoff(Hz::new(100.0));
    *s.env_mut() = PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Time(EnvPos::new(u7(50))),
        EnvSpeed::Med,
    );
    let (_w, mut rg) = rig(&k, &sound());
    hit(&mut rg, 100);
    rg.render(1);
    let moves = |rg: &InstRig| {
        matches!(rg.inst.voice(voice(rg).expect("pad 1 sounds")).slot(),
            EngineSlot::Drum(d) if d.moves_filter())
    };
    assert!(moves(&rg), "a routed pad's filter isn't on the moving path");
    let mut out = Vec::new();
    for _ in 1..blocks(100) {
        rg.render(1);
        out.extend_from_slice(rg.inst.part_bus(0));
    }
    // A stepped sweep lights the 4th difference (energy above ~6 kHz)
    // at its steps' edges; a per-sample ramp leaves the sine's own.
    let hi: Vec<f32> = out
        .windows(5)
        .map(|w| w[0] - 4.0 * w[1] + 6.0 * w[2] - 4.0 * w[3] + w[4])
        .collect();
    let rms = |x: &[f32]| 10.0 * (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).log10();
    let r = rms(&hi) - rms(&out);
    assert!(r < -55.0, "stepped: {r:.1} dB");
}

/// No route, or routes off the filter: the knob glide as before.
#[test]
fn unrouted_pad_keeps_the_knob_glide() {
    let vol = routes([
        route(RouteSource::Lfo, RouteDest::Volume, 64),
        route(RouteSource::ModEnv, RouteDest::Cutoff, 0),
        PadRoute::OFF,
        PadRoute::OFF,
    ]);
    for r in [PadRoutes::OFF, vol] {
        let (_w, mut rg) = rig(&kit(r), &sound());
        hit(&mut rg, 100);
        rg.render(1);
        let v = voice(&rg).expect("pad 1 sounds");
        assert!(
            !matches!(rg.inst.voice(v).slot(), EngineSlot::Drum(d) if d.moves_filter()),
            "{r:?}"
        );
    }
}

/// The voice's glide rule sees the pad's routes: a routed pad renders bit
/// for bit as it does with a Part route forcing the moving path anyway
/// (WHEEL → FREQ with the wheel at rest, which moves nothing). Without
/// the pad in the rule, the lone pad takes the knob glide's steps.
#[test]
fn pad_routes_take_the_moving_path() {
    let r = one(route(RouteSource::ModEnv, RouteDest::Cutoff, 127));
    let mut k = kit(r);
    let p = k.pad_mut(pad(1));
    p.set_machine(Machine::PlainSine);
    p.knobs_mut().set(Knob::B, KnobValue::new(U7::MAX));
    let s = p.sound_mut();
    s.filter_mut().set_cutoff(Hz::new(100.0));
    *s.env_mut() = PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Time(EnvPos::new(u7(50))),
        EnvSpeed::Med,
    );
    let play = |wheel_route: bool| {
        let (_w, mut rg) = rig(&k, &sound());
        if wheel_route {
            let mut m = ModState::with_sources(MAX_MOD_SOURCES);
            m.set_fixed(ModSource::Wheel.index(), FixedCol::Freq, 1);
            assert!(m.filter_routed());
            *rg.shared.parts[0].mod_state = m;
        }
        hit(&mut rg, 100);
        let mut out = Vec::new();
        let mut cut = Vec::new();
        for _ in 0..blocks(100) {
            rg.render(1);
            out.extend_from_slice(rg.inst.part_bus(0));
            cut.push(cutoff(&rg));
        }
        (out, cut)
    };
    let (alone, forced) = (play(false), play(true));
    // The dummy route moves nothing: the same cutoffs, block by block.
    assert_eq!(alone.1, forced.1, "the wheel route moved CUTOFF");
    assert!(peak(&alone.0) > 1e-2, "silent");
    let same = alone
        .0
        .iter()
        .zip(&forced.0)
        .all(|(a, b)| a.to_bits() == b.to_bits());
    assert!(same, "a routed pad's filter isn't on the moving path");
}
