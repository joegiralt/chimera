//! The kit on the Part, saved with the project (ADR 0265): the `Kit`
//! record round-trips, a 0.3 project loads with the default kit, and an
//! unknown or out-of-range byte loads safely. The pads' sounds (ADR 0269)
//! go in a `KitSound` record a pad; a slice-1 file has none, and its DRUM
//! Part's pads take the Sound's FILTER and AMP.

mod common;

use chimera_core::dsp::Unit;
use chimera_core::dsp::drum::laws::spec;
use chimera_core::dsp::drum::sound::{
    Amount, AmpMode, EnvDecay, EnvPos, LfoMode, PAD_SOUND_BYTES, PadEnv, PadFilter, PadRoute,
    PadSound, RouteDest, RouteId, RouteSource,
};
use chimera_core::dsp::drum::types::{
    ChokeGroup, Hz, Kit, Knob, KnobSet, KnobValue, Machine, PadId, Pan, Tonal,
};
use chimera_core::dsp::filter::{FilterKind, K2040Voice};
use chimera_core::dsp::lfo::LfoShape;
use chimera_core::dsp::lpg;
use chimera_core::dsp::modulator::EnvSpeed;
use chimera_core::dsp::tape_engine::params::Retrig;
use chimera_core::factory::FACTORY_LEN;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::{PartFrom, PartId, PartSource, Project, SlotId};
use chimera_core::storage::{FileError, RecordTag};
use chimera_midi::value::U7;
use common::codec_util::{decode_project_lossy, fix_crc, record_offsets};
use common::project::{decode, encode, load};

const KIT_TAG: u16 = 0x0024;
const KIT_SOUND_TAG: u16 = 0x0025;
/// The pad's index, then `PadSound::to_bytes`.
const KIT_SOUND_BYTES: usize = 1 + PAD_SOUND_BYTES;
const PAD_BYTES: usize = 13;
/// The pads, then TONAL (ADR 0270).
const KIT_BYTES: usize = 12 * PAD_BYTES + 1;

