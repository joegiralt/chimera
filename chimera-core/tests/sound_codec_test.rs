//! The Sound codec (ADR 0045): a Sound encodes to card records and decodes
//! back bit-equal; bad values fall back to the frozen neutral base.

mod common;

use chimera_core::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::filter::FilterMode;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::midi_map::{Cc, MAP_LEN, MapEntry, Mappable, MidiMap};
use chimera_core::mod_path::{LABEL_LEN, MAX_REGISTRY_DESTS};
use chimera_core::modulation::{FixedCol, MAX_MOD_DESTS, MAX_MOD_SOURCES, ModState};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot, PitchParams};
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    BuildStamp, Check, Decode, FileError, FileKind, Framer, Generation, HEADER_LEN, Header,
    MAX_SOUND_COST, Migration, RecordBuf, RecordTag, SoundCheck, SoundDecoder, TRAILER_LEN,
    ValidAddr, decode_block, encode_block, encode_library_sound, encode_sound, write_file,
};
use chimera_hal::store::{ByteSink, StoreError};
use common::XorShift;

struct VecSink(Vec<u8>);

impl ByteSink for VecSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

fn header(s: &Sound) -> Header {
    Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    }
}

fn encode(s: &Sound) -> Vec<u8> {
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &header(s), &mut |w| encode_sound(s, w)).unwrap();
    sink.0
}

/// A Sound file of hand-built records.
fn file(records: &[(RecordTag, Vec<u8>)]) -> Vec<u8> {
    let mut sink = VecSink(Vec::new());
    let h = header(&Sound::neutral(EngineType::Algo));
    write_file(&mut sink, &h, &mut |w| {
        records.iter().try_for_each(|(t, p)| w.put(*t, p))
    })
    .unwrap();
    sink.0
}

/// Two passes: check, then apply once the first `finish` is Ok.
fn decode(bytes: &[u8]) -> Result<Sound, FileError> {
    let mut s = Sound::neutral(EngineType::Algo);
    for apply in [false, true] {
        let mut d = SoundDecoder::new(&mut s);
        let mut f = Framer::new(bytes.len() as u32)?;
        f.push(bytes, &mut |e| if apply { d.apply(e) } else { d.event(e) })?;
        f.finish()?;
        if apply { d.commit() } else { d.end() }?;
    }
    Ok(s)
}

fn block(b: BlockRef, s: &Sound) -> Vec<u8> {
    let mut r = RecordBuf::new();
    encode_block(b, &*s.params.block(b).unwrap(), None, &mut r);
    r.as_slice().to_vec()
}

fn engine(e: EngineType) -> (RecordTag, Vec<u8>) {
    use chimera_core::block::DiskCode;
    (RecordTag::Engine, vec![e.disk_code()])
}

/// A Filter record of `(id, raw value)` entries.
fn filter(entries: &[(u8, [u8; 4])]) -> (RecordTag, Vec<u8>) {
    let mut p = vec![10];
    for (id, v) in entries {
        p.push(*id);
        p.extend_from_slice(v);
    }
    (RecordTag::Block, p)
}

fn assert_round_trip(s: &Sound) {
    let d = decode(&encode(s)).unwrap();
    assert!(d.bits_eq(s), "{}", s.name.as_str());
    assert_eq!(format!("{:?}", d.params), format!("{:?}", s.params));
    assert_eq!(d.name, s.name);
}

#[test]
fn factory_round_trip() {
    for i in 0..FACTORY_LEN {
        assert_round_trip(&factory_sound(i).unwrap());
    }
}

#[test]
fn init_round_trip() {
    for e in EngineType::ALL {
        let s = Sound::init(e);
        assert_round_trip(&s);
        assert_eq!(decode(&encode(&s)).unwrap().engine(), e);
    }
}

#[test]
fn zero_amount_route_survives() {
    let mut s = Sound::init(EngineType::Algo);
    s.mod_state.set_fixed(6, FixedCol::Freq, 0);
    let got = decode(&encode(&s)).unwrap();
    assert_ne!(got.mod_state.fixed_present(FixedCol::Freq) & 1 << 6, 0);
    assert!(got.bits_eq(&s));
}

