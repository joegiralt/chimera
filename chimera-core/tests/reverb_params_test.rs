// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The reverb's TYPE, its semantic slots and their maps (reverb TYPEs
//! spec § Parameters, § Tempo sync).

mod common;

use chimera_core::addr::{BlockRead, BlockRef, Blocks};
use chimera_core::block::{Block, DiskCode, ParamId, find_spec};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::dsp::reverb::{
    Burst, DENSITY_DEFAULT, KnobMemory, REVERB_SPECS, RevSync, ReverbParams, ReverbType,
    in_plate_zone, plate_pull,
};
use chimera_core::storage::decode_block;
use common::SR;
use common::rev::{Noise, blocks, knobs_120};

/// The FX's reverb as a `Blocks`, for the codec.
struct Fx<'a>(&'a mut FxParams);

impl BlockRead for Fx<'_> {
    fn block(&self, b: BlockRef) -> Option<chimera_core::addr::BlockView<'_>> {
        (b == BlockRef::Reverb).then_some(chimera_core::addr::BlockView::Plain(&self.0.reverb))
    }
}

impl Blocks for Fx<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<chimera_core::addr::BlockViewMut<'_>> {
        (b == BlockRef::Reverb)
            .then_some(chimera_core::addr::BlockViewMut::Plain(&mut self.0.reverb))
    }
}

/// A Reverb block record: (id, raw 4 bytes)*.
fn record(entries: &[(u8, [u8; 4])]) -> Vec<u8> {
    let mut v = vec![22u8];
    for &(id, raw) in entries {
        v.push(id);
        v.extend_from_slice(&raw);
    }
    v
}

fn real(id: u8, x: f32) -> (u8, [u8; 4]) {
    (id, x.to_le_bytes())
}

fn close(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() <= 1e-3 * want.abs().max(1e-6),
        "{what}: {got} != {want}"
    );
}

#[test]
fn new_ids_and_idents_are_frozen() {
    let want = [
        (6, "TYPE"),
        (7, "TONE"),
        (8, "DIFFUSE"),
        (9, "PREDELAY"),
        (10, "SHAPE"),
        (11, "SMEAR"),
        (12, "RATE"),
        (13, "SYNC"),
        (14, "DENSITY"),
        // REV › DUCK (ADR 0295).
        (15, "DUCK"),
        (16, "DUCK_RELEASE"),
        // 17: the return's SMART EQ role (ADR 0296).
        (17, "ROLE"),
    ];
    for (id, ident) in want {
        assert_eq!(find_spec(&REVERB_SPECS, ParamId(id)).unwrap().ident, ident);
    }
    assert!(find_spec(&REVERB_SPECS, ParamId(0)).is_none());
    assert!(find_spec(&REVERB_SPECS, ParamId(5)).is_none());
    assert_eq!(REVERB_SPECS.len(), 16);
}

#[test]
fn type_and_sync_codes_are_frozen() {
    use ReverbType::*;
    for (t, c) in [
        (Quad, 0),
        (Room, 1),
        (Hall, 2),
        (Gate, 3),
        (Reverse, 4),
        (Bloom, 5),
    ] {
        assert_eq!(t.disk_code(), c);
        assert_eq!(ReverbType::from_disk_code(c), Some(t));
    }
    assert_eq!(ReverbType::from_disk_code(6), None);
    for (s, c) in [(RevSync::Off, 0), (RevSync::Time, 1), (RevSync::All, 2)] {
        assert_eq!(s.disk_code(), c);
        assert_eq!(RevSync::from_disk_code(c), Some(s));
    }
    assert_eq!(RevSync::from_disk_code(3), None);
}

#[test]
fn an_old_project_loads_as_quad() {
    let old = record(&[
        real(5, 0.3),
        real(1, 0.7),
        real(2, 0.3),
        real(3, 0.5),
        real(4, 0.5),
    ]);
    let mut fx = FxParams::default();
    decode_block(&old, &[], &[], Some(&mut Fx(&mut fx))).unwrap();
    let r = fx.reverb;
    let d = ReverbParams::default();
    assert_eq!(r.kind, ReverbType::Quad);
    assert_eq!((r.time, r.damping, r.size, r.mix), (0.7, 0.3, 0.5, 0.5));
    assert_eq!(r.smear, 0.0);
    assert_eq!(
        (r.tone, r.diffuse, r.predelay, r.shape, r.rate, r.sync),
        (d.tone, d.diffuse, d.predelay, d.shape, d.rate, d.sync)
    );
}

