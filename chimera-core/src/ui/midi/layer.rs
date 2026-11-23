// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Each Part's controller map (ADR 0226, 0323): one flat list of up to 90
//! rows, held in a pool of shared slots in AXI (`MapPool`), read from the
//! card when a Part picks it, written back when edited, the card's maps
//! listed for MIDI MAP's pick, and the save, rename and delete run as card
//! work. A Part plays its map through 16 seats (`Layers::layers`), the
//! binding slots after its Sound's own: a map change seats its ECHO rows
//! first, then rows in order; a row heard unseated takes the resting seat
//! heard least recently (`UiState::seat_where`). Bank 2's, as `ui::midi`
//! is.

// Without `midi-map` the door never reads a map: none of this runs.
#![cfg_attr(not(feature = "midi-map"), allow(dead_code, unused_imports))]

use core::fmt::Write;

use chimera_hal::store::{Store, StoreError, VolumeId};

use super::super::UiState;
use crate::hw::MAX_PARTS;
use crate::midi_map::ctl::{self, ControllerMap, FileBuf, MAX_MAPS};
use crate::midi_map::{Binding, Feedback, MAP_LEN, MapFull, MidiMap};
use crate::name::SoundName;
use crate::params::EngineType;
use crate::project::{Line, PartId};
use crate::storage::{FileError, Generation, LoadError, Ready, SaveError};

/// Each Part's seats; none without `midi-map` (the bench builds, deaf).
const LAYERS: usize = if cfg!(feature = "midi-map") {
    MAX_PARTS
} else {
    0
};

/// A Part's seats: the binding slots after its Sound's 16.
pub const SEATS: usize = MAP_LEN;
const _: () = assert!(SEATS <= 16, "`Layers::seat`'s resting set is a `u16`");

/// No seats: a named static, so its image is bank 2's.
static NO_LAYER: MidiMap = MidiMap::EMPTY;

/// What a Part's map was read for: the map, the engine (its seats), and
/// the read count then.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Read {
    name: Option<SoundName>,
    engine: EngineType,
    saves: u8,
}

/// A map's name as a queued rename carries it: FNV-1a 32 of its padded
/// bytes, not the name, so the job and the editor's act are no wider than
/// the widest they were (the act queue is ITCM's). Bytes, to align as a
/// name does. Two names of one mark: the rename takes the other map only
/// if the Part also moved to it while the job waited.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MapMark([u8; 4]);

impl MapMark {
    /// `n`'s mark. Inlined: the editor's check takes it in its own slot.
    #[inline(always)]
    pub(crate) fn of(n: SoundName) -> MapMark {
        // A counted loop: an array's iterator is an instance bank 1 holds.
        let (bytes, mut h, mut i) = (n.padded(), 0x811C_9DC5u32, 0);
        while i < bytes.len() {
            h = (h ^ u32::from(bytes[i])).wrapping_mul(0x0100_0193);
            i += 1;
        }
        MapMark(h.to_le_bytes())
    }
}

/// Card work MIDI MAP asked for.
#[derive(Clone, Copy)]
pub enum CtlJob {
    /// The Part's rows saved into map `name`, which the Part then picks.
    Save(PartId, SoundName),
    /// Row `b` dropped from the Part's map: done at once in the map the
    /// Parts share (`ctl_ask_in`), the card's copy written by card work.
    Drop(PartId, Binding),
    /// The Part's map renamed: every Part on it follows.
    Rename(PartId, SoundName),
    /// Map `.1`, the one DELETE named, deleted from the card: every Part on
    /// it plays NONE.
    Delete(PartId, SoundName),
    /// The browser editor's rename (ADR 0289), asked of the map marked
    /// `.2`: the Part on another by the card work, nothing is renamed.
    RenameOf(PartId, SoundName, MapMark),
}

/// A pool slot's index, below `MAX_PARTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SlotIx(u8);

impl SlotIx {
    const fn get(self) -> usize {
        self.0 as usize
    }
}

/// What a pool slot holds: its map's name, the read count it was read at,
/// the card's generation it matches, and whether the card's copy is behind
/// it. Always written whole by `pool_at` (`None`) or a read: never read
/// from the zeroed static, where a niche in `Option<Held>` could make all
/// zero a `Some`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Held {
    name: SoundName,
    saves: u8,
    dirty: bool,
    generation: Option<Generation>,
}

/// The maps the Parts play (ADR 0323): one slot a Part at most, so a pick
/// never runs out; Parts on one map share its slot. And the file buffer
/// every card read and write of a map goes through. A fixed static in AXI
/// (`MidiSlot::take`), about 6 KB.
pub struct MapPool {
    maps: [ControllerMap; MAX_PARTS],
    held: [Option<Held>; MAX_PARTS],
    /// Each Part's slot; `None`, no map.
    at: [Option<SlotIx>; MAX_PARTS],
    buf: FileBuf,
    /// The map a Part on none plays: empty, never written. Here, not a
    /// static, so a read of it names no bank-2 symbol the outliner can
    /// share into bank 1 (ADR 0175).
    none: ControllerMap,
}

