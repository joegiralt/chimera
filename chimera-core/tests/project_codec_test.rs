//! The project codec (projects spec § Format): slots, Parts and FX as card
//! records, round-tripped bit for bit; the structure rules; the CRC.

mod common;

use chimera_core::block::DiskCode;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::name::{ProjectName, SoundName};
use chimera_core::params::EngineType;
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::project::{Origin, PartFrom, PartId, PartSource, Project, SlotId, project_crc};
use chimera_core::storage::{FileError, RecordTag};
use common::codec_util::{decode as decode_sound, fix_crc};
use common::project::{decode, encode, full, load, same};

const SLOT: u16 = 0x8007;
const PART: u16 = 0x8008;
const FX: u16 = 0x8009;
const ORIGIN: u16 = 0x000A;
const KIT: u16 = 0x0024;
const KIT_SOUND: u16 = 0x0025;
/// A Part's last records: its Origin, its Kit, then a `KitSound` a pad.
const ORIGIN_FROM_END: usize = 2 + 12;
const BLOCK: u16 = 0x0001;
const ENGINE: u16 = 0x8002;
const PART_BLOCK: u8 = 25;

/// A file's header and its records, `(tag, payload)`.
fn records(f: &[u8]) -> (Vec<u8>, Vec<(u16, Vec<u8>)>) {
    let end = f.len() - 4;
    let mut v = vec![];
    let mut p = 28;
    while p < end {
        let tag = u16::from_le_bytes([f[p], f[p + 1]]);
        let len = usize::from(u16::from_le_bytes([f[p + 2], f[p + 3]]));
        v.push((tag, f[p + 4..p + 4 + len].to_vec()));
        p += 4 + len;
    }
    (f[..28].to_vec(), v)
}

/// The header, the records and a fresh CRC.
fn rebuild(header: &[u8], recs: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut f = header.to_vec();
    for (tag, p) in recs {
        f.extend(tag.to_le_bytes());
        f.extend((p.len() as u16).to_le_bytes());
        f.extend(p);
    }
    f.extend([0; 4]);
    fix_crc(&mut f);
    f
}

/// `encode(p)` with `change` applied to its records.
fn edited(p: &Project, change: impl FnOnce(&mut Vec<(u16, Vec<u8>)>)) -> Vec<u8> {
    let (h, mut recs) = records(&encode(p));
    change(&mut recs);
    rebuild(&h, &recs)
}

fn is_context(tag: u16) -> bool {
    matches!(tag, SLOT | PART | FX)
}

/// The record range of the context `tag` with index `n` (`FX` ignores it).
fn context(recs: &[(u16, Vec<u8>)], tag: u16, n: u8) -> std::ops::Range<usize> {
    let start = recs
        .iter()
        .position(|(t, p)| *t == tag && (tag == FX || p[0] == n))
        .expect("the context");
    let end = recs[start + 1..]
        .iter()
        .position(|(t, _)| is_context(*t))
        .map_or(recs.len(), |i| start + 1 + i);
    start..end
}

fn part_ctx(recs: &[(u16, Vec<u8>)], part: PartId) -> std::ops::Range<usize> {
    context(recs, PART, part.index() as u8)
}

fn new_project() -> Box<Project> {
    Project::boxed().0
}

fn decode_err(f: &[u8]) -> FileError {
    decode(f, &mut new_project()).unwrap_err()
}

fn with_origin(part: PartId, payload: &[u8]) -> Vec<u8> {
    edited(&new_project(), |recs| {
        let r = part_ctx(recs, part);
        let at = recs[r.clone()]
            .iter()
            .position(|(t, _)| *t == ORIGIN)
            .unwrap();
        recs[r.start + at].1 = payload.to_vec();
    })
}

