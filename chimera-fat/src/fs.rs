//! Directories, `list`, `read`, `write`, `delete` and `make_dir` over a
//! volume's blocks. Every buffer is borrowed from the caller, so none sits on
//! the stack.
//!
//! A device error (`FsError::Dev`) ends the operation where it stands: an
//! unflushed FAT change is dropped and nothing is retried. A failed flush has
//! already emptied the FAT cache, so a retry would write nothing and lose the
//! change without a word; the store re-inits the card instead.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::dir::{self, ENTRY, Entry, ShortName, Slot, encode, lfn_checksum, parse};
use crate::fat::{FatCache, Table};
use crate::fsinfo;
use crate::volume::{Layout, Link, Root};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, StoreError};
use core::ops::ControlFlow;

/// The directories from the root down to `dir`.
fn path(dir: Dir) -> &'static [Dir] {
    match dir {
        Dir::Chimera => &[Dir::Chimera],
        Dir::Projects => &[Dir::Chimera, Dir::Projects],
        Dir::Sounds => &[Dir::Chimera, Dir::Sounds],
    }
}

/// The longest long-name run: 255 characters, 13 a slot.
const MAX_LFN: usize = 20;

/// A directory slot: its block, and its index in the block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Loc {
    block: u32,
    slot: usize,
}

/// The long-name slots right before the current one whose checksum matches.
struct Run {
    at: [Loc; MAX_LFN],
    n: usize,
}

impl Run {
    fn see(&mut self, s: Slot, at: Loc, sum: u8) {
        match s {
            Slot::Lfn(x) if x == sum && self.n < MAX_LFN => {
                self.at[self.n] = at;
                self.n += 1;
            }
            _ => self.n = 0,
        }
    }
}

/// Where a new entry goes: a free slot, or a new cluster on the end of the
/// directory's chain.
enum Space {
    Slot(Loc),
    Grow(u32),
}

impl Space {
    /// A directory with no free slot grows; the FAT16 root can't: `Full`,
    /// found before anything is written.
    fn find<E>(free: Option<Loc>, dir: Root) -> Result<Space, FsError<E>> {
        match (free, dir) {
            (Some(at), _) => Ok(Space::Slot(at)),
            (None, Root::Cluster(start)) => Ok(Space::Grow(start)),
            (None, Root::Fixed { .. }) => Err(FsError::Full),
        }
    }
}

/// A name's entry in a directory, the first free slot, and the long-name
/// run before the entry.
struct Lookup {
    hit: Option<(Slot, Loc)>,
    free: Option<Loc>,
    run: Run,
}

/// One volume, for one operation.
pub struct Fs<'a, B: Blocks> {
    table: Table<'a, B>,
    buf: &'a mut [u8; BLOCK],
    /// Where the allocator starts: the cluster after the last one it took.
    hint: &'a mut Option<u32>,
}

impl<'a, B: Blocks> Fs<'a, B> {
    pub fn new(
        blocks: &'a mut B,
        layout: Layout,
        fat: &'a mut FatCache,
        buf: &'a mut [u8; BLOCK],
        hint: &'a mut Option<u32>,
    ) -> Self {
        Self {
            table: Table::new(blocks, &layout, fat),
            buf,
            hint,
        }
    }

    /// Calls `f` on each file in `dir` that `FileName` takes, with its length.
    pub fn list(
        &mut self,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), FsError<B::Error>> {
        let at = self.open(dir)?;
        self.scan(at, |s, _| {
            if let Slot::File(e) = s
                && let Some(name) = e.name.to_file(dir)
            {
                f(name, e.len);
            }
            ControlFlow::<()>::Continue(())
        })?;
        Ok(())
    }

