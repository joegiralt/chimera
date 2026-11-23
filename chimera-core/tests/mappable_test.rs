// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-Part MIDI map's types (spec §§ 2-3): the whitelist, learn, the
//! place law and the reserved CCs.

mod common;
use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::lfo::LfoParams;
use chimera_core::dsp::modal::ModalParams;
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::dsp::va::VaParams;
use chimera_core::dsp::voice_fx::params::VoiceFxParams;
use chimera_core::midi_in::Cc;
use chimera_core::midi_map::{Binding, Feedback, Input, MAP_LEN, MapFull, Mappable, MidiMap, Span};
use chimera_core::orbit::RingIdx;
use chimera_core::params::{FilterParams, OutParams};
use chimera_core::part::PartParams;
use chimera_core::project::PartId;

fn cc(n: u8) -> Cc {
    Cc::new(n).unwrap()
}

fn inp(n: u8) -> Input {
    Input::cc(n).unwrap()
}

fn mappable(b: BlockRef, p: chimera_core::block::ParamId) -> Mappable {
    Mappable::new(ParamAddr::new(b, p)).expect("mappable")
}

#[test]
fn mappable_whitelist() {
    for b in BlockRef::ALL {
        for s in b.specs() {
            // TYPE and PLACE are switches (`fx_pots_are_mappable`).
            let switch = b == BlockRef::VoiceFx && s.id.0 < 2;
            let want = b.in_sound() && !s.patching && s.stored && !switch;
            // LEVEL, PAN and TILT (ADR 0284).
            let part_mix = b == BlockRef::Part
                && matches!(s.id, PartParams::LEVEL | PartParams::PAN | PartParams::TILT);
            // The master FX's knobs and ORBIT's ring amounts (#512,
            // `midi_fx_orbit_test`).
            let global = matches!(
                b,
                BlockRef::Chorus
                    | BlockRef::Delay
                    | BlockRef::Reverb
                    | BlockRef::Tape
                    | BlockRef::Comp
                    | BlockRef::OrbitRing(_)
            );
            if global {
                continue;
            }
            assert_eq!(
                Mappable::new(ParamAddr::new(b, s.id)).is_some(),
                want || part_mix,
                "{b:?} {}",
                s.label
            );
        }
    }
    let no = |b, p| assert!(Mappable::new(ParamAddr::new(b, p)).is_none(), "{b:?} {p:?}");
    for p in [PartParams::CHANNEL, PartParams::MODE, PartParams::BEND] {
        no(BlockRef::Part, p);
    }
    no(BlockRef::PartMix(PartId::ALL[0]), PartParams::LEVEL);
    no(
        BlockRef::Reverb,
        chimera_core::dsp::reverb::ReverbParams::TYPE,
    );
    no(
        BlockRef::OrbitRing(RingIdx::ALL[0]),
        chimera_core::orbit::scene::SceneRing::STEPS,
    );
    no(BlockRef::Tempo, chimera_core::block::ParamId(0));
    no(BlockRef::Filter, FilterParams::FM3_IN);
    no(BlockRef::Theme, chimera_core::block::ParamId(0));

    // Section 9's default targets are all Mappable.
    let lfo1 = BlockRef::Lfo(LfoSlot::Lfo1);
    for (b, p) in [
        (BlockRef::Filter, FilterParams::CUTOFF),
        (BlockRef::Filter, FilterParams::RESONANCE),
        (BlockRef::Part, PartParams::LEVEL),
        (BlockRef::Part, PartParams::PAN),
        (BlockRef::Out, OutParams::SEND1),
        (BlockRef::Out, OutParams::SEND3),
        (lfo1, LfoParams::DEPTH),
        (BlockRef::Algo, AlgoParams::MORPH),
        (BlockRef::Algo, AlgoParams::VOWEL),
        (BlockRef::VoiceFx, VoiceFxParams::P1),
        (BlockRef::VoiceFx, VoiceFxParams::P2),
        (BlockRef::Modal, ModalParams::STRUCTURE),
        (BlockRef::Modal, ModalParams::BRIGHT),
        (BlockRef::Modal, ModalParams::DAMP),
        (BlockRef::Modal, ModalParams::POS),
        (BlockRef::Va, VaParams::AMOUNT),
        (BlockRef::Va, VaParams::SHAPE1),
        (BlockRef::Va, VaParams::SHAPE2),
        (BlockRef::Va, VaParams::SUB),
    ] {
        let a = ParamAddr::new(b, p);
        assert_eq!(Mappable::new(a).map(Mappable::addr), Some(a), "{b:?} {p:?}");
    }
}

