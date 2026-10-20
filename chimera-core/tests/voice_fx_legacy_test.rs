//! Old Sounds load into the voice's FX slot (#614, spec § Loading old
//! Sounds): DRV's block 9 as SOFT PRE, the folder's block 11 as FOLD POST,
//! DRV first when both ran. Each file is written byte for byte as the old
//! firmware did.

mod common;

use std::path::PathBuf;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::voice_fx::params::{FxType, Place, VoiceFxParams};
use chimera_core::midi_map::Mappable;
use chimera_core::mod_path::LABEL_LEN;
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::project::Project;
use chimera_core::storage::{Framer, RecordTag, SoundCheck};
use chimera_core::taps::{Jack, TapOp};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::codec_util::{decode_project_lossy, fix_crc, record_offsets, sound_file};
use common::{Rig, clicks};

const DRIVE: u8 = 9;
const FOLDER: u8 = 11;

fn engine() -> (RecordTag, Vec<u8>) {
    (RecordTag::Engine, vec![EngineType::Algo.disk_code()])
}

/// A Block record of an old block: `(id, value)` entries.
fn old_block(code: u8, entries: &[(u8, f32)]) -> (RecordTag, Vec<u8>) {
    let mut p = vec![code];
    for (id, v) in entries {
        p.push(*id);
        p.extend_from_slice(&v.to_le_bytes());
    }
    (RecordTag::Block, p)
}

/// Decoded, and the entries lost.
fn load(records: &[(RecordTag, Vec<u8>)]) -> (Sound, u16) {
    let bytes = sound_file(records);
    let mut s = Sound::neutral(EngineType::Algo);
    let mut c = SoundCheck::new();
    for apply in [false, true] {
        let mut f = Framer::new(bytes.len() as u32).unwrap();
        let t = apply.then_some(&mut s);
        match t {
            None => f.push(&bytes, &mut |e| c.step(e, None)).unwrap(),
            Some(t) => f.push(&bytes, &mut |e| c.step(e, Some(&mut *t))).unwrap(),
        }
        f.finish().unwrap();
    }
    c.finish(Some(&mut s)).unwrap();
    (s, c.skips())
}

fn fx(s: &Sound) -> (FxType, Place, [f32; 3], f32) {
    let f = &s.params.fx;
    (f.ty(), f.place, f.pots, f.mix)
}

const P1: ParamAddr = ParamAddr::new(BlockRef::VoiceFx, VoiceFxParams::P1);
const P2: ParamAddr = ParamAddr::new(BlockRef::VoiceFx, VoiceFxParams::P2);

#[test]
fn old_drv_loads_as_soft_pre() {
    let (s, skips) = load(&[engine(), old_block(DRIVE, &[(0, 0.4), (1, 0.7), (2, 0.8)])]);
    assert_eq!(fx(&s), (FxType::Soft, Place::Pre, [0.4, 0.7, 0.5], 0.8));
    assert_eq!(skips, 0);
}

/// A file's values past the old 0..1 ranges load clamped.
#[test]
fn out_of_range_values_clamp() {
    let (s, _) = load(&[
        engine(),
        old_block(DRIVE, &[(0, 5.0), (1, -3.0), (2, -1.0)]),
    ]);
    assert_eq!(fx(&s), (FxType::Soft, Place::Pre, [1.0, 0.0, 0.5], 0.0));
}

#[test]
fn old_fold_loads_as_fold_post() {
    let (s, skips) = load(&[engine(), old_block(FOLDER, &[(0, 0.3), (1, 0.2)])]);
    assert_eq!(fx(&s), (FxType::Fold, Place::Post, [0.3, 0.2, 0.5], 0.5));
    assert_eq!(skips, 0);
}

#[test]
fn drv_wins_when_both_ran() {
    let (s, skips) = load(&[
        engine(),
        old_block(DRIVE, &[(0, 0.4), (1, 0.7), (2, 0.8)]),
        old_block(FOLDER, &[(0, 0.3), (1, 0.2), (2, 0.9)]),
    ]);
    assert_eq!(fx(&s), (FxType::Soft, Place::Pre, [0.4, 0.7, 0.5], 0.8));
    assert_eq!(skips, 0);
}

