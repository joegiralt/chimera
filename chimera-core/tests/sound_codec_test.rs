//! The Sound codec (ADR 0045): a Sound encodes to card records and decodes
//! back bit-equal; bad values fall back to the frozen neutral base.

use chimera_core::addr::{BlockRef, Blocks, ParamAddr};
use chimera_core::block::{Block, ParamId};
use chimera_core::dsp::filter::FilterMode;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::mod_path::{LABEL_LEN, MAX_REGISTRY_DESTS};
use chimera_core::modulation::{CUTOFF, MAX_MOD_DESTS, MAX_MOD_SOURCES, ModState};
use chimera_core::params::{EngineType, FilterParams, ParamSnapshot, PitchParams};
use chimera_core::preset::Sound;
use chimera_core::storage::{
    Check, Decode, FileError, FileKind, Framer, Generation, Header, Migration, RecordBuf,
    RecordTag, SoundDecoder, ValidAddr, decode_block, encode_block, encode_sound, write_file,
};
use chimera_hal::store::{ByteSink, StoreError};

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
    encode_block(b, s.params.block(b).unwrap(), &mut r);
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
    let d = s.mod_state.find(CUTOFF).unwrap();
    s.mod_state.set_route(6, d, 0);
    let got = decode(&encode(&s)).unwrap();
    assert_ne!(got.mod_state.present(d) & 1 << 6, 0);
    assert!(got.bits_eq(&s));
}

/// The first `n` modulatable addresses, in `BlockRef::ALL` order.
fn modulatable(n: usize) -> Vec<ParamAddr> {
    BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter(|a| a.modulatable())
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
    }
    let got = decode(&encode(&s)).unwrap();
    assert!(got.bits_eq(&s));
    assert_eq!(got.dest_registry.len(), MAX_REGISTRY_DESTS);
    assert_eq!(got.mod_state.present(MAX_MOD_DESTS - 1), 0xFF);
}

#[test]
fn bits_eq_sees_differences_in_range() {
    let a = Sound::init(EngineType::Algo);
    let mut b = a.clone();
    assert!(a.bits_eq(&b));
    b.mod_state.set_route(1, 0, 5);
    assert!(!a.bits_eq(&b));

    let mut b = a.clone();
    b.dest_registry.remove(CUTOFF);
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
    // Unknown block codes and param ids are skipped.
    assert_eq!(
        decode_block(&[200, 1, 0, 0, 0, 0], &[], &[], Some(&mut p)),
        Ok(())
    );
    assert_eq!(
        decode_block(&[10, 99, 0, 0, 0, 0], &[], &[], Some(&mut p)),
        Ok(())
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

/// xorshift32: the same Sounds every run.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        self.next() as usize % n
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1 << 24) as f32
    }
}

/// Every stored param of every voice block at a random value in range (an
/// enum at a random code it accepts), random primed dests and routes.
fn random_sound(e: EngineType, rng: &mut Rng) -> Sound {
    let mut s = Sound::neutral(e);
    for b in BlockRef::ALL {
        let Some(blk) = s.params.block_mut(b) else {
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
                blk.set(spec.id, spec.min + rng.unit() * (spec.max - spec.min));
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
                s.mod_state.set_route(src, d, rng.next() as u8 as i8);
            }
        }
    }
    s
}

#[test]
fn random_sounds_round_trip() {
    let mut rng = Rng(0xC0DE_CAFE);
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

/// The frozen base, pinned per engine: a change to a default moves every
/// old file's sound.
#[test]
fn neutral_is_pinned() {
    let golden = include_str!("fixtures/neutral_v1.txt");
    for e in EngineType::ALL {
        let want = golden.replace("engine: ENGINE,", &format!("engine: {e:?},"));
        let got = format!("{:#?}\n", Sound::neutral(e));
        assert!(got == want, "neutral {e:?} moved; now:\n{got}");
    }
}