fn fixture(name: &str) -> Vec<u8> {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v1/");
    std::fs::read(format!("{p}{name}")).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

fn pad(n: u8) -> PadId {
    PadId::new(n - 1).unwrap()
}

/// Each Kit record: its offset and payload.
fn kit_records(f: &[u8]) -> Vec<(usize, Vec<u8>)> {
    records(f, KIT_TAG)
}

/// Each `tag` record: its offset and payload.
fn records(f: &[u8], tag: u16) -> Vec<(usize, Vec<u8>)> {
    let offs = record_offsets(f);
    offs[..offs.len() - 1]
        .iter()
        .filter(|&&o| u16::from_le_bytes([f[o], f[o + 1]]) == tag)
        .map(|&o| {
            let len = usize::from(u16::from_le_bytes([f[o + 2], f[o + 3]]));
            (o, f[o + 4..o + 4 + len].to_vec())
        })
        .collect()
}

/// Parts 2 and 5 edited in every field of a few pads; Part 2 TONAL on pad 9.
/// Part 2's pad 1 and Part 5's pad 7 with sounds of their own.
fn edited() -> Box<Project> {
    let (mut p, _) = Project::boxed();
    let k = p.kit_mut(PartId::ALL[1]);
    k.set_tonal(Tonal::Pad(pad(9)));
    let a = k.pad_mut(pad(1));
    a.set_machine(Machine::MetalJingle);
    a.knobs_mut().set(Knob::C, KnobValue::new(u7(5)));
    a.knobs_mut().set(Knob::H, KnobValue::new(u7(127)));
    a.set_level(u7(33));
    a.set_pan(Pan::clamped(-64));
    a.set_choke(ChokeGroup::D);
    a.set_retrig(Retrig::Stack);
    let b = k.pad_mut(pad(12));
    b.set_pan(Pan::clamped(63));
    b.set_level(u7(0));
    let c = p.kit_mut(PartId::ALL[4]).pad_mut(pad(7));
    c.set_machine(Machine::PlainNoise);
    c.knobs_mut().set(Knob::A, KnobValue::ZERO);
    c.set_choke(ChokeGroup::Off);
    *c.sound_mut() = sound_b();
    *p.kit_mut(PartId::ALL[1]).pad_mut(pad(1)).sound_mut() = sound_a();
    p
}

/// Every part of a pad's sound off its default.
fn sound_a() -> PadSound {
    let mut s = PadSound::DEFAULT;
    let f = s.filter_mut();
    f.set_kind(FilterKind::K2040);
    f.set_voice(K2040Voice::Exp);
    f.set_cutoff(Hz::new(640.0));
    f.set_resonance(Unit::new(0.3));
    f.set_morph(Unit::new(0.7));
    let a = s.amp_mut();
    *a.env_mut() = PadEnv::new(
        EnvPos::new(u7(3)),
        EnvPos::new(u7(9)),
        EnvDecay::Time(EnvPos::new(u7(70))),
        EnvSpeed::Fast,
    );
    a.set_vel(Unit::new(0.5));
    a.set_gate(lpg::Knobs::of([0.1, 0.2, 0.3]));
    a.set_mode(AmpMode::Gated);
    *s.env_mut() = PadEnv::new(
        EnvPos::new(u7(1)),
        EnvPos::ZERO,
        EnvDecay::Infinite,
        EnvSpeed::Slow,
    );
    let l = s.lfo_mut();
    l.set_wave(LfoShape::Square);
    l.set_speed(u7(90));
    l.set_depth(u7(40));
    l.set_phase(u7(64));
    l.set_mode(LfoMode::Trig);
    let r = s.routes_mut();
    r.set(
        RouteId::ALL[0],
        PadRoute::new(RouteSource::Lfo, RouteDest::Volume, Amount::clamped(-127)),
    );
    r.set(
        RouteId::ALL[3],
        PadRoute::new(RouteSource::Lfo3, RouteDest::LfoDepth, Amount::clamped(55)),
    );
    s
}

fn sound_b() -> PadSound {
    let mut s = PadSound::DEFAULT;
    s.filter_mut().set_kind(FilterKind::Sem);
    s.routes_mut().set(
        RouteId::ALL[1],
        PadRoute::new(RouteSource::ModEnv, RouteDest::Cutoff, Amount::clamped(100)),
    );
    s
}

#[test]
fn kit_record_is_frozen_and_not_critical() {
    assert_eq!(RecordTag::Kit.code(), KIT_TAG);
    assert_eq!(RecordTag::from_code(KIT_TAG), Some(RecordTag::Kit));
    let f = encode(&edited());
    let kits = kit_records(&f);
    assert_eq!(kits.len(), 6, "one a Part");
    for (_, payload) in &kits {
        assert_eq!(payload.len(), KIT_BYTES);
    }
    // TONAL: 0 OFF, else the pad's number.
    let tonal: Vec<u8> = kits.iter().map(|(_, k)| k[KIT_BYTES - 1]).collect();
    assert_eq!(tonal, [0, 9, 0, 0, 0, 0]);
    // Part 2's pad 1: machine, A–H, level, pan, choke, retrig.
    assert_eq!(
        kits[1].1[..PAD_BYTES],
        // TONE KICK's defaults under C and H's edits, as METAL JINGLE.
        [8, 40, 80, 5, 40, 20, 10, 0, 127, 33, 0xC0, 4, 0][..],
        "{:?}",
        &kits[1].1[..PAD_BYTES]
    );
}

#[test]
fn an_edited_kit_round_trips_byte_exact() {
    let p = edited();
    let f = encode(&p);
    let (mut q, _) = Project::boxed();
    decode(&f, &mut q).unwrap();
    for id in PartId::ALL {
        assert_eq!(q.kit(id), p.kit(id), "Part {}", id.index());
    }
    assert_eq!(encode(&q), f);
}

/// Review Focus 2: a 0.3 project has no Kit record. Every Part takes the
/// default kit, over whatever kit the target had, and nothing else moves.
#[test]
fn a_project_without_kits_loads_the_default_kit() {
    for name in ["project.prj", "orbit.prj", "sends.prj"] {
        let f = fixture(name);
        assert!(kit_records(&f).is_empty(), "{name} is from before kits");
        let (mut clean, _) = Project::boxed();
        decode(&f, &mut clean).unwrap();
        let mut dirty = edited();
        let lossy = decode_project_lossy(&f, &mut dirty).unwrap();
        for id in PartId::ALL {
            assert_eq!(*dirty.kit(id), Kit::DEFAULT, "{name} Part {}", id.index());
        }
        // Everything else as a load into a fresh project reads it.
        common::project::same(&clean, &dirty);
        assert_eq!(encode(&clean), encode(&dirty), "{name}");
        // The default kit adds nothing a save would change.
        let mut plain = Project::boxed().0;
        assert_eq!(
            decode_project_lossy(&f, &mut plain).unwrap(),
            lossy,
            "{name}"
        );
    }
}

/// `f` with Part `part`'s pad `p` byte `at` set to `v`, CRC fixed.
fn patched(f: &[u8], part: usize, p: PadId, at: usize, v: u8) -> Vec<u8> {
    let mut g = f.to_vec();
    let (o, _) = kit_records(f)[part];
    g[o + 4 + p.index() * PAD_BYTES + at] = v;
    fix_crc(&mut g);
    g
}

#[test]
fn an_unknown_machine_loads_as_the_pads_default() {
    let p = edited();
    let f = encode(&p);
    let g = patched(&f, 1, pad(1), 0, 200);
    let (mut q, _) = Project::boxed();
    assert!(
        decode_project_lossy(&g, &mut q).unwrap(),
        "a save would differ"
    );
    let (got, want, was) = (
        q.kit(PartId::ALL[1]).pad(pad(1)),
        Kit::DEFAULT.pad(pad(1)),
        p.kit(PartId::ALL[1]).pad(pad(1)),
    );
    assert_eq!(got.machine(), want.machine());
    assert_eq!(got.knobs(), want.knobs());
    // The rest of the pad is the file's.
    assert_eq!(
        (got.level(), got.pan(), got.choke(), got.retrig()),
        (was.level(), was.pan(), was.choke(), was.retrig())
    );
    // The other pads and Parts as saved.
    for id in PartId::ALL {
        for n in PadId::ALL {
            if (id.index(), n) != (1, pad(1)) {
                assert_eq!(q.kit(id).pad(n), p.kit(id).pad(n));
            }
        }
    }
}

#[test]
fn out_of_range_bytes_clamp_or_default() {
    let f = encode(&edited());
    let at = |at: usize, v: u8| {
        let (mut q, _) = Project::boxed();
        let lossy = decode_project_lossy(&patched(&f, 3, pad(5), at, v), &mut q).unwrap();
        (*q.kit(PartId::ALL[3]).pad(pad(5)), lossy)
    };
    let d = *Kit::DEFAULT.pad(pad(5));
    let (k, lossy) = at(1 + Knob::B as usize, 200);
    assert_eq!(k.knobs().get(Knob::B), KnobValue::new(U7::MAX));
    assert!(lossy);
    let (k, lossy) = at(9, 255);
    assert_eq!(k.level(), U7::MAX);
    assert!(lossy);
    let (k, lossy) = at(10, 0x80);
    assert_eq!(k.pan(), Pan::clamped(-64));
    assert!(lossy);
    let (k, lossy) = at(11, 9);
    assert_eq!(k.choke(), d.choke());
    assert!(lossy);
    let (k, lossy) = at(12, 7);
    assert_eq!(k.retrig(), d.retrig());
    assert!(lossy);
    // In range: as written, not lossy.
    let (k, lossy) = at(10, (-3i8) as u8);
    assert_eq!(k.pan(), Pan::clamped(-3));
    assert!(!lossy);
}

#[test]
fn a_kit_record_of_the_wrong_shape_or_twice_is_refused() {
    let f = encode(&edited());
    let (o, payload) = kit_records(&f)[2].clone();
    let end = o + 4 + payload.len();
    // Two bytes short (one short is a kit from before TONAL).
    let mut g = f.clone();
    g.drain(end - 2..end);
    g[o + 2..o + 4].copy_from_slice(&((payload.len() - 2) as u16).to_le_bytes());
    fix_crc(&mut g);
    let (mut q, _) = Project::boxed();
    assert_eq!(decode(&g, &mut q), Err(FileError::Bounds));
    // Twice in one Part.
    let mut g = f.clone();
    let rec = f[o..end].to_vec();
    g.splice(end..end, rec);
    fix_crc(&mut g);
    assert_eq!(decode(&g, &mut q), Err(FileError::Corrupt));
}

#[test]
fn default_kit_is_the_rytm_layout() {
    use Machine::*;
    let k = Kit::DEFAULT;
    let machines = [
        ToneKick,
        ToneSnare,
        PlainImpulse,
        ToneClap,
        ToneTom,
        ToneTom,
        ToneTom,
        MetalHat,
        MetalHat,
        ToneCymbal,
        ToneCowbell,
        BodyShaker,
    ];
    for (n, m) in PadId::ALL.into_iter().zip(machines) {
        assert_eq!(k.pad(n).machine(), m, "pad {}", n.number());
        assert_eq!(k.pad(n).retrig(), Retrig::Cut, "pad {}", n.number());
        assert_eq!(k.pad(n).pan(), Pan::CENTRE);
    }
    let choke = |n: u8| k.pad(pad(n)).choke();
    for (group, pair) in [
        (ChokeGroup::A, [3, 4]),
        (ChokeGroup::B, [6, 7]),
        (ChokeGroup::C, [8, 9]),
        (ChokeGroup::D, [10, 11]),
    ] {
        assert_eq!(pair.map(choke), [group; 2]);
    }
    for n in [1, 2, 5, 12] {
        assert_eq!(choke(n), ChokeGroup::Off, "pad {n}");
    }
    // Toms low, mid, high by PITCH; the closed hat shorter than the open.
    assert_eq!(spec(ToneTom).knob(Knob::A).unwrap().label, "PITCH");
    let pitch = |n: u8| k.pad(pad(n)).knobs().get(Knob::A);
    assert!(pitch(5) < pitch(6) && pitch(6) < pitch(7));
    assert_eq!(spec(MetalHat).knob(Knob::B).unwrap().label, "DECAY");
    let decay = |n: u8| k.pad(pad(n)).knobs().get(Knob::B);
    assert!(decay(8) < decay(9));
    // Every other knob the machine's own default.
    for n in PadId::ALL {
        let (m, got) = (k.pad(n).machine(), k.pad(n).knobs());
        let mut want = KnobSet::defaults(m);
        if m == ToneTom {
            want.set(Knob::A, got.get(Knob::A));
        }
        if m == MetalHat {
            want.set(Knob::B, got.get(Knob::B));
        }
        assert_eq!(*got, want, "pad {}", n.number());
    }
}

/// DRUM INIT is a browser INIT row and one of NEW's INIT Sounds.
#[test]
fn drum_init_is_playable() {
    use chimera_core::ui::browser::{INIT_TYPES, Soon};
    assert!(EngineType::PLAYABLE.contains(&EngineType::Drum));
    assert!(INIT_TYPES.contains(&EngineType::Drum));
    assert!(Soon::ALL.is_empty(), "DRUM is no longer coming soon");
    let (p, _) = Project::boxed();
    let i = EngineType::PLAYABLE
        .iter()
        .position(|&e| e == EngineType::Drum)
        .unwrap();
    let slot = p.pool().get(SlotId::ALL[FACTORY_LEN + i]).unwrap();
    assert!(slot.to_sound().bits_eq(&Sound::init(EngineType::Drum)));
}

/// The kit is the Part's, not the Sound's: a Sound load, DRUM INIT too,
/// leaves it.
#[test]
fn a_sound_load_keeps_the_kit() {
    let mut p = edited();
    let t = Project::boxed().1;
    let part = PartId::ALL[1];
    let kit = *p.kit(part);
    let from = PartFrom::Init(EngineType::Drum);
    load(&mut p, t, PartSource { part, from }).unwrap();
    assert_eq!(p.part(part).sound.engine(), EngineType::Drum);
    assert_eq!(*p.kit(part), kit);
}

/// ADR 0270: a kit saved before TONAL (pads only, 156 B) loads TONAL OFF,
/// its pads as written, nothing lost.
#[test]
fn a_kit_from_before_tonal_loads_tonal_off() {
    let p = edited();
    let f = encode(&p);
    let mut g = f.clone();
    // Each Kit record one byte shorter, last first so offsets hold.
    for (o, payload) in kit_records(&f).into_iter().rev() {
        let end = o + 4 + payload.len();
        g.remove(end - 1);
        g[o + 2..o + 4].copy_from_slice(&((KIT_BYTES - 1) as u16).to_le_bytes());
    }
    fix_crc(&mut g);
    let (mut q, _) = Project::boxed();
    let mut plain = Project::boxed().0;
    assert_eq!(
        decode_project_lossy(&g, &mut q).unwrap(),
        decode_project_lossy(&f, &mut plain).unwrap(),
        "no more lossy than the file with TONAL"
    );
    for id in PartId::ALL {
        assert_eq!(q.kit(id).tonal(), Tonal::Off, "Part {}", id.index());
        for n in PadId::ALL {
            assert_eq!(q.kit(id).pad(n), p.kit(id).pad(n));
        }
    }
}

#[test]
fn an_unknown_tonal_loads_off() {
    let f = encode(&edited());
    let (o, _) = kit_records(&f)[1];
    let mut g = f.clone();
    g[o + 4 + KIT_BYTES - 1] = 13;
    fix_crc(&mut g);
    let (mut q, _) = Project::boxed();
    assert!(
        decode_project_lossy(&g, &mut q).unwrap(),
        "a save would differ"
    );
    assert_eq!(q.kit(PartId::ALL[1]).tonal(), Tonal::Off);
}

/// The slice-1 project the DRUM slice 2 baseline golden loads
/// (`golden_test`'s `drum_slice1_kit`): written once from slice-1 code,
/// never regenerated after (`--ignored`).
pub const SLICE1_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/drum_slice1.prj"
);

