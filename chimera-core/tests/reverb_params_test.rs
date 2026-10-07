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
    KnobMemory, REVERB_SPECS, RevSync, ReverbParams, ReverbType, damp_hz, in_plate_zone, plate_pull,
};
use chimera_core::storage::decode_block;
use common::SR;
use common::rev::{Noise, blocks, knobs_120};

/// The FX's reverb as a `Blocks`, for the codec.
struct Fx<'a>(&'a mut FxParams);

impl BlockRead for Fx<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        (b == BlockRef::Reverb).then_some(&self.0.reverb as &dyn Block)
    }
}

impl Blocks for Fx<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        (b == BlockRef::Reverb).then_some(&mut self.0.reverb as &mut dyn Block)
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
    ];
    for (id, ident) in want {
        assert_eq!(find_spec(&REVERB_SPECS, ParamId(id)).unwrap().ident, ident);
    }
    assert!(find_spec(&REVERB_SPECS, ParamId(0)).is_none());
    assert!(find_spec(&REVERB_SPECS, ParamId(5)).is_none());
    assert_eq!(REVERB_SPECS.len(), 12);
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
            ];
            for v in all {
                assert!(v.is_finite(), "{t:?}");
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
        (0.65, 0.45, 0.3, 0.4, 0.15, 0.35, 0.4, 0.3)
    );
    assert_eq!((h.mix, h.sync), (0.7, RevSync::Time));
    let q = h.with_type(ReverbType::Quad, &KnobMemory::new());
    assert_eq!((q.smear, q.rate), (0.0, 0.5));
    assert_eq!((q.size, q.time, q.damping), (0.5, 0.5, 0.3));
}