/// A HALL Sound saved before DENSITY (no id 14) loads with HALL's
/// starting taps, `DENSITY_DEFAULT`; one that has it keeps it.
#[test]
fn a_hall_sound_without_density_loads_its_default() {
    let old = record(&[real(1, 0.7), real(3, 0.5), real(4, 0.5)]);
    let mut fx = FxParams::default();
    fx.reverb.density = 0.9;
    fx.reverb.kind = ReverbType::Hall;
    let mut fresh = FxParams::default();
    decode_block(&old, &[], &[], Some(&mut Fx(&mut fresh))).unwrap();
    assert_eq!(fresh.reverb.density, DENSITY_DEFAULT);
    let new = record(&[real(1, 0.7), real(14, 0.25)]);
    decode_block(&new, &[], &[], Some(&mut Fx(&mut fx))).unwrap();
    assert_eq!(fx.reverb.density, 0.25);
}

/// Review Focus 2: an unknown TYPE code and NaN in every new slot keep
/// each TYPE's stored state, any TYPE code with NaN slots loads that TYPE,
/// and a second of noise through each stays finite.
#[test]
fn unknown_type_code_and_nan_slots_load_as_safe_quad() {
    let nan_slots = || {
        let mut e: Vec<(u8, [u8; 4])> = (7..=12).map(|id| real(id, f32::NAN)).collect();
        e.push((13, f32::NAN.to_le_bytes()));
        e.extend([real(1, 0.5), real(3, 0.5), real(4, 1.0)]);
        e
    };
    let load = |base: ReverbParams, code: u32| {
        let mut entries = vec![(6u8, code.to_le_bytes())];
        entries.extend(nan_slots());
        let mut fx = FxParams {
            reverb: base,
            ..FxParams::default()
        };
        decode_block(&record(&entries), &[], &[], Some(&mut Fx(&mut fx))).unwrap();
        fx.reverb
    };
    // A file with no TYPE, or one this firmware lacks, is QUAD.
    assert_eq!(load(ReverbParams::default(), 9).kind, ReverbType::Quad);
    let mut loaded = Vec::new();
    for t in ReverbType::ALL {
        let base = ReverbParams::default().with_type(t, &KnobMemory::new());
        let kept = load(base, 9);
        assert_eq!(kept.kind, t, "an unknown code keeps {t:?}");
        let named = load(ReverbParams::default(), t.disk_code().into());
        assert_eq!(named.kind, t);
        assert_eq!(named.sync, RevSync::Off);
        loaded.extend([kept, named]);
    }
    for p in &loaded {
        for id in 7..=12 {
            assert!(p.get(ParamId(id)).is_finite(), "{:?} slot {id}", p.kind);
        }
    }
    // NaN that got past the codec (a CC, a bug) still renders finite.
    let raw: Vec<ReverbParams> = ReverbType::ALL
        .iter()
        .map(|&t| {
            let mut p = ReverbParams::default().with_type(t, &KnobMemory::new());
            for id in [1, 2, 3, 7, 8, 9, 10, 11, 12] {
                p.write(ParamId(id), f32::NAN);
            }
            let k = knobs_120(&p);
            let all = [
                k.length_ms,
                k.rt60_s,
                k.damp_hz,
                k.smear_ms,
                k.rate_hz,
                k.tone_hz,
                k.tail,
                k.regen,
                k.taps,
                k.diffuse_g,
                k.predelay_ms,
                k.shape_db,
                k.wet,
            ];
            // HALL's TONE at 0 (NaN reads 0) is OFF: an open corner, +∞.
            for v in all {
                assert!(
                    v.is_finite() || v == f32::INFINITY && v == k.tone_hz,
                    "{t:?}"
                );
            }
            p
        })
        .collect();
    for params in loaded.iter().chain(&raw) {
        let mut rv = Reverb::new();
        let mut n = Noise(0x5EED);
        for _ in 0..blocks(1.0) {
            let send = core::array::from_fn(|_| 0.5 * n.next());
            let mut out = Stereo::SILENT;
            rv.process(&send, params, 1.0, Tempo::DEFAULT, SR, &mut out);
            assert!(
                out.l.iter().chain(&out.r).all(|s| s.is_finite()),
                "{:?}",
                params.kind
            );
        }
    }
}