#[test]
#[ignore = "writes tests/fixtures/drum_slice1.prj from the base"]
fn write_slice1_fixture() {
    use chimera_core::dsp::filter::FilterKind;
    use chimera_core::dsp::filter::FilterMode;
    use chimera_core::project::{PartFrom, PartId, PartSource, Project};
    let (mut p, t) = Project::boxed();
    let (a, b) = (PartId::ALL[0], PartId::ALL[1]);
    for (part, e) in [(a, EngineType::Drum), (b, EngineType::Va)] {
        let from = PartFrom::Init(e);
        load(&mut p, t, PartSource { part, from }).unwrap();
    }
    {
        let s = &mut p.edit_part(a).sound.params;
        s.filter.set_kind(FilterKind::Ladder);
        assert!(s.filter.set_mode(FilterMode::Lp24));
        s.filter.cutoff = 420.0;
        s.filter.resonance = 0.62;
        s.out.vca_vel = 0.4;
        s.out.gate = [0.5, 0.3, 0.2];
    }
    p.kit_mut(a).pad_mut(pad(9)).set_pan(Pan::clamped(20));
    std::fs::write(SLICE1_FIXTURE, encode(&p)).unwrap();
}

/// Review Focus 2, ADR 0269: a slice-1 project's DRUM Part had one FILTER
/// and AMP; its pads take them, so its kit sounds as it did. Its VA Part's
/// kit stays the default's. Nothing lost.
#[test]
fn slice1_file_takes_the_sounds_filter() {
    let f = std::fs::read(SLICE1_FIXTURE).unwrap();
    assert!(records(&f, KIT_SOUND_TAG).is_empty(), "slice 1 has none");
    let (mut p, _) = Project::boxed();
    assert!(!decode_project_lossy(&f, &mut p).unwrap());
    let (a, b) = (PartId::ALL[0], PartId::ALL[1]);
    assert_eq!(p.part(a).sound.engine(), EngineType::Drum);
    let s = PadSound::from_sound(&p.part(a).sound.params);
    let (flt, amp) = (s.filter(), s.amp());
    assert_eq!(
        (flt.kind(), flt.cutoff().get(), flt.resonance().get()),
        (FilterKind::Ladder, 420.0, 0.62)
    );
    assert_eq!(amp.vel().get(), 0.4);
    let g = amp.gate();
    assert_eq!(
        [g.lpg.get(), g.decay.get(), g.strike.get()],
        [0.5, 0.3, 0.2]
    );
    assert_eq!(*amp.env(), PadEnv::OPEN, "slice 1's AMP");
    for q in PadId::ALL {
        assert_eq!(*p.kit(a).pad(q).sound(), s, "pad {}", q.number());
        assert_eq!(*p.kit(b).pad(q).sound(), PadSound::DEFAULT);
    }
    // Pad 9's pan as written: the kit is the file's.
    assert_eq!(p.kit(a).pad(pad(9)).pan(), Pan::clamped(20));
}