/// A record inserted after the Part's Engine record.
fn with_record_in_part(p: &Project, part: PartId, tag: u16, payload: &[u8]) -> Vec<u8> {
    edited(p, |recs| {
        let r = part_ctx(recs, part);
        let engine = recs[r.clone()]
            .iter()
            .position(|(t, _)| *t == ENGINE)
            .unwrap();
        recs.insert(r.start + engine + 1, (tag, payload.to_vec()));
    })
}

fn without_mix(p: &Project, part: PartId) -> Vec<u8> {
    edited(p, |recs| {
        let r = part_ctx(recs, part);
        let at = recs[r.clone()]
            .iter()
            .position(|(t, b)| *t == BLOCK && b[0] == PART_BLOCK)
            .unwrap();
        recs.remove(r.start + at);
    })
}

fn without_fx(p: &Project) -> Vec<u8> {
    edited(p, |recs| {
        recs.drain(context(recs, FX, 0));
    })
}

fn missing_part(part: PartId) -> Vec<u8> {
    edited(&new_project(), |recs| {
        recs.drain(part_ctx(recs, part));
    })
}

fn duplicate_slot(slot: SlotId) -> Vec<u8> {
    edited(&new_project(), |recs| {
        let r = context(recs, SLOT, slot.index() as u8);
        let copy: Vec<_> = recs[r.clone()].to_vec();
        recs.splice(r.end..r.end, copy);
    })
}

fn slot_index(n: u8) -> Vec<u8> {
    edited(&new_project(), |recs| {
        let r = context(recs, SLOT, 0);
        recs[r.start].1[0] = n;
    })
}

fn part_index(n: u8) -> Vec<u8> {
    edited(&new_project(), |recs| {
        let r = part_ctx(recs, PartId::ALL[0]);
        recs[r.start].1[0] = n;
    })
}

fn slot_named(name: &[u8; 16]) -> Vec<u8> {
    edited(&new_project(), |recs| {
        let r = context(recs, SLOT, 0);
        recs[r.start].1[1..].copy_from_slice(name);
    })
}

/// An FX block before the Fx record.
fn block_before_context() -> Vec<u8> {
    edited(&new_project(), |recs| {
        let r = context(recs, FX, 0);
        let block = recs[r.start + 1].clone();
        recs.insert(0, block);
    })
}

fn nameless_header() -> Vec<u8> {
    let mut f = encode(&new_project());
    f[12..28].fill(0);
    fix_crc(&mut f);
    f
}

/// A Sound file: an Engine record, then `recs`.
fn sound_file_decode_err(recs: &[(RecordTag, Vec<u8>)]) -> FileError {
    let mut f = common::codec_util::encode(&Sound::init(EngineType::Algo));
    let (h, mut all) = records(&f);
    let tail: Vec<_> = recs.iter().map(|(t, p)| (t.code(), p.clone())).collect();
    all.splice(1..1, tail);
    f = rebuild(&h, &all);
    decode_sound(&f).unwrap_err()
}

#[test]
fn new_round_trip() {
    let (p, _) = Project::boxed();
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    same(&p, &q);
}

#[test]
fn full_pool_round_trip() {
    let (p, _) = full();
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    same(&p, &q);
    assert_eq!(encode(&q), encode(&p));
}

#[test]
fn record_order_is_fx_then_slots_then_parts() {
    let (p, _) = full();
    let (_, recs) = records(&encode(&p));
    let contexts: Vec<(u16, u8)> = recs
        .iter()
        .filter(|(t, _)| is_context(*t))
        .map(|(t, b)| (*t, b.first().copied().unwrap_or(0)))
        .collect();
    let mut want = vec![(FX, 0)];
    want.extend((0..128).map(|s| (SLOT, s)));
    want.extend((0..6).map(|n| (PART, n)));
    assert_eq!(contexts, want);
    assert_eq!(recs[0], (FX, vec![]));
    // FX: its five blocks, chorus to comp.
    let fx: Vec<u8> = recs[1..6].iter().map(|(_, b)| b[0]).collect();
    assert_eq!(fx, [20, 21, 22, 23, 24]);
    // A Part: Engine first, its mix, Origin, kit and pad sounds last.
    let r = part_ctx(&recs, PartId::ALL[1]);
    let part = &recs[r];
    assert_eq!(part[1].0, ENGINE);
    let n = part.len() - 12;
    assert_eq!((part[n - 3].0, part[n - 3].1[0]), (BLOCK, PART_BLOCK));
    assert_eq!(part[n - 2].0, ORIGIN);
    assert_eq!(part[n - 1].0, KIT);
    for (i, (t, b)) in part[n..].iter().enumerate() {
        assert_eq!((*t, b[0]), (KIT_SOUND, i as u8));
    }
}

