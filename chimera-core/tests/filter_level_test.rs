//! LEVEL (Filter 12): its law, and DRIVE's (Filter 2, retired) translation
//! and re-address on load (spec § LEVEL, ADR 0096).

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::dsp::filter::{level_from_drive, level_gain};
use chimera_core::factory::factory_sound;
use chimera_core::mod_path::LABEL_LEN;
use chimera_core::modulation::{ModSource, ModState};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::preset::Sound;
use chimera_core::storage::{
    FileKind, Generation, Header, MIGRATIONS, RecordTag, TRANSLATIONS, decode_block, write_file,
};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::Rig;
use common::codec_util::{VecSink, decode};

const LEVEL: ParamAddr = ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL);

fn db(x: f32) -> f32 {
    20.0 * x.log10()
}

/// A Sound file of hand-built records.
fn file(records: &[(RecordTag, Vec<u8>)]) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(Sound::neutral(EngineType::Algo).name),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| {
        records.iter().try_for_each(|(t, p)| w.put(*t, p))
    })
    .unwrap();
    sink.0
}

fn engine() -> (RecordTag, Vec<u8>) {
    (RecordTag::Engine, vec![EngineType::Algo.disk_code()])
}

/// A Filter Block payload of `(id, value)` entries.
fn filter_payload(entries: &[(u8, f32)]) -> Vec<u8> {
    let mut p = vec![10];
    for (id, v) in entries {
        p.push(*id);
        p.extend_from_slice(&v.to_le_bytes());
    }
    p
}

#[test]
fn level_law_is_unity_at_half() {
    assert_eq!(level_gain(0.5), 1.0);
    assert!(
        (level_gain(0.0) - 0.25).abs() <= 1e-6,
        "{}",
        level_gain(0.0)
    );
    assert!((level_gain(1.0) - 4.0).abs() <= 1e-5, "{}", level_gain(1.0));
}

#[test]
fn drive_translates_to_the_same_gain_up_to_0_75() {
    for i in 0..=15 {
        let d = i as f32 * 0.05;
        let err = db(level_gain(level_from_drive(d)) / (1.0 + 4.0 * d));
        assert!(err.abs() <= 0.1, "DRIVE {d}: {err} dB");
    }
    assert_eq!(level_from_drive(0.0), 0.5);
    // ×4 at the ceiling: 20·log10(5/4) = 1.94 dB under DRIVE 1's ×5.
    assert_eq!(level_from_drive(1.0), 1.0);
    // Any finite DRIVE a file holds lands in range.
    for d in [-1e30, -5.0, -0.3, 1.5, 1e30] {
        let l = level_from_drive(d);
        assert!((0.5..=1.0).contains(&l), "DRIVE {d}: LEVEL {l}");
    }
}

/// The RMS of a held SAW LEAD note through `filter`, its routes off.
fn held_rms(filter: FilterParams) -> f32 {
    let mut p: ParamSnapshot = factory_sound(4).unwrap().params;
    p.filter = filter;
    let mods = ModState::new();
    let mut rig = Rig::new(chimera_hal::SAMPLE_RATE);
    rig.note_on(MidiNote::new(48).unwrap(), Velocity::new(100).unwrap(), &p);
    let mut buf = [0.0f32; BLOCK_SIZE];
    let mut sum = 0.0f64;
    let mut n = 0;
    for b in 0..600 {
        rig.render(&mut buf, &p, &mods);
        if b >= 150 {
            sum += buf.iter().map(|&x| f64::from(x * x)).sum::<f64>();
            n += BLOCK_SIZE;
        }
    }
    (sum / n as f64).sqrt() as f32
}

#[test]
fn a_drive_only_file_loads_at_its_old_level() {
    const DRIVE: f32 = 0.3;
    let f = filter_payload(&[(0, 20_000.0), (1, 0.0), (2, DRIVE)]);
    let s = decode(&file(&[engine(), (RecordTag::Block, f)])).unwrap();
    assert_eq!(s.params.filter.level, level_from_drive(DRIVE));

    // The old pre-gain, `1 + 4·DRIVE`, on the unity path: wide open, the
    // filter's states stay under `sat_c1`'s knee, so the chain is linear.
    let mut unity = s.params.filter;
    unity.level = 0.5;
    let want = held_rms(unity) * (1.0 + 4.0 * DRIVE);
    let got = held_rms(s.params.filter);
    let err = db(got / want);
    assert!(err.abs() <= 0.1, "{err} dB ({got} vs {want})");
}

#[test]
fn a_file_with_neither_decodes_to_unity() {
    let f = filter_payload(&[(0, 5_000.0), (1, 0.2)]);
    let s = decode(&file(&[engine(), (RecordTag::Block, f)])).unwrap();
    assert_eq!(s.params.filter.level, 0.5);
}

/// Only DRIVE translates: a file with the filter's older retired ids (FM,
/// ENV, KEY) runs the translation and leaves LEVEL where it was.
#[test]
fn fm_env_key_alone_leave_level() {
    let mut snap = ParamSnapshot::for_engine(EngineType::Algo);
    snap.filter.level = 0.7;
    let f = filter_payload(&[(3, 0.4), (4, 0.9), (5, 1.0)]);
    decode_block(&f, MIGRATIONS, TRANSLATIONS, Some(&mut snap)).unwrap();
    assert_eq!(snap.filter.level, 0.7);
}

#[test]
fn routes_into_drive_land_on_level() {
    let label: [u8; LABEL_LEN] = *b"FLTDRIVE";
    let src = ModSource::Lfo2;
    let mut reg = vec![10, 2];
    reg.extend_from_slice(&label);
    let s = decode(&file(&[
        engine(),
        (RecordTag::Registry, reg),
        (RecordTag::ModDests, vec![8, 10, 2]),
        (RecordTag::Routes, vec![src.disk_code(), 10, 2, 40]),
    ]))
    .unwrap();
    let m = &s.mod_state;
    assert_eq!(m.num_dests(), 1);
    let d = m.find(LEVEL).expect("DRIVE's dest at LEVEL");
    assert_eq!(m.amount(src.index(), d), 40);
    let e = s.dest_registry.get(0).expect("the registry entry");
    assert_eq!(e.addr, LEVEL);
    assert_eq!(e.label, label);
}