/// The first `n` modulatable registry addresses (no fixed column's), in
/// `BlockRef::ALL` order.
fn modulatable(n: usize) -> Vec<ParamAddr> {
    BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter(|&a| a.modulatable() && FixedCol::of(a).is_none())
        .take(n)
        .collect()
}

#[test]
fn full_matrix_round_trip() {
    let mut s = Sound::neutral(EngineType::Modal);
    for (i, a) in modulatable(MAX_MOD_DESTS).into_iter().enumerate() {
        s.dest_registry.add(a, [b'A' + i as u8; LABEL_LEN]).unwrap();
    }
    s.mod_state = ModState::from_registry(&s.dest_registry, MAX_MOD_SOURCES);
    assert_eq!(s.mod_state.num_dests(), MAX_MOD_DESTS);
    let mut x: u32 = 0x9E37_79B9;
    for src in 0..MAX_MOD_SOURCES {
        for d in 0..MAX_MOD_DESTS {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            s.mod_state.set_route(src, d, x as u8 as i8);
        }
        // Every fixed column too: the largest Routes record there is.
        for c in FixedCol::ALL {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            s.mod_state.set_fixed(src, c, x as u8 as i8);
        }
    }
    let got = decode(&encode(&s)).unwrap();
    assert!(got.bits_eq(&s));
    assert_eq!(got.dest_registry.len(), MAX_REGISTRY_DESTS);
    assert_eq!(got.mod_state.present(MAX_MOD_DESTS - 1), 0xFF);
    assert_eq!(got.mod_state.fixed_present(FixedCol::Fx), 0xFF);
}

#[test]
fn bits_eq_sees_differences_in_range() {
    let a = Sound::init(EngineType::Algo);
    let mut b = a.clone();
    assert!(a.bits_eq(&b));
    b.mod_state.set_fixed(1, FixedCol::Freq, 5);
    assert!(!a.bits_eq(&b));

    let mut b = a.clone();
    b.dest_registry
        .add(modulatable(1)[0], [b'A'; LABEL_LEN])
        .unwrap();
    assert!(!a.bits_eq(&b));

    let mut b = a.clone();
    b.params.filter.cutoff += 1.0;
    assert!(!a.bits_eq(&b));
}

// `bits_eq_ignores_dead_slots` lives beside `ModState` and `ModDestRegistry`:
// only their own module can fill a slot past the live range.

#[test]
fn mode_before_kind_still_applies() {
    use chimera_core::block::DiskCode;
    let bp = u32::from(FilterMode::Bp12.disk_code()).to_le_bytes();
    let kind = u32::from(chimera_core::dsp::filter::FilterKind::Svf.disk_code()).to_le_bytes();
    let bytes = file(&[engine(EngineType::Algo), filter(&[(7, bp), (6, kind)])]);
    assert_eq!(
        decode(&bytes).unwrap().params.filter.mode(),
        FilterMode::Bp12
    );
}

fn neutral_filter() -> FilterParams {
    Sound::neutral(EngineType::Algo).params.filter
}

#[test]
fn nan_takes_base() {
    let bytes = file(&[
        engine(EngineType::Algo),
        filter(&[(0, 0x7FC0_0000u32.to_le_bytes())]),
    ]);
    let got = decode(&bytes).unwrap().params.filter.cutoff;
    assert_eq!(got.to_bits(), neutral_filter().cutoff.to_bits());
}

#[test]
fn out_of_range_clamps() {
    let bytes = file(&[
        engine(EngineType::Algo),
        filter(&[(0, 1e9f32.to_le_bytes())]),
        (RecordTag::Block, {
            let mut p = vec![19, PitchParams::PITCH.0];
            p.extend_from_slice(&3.4f32.to_le_bytes());
            p
        }),
    ]);
    let got = decode(&bytes).unwrap();
    assert_eq!(got.params.filter.cutoff, 20_000.0);
    assert_eq!(got.params.pitch.get(PitchParams::PITCH), 3.0);
}

