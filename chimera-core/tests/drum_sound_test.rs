// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A pad's sound (ADR 0269): bytes round-trip, any bytes are a sound, and
//! a pad copied from its Sound overlays exactly what the Sound holds.

use chimera_core::dsp::Unit;
use chimera_core::dsp::drum::sound::{
    Amount, AmpMode, EnvDecay, EnvPos, PAD_SOUND_BYTES, PadEnv, PadFilter, PadRoute, PadSound,
    RouteDest, RouteId, RouteSource,
};
use chimera_core::dsp::drum::types::{Hz, Kit, Pad, PadId};
use chimera_core::dsp::filter::{FilterKind, FilterMode};
use chimera_core::dsp::modulator::EnvSpeed;
use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::ui::filter_panel::{self, PanelTarget};
use chimera_midi::value::U7;

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

fn edited() -> PadSound {
    let mut s = PadSound::DEFAULT;
    let f = s.filter_mut();
    f.set_kind(FilterKind::Ladder);
    f.set_cutoff(Hz::new(420.0));
    f.set_resonance(Unit::new(0.62));
    let a = s.amp_mut();
    *a.env_mut() = PadEnv::new(
        EnvPos::new(u7(10)),
        EnvPos::ZERO,
        EnvDecay::from(u7(64)),
        EnvSpeed::Fast,
    );
    a.set_mode(AmpMode::Gated);
    let r = s.routes_mut();
    r.set(
        RouteId::ALL[0],
        PadRoute::new(
            RouteSource::ModEnv,
            RouteDest::Cutoff,
            Amount::new(90).unwrap(),
        ),
    );
    r.set(
        RouteId::ALL[1],
        PadRoute::new(RouteSource::Lfo, RouteDest::Reso, Amount::new(-40).unwrap()),
    );
    s
}

#[test]
fn pad_sound_bytes_round_trip() {
    for s in [PadSound::DEFAULT, edited()] {
        assert_eq!(PadSound::from_bytes(s.to_bytes(), &PadSound::DEFAULT), s);
    }
    let e = edited();
    assert_ne!(e, PadSound::DEFAULT);
    assert_eq!(e.amp().mode(), AmpMode::Gated);
    assert!(e.routes().moves_filter() && !PadSound::DEFAULT.routes().moves_filter());
    assert_eq!(PAD_SOUND_BYTES, 57);
}