#[test]
fn a_routed_drive_at_zero_ran() {
    let src = ModSource::Lfo1;
    let (s, skips) = load(&[
        engine(),
        old_block(DRIVE, &[(0, 0.0)]),
        (RecordTag::Routes, vec![src.disk_code(), DRIVE, 0, 50]),
    ]);
    assert_eq!(s.params.fx.ty(), FxType::Soft);
    assert_eq!(s.mod_state.fixed_amount(src.index(), FixedCol::Fx), 50);
    assert_eq!(skips, 0);
}

/// An ALGO tap on the old FOLD ran the fold at 0; a new file's tap on the
/// FX column, with TYPE OFF, runs nothing (it is stored at the slot's P1).
#[test]
fn an_old_fold_tap_ran_the_fold() {
    let jack = Jack::Tap(TapOp::Op6).disk_code();
    let (s, skips) = load(&[engine(), (RecordTag::TapRoutes, vec![jack, FOLDER, 0, 100])]);
    assert_eq!(fx(&s), (FxType::Fold, Place::Post, [0.0, 0.5, 0.5], 0.5));
    assert_eq!(s.params.taps.get(TapOp::Op6, FixedCol::Fx), Some(100));
    assert_eq!(skips, 0);
    let p1 = [BlockRef::VoiceFx.disk_code().unwrap(), VoiceFxParams::P1.0];
    let (s, _) = load(&[
        engine(),
        (RecordTag::TapRoutes, vec![jack, p1[0], p1[1], 100]),
    ]);
    assert_eq!(s.params.fx.ty(), FxType::Off);
}

#[test]
fn neither_ran_stays_off() {
    let (s, skips) = load(&[
        engine(),
        old_block(DRIVE, &[(0, 0.0), (1, 0.9)]),
        old_block(FOLDER, &[(0, 0.0), (1, 0.1)]),
    ]);
    assert_eq!(s.params.fx, VoiceFxParams::default());
    assert_eq!(skips, 0);
}

/// (RF 1) DRIVE, a TONE route and its registry column, the fold's SYM
/// registry column and route, and a FOLD route: the routes meet on the
/// slot's P2 column and the FX column.
#[test]
fn old_routes_and_registry_collapse_onto_the_slot() {
    let label = |n: &[u8; 4]| {
        let mut l = [b' '; LABEL_LEN];
        l[..4].copy_from_slice(n);
        l
    };
    let mut reg = vec![DRIVE, 1];
    reg.extend_from_slice(&label(b"TONE"));
    reg.extend_from_slice(&[FOLDER, 1]);
    reg.extend_from_slice(&label(b"SYM "));
    let (lfo1, env1, lfo2) = (ModSource::Lfo1, ModSource::Env1, ModSource::Lfo2);
    let (s, skips) = load(&[
        engine(),
        old_block(DRIVE, &[(0, 0.5)]),
        (RecordTag::Registry, reg),
        (RecordTag::ModDests, vec![8, DRIVE, 1, FOLDER, 1]),
        (
            RecordTag::Routes,
            vec![
                lfo1.disk_code(),
                DRIVE,
                1,
                30,
                env1.disk_code(),
                FOLDER,
                1,
                20,
                lfo2.disk_code(),
                FOLDER,
                0,
                40,
            ],
        ),
    ]);
    assert_eq!(skips, 0);
    assert_eq!(s.params.fx.ty(), FxType::Soft);
    let m = &s.mod_state;
    assert_eq!(m.num_dests(), 1);
    let d = m.find(P2).expect("one P2 column");
    assert_eq!(m.amount(lfo1.index(), d), 30);
    assert_eq!(m.amount(env1.index(), d), 20);
    assert_eq!(s.dest_registry.get(0).map(|e| e.addr), Some(P2));
    assert!(s.dest_registry.get(1).is_none());
    assert_eq!(m.fixed_amount(lfo2.index(), FixedCol::Fx), 40);
}