#[test]
fn bad_enum_code_non_critical_keeps_base() {
    let bytes = file(&[
        engine(EngineType::Algo),
        filter(&[(7, 250u32.to_le_bytes())]),
    ]);
    assert_eq!(
        decode(&bytes).unwrap().params.filter.mode(),
        neutral_filter().mode()
    );
}

#[test]
fn missing_block_takes_neutral() {
    for e in EngineType::ALL {
        let mut s = factory_sound(0).unwrap();
        s.params = ParamSnapshot::for_engine(e);
        s.params.filter.cutoff = 440.0;
        s.params.filter.resonance = 0.5;
        let mut records = vec![engine(e)];
        for b in BlockRef::ALL {
            if b != BlockRef::Filter && s.params.block(b).is_some() {
                records.push((RecordTag::Block, block(b, &s)));
            }
        }
        let got = decode(&file(&records)).unwrap();
        assert_eq!(
            format!("{:?}", got.params.filter),
            format!("{:?}", Sound::neutral(e).params.filter)
        );
    }
}

#[test]
fn engine_not_first_is_corrupt() {
    let s = Sound::init(EngineType::Algo);
    let bytes = file(&[
        (RecordTag::Block, block(BlockRef::Filter, &s)),
        engine(EngineType::Algo),
    ]);
    assert_eq!(decode(&bytes).err(), Some(FileError::Corrupt));
    assert_eq!(decode(&file(&[])).err(), Some(FileError::Corrupt));
}

#[test]
fn unknown_engine_needs_newer() {
    let bytes = file(&[(RecordTag::Engine, vec![9])]);
    assert_eq!(decode(&bytes).err(), Some(FileError::NeedsNewerFirmware));
}

#[test]
fn block_payload_shape_is_checked() {
    let mut p = Sound::neutral(EngineType::Algo).params;
    assert_eq!(decode_block(&[], &[], &[], None), Err(FileError::Bounds));
    assert_eq!(
        decode_block(&[10, 0, 0], &[], &[], None),
        Err(FileError::Bounds)
    );
    let dup = [10, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0];
    assert_eq!(decode_block(&dup, &[], &[], None), Err(FileError::Corrupt));
    // Unknown block codes and param ids are skipped, and counted.
    assert_eq!(
        decode_block(&[200, 1, 0, 0, 0, 0], &[], &[], Some(&mut p)).map(|a| a.count),
        Ok(1)
    );
    assert_eq!(
        decode_block(&[10, 99, 0, 0, 0, 0], &[], &[], Some(&mut p)).map(|a| a.count),
        Ok(1)
    );
}

#[test]
fn migration_maps_old_id() {
    let m = Migration {
        block: 10,
        old: ParamId(5),
        new: ParamId(1),
        map: |v| v * 0.5,
    };
    let mut p = vec![10, 5];
    p.extend_from_slice(&0.8f32.to_le_bytes());
    let mut snap = Sound::neutral(EngineType::Algo).params;
    decode_block(&p, &[m], &[], Some(&mut snap)).unwrap();
    assert_eq!(snap.filter.resonance, 0.4);
}

#[test]
fn init_is_named_init() {
    for e in EngineType::ALL {
        assert_eq!(Sound::init(e).name.as_str(), "INIT");
        assert_eq!(Sound::neutral(e).name.as_str(), "INIT");
    }
}

/// A draw in 0..1.
fn unit(rng: &mut XorShift) -> f32 {
    (rng.next_u32() >> 8) as f32 / (1 << 24) as f32
}