#[test]
fn any_bytes_are_a_sound() {
    let mut nan = [0u8; PAD_SOUND_BYTES];
    for at in [3, 7, 11, 20, 24, 28, 32] {
        nan[at..at + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    }
    let mut kind_lacks_mode = PadSound::DEFAULT.to_bytes();
    kind_lacks_mode[0] = 4; // the 2040: LP4 only
    kind_lacks_mode[1] = 6; // NOTCH
    for b in [
        [0; PAD_SOUND_BYTES],
        [0xFF; PAD_SOUND_BYTES],
        nan,
        kind_lacks_mode,
    ] {
        let s = PadSound::from_bytes(b, &PadSound::DEFAULT);
        assert!(s.filter().kind().modes().contains(&s.filter().mode()));
        assert!(s.filter().cutoff().get().is_finite());
        assert!(s.amp().vel().get().is_finite());
        assert!(s.amp().gate().strike.get().is_finite());
        for r in RouteId::ALL {
            assert!(s.routes().get(r).amount().get() >= -127);
        }
    }
    let s = PadSound::from_bytes([0xFF; PAD_SOUND_BYTES], &PadSound::DEFAULT);
    assert_eq!(s.amp().env().decay(), EnvDecay::Infinite);
    assert_eq!(s.amp().mode(), AmpMode::OneShot, "an unknown code is d's");
    assert_eq!(s.filter().kind(), FilterKind::Svf);
    let s = PadSound::from_bytes(nan, &edited());
    assert_eq!(s.filter().cutoff(), Hz::new(420.0), "NaN is d's");
}

fn odd_sound() -> ParamSnapshot {
    let mut p = Sound::init(EngineType::Drum).params;
    p.filter.set_kind(FilterKind::Sem);
    p.filter.set_mode(FilterMode::Bp12);
    p.filter.cutoff = 1234.567;
    p.filter.resonance = 0.3141;
    p.filter.morph = 0.777;
    p.out.vca_vel = 0.4;
    p.out.gate = [0.1, 0.2, 0.3];
    p
}

#[test]
fn overlay_of_from_sound_is_identity() {
    for p in [Sound::init(EngineType::Drum).params, odd_sound()] {
        let mut q = p.clone();
        q.filter.set_kind(FilterKind::Dirty);
        q.filter.cutoff = 50.0;
        q.out.gate = [0.9; 3];
        q.out.vca_vel = 0.25;
        PadSound::from_sound(&p).overlay(&mut q);
        let (a, b) = (&q.filter, &p.filter);
        assert_eq!(a.cutoff.to_bits(), b.cutoff.to_bits());
        assert_eq!(a.resonance.to_bits(), b.resonance.to_bits());
        assert_eq!(a.morph.to_bits(), b.morph.to_bits());
        assert_eq!(a.kind(), b.kind());
        assert_eq!(a.mode(), b.mode());
        assert_eq!(a.voice, b.voice);
        // #963: the pad's VEL is the hit's one velocity law, at its kernel:
        // the Sound's AMP VEL plays nothing on DRUM.
        assert_eq!(q.out.vca_vel, 0.0, "VEL is the pad's own, not the VCA's");
        for i in 0..3 {
            assert_eq!(q.out.gate[i].to_bits(), p.out.gate[i].to_bits());
        }
    }
}

#[test]
fn default_is_drum_init() {
    assert_eq!(
        PadSound::DEFAULT,
        PadSound::from_sound(&Sound::init(EngineType::Drum).params)
    );
    assert!(PadSound::DEFAULT.amp().env().is_open());
    assert_eq!(PadSound::DEFAULT.amp().mode(), AmpMode::OneShot);
    assert!(
        PadId::ALL
            .iter()
            .all(|&p| *Kit::DEFAULT.pad(p).sound() == PadSound::DEFAULT)
    );
}

#[test]
fn decay_127_is_infinite() {
    assert_eq!(EnvDecay::from(U7::MAX), EnvDecay::Infinite);
    assert_eq!(EnvDecay::Infinite.code(), U7::MAX);
    assert_eq!(
        EnvDecay::from(u7(126)),
        EnvDecay::Time(EnvPos::new(u7(126)))
    );
    let e = PadEnv::new(
        EnvPos::ZERO,
        EnvPos::ZERO,
        EnvDecay::Time(EnvPos::new(U7::MAX)),
        EnvSpeed::Med,
    );
    assert_eq!(e.decay(), EnvDecay::Infinite, "one value, one code");
    assert!(e.is_open());
}

#[test]
fn the_pad_carries_the_flt_front() {
    for k in FilterKind::BUILT {
        for c in 0..=2 {
            if let Some(PanelTarget::Filter(id)) = filter_panel::knob(k, c).map(|p| p.target) {
                assert!(PadFilter::IDS.contains(&id), "{k:?} cell {c}");
            }
        }
    }
}

#[test]
fn amounts_clamp() {
    assert_eq!(Amount::new(-128), None);
    assert_eq!(Amount::new(-127).map(Amount::get), Some(-127));
    assert_eq!(Amount::clamped(127).get(), 127);
    assert_eq!(Amount::clamped(-128).get(), -127);
}

#[test]
fn pads_keep_their_sound_through_their_bytes() {
    let d = Kit::DEFAULT.pad(PadId::ALL[0]).with_sound(edited());
    let p = Pad::from_bytes(Kit::DEFAULT.pad(PadId::ALL[0]).to_bytes(), &d);
    assert_eq!(*p.sound(), edited());
    eprintln!(
        "PadSound = {} B, Pad = {} B",
        size_of::<PadSound>(),
        size_of::<Pad>()
    );
}
