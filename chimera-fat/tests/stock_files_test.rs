//! Chimera never touches stock PreenFM3 files (#405): on a FAT32 card
//! holding the stock firmware's `/pfm3` tree beside `/CHIMERA`, every
//! write path the firmware has (project saves on both sides, Sound saves,
//! a console put of a TAPE source, the sweep mark, the lost-cluster sweep
//! freeing planted chains), each cut at every block and torn at every
//! block, leaves every stock entry, FAT link and data byte as it was, and
//! `fsck.fat` passes once the boot sweeps have run.
//!
//! The stock layout is the one in ixox/preenfm3's `Storage.h`:
//! `/pfm3/{mix.dfl,seq.dfl,Settings.txt,MidiCtl1.bin}`, `/pfm3/waveform/
//! usr#.{txt,bin}`, `/pfm3/envcurve/usr#.{txt,bin}`, plus the banks,
//! combos, Scala and DX7 files its browsers read.

#[path = "common/ab.rs"]
mod ab;
#[path = "common/console.rs"]
mod console;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tools.rs"]
mod tools;
#[path = "common/tree.rs"]
mod tree;

use ab::{Slot, copy, save_on, slot, sound};
use chimera_core::block::Block as _;
use chimera_core::name::Name;
use chimera_core::part::PartParams;
use chimera_core::project::test_support::save_at;
use chimera_core::project::{
    Confirmed, LibPage, LibraryPart, PageAt, PartId, Project, ReplaceGuard, list_library,
    read_library_sound, save_library_sound,
};
use chimera_core::storage::{
    BuildStamp, Card, FileKind, Generation, Header, boot_sweep, encode_library_sound, write_file,
};
use chimera_fat::volume::FsKind;
use chimera_hal::store::{Store, Sweep};
use console::{Bytes, ask, chunked, crc, lines, source_file_bytes, src_line};
use image::{Cut, PART_LBA, RamDisk, Tear, fat_check, layout_of};
use probe::{marks_for, probed, swept};
use std::rc::Rc;
use tools::{fsck, mkfs};
use tree::{DirAt, Tree};

/// What the stock files hold before Chimera writes: the blocks of their
/// files and directories, their FAT links in both copies, and every entry
/// the root held.
struct Stock {
    blocks: Vec<(u32, [u8; 512])>,
    fat: Vec<(u32, [u32; 2])>,
    root: Vec<(u32, usize, [u8; 32])>,
}

fn fat_values(disk: &RamDisk, c: u32) -> [u32; 2] {
    let l = layout_of(disk);
    let mut v = [0; 2];
    for (i, b) in l.fat_copies(l.fat_block(c)).enumerate() {
        v[i] = l.entry(&disk.block(PART_LBA + b), c);
    }
    v
}

/// `name`.`ext` as a short name, `long` its LFN.
fn n83(name: &str, ext: &str) -> [u8; 11] {
    let mut n = [b' '; 11];
    n[..name.len()].copy_from_slice(name.as_bytes());
    n[8..8 + ext.len()].copy_from_slice(ext.as_bytes());
    n
}

fn lfn(stem: &str, ext: &str) -> String {
    format!("{stem}.{ext}")
}

