// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Long names read off their LFN runs (ADR 0259): a file copied with a
//! long name lists with it, upper case, `?` past ASCII, its first 32
//! characters. A run whose order or checksum is off, or one cut off from
//! its entry, is no name; a Mac's `._name` is never listed.

#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tree.rs"]
mod tree;

use chimera_fat::dir::{self, Long, Slot};
use chimera_fat::volume::FsKind;
use chimera_hal::store::{Dir, Store};
use image::with_clusters;
use probe::probed;
use std::ops::ControlFlow;
use std::rc::Rc;
use tree::{Tree, lfn_run};

const FILE: u8 = 0x20;

/// `long`'s run then its entry, through `Long`: what it reads.
fn decode(short: &[u8; 11], run: &[[u8; 32]]) -> Vec<u8> {
    let mut l = Long::new();
    for r in run {
        l.see(r);
    }
    let Slot::File(e) = dir::parse(&tree::raw(*short, FILE, 0, 0), FsKind::Fat32) else {
        panic!("not a file");
    };
    l.take(&e.name).to_vec()
}

#[test]
fn a_run_reads_upper_case_across_its_slots() {
    let short = *b"BOHLEN~1SCL";
    let run = lfn_run(&short, "bohlen-pierce.scl");
    assert_eq!(run.len(), 2, "two slots: 17 characters");
    assert_eq!(decode(&short, &run), b"BOHLEN-PIERCE.SCL");
}

#[test]
fn a_long_run_keeps_its_first_32_and_marks_non_ascii() {
    let short = *b"GAMELA~1SCL";
    let long = "gamelan slendro über alles, ten-tone version.scl";
    let run = lfn_run(&short, long);
    assert_eq!(run.len(), 4);
    let got = decode(&short, &run);
    assert_eq!(got, b"GAMELAN SLENDRO ?BER ALLES, TEN-");
}

#[test]
fn a_run_that_isnt_whole_or_isnt_its_entrys_is_no_name() {
    let short = *b"BOHLEN~1SCL";
    let run = lfn_run(&short, "bohlen-pierce.scl");
    // Missing its first slot (an orphan of a deleted run).
    assert_eq!(decode(&short, &run[1..]), b"");
    // Its last slot missing: the entry comes too soon.
    assert_eq!(decode(&short, &run[..1]), b"");
    // Another entry's checksum.
    assert_eq!(decode(b"OTHER   SCL", &run), b"");
    // Out of order.
    assert_eq!(decode(&short, &[run[1], run[0]]), b"");
    // A slot of another run in between.
    let other = lfn_run(b"OTHER   SCL", "some other long name.scl");
    assert_eq!(decode(&short, &[run[0], other[1], run[1]]), b"");
}

fn listing(entries: &[([u8; 11], u8, Option<&str>)]) -> Vec<(String, String)> {
    let disk = Rc::new(with_clusters(4_200, 0x7E57));
    {
        let mut t = Tree::new(&disk);
        let mut root = t.root();
        let mut chimera = t.mkdir(&mut root, *b"CHIMERA    ", None);
        let mut d = t.mkdir(&mut chimera, *b"TUNINGS    ", None);
        for &(short, attr, long) in entries {
            t.put_named(&mut d, tree::raw(short, attr, 0, 0), long);
        }
    }
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    let mut got = Vec::new();
    s.list_long(vol, Dir::Tunings, &mut |f, _, l| {
        got.push((
            String::from_utf8(f.stem().to_vec()).unwrap(),
            String::from_utf8(l.to_vec()).unwrap(),
        ));
        ControlFlow::Continue(())
    })
    .unwrap();
    got
}

