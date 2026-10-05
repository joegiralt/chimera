//! A raw tree writer for the sweep's tests: directories, files and lost
//! chains written as raw 32-byte entries and FAT values into both FATs of
//! any FAT16 or FAT32 image (`mkfs.fat`'s or `image`'s). Nothing passes
//! through `chimera_fat`'s writer, so names can be anything a computer
//! leaves: `~ _ $`, spaces, a 0x05 lead byte, high-bit bytes, LFN runs.
// Each test binary uses its own part of this module.
#![allow(dead_code)]

use crate::image::{PART_LBA, RamDisk, layout_of};
use crate::probe::XorShift;
use chimera_fat::volume::{FsKind, Layout, Root};

pub const ENTRY: usize = 32;
const SLOTS: usize = 512 / ENTRY;
const ATTR_DIR: u8 = 0x10;
const ATTR_FILE: u8 = 0x20;
const ATTR_LFN: u8 = 0x0F;
pub const DOT: [u8; 11] = *b".          ";
pub const DOT_DOT: [u8; 11] = *b"..         ";

/// A raw entry.
pub fn raw(name: [u8; 11], attr: u8, start: u32, len: u32) -> [u8; ENTRY] {
    let mut e = [0; ENTRY];
    e[..11].copy_from_slice(&name);
    e[11] = attr;
    e[20..22].copy_from_slice(&((start >> 16) as u16).to_le_bytes());
    e[26..28].copy_from_slice(&(start as u16).to_le_bytes());
    e[28..32].copy_from_slice(&len.to_le_bytes());
    e
}

/// The checksum of a short name that ties its LFN run to it.
pub fn lfn_sum(name: &[u8; 11]) -> u8 {
    name.iter()
        .fold(0u8, |s, &c| s.rotate_right(1).wrapping_add(c))
}

/// The LFN slots for `long`, in the order they sit on the card (the last
/// part first, flagged 0x40), each tied to `short`.
pub fn lfn_run(short: &[u8; 11], long: &str) -> Vec<[u8; ENTRY]> {
    let units: Vec<u16> = long.encode_utf16().collect();
    let parts = units.len().div_ceil(13).max(1);
    let sum = lfn_sum(short);
    let mut run = Vec::new();
    for p in (0..parts).rev() {
        let mut e = [0; ENTRY];
        e[0] = (p + 1) as u8 | if p == parts - 1 { 0x40 } else { 0 };
        e[11] = ATTR_LFN;
        e[13] = sum;
        let at = [1, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30];
        for (i, &off) in at.iter().enumerate() {
            let k = p * 13 + i;
            let u = match k.cmp(&units.len()) {
                core::cmp::Ordering::Less => units[k],
                core::cmp::Ordering::Equal => 0,
                core::cmp::Ordering::Greater => 0xFFFF,
            };
            e[off..off + 2].copy_from_slice(&u.to_le_bytes());
        }
        run.push(e);
    }
    run
}

/// A directory being written: its first cluster (0: FAT16's fixed root),
/// its clusters, and the slots used.
#[derive(Clone, Debug)]
pub struct DirAt {
    pub start: u32,
    pub is_root: bool,
    pub chain: Vec<u32>,
    pub used: usize,
}

/// A file as written: the entry's block and slot, its chain and length.
#[derive(Clone, Debug)]
pub struct FileAt {
    pub chain: Vec<u32>,
    pub len: u32,
}

/// The writer over one image.
pub struct Tree<'a> {
    pub disk: &'a RamDisk,
    pub l: Layout,
    /// Where the next cluster search starts.
    next: u32,
    pub files: Vec<FileAt>,
    pub dirs: Vec<DirAt>,
}

impl<'a> Tree<'a> {
    /// FAT32's FSInfo free count set to unknown: this writer doesn't keep
    /// it.
    pub fn new(disk: &'a RamDisk) -> Self {
        let l = layout_of(disk);
        if let Some(at) = l.fs_info() {
            disk.0.borrow_mut()[(PART_LBA + at) as usize][488..492].fill(0xFF);
        }
        Self {
            disk,
            l,
            next: 2,
            files: Vec::new(),
            dirs: Vec::new(),
        }
    }

    fn block_mut(&self, lba: u32, f: impl FnOnce(&mut [u8; 512])) {
        f(&mut self.disk.0.borrow_mut()[(PART_LBA + lba) as usize]);
    }

    /// `c`'s FAT 1 value.
    pub fn get(&self, c: u32) -> u32 {
        let at = (PART_LBA + self.l.fat_block(c)) as usize;
        self.l.entry(&self.disk.0.borrow()[at], c)
    }

    /// `c`'s value set to `v` in every FAT.
    pub fn set(&self, c: u32, v: u32) {
        for copy in self.l.fat_copies(self.l.fat_block(c)) {
            self.block_mut(copy, |b| self.l.put_entry(b, c, v));
        }
    }

