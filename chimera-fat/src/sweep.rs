//! The lost-cluster sweep (#360, ADR 0094): clusters the FAT holds that no
//! directory on the volume reaches, found in one operation at boot and,
//! only in `Sweep::Free`, freed.
//!
//! Four phases. **Compare** reads FAT 1 and FAT 2 side by side: a cut can
//! leave one sector `D` differing, and its FAT 2 copy is kept. **Mark**
//! walks every directory from the root and each live entry's chain into a
//! bitmap; through `D`, FAT 2 may differ only as a FAT 1-first cut leaves it. **Count**
//! passes over FAT 1 once. **Free** writes `D`'s FAT 2 copy from FAT 1,
//! then FSInfo, then frees each lost cluster.
//!
//! Every phase before Free only reads, and any doubt ends the sweep with
//! nothing written: a cluster it frees must be one no entry reaches. The
//! mark phase reads raw entries, byte 0 and byte 11, never `dir::parse`:
//! that one gives `Slot::Other` to names outside A-Z and 0-9, which here
//! would read as no entry and free a real file.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::dir::ENTRY;
use crate::fat::{FREE, FatCache, Table};
use crate::fsinfo;
use crate::volume::{FsKind, Layout, Link, Root};
use chimera_hal::store::{Doubt, SweepStats};

/// Deeper than this aborts: the root is depth 0.
pub const MAX_DEPTH: usize = 8;
/// More directories than this, the root not counted, aborts.
pub const MAX_DIRS: usize = 256;

const FIRST_CLUSTER: u32 = 2;
const SLOTS: usize = BLOCK / ENTRY;
const END: u8 = 0x00;
const DELETED: u8 = 0xE5;
const ATTR: usize = 11;
const ATTR_LABEL: u8 = 0x08;
const ATTR_DIR: u8 = 0x10;
const ATTR_LFN: u8 = 0x0F;
const ATTR_LFN_MASK: u8 = 0x3F;
const CLUSTER_HI: usize = 20;
const CLUSTER_LO: usize = 26;
const SIZE: usize = 28;
const DOT: [u8; 11] = *b".          ";
const DOT_DOT: [u8; 11] = *b"..         ";

/// Why a read phase stopped.
pub(crate) enum Fail<E> {
    Doubt(Doubt),
    Dev(E),
}

impl<E> From<Doubt> for Fail<E> {
    fn from(d: Doubt) -> Self {
        Fail::Doubt(d)
    }
}

impl<E> From<FsError<E>> for Fail<E> {
    fn from(e: FsError<E>) -> Self {
        match e {
            FsError::Dev(e) => Fail::Dev(e),
            // `Table` refuses only an unheld cluster, which mark never asks.
            _ => Fail::Doubt(Doubt::DirBroken),
        }
    }
}

/// What the read phases found.
pub(crate) struct Found {
    pub lost: u32,
    /// FAT 1's block of the one sector FAT 2 differs in; its FAT 2 copy
    /// is the caller's `d`.
    pub differs: Option<u32>,
}

/// A reached cluster's link when its sector differs: FAT 1's `one`, if
/// FAT 2's (`two`, its raw value `raw2`) is what FAT 1's was before a FAT
/// 1-first write: an end mark a link replaced, or a free cluster an
/// allocation took. Any other difference, FAT 2 linking past FAT 1 above
/// all, is `FatsDiffer`: the free phase copies FAT 1 over FAT 2, which
/// would strand whatever only FAT 2 reaches.
fn fat1_first(one: Link, two: Link, raw2: u32) -> Result<Link, Doubt> {
    match (one, two) {
        (a, b) if a == b => Ok(a),
        (Link::Next(_), Link::End) => Ok(one),
        (Link::Next(_) | Link::End, _) if raw2 == FREE => Ok(one),
        _ => Err(Doubt::FatsDiffer),
    }
}

/// A raw FAT value that holds a cluster: a link or an end mark. Free,
/// cluster 1, out of range, reserved and bad stay as they are.
fn holds_cluster(l: &Layout, v: u32) -> bool {
    v != FREE && matches!(l.as_link(v), Link::Next(_) | Link::End)
}

/// Bit `c - 2` of the reachability bitmap: cluster `c` is reached.
fn marked(marks: &[u8], c: u32) -> bool {
    let i = (c - FIRST_CLUSTER) as usize;
    marks[i / 8] & (1 << (i % 8)) != 0
}

/// The reachability bitmap, while mark fills it.
struct Marks<'a>(&'a mut [u8]);

impl Marks<'_> {
    fn get(&self, c: u32) -> bool {
        marked(self.0, c)
    }

    /// False if `c` was marked already.
    fn set(&mut self, c: u32) -> bool {
        let i = (c - FIRST_CLUSTER) as usize;
        let was = self.0[i / 8] & (1 << (i % 8)) != 0;
        self.0[i / 8] |= 1 << (i % 8);
        !was
    }
}