#[test]
fn slot_and_part_names_round_trip() {
    let (mut p, t) = Project::boxed();
    let mut s = factory_sound(0).unwrap();
    s.name = SoundName::new("DUB-042").unwrap();
    p.pool_store(SlotId::ALL[12], &s).unwrap();
    load(
        &mut p,
        t,
        PartSource {
            part: PartId::ALL[1],
            from: PartFrom::Slot(SlotId::ALL[12]),
        },
    )
    .unwrap();
    p.edit_part(PartId::ALL[1]).sound.name = SoundName::new("DUB-043").unwrap();
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    assert_eq!(
        q.pool().get(SlotId::ALL[12]).unwrap().name().as_str(),
        "DUB-042"
    );
    assert_eq!(q.part(PartId::ALL[1]).sound.name.as_str(), "DUB-043");
    assert_eq!(
        q.pool().get(SlotId::ALL[0]).unwrap().name(),
        factory_sound(0).unwrap().name
    );
}

/// Generations are never set from a file: every slot is cleared, then
/// stored, so each moves (a wrapping `u16`, never lowered by a load).
#[test]
fn a_load_moves_every_generation() {
    let (p, _) = full();
    let (mut q, _) = Project::boxed();
    let before: Vec<u16> = SlotId::ALL
        .iter()
        .map(|&s| q.pool().generation(s))
        .collect();
    decode(&encode(&p), &mut q).unwrap();
    for (s, g) in SlotId::ALL.iter().zip(before) {
        assert_ne!(q.pool().generation(*s), g, "slot {}", s.index());
        let filled = u16::from(q.pool().get(*s).is_some());
        assert_eq!(q.pool().generation(*s), g.wrapping_add(1 + filled));
    }
}

#[test]
fn origins_resolve_against_the_loaded_pool() {
    let (p, _) = full();
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    for id in PartId::ALL {
        match q.part(id).origin() {
            Origin::Slot {
                slot,
                generation,
                crc,
            } => {
                let s = q.pool().get(slot).expect("a filled slot");
                assert_eq!(generation, q.pool().generation(slot));
                assert_eq!(crc, s.crc());
            }
            Origin::Init(e) => assert_eq!(p.part(id).origin(), Origin::Init(e)),
        }
    }
}

#[test]
fn origin_to_empty_slot_reads_as_init() {
    // Part 0's Origin [0, 31], and no Slot(31) record.
    let (mut q, _) = Project::boxed();
    decode(&with_origin(PartId::ALL[0], &[0, 31]), &mut q).unwrap();
    let engine = q.part(PartId::ALL[0]).sound.engine();
    assert_eq!(q.part(PartId::ALL[0]).origin(), Origin::Init(engine));
}

