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
    KnobMemory, REVERB_SPECS, RevSync, ReverbParams, ReverbType, knobs,
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
            let k = knobs(&p);
            for v in [k.length_ms, k.rt60_s, k.damp_hz, k.smear_ms, k.rate_hz] {
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
        knobs(&p)
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