    /// `n` free clusters linked into a chain, its end marked; with `rng`,
    /// scattered. Each search goes on from where the last stopped, and
    /// wraps once.
    pub fn take(&mut self, n: u32, mut rng: Option<&mut XorShift>) -> Vec<u32> {
        let mut chain = Vec::new();
        let end = 2 + self.l.clusters();
        let (mut c, mut wrapped) = (self.next, false);
        while chain.len() < n as usize {
            if let Some(r) = rng.as_deref_mut()
                && r.below(4) == 0
            {
                c += r.below(40) as u32;
            }
            if c >= end {
                // Once round to the front: scattering skips clusters.
                assert!(!wrapped, "the image is full");
                (c, wrapped) = (2, true);
            }
            if self.get(c) == 0 && !(wrapped && chain.contains(&c)) {
                chain.push(c);
            }
            c += 1;
        }
        self.next = c;
        for w in chain.windows(2) {
            self.set(w[0], w[1]);
        }
        if let Some(&last) = chain.last() {
            self.set(last, self.l.end_mark());
        }
        chain
    }

    /// Every block of `chain` filled from `seed`, or zeroed.
    pub fn fill(&self, chain: &[u32], seed: Option<u8>) {
        for &c in chain {
            let first = self.l.cluster_block(c).unwrap();
            for b in first..first + self.l.blocks_per_cluster() {
                self.block_mut(b, |blk| match seed {
                    Some(s) => {
                        for (i, v) in blk.iter_mut().enumerate() {
                            *v = (i as u8).wrapping_mul(31) ^ s ^ (b as u8);
                        }
                    }
                    None => blk.fill(0),
                });
            }
        }
    }

    /// The root, with the slots already in use counted.
    pub fn root(&self) -> DirAt {
        let (start, chain) = match self.l.root() {
            Root::Fixed { .. } => (0, Vec::new()),
            Root::Cluster(c) => {
                let mut chain = vec![c];
                loop {
                    let v = self.get(*chain.last().unwrap());
                    if !self.l.holds(v) {
                        break (c, chain);
                    }
                    chain.push(v);
                }
            }
        };
        let mut d = DirAt {
            start,
            is_root: true,
            chain,
            used: 0,
        };
        while self
            .slot(&d, d.used)
            .is_some_and(|(b, i)| self.disk.block(PART_LBA + b)[i * ENTRY] != 0)
        {
            d.used += 1;
        }
        d
    }

    /// Slot `n` of `d`: its block and index, if `d` has it yet.
    fn slot(&self, d: &DirAt, n: usize) -> Option<(u32, usize)> {
        let (block, i) = (n / SLOTS, n % SLOTS);
        match (d.is_root, self.l.root()) {
            (true, Root::Fixed { first, blocks }) => {
                (block < blocks as usize).then_some((first + block as u32, i))
            }
            _ => {
                let spc = self.l.blocks_per_cluster() as usize;
                let c = *d.chain.get(block / spc)?;
                Some((self.l.cluster_block(c).unwrap() + (block % spc) as u32, i))
            }
        }
    }

    /// Writes `e` in `d`'s next slot, growing `d` by a zeroed cluster when
    /// it is full.
    pub fn put(&mut self, d: &mut DirAt, e: [u8; ENTRY]) {
        if self.slot(d, d.used).is_none() {
            assert!(
                !(d.is_root && self.l.kind() == FsKind::Fat16),
                "FAT16's root is full"
            );
            let c = self.take(1, None)[0];
            self.fill(&[c], None);
            self.set(*d.chain.last().unwrap(), c);
            d.chain.push(c);
        }
        let (b, i) = self.slot(d, d.used).unwrap();
        self.block_mut(b, |blk| blk[i * ENTRY..(i + 1) * ENTRY].copy_from_slice(&e));
        d.used += 1;
    }

    /// `e`, after the LFN run for `long` if any.
    pub fn put_named(&mut self, d: &mut DirAt, e: [u8; ENTRY], long: Option<&str>) {
        if let Some(long) = long {
            let short: [u8; 11] = e[..11].try_into().unwrap();
            for s in lfn_run(&short, long) {
                self.put(d, s);
            }
        }
        self.put(d, e);
    }

    /// A new directory in `parent`: one zeroed cluster holding `.` and
    /// `..` (0 under the root).
    pub fn mkdir(&mut self, parent: &mut DirAt, name: [u8; 11], long: Option<&str>) -> DirAt {
        let c = self.take(1, None)[0];
        self.fill(&[c], None);
        let up = if parent.is_root { 0 } else { parent.start };
        let mut d = DirAt {
            start: c,
            is_root: false,
            chain: vec![c],
            used: 0,
        };
        self.put(&mut d, raw(DOT, ATTR_DIR, c, 0));
        self.put(&mut d, raw(DOT_DOT, ATTR_DIR, up, 0));
        self.put_named(parent, raw(name, ATTR_DIR, c, 0), long);
        self.dirs.push(d.clone());
        d
    }