/// The pool empty, built at `p`.
///
/// # Safety
/// `p` is valid for a write of `MapPool`.
#[inline(never)]
pub(super) unsafe fn pool_at(p: *mut MapPool) {
    use core::ptr::addr_of_mut;
    // SAFETY: each field of the caller's `*p`, written whole; the maps and
    // the buffer are plain bytes, all zero empty.
    unsafe {
        core::ptr::write_bytes(addr_of_mut!((*p).maps), 0, 1);
        core::ptr::write_bytes(addr_of_mut!((*p).buf), 0, 1);
        core::ptr::write_bytes(addr_of_mut!((*p).none), 0, 1);
        super::fill(
            addr_of_mut!((*p).held).cast::<Option<Held>>(),
            MAX_PARTS,
            None,
        );
        super::fill(
            addr_of_mut!((*p).at).cast::<Option<SlotIx>>(),
            MAX_PARTS,
            None,
        );
    }
}

impl MapPool {
    /// Part `p`'s map; an empty one on none.
    #[inline(always)]
    pub fn map(&self, p: PartId) -> &ControllerMap {
        match self.at[p.index()] {
            Some(i) => &self.maps[i.get()],
            None => &self.none,
        }
    }

    /// The file buffer a card read or write goes through.
    pub(in crate::ui) fn buf(&mut self) -> &mut FileBuf {
        &mut self.buf
    }

    /// The slot holding map `n`, read at `saves`.
    fn holding(&self, n: SoundName, saves: u8) -> Option<SlotIx> {
        (0..MAX_PARTS)
            .find(|&i| self.held[i].is_some_and(|h| h.name == n && h.saves == saves))
            .map(|i| SlotIx(i as u8))
    }

    /// Whether a Part other than `p` plays slot `i`.
    fn shared(&self, i: usize, p: PartId) -> bool {
        PartId::ALL
            .iter()
            .any(|&q| q != p && self.at[q.index()] == Some(SlotIx(i as u8)))
    }

    /// A slot Part `p` may read a map into: its own unless another Part
    /// shares it, else one no Part plays, a clean one first. Six slots for
    /// six Parts: there is always one. Plain loops: a closure's instance is
    /// one the outliner can leave in bank 1 (ADR 0175).
    fn free_for(&self, p: PartId) -> SlotIx {
        let mut any = p.index();
        let mut found = false;
        let mut i = 0;
        while i < MAX_PARTS {
            if !self.shared(i, p) {
                let dirty = matches!(self.held[i], Some(Held { dirty: true, .. }));
                if !dirty {
                    return SlotIx(i as u8);
                }
                if !found {
                    (any, found) = (i, true);
                }
            }
            i += 1;
        }
        // At most five other Parts: a slot none of them plays is there.
        debug_assert!(found, "a slot for Part {}", p.index());
        SlotIx(any as u8)
    }

    /// The slot whose card copy is behind, if any.
    fn dirty(&self) -> Option<(usize, SoundName)> {
        (0..MAX_PARTS).find_map(|i| match self.held[i] {
            Some(h) if h.dirty => Some((i, h.name)),
            _ => None,
        })
    }

    /// The Parts on slot `k`, a bit a Part.
    fn on(&self, k: SlotIx) -> u8 {
        PartId::ALL
            .iter()
            .filter(|q| self.at[q.index()] == Some(k))
            .fold(0, |b, q| b | 1 << q.index())
    }

    /// Part `p`'s map edited by `f` in place (`true`: it changed), marked
    /// for the card: the Parts on it, a bit a Part. No map: none.
    fn edit(
        &mut self,
        p: PartId,
        f: impl FnOnce(&mut ControllerMap) -> Result<bool, MapFull>,
    ) -> Result<u8, MapFull> {
        let Some(i) = self.at[p.index()] else {
            return Ok(0);
        };
        if !f(&mut self.maps[i.get()])? {
            return Ok(0);
        }
        if let Some(h) = &mut self.held[i.get()] {
            h.dirty = true;
        }
        Ok(self.on(i))
    }
}