#[test]
fn a_folder_lists_each_file_with_its_long_name() {
    let got = listing(&[
        (*b"BOHLEN~1SCL", FILE, Some("bohlen-pierce.scl")),
        (*b"BOHLEN~1KBM", FILE, Some("bohlen-pierce.kbm")),
        (*b"PELOG   SCL", FILE, None),
        // A Mac's AppleDouble twin: refused by its short name and by its
        // long one.
        (*b"_BOHLE~1SCL", FILE, Some("._bohlen-pierce.scl")),
        (*b"BOHLEN~2SCL", FILE, Some(".bohlen-pierce.scl")),
    ]);
    assert_eq!(
        got,
        [
            ("BOHLEN~1".into(), "BOHLEN-PIERCE.SCL".into()),
            ("BOHLEN~1".into(), "BOHLEN-PIERCE.KBM".into()),
            ("PELOG".into(), String::new()),
        ]
    );
}

#[test]
fn an_orphaned_run_names_nothing_after_it() {
    let disk_entries = [(*b"PELOG   SCL", FILE, None)];
    // The orphan: a run for a name whose entry was deleted, right before
    // PELOG's entry.
    let disk = Rc::new(with_clusters(4_200, 0x7E57));
    {
        let mut t = Tree::new(&disk);
        let mut root = t.root();
        let mut chimera = t.mkdir(&mut root, *b"CHIMERA    ", None);
        let mut d = t.mkdir(&mut chimera, *b"TUNINGS    ", None);
        for r in lfn_run(b"GONE    SCL", "a gone file.scl") {
            t.put(&mut d, r);
        }
        for (short, attr, long) in disk_entries {
            t.put_named(&mut d, tree::raw(short, attr, 0, 0), long);
        }
    }
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    let mut got = Vec::new();
    s.list_long(vol, Dir::Tunings, &mut |f, _, l| {
        got.push((f.stem().to_vec(), l.to_vec()));
        ControlFlow::Continue(())
    })
    .unwrap();
    assert_eq!(got, [(b"PELOG".to_vec(), Vec::new())]);
}

/// A whole Scala archive copied in (5,000 long-named files, a Mac's twin
/// beside each: 40,000 entries, about 2,500 blocks): a TUNING detent's
/// folder read stops after the first `MAX_FILES`, a bounded number of
/// blocks (ADR 0259).
#[test]
fn a_detent_reads_a_bounded_part_of_a_huge_folder() {
    use chimera_core::ui::tuning_walk::{MAX_FILES, scan_scales};
    let disk = Rc::new(with_clusters(8_000, 0x7E57));
    {
        let mut t = Tree::new(&disk);
        let mut root = t.root();
        let mut chimera = t.mkdir(&mut root, *b"CHIMERA    ", None);
        let mut d = t.mkdir(&mut chimera, *b"TUNINGS    ", None);
        for i in 0..5_000u32 {
            let stem = format!("SC{i:04}~1");
            let mut short = *b"        SCL";
            short[..8].copy_from_slice(stem.as_bytes());
            let long = format!("scale number {i:04} of the archive.scl");
            t.put_named(&mut d, tree::raw(short, FILE, 0, 0), Some(&long));
            let mut twin = *b"_SC~    SCL";
            twin[4..8].copy_from_slice(format!("{:04}", i % 10_000).as_bytes());
            t.put_named(
                &mut d,
                tree::raw(twin, 0x22, 0, 0),
                Some(&format!("._{long}")),
            );
        }
    }
    let mut s = probed(&disk);
    let vol = s.mount().unwrap();
    let before = probe::log(&s).reads.get();
    let mut all = 0;
    s.list_long(vol, Dir::Tunings, &mut |_, _, _| {
        all += 1;
        ControlFlow::Continue(())
    })
    .unwrap();
    let whole = probe::log(&s).reads.get() - before;
    assert_eq!(all, 5_000);
    let before = probe::log(&s).reads.get();
    let t = std::time::Instant::now();
    let mut n = 0;
    let more = scan_scales(&mut s, vol, &mut |_, _| n += 1).unwrap();
    let us = t.elapsed().as_micros();
    let detent = probe::log(&s).reads.get() - before;
    eprintln!(
        "5,000 files: whole folder {whole} blocks; a detent {detent} blocks, {us} us on the host"
    );
    assert_eq!((n, more), (MAX_FILES, true));
    assert!(detent <= 100, "a folder read {detent} blocks");
    assert!(whole > 1_000);
}
