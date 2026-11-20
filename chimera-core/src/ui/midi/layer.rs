//! Each Part's controller layer (ADR 0226): read from the card when it
//! goes stale (a pick, a load, an engine change, a save), the card's maps
//! listed for MIDI MAP's pick, and the save and the row delete run as card
//! work. Bank 2's, as `ui::midi` is; the state is `MidiUi`'s, in AXI.

// Without `midi-map` the door never reads a layer: none of this runs.
#![cfg_attr(not(feature = "midi-map"), allow(dead_code, unused_imports))]

use core::fmt::Write;

use chimera_hal::store::{Store, StoreError, VolumeId};

use super::super::UiState;
use crate::hw::MAX_PARTS;
use crate::midi_map::ctl::{self, MAX_MAPS};
use crate::midi_map::{Binding, MapFull, MidiMap};
use crate::name::SoundName;
use crate::params::EngineType;
use crate::project::{Line, PartId};
use crate::storage::{FileError, LoadError, Ready, SaveError};

/// Each Part's layer; none without `midi-map` (the bench builds, deaf).
const LAYERS: usize = if cfg!(feature = "midi-map") {
    MAX_PARTS
} else {
    0
};

/// No layer: a named static, so its image is bank 2's.
static NO_LAYER: MidiMap = MidiMap::EMPTY;

/// What a Part's layer was read for: the map, the engine, and the save
/// count then.
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
    /// Row `b` dropped from the Part's map.
    Drop(PartId, Binding),
    /// Row `.1` of the Part's map, as the card holds it, made `.2`.
    Edit(PartId, Binding, Binding),
    /// The Part's map renamed: every Part on it follows.
    Rename(PartId, SoundName),
    /// Map `.1`, the one DELETE named, deleted from the card: every Part on
    /// it plays NONE.
    Delete(PartId, SoundName),
}

/// Each Part's layer and what it was read for, the card's maps as last
/// listed, and the job waiting.
#[derive(Clone, Copy)]
pub struct Layers {
    layers: [MidiMap; LAYERS],
    read: [Option<Read>; MAX_PARTS],
    /// A bit a Part whose last read found its map: only such a map gone
    /// since was removed (#905); a name the card never held stays.
    found: u8,
    /// Moves on every save: every layer reads again.
    saves: u8,
    /// `maps_put` when last seen: a console `put` or `rm` since reads every
    /// layer again.
    puts: u8,
    pub(in crate::ui) list: [Option<SoundName>; MAX_MAPS],
    pub(in crate::ui) listed: bool,
    /// The card holds more maps than `list`.
    pub(in crate::ui) more: bool,
    pub(in crate::ui) job: Option<CtlJob>,
    /// The waiting rename is the browser editor's (ADR 0289), asked of
    /// the map so marked: the Part on another by the job, nothing is
    /// renamed. `None`: the box's own. Written by every ask of a rename
    /// and read by a rename alone. Beside `job`, not in it: `CtlJob` is
    /// the door's, and bank 1 builds it.
    of: Option<MapMark>,
    /// The card last worked on: another reads every layer again.
    vol: Option<VolumeId>,
    /// A map read from the card (`ctl_work_in`): here, not on the stack,
    /// where a door entry's depth is charged to every call through a
    /// pointer (`stack_depth.py`); SysEx keys made its rows 12 B (#846).
    /// With the map only: a bench build's slot is AXI's, held to its margin.
    #[cfg(feature = "midi-map")]
    scratch: ctl::ControllerMap,
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
        addr_of_mut!((*p).of).write(None);
        addr_of_mut!((*p).vol).write(None);
        #[cfg(feature = "midi-map")]
        ctl::empty_at(addr_of_mut!((*p).scratch));
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
    /// Part `i`'s layer.
    pub fn layer(&self, i: usize) -> &MidiMap {
        self.layers.get(i).unwrap_or(&NO_LAYER)
    }

    /// Row `i` of Part `p`'s layer, `was`, made `to` here and, by the next
    /// card work, in the map; an edit of the row waiting takes it on.
    /// `false`: another job waits, nothing changed.
    /// Inlined: MIDI MAP's keys and the editor's (`ed_cue`) call it, in
    /// two bank-2 groups.
    #[cfg(feature = "midi-map")]
    #[inline(always)]
    pub(in crate::ui) fn edit(&mut self, p: PartId, i: usize, was: Binding, to: Binding) -> bool {
        let first = match self.job {
            None => was,
            // The same row: the whole binding, as two rows may share a
            // target on two controls.
            Some(CtlJob::Edit(q, first, last))
                if q == p && last.target == was.target && super::page::same(&last, &was) =>
            {
                first
            }
            Some(_) => return false,
        };
        self.job = Some(CtlJob::Edit(p, first, to));
        if let Some(l) = self.layers.get_mut(p.index()) {
            l.set(i, to);
        }
        true
    }