/// Each Part's seats and what its map was read for, the card's maps as
/// last listed, and the job waiting.
#[derive(Clone, Copy)]
pub struct Layers {
    /// Each Part's seated map rows.
    layers: [MidiMap; LAYERS],
    /// When each seat was last taken or heard, by `tick`.
    stamp: [[u16; SEATS]; LAYERS],
    tick: u16,
    read: [Option<Read>; MAX_PARTS],
    /// A bit a Part whose last read found its map: only such a map gone
    /// since was removed (#905); a name the card never held stays.
    found: u8,
    /// Moves when every map should be read again: another card, a `put`.
    saves: u8,
    /// `maps_put` when last seen: a console `put` or `rm` since reads every
    /// map again.
    puts: u8,
    pub(in crate::ui) list: [Option<SoundName>; MAX_MAPS],
    pub(in crate::ui) listed: bool,
    /// The card holds more maps than `list`.
    pub(in crate::ui) more: bool,
    pub(in crate::ui) job: Option<CtlJob>,
    /// The card last worked on: another reads every map again.
    vol: Option<VolumeId>,
    /// The last card work found no card: an edited map waits for the next
    /// card work another need runs, so no pass mounts for it alone.
    no_card: bool,
}

/// Nothing read, built at `p`: a few stores, not a static's image (ADR
/// 0251).
///
/// # Safety
/// `p` is valid for a write of `Layers`.
#[inline(never)]
pub(super) unsafe fn rest_at(p: *mut Layers) {
    use core::ptr::addr_of_mut;
    // SAFETY: each field of the caller's `*p`, written whole.
    unsafe {
        let maps = addr_of_mut!((*p).layers).cast::<MidiMap>();
        for i in 0..size_of::<[MidiMap; LAYERS]>() / size_of::<MidiMap>() {
            MidiMap::empty_at(maps.add(i));
        }
        core::ptr::write_bytes(addr_of_mut!((*p).stamp), 0, 1);
        addr_of_mut!((*p).tick).write(0);
        super::fill(
            addr_of_mut!((*p).read).cast::<Option<Read>>(),
            MAX_PARTS,
            None,
        );
        addr_of_mut!((*p).found).write(0);
        addr_of_mut!((*p).saves).write(0);
        addr_of_mut!((*p).puts).write(crate::midi_map::maps_put());
        super::fill(
            addr_of_mut!((*p).list).cast::<Option<SoundName>>(),
            MAX_MAPS,
            None,
        );
        addr_of_mut!((*p).listed).write(false);
        addr_of_mut!((*p).more).write(false);
        addr_of_mut!((*p).job).write(None);
        addr_of_mut!((*p).vol).write(None);
        addr_of_mut!((*p).no_card).write(false);
    }
}

/// The words a save or a delete says, `|` between: a named static, so its
/// bytes are bank 2's.
static WORDS: [u8; 118] =
    *b"SAVED |MAP FULL|CONTROLLER|NONE|IN |NO CONTROLLER MAP|NAME TAKEN|CARD HOLDS MORE: 16 SHOWN|\
MIX+EDIT RENAME|MAP CHANGED";

#[derive(Clone, Copy)]
#[repr(u8)]
pub(super) enum W {
    Saved,
    Full,
    Controller,
    None,
    In,
    NoMap,
    Taken,
    More,
    Whole,
    Changed,
}

#[inline(never)]
pub(super) fn word(w: W) -> &'static str {
    super::pick(&WORDS, w as u8)
}

impl Layers {
    /// Part `i`'s seats.
    pub fn layer(&self, i: usize) -> &MidiMap {
        self.layers.get(i).unwrap_or(&NO_LAYER)
    }

    /// The maps listed, `None` before a listing. Inlined: MIDI MAP's
    /// keys and the editor's call it, in two bank-2 groups.
    #[inline(always)]
    pub fn listed(&self) -> Option<&[Option<SoundName>]> {
        self.listed
            .then(|| &self.list[..self.list.iter().take_while(|n| n.is_some()).count()])
    }

    /// Seat `k` of Part `i` taken or heard now.
    pub(super) fn touch(&mut self, i: usize, k: usize) {
        self.tick = self.tick.wrapping_add(1);
        if let Some(s) = self.stamp.get_mut(i).and_then(|s| s.get_mut(k)) {
            *s = self.tick;
        }
    }

    /// Row `b` seated on Part `i`: a free seat, else of the seats resting
    /// (a bit a seat in `rests`) the one heard least recently. Its seat,
    /// `None` with none. Plain loops: no closure instance (ADR 0175).
    pub(super) fn seat(&mut self, i: usize, b: Binding, rests: u16) -> Option<usize> {
        let tick = self.tick;
        let s = *self.stamp.get(i)?;
        let l = self.layers.get_mut(i)?;
        let k = if l.len() < SEATS && l.learn(b.input, b.target).is_ok() {
            l.len() - 1
        } else {
            let (mut best, mut age) = (None, 0u16);
            let mut k = 0;
            while k < l.len() {
                let a = tick.wrapping_sub(s[k]);
                if rests >> k & 1 != 0 && (best.is_none() || a > age) {
                    (best, age) = (Some(k), a);
                }
                k += 1;
            }
            best?
        };
        self.layers.get_mut(i)?.set(k, b);
        self.touch(i, k);
        Some(k)
    }
}

