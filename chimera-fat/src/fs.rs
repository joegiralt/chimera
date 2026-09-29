//! Directories, `list` and `read` over a volume's blocks. Every buffer is
//! borrowed from the caller, so none sits on the stack.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::dir::{ENTRY, ShortName, Slot, parse};
use crate::fat::{FatCache, Table};
use crate::volume::{Layout, Link, Root};
use chimera_hal::store::{Dir, FileName, ReadSink};
use core::ops::ControlFlow;

/// The directories from the root down to `dir`.
fn path(dir: Dir) -> &'static [Dir] {
    match dir {
        Dir::Chimera => &[Dir::Chimera],
        Dir::Projects => &[Dir::Chimera, Dir::Projects],
        Dir::Sounds => &[Dir::Chimera, Dir::Sounds],
    }
}

/// One volume, for one operation.
pub struct Fs<'a, B: Blocks> {
    table: Table<'a, B>,
    buf: &'a mut [u8; BLOCK],
    #[expect(
        dead_code,
        reason = "the allocator's start point, used by the write path"
    )]
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
        self.scan(at, |s| {
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
        let Some(Slot::File(e)) = self.find(at, ShortName::file(&file))? else {
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

    /// `dir`'s blocks. A missing directory is `NotFound`; a file under a
    /// directory's name on the path is `Corrupt`.
    fn open(&mut self, dir: Dir) -> Result<Root, FsError<B::Error>> {
        let mut at = self.table.layout().root();
        for &d in path(dir) {
            at = match self.find(at, ShortName::dir(d))? {
                Some(Slot::Dir(e)) => Root::Cluster(e.start),
                Some(_) => return Err(FsError::Corrupt),
                None => return Err(FsError::NotFound),
            };
        }
        Ok(at)
    }

    /// The first file or directory entry named `name` in `dir`.
    fn find(&mut self, dir: Root, name: ShortName) -> Result<Option<Slot>, FsError<B::Error>> {
        self.scan(dir, |s| match s {
            Slot::File(e) | Slot::Dir(e) if e.name == name => ControlFlow::Break(s),
            _ => ControlFlow::Continue(()),
        })
    }

    /// Calls `f` on each slot of `dir` (a fixed root, or a directory's
    /// chain) up to the end mark, until it breaks. A chain is walked only
    /// once `chain_len` has checked it ends.
    fn scan<T>(
        &mut self,
        dir: Root,
        mut f: impl FnMut(Slot) -> ControlFlow<T>,
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
            for raw in self.buf.as_chunks::<ENTRY>().0 {
                match parse(raw, layout.kind()) {
                    Slot::End => return Ok(None),
                    s => {
                        if let ControlFlow::Break(t) = f(s) {
                            return Ok(Some(t));
                        }
                    }
                }
            }
            (block, left) = (block + 1, left - 1);
        }
    }
}
