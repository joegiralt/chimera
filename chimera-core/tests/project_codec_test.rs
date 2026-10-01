//! The project codec (projects spec § Format): slots, Parts and FX as card
//! records, round-tripped bit for bit; the structure rules; the CRC.

mod common;

use chimera_core::block::DiskCode;
use chimera_core::dsp::fx_bus::FxParams;
use chimera_core::factory::factory_sound;
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
    want.extend((0..32).map(|s| (SLOT, s)));
    want.extend((0..6).map(|n| (PART, n)));
    assert_eq!(contexts, want);
    assert_eq!(recs[0], (FX, vec![]));
    // FX: its five blocks, chorus to comp.
    let fx: Vec<u8> = recs[1..6].iter().map(|(_, b)| b[0]).collect();
    assert_eq!(fx, [20, 21, 22, 23, 24]);
    // A Part: Engine first, its mix and Origin last.
    let r = part_ctx(&recs, PartId::ALL[1]);
    let part = &recs[r];
    assert_eq!(part[1].0, ENGINE);
    let n = part.len();
    assert_eq!((part[n - 2].0, part[n - 2].1[0]), (BLOCK, PART_BLOCK));
    assert_eq!(part[n - 1].0, ORIGIN);
}

#[test]
fn slot_and_part_names_round_trip() {
    let (mut p, t) = Project::boxed();
    let mut s = factory_sound(0).unwrap();
    s.name = SoundName::new("DUB-042").unwrap();
    p.pool_store(SlotId::ALL[12], s);
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
        q.pool().get(SlotId::ALL[12]).unwrap().name.as_str(),
        "DUB-042"
    );
    assert_eq!(q.part(PartId::ALL[1]).sound.name.as_str(), "DUB-043");
    assert_eq!(
        q.pool().get(SlotId::ALL[0]).unwrap().name,
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
                assert_eq!(crc, chimera_core::storage::sound_crc(s));
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
            from: PartFrom::Slot(SlotId::ALL[9]),
        },
    )
    .unwrap();
    let modal = Origin::Init(EngineType::Modal);
    let (mut q, _) = Project::boxed();
    // An engine code no firmware has.
    decode(
        &edited(&p, |recs| {
            let r = part_ctx(recs, PartId::ALL[0]);
            recs[r.end - 1].1 = vec![1, 0xEE];
        }),
        &mut q,
    )
    .unwrap();
    assert_eq!(q.part(PartId::ALL[0]).origin(), modal);
    // No Origin record.
    decode(
        &edited(&p, |recs| {
            let r = part_ctx(recs, PartId::ALL[0]);
            recs.remove(r.end - 1);
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
    assert_eq!(decode_err(&slot_index(32)), FileError::Bounds);
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
    p.pool_store(SlotId::ALL[20], Sound::init(EngineType::Algo));
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