    /// Streams `file` to `sink`. The chain is checked to hold the entry's
    /// length before `begin`.
    pub fn read(
        &mut self,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), FsError<B::Error>> {
        let at = self.open(file.dir())?;
        let Some((Slot::File(e), _)) = self.lookup(at, ShortName::file(&file))?.hit else {
            return Err(FsError::NotFound);
        };
        let layout = *self.table.layout();
        if e.len > 0 && self.table.chain_len(e.start)? < e.len.div_ceil(layout.cluster_bytes()) {
            return Err(FsError::Corrupt);
        }
        if sink.begin(e.len).is_break() {
            return Ok(());
        }
        let (mut c, mut left) = (e.start, e.len as usize);
        while left > 0 {
            let first = layout.cluster_block(c).ok_or(FsError::Corrupt)?;
            for b in first..first + layout.blocks_per_cluster() {
                self.table
                    .blocks()
                    .read(b, self.buf)
                    .map_err(FsError::Dev)?;
                let n = left.min(BLOCK);
                left -= n;
                if sink.chunk(&self.buf[..n]).is_break() || left == 0 {
                    return Ok(());
                }
            }
            c = match self.table.link(c)? {
                Link::Next(next) => next,
                Link::End | Link::Broken => return Err(FsError::Corrupt),
            };
        }
        Ok(())
    }