#[test]
fn with_type_loads_starting_knobs_and_keeps_mix_and_sync() {
    let mut p = ReverbParams::default();
    (p.mix, p.sync) = (0.7, RevSync::Time);
    let h = p.with_type(ReverbType::Hall, &KnobMemory::new());
    assert_eq!(h.kind, ReverbType::Hall);
    assert_eq!(
        (
            h.size, h.time, h.tone, h.diffuse, h.predelay, h.smear, h.rate, h.damping
        ),
        (
            0.921878, 0.300266, 0.012, 0.383327, 0.058513, 0.0, 0.75, 0.626335
        )
    );
    assert_eq!((h.shape, h.density), (0.878866, DENSITY_DEFAULT));
    assert_eq!((h.mix, h.sync), (0.7, RevSync::Time));
    let q = h.with_type(ReverbType::Quad, &KnobMemory::new());
    assert_eq!((q.smear, q.rate), (0.0, 0.75));
    assert_eq!((q.size, q.time, q.damping), (0.5, 0.5, 0.3));
}

#[test]
fn knob_memory_is_120_bytes_restores_and_clears() {
    assert_eq!(core::mem::size_of::<KnobMemory>(), 120);
    let mut m = KnobMemory::new();
    assert!(m.recall(ReverbType::Room).is_none());
    let mut r = ReverbParams::default().with_type(ReverbType::Room, &m);
    r.size = 0.9;
    m.store(&r);
    let back = r
        .with_type(ReverbType::Hall, &m)
        .with_type(ReverbType::Room, &m);
    assert!((back.size - 0.9).abs() <= 1.0 / 65_534.0);
    assert!(m.recall(ReverbType::Hall).is_none(), "Hall never left");
    m.clear();
    assert!(m.recall(ReverbType::Room).is_none());
}

/// QUAD's laws, as the ring plays them (ADR 0028): the other TYPEs' rows
/// land with their engines (Tasks 8–11).
#[test]
fn quad_knobs_are_the_rings_laws() {
    let at = |f: &dyn Fn(&mut ReverbParams)| {
        let mut p = ReverbParams::default();
        f(&mut p);
        knobs_120(&p)
    };
    close(at(&|p| p.size = 0.0).length_ms, 338.583, "size 0");
    close(at(&|p| p.time = 0.5).rt60_s, 0.3 * 40f32.sqrt(), "time .5");
    close(at(&|p| p.damping = 0.3).damp_hz, 6_050.65, "damp .3");
    close(at(&|p| p.rate = 0.0).rate_hz, 0.1, "rate 0");
    close(at(&|p| p.rate = 0.75).rate_hz, 0.9457, "rate .75");
    close(at(&|p| p.rate = 1.0).rate_hz, 2.0, "rate 1");
    // SMEAR is 0.15 % of the round trip at 1, at every SIZE (ADR 0121).
    for i in 0..=31 {
        let k = at(&|p| (p.size, p.smear) = (i as f32 / 31.0, 1.0));
        close(k.smear_ms, 0.0015 * k.length_ms, "smear cap");
    }
}

/// `KnobMemory`'s zero bytes are a clear memory.
#[test]
fn knob_memory_zeroed_is_clear() {
    // SAFETY: `KnobMemory` is plain `u16`s; the test checks exactly that
    // zero bytes read as `new()`.
    let z: KnobMemory = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
    assert_eq!(z, KnobMemory::new());
    let mut m = KnobMemory::new();
    let mut q = ReverbParams::default();
    (q.size, q.time) = (0.0, 1.0);
    m.store(&q);
    let back = m.recall(ReverbType::Quad).unwrap();
    assert_eq!((back[0], back[2]), (1.0, 0.0), "the ends are exact");
    assert!((back[1] - 0.3).abs() <= 0.5 / 65_534.0);
}