#[test]
fn unknown_or_missing_origins_read_as_init() {
    let (mut p, t) = Project::boxed();
    load(
        &mut p,
        t,
        PartSource {
            part: PartId::ALL[0],
            from: PartFrom::Slot(SlotId::ALL[FACTORY_LEN + 1]),
        },
    )
    .unwrap();
    let modal = Origin::Init(EngineType::Modal);
    let (mut q, _) = Project::boxed();
    // An engine code no firmware has.
    decode(
        &edited(&p, |recs| {
            let r = part_ctx(recs, PartId::ALL[0]);
            recs[r.end - ORIGIN_FROM_END].1 = vec![1, 0xEE];
        }),
        &mut q,
    )
    .unwrap();
    assert_eq!(q.part(PartId::ALL[0]).origin(), modal);
    // No Origin record.
    decode(
        &edited(&p, |recs| {
            let r = part_ctx(recs, PartId::ALL[0]);
            recs.remove(r.end - ORIGIN_FROM_END);
        }),
        &mut q,
    )
    .unwrap();
    assert_eq!(q.part(PartId::ALL[0]).origin(), modal);
    // INIT of an engine other than the Part's is kept as written.
    let code = EngineType::Modal.disk_code();
    decode(&with_origin(PartId::ALL[0], &[1, code]), &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[0]).sound.engine(), EngineType::Algo);
    assert_eq!(q.part(PartId::ALL[0]).origin(), modal);
}

#[test]
fn structure_errors() {
    assert_eq!(
        decode_err(&missing_part(PartId::ALL[5])),
        FileError::Corrupt
    );
    assert_eq!(
        decode_err(&missing_part(PartId::ALL[0])),
        FileError::Corrupt
    );
    assert_eq!(
        decode_err(&duplicate_slot(SlotId::ALL[2])),
        FileError::Corrupt
    );
    assert_eq!(decode_err(&slot_index(128)), FileError::Bounds);
    assert_eq!(decode_err(&part_index(6)), FileError::Bounds);
    assert_eq!(decode_err(&block_before_context()), FileError::Corrupt);
    assert_eq!(decode_err(&nameless_header()), FileError::Corrupt);
    assert_eq!(
        decode_err(&slot_named(b"\x01bad            ")),
        FileError::BadName
    );
    // The Slot record after Engine, so Corrupt is for the Slot itself.
    let mut slot = vec![0u8];
    slot.extend_from_slice(&[0; 16]);
    assert_eq!(
        sound_file_decode_err(&[(RecordTag::Slot, slot)]),
        FileError::Corrupt
    );
    for tag in [RecordTag::Part, RecordTag::Fx, RecordTag::Origin] {
        let payload = if tag == RecordTag::Fx {
            vec![]
        } else {
            vec![0; 2]
        };
        assert_eq!(
            sound_file_decode_err(&[(tag, payload)]),
            FileError::Corrupt,
            "{tag:?}"
        );
    }
}

#[test]
fn more_structure_errors() {
    let p = new_project();
    // A second Fx.
    let twice = edited(&p, |recs| {
        let fx = recs[context(recs, FX, 0)].to_vec();
        recs.splice(0..0, fx);
    });
    assert_eq!(decode_err(&twice), FileError::Corrupt);
    // A Part twice.
    let twice = edited(&p, |recs| {
        let r = part_ctx(recs, PartId::ALL[2]);
        let copy = recs[r.clone()].to_vec();
        recs.splice(r.end..r.end, copy);
    });
    assert_eq!(decode_err(&twice), FileError::Corrupt);
    // A Sound record in FX.
    let engine = edited(&p, |recs| {
        recs.insert(1, (ENGINE, vec![1]));
    });
    assert_eq!(decode_err(&engine), FileError::Corrupt);
    // A voice block in FX.
    let voice = edited(&p, |recs| {
        let r = context(recs, SLOT, 0);
        let b = recs[r.start + 2].clone();
        assert_eq!(b.0, BLOCK);
        recs.insert(1, b);
    });
    assert_eq!(decode_err(&voice), FileError::Corrupt);
    // An FX block twice.
    let again = edited(&p, |recs| {
        let b = recs[1].clone();
        recs.insert(1, b);
    });
    assert_eq!(decode_err(&again), FileError::Corrupt);
    // A Part's mix or Origin twice.
    for what in [BLOCK, ORIGIN] {
        let twice = edited(&p, |recs| {
            let r = part_ctx(recs, PartId::ALL[1]);
            let at = r.start
                + recs[r.clone()]
                    .iter()
                    .position(|(t, b)| *t == what && (what == ORIGIN || b[0] == PART_BLOCK))
                    .unwrap();
            let rec = recs[at].clone();
            recs.insert(at, rec);
        });
        assert_eq!(decode_err(&twice), FileError::Corrupt, "{what:#x}");
    }
    // A slot with no Engine record.
    let bare = edited(&p, |recs| {
        let r = context(recs, SLOT, 3);
        recs.drain(r.start + 1..r.end);
    });
    assert_eq!(decode_err(&bare), FileError::Corrupt);
    // Context payloads of the wrong length; Origin's too.
    let short = edited(&p, |recs| {
        let r = context(recs, SLOT, 0);
        recs[r.start].1.pop();
    });
    assert_eq!(decode_err(&short), FileError::Bounds);
    assert_eq!(
        decode_err(&with_origin(PartId::ALL[0], &[0])),
        FileError::Bounds
    );
    let fx = edited(&p, |recs| recs[0].1.push(0));
    assert_eq!(decode_err(&fx), FileError::Bounds);
    // A Sound file's header kind.
    let mut wrong = encode(&p);
    wrong[6] = 1;
    fix_crc(&mut wrong);
    assert_eq!(decode_err(&wrong), FileError::WrongKind);
}