    /// Replaces `file` with what `body` puts, and returns its length. In
    /// order, so a cut leaves an empty or short file and lost clusters,
    /// never a length over a short chain:
    /// 1. the entry is reset to length 0 and no chain, or made so in a free
    ///    slot;
    /// 2. the old chain is freed, and flushed;
    /// 3. the data streams into new clusters;
    /// 4. the FAT is flushed;
    /// 5. the entry gets its chain and length.
    ///
    /// The flush in step 2 comes before any allocation, so within a cached
    /// FAT sector entries only ever move away from the card's values, and
    /// no flush rewrites a sector unchanged.
    ///
    /// On a `body` error or `Full`, what was put so far stays: steps 4 and 5
    /// run, then the error returns. A device error or `Full` is sticky in
    /// the sink (`body` sees `Io` or `Full`) and outranks `body`'s own.
    pub fn write(
        &mut self,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, FsError<B::Error>> {
        let r = self.replace(file, body);
        self.settle(r)
    }

    /// Deletes `file`: its entry, and the long-name run before it if the
    /// run's checksum matches, become 0xE5, then its chain is freed. A
    /// directory under the name is `NotFound`.
    pub fn delete(&mut self, file: FileName) -> Result<(), FsError<B::Error>> {
        let r = self.remove(file);
        self.settle(r)
    }

    /// Makes `dir`. One that exists is `Ok`, with nothing written. A new
    /// one is a zeroed cluster holding `.` and `..` (0 under the root),
    /// flushed before the parent's entry points at it.
    pub fn make_dir(&mut self, dir: Dir) -> Result<(), FsError<B::Error>> {
        let r = self.add_dir(dir);
        self.settle(r)
    }

    /// Ends a changing operation: an error drops what the FAT cache held
    /// unflushed. After a device error that is the rule; the other errors
    /// flush before they return, so for them it only empties the cache.
    fn settle<T>(&mut self, r: Result<T, FsError<B::Error>>) -> Result<T, FsError<B::Error>> {
        if r.is_err() {
            self.table.discard();
        }
        r
    }

    fn replace(
        &mut self,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, FsError<B::Error>> {
        let dir = self.open(file.dir())?;
        let name = ShortName::file(&file);
        let found = self.lookup(dir, name)?;
        let (space, old) = match found.hit {
            Some((Slot::File(e), at)) => (Space::Slot(at), Some(e.start)),
            Some(_) => return Err(FsError::Corrupt),
            None => (Space::find(found.free, dir)?, None),
        };
        self.will_change()?;
        let at = self.take(space)?;
        let empty = Entry {
            name,
            start: 0,
            len: 0,
        };
        self.put_entry(at, &empty, false)?;
        if let Some(old) = old {
            self.table.free_chain(old)?;
            self.table.flush()?;
        }

        let mut out = Stream {
            fs: self,
            first: 0,
            last: 0,
            used: 0,
            fill: 0,
            len: 0,
            failed: None,
        };
        let r = body(&mut out);
        if out.failed.is_none() && out.fill > 0 {
            out.emit();
        }
        let Stream {
            first, len, failed, ..
        } = out;
        if let Some(e @ FsError::Dev(_)) = failed {
            return Err(e);
        }
        self.table.flush()?;
        self.put_entry(
            at,
            &Entry {
                start: first,
                len,
                ..empty
            },
            false,
        )?;
        match (failed, r) {
            (Some(e), _) => Err(e),
            (None, Err(e)) => Err(FsError::Body(e)),
            (None, Ok(())) => Ok(len),
        }
    }

    fn remove(&mut self, file: FileName) -> Result<(), FsError<B::Error>> {
        let dir = self.open(file.dir())?;
        let found = self.lookup(dir, ShortName::file(&file))?;
        let Some((Slot::File(e), at)) = found.hit else {
            return Err(FsError::NotFound);
        };
        self.will_change()?;
        self.mark_free(at, &found.run.at[..found.run.n])?;
        self.table.free_chain(e.start)?;
        self.table.flush()
    }

    fn add_dir(&mut self, dir: Dir) -> Result<(), FsError<B::Error>> {
        let root = self.table.layout().root();
        let parent = match dir {
            Dir::Chimera => root,
            Dir::Projects | Dir::Sounds => self.open(Dir::Chimera)?,
        };
        let name = ShortName::dir(dir);
        let found = self.lookup(parent, name)?;
        match found.hit {
            Some((Slot::Dir(_), _)) => return Ok(()),
            Some(_) => return Err(FsError::Corrupt),
            None => {}
        }
        let space = Space::find(found.free, parent)?;
        self.will_change()?;
        let at = self.take(space)?;
        let c = self.claim(None)?;
        let up = match parent {
            Root::Cluster(p) if parent != root => p,
            _ => 0,
        };
        self.fresh(c, Some(up))?;
        self.table.flush()?;
        let e = Entry {
            name,
            start: c,
            len: 0,
        };
        self.put_entry(at, &e, true)
    }

    /// Before an operation's first write. FAT32's FSInfo gets its one patch
    /// (the free count to unknown), and, with no hint in RAM yet, its
    /// next-free seeds the hint. FSInfo is only ever a start point. Then a
    /// FAT sector an earlier failure left stale in FAT 2 is healed.
    fn will_change(&mut self) -> Result<(), FsError<B::Error>> {
        self.patch_fs_info()?;
        self.table.heal()
    }

    fn patch_fs_info(&mut self) -> Result<(), FsError<B::Error>> {
        let layout = *self.table.layout();
        let Some(at) = layout.fs_info() else {
            return Ok(());
        };
        let blocks = self.table.blocks();
        blocks.read(at, self.buf).map_err(FsError::Dev)?;
        if self.hint.is_none() {
            *self.hint = fsinfo::hint(self.buf, &layout);
        }
        if fsinfo::patch(self.buf) {
            blocks.write(at, self.buf).map_err(FsError::Dev)?;
        }
        Ok(())
    }

    /// A free cluster from the hint, linked from `prev`; the hint moves past
    /// it.
    fn claim(&mut self, prev: Option<u32>) -> Result<u32, FsError<B::Error>> {
        // No hint is cluster 0, which the allocator starts at cluster 2.
        let c = self.table.alloc(self.hint.unwrap_or(0), prev)?;
        *self.hint = Some(c + 1);
        Ok(c)
    }

    /// Writes cluster `c` as zeros, or as a new directory holding `.` and
    /// `..` (`up`, the parent's cluster). Returns its first block.
    fn fresh(&mut self, c: u32, up: Option<u32>) -> Result<u32, FsError<B::Error>> {
        let layout = *self.table.layout();
        let first = layout.cluster_block(c).ok_or(FsError::Corrupt)?;
        self.buf.fill(0);
        if let Some(up) = up {
            let slots = self.buf.as_chunks_mut::<ENTRY>().0;
            for (raw, (name, start)) in slots
                .iter_mut()
                .zip([(ShortName::DOT, c), (ShortName::DOT_DOT, up)])
            {
                encode(
                    &Entry {
                        name,
                        start,
                        len: 0,
                    },
                    true,
                    layout.kind(),
                    raw,
                );
            }
        }
        let blocks = self.table.blocks();
        blocks.write(first, self.buf).map_err(FsError::Dev)?;
        if up.is_some() {
            self.buf.fill(0);
        }
        for b in first + 1..first + layout.blocks_per_cluster() {
            blocks.write(b, self.buf).map_err(FsError::Dev)?;
        }
        Ok(first)
    }

    /// The slot `space` names, growing the directory if it has none.
    fn take(&mut self, space: Space) -> Result<Loc, FsError<B::Error>> {
        match space {
            Space::Slot(at) => Ok(at),
            Space::Grow(start) => self.grow(start),
        }
    }

    /// Adds a zeroed cluster to the end of the directory chain from
    /// `start`, flushed, and returns its first slot.
    fn grow(&mut self, start: u32) -> Result<Loc, FsError<B::Error>> {
        let mut last = start;
        for _ in 0..self.table.layout().clusters() {
            match self.table.link(last)? {
                Link::Next(next) => last = next,
                Link::End => {
                    let c = self.claim(Some(last))?;
                    let block = self.fresh(c, None)?;
                    self.table.flush()?;
                    return Ok(Loc { block, slot: 0 });
                }
                Link::Broken => break,
            }
        }
        Err(FsError::Corrupt)
    }

    /// Writes `e` into slot `at`, unless it holds those bytes already.
    fn put_entry(&mut self, at: Loc, e: &Entry, is_dir: bool) -> Result<(), FsError<B::Error>> {
        let kind = self.table.layout().kind();
        let blocks = self.table.blocks();
        blocks.read(at.block, self.buf).map_err(FsError::Dev)?;
        let mut raw = [0; ENTRY];
        encode(e, is_dir, kind, &mut raw);
        let slot = &mut self.buf.as_chunks_mut::<ENTRY>().0[at.slot];
        if *slot == raw {
            return Ok(());
        }
        *slot = raw;
        blocks.write(at.block, self.buf).map_err(FsError::Dev)
    }

    /// Marks `entry` and then `run` deleted, each block read and written
    /// once, `entry`'s first.
    fn mark_free(&mut self, entry: Loc, run: &[Loc]) -> Result<(), FsError<B::Error>> {
        let all = || core::iter::once(entry).chain(run.iter().copied());
        for (i, at) in all().enumerate() {
            if all().take(i).any(|x| x.block == at.block) {
                continue;
            }
            let blocks = self.table.blocks();
            blocks.read(at.block, self.buf).map_err(FsError::Dev)?;
            let slots = self.buf.as_chunks_mut::<ENTRY>().0;
            for x in all().filter(|x| x.block == at.block) {
                dir::free(&mut slots[x.slot]);
            }
            blocks.write(at.block, self.buf).map_err(FsError::Dev)?;
        }
        Ok(())
    }

    /// `dir`'s blocks. A missing directory is `NotFound`; a file under a
    /// directory's name on the path is `Corrupt`.
    fn open(&mut self, dir: Dir) -> Result<Root, FsError<B::Error>> {
        let mut at = self.table.layout().root();
        for &d in path(dir) {
            at = match self.lookup(at, ShortName::dir(d))?.hit {
                Some((Slot::Dir(e), _)) => Root::Cluster(e.start),
                Some(_) => return Err(FsError::Corrupt),
                None => return Err(FsError::NotFound),
            };
        }
        Ok(at)
    }

    /// The first file or directory entry named `name` in `dir`, the first
    /// free slot before it, and the matching long-name run right before it.
    fn lookup(&mut self, dir: Root, name: ShortName) -> Result<Lookup, FsError<B::Error>> {
        let sum = lfn_checksum(&name);
        let mut free = None;
        let mut run = Run {
            at: [Loc { block: 0, slot: 0 }; MAX_LFN],
            n: 0,
        };
        let hit = self.scan(dir, |s, at| {
            match s {
                Slot::File(e) | Slot::Dir(e) if e.name == name => {
                    return ControlFlow::Break((s, at));
                }
                Slot::Free | Slot::End => _ = free.get_or_insert(at),
                _ => {}
            }
            run.see(s, at, sum);
            ControlFlow::Continue(())
        })?;
        Ok(Lookup { hit, free, run })
    }

    /// Calls `f` on each slot of `dir` (a fixed root, or a directory's
    /// chain) up to and including the end mark, until it breaks. A chain is
    /// walked only once `chain_len` has checked it ends.
    fn scan<T>(
        &mut self,
        dir: Root,
        mut f: impl FnMut(Slot, Loc) -> ControlFlow<T>,
    ) -> Result<Option<T>, FsError<B::Error>> {
        let layout = *self.table.layout();
        let (mut block, mut left, mut cluster) = match dir {
            Root::Fixed { first, blocks } => (first, blocks, None),
            Root::Cluster(c) => {
                self.table.chain_len(c)?;
                let first = layout.cluster_block(c).ok_or(FsError::Corrupt)?;
                (first, layout.blocks_per_cluster(), Some(c))
            }
        };
        loop {
            if left == 0 {
                let Some(c) = cluster else {
                    return Ok(None);
                };
                let next = match self.table.link(c)? {
                    Link::Next(next) => next,
                    Link::End => return Ok(None),
                    Link::Broken => return Err(FsError::Corrupt),
                };
                block = layout.cluster_block(next).ok_or(FsError::Corrupt)?;
                (left, cluster) = (layout.blocks_per_cluster(), Some(next));
            }
            self.table
                .blocks()
                .read(block, self.buf)
                .map_err(FsError::Dev)?;
            for (slot, raw) in self.buf.as_chunks::<ENTRY>().0.iter().enumerate() {
                let s = parse(raw, layout.kind());
                if let ControlFlow::Break(t) = f(s, Loc { block, slot }) {
                    return Ok(Some(t));
                }
                if s == Slot::End {
                    return Ok(None);
                }
            }
            (block, left) = (block + 1, left - 1);
        }
    }
}

/// `write`'s sink: one block buffered in the `Fs`'s `buf`, written to the
/// file's next block when full.
struct Stream<'s, 'a, B: Blocks> {
    fs: &'s mut Fs<'a, B>,
    /// The chain's first and last clusters (0: none yet), and the blocks
    /// used in the last.
    first: u32,
    last: u32,
    used: u32,
    /// Bytes in `buf`, and bytes written.
    fill: usize,
    len: u32,
    /// A device error or `Full`: nothing more is written.
    failed: Option<FsError<B::Error>>,
}

impl<B: Blocks> Stream<'_, '_, B> {
    /// Writes `buf` to the file's next block, zero padded.
    fn emit(&mut self) {
        if let Err(e) = self.try_emit() {
            self.failed = Some(e);
        }
    }

    fn try_emit(&mut self) -> Result<(), FsError<B::Error>> {
        let layout = *self.fs.table.layout();
        let len = u32::try_from(self.fill)
            .ok()
            .and_then(|n| self.len.checked_add(n))
            .ok_or(FsError::Full)?;
        if self.last == 0 || self.used == layout.blocks_per_cluster() {
            let c = self.fs.claim((self.last != 0).then_some(self.last))?;
            if self.first == 0 {
                self.first = c;
            }
            (self.last, self.used) = (c, 0);
        }
        let block = layout.cluster_block(self.last).ok_or(FsError::Corrupt)? + self.used;
        self.fs.buf[self.fill..].fill(0);
        self.fs
            .table
            .blocks()
            .write(block, self.fs.buf)
            .map_err(FsError::Dev)?;
        (self.used, self.fill, self.len) = (self.used + 1, 0, len);
        Ok(())
    }
}

impl<B: Blocks> ByteSink for Stream<'_, '_, B> {
    fn put(&mut self, mut bytes: &[u8]) -> Result<(), StoreError> {
        loop {
            match self.failed {
                Some(FsError::Full) => return Err(StoreError::Full),
                Some(FsError::Corrupt) => return Err(StoreError::Corrupt),
                Some(_) => return Err(StoreError::Io),
                None if bytes.is_empty() => return Ok(()),
                None => {}
            }
            let n = (BLOCK - self.fill).min(bytes.len());
            self.fs.buf[self.fill..self.fill + n].copy_from_slice(&bytes[..n]);
            (self.fill, bytes) = (self.fill + n, &bytes[n..]);
            if self.fill == BLOCK {
                self.emit();
            }
        }
    }
}