/// `f` with Part `part`'s pad `p` sound byte `at` set to `v`, CRC fixed.
fn sound_patched(f: &[u8], part: usize, p: PadId, at: usize, v: &[u8]) -> Vec<u8> {
    let mut g = f.to_vec();
    let (o, _) = records(f, KIT_SOUND_TAG)[part * 12 + p.index()];
    g[o + 5 + at..o + 5 + at + v.len()].copy_from_slice(v);
    fix_crc(&mut g);
    g
}

#[test]
fn kit_sound_record_is_frozen_and_not_critical() {
    assert_eq!(RecordTag::KitSound.code(), KIT_SOUND_TAG);
    assert_eq!(
        RecordTag::from_code(KIT_SOUND_TAG),
        Some(RecordTag::KitSound)
    );
    assert_eq!(KIT_SOUND_TAG & 0x8000, 0, "not critical");
    let p = edited();
    let f = encode(&p);
    let rs = records(&f, KIT_SOUND_TAG);
    assert_eq!(rs.len(), 12 * 6, "one a pad, every Part");
    let kits = kit_records(&f);
    for (i, (o, b)) in rs.iter().enumerate() {
        let (part, q) = (PartId::ALL[i / 12], PadId::ALL[i % 12]);
        assert_eq!(b.len(), KIT_SOUND_BYTES);
        assert_eq!(b[0], q.index() as u8);
        assert_eq!(b[1..], p.kit(part).pad(q).sound().to_bytes());
        assert!(*o > kits[i / 12].0, "after the Part's Kit");
    }
}