/// ROOM's, HALL's, GATE's and REVERSE's rows (spec § Each TYPE's knobs): every
/// range's ends and the starting knobs' values, to 1e-3.
#[test]
fn knobs_hit_the_spec_endpoints() {
    let at = |t: ReverbType, f: &dyn Fn(&mut ReverbParams)| {
        let mut p = ReverbParams::default().with_type(t, &KnobMemory::new());
        f(&mut p);
        knobs_120(&p)
    };
    // ADR 0095 § GATE and REVERSE, as built.
    // SMEAR's depths and RATE's law, ADR 0121.
    for (t, smear_ms) in [(ReverbType::Gate, 0.85), (ReverbType::Reverse, 2.4)] {
        let k = |f: &dyn Fn(&mut ReverbParams)| at(t, f);
        close(k(&|p| p.tone = 0.0).tone_hz, 12_000.0, "tone 0");
        close(k(&|p| p.tone = 1.0).tone_hz, 1_000.0, "tone 1");
        close(k(&|p| p.smear = 1.0).smear_ms, smear_ms, "smear 1");
        assert_eq!(k(&|p| p.smear = 0.0).smear_ms, 0.0);
        close(k(&|p| p.rate = 0.0).rate_hz, 0.1, "rate 0");
        close(k(&|p| p.rate = 1.0).rate_hz, 2.0, "rate 1");
        close(k(&|_| {}).rate_hz, 0.9457, "rate default");
    }
    let g = |f: &dyn Fn(&mut ReverbParams)| at(ReverbType::Gate, f);
    close(g(&|p| p.size = 0.0).length_ms, 40.0, "gate length 0");
    close(g(&|p| p.size = 1.0).length_ms, 600.0, "gate length 1");
    close(g(&|p| p.time = 0.0).rt60_s, 0.15, "tail 0");
    close(g(&|p| p.time = 1.0).rt60_s, 0.8, "tail 1");
    assert_eq!(g(&|p| p.time = 0.0).tail, 0.0, "TAIL 0 is the fast gate");
    close(g(&|p| p.time = 0.025).tail, 0.5, "tail fades in");
    close(g(&|p| p.time = 0.05).tail, 1.0, "tail in by 0.05");
    close(g(&|p| p.shape = 0.0).shape_db, 0.0, "shape 0");
    close(g(&|p| p.shape = 1.0).shape_db, -30.0, "shape 1");
    close(g(&|p| p.diffuse = 0.0).taps, 6.0, "taps 0");
    close(g(&|p| p.diffuse = 1.0).taps, 12.0, "taps 1");
    close(g(&|p| p.diffuse = 0.0).diffuse_g, 0.2, "g 0");
    close(g(&|p| p.diffuse = 1.0).diffuse_g, 0.8, "g 1");
    // The diffused share, 1.5 × DENSITY: dry at 0, whole from ⅔.
    assert_eq!(g(&|p| p.diffuse = 0.0).wet, 0.0);
    close(g(&|p| p.diffuse = 0.4).wet, 0.6, "wet 0.4");
    assert_eq!(g(&|p| p.diffuse = 0.7).wet, 1.0);
    close(g(&|p| p.predelay = 0.0).predelay_ms, 0.0, "pre 0");
    close(g(&|p| p.predelay = 1.0).predelay_ms, 80.0, "pre 1");
    close(g(&|p| p.damping = 0.0).damp_hz, 12_000.0, "gate damp 0");
    close(g(&|p| p.damping = 1.0).damp_hz, 2_000.0, "gate damp 1");
    assert_eq!(g(&|_| {}).regen, 0.0, "GATE has no REGEN");
    assert_eq!(g(&|_| {}).burst, Burst::Gate);
    let r = |f: &dyn Fn(&mut ReverbParams)| at(ReverbType::Reverse, f);
    close(r(&|p| p.size = 0.0).length_ms, 150.0, "reverse length 0");
    close(r(&|p| p.size = 1.0).length_ms, 800.0, "reverse length 1");
    assert_eq!(r(&|p| p.time = 0.0).regen, 0.0);
    close(r(&|p| p.time = 1.0).regen, 0.8, "regen 1");
    // −1.94 dB a pass: 24.8 s at 800 ms.
    close(
        r(&|p| (p.time, p.size) = (1.0, 1.0)).rt60_s,
        24.77,
        "regen rt60",
    );
    close(r(&|p| p.shape = 0.0).shape_db, 0.0, "curve 0");
    close(r(&|p| p.shape = 1.0).shape_db, 55.0, "curve 1");
    close(r(&|p| p.diffuse = 0.0).diffuse_g, 0.0, "reverse g 0");
    close(r(&|p| p.diffuse = 1.0).diffuse_g, 0.7, "reverse g 1");
    assert_eq!(r(&|p| p.diffuse = 0.0).wet, 1.0, "REVERSE is all diffused");
    close(r(&|p| p.predelay = 1.0).predelay_ms, 150.0, "onset 1");
    assert_eq!(r(&|_| {}).tail, 0.0, "REVERSE has no TAIL");
    assert_eq!(r(&|_| {}).damp_hz, 0.0, "REVERSE has no DAMP");
    assert_eq!(r(&|_| {}).burst, Burst::Reverse);
    let m = |f: &dyn Fn(&mut ReverbParams)| at(ReverbType::Room, f);
    // SIZE is loop A's round trip, `lin` in k 0.4 → 1 (ADR 0095): 42.3 ms
    // of allpasses and 94.6 ms of delays × k.
    close(m(&|p| p.size = 0.0).length_ms, 80.14, "room size 0");
    close(m(&|p| p.size = 0.5).length_ms, 108.52, "room size ½");
    close(m(&|p| p.size = 1.0).length_ms, 136.9, "room size 1");
    close(m(&|p| p.time = 0.0).rt60_s, 0.2, "decay 0");
    close(m(&|p| p.time = 1.0).rt60_s, 2.0, "decay 1");
    close(m(&|p| p.damping = 0.0).damp_hz, 10_000.0, "room damp 0");
    close(m(&|p| p.damping = 1.0).damp_hz, 1_500.0, "room damp 1");
    close(m(&|p| p.shape = 0.0).shape_db, 0.0, "loops 0");
    close(m(&|p| p.shape = 1.0).shape_db, 90.0, "loops 1");
    close(m(&|p| p.tone = 0.0).tone_hz, 10_000.0, "room tone 0");
    close(m(&|p| p.tone = 1.0).tone_hz, 2_500.0, "room tone 1");
    close(m(&|p| p.diffuse = 0.0).diffuse_g, 0.45, "diffuse 0");
    close(m(&|p| p.diffuse = 1.0).diffuse_g, 0.70, "diffuse 1");
    close(m(&|p| p.predelay = 0.0).predelay_ms, 0.0, "pre 0");
    close(m(&|p| p.predelay = 1.0).predelay_ms, 50.0, "pre 1");
    close(
        m(&|p| (p.smear, p.size) = (1.0, 1.0)).smear_ms,
        1.65,
        "smear 1, under the cap",
    );
    close(
        m(&|p| (p.smear, p.size) = (1.0, 0.0)).smear_ms,
        0.014 * 80.14,
        "smear 1, at the cap",
    );
    close(m(&|p| p.rate = 0.0).rate_hz, 0.1, "room rate 0");
    close(m(&|p| p.rate = 1.0).rate_hz, 2.0, "room rate 1");
    let d = m(&|_| {});
    // MidiVerb II program 5's fitted knobs (ADR 0095), SMEAR 0: the program exactly.
    close(d.length_ms, 116.69, "room size default");
    close(d.rt60_s, 1.045, "room decay default");
    close(d.tone_hz, 10_000.0, "room tone default");
    close(d.diffuse_g, 0.45, "room diffuse default");
    close(d.predelay_ms, 4.25, "room pre default");
    close(d.damp_hz, 4_456.7, "room damp default");
    close(d.shape_db, 57.06, "room loops default");
    close(d.smear_ms, 0.0, "room smear default");
    close(d.rate_hz, 0.9457, "room rate default");
    // HALL, above its plate zone (SIZE ≥ 0.05).
    let h = |f: &dyn Fn(&mut ReverbParams)| at(ReverbType::Hall, f);
    close(h(&|p| p.size = 0.0).length_ms, 120.0, "hall size 0");
    close(h(&|p| p.size = 1.0).length_ms, 600.0, "hall size 1");
    close(h(&|p| p.time = 0.0).rt60_s, 0.8, "hall decay 0");
    close(h(&|p| p.size = 0.5).length_ms, 360.0, "hall size ½: lin");
    close(h(&|p| p.time = 1.0).rt60_s, 16.0, "hall decay 1");
    assert_eq!(
        h(&|p| p.tone = 0.0).tone_hz,
        f32::INFINITY,
        "hall tone 0: OFF"
    );
    close(
        h(&|p| p.tone = 1.0 / 128.0).tone_hz,
        12_000.0 * (1.0f32 / 6.0).powf(1.0 / 128.0),
        "hall tone 1 tick",
    );
    close(h(&|p| p.tone = 1.0).tone_hz, 2_000.0, "hall tone 1");
    close(h(&|p| p.diffuse = 0.0).diffuse_g, 0.3, "hall diffuse 0");
    close(h(&|p| p.diffuse = 1.0).diffuse_g, 0.8, "hall diffuse 1");
    close(h(&|p| p.damping = 0.0).damp_hz, 14_000.0, "hall damp 0");
    close(h(&|p| p.damping = 1.0).damp_hz, 1_300.0, "hall damp 1");
    close(h(&|p| p.shape = 0.0).spread.scale(), 0.05, "hall spread 0");
    close(h(&|p| p.shape = 1.0).spread.scale(), 1.10, "hall spread 1");
    close(h(&|p| p.density = 0.0).density.get(), 3.0, "hall density 0");
    close(h(&|p| p.density = 1.0).density.get(), 8.0, "hall density 1");
    close(h(&|p| p.predelay = 1.0).predelay_ms, 125.0, "hall pre 1");
    assert_eq!(h(&|p| p.predelay = 0.0).predelay_ms, 0.0);
    close(
        h(&|p| (p.smear, p.size) = (1.0, 1.0)).smear_ms,
        0.6,
        "hall smear 1",
    );
    close(
        h(&|p| (p.smear, p.size) = (1.0, 0.05)).smear_ms,
        0.0011 * 144.0,
        "hall smear 1, at the cap",
    );
    close(h(&|p| p.rate = 0.0).rate_hz, 0.1, "hall rate 0");
    close(h(&|p| p.rate = 1.0).rate_hz, 2.0, "hall rate 1");
    let d = h(&|_| {});
    close(d.length_ms, 562.50146, "hall size default");
    close(d.rt60_s, 1.96673, "hall decay default");
    close(d.tone_hz, 11_744.74, "hall tone default");
    close(d.diffuse_g, 0.4916635, "hall diffuse default");
    close(d.damp_hz, 3_159.62, "hall damp default");
    close(d.predelay_ms, 7.314125, "hall pre default");
    assert_eq!(d.smear_ms, 0.0, "hall smear default: START is the preset");
    close(d.rate_hz, 0.9457, "hall rate default");
    close(d.spread.scale(), 0.9728093, "hall spread default");
    close(d.density.get(), 4.03649, "hall density default");
    // HALL's DAMP is its own law now (ADR 0095 § HALL, as built), not
    // QUAD's: one damper a loop, 14 → 1.3 kHz.
    for p in [0.0, 0.3, 0.7, 1.0] {
        let want = 14_000.0 * (1_300.0f32 / 14_000.0).powf(p);
        close(h(&|q| q.damping = p).damp_hz, want, "hall damp law");
    }
}