/// Every stored param of every voice block at a random value in range (an
/// enum at a random code it accepts), random primed dests and routes.
fn random_sound(e: EngineType, rng: &mut XorShift) -> Sound {
    let mut s = Sound::neutral(e);
    for b in BlockRef::ALL {
        let Some(mut blk) = s.params.block_mut(b) else {
            continue;
        };
        for a in ValidAddr::of_block(b) {
            let spec = a.spec();
            if a.coded() {
                // Codes this value accepts now (KIND already set: MODE's
                // list is the KIND's). A refused code writes nothing.
                let ok: Vec<u8> = (0..=u8::MAX)
                    .filter(|&c| blk.set_enum_code(spec.id, c))
                    .collect();
                assert!(blk.set_enum_code(spec.id, ok[rng.below(ok.len())]));
            } else {
                blk.set(spec.id, spec.min + unit(rng) * (spec.max - spec.min));
            }
        }
    }
    let mut addrs = modulatable(usize::MAX);
    let n = rng.below(MAX_MOD_DESTS + 1);
    for i in 0..n {
        let a = addrs.swap_remove(rng.below(addrs.len()));
        let label = [b'A' + i as u8; LABEL_LEN];
        s.dest_registry.add(a, label).unwrap();
    }
    s.mod_state = ModState::from_registry(&s.dest_registry, 1 + rng.below(MAX_MOD_SOURCES));
    for src in 0..s.mod_state.num_sources() {
        for d in 0..s.mod_state.num_dests() {
            if rng.below(2) == 0 {
                s.mod_state.set_route(src, d, rng.next_u32() as u8 as i8);
            }
        }
        for c in FixedCol::ALL {
            if rng.below(2) == 0 {
                s.mod_state.set_fixed(src, c, rng.next_u32() as u8 as i8);
            }
        }
    }
    s
}

#[test]
fn random_sounds_round_trip() {
    let mut rng = XorShift(0xC0DE_CAFE);
    for e in EngineType::ALL {
        for _ in 0..150 {
            assert_round_trip(&random_sound(e, &mut rng));
        }
    }
}

#[test]
fn a_block_record_twice_is_corrupt() {
    let s = Sound::init(EngineType::Algo);
    let f = (RecordTag::Block, block(BlockRef::Filter, &s));
    let bytes = file(&[engine(EngineType::Algo), f.clone(), f]);
    assert_eq!(decode(&bytes).err(), Some(FileError::Corrupt));
}

/// The frozen base, pinned per engine. A sparse file omits every param at
/// neutral, so a change here moves every saved Sound: it needs a migration.
#[test]
fn neutral_is_a_contract() {
    let golden = include_str!("fixtures/neutral_v1.txt");
    for e in EngineType::ALL {
        let want = golden
            .replace("engine: ENGINE,", &format!("engine: {e:?},"))
            .replace(
                "kind: Svf,",
                &format!(
                    "kind: {:?},",
                    chimera_core::dsp::filter::FilterKind::default_for(e)
                ),
            );
        let got = format!("{:#?}\n", Sound::neutral(e));
        assert!(
            got == want,
            "neutral {e:?} moved: changing neutral needs a migration (ADR 0087); now:\n{got}"
        );
    }
}

/// STEAL and GLIDE TIME (#254) round-trip.
#[test]
fn steal_and_glide_time_round_trip() {
    use chimera_core::params::Steal;
    for e in EngineType::ALL {
        let mut s = Sound::init(e);
        s.params.pitch.steal = Steal::Glide;
        s.params.pitch.set(PitchParams::GLIDE_TIME, 0.25);
        assert_round_trip(&s);
        let d = decode(&encode(&s)).unwrap();
        assert_eq!(d.params.pitch.steal, Steal::Glide);
        assert_eq!(d.params.pitch.glide_time, 0.25);
    }
}

/// A Pitch record from before STEAL (PITCH and FINE only) decodes to CUT,
/// GLIDE TIME at its 150 ms.
#[test]
fn a_pitch_record_before_steal_decodes_to_cut() {
    use chimera_core::params::Steal;
    let mut p = vec![19];
    for (id, v) in [(PitchParams::PITCH, 3.0_f32), (PitchParams::FINE, 0.0)] {
        p.push(id.0);
        p.extend_from_slice(&v.to_le_bytes());
    }
    for e in EngineType::ALL {
        let got = decode(&file(&[engine(e), (RecordTag::Block, p.clone())])).unwrap();
        assert_eq!(got.params.pitch.pitch, 3.0);
        assert_eq!(got.params.pitch.steal, Steal::Cut, "{e:?}");
        assert!(
            (got.params.pitch.glide_secs() - 0.150).abs() < 1e-4,
            "{e:?}"
        );
    }
}