#[test]
fn kit_sound_round_trips() {
    let p = edited();
    assert_ne!(
        *p.kit(PartId::ALL[1]).pad(pad(1)).sound(),
        PadSound::DEFAULT
    );
    let f = encode(&p);
    let (mut q, _) = Project::boxed();
    assert!(!decode_project_lossy(&f, &mut q).unwrap());
    for id in PartId::ALL {
        for n in PadId::ALL {
            assert_eq!(q.kit(id).pad(n).sound(), p.kit(id).pad(n).sound());
        }
    }
    assert_eq!(encode(&q), f);
}

/// A DRUM Part with pad sounds of its own: they win over the slice-1 rule,
/// and a pad without its record takes the Sound's FILTER and AMP.
#[test]
fn kit_sounds_beat_the_slice1_rule() {
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[0];
    let from = PartFrom::Init(EngineType::Drum);
    load(&mut p, t, PartSource { part: a, from }).unwrap();
    p.edit_part(a).sound.params.filter.cutoff = 900.0;
    *p.kit_mut(a).pad_mut(pad(3)).sound_mut() = sound_a();
    let f = encode(&p);
    let (mut q, _) = Project::boxed();
    assert!(!decode_project_lossy(&f, &mut q).unwrap());
    assert_eq!(*q.kit(a).pad(pad(3)).sound(), sound_a());
    assert_eq!(*q.kit(a).pad(pad(4)).sound(), PadSound::DEFAULT);
    // Pad 4's record gone: it takes the Sound's; pad 3 keeps its own.
    let (o, b) = records(&f, KIT_SOUND_TAG)[3].clone();
    let mut g = f.clone();
    g.drain(o..o + 4 + b.len());
    fix_crc(&mut g);
    assert!(!decode_project_lossy(&g, &mut q).unwrap());
    let rule = PadSound::from_sound(&q.part(a).sound.params);
    assert_eq!(rule.filter().cutoff().get(), 900.0);
    assert_eq!(*q.kit(a).pad(pad(4)).sound(), rule);
    assert_eq!(*q.kit(a).pad(pad(3)).sound(), sound_a());
}