/// Whether `marks` has a bit for every cluster of `l`.
pub(crate) fn has_room(l: &Layout, marks: &[u8]) -> bool {
    marks.len() as u64 * 8 >= u64::from(l.clusters())
}

/// Compare, mark and count, reading only, through `t`. `d` holds FAT 2's
/// copy of the differing sector, if any, for the free phase.
pub(crate) fn survey<B: Blocks>(
    t: &mut Table<'_, B>,
    buf: &mut [u8; BLOCK],
    marks: &mut [u8],
    d: &mut [u8; BLOCK],
    clock: &dyn Fn() -> u32,
    stats: &mut SweepStats,
) -> Result<Found, Fail<B::Error>> {
    let l = *t.layout();
    debug_assert!(has_room(&l, marks));
    let t0 = clock();
    let differs = compare(t.blocks(), &l, buf, d)?;
    let t1 = clock();
    stats.compare_ms = t1.wrapping_sub(t0);
    let marks = &mut marks[..l.clusters().div_ceil(8) as usize];
    marks.fill(0);
    let mut walk = Walk {
        t: &mut *t,
        l,
        d: differs.map(|s| (s, &*d)),
        buf: &mut *buf,
        marks: Marks(marks),
    };
    let r = walk.run();
    let Walk { marks: m, .. } = walk;
    let t2 = clock();
    stats.mark_ms = t2.wrapping_sub(t1);
    r?;
    let lost = count(t.blocks(), &l, buf, &m, differs.map(|s| (s, &*d)))?;
    stats.count_ms = clock().wrapping_sub(t2);
    Ok(Found { lost, differs })
}

/// FAT 1 against every other copy, sector by sector: the one sector that
/// differs, its FAT 2 copy in `d`; more than one is `FatsDiffer`.
fn compare<B: Blocks>(
    blocks: &mut B,
    l: &Layout,
    buf: &mut [u8; BLOCK],
    d: &mut [u8; BLOCK],
) -> Result<Option<u32>, Fail<B::Error>> {
    let mut differs = None;
    let mut other = [0; BLOCK];
    for s in l.fat1() {
        blocks.read(s, buf).map_err(Fail::Dev)?;
        for copy in l.fat_copies(s).skip(1) {
            blocks.read(copy, &mut other).map_err(Fail::Dev)?;
            if other != *buf {
                if differs.is_some_and(|x| x != s) {
                    return Err(Doubt::FatsDiffer.into());
                }
                differs = Some(s);
                *d = other;
            }
        }
    }
    Ok(differs)
}

/// Lost clusters: in `2..clusters + 2`, unmarked, holding a cluster in
/// FAT 1 and, in the differing sector, in FAT 2's copy too.
fn count<B: Blocks>(
    blocks: &mut B,
    l: &Layout,
    buf: &mut [u8; BLOCK],
    m: &Marks<'_>,
    d: Option<(u32, &[u8; BLOCK])>,
) -> Result<u32, Fail<B::Error>> {
    let mut lost = 0;
    let mut loaded = None;
    for c in FIRST_CLUSTER..FIRST_CLUSTER + l.clusters() {
        let s = l.fat_block(c);
        if loaded != Some(s) {
            blocks.read(s, buf).map_err(Fail::Dev)?;
            loaded = Some(s);
        }
        if !m.get(c) && is_lost(l, c, buf, d) {
            lost += 1;
        }
    }
    Ok(lost)
}

/// Unmarked `c`, its entry in `fat1` (its FAT 1 sector), lost in both
/// copies.
fn is_lost(l: &Layout, c: u32, fat1: &[u8; BLOCK], d: Option<(u32, &[u8; BLOCK])>) -> bool {
    holds_cluster(l, l.entry(fat1, c))
        && match d {
            Some((s, copy)) if s == l.fat_block(c) => holds_cluster(l, l.entry(copy, c)),
            _ => true,
        }
}