/// Below SIZE 0.05, HALL plays DIFFUSE pulled up and TONE and PRE-DLY
/// down by w = 1 − SIZE/0.05; the stored knobs never change, and no other
/// TYPE has the zone.
#[test]
fn plate_zone_pulls_diffuse_tone_predelay_spread() {
    let p = |size: f32| ReverbParams {
        kind: ReverbType::Hall,
        size,
        diffuse: 0.4,
        tone: 0.6,
        predelay: 0.5,
        shape: 0.8,
        ..ReverbParams::default()
    };
    let eff = |size: f32| {
        let q = plate_pull(&p(size));
        [q.diffuse, q.tone, q.predelay, q.shape]
    };
    let near = |a: [f32; 4], b: [f32; 4]| {
        assert!(
            a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6),
            "{a:?} != {b:?}"
        );
    };
    near(eff(0.0), [1.0, 0.0, 0.0, 0.0]);
    near(eff(0.025), [0.7, 0.3, 0.25, 0.4]);
    for size in [0.05, 0.5, 1.0] {
        near(eff(size), [0.4, 0.6, 0.5, 0.8]);
    }
    assert!(in_plate_zone(&p(0.049)) && !in_plate_zone(&p(0.05)));
    // The engine plays the pulled knobs; the stored ones stay.
    let k = knobs_120(&p(0.0));
    assert_eq!(
        (k.diffuse_g, k.tone_hz, k.predelay_ms, k.spread.scale()),
        (0.8, f32::INFINITY, 0.0, 0.05)
    );
    assert_eq!(p(0.0).diffuse, 0.4);
    let room = ReverbParams {
        kind: ReverbType::Room,
        ..p(0.0)
    };
    assert_eq!(plate_pull(&room), room);
    assert!(!in_plate_zone(&room));
}