/// A FAT32 card with the stock tree, and two planted lost chains.
fn stock_card() -> (RamDisk, Stock) {
    let disk = mkfs(FsKind::Fat32, 70_000, 1, 0x5F3_0001);
    let mut t = Tree::new(&disk);
    let mut root = t.root();
    let mut seed = 0u8;
    let mut put = |t: &mut Tree, d: &mut DirAt, stem: &str, ext: &str, len: u32| {
        seed += 1;
        let long = (stem.chars().any(|c| c.is_ascii_lowercase()) || stem.len() > 8)
            .then(|| lfn(stem, ext));
        let short = stem
            .to_ascii_uppercase()
            .chars()
            .take(8)
            .collect::<String>();
        t.file(
            d,
            n83(&short, &ext.to_ascii_uppercase()),
            long.as_deref(),
            (len, 0),
            seed,
            None,
        );
    };
    put(&mut t, &mut root, "preenfm3", "bin", 24_000);
    let mut pfm3 = t.mkdir(&mut root, n83("PFM3", ""), Some("pfm3"));
    put(&mut t, &mut pfm3, "Settings", "txt", 700);
    put(&mut t, &mut pfm3, "mix", "dfl", 1_300);
    put(&mut t, &mut pfm3, "seq", "dfl", 4_096);
    put(&mut t, &mut pfm3, "MidiCtl1", "bin", 600);
    put(&mut t, &mut pfm3, "Default", "bnk", 40_000);
    put(&mut t, &mut pfm3, "Default", "cmb", 9_000);
    for (dir, files) in [
        ("waveform", ["usr1.txt", "usr1.bin", "usr2.bin"]),
        ("envcurve", ["usr1.txt", "usr1.bin", "usr2.txt"]),
        ("scala", ["just_intonation.scl", "19edo.scl", "pelog.scl"]),
        ("dx7", ["rom1a.syx", "rom1b.syx", "brass.syx"]),
    ] {
        let mut d = t.mkdir(&mut pfm3, n83(&dir.to_ascii_uppercase(), ""), Some(dir));
        for f in files {
            let (stem, ext) = f.split_once('.').unwrap();
            put(&mut t, &mut d, stem, ext, 2_048);
        }
    }
    t.lose(3, None);
    t.lose(7, None);
    let l = t.l;
    let mut s = Stock {
        blocks: Vec::new(),
        fat: Vec::new(),
        root: Vec::new(),
    };
    let chains = t
        .files
        .iter()
        .map(|f| &f.chain)
        .chain(t.dirs.iter().map(|d| &d.chain));
    for c in chains.flatten() {
        let first = PART_LBA + l.cluster_block(*c).unwrap();
        for b in first..first + l.blocks_per_cluster() {
            s.blocks.push((b, disk.block(b)));
        }
        s.fat.push((*c, fat_values(&disk, *c)));
    }
    for c in &root.chain {
        let first = PART_LBA + l.cluster_block(*c).unwrap();
        for b in first..first + l.blocks_per_cluster() {
            let blk = disk.block(b);
            for (i, e) in blk.chunks(32).enumerate() {
                if e[0] != 0 {
                    s.root.push((b, i * 32, e.try_into().unwrap()));
                }
            }
        }
    }
    assert!(s.root.len() > 4 && s.blocks.len() > 100 && s.fat.len() > 100);
    (disk, s)
}

/// Every stock entry, link and byte as it was.
fn assert_stock(what: &str, disk: &RamDisk, s: &Stock) {
    for (b, want) in &s.blocks {
        assert!(disk.block(*b) == *want, "{what}: stock data block {b}");
    }
    for (c, want) in &s.fat {
        assert_eq!(fat_values(disk, *c), *want, "{what}: stock FAT link {c}");
    }
    for (b, at, want) in &s.root {
        assert!(
            disk.block(*b)[*at..*at + 32] == want[..],
            "{what}: stock root entry at block {b}+{at}"
        );
    }
}

fn project(n: u32) -> Box<Project> {
    let mut p = Project::boxed().0;
    p.edit_part(PartId::ALL[0])
        .mix
        .set(PartParams::LEVEL, 0.1 * n as f32);
    p
}