    /// A file of `len` bytes in `d` with `extra` clusters past what it
    /// needs, its bytes from `seed`. `len` 0 and `extra` 0 has no chain.
    pub fn file(
        &mut self,
        d: &mut DirAt,
        name: [u8; 11],
        long: Option<&str>,
        (len, extra): (u32, u32),
        seed: u8,
        rng: Option<&mut XorShift>,
    ) -> FileAt {
        let n = len.div_ceil(self.l.cluster_bytes()) + extra;
        let chain = self.take(n, rng);
        self.fill(&chain, Some(seed));
        let start = chain.first().copied().unwrap_or(0);
        self.put_named(d, raw(name, ATTR_FILE, start, len), long);
        let f = FileAt { chain, len };
        self.files.push(f.clone());
        f
    }

    /// A chain of `n` no entry holds.
    pub fn lose(&mut self, n: u32, rng: Option<&mut XorShift>) -> Vec<u32> {
        self.take(n, rng)
    }

    /// Clusters FAT 1 holds: neither free nor bad or reserved.
    pub fn held(&self) -> u32 {
        (2..2 + self.l.clusters())
            .filter(|&c| {
                let v = self.get(c);
                v != 0 && (self.l.holds(v) || v >= self.l.end_mark() - 7)
            })
            .count() as u32
    }
}

/// A short name no other in its directory has: a stem from `kind`'s
/// alphabet with `n` folded in, and an extension.
pub fn short_name(kind: usize, n: u32) -> [u8; 11] {
    let mut name = [b' '; 11];
    let digits = format!("{n:X}");
    let stem: Vec<u8> = match kind % 7 {
        0 => b"F".to_vec(),
        1 => b"A~".to_vec(),
        2 => b"_X".to_vec(),
        3 => b"$R".to_vec(),
        4 => b"B C".to_vec(),
        5 => vec![0x05, b'E'],
        _ => vec![0x82, 0xA0],
    };
    let stem: Vec<u8> = stem.into_iter().chain(digits.bytes()).take(8).collect();
    name[..stem.len()].copy_from_slice(&stem);
    name[8..11].copy_from_slice(match kind % 3 {
        0 => b"TXT",
        1 => b"BIN",
        _ => b"   ",
    });
    name
}

/// What `random_tree` built: its files, its directories and its deepest.
pub struct Built {
    pub files: usize,
    pub dirs: usize,
    pub depth: usize,
}

/// `dirs` directories below the root, none deeper than `depth` (a chain
/// of `depth` first, then the rest anywhere above it; on FAT16, `depth`
/// is at least 2, so the fixed root never fills), each with a few
/// files of every kind: names from every alphabet `short_name` has, LFN
/// runs, deleted entries with stale clusters, zero-length files with and
/// without a cluster, and, if `long`, chains longer than their length.
pub fn random_tree(
    t: &mut Tree,
    rng: &mut XorShift,
    dirs: usize,
    depth: usize,
    long: bool,
) -> Built {
    let mut all: Vec<(DirAt, usize)> = vec![(t.root(), 0)];
    let mut n = 0u32;
    let mut name = |rng: &mut XorShift| {
        n += 1;
        short_name(rng.below(7), n)
    };
    let fixed_root = t.l.kind() == FsKind::Fat16;
    assert!(!fixed_root || depth >= 2, "FAT16's root would fill");
    for i in 0..dirs {
        let p = match i < depth {
            true => i,
            // FAT16's root holds 512 slots: past 300 the rest go deeper.
            false => loop {
                let p = rng.below(all.len());
                if all[p].1 < depth && !(p == 0 && fixed_root && all[0].0.used > 300) {
                    break p;
                }
            },
        };
        let nm = name(rng);
        let lfn = (rng.below(2) == 0).then(|| format!("Dir {i} ~ long $ name"));
        let mut parent = all[p].0.clone();
        let d = t.mkdir(&mut parent, nm, lfn.as_deref());
        all[p].0 = parent;
        all.push((d, all[p].1 + 1));
    }
    let mut files = 0;
    for (k, (d, _)) in all.iter_mut().enumerate() {
        let count = match k == 0 && fixed_root {
            true => rng.below(3),
            false => rng.below(4),
        };
        for _ in 0..count {
            let nm = name(rng);
            let lfn = (rng.below(3) == 0).then(|| format!("File {files} with a long name.wav"));
            let seed = rng.below(256) as u8;
            let cb = t.l.cluster_bytes();
            let (len, extra) = match rng.below(8) {
                0 => (0, 0),
                1 if long => (0, 1),
                2 if long => (
                    1 + rng.below(cb as usize * 2) as u32,
                    1 + rng.below(2) as u32,
                ),
                _ => (1 + rng.below(cb as usize * 3) as u32, 0),
            };
            t.file(d, nm, lfn.as_deref(), (len, extra), seed, Some(rng));
            files += 1;
            if rng.below(6) == 0 {
                let mut gone = raw(
                    name(rng),
                    ATTR_FILE,
                    2 + rng.below(t.l.clusters() as usize) as u32,
                    99,
                );
                gone[0] = 0xE5;
                t.put(d, gone);
            }
        }
    }
    Built {
        files,
        dirs,
        depth: all.iter().map(|d| d.1).max().unwrap_or(0),
    }
}