/// `<engine code> <KIND code> <MODE code>`, append-only: a new engine adds a row.
const NEUTRAL_FILTER_ROWS: &str = include_str!("fixtures/neutral_filter_v1.txt");

#[test]
fn neutral_filter_rows_are_pinned() {
    use chimera_core::block::DiskCode;
    let rows: Vec<[u8; 3]> = NEUTRAL_FILTER_ROWS
        .lines()
        .map(|l| {
            let v: Vec<u8> = l.split(' ').map(|n| n.parse().unwrap()).collect();
            [v[0], v[1], v[2]]
        })
        .collect();
    assert_eq!(rows.len(), EngineType::ALL.len());
    for e in EngineType::ALL {
        let row = rows
            .iter()
            .find(|r| r[0] == e.disk_code())
            .unwrap_or_else(|| panic!("no row for {e:?}"));
        let codes = |s: &Sound| {
            [
                e.disk_code(),
                s.params.filter.kind().disk_code(),
                s.params.filter.mode().disk_code(),
            ]
        };
        assert_eq!(&codes(&Sound::init(e)), row, "init {e:?}");
        assert_eq!(&codes(&Sound::neutral(e)), row, "neutral {e:?}");
        let bare = decode(&file(&[engine(e)])).unwrap();
        assert_eq!(&codes(&bare), row, "decoded {e:?}");
    }
}

/// The factory Sounds, INIT on each engine, then `n` random Sounds.
fn sparse_cases(n: usize) -> Vec<Sound> {
    let mut v: Vec<Sound> = (0..FACTORY_LEN)
        .map(|i| factory_sound(i).unwrap())
        .collect();
    v.extend(EngineType::ALL.map(Sound::init));
    let mut rng = XorShift(0x5EED_5A25);
    for i in 0..n {
        v.push(random_sound(EngineType::ALL[i % 2], &mut rng));
    }
    v
}

/// The bytes a Sound costs in the pool: its 16 B name and its records.
fn cost(file: &[u8]) -> usize {
    16 + file.len() - HEADER_LEN - TRAILER_LEN
}

#[test]
fn sparse_round_trip() {
    for s in sparse_cases(1000) {
        assert_round_trip(&s);
    }
}

#[test]
fn sparse_byte_identity() {
    for s in sparse_cases(1000) {
        let f = encode(&s);
        assert_eq!(encode(&decode(&f).unwrap()), f, "{}", s.name.as_str());
    }
    // The old encoder's full files (the eight table Sounds, from before the
    // formant demos): a factory one re-encodes to its Sound's sparse bytes,
    // and INIT's sparse form decodes the same.
    for i in 0..8 {
        let full = v1_fixture(&format!("factory_{i}.snd"));
        let mut s = factory_sound(i).unwrap();
        s.map = chimera_core::midi_map::MidiMap::EMPTY; // the old files predate it
        assert_eq!(cost(&full), 1143 + 16 * usize::from(i == 6), "{i}");
        assert_eq!(encode(&decode(&full).unwrap()), encode(&s), "{i}");
    }
    for name in ["init_algo.snd", "init_modal.snd"] {
        let old = decode(&v1_fixture(name)).unwrap();
        let sparse = encode(&old);
        assert!(decode(&sparse).unwrap().bits_eq(&old), "{name}");
    }
}

fn v1_fixture(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1");
    std::fs::read(p.join(name)).unwrap()
}

/// One random stored param of `s` moved one step.
fn nudged(s: &Sound, rng: &mut XorShift) -> Sound {
    let mut t = s.clone();
    let blocks: Vec<BlockRef> = BlockRef::ALL
        .into_iter()
        .filter(|&b| s.params.block(b).is_some())
        .collect();
    let b = blocks[rng.below(blocks.len())];
    let addrs: Vec<ValidAddr> = ValidAddr::of_block(b).collect();
    let id = addrs[rng.below(addrs.len())].spec().id;
    let dir = if rng.below(2) == 0 { -1 } else { 1 };
    t.params.block_mut(b).unwrap().nudge(id, dir);
    t
}