type Op = (&'static str, fn(&Slot));

/// Each op on a fresh store, as after a power cycle; a failed (cut) op is
/// fine, only the card's state is judged.
const OPS: [Op; 9] = [
    ("project save A", |s| {
        let _ = save_at(&mut Card::new(), &mut probed(s), &mut project(1), 1);
    }),
    ("project save B", |s| {
        let _ = save_at(&mut Card::new(), &mut probed(s), &mut project(2), 1);
    }),
    ("project save A again", |s| {
        let _ = save_at(&mut Card::new(), &mut probed(s), &mut project(3), 1);
    }),
    ("sound save", |s| {
        let _ = save_on(&mut probed(s), 1, &|_| Ok(()));
    }),
    ("sound save, other side", |s| {
        let _ = save_on(&mut probed(s), 2, &|_| Ok(()));
    }),
    ("console put of a TAPE source", |s| {
        let (h, file) = source_file_bytes(7, 4_000);
        let _ = ask(&mut probed(s), &src_line(&h, &file), &chunked(&file));
    }),
    ("sweep mark request", |s| {
        let mut st = probed(s);
        if let Ok(vol) = st.mount() {
            let _ = st.request_sweep(vol);
        }
    }),
    ("boot sweep, freeing", |s| {
        let _ = boot_sweep(&mut probed(s), Sweep::Free, &mut marks_for(&**s));
    }),
    ("boot sweep again", |s| {
        let _ = boot_sweep(&mut probed(s), Sweep::Free, &mut marks_for(&**s));
    }),
];

/// The boots after a cut: sweeps until one finds nothing, then fsck.
fn recover(what: &str, cut: &Slot, stock: &Stock, check_fsck: bool) {
    let disk = Rc::new(copy(&cut.inner));
    for boot in 0.. {
        assert!(boot < 4, "{what}: sweeps still finding");
        let s = swept(&disk);
        if s.freed == 0 && !s.healed {
            break;
        }
    }
    assert_stock(&format!("{what}, after the boot sweeps"), &disk, stock);
    if check_fsck {
        let f = fsck(&disk);
        assert_eq!(f.code, 0, "{what}: fsck {}", f.out);
        assert_eq!(f.reclaimed, 0, "{what}: fsck found lost clusters");
    }
}

/// Every op in `ops` cut and torn at every block, in order on one card:
/// the stock files stay as they were, the card ends fsck-clean.
fn cut_every_op(card: RamDisk, stock: &Stock, ops: &[Op]) -> RamDisk {
    let f = fsck(&card);
    assert!(f.findings().is_empty(), "stock card before: {}", f.out);
    let mut main = slot(card);
    let mut cuts = 0;
    for &(name, op) in ops {
        let writes = {
            let dry = slot(copy(&main.inner));
            op(&dry);
            dry.writes.get()
        };
        // The second sweep finds the mark clean and writes nothing; reads
        // write nothing.
        assert!(
            writes > 0
                || name == "boot sweep again"
                || name.starts_with("read")
                || name.starts_with("list"),
            "{name}: no writes"
        );
        for k in 0..writes {
            for (kind, cut) in [
                ("cut", Cut::After(k)),
                ("torn", Cut::TornAfter(k, Tear::HalfOld)),
            ] {
                let what = format!("{name}: {kind} at write {k} of {writes}");
                let c = slot(copy(&main.inner));
                c.cut.set(cut);
                op(&c);
                c.cut.set(Cut::Never);
                assert_stock(&what, &c.inner, stock);
                recover(&what, &c, stock, cuts % 9 == 0);
                cuts += 1;
            }
        }
        op(&main);
        let what = format!("{name}: whole");
        assert_stock(&what, &main.inner, stock);
        main = slot(copy(&main.inner));
    }
    // The ops end with the sweeps: the card is whole and fsck-clean.
    let f = fsck(&main.inner);
    assert_eq!(f.code, 0, "after every op: {}", f.out);
    assert_eq!(f.reclaimed, 0, "the sweep freed the planted chains");
    assert_stock("after every op", &main.inner, stock);
    eprintln!("{cuts} cuts, every stock byte kept");
    copy(&main.inner)
}

#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn chimera_leaves_stock_files_bit_identical() {
    let (card, stock) = stock_card();
    cut_every_op(card, &stock, &OPS);
}

const STAMP: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);

/// Factory Sound `n` as a library file body.
fn sound_file() -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(Name::new("LIB").unwrap()),
    };
    let mut out = Bytes(Vec::new());
    write_file(&mut out, &h, &mut |w| {
        encode_library_sound(&sound(3), &STAMP, w)
    })
    .unwrap();
    out.0
}

/// Whether `s` is an uncut run: its results are then judged.
fn whole(s: &Slot) -> bool {
    matches!(s.cut.get(), Cut::Never)
}

/// Both pages of the library listed (the newest 48, then the page below
/// its last row), a row of each read, and the first adopted.
fn list_and_read(s: &Slot, adopt: bool) {
    let mut st = probed(s);
    let mut card = Card::new();
    let mut newest = Box::new(LibPage::EMPTY);
    list_library(&mut card, &mut st, PageAt::Newest, &mut newest)
        .out
        .unwrap();
    let rows: Vec<_> = newest.entries().iter().flatten().collect();
    assert_eq!(rows.len(), 48, "a full first page");
    assert!(newest.older, "an older page");
    let mut older = Box::new(LibPage::EMPTY);
    let below = PageAt::Below(rows[47].id);
    list_library(&mut card, &mut st, below, &mut older)
        .out
        .unwrap();
    assert!(older.entries().iter().flatten().count() >= 2, "page 2");
    assert!(older.newer, "a newer page");
    for page in [&newest, &older] {
        let e = page.entries().iter().flatten().next().unwrap();
        let file = page.file(e).unwrap();
        let (mut p, template) = Project::boxed();
        read_library_sound(&mut card, &mut st, &mut p, file)
            .out
            .unwrap();
        if adopt {
            let part = PartId::ALL[0];
            let c: Confirmed<LibraryPart> =
                ReplaceGuard::check(&p, template, LibraryPart { part, file }).unwrap();
            p.adopt(c, None).unwrap();
        }
    }
}