/// Part `p`'s seats as a map change leaves them: its ECHO rows first (an
/// echo needs a seat), then its rows in order, live on `engine` and not on
/// a control `own` holds, up to `SEATS`.
fn seats_of(m: &ControllerMap, own: &MidiMap, engine: EngineType) -> MidiMap {
    let mut l = MidiMap::EMPTY;
    for echo in [true, false] {
        for b in m.rows() {
            if l.len() >= SEATS {
                return l;
            }
            if (b.feedback == Feedback::Value) == echo
                && b.target.live(engine)
                && !shadowed(own, &b)
                && l.learn(b.input, b.target).is_ok()
            {
                l.set(l.len() - 1, b);
            }
        }
    }
    l
}

/// What a Part's map read did with the card.
enum Attach {
    /// Shared or emptied: no card read.
    NoRead,
    /// One card read this pass.
    Read,
    /// The card was busy: nothing changed.
    Busy,
}

#[cfg(feature = "midi-map")]
impl UiState {
    /// What Part `p`'s map should be read for now.
    fn ctl_want(&self, p: PartId, saves: u8) -> Read {
        Read {
            name: self.project.controller(p),
            engine: self.project.part(p).sound.params.engine(),
            saves,
        }
    }

    /// Part `p`'s map as it plays: an empty one before the door ran.
    /// Inlined: a shared call into bank 2 is what the outliner moves into
    /// bank 1 (ADR 0175).
    #[inline(always)]
    pub(in crate::ui) fn part_map(&self, p: PartId) -> &ControllerMap {
        match self.cc.pool() {
            Some(pool) => pool.map(p),
            // SAFETY: `take` points `maps` at the slot's pool, zeroed at
            // boot; its `none` is plain bytes, all zero an empty map, and
            // never written. Only that field is read before a build.
            None => unsafe { &*core::ptr::addr_of!((*self.cc.maps).none) },
        }
    }

    /// Part `p`'s seats filled again from its map: ECHO rows first, then
    /// rows in order (`seats_of`); its slots at rest if they changed.
    pub(in crate::ui) fn reseat(&mut self, p: PartId) {
        let s = &self.project.part(p).sound;
        let (own, engine) = (s.map, s.params.engine());
        let (ui, pool) = self.cc.split();
        let l = seats_of(pool.map(p), &own, engine);
        // Seats as they were: nothing moving is stopped.
        match ui.ctl.layers.get_mut(p.index()) {
            Some(x) if !same_rows(x, &l) => *x = l,
            Some(_) | None => return,
        }
        self.cc.reset_part(p);
    }

    /// The Parts in `parts` (a bit a Part) reseated: their map changed.
    pub(in crate::ui) fn reseat_all(&mut self, parts: u8) {
        for p in PartId::ALL {
            if parts >> p.index() & 1 != 0 {
                self.reseat(p);
            }
        }
    }

    /// Part `p`'s map `f`-edited in place (`true`: changed): the card's
    /// copy written by the next card work, every Part on it reseated.
    /// `MAP FULL` said. Whether it changed.
    pub(in crate::ui) fn map_edit(
        &mut self,
        p: PartId,
        f: impl FnOnce(&mut ControllerMap) -> Result<bool, MapFull>,
    ) -> bool {
        let (_, pool) = self.cc.split();
        match pool.edit(p, f) {
            Ok(parts) => {
                self.reseat_all(parts);
                parts != 0
            }
            Err(MapFull) => {
                self.error(Line::new(word(W::Full)));
                false
            }
        }
    }

    /// Row `k` of Part `p`'s map made `b` (`map_edit`). The one call of
    /// `ControllerMap::set` for a row edit: three callers sharing its call
    /// set-up are what the outliner moves into bank 1 (ADR 0175).
    #[inline(never)]
    pub(in crate::ui) fn map_row_set(&mut self, p: PartId, k: usize, b: Binding) -> bool {
        self.map_edit(p, |m| m.set(k, b).map(|()| true))
    }

    /// Card work waits: a job, a map to read or write, or MIDI MAP open
    /// unlisted. The door's `ctl_due`.
    // Out of line: the editor's wrappers (`ed_cue::ctl_due`, `ctl_work`)
    // call it, and it stays in the MIDI UI's group.
    #[inline(never)]
    pub(in crate::ui) fn ctl_due_in(&self) -> bool {
        let mapping = self.screen() == Some(super::super::settings::Screen::MidiMap);
        let Some(m) = self.cc.built() else {
            return mapping
                || PartId::ALL
                    .iter()
                    .any(|&p| self.project.controller(p).is_some());
        };
        let c = &m.ctl;
        c.job.is_some()
            || (!c.no_card && self.cc.pool().is_some_and(|p| p.dirty().is_some()))
            || c.puts != crate::midi_map::maps_put()
            || (!c.listed && mapping)
            || PartId::ALL
                .iter()
                .any(|&p| c.read[p.index()] != Some(self.ctl_want(p, c.saves)))
    }