#[test]
fn sparse_is_canonical() {
    let mut rng = XorShift(0xCA70_0001);
    let (mut same, mut moved) = (0, 0);
    for (i, a) in sparse_cases(1000).into_iter().enumerate().take(1000) {
        let b = nudged(&a, &mut rng);
        let eq = a.bits_eq(&b);
        assert_eq!(encode(&a) == encode(&b), eq, "case {i}");
        if eq { same += 1 } else { moved += 1 }
    }
    assert!(same > 0 && moved > 900, "{same} same, {moved} moved");
}

#[test]
fn sparse_sizes() {
    let costs: Vec<usize> = (0..FACTORY_LEN)
        .map(|i| cost(&encode(&factory_sound(i).unwrap())))
        .collect();
    println!("factory: {costs:?}");
    // The table Sounds, then the formant demos, whose routes, LFOs and
    // envelopes add about 200 B.
    let (table, demos) = costs.split_at(8);
    assert!(table.iter().all(|c| (190..=320).contains(c)), "{costs:?}");
    assert!(demos.iter().all(|c| (459..=539).contains(c)), "{costs:?}");
    // 46 B of name, Engine, an empty Registry, ModDests and Routes (its
    // three fixed-FREQ routes), and 70 B for the values INIT moved off
    // neutral (ADR 0063): RR on six operators and OUT LEVEL, a one-param
    // block each.
    // TAPE's INIT routes ENV 1 into VCA: one fixed-column route more, 4 B.
    // VA's too, and its FINE2 and STAB: one block of two params, 15 B.
    for e in EngineType::ALL {
        let more = match e {
            EngineType::Algo | EngineType::Modal => 0,
            EngineType::Tape => 4,
            EngineType::Va => 4 + 15,
        };
        assert_eq!(
            cost(&encode(&Sound::plain_init(e))),
            46 + 7 * 10 + more,
            "{e:?}"
        );
    }
    // INIT as it ships: the default map's eleven entries and the wheel's
    // vibrato (spec § 9).
    assert_eq!(encode(&Sound::init(EngineType::Algo)).len(), 217);
}

/// `neutral(e)` with every stored param that can move at another value, a
/// full registry, every route and tap and a source; and how many can't
/// (one-value enums).
fn worst(e: EngineType) -> (Sound, usize) {
    let neutral = Sound::neutral(e);
    let mut s = Sound::neutral(e);
    let mut stuck = 0;
    for b in BlockRef::ALL {
        let Some(mut blk) = s.params.block_mut(b) else {
            continue;
        };
        let base = neutral.params.block(b).unwrap();
        for a in ValidAddr::of_block(b) {
            let (id, sp) = (a.spec().id, a.spec());
            if a.coded() {
                let was = base.enum_code(id).unwrap();
                if !(0..=u8::MAX).any(|c| c != was && blk.set_enum_code(id, c)) {
                    stuck += 1;
                }
            } else {
                blk.set(
                    id,
                    if base.get(id) == sp.max {
                        sp.min
                    } else {
                        sp.max
                    },
                );
            }
        }
        for a in ValidAddr::of_block(b) {
            let (id, ident) = (a.spec().id, a.spec().ident);
            let moved = blk.get(id).to_bits() != base.get(id).to_bits();
            assert!(moved || a.coded(), "{b:?} {ident} didn't move");
        }
    }
    for (i, a) in modulatable(MAX_REGISTRY_DESTS).into_iter().enumerate() {
        s.dest_registry.add(a, [b'A' + i as u8; LABEL_LEN]).unwrap();
    }
    s.mod_state = ModState::from_registry(&s.dest_registry, MAX_MOD_SOURCES);
    for src in 0..MAX_MOD_SOURCES {
        for d in 0..MAX_MOD_DESTS {
            s.mod_state.set_route(src, d, 1);
        }
        for c in FixedCol::ALL {
            s.mod_state.set_fixed(src, c, 1);
        }
    }
    for op in chimera_core::taps::TapOp::ALL {
        for c in FixedCol::ALL {
            s.params.taps.set(op, c, 1);
        }
    }
    let tune = chimera_core::storage::ZoneTune::new(&[(6_000, 1 << 16); 24]);
    assert!(tune.is_some());
    s.source = Some(chimera_core::storage::TapeSource {
        r: chimera_core::project::SourceRef {
            crc: 0xC401_0001,
            frames: 27_900,
            rate: 24_000,
        },
        tune,
    });
    s.map = mapped();
    (s, stuck)
}

