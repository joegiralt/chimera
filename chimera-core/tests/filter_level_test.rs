// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! LEVEL (Filter 12): its law, and DRIVE's (Filter 2, retired) translation
//! and re-address on load (spec § LEVEL, ADR 0096).

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::DiskCode;
use chimera_core::dsp::algo::params::AlgoOpParams;
use chimera_core::dsp::algo::waves::WaveId;
use chimera_core::dsp::filter::{
    Clean, FilterMods, LevelRamp, LevelStage, clip_amount, clipped, level_from_drive, level_gain,
};
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::lfo::LfoShape;
use chimera_core::dsp::voice_fx::params::{FxType, VoiceFxParams};
use chimera_core::factory::factory_sound;
use chimera_core::meter::{Clip, FILTER_CLIP};
use chimera_core::mod_path::{LABEL_LEN, ModDestRegistry};
use chimera_core::modulation::{ModSource, ModState};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot};
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    FileKind, Generation, Header, MIGRATIONS, RecordTag, TRANSLATIONS, decode_block, write_file,
};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::Rig;
use common::codec_util::{VecSink, decode};
use common::sweep::{Bench, Param, Patch, Tape, registry};
use std::sync::Mutex;

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
        rig.render_glided(&mut buf, &p, mods);
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

/// A stored LEVEL jump eases over 20 ms (the Part's knobs, ADR 0112), not one block: a
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

// ── The clipper, its gate fade and the LED (Task 7) ───────────────────

/// `FILTER_CLIP` is global: the tests here that render an Instrument take
/// turns, so none reads a meter another just published.
static METER: Mutex<()> = Mutex::new(());

fn sine(amp: f32, b: usize) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| amp * ((b * BLOCK_SIZE + i) as f32 * 0.0731).sin())
}

/// A full-scale saw, ±1, at 110 Hz.
fn saw_block(b: usize) -> [f32; BLOCK_SIZE] {
    core::array::from_fn(|i| ((b * BLOCK_SIZE + i) % 436) as f32 / 218.0 - 1.0)
}

/// Gain ×2 (LEVEL 0.75): a 0.4 sine peaks at 0.8 and passes exactly; a
/// 0.6 one (1.2) is clipped, so the clipper is in.
#[test]
fn the_clipper_is_transparent_to_one() {
    let g = level_gain(0.75);
    assert_eq!(g, 2.0);
    let lv = LevelRamp::new((g, g), &FilterMods::NONE);
    assert!(lv.open());
    let mut st = LevelStage::default();
    for b in 0..40 {
        let x = sine(0.4, b);
        let mut y = x;
        st.run(&mut y, lv);
        for (a, b) in x.iter().zip(&y) {
            assert!((a * 2.0 - b).abs() <= 1e-6, "{a} × 2 → {b}");
        }
    }
    let mut y = sine(0.6, 40);
    st.run(&mut y, lv);
    let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak < 1.19, "{peak}");
}

/// A fresh note past unity starts clipped: nothing came before it to fade
/// from, so it plays as a filter whose clipper had long faded in (one fed
/// silence, so its states are as fresh), not 20 ms of ×2 unclipped.
#[test]
fn a_fresh_filter_clips_from_its_first_sample() {
    let mut p = FilterParams::default();
    p.level = 0.75;
    let mut warm = Clean::new();
    for _ in 0..20 {
        warm.process(&mut [0.0; BLOCK_SIZE], &p, 48_000, &FilterMods::NONE);
    }
    let (mut a, mut b) = (sine(0.9, 3), sine(0.9, 3));
    Clean::new().process(&mut a, &p, 48_000, &FilterMods::NONE);
    warm.process(&mut b, &p, 48_000, &FilterMods::NONE);
    assert_eq!(a, b);
    // A voice reused from a note at LEVEL ½ (gate shut) starts fresh too.
    let mut reused = Clean::new();
    let mut half = p;
    half.level = 0.5;
    for _ in 0..20 {
        reused.process(&mut [0.0; BLOCK_SIZE], &half, 48_000, &FilterMods::NONE);
    }
    reused.reset();
    let mut c = sine(0.9, 3);
    reused.process(&mut c, &p, 48_000, &FilterMods::NONE);
    assert_eq!(c, a);
}

