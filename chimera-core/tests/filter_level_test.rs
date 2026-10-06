//! LEVEL (Filter 12): its law, and DRIVE's (Filter 2, retired) translation
//! and re-address on load (spec § LEVEL, ADR 0096).

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::dsp::filter::{level_from_drive, level_gain};
use chimera_core::dsp::lfo::LfoShape;
use chimera_core::factory::factory_sound;
use chimera_core::mod_path::{LABEL_LEN, ModDestRegistry};
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

/// A file naming both DRIVE's and LEVEL's dest (crafted: the firmware
/// never writes DRIVE) loads one dest at LEVEL, with both routes on it.
#[test]
fn drive_and_level_dests_load_as_one() {
    let s = decode(&file(&[
        engine(),
        (RecordTag::ModDests, vec![8, 10, 2, 10, 12]),
        (
            RecordTag::Routes,
            vec![
                ModSource::Lfo2.disk_code(),
                10,
                2,
                40,
                ModSource::Env1.disk_code(),
                10,
                12,
                30,
            ],
        ),
    ]))
    .unwrap();
    let m = &s.mod_state;
    assert_eq!(m.num_dests(), 1);
    let d = m.find(LEVEL).unwrap();
    assert_eq!(m.amount(ModSource::Lfo2.index(), d), 40);
    assert_eq!(m.amount(ModSource::Env1.index(), d), 30);
}

/// A file holding both DRIVE and LEVEL (crafted) loads its LEVEL.
#[test]
fn a_stored_level_beats_drive() {
    let f = filter_payload(&[(2, 0.3), (12, 0.6)]);
    let s = decode(&file(&[engine(), (RecordTag::Block, f)])).unwrap();
    assert_eq!(s.params.filter.level, 0.6);
}

/// Blocks a sound holds still before its LEVEL is read: past the attack.
const HELD: usize = 150;

/// A held 110 Hz sine (OP A alone) through a wide-open CLEAN, `at` setting
/// block `b`'s params.
fn sine_voice(blocks: usize, mods: &ModState, at: impl Fn(usize, &mut ParamSnapshot)) -> Vec<f32> {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    for (i, op) in p.algo.ops.iter_mut().enumerate() {
        op.level = if i == 0 { 99 } else { 0 };
    }
    (p.filter.cutoff, p.filter.resonance) = (20_000.0, 0.0);
    let mut rig = Rig::new(chimera_hal::SAMPLE_RATE);
    at(0, &mut p);
    rig.note_on(MidiNote::new(45).unwrap(), Velocity::new(100).unwrap(), &p);
    let mut out = Vec::new();
    let mut buf = [0.0f32; BLOCK_SIZE];
    for b in 0..blocks {
        at(b, &mut p);
        rig.render(&mut buf, &p, mods);
        out.extend_from_slice(&buf);
    }
    out
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

fn block(x: &[f32], b: usize) -> &[f32] {
    &x[b * BLOCK_SIZE..][..BLOCK_SIZE]
}

/// A stepped LFO 2 square → LEVEL at 127 swings it ×¼ ↔ ×4 every half
/// period, a step a block; CLEAN's per-sample ramp spreads each over the
/// block (never snap). Ramped, the gain moves at most 3.75/64 of a sample's
/// level a sample, so the second difference stays near 1.5 % of the ×4
/// peak; a snap at a block edge reaches up to ×(3.75/4) of it.
#[test]
fn modulated_level_ramps_across_each_block() {
    let mut mods = ModState::from_registry(&ModDestRegistry::new(), 8);
    let d = mods.push(LEVEL).unwrap();
    mods.set_route(ModSource::Lfo2.index(), d, 127);
    let out = sine_voice(800, &mods, |_, p| {
        (p.lfos[1].shape, p.lfos[1].rate) = (LfoShape::Square as u8, 5.0);
    });
    let full = sine_voice(800, &ModState::new(), |_, p| p.filter.level = 1.0);
    let peak = full[HELD * BLOCK_SIZE..]
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.01, "{peak}");

    // The route really swings LEVEL end to end (24 dB).
    let levels: Vec<f32> = (HELD..800).map(|b| rms(block(&out, b))).collect();
    let (lo, hi) = levels
        .iter()
        .fold((f32::MAX, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
    assert!(db(hi / lo) > 20.0, "swing {} dB", db(hi / lo));

    let d2 = out[HELD * BLOCK_SIZE..]
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0f32, f32::max);
    assert!(d2 <= 0.05 * peak, "step {} of the ×4 peak", d2 / peak);
}

/// A stored LEVEL jump eases over 20 ms (`StageEase`), not one block: a
/// block after ×¼ → ×4 the gain is still near ×¼, and 100 blocks on it has
/// landed.
#[test]
fn a_stored_level_jump_eases() {
    const JUMP: usize = 300;
    let none = ModState::new();
    let out = sine_voice(JUMP + 101, &none, |b, p| {
        p.filter.level = if b < JUMP { 0.0 } else { 1.0 };
    });
    let full = sine_voice(JUMP + 101, &none, |_, p| p.filter.level = 1.0);
    let at = |b| rms(block(&out, b)) / rms(block(&full, b));
    assert!(at(JUMP) < 0.25, "{} of ×4 a block in", at(JUMP));
    assert!(db(at(JUMP + 100)).abs() < 0.1, "{} dB", db(at(JUMP + 100)));
}
