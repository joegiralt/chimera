// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The FAT: one cached sector, written to every copy, bounded chain walks and
//! the allocator. Only clusters `Layout::holds` accepts are ever read,
//! followed or written.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::volume::{Layout, Link};

const FIRST_CLUSTER: u32 = 2;
pub(crate) const FREE: u32 = 0;

/// A FAT 1 sector whose write reached FAT 1 but not every other copy.
/// Only a failed `Table::flush` makes one, so no caller can name a sector
/// to be copied over.
#[derive(Debug, PartialEq, Eq)]
pub struct Stale(u32);

/// A chain no entry holds, which a write a device error stopped left
/// allocated: its first cluster still on the card, and the `(prev, c)` of
/// an `alloc` whose link may not have landed (`Table::fresh`).
pub type Orphan = (u32, Option<(u32, u32)>);

/// What `free_chain` freed: how many, the last, and whether the walk
/// stopped on an end mark (not a free, reserved or broken link).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Freed {
    pub n: u32,
    pub tail: Option<u32>,
    pub ended: bool,
}

/// How far a flush got before a copy failed.
enum Landed<E> {
    /// FAT 1's write failed: the card may hold the old sector.
    Nowhere(E),
    /// FAT 1 holds the sector; a later copy failed.
    Fat1(E),
}

/// One FAT 1 sector, and whether it differs from the card.
pub struct FatCache {
    sector: Option<u32>,
    dirty: bool,
    buf: [u8; BLOCK],
    /// Outlives the operation: the store carries it for its volume, and the
    /// next change there copies it over first (`Table::heal`).
    stale: Option<Stale>,
    /// What a device error left allocated with no entry: carried the same
    /// way, freed by `heal`.
    orphan: Option<Orphan>,
}

impl FatCache {
    pub const fn new() -> Self {
        Self {
            sector: None,
            dirty: false,
            buf: [0; BLOCK],
            stale: None,
            orphan: None,
        }
    }
}

impl FatCache {
    /// The sector a failed flush left stale, for the store to keep.
    pub fn take_stale(&mut self) -> Option<Stale> {
        self.stale.take()
    }

    /// Hands back what `take_stale` gave, for the volume it came from.
    pub fn keep_stale(&mut self, stale: Option<Stale>) {
        self.stale = stale;
    }

    /// The chain a failed write left, for the store to keep.
    pub fn take_orphan(&mut self) -> Option<Orphan> {
        self.orphan.take()
    }

    /// Hands back what `take_orphan` gave, for the volume it came from.
    pub fn keep_orphan(&mut self, orphan: Option<Orphan>) {
        self.orphan = orphan;
    }