    /// Part `p` onto the map `want` names: its seats again if only the
    /// engine moved, a slot another Part shares, else a read into a free
    /// slot (with a card). A map gone from the card since it was found
    /// puts the Part on NONE (#905).
    fn ctl_attach(
        &mut self,
        p: PartId,
        want: Read,
        card: Option<(&mut dyn Store, &Ready)>,
    ) -> Attach {
        let i = p.index();
        let was = self.cc.ui().ctl.read[i];
        let same = was.is_some_and(|w| w.name == want.name && w.saves == want.saves);
        let pool = self.cc.split().1;
        let at = match want.name {
            None => None,
            Some(_) if same => pool.at[i],
            Some(n) => match (pool.holding(n, want.saves), card) {
                (Some(k), _) => {
                    // Held: found, as a read would have (#905).
                    self.cc.ui().ctl.found |= 1 << i;
                    Some(k)
                }
                (None, None) => None,
                (None, Some((s, r))) => return self.ctl_read(p, n, want, was, s, r),
            },
        };
        self.cc.split().1.at[i] = at;
        self.cc.ui().ctl.read[i] = Some(want);
        self.reseat(p);
        Attach::NoRead
    }

    /// Part `p`'s map `n` read from the card into a free slot.
    fn ctl_read(
        &mut self,
        p: PartId,
        n: SoundName,
        want: Read,
        was: Option<Read>,
        s: &mut dyn Store,
        r: &Ready,
    ) -> Attach {
        let i = p.index();
        let pool = self.cc.split().1;
        let k = pool.free_for(p);
        let read = {
            let MapPool { maps, buf, .. } = &mut *pool;
            ctl::load(s, r, &n, buf, &mut maps[k.get()])
        };
        if let Err(LoadError::Store(StoreError::Busy)) = read {
            return Attach::Busy;
        }
        let bit = 1 << i;
        let c = &mut self.cc.ui().ctl;
        if let Err(LoadError::Missing) = read
            && c.found & bit != 0
            && was.is_some_and(|w| w.name == want.name)
        {
            self.project.set_controller(p, None);
            return Attach::Read;
        }
        c.found = c.found & !bit | if read.is_ok() { bit } else { 0 };
        c.read[i] = Some(want);
        let pool = self.cc.split().1;
        let generation = match read {
            Ok(_) => ctl::generation(s, r, &n, &mut pool.buf).ok().flatten(),
            Err(_) => None,
        };
        pool.held[k.get()] = read.is_ok().then_some(Held {
            name: n,
            saves: want.saves,
            dirty: false,
            generation,
        });
        pool.at[i] = read.is_ok().then_some(k);
        self.reseat(p);
        Attach::Read
    }