#[test]
fn knob_memory_is_108_bytes_restores_and_clears() {
    assert_eq!(core::mem::size_of::<KnobMemory>(), 108);
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
    close(at(&|p| p.rate = 0.5).rate_hz, 0.2236, "rate .5");
    // SMEAR is 0.18 % of the round trip at 1, at every SIZE.
    for i in 0..=31 {
        let k = at(&|p| (p.size, p.smear) = (i as f32 / 31.0, 1.0));
        close(k.smear_ms, 0.0018 * k.length_ms, "smear cap");
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
    for t in [ReverbType::Gate, ReverbType::Reverse] {
        let k = |f: &dyn Fn(&mut ReverbParams)| at(t, f);
        close(k(&|p| p.tone = 0.0).tone_hz, 10_000.0, "tone 0");
        close(k(&|p| p.tone = 1.0).tone_hz, 2_500.0, "tone 1");
        close(k(&|p| p.diffuse = 0.0).taps, 6.0, "taps 0");
        close(k(&|p| p.diffuse = 1.0).taps, 12.0, "taps 1");
        close(k(&|p| p.diffuse = 0.0).diffuse_g, 0.5, "g 0");
        close(k(&|p| p.diffuse = 1.0).diffuse_g, 0.7, "g 1");
        close(k(&|p| p.smear = 1.0).smear_ms, 1.0, "smear 1");
        assert_eq!(k(&|p| p.smear = 0.0).smear_ms, 0.0);
        close(k(&|p| p.rate = 0.0).rate_hz, 0.1, "rate 0");
        close(k(&|p| p.rate = 1.0).rate_hz, 2.0, "rate 1");
        close(k(&|_| {}).rate_hz, 0.4472, "rate default");
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
    close(g(&|p| p.shape = 1.0).shape_db, -18.0, "shape 1");
    assert_eq!(g(&|_| {}).regen, 0.0, "GATE has no REGEN");
    let d = g(&|_| {});
    close(d.length_ms, 177.27, "gate length default");
    close(d.rt60_s, 0.2479, "tail default");
    close(d.tone_hz, 7_578.6, "gate tone default");
    close(d.taps, 10.2, "gate taps default");
    close(d.diffuse_g, 0.64, "gate g default");
    close(d.shape_db, -3.6, "shape default");
    assert!(!d.rising);
    let r = |f: &dyn Fn(&mut ReverbParams)| at(ReverbType::Reverse, f);
    close(r(&|p| p.size = 0.0).length_ms, 150.0, "reverse length 0");
    close(r(&|p| p.size = 1.0).length_ms, 800.0, "reverse length 1");
    assert_eq!(r(&|p| p.time = 0.0).regen, 0.0);
    close(r(&|p| p.time = 1.0).regen, 0.708, "regen 1");
    // −3 dB a pass: 16 s at 800 ms.
    close(
        r(&|p| (p.time, p.size) = (1.0, 1.0)).rt60_s,
        16.0,
        "regen rt60",
    );
    close(r(&|p| p.shape = 0.0).shape_db, 12.0, "curve 0");
    close(r(&|p| p.shape = 1.0).shape_db, 30.0, "curve 1");
    assert_eq!(r(&|_| {}).tail, 0.0, "REVERSE has no TAIL");
    let d = r(&|_| {});
    close(d.length_ms, 346.41, "reverse length default");
    assert_eq!((d.regen, d.rt60_s), (0.0, 0.0));
    close(d.tone_hz, 10_000.0, "reverse tone default");
    close(d.taps, 10.8, "reverse taps default");
    close(d.diffuse_g, 0.66, "reverse g default");
    close(d.shape_db, 24.06, "curve default");
    assert!(d.rising);
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
        0.0072 * 136.9,
        "smear 1, at the cap",
    );
    close(m(&|p| p.rate = 0.0).rate_hz, 0.1, "room rate 0");
    close(m(&|p| p.rate = 1.0).rate_hz, 1.0, "room rate 1");
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
    close(d.rate_hz, 0.3162, "room rate default");
    // HALL, above its plate zone (SIZE ≥ 0.15).
    let h = |f: &dyn Fn(&mut ReverbParams)| at(ReverbType::Hall, f);
    close(h(&|p| p.size = 0.0).length_ms, 120.0, "hall size 0");
    close(h(&|p| p.size = 1.0).length_ms, 600.0, "hall size 1");
    close(h(&|p| p.time = 0.0).rt60_s, 0.8, "hall decay 0");
    close(h(&|p| p.time = 1.0).rt60_s, 10.0, "hall decay 1");
    close(h(&|p| p.tone = 0.0).tone_hz, 10_000.0, "hall tone 0");
    close(h(&|p| p.tone = 1.0).tone_hz, 2_500.0, "hall tone 1");
    close(h(&|p| p.diffuse = 0.0).diffuse_g, 0.5, "hall diffuse 0");
    close(h(&|p| p.diffuse = 1.0).diffuse_g, 0.75, "hall diffuse 1");
    close(h(&|p| p.damping = 0.0).damp_hz, 11_000.0, "hall damp 0");
    close(h(&|p| p.damping = 1.0).damp_hz, 1_500.0, "hall damp 1");
    close(h(&|p| p.predelay = 1.0).predelay_ms, 125.0, "hall pre 1");
    assert_eq!(h(&|p| p.predelay = 0.0).predelay_ms, 0.0);
    close(
        h(&|p| (p.smear, p.size) = (1.0, 1.0)).smear_ms,
        2.0,
        "hall smear 1",
    );
    close(h(&|p| p.rate = 0.0).rate_hz, 0.1, "hall rate 0");
    close(h(&|p| p.rate = 1.0).rate_hz, 1.5, "hall rate 1");
    let d = h(&|_| {});
    close(d.length_ms, 341.6, "hall size default");
    close(d.rt60_s, 2.4929, "hall decay default");
    close(d.tone_hz, 6_597.5, "hall tone default");
    close(d.diffuse_g, 0.6, "hall diffuse default");
    close(d.damp_hz, 6_050.7, "hall damp default");
    close(d.predelay_ms, 18.75, "hall pre default");
    close(d.smear_ms, 0.7, "hall smear default");
    close(d.rate_hz, 0.2954, "hall rate default");
    // HALL's DAMP is QUAD's law.
    for p in [0.0, 0.3, 0.7, 1.0] {
        close(h(&|q| q.damping = p).damp_hz, damp_hz(p), "hall damp law");
    }
}

/// Below SIZE 0.15, HALL plays DIFFUSE pulled up and TONE and PRE-DLY
/// down by w = 1 − SIZE/0.15; the stored knobs never change, and no other
/// TYPE has the zone.
#[test]
fn plate_zone_pulls_diffuse_tone_predelay() {
    let p = |size: f32| ReverbParams {
        kind: ReverbType::Hall,
        size,
        diffuse: 0.4,
        tone: 0.6,
        predelay: 0.5,
        ..ReverbParams::default()
    };
    let eff = |size: f32| {
        let q = plate_pull(&p(size));
        (q.diffuse, q.tone, q.predelay)
    };
    let near = |a: (f32, f32, f32), b: (f32, f32, f32)| {
        assert!(
            (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6 && (a.2 - b.2).abs() < 1e-6,
            "{a:?} != {b:?}"
        );
    };
    near(eff(0.0), (1.0, 0.0, 0.0));
    near(eff(0.075), (0.7, 0.3, 0.25));
    for size in [0.15, 0.5, 1.0] {
        near(eff(size), (0.4, 0.6, 0.5));
    }
    assert!(in_plate_zone(&p(0.149)) && !in_plate_zone(&p(0.15)));
    // The engine plays the pulled knobs; the stored ones stay.
    let k = knobs_120(&p(0.0));
    assert_eq!(
        (k.diffuse_g, k.tone_hz, k.predelay_ms),
        (0.75, 10_000.0, 0.0)
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
#[test]
fn smear_caps_hold_at_every_size() {
    for (t, cap) in [
        (ReverbType::Room, 0.0072),
        (ReverbType::Hall, 0.0036),
        (ReverbType::Quad, 0.0018),
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
