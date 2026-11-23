// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ADR 0123: a project written before the sends moved to the Sound carries
//! them in each Part's mix (PART ids 5-7); they land in that Part's Sound.

mod common;

use std::path::PathBuf;

use chimera_core::addr::BlockRef;
use chimera_core::project::{Project, ProjectStatus, project_status};
use chimera_core::storage::RecordTag;
use common::codec_util::{decode_project_lossy, encode_project, fix_crc, record_offsets};

/// Frozen by `codec_compat_test`'s `sends_fixture_is_frozen`.
const SENDS_FIXTURE: &str = "sends.prj";

/// The file's header, then each record's `(tag, payload)`.
type Records = (Vec<u8>, Vec<(u16, Vec<u8>)>);

fn fixture() -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/v1")
        .join(SENDS_FIXTURE);
    std::fs::read(&p).unwrap_or_else(|e| panic!("{SENDS_FIXTURE}: {e}"))
}

fn split(f: &[u8]) -> Records {
    let o = record_offsets(f);
    let recs = o
        .windows(2)
        .map(|w| {
            let tag = u16::from_le_bytes([f[w[0]], f[w[0] + 1]]);
            (tag, f[w[0] + 4..w[1]].to_vec())
        })
        .collect();
    (f[..28].to_vec(), recs)
}

fn join((head, recs): &Records) -> Vec<u8> {
    let mut f = head.clone();
    for (t, p) in recs {
        f.extend_from_slice(&t.to_le_bytes());
        f.extend_from_slice(&(p.len() as u16).to_le_bytes());
        f.extend_from_slice(p);
    }
    f.extend_from_slice(&[0; 4]);
    fix_crc(&mut f);
    f
}

fn tag(t: RecordTag) -> u16 {
    t.code()
}

fn code(b: BlockRef) -> u8 {
    b.disk_code().unwrap()
}

/// The `[start, end)` of Part `n`'s records.
fn part_range(recs: &[(u16, Vec<u8>)], n: u8) -> (usize, usize) {
    let start = recs
        .iter()
        .position(|(t, p)| *t == tag(RecordTag::Part) && p[0] == n)
        .unwrap();
    let end = recs[start + 1..]
        .iter()
        .position(|(t, _)| *t == tag(RecordTag::Part) || *t == tag(RecordTag::Orbit))
        .map_or(recs.len(), |i| start + 1 + i);
    (start, end)
}

fn load(bytes: &[u8]) -> (Box<Project>, bool, chimera_core::project::TemplateCrc) {
    let (mut p, t) = Project::boxed();
    let lossy = decode_project_lossy(bytes, &mut p).unwrap();
    (p, lossy, t)
}

fn sends(p: &Project, n: usize) -> [f32; 3] {
    p.perf().parts()[n].sound.params.out.sends.map(|s| s.get())
}

fn entry(id: u8, v: f32) -> [u8; 5] {
    let b = v.to_le_bytes();
    [id, b[0], b[1], b[2], b[3]]
}

/// Part 0's PART block payload with ids 5-7 set to `v`.
fn set_old_sends(r: &mut Records, v: [f32; 3]) {
    let (s, e) = part_range(&r.1, 0);
    let blk = r.1[s..e]
        .iter_mut()
        .find(|(t, p)| *t == tag(RecordTag::Block) && p[0] == code(BlockRef::Part))
        .unwrap();
    let mut out = vec![code(BlockRef::Part)];
    for e in blk.1[1..].chunks(5).filter(|e| !(5..=7).contains(&e[0])) {
        out.extend_from_slice(e);
    }
    for (i, x) in v.into_iter().enumerate() {
        out.extend_from_slice(&entry(5 + i as u8, x));
    }
    blk.1 = out;
}

#[test]
fn old_sends_land_in_each_parts_sound() {
    let (p, lossy, t) = load(&fixture());
    assert_eq!(sends(&p, 0), [0.25, 0.5, 0.75]);
    assert_eq!(sends(&p, 1), [0.5, 0.0, 0.0]);
    for n in 2..6 {
        assert_eq!(sends(&p, n), [0.0; 3], "Part {n}");
    }
    assert!(lossy);
    assert_eq!(project_status(&p, t), ProjectStatus::Modified);
}

#[test]
fn a_sound_with_sends_keeps_them() {
    let mut r = split(&fixture());
    let (s, e) = part_range(&r.1, 0);
    let out = code(BlockRef::Out);
    let drop = r.1[s..e]
        .iter()
        .position(|(t, p)| *t == tag(RecordTag::Block) && p[0] == out);
    if let Some(i) = drop {
        r.1.remove(s + i);
    }
    let at = r.1[s..]
        .iter()
        .position(|(t, _)| *t == tag(RecordTag::Engine))
        .unwrap()
        + s
        + 1;
    let mut payload = vec![out];
    payload.extend_from_slice(&entry(4, 0.1));
    r.1.insert(at, (tag(RecordTag::Block), payload));
    let (p, _, _) = load(&join(&r));
    assert_eq!(sends(&p, 0), [0.1, 0.0, 0.0]);
}

#[test]
fn part_before_engine_still_lands() {
    let mut r = split(&fixture());
    let (s, e) = part_range(&r.1, 0);
    let i = r.1[s..e]
        .iter()
        .position(|(t, p)| *t == tag(RecordTag::Block) && p[0] == code(BlockRef::Part))
        .unwrap()
        + s;
    let rec = r.1.remove(i);
    r.1.insert(s + 1, rec);
    let (p, _, _) = load(&join(&r));
    assert_eq!(sends(&p, 0), [0.25, 0.5, 0.75]);
}

#[test]
fn bad_old_sends_are_safe() {
    let mut r = split(&fixture());
    set_old_sends(&mut r, [f32::NAN, f32::INFINITY, 3.0]);
    let (p, _, _) = load(&join(&r));
    assert_eq!(sends(&p, 0), [0.0, 0.0, 1.0]);
}

#[test]
fn a_new_project_is_not_lossy() {
    let (p, _) = Project::boxed();
    let (_, lossy, _) = load(&encode_project(&p));
    assert!(!lossy);
}