    /// One pass of card work (`ctl_due`): the job, else a map's write, else
    /// one Part's read, else the listing. Another card in the slot reads
    /// every map again. No card (`None`): a Part shares a map another
    /// holds, else plays none; the job dropped and the listing empty, so no
    /// pass mounts again for them. The door's `ctl_work`.
    // Out of line: the editor's wrappers (`ed_cue::ctl_due`, `ctl_work`)
    // call it, and it stays in the MIDI UI's group.
    #[inline(never)]
    pub(in crate::ui) fn ctl_work_in(&mut self, s: &mut dyn Store, r: Option<&Ready>) {
        let Some(r) = r else {
            let saves = self.cc.ui().ctl.saves;
            for p in PartId::ALL {
                let want = self.ctl_want(p, saves);
                if self.cc.ui().ctl.read[p.index()] != Some(want) {
                    self.ctl_attach(p, want, None);
                }
            }
            let c = &mut self.cc.ui().ctl;
            (c.job, c.list, c.listed, c.more) = (None, [None; MAX_MAPS], true, false);
            c.puts = crate::midi_map::maps_put();
            c.no_card = true;
            return;
        };
        let c = &mut self.cc.ui().ctl;
        c.no_card = false;
        let puts = crate::midi_map::maps_put();
        let swapped = c.vol.replace(r.volume()).is_some_and(|v| v != r.volume());
        if swapped {
            // Another card: its maps are new reads, never a removal.
            c.found = 0;
        }
        if swapped || c.puts != puts {
            c.puts = puts;
            c.saves = c.saves.wrapping_add(1);
            c.listed = false;
        }
        if let Some(j) = self.cc.ui().ctl.job.take() {
            return self.ctl_job(s, r, j);
        }
        // An edited map's card copy, one a pass, before any read.
        let pool = self.cc.split().1;
        if let Some((k, n)) = pool.dirty() {
            let was = pool.held[k].and_then(|h| h.generation);
            let MapPool {
                maps, buf, held, ..
            } = &mut *pool;
            // A map gone from the card since it was read is never written
            // back (FILE NOT FOUND); one put over it since (a console
            // `put`, another generation) is the card's: the edit gives way.
            let got = match ctl::generation(s, r, &n, buf) {
                Ok(Some(g)) if was.is_none_or(|w| w == g) => ctl::save(s, r, &n, buf, &maps[k])
                    .map(|()| ctl::generation(s, r, &n, buf).ok().flatten()),
                Ok(Some(_)) => Ok(None),
                Ok(None) => Err(SaveError::Store(StoreError::NotFound)),
                Err(LoadError::Store(e)) => Err(SaveError::Store(e)),
                Err(_) => Err(SaveError::Store(StoreError::NotFound)),
            };
            if let Err(SaveError::Store(StoreError::Busy)) = got {
                return;
            }
            if let Some(h) = &mut held[k] {
                h.dirty = false;
                h.generation = got.ok().flatten();
            }
            if let Err(e) = got {
                self.error(Line::new(save_words(e)));
            }
            return;
        }
        let saves = self.cc.ui().ctl.saves;
        for p in PartId::ALL {
            let want = self.ctl_want(p, saves);
            if self.cc.ui().ctl.read[p.index()] == Some(want) {
                continue;
            }
            match self.ctl_attach(p, want, Some((&mut *s, r))) {
                Attach::NoRead => {}
                Attach::Read | Attach::Busy => return,
            }
        }
        if self.screen() == Some(super::super::settings::Screen::MidiMap) {
            let (ui, pool) = self.cc.split();
            let c = &mut ui.ctl;
            c.list = [None; MAX_MAPS];
            let got = ctl::list(s, r, &mut pool.buf, &mut c.list);
            c.more = matches!(got, Ok((_, true)));
            c.listed = !matches!(got, Err(StoreError::Busy));
        }
    }

    /// The editor's rename, asked of the map marked `was`, is still
    /// asked of the map the Part is on, `now`: the Part on another, or on
    /// none, it renames nothing and the box says MAP CHANGED.
    #[inline(never)]
    fn held(&mut self, now: Option<SoundName>, was: MapMark) -> bool {
        match now {
            Some(n) if MapMark::of(n) == was => true,
            Some(_) | None => {
                self.error(Line::new(word(W::Changed)));
                false
            }
        }
    }

    fn ctl_job(&mut self, s: &mut dyn Store, r: &Ready, j: CtlJob) {
        let p = match j {
            CtlJob::Save(p, _)
            | CtlJob::Drop(p, _)
            | CtlJob::Rename(p, _)
            | CtlJob::RenameOf(p, ..)
            | CtlJob::Delete(p, _) => p,
        };
        let now = self.project.controller(p);
        if let CtlJob::RenameOf(_, _, of) = j
            && !self.held(now, of)
        {
            return;
        }
        let i = p.index();
        let saves = self.cc.ui().ctl.saves;
        let own = self.project.part(p).sound.map;
        let pool = self.cc.split().1;
        // The Parts a map moved under (a bit a Part), and what they move to.
        let got = match j {
            // Done at once by `ctl_ask_in`; never queued.
            CtlJob::Drop(..) => return,
            CtlJob::Delete(_, n) => job_delete(s, r, pool, n).map(|()| (Some(n), None)),
            CtlJob::Rename(_, n) | CtlJob::RenameOf(_, n, _) => match now {
                Some(was) if was != n => {
                    job_rename(s, r, pool, i, was, n).map(|()| (Some(was), Some(n)))
                }
                _ => return,
            },
            CtlJob::Save(_, n) => job_save(s, r, pool, p, saves, now, n, &own).map(|parts| {
                self.project.set_controller(p, Some(n));
                self.cc.ui().ctl.read[i] = Some(self.ctl_want(p, saves));
                if !own.is_empty() {
                    self.set_map(p, MidiMap::EMPTY);
                }
                self.reseat_all(parts | 1 << i);
                let mut l = Line::new(word(W::Saved));
                let _ = l.write_str(n.as_str());
                self.notice(l);
                (None, None)
            }),
        };
        match got {
            Ok((Some(from), to)) => {
                for q in PartId::ALL {
                    if self.project.controller(q) == Some(from) {
                        self.project.set_controller(q, to);
                        // Its slot follows: no read again.
                        self.cc.ui().ctl.read[q.index()] = Some(self.ctl_want(q, saves));
                    }
                }
                self.cc.ui().ctl.listed = false;
            }
            Ok((None, _)) => self.cc.ui().ctl.listed = false,
            Err(e) => {
                // A failed save may have read over the Part's slot: its map
                // is read again (the card's copy is whole).
                if let CtlJob::Save(..) = j {
                    self.cc.ui().ctl.read[i] = None;
                }
                match e {
                    JobErr::Busy => self.cc.ui().ctl.job = Some(j),
                    JobErr::Say(l) => self.error(l),
                }
            }
        }
    }