#[test]
fn unknown_route_code_defaults() {
    let p = edited();
    let f = encode(&p);
    // Routes start at 45: filter 15, AMP 21, mod env 4, LFO 5.
    const ROUTES: usize = 45;
    let (mut q, _) = Project::boxed();
    // An OFF route with an unknown source: still OFF.
    let g = sound_patched(&f, 0, pad(2), ROUTES, &[99]);
    assert!(decode_project_lossy(&g, &mut q).unwrap(), "lossy");
    let r = q
        .kit(PartId::ALL[0])
        .pad(pad(2))
        .sound()
        .routes()
        .get(RouteId::ALL[0]);
    assert_eq!(r, PadRoute::OFF);
    // An edited route's unknown dest: the default's dest, the rest as saved.
    let g = sound_patched(&f, 1, pad(1), ROUTES + 1, &[0]);
    assert!(decode_project_lossy(&g, &mut q).unwrap(), "lossy");
    let r = q
        .kit(PartId::ALL[1])
        .pad(pad(1))
        .sound()
        .routes()
        .get(RouteId::ALL[0]);
    assert_eq!(
        r,
        PadRoute::new(RouteSource::Lfo, RouteDest::Cutoff, Amount::clamped(-127))
    );
    // A NaN CUTOFF: the default's, the rest of the filter as saved.
    let g = sound_patched(&f, 1, pad(1), 3, &f32::NAN.to_le_bytes());
    assert!(decode_project_lossy(&g, &mut q).unwrap(), "lossy");
    let got = *q.kit(PartId::ALL[1]).pad(pad(1)).sound().filter();
    let mut want = *sound_a().filter();
    want.set_cutoff(PadSound::DEFAULT.filter().cutoff());
    assert_eq!(got, want);
}