/// The bench: P1 plays `wave` on OP A alone (0.94 full scale), dry,
/// through DRIVE at `drive` and `tone` (MIX 1; DRIVE's wet reaches ±1.5 at
/// TONE 0) and a wide-open CLEAN, so a step reaches the output. Its LEVEL
/// param.
fn bench(wave: WaveId, drive: f32, tone: f32) -> (Bench, Param) {
    let mut b = Bench::new(Patch::Probe, PartParams::default(), FxParams::OFF);
    let reg = registry(false);
    let find = |block: fn(BlockRef) -> bool, id| {
        *reg.iter()
            .find(|p| block(p.block) && p.spec.id == id)
            .expect("a param")
    };
    let op_a = |b| matches!(b, BlockRef::AlgoOp(o) if o.index() == 0);
    b.set(&find(op_a, AlgoOpParams::WAVE), f32::from(wave.get()));
    let fx = |b| b == BlockRef::VoiceFx;
    let soft = FxType::LIST
        .iter()
        .position(|&t| t == FxType::Soft)
        .unwrap();
    b.set(&find(fx, VoiceFxParams::TYPE), soft as f32);
    b.set(&find(fx, VoiceFxParams::P1), drive);
    b.set(&find(fx, VoiceFxParams::P2), tone);
    // SOFT's `MAKE_UP` (0.483) level-matched the wet; LEVEL 0.7625 (x2.07)
    // gives the sine its old peak.
    b.set(&find(fx, VoiceFxParams::P3), 0.7625);
    b.set(&find(fx, VoiceFxParams::MIX), 1.0);
    let flt = |b| b == BlockRef::Filter;
    b.set(&find(flt, FilterParams::CUTOFF), 20_000.0);
    (b, find(flt, FilterParams::LEVEL))
}

/// The bill's flag through a voice: the clipper runs past unity, and on
/// after LEVEL is set back to ½ while the ease lands and the fade falls;
/// then it stops.
#[test]
fn a_voice_reports_its_clipper_running() {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = chimera_core::dsp::algo::params::AlgoParams::single(WaveId::SAW);
    p.filter.level = 0.75;
    let mut rig = Rig::new(chimera_hal::SAMPLE_RATE);
    rig.note_on(MidiNote::new(45).unwrap(), Velocity::new(127).unwrap(), &p);
    let mut buf = [0.0f32; BLOCK_SIZE];
    for _ in 0..20 {
        rig.render_glided(&mut buf, &p, &ModState::new());
        assert!(rig.voice.filter_level_running());
    }
    p.filter.level = 0.5;
    let mut on = 0;
    for _ in 0..400 {
        rig.render_glided(&mut buf, &p, &ModState::new());
        if rig.voice.filter_level_running() {
            on += 1;
        }
    }
    println!("running {on} blocks after LEVEL 0.75 → ½");
    assert!(on > 15 && on < 400, "{on}");
    assert!(!rig.voice.filter_level_running());
}

/// LEVEL 0.5 ↔ 0.51 every 100 ms for 2 s, the gate opening and shutting on
/// a sine DRIVE pushes to ±1.4: the 20 ms fade, never a step. Not a saw:
/// its own edge is a second difference of 1.9 a period, which hides a
/// gate's step (a hard gate passes on a saw); a sine's is 0.004.
#[test]
fn crossing_unity_does_not_step() {
    let _m = METER.lock().unwrap_or_else(|e| e.into_inner());
    const EVERY: usize = 75; // 100 ms
    const START: usize = 150;
    let (mut b, level) = bench(WaveId::W1, 0.32, 0.0);
    // B♭2: 100 ms is 11.65 periods, so the events fall at every phase (at
    // A2's 11.00 each would meet the sine where it was).
    b.note(46, Some(127));
    let mut tape = Tape::default();
    let (mut events, mut lit) = (Vec::new(), 0.0f32);
    for blk in 0..START + 20 * EVERY + 80 {
        if blk >= START && (blk - START).is_multiple_of(EVERY) && events.len() < 20 {
            b.set(&level, if events.len() % 2 == 0 { 0.51 } else { 0.5 });
            events.push(blk);
        }
        b.render(&mut tape);
        lit = lit.max(FILTER_CLIP[0].take().get());
    }
    // The gate really opened on a saw past ±1: the clipper ran, and bit.
    println!("LED at most {lit}");
    assert!(lit > 0.0, "{lit}");
    for (i, &e) in events.iter().enumerate() {
        let prev = if i == 0 { 0 } else { events[i - 1] };
        let next = events.get(i + 1).copied().unwrap_or(usize::MAX);
        let c = tape.click(0, e, (prev, next), true);
        assert!(c.is_none(), "event {i} at block {e}: click ×{c:?}");
    }
}