    /// MIDI MAP's job for the next card work: a save, a rename or the
    /// map's delete. One waits at a time: while another waits for the
    /// card, this one is refused on screen, never dropped unsaid. A row's
    /// drop is done here, in the map the Parts share. The door's
    /// `ctl_ask`.
    pub(in crate::ui) fn ctl_ask_in(&mut self, j: CtlJob) {
        if let CtlJob::Drop(p, b) = j {
            self.map_edit(p, |m| m.change(&b, None));
            return;
        }
        let c = &mut self.cc.ui().ctl;
        if c.job.is_some() {
            return self.error(Line::new(StoreError::Busy.message()));
        }
        c.job = Some(j);
    }

    /// Part `p`'s pick `d` steps along NONE and the listed maps.
    pub(super) fn ctl_pick(&mut self, p: PartId, d: i8) {
        let Some(list) = self.cc.ui().ctl.listed() else {
            return;
        };
        let now = self.project.controller(p);
        let at = match now {
            None => 0,
            Some(n) => list.iter().position(|&x| x == Some(n)).map_or(0, |i| i + 1),
        };
        let to = (at as i32 + i32::from(d)).clamp(0, list.len() as i32) as usize;
        let next = to.checked_sub(1).and_then(|i| list[i]);
        // A name the card lacks sits at NONE's place: B down reaches NONE.
        if next != now {
            self.project.set_controller(p, next);
        }
    }
}

/// `a` and `b` hold the same rows, compared by their codes (`page::same`)
/// and targets: no `Input` equality instance, which bank 1 would hold.
#[cfg(feature = "midi-map")]
#[inline(always)]
pub(in crate::ui) fn same_row(a: &Binding, b: &Binding) -> bool {
    a.target == b.target && super::page::same(a, b)
}

