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

/// Card work MIDI MAP asked for.
#[derive(Clone, Copy)]
pub enum CtlJob {
    /// The Part's rows saved into map `name`, which the Part then picks.
    Save(PartId, SoundName),
    /// Row `b` dropped from the Part's map.
    Drop(PartId, Binding),
}

/// Each Part's layer and what it was read for, the card's maps as last
/// listed, and the job waiting.
#[derive(Clone, Copy)]
pub struct Layers {
    layers: [MidiMap; LAYERS],
    read: [Option<Read>; MAX_PARTS],
    /// Moves on every save: every layer reads again.
    saves: u8,
    list: [Option<SoundName>; MAX_MAPS],
    listed: bool,
    /// The card holds more maps than `list`.
    pub(super) more: bool,
    job: Option<CtlJob>,
    /// The card last worked on: another reads every layer again.
    vol: Option<VolumeId>,
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
        addr_of_mut!((*p).saves).write(0);
        super::fill(
            addr_of_mut!((*p).list).cast::<Option<SoundName>>(),
            MAX_MAPS,
            None,
        );
        addr_of_mut!((*p).listed).write(false);
        addr_of_mut!((*p).more).write(false);
        addr_of_mut!((*p).job).write(None);
        addr_of_mut!((*p).vol).write(None);
    }
}

/// The words a save or a delete says, `|` between: a named static, so its
/// bytes are bank 2's.
static WORDS: [u8; 90] =
    *b"SAVED |MAP FULL|CONTROLLER|NONE|IN |NO CONTROLLER MAP|NAME TAKEN|CARD HOLDS MORE: 16 SHOWN";

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

    /// The maps listed, `None` before a listing.
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
            return;
        };
        let c = &mut self.cc.ui().ctl;
        if c.vol.replace(r.volume()).is_some_and(|v| v != r.volume()) {
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
            let mut m = core::mem::MaybeUninit::uninit();
            // SAFETY: `empty_at` writes the whole map.
            let m = unsafe {
                ctl::empty_at(m.as_mut_ptr());
                m.assume_init_mut()
            };
            let read = want.name.map(|n| ctl::load(s, r, &n, m));
            if let Some(Err(LoadError::Store(StoreError::Busy))) = read {
                // The streams hold the card: the next pass.
                return;
            }
            let c = &mut self.cc.ui().ctl;
            let layer = m.layer(want.engine).0;
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

    fn ctl_job(&mut self, s: &mut dyn Store, r: &Ready, j: CtlJob) {
        let (p, name) = match j {
            CtlJob::Save(p, n) => (p, Some(n)),
            CtlJob::Drop(p, _) => (p, self.project.controller(p)),
        };
        let Some(name) = name else { return };
        let engine = self.project.part(p).sound.params.engine();
        // The map there, else (a save under a new name) the Part's own.
        let mut m = core::mem::MaybeUninit::uninit();
        // SAFETY: `empty_at` writes the whole map.
        let m = unsafe {
            ctl::empty_at(m.as_mut_ptr());
            m.assume_init_mut()
        };
        let mut from = [Some(name), self.project.controller(p)];
        if let CtlJob::Drop(..) = j {
            from[1] = None;
        }
        let mut found = false;
        for n in from.into_iter().flatten() {
            match ctl::load(s, r, &n, m) {
                Ok(_) => {
                    found = true;
                    break;
                }
                Err(LoadError::Missing) => {}
                Err(LoadError::Store(StoreError::Busy)) => {
                    self.cc.ui().ctl.job = Some(j);
                    return;
                }
                Err(e) => return self.error(Line::new(load_words(e))),
            }
        }
        // A row's map gone from the card (or its stem another map's):
        // nothing to drop it from, and nothing written in its place.
        if let (CtlJob::Drop(..), false) = (j, found) {
            return self.error(Line::new(load_words(LoadError::Missing)));
        }
        let own = self.project.part(p).sound.map;
        let edited = match j {
            CtlJob::Save(..) => m.absorb(&own, engine),
            CtlJob::Drop(_, b) => {
                m.drop(&b, engine);
                Ok(())
            }
        };
        if edited == Err(MapFull) {
            return self.error(Line::new(word(W::Full)));
        }
        match ctl::save(s, r, &name, m) {
            Ok(()) => {}
            Err(SaveError::Store(StoreError::Busy)) => {
                self.cc.ui().ctl.job = Some(j);
                return;
            }
            // Another map holds the name's stem: it stays.
            Err(SaveError::File(FileError::BadName)) => {
                return self.error(Line::new(word(W::Taken)));
            }
            Err(SaveError::Store(e)) => return self.error(Line::new(e.message())),
            Err(SaveError::File(e)) => return self.error(Line::new(e.message())),
            Err(SaveError::Damaged) => return self.error(Line::new(StoreError::Corrupt.message())),
        }
        let c = &mut self.cc.ui().ctl;
        c.saves = c.saves.wrapping_add(1);
        c.listed = false;
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

    /// MIDI MAP's save (`n`) or a layer row's drop (`b`), for the next card
    /// work. The door's `ctl_ask`.
    pub(in crate::ui) fn ctl_ask_in(
        &mut self,
        p: PartId,
        n: Option<SoundName>,
        b: Option<Binding>,
    ) {
        let j = match (n, b) {
            (Some(n), _) => CtlJob::Save(p, n),
            (None, Some(b)) => CtlJob::Drop(p, b),
            (None, None) => return,
        };
        self.cc.ui().ctl.job = Some(j);
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
        if to != at && next != now {
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