/// 16 + 5 (`Engine`) + 20 × 5 (block heads) + 277 × 5 (params) + 164
/// (`Registry`) + 37 (`ModDests`) + 644 (`Routes`, the fixed columns'
/// among them) + 36 (`TapRoutes`) + 14 (`SourceRef`) + 148 (`ZoneTune`,
/// 24 zones) + 84 (`MidiMap`, 16 entries) = 2 633 B, less 5 B for each param that has nowhere to move.
/// A new param moves it: raise
/// `MAX_SOUND_COST` with it.
#[test]
fn sparse_bound() {
    let mut most = 0;
    for e in EngineType::ALL {
        let (s, stuck) = worst(e);
        let c = cost(&encode(&s));
        println!("worst {e:?}: {c} B, {stuck} stuck");
        assert!(c <= MAX_SOUND_COST, "{e:?} {c}");
        most = most.max(c + 5 * stuck);
    }
    assert_eq!(most, 2633);
    assert_eq!(MAX_SOUND_COST, most.next_multiple_of(16));
}

/// The sends live on the Sound's OUT block (ADR 0123): they round-trip, in
/// a plain file and in a library one, and at 0 write nothing.
#[test]
fn sends_round_trip_and_cost_nothing_at_zero() {
    let mut s = Sound::init(EngineType::Algo);
    s.params.out.sends = [0.25, 0.5, 1.0];
    assert_round_trip(&s);
    assert_eq!(
        decode(&encode(&s)).unwrap().params.out.sends,
        [0.25, 0.5, 1.0]
    );

    // The library save and load.
    let h = header(&s);
    let st = BuildStamp::new("0.1.0", "02e440e2", false);
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| {
        encode_library_sound(&s, &st, None, w)
    })
    .unwrap();
    let back = decode(&sink.0).unwrap();
    assert!(back.bits_eq(&s));
    assert_eq!(back.params.out.sends, [0.25, 0.5, 1.0]);

    // Sends at 0 write nothing: INIT's bytes are the old encoder's.
    let init = Sound::plain_init(EngineType::Algo);
    let plain = encode(&init);
    // Pinned from the encoder before the sends moved: length and CRC.
    assert_eq!(
        (plain.len(), plain[plain.len() - 4..].to_vec()),
        (132, vec![87, 103, 46, 253])
    );
    let mut lib = VecSink(Vec::new());
    write_file(&mut lib, &header(&init), &mut |w| {
        encode_library_sound(&init, &st, None, w)
    })
    .unwrap();
    assert_eq!(decode(&lib.0).unwrap().params.out.sends, [0.0; 3]);
}

/// 16 mappings: Part LEVEL and PAN, then the first Sound params, with an
/// inverted range and the widest `max`.
fn mapped() -> MidiMap {
    let mut m = MidiMap::EMPTY;
    let targets = BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter_map(Mappable::new)
        .filter(|t| t.addr().block != BlockRef::Part);
    let part = [PartParams::LEVEL, PartParams::PAN]
        .map(|id| Mappable::new(ParamAddr::new(BlockRef::Part, id)).unwrap());
    for (i, t) in part.into_iter().chain(targets).take(MAP_LEN).enumerate() {
        m.learn(Cc::new(70 + i as u8).unwrap(), t).unwrap();
    }
    assert_eq!(m.len(), MAP_LEN);
    let mut e = |i: usize, f: fn(&mut MapEntry)| {
        let mut x = m.entries()[i];
        f(&mut x);
        m.set(i, x);
    };
    e(3, |x| (x.min, x.max) = (200, 10));
    e(4, |x| (x.min, x.max) = (0, 128));
    m
}

fn mapped_sound() -> Sound {
    let mut s = Sound::init(EngineType::Algo);
    s.map = mapped();
    s
}