/// The free phase, after `survey` found no doubt: `D`'s FAT 2 copy from
/// FAT 1, then FSInfo's free count to unknown, then each lost cluster
/// freed in every FAT, sector by sector. Returns how many it freed. A
/// stop partway is safe: each was proven unreachable before the first
/// write.
pub(crate) fn free<B: Blocks>(
    blocks: &mut B,
    l: &Layout,
    cache: &mut FatCache,
    buf: &mut [u8; BLOCK],
    marks: &[u8],
    found: &Found,
    d: &[u8; BLOCK],
) -> Result<u32, FsError<B::Error>> {
    let mut t = Table::new(blocks, l, cache);
    if let Some(s) = found.differs {
        t.copy_fat1(s)?;
    }
    if found.lost == 0 {
        return Ok(0);
    }
    if let Some(at) = l.fs_info() {
        let blocks = t.blocks();
        blocks.read(at, buf).map_err(FsError::Dev)?;
        if fsinfo::patch(buf) {
            blocks.write(at, buf).map_err(FsError::Dev)?;
        }
    }
    let d = found.differs.map(|s| (s, d));
    let mut freed = 0;
    for c in FIRST_CLUSTER..FIRST_CLUSTER + l.clusters() {
        if marked(marks, c) {
            continue;
        }
        let v = t.value(c)?;
        let both = match d {
            Some((s, copy)) if s == l.fat_block(c) => holds_cluster(l, l.entry(copy, c)),
            _ => true,
        };
        if holds_cluster(l, v) && both {
            t.set(c, FREE)?;
            freed += 1;
        }
    }
    t.flush()?;
    Ok(freed)
}

/// Where a directory's next block is.
#[derive(Clone, Copy)]
enum Cursor {
    /// FAT16's root: `left` blocks from `block`.
    Fixed { block: u32, left: u32 },
    /// Block `i` of cluster `c`.
    Chain { c: u32, i: u32 },
}

/// One directory on the walk's stack.
#[derive(Clone, Copy)]
struct Frame {
    at: Cursor,
    /// The slot to read next in the block at `at`.
    slot: usize,
    /// Its first cluster; 0 for the root.
    start: u32,
    /// Its parent's first cluster; 0 under the root.
    parent: u32,
    depth: usize,
    /// A `0x00` entry was seen: nothing live may follow.
    ended: bool,
}

impl Frame {
    fn first_block(&self) -> bool {
        matches!(self.at, Cursor::Chain { c, i: 0 } if c == self.start && self.start != 0)
    }
}

/// What a chain is, for the doubt it raises.
#[derive(Clone, Copy)]
enum Owner {
    Dir,
    File { len: u32 },
}

/// The mark phase.
struct Walk<'w, 'm, 'b, B: Blocks> {
    t: &'w mut Table<'b, B>,
    l: Layout,
    d: Option<(u32, &'w [u8; BLOCK])>,
    buf: &'w mut [u8; BLOCK],
    marks: Marks<'m>,
}