#[test]
fn wrong_length_is_bounds() {
    let f = encode(&edited());
    let (o, b) = records(&f, KIT_SOUND_TAG)[5].clone();
    let end = o + 4 + b.len();
    let (mut q, _) = Project::boxed();
    // One byte short.
    let mut g = f.clone();
    g.remove(end - 1);
    g[o + 2..o + 4].copy_from_slice(&((b.len() - 1) as u16).to_le_bytes());
    fix_crc(&mut g);
    assert_eq!(decode(&g, &mut q), Err(FileError::Bounds));
    // A pad past 12.
    let mut g = f.clone();
    g[o + 4] = 12;
    fix_crc(&mut g);
    assert_eq!(decode(&g, &mut q), Err(FileError::Bounds));
}

#[test]
fn two_kit_sounds_are_corrupt() {
    let f = encode(&edited());
    let (o, b) = records(&f, KIT_SOUND_TAG)[14].clone();
    let end = o + 4 + b.len();
    let mut g = f.clone();
    g.splice(end..end, f[o..end].to_vec());
    fix_crc(&mut g);
    let (mut q, _) = Project::boxed();
    assert_eq!(decode(&g, &mut q), Err(FileError::Corrupt));
    // Another pad's index on the next record: that pad twice.
    let mut g = f.clone();
    g[end + 4] = b[0];
    fix_crc(&mut g);
    assert_eq!(decode(&g, &mut q), Err(FileError::Corrupt));
}

/// NEW, and a file load, reset the pad sounds with the kit.
#[test]
fn new_resets_pad_sounds() {
    use chimera_core::project::{LoadLink, ProjectSource, ReplaceGuard, load_project};
    use chimera_core::storage::Card;
    use chimera_hal::testkit::MemStore;
    let (mut p, t) = (edited(), Project::boxed().1);
    let go = ReplaceGuard::check(&p, t, ProjectSource::New)
        .unwrap_or_else(|n| n.into_pending().anyway(&p));
    let (mut card, mut s) = (Card::new(), MemStore::new(1));
    let _ = load_project(&mut card, &mut s, &mut p, go, &LoadLink::new());
    for id in PartId::ALL {
        for n in PadId::ALL {
            assert_eq!(*p.kit(id).pad(n).sound(), PadSound::DEFAULT);
        }
    }
    // A file without pad sounds, into an edited project.
    let (mut p, f) = (edited(), encode(&Project::boxed().0));
    let mut g = f.clone();
    for (o, b) in records(&f, KIT_SOUND_TAG).into_iter().rev() {
        g.drain(o..o + 4 + b.len());
    }
    fix_crc(&mut g);
    assert!(!decode_project_lossy(&g, &mut p).unwrap());
    for id in PartId::ALL {
        for n in PadId::ALL {
            assert_eq!(*p.kit(id).pad(n).sound(), PadSound::DEFAULT);
        }
    }
}

