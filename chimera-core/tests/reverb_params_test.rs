//! The reverb's TYPE, its semantic slots and their maps (reverb TYPEs
//! spec § Parameters, § Tempo sync).

mod common;

use chimera_core::addr::{BlockRead, BlockRef, Blocks};
use chimera_core::block::{Block, DiskCode, ParamId, find_spec};
use chimera_core::dsp::Stereo;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::dsp::rev::Reverb;
use chimera_core::dsp::rev::sync::{FoldState, Tempo, fold};
use chimera_core::dsp::reverb::{
    EngineKnobs, KnobMemory, REVERB_SPECS, RevSync, ReverbParams, ReverbType, knobs,
};
use chimera_core::storage::decode_block;
use common::SR;
use common::rev::{Noise, blocks};

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

fn knobs_of(p: &ReverbParams) -> EngineKnobs {
    knobs(p, Tempo::DEFAULT, &mut FoldState(0))
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

/// Review Focus 2: an unknown TYPE code and NaN in every new slot load as
/// QUAD, and a second of noise through it stays finite.
#[test]
fn unknown_type_code_and_nan_slots_load_as_safe_quad() {
    let mut entries = vec![(6u8, 9u32.to_le_bytes())];
    entries.extend((7..=12).map(|id| real(id, f32::NAN)));
    entries.push((13, f32::NAN.to_le_bytes()));
    entries.extend([real(1, 0.5), real(3, 0.5), real(4, 1.0)]);
    let mut fx = FxParams::default();
    decode_block(&record(&entries), &[], &[], Some(&mut Fx(&mut fx))).unwrap();
    let p = fx.reverb;
    assert_eq!(p.kind, ReverbType::Quad);
    assert_eq!(p.sync, RevSync::Off);
    for id in 7..=12 {
        assert!(p.get(ParamId(id)).is_finite(), "slot {id}");
    }
    // NaN that got past the codec (a CC, a bug) still renders finite.
    let mut raw = p;
    for id in [1, 2, 3, 7, 8, 9, 10, 11, 12] {
        raw.write(ParamId(id), f32::NAN);
    }
    let k = knobs_of(&raw);
    for (what, v) in [
        ("length", k.length_ms),
        ("rt60", k.rt60_s),
        ("tone", k.tone_hz),
        ("smear", k.smear_ms),
        ("rate", k.rate_hz),
    ] {
        assert!(v.is_finite(), "{what}");
    }
    for params in [p, raw] {
        let mut rv = Reverb::new();
        let mut n = Noise(0x5EED);
        for _ in 0..blocks(1.0) {
            let send = core::array::from_fn(|_| 0.5 * n.next());
            let mut out = Stereo::SILENT;
            rv.process(&send, &params, 1.0, Tempo::DEFAULT, SR, &mut out);
            assert!(out.l.iter().chain(&out.r).all(|s| s.is_finite()));
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

fn on(t: ReverbType) -> ReverbParams {
    ReverbParams::default().with_type(t, &KnobMemory::new())
}

#[test]
fn knobs_hit_the_spec_endpoints() {
    use ReverbType::*;
    let at = |t, f: &dyn Fn(&mut ReverbParams)| {
        let mut p = on(t);
        f(&mut p);
        knobs_of(&p)
    };
    for (s, ms) in [(0.0, 100.0), (0.5, 141.42), (1.0, 200.0)] {
        close(at(Room, &|p| p.size = s).length_ms, ms, "Room size");
    }
    close(at(Room, &|p| p.time = 0.5).rt60_s, 0.5477, "Room decay");
    close(at(Hall, &|p| p.size = 0.65).length_ms, 341.595, "Hall size");
    close(
        at(Hall, &|p| p.time = 0.45).rt60_s,
        2.4929,
        "Hall decay .45",
    );
    close(at(Hall, &|p| p.time = 1.0).rt60_s, 10.0, "Hall decay 1");
    close(at(Hall, &|p| p.damping = 0.3).damp_hz, 6_051.0, "Hall damp");
    close(
        at(Hall, &|p| p.predelay = 1.0).predelay_ms,
        125.0,
        "Hall pre",
    );
    close(
        at(Gate, &|p| p.size = 0.55).length_ms,
        177.38,
        "Gate length",
    );
    close(at(Gate, &|p| p.time = 0.3).rt60_s, 0.2478, "Gate tail");
    assert_eq!(at(Gate, &|p| p.time = 0.0).tail, 0.0, "TAIL 0 is none");
    close(at(Gate, &|p| p.time = 0.025).tail, 0.5, "TAIL fades in");
    close(at(Gate, &|p| p.shape = 0.2).shape_db, -3.6, "Gate shape");
    close(at(Gate, &|p| p.diffuse = 0.7).taps, 10.2, "Gate taps");
    close(at(Gate, &|p| p.diffuse = 0.7).diffuse_g, 0.64, "Gate g");
    close(
        at(Reverse, &|p| p.size = 0.5).length_ms,
        346.4,
        "Reverse len",
    );
    close(
        at(Reverse, &|p| p.shape = 0.67).shape_db,
        24.06,
        "Reverse curve",
    );
    close(at(Reverse, &|p| p.time = 1.0).regen, 0.708, "Reverse regen");
    close(
        at(Bloom, &|p| p.size = 0.7).length_ms,
        513.985,
        "Bloom swell",
    );
    close(at(Bloom, &|p| p.time = 0.6).rt60_s, 5.8603, "Bloom decay");
    close(at(Bloom, &|p| p.tone = 0.35).tone_hz, 5_147.9, "Bloom tone");
    close(at(Bloom, &|p| p.rate = 0.4).rate_hz, 0.1256, "Bloom rate");
    close(at(Room, &|p| p.tone = 0.0).tone_hz, 10_000.0, "tone 0");
    close(at(Room, &|p| p.tone = 1.0).tone_hz, 2_500.0, "tone 1");
    close(
        at(Room, &|p| p.diffuse = 0.4).diffuse_g,
        0.6,
        "Room diffuse",
    );
    close(at(Room, &|p| p.rate = 0.5).rate_hz, 0.3162, "Room rate");
    // QUAD keeps its own laws (ADR 0028).
    close(at(Quad, &|p| p.rate = 0.5).rate_hz, 0.2236, "Quad rate");
    close(
        at(Quad, &|p| p.size = 0.0).length_ms,
        338.583,
        "Quad size 0",
    );
}

#[test]
fn plate_zone_pulls_diffuse_tone_predelay() {
    let k = |size: f32| {
        let mut p = on(ReverbType::Hall);
        (p.size, p.diffuse, p.tone, p.predelay) = (size, 0.4, 0.6, 0.5);
        knobs_of(&p)
    };
    let want = |d: f32, t: f32, pre: f32| {
        let mut p = on(ReverbType::Hall);
        (p.size, p.diffuse, p.tone, p.predelay) = (0.5, d, t, pre);
        knobs_of(&p)
    };
    for (size, (d, t, pre)) in [
        (0.0, (1.0, 0.0, 0.0)),
        (0.15, (0.4, 0.6, 0.5)),
        (0.5, (0.4, 0.6, 0.5)),
        (0.075, (0.7, 0.3, 0.25)),
    ] {
        let (g, w) = (k(size), want(d, t, pre));
        close(g.diffuse_g, w.diffuse_g, "diffuse");
        close(g.tone_hz, w.tone_hz, "tone");
        assert!(
            (g.predelay_ms - w.predelay_ms).abs() < 1e-3,
            "pre at {size}"
        );
    }
    // The stored knobs never change.
    let mut p = on(ReverbType::Hall);
    p.size = 0.0;
    let _ = knobs_of(&p);
    assert_eq!((p.diffuse, p.tone), (0.4, 0.3));
}

#[test]
fn smear_caps_hold_at_every_size() {
    use ReverbType::*;
    for (t, cap) in [
        (Room, 0.0072),
        (Hall, 0.0036),
        (Bloom, 0.0024),
        (Quad, 0.0018),
    ] {
        for i in 0..=128 {
            let mut p = on(t);
            (p.size, p.smear) = (i as f32 / 128.0, 1.0);
            let k = knobs_of(&p);
            assert!(
                k.smear_ms <= cap * k.length_ms * (1.0 + 1e-5),
                "{t:?} at size {i}/128: D {} vs L {}",
                k.smear_ms,
                k.length_ms
            );
        }
    }
    // Under the cap, the knob's own law: HALL ±2 ms at 1, 0.7 at .35.
    let mut p = on(Hall);
    (p.size, p.smear) = (1.0, 0.35);
    close(knobs_of(&p).smear_ms, 0.7, "Hall smear .35");
    // Outside every loop, no cap: GATE ±1 ms.
    let mut p = on(Gate);
    p.smear = 1.0;
    close(knobs_of(&p).smear_ms, 1.0, "Gate smear 1");
}

#[test]
fn fold_halves_doubles_and_holds() {
    let mut st = FoldState(0);
    assert_eq!(fold(667.0, 40.0, 600.0, &mut st), 333.5); // GATE 1/4 at 90 BPM plays 1/8
    assert_eq!(fold(20.0, 40.0, 600.0, &mut st), 40.0); // doubled once
    assert_eq!(st, FoldState(-1));
    let mut st = FoldState(0);
    for ms in [600.0, 606.0, 594.0, 611.0] {
        let _ = fold(ms, 40.0, 600.0, &mut st);
    }
    assert_eq!(st, FoldState(0)); // within 2 %: no flip
    assert_eq!(fold(613.0, 40.0, 600.0, &mut st), 306.5); // past 2 %: one halving
    // Back inside by 2 % before it unfolds.
    let _ = fold(590.0, 40.0, 600.0, &mut st);
    assert_eq!(st, FoldState(1));
    assert_eq!(fold(580.0, 40.0, 600.0, &mut st), 580.0);
    assert_eq!(st, FoldState(0));
    assert_eq!(fold(f32::NAN, 40.0, 600.0, &mut st), 40.0);
}

#[test]
fn synced_lengths_walk_their_lists() {
    // GATE, SYNC TIME: LENGTH's p picks a division; 1/8 at 120 is 250 ms.
    let mut p = on(ReverbType::Gate);
    p.sync = RevSync::Time;
    p.size = 7.0 / 10.0; // GATE_LENGTH[7] = 1/8
    close(knobs_of(&p).length_ms, 250.0, "Gate 1/8");
    // 1/4 at 90 BPM (667 ms) folds to 1/8.
    let slow = Tempo(Tempo::RATE * 60.0 / 90.0);
    p.size = 1.0;
    let mut st = FoldState(0);
    close(knobs(&p, slow, &mut st).length_ms, 333.3, "Gate 1/4 at 90");
    assert_eq!(st, FoldState(1));
    // HALL, SYNC ALL: DECAY in beats, clamped to its seconds.
    let mut h = on(ReverbType::Hall);
    (h.sync, h.time) = (RevSync::All, 1.0); // 32 beats = 16 s
    close(knobs_of(&h).rt60_s, 10.0, "Hall 32 beats");
    h.time = 3.0 / 9.0; // 4 beats = 2 s
    close(knobs_of(&h).rt60_s, 2.0, "Hall 4 beats");
    // ROOM reads a stored ALL as TIME: DECAY stays in seconds.
    let mut r = on(ReverbType::Room);
    (r.sync, r.time) = (RevSync::All, 0.5);
    close(knobs_of(&r).rt60_s, 0.5477, "Room decay under ALL");
}

#[test]
fn with_sync_keeps_the_length_close() {
    // GATE at 250 ms, OFF → TIME: the nearest division, 1/8.
    let mut p = on(ReverbType::Gate);
    p.size = (250.0f32 / 40.0).ln() / 15.0f32.ln();
    let t = p.with_sync(RevSync::Time, Tempo::DEFAULT);
    assert_eq!(t.sync, RevSync::Time);
    close(knobs_of(&t).length_ms, 250.0, "to TIME");
    // TIME → OFF: the p whose ms is the division that played.
    let back = t.with_sync(RevSync::Off, Tempo::DEFAULT);
    close(knobs_of(&back).length_ms, 250.0, "to OFF");
    // HALL OFF → ALL: DECAY 2.5 s lands on 4 or 6 beats (2 or 3 s).
    let h = on(ReverbType::Hall).with_sync(RevSync::All, Tempo::DEFAULT);
    let s = knobs_of(&h).rt60_s;
    assert!(s == 2.0 || s == 3.0, "{s}");
    // QUAD takes no sync.
    let q = ReverbParams::default().with_sync(RevSync::Time, Tempo::DEFAULT);
    assert_eq!(q, ReverbParams::default());
}
