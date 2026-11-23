// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! VA's params block (spec § Pages, § Params and Motion).

use chimera_core::addr::BlockRef;
use chimera_core::block::{Block, Change, DiskCode, Motion, ParamId, Smoothing};
use chimera_core::dsp::va::{NoiseKind, VA_SPECS, VaMode, VaParams};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    Check, Decode, FileKind, Framer, Generation, Header, SoundDecoder, encode_sound, write_file,
};
use chimera_core::ui::glyph::FocusGlyph;
use chimera_hal::store::{ByteSink, StoreError};

struct VecSink(Vec<u8>);

impl ByteSink for VecSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

#[test]
fn neutral_is_the_spec() {
    let p = VaParams::default();
    assert_eq!(
        (p.wave, p.shape, p.coarse, p.fine),
        ([2.0 / 3.0; 2], [0.0; 2], [0; 2], [0.0; 2])
    );
    assert_eq!(
        (p.mode, p.amount, p.sub, p.stability),
        (VaMode::Mix, 0.5, 0.0, 1.0)
    );
    // NOISE (ADR 0197): off, so an old Sound plays as it did
    assert_eq!((p.noise_kind, p.noise), (NoiseKind::White, 0.0));
}

#[test]
fn every_param_declares_its_motion() {
    for s in &VA_SPECS {
        let want = match s.id {
            VaParams::CRSE1 | VaParams::CRSE2 | VaParams::NOISE_KIND => {
                Motion::Stepped(Change::Snap)
            }
            VaParams::MODE => Motion::Stepped(Change::Crossfade),
            _ => Motion::Continuous(Smoothing::KNOB),
        };
        assert_eq!(s.motion(), want, "{}", s.label);
        let choice = matches!(s.id, VaParams::MODE | VaParams::NOISE_KIND);
        assert_eq!(s.modulatable, !choice, "{}", s.label);
    }
    assert_eq!(VA_SPECS.iter().filter(|s| s.modulatable).count(), 12);
    for (i, s) in VA_SPECS.iter().enumerate() {
        assert_eq!(s.id, ParamId(i as u8));
    }
}

#[test]
fn all_fourteen_round_trip_on_disk() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.va = VaParams {
        wave: [0.1, 0.9],
        shape: [0.3, 0.7],
        coarse: [-24, 24],
        fine: [-100.0, 100.0],
        mode: VaMode::Bits,
        amount: 1.0,
        sub: 1.0,
        stability: 0.0,
        noise_kind: NoiseKind::Digital,
        noise: 0.6,
    };
    let mut sink = VecSink(Vec::new());
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    };
    write_file(&mut sink, &h, &mut |w| encode_sound(&s, w)).unwrap();
    let bytes = sink.0;
    let mut got = Sound::neutral(EngineType::Algo);
    for apply in [false, true] {
        let mut d = SoundDecoder::new(&mut got);
        let mut f = Framer::new(bytes.len() as u32).unwrap();
        f.push(&bytes, &mut |e| if apply { d.apply(e) } else { d.event(e) })
            .unwrap();
        f.finish().unwrap();
        if apply { d.commit() } else { d.end() }.unwrap();
    }
    assert!(got.bits_eq(&s));
    assert_eq!(got.params.va, s.params.va);
}

#[test]
fn unknown_mode_code_loads_as_mix() {
    // the first code past the last MODE
    let past = VaMode::ALL.len() as u8;
    let mut p = VaParams::default();
    assert!(!p.set_enum_code(VaParams::MODE, past));
    assert_eq!(p.mode, VaMode::Mix);
    assert_eq!(VaMode::from_disk_code(past), None);
}

#[test]
fn unknown_kind_code_loads_as_white() {
    let past = NoiseKind::ALL.len() as u8;
    let mut p = VaParams::default();
    assert!(!p.set_enum_code(VaParams::NOISE_KIND, past));
    assert_eq!(p.noise_kind, NoiseKind::White);
    for k in NoiseKind::ALL {
        assert!(p.set_enum_code(VaParams::NOISE_KIND, k.disk_code()));
        assert_eq!(p.noise_kind, k);
    }
}

#[test]
fn crse_rounds_a_modulated_value() {
    let mut p = VaParams::default();
    p.write(VaParams::CRSE1, 6.4);
    assert_eq!(p.get(VaParams::CRSE1), 6.0);
    p.write(VaParams::CRSE1, -24.6);
    assert_eq!(p.get(VaParams::CRSE1), -24.0);
    p.write(VaParams::CRSE1, f32::NAN);
    assert!((-24.0..=24.0).contains(&p.get(VaParams::CRSE1)));
}

#[test]
fn block_35_is_va() {
    assert_eq!(BlockRef::Va.disk_code(), Some(35));
    assert_eq!(BlockRef::Va.disk_ident(), Some("VA"));
    assert_eq!(BlockRef::from_disk_code(35), Some(BlockRef::Va));
}

#[test]
fn wave_draws_its_scope() {
    use chimera_core::ui::glyph::{VaGlyph, VaOsc};
    assert_eq!(
        VA_SPECS[VaParams::WAVE1.0 as usize].glyph,
        FocusGlyph::Va(VaGlyph::Wave(VaOsc::One))
    );
}
