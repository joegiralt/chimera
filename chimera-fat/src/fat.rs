//! The FAT: one cached sector, written to every copy, bounded chain walks and
//! the allocator. Only clusters `Layout::holds` accepts are ever read,
//! followed or written.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::volume::{Layout, Link};

const FIRST_CLUSTER: u32 = 2;
const FREE: u32 = 0;

/// One FAT 1 sector, and whether it differs from the card.
pub struct FatCache {
    sector: Option<u32>,
    dirty: bool,
    buf: [u8; BLOCK],
    /// A FAT 1 sector whose write reached FAT 1 but not every other copy.
    /// It outlives the operation: the store keeps it for its volume, and
    /// the next change there copies it over first (`Table::heal`).
    pub stale: Option<u32>,
}

impl FatCache {
    pub const fn new() -> Self {
        Self {
            sector: None,
            dirty: false,
            buf: [0; BLOCK],
            stale: None,
        }
    }
}

impl Default for FatCache {
    fn default() -> Self {
        Self::new()
    }
}

/// The FAT of one volume, for one operation.
pub struct Table<'a, B: Blocks> {
    blocks: &'a mut B,
    /// A copy, so an `Fs` can own its `Table` for a whole operation.
    layout: Layout,
    cache: &'a mut FatCache,
}