#[test]
fn compatibility() {
    let (p, _) = full();
    let (mut q, _) = Project::boxed();
    decode(
        &with_record_in_part(&p, PartId::ALL[2], 0x0070, &[1, 2, 3]),
        &mut q,
    )
    .unwrap();
    same(&p, &q); // unknown non-critical: skipped
    assert_eq!(
        decode_err(&with_record_in_part(&p, PartId::ALL[2], 0x8070, &[])),
        FileError::NeedsNewerFirmware
    ); // unknown critical
    // Unknown non-critical before the first context, and in FX: skipped.
    let f = edited(&p, |recs| {
        recs.insert(0, (0x0070, vec![9; 4]));
        recs.insert(2, (0x0071, vec![]));
    });
    decode(&f, &mut q).unwrap();
    same(&p, &q);
    decode(&without_mix(&p, PartId::ALL[3]), &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[3]).mix, PartParams::for_part(3)); // missing record: its base
    decode(&without_fx(&p), &mut q).unwrap();
    assert_eq!(q.perf().fx, FxParams::default());
}

#[test]
fn a_load_replaces_everything_it_holds() {
    let (mut q, _) = full();
    let (p, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    same(&p, &q);
}

#[test]
fn crc_sees_name_pool_fx_and_mix() {
    let (mut p, t) = Project::boxed();
    let base = project_crc(&p);
    assert_eq!(t.get(), base);
    p.set_name(ProjectName::new("ACID PARTY").unwrap());
    assert_ne!(project_crc(&p), base);
    p.set_name(ProjectName::new("NEW PROJECT").unwrap());
    assert_eq!(project_crc(&p), base);
    p.edit_fx().delay.mix = 0.3;
    assert_ne!(project_crc(&p), base);
    p.edit_fx().delay.mix = FxParams::default().delay.mix;
    assert_eq!(project_crc(&p), base);
    p.edit_part(PartId::ALL[0]).mix.pan = 0.5;
    assert_ne!(project_crc(&p), base);
    p.edit_part(PartId::ALL[0]).mix.pan = 0.0;
    assert_eq!(project_crc(&p), base);
    p.pool_store(SlotId::ALL[20], &Sound::init(EngineType::Algo))
        .unwrap();
    assert_ne!(project_crc(&p), base);
    p.pool_clear(SlotId::ALL[20]).unwrap();
    assert_eq!(project_crc(&p), base); // content, not generations
}

#[test]
fn new_loaded_over_anything_is_the_template() {
    let (new, t) = Project::boxed();
    let (mut p, _) = full();
    assert_ne!(project_crc(&p), t.get());
    decode(&encode(&new), &mut p).unwrap();
    assert_eq!(project_crc(&p), t.get());
    same(&p, &new);
}

/// A context's Sound as its records alone decode, through a Sound file
/// (the mix, Origin and ORBIT's records dropped); and those records' bytes.
fn alone(recs: &[(u16, Vec<u8>)], r: std::ops::Range<usize>) -> (Sound, usize) {
    let sound_tag = |t: u16, p: &[u8]| match t {
        BLOCK => p[0] != PART_BLOCK,
        _ => matches!(t, ENGINE | 0x0003 | 0x0004 | 0x0005),
    };
    let body: Vec<_> = recs[r.start + 1..r.end]
        .iter()
        .filter(|(t, p)| sound_tag(*t, p))
        .cloned()
        .collect();
    let (h, _) = records(&common::codec_util::encode(&Sound::init(EngineType::Algo)));
    let mut s = decode_sound(&rebuild(&h, &body)).unwrap();
    s.name = SoundName::from_padded(recs[r.start].1[1..].try_into().unwrap()).unwrap();
    (s, body.iter().map(|(_, p)| 4 + p.len()).sum())
}

/// `s`'s records as `encode_sound` writes them now.
fn sparse(s: &Sound) -> Vec<u8> {
    let f = common::codec_util::encode(s);
    f[28..f.len() - 4].to_vec()
}

/// The v1 fixture, written before sparse Sounds: every Part decodes as its
/// records alone do, and every slot holds its Sound re-encoded sparse.
#[test]
fn v1_project_fixture_loads_same() {
    let f = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/v1/project.prj"
    ))
    .unwrap();
    let (mut q, _) = Project::boxed();
    decode(&f, &mut q).unwrap();
    let (_, recs) = records(&f);
    for id in PartId::ALL {
        let (want, _) = alone(&recs, part_ctx(&recs, id));
        assert!(q.part(id).sound.bits_eq(&want), "Part {}", id.index());
    }
    let mut slots = 0;
    for s in SlotId::ALL {
        let Some(e) = q.pool().get(s) else { continue };
        slots += 1;
        let (want, dense_len) = alone(&recs, context(&recs, SLOT, s.index() as u8));
        assert!(e.to_sound().bits_eq(&want), "slot {}", s.index());
        assert_eq!(e.bytes()[..16], want.name.padded());
        assert_eq!(e.bytes()[16..], sparse(&want), "slot {}", s.index());
        assert!(e.bytes().len() < 16 + dense_len, "slot {}", s.index());
    }
    assert_eq!(slots, 11);
}