impl<B: Blocks> Walk<'_, '_, '_, B> {
    /// `c`'s link: FAT 1's, which in the differing sector FAT 2's must
    /// agree with as a FAT 1-first cut leaves it (`fat1_first`).
    fn link(&mut self, c: u32) -> Result<Link, Fail<B::Error>> {
        let one = self.t.link(c)?;
        Ok(match self.d {
            Some((s, copy)) if s == self.l.fat_block(c) => {
                fat1_first(one, self.l.link(copy, c), self.l.entry(copy, c))?
            }
            _ => one,
        })
    }

    /// Marks the chain from `start` in full: a cluster reached twice is
    /// `CrossLinked`, a break before an end mark is the owner's doubt, and
    /// so is a file chain shorter than its length.
    fn mark_chain(&mut self, start: u32, owner: Owner) -> Result<(), Fail<B::Error>> {
        let broken = match owner {
            Owner::Dir => Doubt::DirBroken,
            Owner::File { .. } => Doubt::FileShort,
        };
        let (mut c, mut n) = (start, 0u64);
        loop {
            if !self.l.holds(c) {
                return Err(broken.into());
            }
            if !self.marks.set(c) {
                return Err(Doubt::CrossLinked.into());
            }
            n += 1;
            match self.link(c)? {
                Link::Next(next) => c = next,
                Link::End => break,
                Link::Broken => return Err(broken.into()),
            }
        }
        match owner {
            Owner::File { len } if n * u64::from(self.l.cluster_bytes()) < u64::from(len) => {
                Err(Doubt::FileShort.into())
            }
            _ => Ok(()),
        }
    }

    fn start(&self, e: &[u8]) -> u32 {
        let lo = u32::from(u16::from_le_bytes([e[CLUSTER_LO], e[CLUSTER_LO + 1]]));
        match self.l.kind() {
            FsKind::Fat16 => lo,
            FsKind::Fat32 => {
                lo | u32::from(u16::from_le_bytes([e[CLUSTER_HI], e[CLUSTER_HI + 1]])) << 16
            }
        }
    }

    /// Every directory from the root, depth first, on an explicit stack.
    fn run(&mut self) -> Result<(), Fail<B::Error>> {
        let root = match self.l.root() {
            Root::Fixed { first, blocks } => Frame {
                at: Cursor::Fixed {
                    block: first,
                    left: blocks,
                },
                slot: 0,
                start: 0,
                parent: 0,
                depth: 0,
                ended: false,
            },
            Root::Cluster(c) => {
                self.mark_chain(c, Owner::Dir)?;
                Frame {
                    at: Cursor::Chain { c, i: 0 },
                    slot: 0,
                    start: 0,
                    parent: 0,
                    depth: 0,
                    ended: false,
                }
            }
        };
        let root_cluster = match self.l.root() {
            Root::Cluster(c) => c,
            Root::Fixed { .. } => 0,
        };
        let mut stack = [root; MAX_DEPTH + 1];
        let mut top = 0;
        let mut dirs = 0;
        'frames: loop {
            let f = stack[top];
            let block = match f.at {
                Cursor::Fixed { left: 0, .. } => None,
                Cursor::Fixed { block, .. } => Some(block),
                // `mark_chain` checked `holds(c)`, so this is never `None`;
                // if it is, the directory is broken, never skipped.
                Cursor::Chain { c, i } => match self.l.cluster_block(c) {
                    Some(b) => Some(b + i),
                    None => return Err(Doubt::DirBroken.into()),
                },
            };
            if let Some(block) = block {
                self.t.blocks().read(block, self.buf).map_err(Fail::Dev)?;
                for slot in f.slot..SLOTS {
                    let e = &self.buf[slot * ENTRY..(slot + 1) * ENTRY];
                    let (b0, attr) = (e[0], e[ATTR]);
                    // A subdirectory opens with `.`, then `..`.
                    if f.first_block() && slot < 2 && e[..11] != [DOT, DOT_DOT][slot] {
                        return Err(Doubt::DirBroken.into());
                    }
                    if b0 == END {
                        stack[top].ended = true;
                        continue;
                    }
                    if b0 == DELETED {
                        continue;
                    }
                    if attr & ATTR_LFN_MASK == ATTR_LFN {
                        // An LFN slot's bytes 20-21 are name characters.
                        if e[CLUSTER_LO..CLUSTER_LO + 2] != [0, 0] {
                            return Err(Doubt::DirBroken.into());
                        }
                        continue;
                    }
                    if attr & ATTR_LABEL != 0 {
                        if attr & ATTR_DIR != 0 || self.start(e) != 0 {
                            return Err(Doubt::DirBroken.into());
                        }
                        continue;
                    }
                    // Live, a 0x05 lead byte included.
                    if stack[top].ended {
                        return Err(Doubt::DirBroken.into());
                    }
                    let start = self.start(e);
                    let name = &e[..11];
                    if name == DOT || name == DOT_DOT {
                        let want = match name == DOT {
                            true => (0, f.start == start),
                            false => (
                                1,
                                start == f.parent || (f.parent == 0 && start == root_cluster),
                            ),
                        };
                        if !(f.first_block() && slot == want.0 && want.1) {
                            return Err(Doubt::DirBroken.into());
                        }
                        continue;
                    }
                    if attr & ATTR_DIR == 0 {
                        let len =
                            u32::from_le_bytes([e[SIZE], e[SIZE + 1], e[SIZE + 2], e[SIZE + 3]]);
                        match start {
                            0 if len > 0 => return Err(Doubt::FileShort.into()),
                            0 => {}
                            _ => self.mark_chain(start, Owner::File { len })?,
                        }
                        continue;
                    }
                    if start == 0 {
                        return Err(Doubt::DirBroken.into());
                    }
                    if f.depth + 1 > MAX_DEPTH || dirs + 1 > MAX_DIRS {
                        return Err(Doubt::TooDeep.into());
                    }
                    dirs += 1;
                    self.mark_chain(start, Owner::Dir)?;
                    stack[top].slot = slot + 1;
                    top += 1;
                    stack[top] = Frame {
                        at: Cursor::Chain { c: start, i: 0 },
                        slot: 0,
                        start,
                        parent: f.start,
                        depth: f.depth + 1,
                        ended: false,
                    };
                    continue 'frames;
                }
            }
            // The block is done: on to the next, or up a level.
            let next = match f.at {
                Cursor::Fixed { left: 0, .. } => None,
                Cursor::Fixed { block, left } => Some(Cursor::Fixed {
                    block: block + 1,
                    left: left - 1,
                }),
                Cursor::Chain { c, i } if i + 1 < self.l.blocks_per_cluster() => {
                    Some(Cursor::Chain { c, i: i + 1 })
                }
                Cursor::Chain { c, .. } => match self.link(c)? {
                    Link::Next(n) => Some(Cursor::Chain { c: n, i: 0 }),
                    Link::End => None,
                    Link::Broken => return Err(Doubt::DirBroken.into()),
                },
            };
            match next {
                Some(at) => {
                    stack[top] = Frame {
                        at,
                        slot: 0,
                        ..stack[top]
                    }
                }
                None if top == 0 => return Ok(()),
                None => top -= 1,
            }
        }
    }
}