/// Review Focus 4: the pad sounds are the Part's, not the Sound's. A VA
/// Sound plays its own filter; DRUM again finds pad 3 as it was.
#[test]
fn engine_switch_keeps_pad_sounds() {
    use chimera_core::MidiChannel;
    use chimera_core::hw::{CPU_HZ_REV_V, MAX_VOICES, SampleBudget};
    use common::InstRig;
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[0];
    let init = |e| PartSource {
        part: a,
        from: PartFrom::Init(e),
    };
    load(&mut p, t, init(EngineType::Drum)).unwrap();
    let f = p.kit_mut(a).pad_mut(pad(3)).sound_mut().filter_mut();
    f.set_kind(FilterKind::Ladder);
    f.set_cutoff(Hz::new(333.0));
    let want = *p.kit(a).pad(pad(3)).sound();
    load(&mut p, t, init(EngineType::Va)).unwrap();
    let va = p.part(a).sound.params.clone();
    let mut r = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    for (i, part) in r.shared.parts.iter_mut().enumerate() {
        part.mix.channel = MidiChannel::new(if i == 0 { 0 } else { 15 }).unwrap();
    }
    *r.shared.parts[0].params = va.clone();
    r.note_on(0, 60);
    r.render(1);
    let v = (0..MAX_VOICES)
        .find(|&v| r.inst.voice(v).is_active())
        .expect("a VA voice");
    assert_eq!(
        PadFilter::of(&r.inst.voice(v).played_filter()),
        PadFilter::of(&va.filter)
    );
    load(&mut p, t, init(EngineType::Drum)).unwrap();
    assert_eq!(*p.kit(a).pad(pad(3)).sound(), want);
}

/// The slice-1 rule reads the Sound as finished: a MODE written before
/// its KIND (refused, then retried at the Sound's end) reaches the pads.
#[test]
fn slice1_rule_reads_the_finished_sound() {
    use chimera_core::dsp::filter::FilterMode;
    let (mut p, t) = Project::boxed();
    let a = PartId::ALL[0];
    let from = PartFrom::Init(EngineType::Drum);
    load(&mut p, t, PartSource { part: a, from }).unwrap();
    let flt = &mut p.edit_part(a).sound.params.filter;
    flt.set_kind(FilterKind::Dirty);
    assert!(flt.set_mode(FilterMode::Hp12), "DIRTY's, not CLEAN's");
    let f = encode(&p);
    // No pad sounds; Part 1's Filter block with MODE (7) before KIND (6).
    let mut g = f.clone();
    for (o, b) in records(&f, KIT_SOUND_TAG).into_iter().rev() {
        g.drain(o..o + 4 + b.len());
    }
    let (part, kit) = (records(&g, 0x8008)[0].0, kit_records(&g)[0].0);
    let (o, b) = records(&g, 0x0001)
        .into_iter()
        .find(|(o, b)| (part..kit).contains(o) && b[0] == 10)
        .expect("Part 1's Filter block");
    let entries: Vec<&[u8]> = b[1..].chunks(5).collect();
    let at = |id: u8| entries.iter().position(|e| e[0] == id).unwrap();
    let (kind, mode) = (at(6), at(7));
    assert!(kind < mode, "written KIND first");
    let mut e: Vec<&[u8]> = entries.clone();
    e.swap(kind, mode);
    g[o + 5..o + 4 + b.len()].copy_from_slice(&e.concat());
    fix_crc(&mut g);
    let (mut q, _) = Project::boxed();
    decode(&g, &mut q).unwrap();
    assert_eq!(q.part(a).sound.params.filter.mode(), FilterMode::Hp12);
    for n in PadId::ALL {
        let s = q.kit(a).pad(n).sound().filter();
        assert_eq!((s.kind(), s.mode()), (FilterKind::Dirty, FilterMode::Hp12));
    }
}