/// The stage alone, its gate toggled every 100 ms with no ease in the way
/// (in a voice `StageEase` holds LEVEL off ½ about 65 blocks after it is set
/// back, so the gate there shuts once a cycle at most): a 1.4 sine, gain
/// 1 ↔ 1.03, the events at every phase. The fade moves the blend 1/960 a
/// sample, so the largest second difference stays within 10 % of the sine's
/// own held clipped (`sat_c1`'s knee bends it) plus the gain ramp's corner;
/// a hard gate steps by up to `y − sat_c1(y)`, 0.13.
#[test]
fn the_gate_fades_both_ways() {
    let toggled = gate_run(|b| if (b / 75) % 2 == 1 { 1.03 } else { 1.0 });
    let held = gate_run(|_| 1.03);
    // The gain ramp's own corner at a block edge: |x|·Δg/64.
    let corner = 1.4 * 0.03 / BLOCK_SIZE as f32;
    println!("largest second difference {toggled}, held open {held}, corner {corner}");
    assert!(
        toggled <= 1.1 * (held + corner),
        "{toggled} against {held} + {corner}"
    );
}

/// The fade is 20 ms, 15 blocks, both ways: a held 1.4 sine at gain 1, the
/// gate opened (a route on LEVEL) at block 0. Block 7's blend of clipped
/// into dry lies in 7/15 ± 1/15 on every sample the knee bends; block 15 is
/// `clipped` exactly. Shut again, the same toward dry, and block 15 is the
/// dry exactly. `running` holds through the fade-out and drops after it.
#[test]
fn the_fade_takes_20_ms_both_ways() {
    let x = |b: usize| -> [f32; BLOCK_SIZE] {
        core::array::from_fn(|i| 1.4 * ((b * BLOCK_SIZE + i) as f32 * 0.21).sin())
    };
    let routed = FilterMods {
        level_routed: true,
        ..FilterMods::NONE
    };
    let (open, shut) = (
        LevelRamp::new((1.0, 1.0), &routed),
        LevelRamp::new((1.0, 1.0), &FilterMods::NONE),
    );
    let mut st = LevelStage::default();
    // Primed shut.
    st.run(&mut x(0), shut);
    assert!(!st.running());
    for (lv, toward) in [(open, 1.0f32), (shut, 0.0)] {
        for b in 0..=15 {
            let dry = x(b);
            let mut y = dry;
            st.run(&mut y, lv);
            assert_eq!(st.running(), lv.open() || b < 14, "block {b}");
            let blends: Vec<f32> = dry
                .iter()
                .zip(&y)
                .filter(|(d, _)| (clipped(**d) - **d).abs() > 0.05)
                .map(|(d, y)| (y - d) / (clipped(*d) - d))
                .collect();
            assert!(blends.len() > 4, "block {b}");
            let want = if toward == 1.0 {
                b as f32 / 15.0
            } else {
                1.0 - b as f32 / 15.0
            };
            if b == 7 {
                for f in &blends {
                    assert!(
                        (f - want).abs() <= 1.0 / 15.0 + 1e-4,
                        "block 7: {f}, want {want}"
                    );
                }
            }
            if b == 15 {
                let exact: [f32; BLOCK_SIZE] =
                    dry.map(|d| if toward == 1.0 { clipped(d) } else { d });
                assert_eq!(y, exact, "block 15 toward {toward}");
            }
        }
    }
    assert!(!st.running());
}

/// The largest second difference of a 1.4 sine through a `LevelStage` whose
/// block `b` ends at gain `gain(b)`, over 20 × 100 ms.
fn gate_run(gain: impl Fn(usize) -> f32) -> f32 {
    let x = |n: usize| 1.4 * (n as f32 * 0.0153).sin();
    let mut st = LevelStage::default();
    let mut out = Vec::new();
    for b in 0..20 * 75 {
        let from = if b == 0 { gain(0) } else { gain(b - 1) };
        let mut buf: [f32; BLOCK_SIZE] = core::array::from_fn(|i| x(b * BLOCK_SIZE + i));
        st.run(&mut buf, LevelRamp::new((from, gain(b)), &FilterMods::NONE));
        out.extend_from_slice(&buf);
    }
    out.windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
        .fold(0.0f32, f32::max)
}

/// A held saw through a voice at `level` and `amp`: the voice's last clip.
fn voice_clip(level: f32, amp: f32) -> f32 {
    let mut p = ParamSnapshot::for_engine(EngineType::Algo);
    p.algo = chimera_core::dsp::algo::params::AlgoParams::single(WaveId::SAW);
    p.algo.ops[0].level = (99.0 * amp) as u8;
    (p.filter.cutoff, p.filter.level) = (20_000.0, level);
    let mut rig = Rig::new(chimera_hal::SAMPLE_RATE);
    rig.note_on(MidiNote::new(45).unwrap(), Velocity::new(127).unwrap(), &p);
    let mut buf = [0.0f32; BLOCK_SIZE];
    let mut most = 0.0f32;
    for b in 0..HELD + 20 {
        rig.render(&mut buf, &p, &ModState::new());
        if b >= HELD {
            most = most.max(rig.voice.filter_clip().get());
        }
    }
    most
}