/// SMEAR 1 never passes its TYPE's cap at any SIZE (§ The SMEAR bound):
/// D ≤ 0.72 % of ROOM's loop, 0.36 % of HALL's, 0.18 % of QUAD's ring.
/// A GATE or REVERSE saved before PRE-DLY, DAMP and ONSET (ADR 0095 §
/// GATE and REVERSE, as built) loads whatever those slots held, from
/// another TYPE or a CC: at either end of each, it loads in range and two
/// seconds of noise stay finite, peaking at most twice the TYPE's own at
/// its longest TAIL or REGEN. It need not sound the same.
#[test]
fn an_old_gate_or_reverse_loads_safely() {
    let peak_of = |t: ReverbType, p: &ReverbParams| {
        let mut rv = Reverb::of_type(t);
        let mut n = Noise(0xD1CE);
        let mut peak = 0.0f32;
        for _ in 0..blocks(2.0) {
            let send = core::array::from_fn(|_| 0.5 * n.next());
            let mut out = Stereo::SILENT;
            rv.process(&send, p, 1.0, Tempo::DEFAULT, SR, &mut out);
            for s in out.l.iter().chain(&out.r) {
                assert!(s.is_finite(), "{t:?}");
                peak = peak.max(s.abs());
            }
        }
        peak
    };
    for (t, code) in [(ReverbType::Gate, 3u32), (ReverbType::Reverse, 4)] {
        let own = ReverbParams {
            time: 1.0,
            ..ReverbParams::default().with_type(t, &KnobMemory::new())
        };
        let bound = 2.0 * peak_of(t, &own);
        for (pre, damp, shape) in [(0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (1.0, 0.0, 0.67)] {
            let entries = [
                (6u8, code.to_le_bytes()),
                real(1, 1.0),
                real(2, damp),
                real(3, 0.0),
                real(4, 1.0),
                real(8, 1.0),
                real(9, pre),
                real(10, shape),
            ];
            let mut fx = FxParams::default();
            decode_block(&record(&entries), &[], &[], Some(&mut Fx(&mut fx))).unwrap();
            let p = fx.reverb;
            assert_eq!(p.kind, t);
            let k = knobs_120(&p);
            let r = chimera_core::dsp::reverb::ranges(t).unwrap();
            assert!(k.predelay_ms <= r.predelay_ms.1 && k.length_ms >= r.length_ms.lo);
            let peak = peak_of(t, &p);
            assert!(
                peak <= bound,
                "{t:?} PRE {pre} DAMP {damp}: {peak} over {bound}"
            );
        }
    }
}

#[test]
fn smear_caps_hold_at_every_size() {
    for (t, cap) in [
        (ReverbType::Room, 0.014),
        (ReverbType::Hall, 0.0011),
        (ReverbType::Bloom, 0.0012),
        (ReverbType::Quad, 0.0015),
    ] {
        let mut p = ReverbParams::default().with_type(t, &KnobMemory::new());
        p.smear = 1.0;
        for i in 0..=128 {
            p.size = i as f32 / 128.0;
            let k = knobs_120(&p);
            assert!(
                k.smear_ms <= cap * k.length_ms * (1.0 + 1e-6),
                "{t:?} SIZE {}: {} ms of {} ms",
                p.size,
                k.smear_ms,
                k.length_ms
            );
        }
    }
}