impl<'a, B: Blocks> Table<'a, B> {
    /// Empties `cache`: what it held may be from another card. A dirty
    /// cache here is a bug: a `Table` dropped with a change unflushed (a
    /// failed flush empties it).
    pub fn new(blocks: &'a mut B, layout: &Layout, cache: &'a mut FatCache) -> Self {
        debug_assert!(!cache.dirty, "unflushed FAT change");
        cache.sector = None;
        cache.dirty = false;
        Self {
            blocks,
            layout: *layout,
            cache,
        }
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The block device, for everything outside the FAT.
    pub fn blocks(&mut self) -> &mut B {
        self.blocks
    }

    /// Loads the FAT 1 sector holding `c`'s entry, flushing the one it
    /// replaces. `c` is held.
    fn load(&mut self, c: u32) -> Result<(), FsError<B::Error>> {
        let sector = self.layout.fat_block(c);
        if self.cache.sector == Some(sector) {
            return Ok(());
        }
        self.flush()?;
        self.cache.sector = None;
        self.blocks
            .read(sector, &mut self.cache.buf)
            .map_err(FsError::Dev)?;
        self.cache.sector = Some(sector);
        Ok(())
    }

    /// `c`'s raw entry; `c` is held.
    fn entry(&mut self, c: u32) -> Result<u32, FsError<B::Error>> {
        self.load(c)?;
        Ok(self.layout.entry(&self.cache.buf, c))
    }

    fn held(&self, c: u32) -> Result<u32, FsError<B::Error>> {
        if self.layout.holds(c) {
            Ok(c)
        } else {
            Err(FsError::Corrupt)
        }
    }

    /// `c`'s entry as a link; `c` not held is `Corrupt`.
    pub fn link(&mut self, c: u32) -> Result<Link, FsError<B::Error>> {
        let v = self.entry(self.held(c)?)?;
        Ok(self.layout.as_link(v))
    }

    /// Sets `c`'s entry. The sector turns dirty only if the value changes.
    pub fn set(&mut self, c: u32, v: u32) -> Result<(), FsError<B::Error>> {
        if self.entry(self.held(c)?)? != v {
            self.layout.put_entry(&mut self.cache.buf, c, v);
            self.cache.dirty = true;
        }
        Ok(())
    }

    /// Writes a dirty sector to every FAT, FAT 1 first. A failed write
    /// empties the cache: what the card holds is unknown, so it is read
    /// again. One that fails after FAT 1's landed leaves the sector `stale`.
    pub fn flush(&mut self) -> Result<(), FsError<B::Error>> {
        let Some(sector) = self.cache.sector.filter(|_| self.cache.dirty) else {
            return Ok(());
        };
        self.cache.dirty = false;
        for (i, copy) in self.layout.fat_copies(sector).enumerate() {
            if let Err(e) = self.blocks.write(copy, &self.cache.buf) {
                self.cache.sector = None;
                if i > 0 {
                    self.cache.stale = Some(sector);
                }
                return Err(FsError::Dev(e));
            }
        }
        Ok(())
    }

    /// Copies the `stale` sector from FAT 1 to every other FAT, then
    /// forgets it; before an operation's first FAT change. A sector outside
    /// this volume's FAT 1 is forgotten unwritten. A failed write keeps it.
    pub fn heal(&mut self) -> Result<(), FsError<B::Error>> {
        let Some(sector) = self.cache.stale else {
            return Ok(());
        };
        if !self.layout.fat1().contains(&sector) {
            self.cache.stale = None;
            return Ok(());
        }
        self.flush()?;
        self.cache.sector = None;
        self.blocks
            .read(sector, &mut self.cache.buf)
            .map_err(FsError::Dev)?;
        self.cache.sector = Some(sector);
        for copy in self.layout.fat_copies(sector).skip(1) {
            if let Err(e) = self.blocks.write(copy, &self.cache.buf) {
                self.cache.sector = None;
                return Err(FsError::Dev(e));
            }
        }
        self.cache.stale = None;
        Ok(())
    }

    /// Drops an unflushed change: after a device error the operation is
    /// over, the card's state is unknown and the next operation reads it
    /// again. Nothing retries a flush.
    pub fn discard(&mut self) {
        self.cache.sector = None;
        self.cache.dirty = false;
    }

    /// The first free cluster from `from` (any value: an unheld one starts
    /// at cluster 2), wrapping once. It becomes the end of a chain, and
    /// `prev`, if any, links to it. `Full` changes nothing.
    pub fn alloc(&mut self, from: u32, prev: Option<u32>) -> Result<u32, FsError<B::Error>> {
        if let Some(p) = prev {
            self.held(p)?;
        }
        let end = FIRST_CLUSTER + self.layout.clusters();
        let from = if self.layout.holds(from) {
            from
        } else {
            FIRST_CLUSTER
        };
        for c in (from..end).chain(FIRST_CLUSTER..from) {
            if self.entry(c)? == FREE {
                self.set(c, self.layout.end_mark())?;
                if let Some(p) = prev {
                    self.set(p, c)?;
                }
                return Ok(c);
            }
        }
        Err(FsError::Full)
    }

    /// The chain's length from `start`, in at most `clusters` steps. A
    /// broken link, or a chain longer than the volume (a loop), is `Corrupt`.
    pub fn chain_len(&mut self, start: u32) -> Result<u32, FsError<B::Error>> {
        let (mut c, mut n) = (start, 1);
        loop {
            match self.link(c)? {
                Link::End => return Ok(n),
                Link::Next(next) if n < self.layout.clusters() => (c, n) = (next, n + 1),
                Link::Next(_) | Link::Broken => return Err(FsError::Corrupt),
            }
        }
    }

    /// Frees the chain from `start` up to its end or first bad link, in at
    /// most `clusters` steps, and returns how many clusters it freed. A
    /// loop stops where it meets a cluster it already freed. A cluster
    /// marked bad or reserved is bad media, not the chain's: it stays.
    pub fn free_chain(&mut self, start: u32) -> Result<u32, FsError<B::Error>> {
        let (mut c, mut n) = (start, 0);
        while n < self.layout.clusters() && self.layout.holds(c) {
            let v = self.entry(c)?;
            if v == FREE || self.layout.is_reserved(v) {
                break;
            }
            self.set(c, FREE)?;
            n += 1;
            match self.layout.as_link(v) {
                Link::Next(next) => c = next,
                Link::End | Link::Broken => break,
            }
        }
        Ok(n)
    }

    /// Free clusters in `2..clusters + 2`.
    pub fn free_count(&mut self) -> Result<u32, FsError<B::Error>> {
        let mut n = 0;
        for c in FIRST_CLUSTER..FIRST_CLUSTER + self.layout.clusters() {
            if self.entry(c)? == FREE {
                n += 1;
            }
        }
        Ok(n)
    }
}