/// The LED: 0 at LEVEL ½ whatever the signal, 0 while `x·gain` stays in ±1,
/// rising with LEVEL on a full-scale saw until it fills at `x·gain` = 3.
#[test]
fn led_is_zero_when_off_and_rises_with_level() {
    // Gate shut: off at any signal, a voice's and the stage's.
    assert_eq!(voice_clip(0.5, 1.0), 0.0);
    let shut = LevelRamp::new((1.0, 1.0), &FilterMods::NONE);
    let mut st = LevelStage::default();
    let mut x = saw_block(0).map(|v| v * 3.0);
    st.run(&mut x, shut);
    assert_eq!(st.clip(), Clip::ZERO);
    // At most 1 after the gain, at any LEVEL past ½: 0.
    for l in [0.55, 0.7, 0.85, 1.0] {
        let g = level_gain(l);
        let mut c = Clean::new();
        let mut p = FilterParams::default();
        p.level = l;
        for b in 0..20 {
            let mut x = saw_block(b).map(|v| v / g);
            assert_eq!(
                c.process(&mut x, &p, 48_000, &FilterMods::NONE),
                Clip::ZERO,
                "LEVEL {l}"
            );
        }
    }
    // A full-scale saw (the engine's, 0.94): strictly rising, never past 1.
    let mut last = 0.0f32;
    for l in [0.6, 0.7, 0.8, 0.9, 1.0] {
        let clip = voice_clip(l, 1.0);
        println!("LEVEL {l}: {clip}");
        assert!(clip > last && clip <= 1.0, "LEVEL {l}: {clip} after {last}");
        last = clip;
    }
    // The stage reads `clip_amount` of its block's peak.
    let mut c = Clean::new();
    let mut p = FilterParams::default();
    p.level = 0.7;
    let mut most = 0.0f32;
    for b in 0..20 {
        most = most.max(
            c.process(&mut saw_block(b), &p, 48_000, &FilterMods::NONE)
                .get(),
        );
    }
    let want = clip_amount(level_gain(0.7)).get();
    assert!((most - want).abs() <= 1e-3, "{most}, want {want}");
}

/// Two voices of P1, OP A velocity-sensitive, at velocities that clip about
/// 0.3 and 0.7 at LEVEL 1: the Part's meter holds the most of its voices'
/// clips until taken, and the idle Parts read 0.
#[test]
fn part_meter_is_the_max_over_its_voices() {
    let _m = METER.lock().unwrap_or_else(|e| e.into_inner());
    let (mut b, level) = bench(WaveId::SAW, 0.0, 0.0);
    let reg = registry(false);
    let vs = *reg
        .iter()
        .find(|p| {
            matches!(p.block, BlockRef::AlgoOp(o) if o.index() == 0)
                && p.spec.id == AlgoOpParams::VELOCITY
        })
        .expect("OP A's VELOCITY");
    b.set(&vs, vs.spec.max);
    b.set(&level, 1.0);
    b.note(45, Some(82));
    b.note(57, Some(104));
    let mut tape = Tape::default();
    let mut most = [0.0f32; 8];
    // The UI takes the meter every block, then every fourth: it reads the
    // most since, not the last block's, so no block between frames is
    // missed.
    for p in &FILTER_CLIP {
        p.take();
    }
    let mut since = Clip::ZERO;
    for blk in 0..HELD {
        b.render(&mut tape);
        let clips = b.filter_clips();
        since = clips.iter().fold(since, |m, &c| m.max(c));
        if blk < HELD / 2 || blk % 4 == 3 {
            assert_eq!(FILTER_CLIP[0].take(), since, "block {blk}: {clips:?}");
            since = Clip::ZERO;
            for (p, m) in FILTER_CLIP.iter().enumerate().skip(1) {
                assert_eq!(m.take(), Clip::ZERO, "P{}", p + 1);
            }
        }
        if blk >= HELD - 20 {
            for (m, c) in most.iter_mut().zip(clips) {
                *m = m.max(c.get());
            }
        }
    }
    let mut on: Vec<f32> = most.into_iter().filter(|&c| c > 0.0).collect();
    on.sort_by(f32::total_cmp);
    println!("voices {on:?}");
    assert_eq!(on.len(), 2, "{most:?}");
    assert!(
        (on[0] - 0.3).abs() < 0.05 && (on[1] - 0.7).abs() < 0.05,
        "{on:?}"
    );
}
