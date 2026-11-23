// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The four permanent matrix columns, FREQ, Q, VCA and FOLD (filter-types
//! spec § 7): outside the registry, migrated from old patches, saved as
//! ordinary route records.

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::modulation::{CUTOFF, FixedCol, MAX_MOD_SOURCES, ModSource, ModState, VCA};
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::preset::Sound;
use chimera_core::storage::{
    Check, Decode, FileError, FileKind, Framer, Generation, Header, RecordTag, SoundDecoder,
    encode_sound, write_file,
};
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;
use common::codec_util::VecSink;

fn header() -> Header {
    Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(Sound::init_name()),
    }
}

fn file(records: &[(RecordTag, Vec<u8>)]) -> Vec<u8> {
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &header(), &mut |w| {
        records.iter().try_for_each(|(t, p)| w.put(*t, p))
    })
    .unwrap();
    sink.0
}

fn encode(s: &Sound) -> Vec<u8> {
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &header(), &mut |w| encode_sound(s, w)).unwrap();
    sink.0
}

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

fn code(a: ParamAddr) -> [u8; 2] {
    [a.block.disk_code().unwrap(), a.param.0]
}

/// `n` modulatable addresses that are neither fixed nor FILTER LEVEL.
fn others(n: usize) -> Vec<ParamAddr> {
    let level = ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL);
    BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter(|&a| a.modulatable() && FixedCol::of(a).is_none() && a != level)
        .take(n)
        .collect()
}

/// A file whose registry and ModDests list `dests` (`(block, id)` codes),
/// each with a route from every source at amount `i + 1` (source `s` adds `s`).
/// The sources a full `Routes` record holds at 640 B: ten into sixteen
/// columns (SLIDE's go to `PerfRoutes` only).
const FILE_SOURCES: [ModSource; 10] = {
    let mut a = [ModSource::Env1; 10];
    let mut i = 0;
    while i < 10 {
        a[i] = ModSource::ALL[i];
        i += 1;
    }
    a
};

fn full_file(dests: &[[u8; 2]]) -> Vec<u8> {
    use chimera_core::block::DiskCode;
    let mut reg = Vec::new();
    let mut md = vec![MAX_MOD_SOURCES as u8];
    let mut routes = Vec::new();
    for (i, d) in dests.iter().enumerate() {
        reg.extend_from_slice(d);
        reg.extend_from_slice(b"TEST\0\0\0\0");
        md.extend_from_slice(d);
        for s in FILE_SOURCES {
            routes.extend_from_slice(&[s.disk_code(), d[0], d[1], amount(i, s)]);
        }
    }
    file(&[
        (RecordTag::Engine, vec![EngineType::Algo.disk_code()]),
        (RecordTag::Registry, reg),
        (RecordTag::ModDests, md),
        (RecordTag::Routes, routes),
    ])
}

fn amount(i: usize, s: ModSource) -> u8 {
    (i + 1 + s.index()) as u8
}

/// The amount `s → a` reads at, fixed or registry; `None` without a route.
fn read(m: &ModState, s: ModSource, a: ParamAddr) -> Option<i8> {
    match FixedCol::of(a) {
        Some(c) => (m.fixed_present(c) & 1 << s.index() != 0).then(|| m.fixed_amount(s.index(), c)),
        None => {
            let d = m.find(a)?;
            (m.present(d) & 1 << s.index() != 0).then(|| m.amount(s.index(), d))
        }
    }
}

#[test]
fn a_full_registry_patch_keeps_every_route_and_frees_four_columns() {
    let mut addrs: Vec<ParamAddr> = FixedCol::ALL.iter().map(|c| c.addr()).collect();
    addrs.extend(others(12));
    let dests: Vec<[u8; 2]> = addrs.iter().map(|&a| code(a)).collect();
    let s = decode(&full_file(&dests)).unwrap();
    let m = &s.mod_state;
    for (i, &a) in addrs.iter().enumerate() {
        for src in FILE_SOURCES {
            assert_eq!(
                read(m, src, a),
                Some(amount(i, src) as i8),
                "{src:?} → {a:?}"
            );
        }
    }
    assert_eq!(m.num_dests(), 12);
    assert_eq!(s.dest_registry.len(), 12);
    let mut m = m.clone();
    let mut reg = s.dest_registry.clone();
    for a in others(16).into_iter().skip(12) {
        assert!(m.push(a).is_some(), "{a:?}");
        reg.add(a, *b"TEST\0\0\0\0").unwrap();
    }
    for c in FixedCol::ALL {
        assert_eq!(m.push(c.addr()), None, "{c:?} is never a registry column");
        assert!(reg.add(c.addr(), *b"TEST\0\0\0\0").is_err(), "{c:?}");
    }
}