    /// A stale sector or an orphan waits for the next change to heal.
    pub fn pending(&self) -> bool {
        self.stale.is_some() || self.orphan.is_some()
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
    /// The `(prev, c)` of the last `alloc` whose `c` sits in another FAT
    /// sector than `prev`: `c` reads end-of-chain before `prev`'s link to
    /// it is known to be in FAT 1 (#362). Cleared once a flush carrying
    /// the link lands there; `alloc` flushes first while one is pending,
    /// so there is at most one.
    fresh: Option<(u32, u32)>,
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
            fresh: None,
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

    /// `c`'s raw entry; `c` not held is `Corrupt`.
    pub(crate) fn value(&mut self, c: u32) -> Result<u32, FsError<B::Error>> {
        self.entry(self.held(c)?)
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
        self.write_out()
            .map_err(|(Landed::Nowhere(e) | Landed::Fat1(e))| FsError::Dev(e))
    }

    /// `flush`, saying whether FAT 1 took the sector when a copy failed.
    /// A flush that lands in FAT 1 with `fresh`'s link clears it.
    fn write_out(&mut self) -> Result<(), Landed<B::Error>> {
        let Some(sector) = self.cache.sector.filter(|_| self.cache.dirty) else {
            return Ok(());
        };
        self.cache.dirty = false;
        let mut r = Ok(());
        for (i, copy) in self.layout.fat_copies(sector).enumerate() {
            if let Err(e) = self.blocks.write(copy, &self.cache.buf) {
                r = Err(if i > 0 {
                    self.cache.stale = Some(Stale(sector));
                    Landed::Fat1(e)
                } else {
                    Landed::Nowhere(e)
                });
                break;
            }
        }
        if !matches!(r, Err(Landed::Nowhere(_)))
            && let Some((p, c)) = self.fresh
            && self.layout.fat_block(p) == sector
            && self.layout.entry(&self.cache.buf, p) == c
        {
            self.fresh = None;
        }
        if r.is_err() {
            self.cache.sector = None;
        }
        r
    }

    /// Writes a dirty sector before `c`'s is loaded, so the orphan can
    /// move on: once FAT 1 holds the clusters freed so far, `c` is the
    /// first the card still holds.
    fn switch_to(&mut self, c: u32, track: bool) -> Result<(), FsError<B::Error>> {
        if !self.cache.dirty || self.cache.sector == Some(self.layout.fat_block(c)) {
            return Ok(());
        }
        let r = self.write_out();
        if track
            && !matches!(r, Err(Landed::Nowhere(_)))
            && let Some((start, _)) = &mut self.cache.orphan
        {
            *start = c;
        }
        r.map_err(|(Landed::Nowhere(e) | Landed::Fat1(e))| FsError::Dev(e))
    }

    /// Copies the `stale` sector from FAT 1 to every other FAT, then
    /// forgets it; before an operation's first FAT change. A sector outside
    /// this volume's FAT 1 is forgotten unwritten. A failed write keeps it.
    pub fn heal(&mut self) -> Result<(), FsError<B::Error>> {
        self.drop_orphan()?;
        let Some(Stale(sector)) = self.cache.stale else {
            return Ok(());
        };
        if !self.layout.fat1().contains(&sector) {
            self.cache.stale = None;
            return Ok(());
        }
        self.copy_fat1(sector)?;
        self.cache.stale = None;
        Ok(())
    }

    /// Copies FAT 1's `sector` over every other FAT. A failed write
    /// empties the cache.
    pub(crate) fn copy_fat1(&mut self, sector: u32) -> Result<(), FsError<B::Error>> {
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
        Ok(())
    }

    /// Frees the chain from `start`, which no entry holds since a device
    /// error stopped its write, as far as the card has it: what the cache
    /// held unflushed is dropped first, so the walk ends where the card's
    /// copy does. A pending `fresh` cluster goes too if the walk ends on
    /// its `prev` (#362). A card that refuses keeps what is left for the
    /// next `heal`, and the error returns. An orphan still unhealed (no
    /// operation reaches here before `heal`) is kept, and this chain left
    /// for the sweep: `Corrupt`.
    pub fn drop_chain(&mut self, start: u32) -> Result<(), FsError<B::Error>> {
        let fresh = self.fresh.take();
        self.discard();
        if self.cache.orphan.is_some() {
            debug_assert!(false, "an orphan unhealed");
            return Err(FsError::Corrupt);
        }
        self.cache.orphan = Some((start, fresh));
        self.drop_orphan().inspect_err(|_| self.discard())
    }

    /// Frees the orphan's chain, then its pending cluster if the walk
    /// freed that cluster's `prev`, ended there on an end mark, and the
    /// cluster still reads end-of-chain: its link never landed, and
    /// nothing else holds it. Every FAT sector FAT 1 takes moves the
    /// orphan on to the first cluster the card still holds, so a failure
    /// partway strands nothing; `None` once all of it is in FAT 1.
    fn drop_orphan(&mut self) -> Result<(), FsError<B::Error>> {
        let Some((start, pending)) = self.cache.orphan else {
            return Ok(());
        };
        let freed = self.walk_free(start, true)?;
        if let Some((p, c)) = pending
            && freed.tail == Some(p)
            && freed.ended
            && self.layout.holds(c)
        {
            self.switch_to(c, true)?;
            let v = self.entry(c)?;
            if self.layout.as_link(v) == Link::End {
                self.set(c, FREE)?;
            }
        }
        match self.write_out() {
            Ok(()) => {
                self.cache.orphan = None;
                Ok(())
            }
            Err(Landed::Fat1(e)) => {
                self.cache.orphan = None;
                Err(FsError::Dev(e))
            }
            Err(Landed::Nowhere(e)) => Err(FsError::Dev(e)),
        }
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
    ///
    /// `c` reads end-of-chain before `prev` links it, so a cut never leaves
    /// a live chain pointing at a free cluster. When the two sit in
    /// different FAT sectors, `(prev, c)` stays `fresh` until the link is
    /// in FAT 1; one still pending is flushed first.
    pub fn alloc(&mut self, from: u32, prev: Option<u32>) -> Result<u32, FsError<B::Error>> {
        if let Some(p) = prev {
            self.held(p)?;
        }
        if self.fresh.is_some() {
            self.flush()?;
            debug_assert!(self.fresh.is_none(), "a pending link no flush carried");
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
                    if self.layout.fat_block(p) != self.layout.fat_block(c) {
                        self.fresh = Some((p, c));
                    }
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
    /// most `clusters` steps. A loop stops where it meets a cluster it
    /// already freed. A cluster marked bad or reserved is bad media, not
    /// the chain's: it stays.
    pub fn free_chain(&mut self, start: u32) -> Result<Freed, FsError<B::Error>> {
        self.walk_free(start, false)
    }

    /// `free_chain`; with `track`, each sector FAT 1 takes moves the
    /// orphan's start on.
    fn walk_free(&mut self, start: u32, track: bool) -> Result<Freed, FsError<B::Error>> {
        let mut c = start;
        let mut freed = Freed {
            n: 0,
            tail: None,
            ended: false,
        };
        while freed.n < self.layout.clusters() && self.layout.holds(c) {
            self.switch_to(c, track)?;
            let v = self.entry(c)?;
            if v == FREE || self.layout.is_reserved(v) {
                break;
            }
            self.set(c, FREE)?;
            (freed.n, freed.tail) = (freed.n + 1, Some(c));
            match self.layout.as_link(v) {
                Link::Next(next) => c = next,
                Link::End => {
                    freed.ended = true;
                    break;
                }
                Link::Broken => break,
            }
        }
        Ok(freed)
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