/// `a` and `b`, two seat lists, alike row for row.
#[cfg(feature = "midi-map")]
fn same_rows(a: &MidiMap, b: &MidiMap) -> bool {
    let (a, b) = (a.entries(), b.entries());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if !same_row(&a[i], &b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// The Part's row `b` is shadowed by one of `own`'s on its control. A
/// plain loop: an iterator adaptor's instance, named by its closure, is
/// placed beside it and can be left out of line where a caller isn't.
#[inline(always)]
pub fn shadowed(own: &MidiMap, b: &Binding) -> bool {
    let e = own.entries();
    let mut i = 0;
    while i < e.len() {
        if e[i].input.same_control(&b.input) {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(feature = "midi-map")]
fn load_words(e: LoadError) -> &'static str {
    match e {
        LoadError::Store(e) => e.message(),
        LoadError::File(e) => e.message(),
        LoadError::Missing | LoadError::Busy => StoreError::NotFound.message(),
    }
}

#[cfg(feature = "midi-map")]
fn save_words(e: SaveError) -> &'static str {
    match e {
        // Another map holds the name's stem: it stays.
        SaveError::File(FileError::BadName) => word(W::Taken),
        SaveError::File(FileError::Bounds) => word(W::Full),
        SaveError::Store(e) => e.message(),
        SaveError::File(e) => e.message(),
        SaveError::Damaged => StoreError::Corrupt.message(),
    }
}

/// Why a map job stopped: the card busy (it runs again), or words to say.
#[cfg(feature = "midi-map")]
enum JobErr {
    Busy,
    Say(Line),
}

#[cfg(feature = "midi-map")]
fn load_err(e: LoadError) -> JobErr {
    match e {
        LoadError::Store(StoreError::Busy) => JobErr::Busy,
        e => JobErr::Say(Line::new(load_words(e))),
    }
}

#[cfg(feature = "midi-map")]
fn save_err(e: SaveError) -> JobErr {
    match e {
        SaveError::Store(StoreError::Busy) => JobErr::Busy,
        e => JobErr::Say(Line::new(save_words(e))),
    }
}

/// Map `n` deleted from the card; the slots holding it forgotten.
#[cfg(feature = "midi-map")]
fn job_delete(
    s: &mut dyn Store,
    r: &Ready,
    pool: &mut MapPool,
    n: SoundName,
) -> Result<(), JobErr> {
    ctl::delete(s, r, &n, &mut pool.buf).map_err(load_err)?;
    for (h, at) in pool.held.iter_mut().zip(0u8..) {
        if h.is_some_and(|h| h.name == n) {
            *h = None;
            for a in &mut pool.at {
                if *a == Some(SlotIx(at)) {
                    *a = None;
                }
            }
        }
    }
    Ok(())
}

/// Part `i`'s map `was` saved as `n`, then `was`'s pair deleted. A rename
/// never writes over a map of its new name.
#[cfg(feature = "midi-map")]
fn job_rename(
    s: &mut dyn Store,
    r: &Ready,
    pool: &mut MapPool,
    i: usize,
    was: SoundName,
    n: SoundName,
) -> Result<(), JobErr> {
    let Some(k) = pool.at[i] else {
        return Err(load_err(LoadError::Missing));
    };
    let MapPool {
        maps, held, buf, ..
    } = pool;
    if ctl::taken(s, r, &n, buf).map_err(load_err)? {
        return Err(JobErr::Say(Line::new(word(W::Taken))));
    }
    ctl::save(s, r, &n, buf, &maps[k.get()]).map_err(save_err)?;
    // The old name's pair goes once the new one is whole. Not retried: the
    // new file would read as taken; a failed delete leaves a copy.
    let _ = ctl::delete(s, r, &was, buf);
    let generation = ctl::generation(s, r, &n, buf).ok().flatten();
    if let Some(h) = &mut held[k.get()] {
        (h.name, h.dirty, h.generation) = (n, false, generation);
    }
    Ok(())
}

/// Part `p`'s own rows `own` saved into map `n`: `n` as a slot or the card
/// holds it, else, a save under a new name, the Part's map (`now`) copied.
/// The Parts on `n` after, a bit a Part.
#[cfg(feature = "midi-map")]
#[allow(clippy::too_many_arguments)]
fn job_save(
    s: &mut dyn Store,
    r: &Ready,
    pool: &mut MapPool,
    p: PartId,
    saves: u8,
    now: Option<SoundName>,
    n: SoundName,
    own: &MidiMap,
) -> Result<u8, JobErr> {
    let i = p.index();
    let k = match pool.holding(n, saves) {
        Some(k) => k,
        None => {
            let k = pool.free_for(p);
            // The Part's map, copied before its slot may be read over.
            let from = pool.at[i]
                .filter(|_| now.is_some())
                .map(|f| pool.maps[f.get()]);
            // Read over from here: whatever the slot held is gone, so a
            // Part on it plays none until its map is read again.
            pool.held[k.get()] = None;
            let mine = pool.at[i] == Some(k);
            if mine {
                pool.at[i] = None;
            }
            let MapPool { maps, buf, .. } = &mut *pool;
            match ctl::load(s, r, &n, buf, &mut maps[k.get()]) {
                Ok(_) => {}
                Err(LoadError::Missing) => match from {
                    Some(f) => maps[k.get()] = f,
                    // SAFETY: the slot is a map's place.
                    None => unsafe { ctl::empty_at(&mut maps[k.get()]) },
                },
                Err(e) => return Err(load_err(e)),
            }
            k
        }
    };
    let mut next = pool.maps[k.get()];
    if next.absorb(own).is_err() {
        return Err(JobErr::Say(Line::new(word(W::Full))));
    }
    ctl::save(s, r, &n, &mut pool.buf, &next).map_err(save_err)?;
    pool.maps[k.get()] = next;
    let generation = ctl::generation(s, r, &n, &mut pool.buf).ok().flatten();
    pool.held[k.get()] = Some(Held {
        name: n,
        saves,
        dirty: false,
        generation,
    });
    pool.at[i] = Some(k);
    Ok(pool.on(k))
}

#[cfg(all(test, feature = "midi-map"))]
mod tests {
    use super::*;
    use crate::addr::{BlockRef, ParamAddr};
    use crate::midi_map::{Input, Mappable};
    use crate::params::FilterParams;

    fn row(cc: u8) -> Binding {
        let t = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
        let mut m = MidiMap::EMPTY;
        m.learn(Input::cc(cc).unwrap(), Mappable::new(t).unwrap())
            .unwrap();
        m.entries()[0]
    }

    /// The editor's rename carries its map's mark in the job, the last
    /// variant, and the job is no wider for it than a drop.
    #[test]
    fn a_marked_rename_is_a_job_no_wider_than_a_drop() {
        let p = PartId::ALL[0];
        let was = MapMark::of(SoundName::new("EC4").unwrap());
        let to = SoundName::new("YOURS").unwrap();
        let j = CtlJob::RenameOf(p, to, was);
        assert!(matches!(j, CtlJob::RenameOf(q, n, m) if q == p && n == to && m == was));
        assert_eq!(
            core::mem::size_of::<CtlJob>(),
            core::mem::size_of_val(&CtlJob::Drop(p, row(21)))
        );
    }
}