#[test]
fn full_registry_with_drive_and_cutoff_keeps_every_route() {
    // (10, 2): the retired FILTER DRIVE, whose routes load at FILTER LEVEL.
    let mut dests = vec![[10, 2], code(CUTOFF)];
    dests.extend(others(14).into_iter().map(code));
    let s = decode(&full_file(&dests)).unwrap();
    let m = &s.mod_state;
    let level = ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL);
    for src in FILE_SOURCES {
        assert_eq!(read(m, src, level), Some(amount(0, src) as i8));
        assert_eq!(read(m, src, CUTOFF), Some(amount(1, src) as i8));
    }
    for (i, a) in others(14).into_iter().enumerate() {
        for src in FILE_SOURCES {
            assert_eq!(read(m, src, a), Some(amount(i + 2, src) as i8));
        }
    }
    assert_eq!(m.num_dests(), 15);
}

#[test]
fn fixed_columns_round_trip_through_a_save() {
    let mut s = Sound::plain_init(EngineType::Algo);
    let m = &mut s.mod_state;
    for (i, c) in FixedCol::ALL.into_iter().enumerate() {
        m.set_fixed(ModSource::Lfo2.index(), c, -(i as i8) - 3);
        m.set_fixed(ModSource::Vel.index(), c, 0);
    }
    for (i, a) in others(3).into_iter().enumerate() {
        assert!(m.set_route_addr(ModSource::Env3.index(), a, 10 + i as i8));
        s.dest_registry.add(a, *b"TEST\0\0\0\0").unwrap();
    }
    let back = decode(&encode(&s)).unwrap();
    assert!(back.bits_eq(&s));
    let mut moved = s.clone();
    moved
        .mod_state
        .clear_fixed(ModSource::Vel.index(), FixedCol::Fx);
    assert!(!back.bits_eq(&moved), "a fixed route's presence counts");
}

#[test]
fn init_routes_live_in_fixed_freq() {
    for e in EngineType::ALL {
        let s = Sound::init(e);
        let m = &s.mod_state;
        let want = [ModSource::Env1, ModSource::Lfo1, ModSource::Note]
            .iter()
            .fold(0u16, |b, s| b | 1 << s.index());
        assert_eq!(m.fixed_present(FixedCol::Freq), want, "{e:?}");
        for c in [FixedCol::Q, FixedCol::Fx] {
            assert_eq!(m.fixed_present(c), 0);
        }
        // TAPE's loop and VA's tone have no end: ENV 1 into VCA ends the note.
        let vca = if matches!(e, EngineType::Tape | EngineType::Va) {
            1 << ModSource::Env1.index()
        } else {
            0
        };
        assert_eq!(m.fixed_present(FixedCol::Vca), vca, "{e:?}");
        // The registry holds only the wheel's PITCH and LFO 1 DEPTH (spec
        // § 9, #876).
        assert_eq!(s.dest_registry.len(), 2);
        assert_eq!(m.num_dests(), 2);
    }
}

#[test]
fn fixed_routes_drive_the_voice_as_registry_routes_did() {
    let mut p = common::init_params(EngineType::Algo);
    p.filter.cutoff = 800.0;
    p.lfos[0].rate = common::MOD_LFO_RATE;
    let mut ms = ModState::with_sources(MAX_MOD_SOURCES);
    assert!(ms.set_route_addr(ModSource::Lfo1.index(), CUTOFF, 64));
    assert_eq!(ms.num_dests(), 0, "CUTOFF is fixed FREQ");
    // Recorded from the registry route (LFO 1 → a CUTOFF column at 64)
    // before the fixed columns existed.
    assert_eq!(
        common::fnv1a(&common::render_sound(&p, &ms)),
        0x8f4c_b2c5_e0cf_afed
    );
}