#[test]
fn learn_replaces_and_fills() {
    let cutoff = mappable(BlockRef::Filter, FilterParams::CUTOFF);
    let reso = mappable(BlockRef::Filter, FilterParams::RESONANCE);
    let mut m = MidiMap::EMPTY;
    m.learn(inp(74), cutoff).unwrap();
    m.learn(inp(20), cutoff).unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m.entries()[0].input, inp(20));
    m.learn(inp(20), reso).unwrap();
    assert_eq!(m.len(), 2);
    assert_eq!(m.targets_of(&inp(20)).count(), 2);
    assert_eq!(m.targets_of(&inp(74)).count(), 0);
    // Another kind on the same CC is the same control.
    let rel = Input::Cc(
        chimera_core::midi_map::LearnCc::new(20).unwrap(),
        chimera_core::midi_map::CcKind::Relative(chimera_midi::rel::ByteEncoding::TwosComplement),
    );
    assert_eq!(m.targets_of(&rel).count(), 2);

    // Fill to 16 distinct targets, then a 17th is refused.
    let mut m = MidiMap::EMPTY;
    let mut targets = BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter_map(Mappable::new);
    for _ in 0..MAP_LEN {
        m.learn(inp(1), targets.next().unwrap()).unwrap();
    }
    assert_eq!(m.len(), MAP_LEN);
    assert_eq!(m.learn(inp(1), targets.next().unwrap()), Err(MapFull));
    // A full map still re-learns a target it holds.
    let held = m.entries()[3].target;
    assert_eq!(m.learn(inp(2), held), Ok(()));
    assert_eq!(m.entries()[3].input, inp(2));
}

#[test]
fn remove_and_set() {
    let t = mappable(BlockRef::Filter, FilterParams::CUTOFF);
    let u = mappable(BlockRef::Filter, FilterParams::RESONANCE);
    let mut m = MidiMap::EMPTY;
    m.learn(inp(74), t).unwrap();
    m.learn(inp(71), u).unwrap();
    m.remove(0);
    assert_eq!((m.len(), m.entries()[0].target), (1, u));
    m.set(
        0,
        Binding {
            input: inp(9),
            target: u,
            span: Span { min: 5, max: 6 },
            feedback: Feedback::Off,
        },
    );
    assert_eq!((m.entries()[0].input, m.entries()[0].span.min), (inp(9), 5));
    m.remove(7);
    assert_eq!(m.len(), 1);
}

#[test]
fn place_inverts() {
    let t = mappable(BlockRef::Filter, FilterParams::CUTOFF);
    let e = |min, max| Span { min, max };
    assert_eq!(e(255, 0).place(0), 1.0);
    assert_eq!(e(0, 255).place(127), 1.0);
    assert_eq!(e(64, 128).place(0), 64.0 / 255.0);
    let _ = t;
}

#[test]
fn cc_reserved() {
    let reserved: Vec<u8> = (0..=127).filter(|&n| cc(n).is_reserved()).collect();
    let mut want = vec![0, 6, 32, 38, 64];
    want.extend(96..=101);
    want.extend(120..=127);
    assert_eq!(reserved, want);
}

#[test]
fn default_map_entries_are_mappable() {
    use chimera_core::midi_map::default_for;
    use chimera_core::params::EngineType;
    for (e, n) in [
        (EngineType::Algo, 10),
        (EngineType::Modal, 10),
        (EngineType::Va, 10),
        (EngineType::Tape, 6),
    ] {
        let m = default_for(e);
        assert_eq!(m.len(), n, "{e:?}");
        for x in m.entries() {
            assert!(Mappable::new(x.target.addr()).is_some());
            assert_eq!((x.span.min, x.span.max), (0, 255), "{e:?}");
        }
    }
    let ccs: Vec<Input> = default_for(EngineType::Modal)
        .entries()
        .iter()
        .map(|x| x.input)
        .collect();
    let want = [74, 71, 7, 10, 91, 93, 16, 17, 18, 19].map(inp);
    assert_eq!(ccs, want);
}

/// INIT before the wheel: DEPTH 1, no PITCH route.
fn old_init(
    e: chimera_core::params::EngineType,
) -> (
    chimera_core::params::ParamSnapshot,
    chimera_core::modulation::ModState,
) {
    use chimera_core::modulation::{FixedCol, MAX_MOD_SOURCES, ModSource, ModState};
    use chimera_core::params::EngineType;
    let mut m = ModState::with_sources(MAX_MOD_SOURCES);
    for s in [ModSource::Env1, ModSource::Lfo1, ModSource::Note] {
        m.set_fixed(s.index(), FixedCol::Freq, 0);
    }
    if matches!(e, EngineType::Tape | EngineType::Va) {
        m.set_fixed(ModSource::Env1.index(), FixedCol::Vca, 127);
    }
    let mut p = chimera_core::preset::Sound::init(e).params;
    p.lfos[0].depth = 1.0;
    (p, m)
}