#[test]
fn map_round_trips() {
    let s = mapped_sound();
    assert_round_trip(&s);
    assert_eq!(decode(&encode(&s)).unwrap().map, s.map);

    let st = BuildStamp::new("0.1.0", "02e440e2", false);
    let mut lib = VecSink(Vec::new());
    write_file(&mut lib, &header(&s), &mut |w| {
        encode_library_sound(&s, &st, None, w)
    })
    .unwrap();
    let back = decode(&lib.0).unwrap();
    assert!(back.bits_eq(&s));
    assert_eq!(back.map, s.map);
}

/// An empty map writes no record: a neutral Sound's bytes are what they
/// were before the field. Length and CRC, pinned from the encoder before
/// `MidiMap` existed.
#[test]
fn empty_map_writes_no_record() {
    let pins = [
        (EngineType::Algo, [179, 214, 46, 66]),
        (EngineType::Modal, [197, 55, 33, 223]),
        (EngineType::Tape, [30, 18, 64, 163]),
        (EngineType::Va, [104, 243, 79, 62]),
    ];
    for (e, crc) in pins {
        let b = encode(&Sound::neutral(e));
        assert_eq!((b.len(), &b[b.len() - 4..]), (50, &crc[..]), "{e:?}");
    }
    let init = encode(&Sound::plain_init(EngineType::Algo));
    assert_eq!(
        (init.len(), init[init.len() - 4..].to_vec()),
        (132, vec![87, 103, 46, 253])
    );
}

fn map_file(p: &[u8]) -> Vec<u8> {
    file(&[engine(EngineType::Algo), (RecordTag::MidiMap, p.to_vec())])
}

fn skips(bytes: &[u8]) -> (u16, Sound) {
    let mut s = Sound::neutral(EngineType::Algo);
    let mut c = SoundCheck::new();
    for pass in [None, Some(&mut s)] {
        let mut f = Framer::new(bytes.len() as u32).unwrap();
        match pass {
            None => f.push(bytes, &mut |e| c.step(e, None)).unwrap(),
            Some(t) => f.push(bytes, &mut |e| c.step(e, Some(&mut *t))).unwrap(),
        }
        f.finish().unwrap();
    }
    c.finish(Some(&mut s)).unwrap();
    (c.skips(), s)
}

#[test]
fn map_record_bounds() {
    for n in [4, 81, 85] {
        assert_eq!(
            decode(&map_file(&vec![0; n])).err(),
            Some(FileError::Bounds),
            "{n} B"
        );
    }
    // An address this firmware lacks; Part CHANNEL, not Mappable; CC 64.
    let part = BlockRef::Part.disk_code().unwrap();
    let cutoff = [10, FilterParams::CUTOFF.0];
    let p = [
        [5, 99, 99, 0, 255],
        [5, part, PartParams::CHANNEL.0, 0, 255],
        [64, cutoff[0], cutoff[1], 0, 255],
    ]
    .concat();
    let (lost, s) = skips(&map_file(&p));
    assert_eq!(lost, 3);
    assert!(s.map.is_empty());
}

#[test]
fn map_record_dedups_to_the_last_and_drops_cc_over_127() {
    let cutoff = [10, FilterParams::CUTOFF.0];
    let p = [
        [5, cutoff[0], cutoff[1], 0, 255],
        [128, 10, FilterParams::RESONANCE.0, 0, 255],
        [9, cutoff[0], cutoff[1], 7, 9],
    ]
    .concat();
    let (lost, s) = skips(&map_file(&p));
    assert_eq!(lost, 1);
    let e = s.map.entries();
    assert_eq!(e.len(), 1);
    assert_eq!((e[0].cc.get(), e[0].min, e[0].max), (9, 7, 9));
}

#[test]
fn map_record_once() {
    let one = vec![5, 10, FilterParams::CUTOFF.0, 0, 255];
    let bytes = file(&[
        engine(EngineType::Algo),
        (RecordTag::MidiMap, one.clone()),
        (RecordTag::MidiMap, one),
    ]);
    assert_eq!(decode(&bytes).err(), Some(FileError::Corrupt));
}