/// ENV 2 → VCA in the fixed column: the voice ends at the end of the block
/// in which ENV 2 goes idle (vca_test's case).
#[test]
fn vca_rule_reads_the_fixed_vca_column() {
    use chimera_core::dsp::envelope::{EnvMods, Envelope};
    let mut p = common::init_params(EngineType::Algo);
    p.algo.ops[0].rr = 1;
    let mut ms = ModState::with_sources(MAX_MOD_SOURCES);
    assert!(ms.set_route_addr(ModSource::Env2.index(), VCA, 127));
    assert_eq!(
        ms.fixed_present(FixedCol::Vca),
        1 << ModSource::Env2.index()
    );
    let mut v = common::Rig::new(common::SR);
    v.note_on(MidiNote::new(60).unwrap(), Velocity::DEFAULT, &p);
    let mut b = [0.0f32; BLOCK_SIZE];
    let alive: Vec<bool> = (0..400)
        .map(|i| {
            if i == 20 {
                v.note_off();
            }
            v.render(&mut b, &p, &ms);
            v.is_active()
        })
        .collect();
    let mut env = Envelope::new();
    env.note_on();
    let idle = (0..400)
        .find(|&b| {
            let mut g = [0.0f32; BLOCK_SIZE];
            env.run_block(
                &p.envelopes[1],
                &EnvMods::NONE,
                b < 20,
                common::SR,
                Some((&mut g, (1.0, 1.0))),
            );
            env.is_idle()
        })
        .unwrap();
    assert!(alive[idle - 1] && !alive[idle], "ends at block {idle}");
}

// ── Task 11 review fold-ins and Task 12's jacks on disk ──

/// Every source into 16 registry columns and the four fixed: the first
/// eight's 160 routes fill the 640 B `Routes` record, the rest
/// `PerfRoutes`, saved and loaded.
#[test]
fn an_eleven_by_twenty_matrix_round_trips() {
    use chimera_core::modulation::MAX_MOD_DESTS;
    let mut s = Sound::plain_init(EngineType::Algo);
    let regs = others(MAX_MOD_DESTS);
    assert_eq!(regs.len(), MAX_MOD_DESTS);
    for &a in &regs {
        s.dest_registry.add(a, *b"TEST\0\0\0\0").unwrap();
    }
    s.mod_state = ModState::from_registry(&s.dest_registry, MAX_MOD_SOURCES);
    let mut n = 0;
    for src in ModSource::ALL {
        for (i, c) in FixedCol::ALL.into_iter().enumerate() {
            s.mod_state
                .set_fixed(src.index(), c, -(i as i8) - src.index() as i8);
            n += 1;
        }
        for (i, &a) in regs.iter().enumerate() {
            assert!(
                s.mod_state
                    .set_route_addr(src.index(), a, (i + src.index()) as i8 + 1)
            );
            n += 1;
        }
    }
    // 160 in `Routes`, WHEEL's, PRESSURE's and SLIDE's 60 in `PerfRoutes`.
    assert_eq!(n, 220);
    let back = decode(&encode(&s)).unwrap();
    assert!(back.bits_eq(&s));
}

/// One route past the room: the Routes record is `Bounds`.
#[test]
fn a_161_route_record_loads_as_bounds() {
    use chimera_core::block::DiskCode;
    use chimera_core::storage::{Event, ReadTag, SoundCheck};
    let route = [
        ModSource::Env1.disk_code(),
        code(CUTOFF)[0],
        code(CUTOFF)[1],
        1,
    ];
    let mut c = SoundCheck::new();
    c.event(Event::Header(header())).unwrap();
    let engine = [EngineType::Algo.disk_code()];
    c.event(Event::Record(ReadTag::Known(RecordTag::Engine), &engine))
        .unwrap();
    let p: Vec<u8> = route.repeat(161);
    assert_eq!(
        c.event(Event::Record(ReadTag::Known(RecordTag::Routes), &p)),
        Err(FileError::Bounds)
    );
    let mut c = SoundCheck::new();
    c.event(Event::Header(header())).unwrap();
    c.event(Event::Record(ReadTag::Known(RecordTag::Engine), &engine))
        .unwrap();
    let p: Vec<u8> = route.repeat(160);
    assert_eq!(
        c.event(Event::Record(ReadTag::Known(RecordTag::Routes), &p)),
        Ok(())
    );
}

/// FM3 IN and QM IN round-trip through a Sound file at every choice.
#[test]
fn the_jacks_round_trip_through_a_sound_file() {
    use chimera_core::block::Block;
    use chimera_core::taps::Jack;
    for (id, choices) in [
        (FilterParams::FM3_IN, &Jack::FM3_CHOICES[..]),
        (FilterParams::QM_IN, &Jack::QM_CHOICES[..]),
    ] {
        for (v, &j) in choices.iter().enumerate() {
            let mut s = Sound::plain_init(EngineType::Algo);
            s.params.filter.set(id, v as f32);
            let got = if id == FilterParams::FM3_IN {
                s.params.filter.fm3_src()
            } else {
                s.params.filter.qm_src()
            };
            assert_eq!(got, j);
            let back = decode(&encode(&s)).unwrap();
            assert!(back.bits_eq(&s), "{id:?} {j:?}");
        }
    }
}