/// The last slot round-trips; past it is out of bounds.
#[test]
fn slot_index_127_round_trips() {
    let (mut p, _) = Project::boxed();
    let last = SlotId::ALL[127];
    let mut s = factory_sound(3).unwrap();
    s.name = SoundName::new("LAST").unwrap();
    p.pool_store(last, &s).unwrap();
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    same(&p, &q);
    assert!(q.pool().get(last).unwrap().to_sound().bits_eq(&s));
    // NEW's slot 0 renumbered 127: it lands there.
    let mut q = new_project();
    decode(&slot_index(127), &mut q).unwrap();
    assert!(q.pool().get(SlotId::ALL[0]).is_none());
    let moved = q.pool().get(last).unwrap().to_sound();
    assert!(moved.bits_eq(&factory_sound(0).unwrap()));
    assert_eq!(decode_err(&slot_index(128)), FileError::Bounds);
}

/// A file whose slots, each a worst-case Sound, sum past the arena loads
/// them by index while they fit (ADR 0087), and is lossy: a save would
/// write fewer.
#[test]
fn slots_past_the_arena_load_in_order() {
    use chimera_core::project::arena::ARENA_BYTES;
    use chimera_core::project::test_support::dense;
    use chimera_core::project::{ProjectDecoder, project_file};
    use chimera_core::storage::{Card, ProjectId, Side, load_ab_in_place};
    use chimera_hal::store::{Dir, Store};
    use chimera_hal::testkit::MemStore;

    let (p, _) = full();
    let (h, recs) = records(&encode(&p));
    let first_slot = recs.iter().position(|(t, _)| *t == SLOT).unwrap();
    let first_part = recs.iter().position(|(t, _)| *t == PART).unwrap();
    let mut out: Vec<(u16, Vec<u8>)> = recs[..first_slot].to_vec();
    let mut sum = 0;
    let mut costs = Vec::new();
    for i in 0..128u8 {
        let mut s = factory_sound(usize::from(i) % 8).unwrap();
        dense(&mut s, u64::from(i));
        let (_, sound) = records(&common::codec_util::encode(&s));
        let mut ctx = vec![i];
        ctx.extend_from_slice(&s.name.padded());
        out.push((SLOT, ctx));
        let c = (16 + sound.iter().map(|(_, b)| 4 + b.len()).sum::<usize>()).next_multiple_of(16);
        sum += c;
        costs.push(c);
        out.extend(sound);
    }
    assert!(sum > ARENA_BYTES, "{sum} B of slots");
    out.extend_from_slice(&recs[first_part..]);
    let file = rebuild(&h, &out);

    let id = ProjectId::new(1).unwrap();
    let mut store = MemStore::new(1);
    let vol = store.mount().unwrap();
    store.make_dir(vol, Dir::Chimera).unwrap();
    store.make_dir(vol, Dir::Projects).unwrap();
    store
        .write(vol, project_file(id).side(Side::A), &mut |w| w.put(&file))
        .unwrap();

    let (mut q, _) = full();
    let mut d = ProjectDecoder::new_for_test(&mut q);
    Card::new()
        .run(&mut store, |s, r| {
            load_ab_in_place(s, r, project_file(id), &mut d)
        })
        .unwrap()
        .result
        .unwrap();
    assert!(d.lossy(), "a save would write fewer slots");
    let mut left = ARENA_BYTES;
    for (i, c) in costs.into_iter().enumerate() {
        let fits = c <= left;
        left -= if fits { c } else { 0 };
        assert_eq!(q.pool().get(SlotId::ALL[i]).is_some(), fits, "slot {i}");
    }
    assert!(q.pool().used().bytes() as usize <= ARENA_BYTES);
}