    /// The maps listed, `None` before a listing. Inlined, as `edit`.
    #[inline(always)]
    pub fn listed(&self) -> Option<&[Option<SoundName>]> {
        self.listed
            .then(|| &self.list[..self.list.iter().take_while(|n| n.is_some()).count()])
    }
}

#[cfg(feature = "midi-map")]
impl UiState {
    /// What Part `p`'s layer should be read for now.
    fn ctl_want(&self, p: PartId, saves: u8) -> Read {
        Read {
            name: self.project.controller(p),
            engine: self.project.part(p).sound.params.engine(),
            saves,
        }
    }

    /// Card work waits: a job, a stale layer, or MIDI MAP open unlisted.
    /// The door's `ctl_due`.
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
            || c.puts != crate::midi_map::maps_put()
            || (!c.listed && mapping)
            || PartId::ALL
                .iter()
                .any(|&p| c.read[p.index()] != Some(self.ctl_want(p, c.saves)))
    }

    /// One pass of card work (`ctl_due`): the job, else one stale layer,
    /// else the listing. Another card in the slot reads every layer again.
    /// No card (`None`): every layer empty, read for what it wants, the job
    /// dropped and the listing empty, so no pass mounts again for them.
    /// The door's `ctl_work`.
    // Out of line: the editor's wrappers (`ed_cue::ctl_due`, `ctl_work`)
    // call it, and it stays in the MIDI UI's group.
    #[inline(never)]
    pub(in crate::ui) fn ctl_work_in(&mut self, s: &mut dyn Store, r: Option<&Ready>) {
        let Some(r) = r else {
            let saves = self.cc.ui().ctl.saves;
            for p in PartId::ALL {
                let want = self.ctl_want(p, saves);
                let c = &mut self.cc.ui().ctl;
                c.read[p.index()] = Some(want);
                if !c.layers[p.index()].is_empty() {
                    c.layers[p.index()] = MidiMap::EMPTY;
                    self.cc.reset_part(p);
                }
            }
            let c = &mut self.cc.ui().ctl;
            (c.job, c.list, c.listed, c.more) = (None, [None; MAX_MAPS], true, false);
            c.puts = crate::midi_map::maps_put();
            return;
        };
        let c = &mut self.cc.ui().ctl;
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
        let saves = self.cc.ui().ctl.saves;
        for p in PartId::ALL {
            let want = self.ctl_want(p, saves);
            if self.cc.ui().ctl.read[p.index()] == Some(want) {
                continue;
            }
            let m = &mut self.cc.ui().ctl.scratch;
            // SAFETY: `empty_at` writes the whole map over the last read.
            unsafe { ctl::empty_at(m) };
            let read = want.name.map(|n| ctl::load(s, r, &n, m));
            if let Some(Err(LoadError::Store(StoreError::Busy))) = read {
                // The streams hold the card: the next pass.
                return;
            }
            let c = &mut self.cc.ui().ctl;
            // Found before and gone now: removed over USB (`rm`). The Parts
            // on it play NONE, as after a delete on the unit (#905).
            let bit = 1 << p.index();
            if let Some(Err(LoadError::Missing)) = read
                && c.found & bit != 0
                && c.read[p.index()].is_some_and(|was| was.name == want.name)
            {
                self.project.set_controller(p, None);
                return;
            }
            c.found = c.found & !bit | if matches!(read, Some(Ok(_))) { bit } else { 0 };
            let layer = c.scratch.layer(want.engine).0;
            c.read[p.index()] = Some(want);
            if c.layers[p.index()] != layer {
                c.layers[p.index()] = layer;
                self.cc.reset_part(p);
            }
            // One card read a pass.
            if read.is_some() {
                return;
            }
        }
        if self.screen() == Some(super::super::settings::Screen::MidiMap) {
            let c = &mut self.cc.ui().ctl;
            c.list = [None; MAX_MAPS];
            let got = ctl::list(s, r, &mut c.list);
            c.more = matches!(got, Ok((_, true)));
            c.listed = !matches!(got, Err(StoreError::Busy));
        }
    }

    /// The waiting rename is still asked of the map the Part is on,
    /// `now`: the editor's is held to the map it marked, and the Part on
    /// another, or on none, it renames nothing and the box says MAP
    /// CHANGED. The box's own carries no mark. Out of line: the job's
    /// own code stays as it was laid out.
    #[inline(never)]
    fn held(&mut self, now: Option<SoundName>) -> bool {
        let Some(was) = self.cc.ui().ctl.of else {
            return true;
        };
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
            | CtlJob::Edit(p, ..)
            | CtlJob::Rename(p, _)
            | CtlJob::Delete(p, _) => p,
        };
        let now = self.project.controller(p);
        if let CtlJob::Rename(..) = j
            && !self.held(now)
        {
            return;
        }
        // The map a rename or a delete takes the Parts off.
        let was = match j {
            CtlJob::Delete(_, n) => Some(n),
            _ => now,
        };
        // The name written, and the maps read: a save's own, else the
        // Part's (a save as); every other job the Part's.
        let (name, from) = match (j, now) {
            (CtlJob::Save(_, n), _) => (n, [Some(n), now]),
            (CtlJob::Rename(_, n), Some(was)) if n != was => (n, [Some(was), None]),
            (CtlJob::Rename(..), _) => return,
            (CtlJob::Delete(_, n), _) => (n, [Some(n), None]),
            (_, None) => return,
            (_, Some(n)) => (n, [Some(n), None]),
        };
        let engine = self.project.part(p).sound.params.engine();
        let own = self.project.part(p).sound.map;
        // The map works in the MIDI UI's scratch, not on the stack: this
        // runs under `ctl_work_in`, a door entry whose depth every call
        // through a pointer is charged (`stack_depth.py`).
        let m = &mut self.cc.ui().ctl.scratch;
        match job_on(s, r, j, name, from, &own, engine, m) {
            Ok(()) => {}
            Err(JobErr::Busy) => {
                self.cc.ui().ctl.job = Some(j);
                return;
            }
            Err(JobErr::Say(l)) => return self.error(l),
        }
        let c = &mut self.cc.ui().ctl;
        c.saves = c.saves.wrapping_add(1);
        c.listed = false;
        let to = match j {
            CtlJob::Rename(_, n) => Some(Some(n)),
            CtlJob::Delete(..) => Some(None),
            _ => None,
        };
        if let Some(to) = to {
            for q in PartId::ALL {
                if self.project.controller(q) == was {
                    self.project.set_controller(q, to);
                }
            }
        }
        if let CtlJob::Save(..) = j {
            self.project.set_controller(p, Some(name));
            if !own.is_empty() {
                self.set_map(p, MidiMap::EMPTY);
            }
            let mut l = Line::new(word(W::Saved));
            let _ = l.write_str(name.as_str());
            self.notice(l);
        }
    }

    /// MIDI MAP's job for the next card work: a save, a rename, a layer
    /// row's drop or the map's delete. One waits at a time: while another
    /// waits for the card, this one is refused on screen, never dropped
    /// unsaid. The door's `ctl_ask`.
    pub(in crate::ui) fn ctl_ask_in(&mut self, j: CtlJob) {
        let c = &mut self.cc.ui().ctl;
        if c.job.is_some() {
            return self.error(Line::new(StoreError::Busy.message()));
        }
        c.job = Some(j);
        c.of = None;
    }

    /// The browser editor's rename of Part `p`'s map to `to` (ADR 0289),
    /// asked while the Part was on the map marked `was`: `ctl_ask_in`'s
    /// rename, held to that map when the card work runs it.
    pub(in crate::ui) fn ctl_rename_of_in(&mut self, p: PartId, was: MapMark, to: SoundName) {
        let c = &mut self.cc.ui().ctl;
        if c.job.is_some() {
            return self.error(Line::new(StoreError::Busy.message()));
        }
        c.job = Some(CtlJob::Rename(p, to));
        c.of = Some(was);
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

/// A card step's end: the card busy runs the job again; else its words.
#[cfg(feature = "midi-map")]
fn busy_or(r: Result<(), LoadError>) -> Result<(), JobErr> {
    match r {
        Ok(()) => Ok(()),
        Err(LoadError::Store(StoreError::Busy)) => Err(JobErr::Busy),
        Err(e) => Err(JobErr::Say(Line::new(load_words(e)))),
    }
}

/// Why a map job stopped: the card busy (it runs again), or words to say.
#[cfg(feature = "midi-map")]
enum JobErr {
    Busy,
    Say(Line),
}

/// Job `j` on `m`: the map read from the first of `from` the card has
/// (else, a save under a new name, the Part's own), `own` absorbed or a
/// row dropped, and the map saved as `name`.
#[cfg(feature = "midi-map")]
#[allow(clippy::too_many_arguments)]
fn job_on(
    s: &mut dyn Store,
    r: &Ready,
    j: CtlJob,
    name: SoundName,
    from: [Option<SoundName>; 2],
    own: &MidiMap,
    engine: EngineType,
    m: &mut ctl::ControllerMap,
) -> Result<(), JobErr> {
    // A rename never writes over a map of its new name.
    if let CtlJob::Rename(..) = j {
        match ctl::load(s, r, &name, m) {
            Ok(_) => return Err(JobErr::Say(Line::new(word(W::Taken)))),
            Err(LoadError::Missing) => {}
            Err(e) => return busy_or(Err(e)),
        }
    }
    // SAFETY: `empty_at` writes the whole map over the last one.
    unsafe { ctl::empty_at(m) };
    let mut found = false;
    for n in from.into_iter().flatten() {
        match ctl::load(s, r, &n, m) {
            Ok(_) => {
                found = true;
                break;
            }
            Err(LoadError::Missing) => {}
            Err(LoadError::Store(StoreError::Busy)) => return Err(JobErr::Busy),
            Err(e) => return Err(JobErr::Say(Line::new(load_words(e)))),
        }
    }
    // A row's map gone from the card (or its stem another map's):
    // nothing to edit, and nothing written in its place.
    if !found && !matches!(j, CtlJob::Save(..)) {
        return Err(JobErr::Say(Line::new(load_words(LoadError::Missing))));
    }
    let edited = match j {
        CtlJob::Save(..) => m.absorb(own, engine),
        CtlJob::Drop(_, b) | CtlJob::Edit(_, b, _) => {
            let to = match j {
                CtlJob::Edit(_, _, to) => Some(to),
                _ => None,
            };
            m.change(&b, to, engine);
            Ok(())
        }
        CtlJob::Delete(..) => return busy_or(ctl::delete(s, r, &name)),
        CtlJob::Rename(..) => Ok(()),
    };
    if edited == Err(MapFull) {
        return Err(JobErr::Say(Line::new(word(W::Full))));
    }
    match ctl::save(s, r, &name, m) {
        // The old name's pair goes once the new one is whole. Not
        // retried: the new file would read as taken; a failed delete
        // leaves a copy.
        Ok(()) => {
            if let (CtlJob::Rename(..), Some(was)) = (j, from[0]) {
                let _ = ctl::delete(s, r, &was);
            }
            Ok(())
        }
        Err(SaveError::Store(StoreError::Busy)) => Err(JobErr::Busy),
        // Another map holds the name's stem: it stays.
        Err(SaveError::File(FileError::BadName)) => Err(JobErr::Say(Line::new(word(W::Taken)))),
        // Past the file (SysEx rows are 12 B): full, as a full layer is.
        Err(SaveError::File(FileError::Bounds)) => Err(JobErr::Say(Line::new(word(W::Full)))),
        Err(SaveError::Store(e)) => Err(JobErr::Say(Line::new(e.message()))),
        Err(SaveError::File(e)) => Err(JobErr::Say(Line::new(e.message()))),
        Err(SaveError::Damaged) => Err(JobErr::Say(Line::new(StoreError::Corrupt.message()))),
    }
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

    /// Two rows on one target (a hand-made file can hold them): an edit
    /// of the second never merges into the first's waiting edit.
    #[test]
    fn an_edit_merges_only_into_its_own_row() {
        let mut l = core::mem::MaybeUninit::<Layers>::uninit();
        // SAFETY: `rest_at` writes the whole `Layers`.
        let l = unsafe {
            rest_at(l.as_mut_ptr());
            l.assume_init_mut()
        };
        let p = PartId::ALL[0];
        let (a, b) = (row(21), row(22));
        // `learn` keeps one row a target: the second row set by hand.
        let reso = ParamAddr::new(BlockRef::Filter, FilterParams::RESONANCE);
        let mut m = MidiMap::EMPTY;
        m.learn(a.input, a.target).unwrap();
        m.learn(b.input, Mappable::new(reso).unwrap()).unwrap();
        m.set(1, b);
        l.layers[0] = m;
        assert_eq!(l.layers[0].entries(), &[a, b]);
        let a2 = Binding {
            span: crate::midi_map::Span { min: 9, max: 255 },
            ..a
        };
        assert!(l.edit(p, 0, a, a2));
        let b2 = Binding {
            span: crate::midi_map::Span { min: 7, max: 255 },
            ..b
        };
        assert!(!l.edit(p, 1, b, b2), "another row's edit waits");
        assert_eq!(l.layers[0].entries(), &[a2, b]);
        // The first row again: merged, from the card's row.
        let a3 = Binding {
            span: crate::midi_map::Span { min: 3, max: 255 },
            ..a
        };
        assert!(l.edit(p, 0, a2, a3));
        assert!(matches!(l.job, Some(CtlJob::Edit(_, x, y)) if x == a && y == a3));
    }
}