/// A jack at its default is left out of a sparse Sound, so a lone route
/// another jack would be inferred from must not move it: QM IN on ENV 3
/// with one LFO 1 → Q route, and FM3 IN on LFO 1 with one ENV 2 → FREQ,
/// reload unchanged from a file and from the pool.
#[test]
fn default_jacks_survive_a_sparse_round_trip() {
    use chimera_core::factory::encode_entry;
    use chimera_core::project::EncodedSound;
    use chimera_core::taps::Jack;
    let mut s = Sound::plain_init(EngineType::Algo);
    s.mod_state
        .set_fixed(ModSource::Lfo1.index(), FixedCol::Q, 40);
    s.mod_state
        .clear_fixed(ModSource::Lfo1.index(), FixedCol::Freq);
    s.mod_state
        .set_fixed(ModSource::Env2.index(), FixedCol::Freq, 30);
    let f = &s.params.filter;
    assert_eq!(f.qm_src(), Jack::Mod(ModSource::Env3));
    assert_eq!(f.fm3_src(), Jack::Mod(ModSource::Lfo1));
    let back = decode(&encode(&s)).unwrap();
    assert!(back.bits_eq(&s), "file");
    let pooled = EncodedSound::new(&encode_entry(&s)).to_sound();
    assert!(pooled.bits_eq(&s), "pool");
    // A jack the inference would reach anyway still reloads as itself.
    s.params.filter.set_qm_src(Jack::Mod(ModSource::Lfo1));
    assert!(decode(&encode(&s)).unwrap().bits_eq(&s), "inferred");
}

/// A Filter block record with only CUTOFF: no jack ids.
fn no_jacks_block() -> Vec<u8> {
    let mut p = vec![
        BlockRef::Filter.disk_code().unwrap(),
        FilterParams::CUTOFF.0,
    ];
    p.extend_from_slice(&800.0f32.to_le_bytes());
    p
}

fn with_routes(routes: &[(ModSource, ParamAddr)], block: Vec<u8>) -> Sound {
    use chimera_core::block::DiskCode;
    let mut r = Vec::new();
    for &(s, a) in routes {
        r.extend_from_slice(&[s.disk_code(), code(a)[0], code(a)[1], 20]);
    }
    decode(&file(&[
        (RecordTag::Engine, vec![EngineType::Algo.disk_code()]),
        (RecordTag::Block, block),
        (RecordTag::Routes, r),
    ]))
    .unwrap()
}

#[test]
fn load_without_jacks_infers_them() {
    use chimera_core::taps::Jack;
    let res = FixedCol::Q.addr();
    let jacks = |s: &Sound| (s.params.filter.fm3_src(), s.params.filter.qm_src());
    let (lfo1, env3) = (Jack::Mod(ModSource::Lfo1), Jack::Mod(ModSource::Env3));
    // One → CUTOFF route that is neither ENV 1 nor NOTE: that is FM3's.
    let s = with_routes(
        &[
            (ModSource::Env1, CUTOFF),
            (ModSource::Lfo2, CUTOFF),
            (ModSource::Note, CUTOFF),
        ],
        no_jacks_block(),
    );
    assert_eq!(jacks(&s), (Jack::Mod(ModSource::Lfo2), env3));
    // Two such: ambiguous, so the default.
    let s = with_routes(
        &[(ModSource::Lfo2, CUTOFF), (ModSource::Vel, CUTOFF)],
        no_jacks_block(),
    );
    assert_eq!(jacks(&s), (lfo1, env3));
    // ENV 1 alone is FM1's, never FM3's.
    let s = with_routes(&[(ModSource::Env1, CUTOFF)], no_jacks_block());
    assert_eq!(jacks(&s), (lfo1, env3));
    // The single → RES route is QM's.
    let s = with_routes(&[(ModSource::Env2, res)], no_jacks_block());
    assert_eq!(jacks(&s), (lfo1, Jack::Mod(ModSource::Env2)));
    let s = with_routes(
        &[(ModSource::Env2, res), (ModSource::Lfo3, res)],
        no_jacks_block(),
    );
    assert_eq!(jacks(&s), (lfo1, env3));
    // Stored jacks win over any inference.
    let mut stored = no_jacks_block();
    stored.push(FilterParams::FM3_IN.0);
    stored.extend_from_slice(&5u32.to_le_bytes()); // LFO 3's code
    stored.push(FilterParams::QM_IN.0);
    stored.extend_from_slice(&6u32.to_le_bytes()); // VEL's code
    let s = with_routes(&[(ModSource::Lfo2, CUTOFF), (ModSource::Env2, res)], stored);
    assert_eq!(
        jacks(&s),
        (Jack::Mod(ModSource::Lfo3), Jack::Mod(ModSource::Vel))
    );
}