/// A Part's MIDI map rides its Sound record in a project.
#[test]
fn a_part_map_round_trips() {
    let (mut p, _) = full();
    let m = {
        let mut m = chimera_core::midi_map::MidiMap::EMPTY;
        let t = chimera_core::midi_map::Mappable::new(chimera_core::addr::ParamAddr::new(
            chimera_core::addr::BlockRef::Part,
            PartParams::PAN,
        ))
        .unwrap();
        m.learn(chimera_core::midi_map::Input::cc(10).unwrap(), t)
            .unwrap();
        m
    };
    p.edit_part(PartId::ALL[2]).sound.map = m;
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    same(&p, &q);
    assert_eq!(q.part(PartId::ALL[2]).sound.map, m);
}

/// Part `part`'s mix payload with its param `ids` taken out.
fn mix_without(p: &Project, part: PartId, ids: &[u8]) -> Vec<u8> {
    edited(p, |recs| {
        let r = part_ctx(recs, part);
        let at = recs[r.clone()]
            .iter()
            .position(|(t, b)| *t == BLOCK && b[0] == PART_BLOCK)
            .unwrap();
        let b = &mut recs[r.start + at].1;
        let kept: Vec<u8> = b[1..]
            .chunks(5)
            .filter(|e| !ids.contains(&e[0]))
            .flatten()
            .copied()
            .collect();
        b.truncate(1);
        b.extend(kept);
    })
}