/// (RF 5) A MIDI map entry on the old DRIVE drives P1, and a CC ramp under
/// SOFT glides.
#[test]
fn an_old_midi_map_cc_on_drive_drives_p1() {
    let (s, skips) = load(&[
        engine(),
        old_block(DRIVE, &[(0, 0.5)]),
        (RecordTag::MidiMap, vec![20, DRIVE, 0, 0, 255]),
    ]);
    assert_eq!(skips, 0);
    let e = s.map.entries()[0];
    assert_eq!(e.target, Mappable::new(P1).unwrap());
    // A held sine: any step is the sweep's, not the wave's own edge.
    let mut p = s.params.clone();
    for (i, op) in p.algo.ops.iter_mut().enumerate() {
        op.level = if i == 0 { 99 } else { 0 };
    }
    (p.filter.cutoff, p.filter.resonance) = (20_000.0, 0.0);
    let blocks = chimera_hal::SAMPLE_RATE as usize / BLOCK_SIZE;
    let mut rig = Rig::new(chimera_hal::SAMPLE_RATE);
    p.fx.pots[0] = e.place(0);
    rig.note_on(MidiNote::new(45).unwrap(), Velocity::new(100).unwrap(), &p);
    let (mut out, mut buf) = (Vec::new(), [0.0f32; BLOCK_SIZE]);
    let mut last = p.fx.pots[0];
    let mut worst = 0.0f32;
    for b in 0..blocks {
        p.fx.pots[0] = e.place((b * 127 / (blocks - 1)) as u8);
        worst = worst.max((p.fx.pots[0] - last).abs());
        last = p.fx.pots[0];
        rig.render_glided(&mut buf, &p, &s.mod_state);
        out.extend_from_slice(&buf);
    }
    assert!(clicks(&out).is_empty());
    assert!(worst <= 1.0 / 64.0, "{worst}");
    assert!(out.iter().any(|x| x.abs() > 0.01));
}

fn fixture() -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1/sends.prj");
    std::fs::read(p).unwrap()
}

#[test]
fn an_old_project_part_loads_its_drv() {
    let f = fixture();
    let o = record_offsets(&f);
    let tag = |i: usize| u16::from_le_bytes([f[o[i]], f[o[i] + 1]]);
    // After Part 0's Engine record.
    let part = (0..o.len() - 1)
        .find(|&i| tag(i) == RecordTag::Part.code())
        .unwrap();
    let at = (part..o.len() - 1)
        .find(|&i| tag(i) == RecordTag::Engine.code())
        .unwrap()
        + 1;
    let (_, rec) = old_block(DRIVE, &[(0, 0.4), (1, 0.7), (2, 0.8)]);
    let mut g = f[..o[at]].to_vec();
    g.extend_from_slice(&RecordTag::Block.code().to_le_bytes());
    g.extend_from_slice(&(rec.len() as u16).to_le_bytes());
    g.extend_from_slice(&rec);
    g.extend_from_slice(&f[o[at]..]);
    fix_crc(&mut g);
    let (mut p, _) = Project::boxed();
    decode_project_lossy(&g, &mut p).unwrap();
    let s = &p.perf().parts()[0].sound;
    assert_eq!(
        (s.params.fx.ty(), s.params.fx.place, s.params.fx.pots),
        (FxType::Soft, Place::Pre, [0.4, 0.7, 0.5])
    );
}

#[test]
fn lpg_code_is_reserved() {
    assert_eq!(FilterKind::from_disk_code(7), None);
    let kind = {
        let mut p = vec![BlockRef::Filter.disk_code().unwrap(), FilterParams::KIND.0];
        p.extend_from_slice(&7u32.to_le_bytes());
        p
    };
    let (s, skips) = load(&[engine(), (RecordTag::Block, kind)]);
    assert_eq!(s.params.filter.kind(), FilterKind::default());
    assert_eq!(skips, 1);
}