/// Sounds saved before the cut loop: past one page of 48.
const SEEDED: u32 = 50;

fn seed_library(card: RamDisk) -> RamDisk {
    let seed = slot(card);
    for n in 0..SEEDED {
        let snd = sound(n);
        save_library_sound(&mut Card::new(), &mut probed(&seed), &snd, snd.name, &STAMP)
            .out
            .unwrap();
    }
    copy(&seed.inner)
}

fn save_op(s: &Slot, n: u32) {
    let snd = sound(n);
    let r = save_library_sound(&mut Card::new(), &mut probed(s), &snd, snd.name, &STAMP).out;
    if whole(s) {
        r.unwrap();
    }
}

fn ask_op(s: &Slot, line: &str, data: &[u8], want: &str) {
    let said = ask(&mut probed(s), line, data);
    if whole(s) {
        assert_eq!(said, want, "{line}");
    }
}

const LIB_OPS: [Op; 8] = [
    ("library save", |s| save_op(s, 1)),
    ("library save again", |s| save_op(s, 2)),
    ("console put of a Sound", |s| {
        let b = sound_file();
        let line = format!(
            "put /CHIMERA/SOUNDS/S0000100.A {} {:08x}\n",
            b.len(),
            crc(&b)
        );
        ask_op(s, &line, &lines(&b), "READY\nOK\n");
    }),
    ("list every page", |s| {
        let said = ask(&mut probed(s), "ls sounds\n", b"");
        assert!(said.ends_with("OK\n"), "{said}");
        list_and_read(s, false);
    }),
    ("read both pages", |s| list_and_read(s, false)),
    ("read and adopt", |s| list_and_read(s, true)),
    ("console rm of a Sound", |s| {
        ask_op(s, "rm /CHIMERA/SOUNDS/S0000100\n", b"", "OK\n");
    }),
    ("sweep after the library", |s| {
        let _ = boot_sweep(&mut probed(s), Sweep::Free, &mut marks_for(&**s));
    }),
];

/// Library saves, listings, loads and console puts and rms, cut and torn
/// at every block, write only under `/CHIMERA/SOUNDS`: the stock tree is
/// as it was, the root gains only `/CHIMERA`, `/CHIMERA` holds only
/// `SOUNDS`, and `SOUNDS` only `S#######.A|B` files.
#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn library_paths_never_touch_stock_files() {
    let (card, stock) = stock_card();
    let mut want = root_names(&card, &stock);
    want.push(*b"CHIMERA    ");
    let end = cut_every_op(seed_library(card), &stock, &LIB_OPS);
    let (mut got, mut want) = (root_names(&end, &stock), want);
    got.sort();
    want.sort();
    assert_eq!(got, want, "the root gained only /CHIMERA");
    assert_inside_sounds(&end);
}

/// `/CHIMERA` holds `SOUNDS` (and the sweep's mark) and nothing else, and
/// `SOUNDS` only library files, as `fat_check` finds them by its own parse.
fn assert_inside_sounds(disk: &RamDisk) {
    let r = fat_check(disk);
    let sounds = r.entry(b"SOUNDS     ").expect("SOUNDS made");
    let mut files = 0;
    for e in &r.entries {
        if e.name == sounds.name {
            continue;
        }
        let n = e.name;
        if n == *b"SWEEP   MRK" {
            continue; // the boot sweep's own mark, by design
        }
        assert!(
            n[0] == b'S'
                && n[1..8].iter().all(u8::is_ascii_digit)
                && matches!(n[8], b'A' | b'B')
                && n[9..] == *b"  ",
            "written outside /CHIMERA/SOUNDS: {:?}",
            String::from_utf8_lossy(&n)
        );
        files += 1;
    }
    assert!(files > 48, "{files} library files");
}

/// The short names of every live root entry, in the root blocks the stock
/// card used.
fn root_names(disk: &RamDisk, stock: &Stock) -> Vec<[u8; 11]> {
    let mut blocks: Vec<u32> = stock.root.iter().map(|r| r.0).collect();
    blocks.dedup();
    blocks
        .into_iter()
        .flat_map(|b| {
            disk.block(b)
                .chunks(32)
                .map(|e| <[u8; 32]>::try_from(e).unwrap())
                .collect::<Vec<_>>()
        })
        .filter(|e| e[0] != 0 && e[0] != 0xE5 && e[11] != 0x0F)
        .map(|e| e[..11].try_into().unwrap())
        .collect()
}