#[test]
fn the_eq_round_trips_and_an_old_file_loads_flat() {
    use chimera_core::part_eq::{LoCut, PartEq, Tilt};
    let (p, _) = full();
    let eq = p.part(PartId::ALL[2]).mix.eq;
    assert_eq!(eq, PartEq::new(Tilt::at(-21), LoCut::On), "full() sets it");
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[2]).mix.eq, eq);
    // A file from before the EQ: flat, LO CUT off, the rest of the mix kept.
    let old = mix_without(
        &p,
        PartId::ALL[2],
        &[PartParams::TILT.0, PartParams::LO_CUT.0],
    );
    decode(&old, &mut q).unwrap();
    let m = q.part(PartId::ALL[2]).mix;
    assert_eq!(m.eq, PartEq::FLAT);
    assert_eq!(m.pan, p.part(PartId::ALL[2]).mix.pan);
}

/// The FX context's block `block` with its param `ids` taken out.
fn fx_without(p: &Project, block: u8, ids: &[u8]) -> Vec<u8> {
    edited(p, |recs| {
        let r = context(recs, FX, 0);
        let at = recs[r.clone()]
            .iter()
            .position(|(t, b)| *t == BLOCK && b[0] == block)
            .unwrap();
        let b = &mut recs[r.start + at].1;
        let kept: Vec<u8> = b[1..]
            .chunks(5)
            .filter(|e| !ids.contains(&e[0]))
            .flatten()
            .copied()
            .collect();
        b.truncate(1);
        b.extend(kept);
    })
}

#[test]
fn smart_eq_and_roles_round_trip_and_an_old_file_loads_mid_and_off() {
    use chimera_core::dsp::comp::CompParams;
    use chimera_core::part_eq::smart::{Amount, Role, SmartEq, Switch};
    let (p, _) = full();
    let on = SmartEq::new(Switch::On, Amount::from_code(100).unwrap());
    assert_eq!(
        p.part(PartId::ALL[2]).mix.role,
        Role::Front,
        "full() sets it"
    );
    assert_eq!(p.perf().fx.comp.smart, on, "full() sets it");
    let (mut q, _) = Project::boxed();
    decode(&encode(&p), &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[2]).mix.role, Role::Front);
    assert_eq!(q.part(PartId::ALL[1]).mix.role, Role::Mid);
    assert_eq!(q.perf().fx.comp.smart, on);
    // A file from before SMART EQ: every Part MID, SMART EQ off.
    let old = mix_without(&p, PartId::ALL[2], &[PartParams::ROLE.0]);
    decode(&old, &mut q).unwrap();
    assert_eq!(q.part(PartId::ALL[2]).mix.role, Role::Mid);
    assert_eq!(q.part(PartId::ALL[2]).mix.eq, p.part(PartId::ALL[2]).mix.eq);
    let old = fx_without(&p, 24, &[CompParams::SMART_EQ.0, CompParams::AMOUNT.0]);
    decode(&old, &mut q).unwrap();
    assert_eq!(q.perf().fx.comp.smart, SmartEq::OFF);
    // The reverb return's role is the project's too; an old file's is BACK.
    let (mut r, _) = full();
    r.edit_fx().reverb.role = Role::Front;
    decode(&encode(&r), &mut q).unwrap();
    assert_eq!(q.perf().fx.reverb.role, Role::Front);
    let old = fx_without(&r, 22, &[chimera_core::dsp::reverb::ReverbParams::ROLE.0]);
    decode(&old, &mut q).unwrap();
    assert_eq!(q.perf().fx.reverb.role, Role::Back);
    assert_eq!(q.perf().fx.comp.vol, p.perf().fx.comp.vol);
}

#[test]
fn a_sound_load_leaves_the_part_s_eq() {
    use chimera_core::part_eq::{LoCut, PartEq, Tilt};
    let (mut p, t) = Project::boxed();
    let eq = PartEq::new(Tilt::at(40), LoCut::On);
    p.edit_part(PartId::ALL[0]).mix.eq = eq;
    let src = PartSource {
        part: PartId::ALL[0],
        from: PartFrom::Init(EngineType::Modal),
    };
    load(&mut p, t, src).unwrap();
    assert_eq!(p.part(PartId::ALL[0]).mix.eq, eq);
}