#[test]
fn init_wheel_vibrato() {
    use chimera_core::modulation::ModSource;
    use chimera_core::params::{EngineType, PitchParams};
    use chimera_core::preset::Sound;
    let pitch = ParamAddr::new(BlockRef::Pitch, PitchParams::PITCH);
    for e in [
        EngineType::Algo,
        EngineType::Modal,
        EngineType::Tape,
        EngineType::Va,
    ] {
        let s = Sound::init(e);
        assert_eq!(s.params.lfos[0].depth, 0.0, "{e:?}");
        assert!(s.dest_registry.is_primed(pitch), "{e:?}");
        let d = s.mod_state.find(pitch).expect("a PITCH column");
        assert_eq!(s.mod_state.amount(ModSource::Lfo1.index(), d), 2, "{e:?}");
        // The wheel at rest: bit-identical to INIT before the wheel.
        let (op, om) = old_init(e);
        let (new, old) = (
            common::render_sound(&s.params, &s.mod_state),
            common::render_sound(&op, &om),
        );
        assert!(
            new.iter()
                .zip(&old)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{e:?} changed at rest"
        );
    }
}

/// With DEPTH 1 on ALGO INIT, 4 Hz: the fundamental in 100 ms Hann windows
/// every 12.5 ms (`fundamental_hz`, 0.05 cent) swings about +-38 cents; the
/// window's own smoothing at 4 Hz costs ~5 % (measured +-34.7).
#[test]
fn init_wheel_vibrato_swing() {
    use chimera_core::params::EngineType;
    use chimera_core::preset::Sound;
    use chimera_hal::BLOCK_SIZE;
    let mut s = Sound::init(EngineType::Algo);
    s.params.lfos[0].depth = 1.0;
    s.params.lfos[0].rate = 4.0;
    let mut v = common::Rig::new(chimera_hal::SAMPLE_RATE);
    v.note_on(
        chimera_core::MidiNote::new(60).unwrap(),
        chimera_core::Velocity::new(100).unwrap(),
        &s.params,
    );
    let mut out = Vec::new();
    let mut b = [0.0f32; BLOCK_SIZE];
    while out.len() < 48_000 * 3 / 2 {
        v.render(&mut b, &s.params, &s.mod_state);
        out.extend_from_slice(&b);
    }
    let f0 = 261.6256f64;
    let cents: Vec<f64> = out[4800..]
        .windows(4800)
        .step_by(600)
        .map(|w| 1200.0 * (common::fundamental_hz(w, f0) / f0).log2())
        .collect();
    let (lo, hi) = cents
        .iter()
        .fold((f64::MAX, f64::MIN), |(l, h), &c| (l.min(c), h.max(c)));
    let amp = (hi - lo) / 2.0;
    assert!(
        (32.0..=40.0).contains(&amp),
        "swing +-{amp} cents, {cents:?}"
    );
}

/// FX pots and MIX map; TYPE and PLACE are switches, not knobs.
#[test]
fn fx_pots_are_mappable() {
    use chimera_core::dsp::voice_fx::params::VoiceFxParams as X;
    let ok = |p| Mappable::new(ParamAddr::new(BlockRef::VoiceFx, p)).is_some();
    for p in [X::P1, X::P2, X::P3, X::MIX] {
        assert!(ok(p), "{p:?}");
    }
    assert!(!ok(X::TYPE) && !ok(X::PLACE));
}

/// MIDI MAP names an FX pot by the Part's type.
#[cfg(feature = "midi-map")]
#[test]
fn the_midi_map_names_an_fx_pot_by_the_type() {
    use chimera_core::dsp::voice_fx::params::{FxType, VoiceFxParams as X};
    use chimera_core::midi_map::{Binding, Feedback, Span};
    use chimera_core::ui::settings::midi_map::name;
    let e = Binding {
        input: inp(20),
        target: mappable(BlockRef::VoiceFx, X::P2),
        span: Span::FULL,
        feedback: Feedback::Off,
    };
    assert_eq!(name(&e, FxType::Soft), "TONE");
    assert_eq!(name(&e, FxType::Fold), "SYMMETRY");
}
