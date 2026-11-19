//! Program change and bank select (#515, ADR 0224): a change loads a
//! Sound into every Part on its channel. Bank 0 is the project's pool
//! (program n, slot n + 1), bank 1 the INITs by engine disk code; banks 2
//! up are ignored. The bank is CC 0, or CC 32 when CC 0 is 0, and stands
//! per port and channel till changed (MIDI 1.0).
//!
//! An edited Part never refuses and never loses its edits (Elektron
//! style): they become its recall copy, and a later change back to that
//! Sound brings them back, still edited. Any other load of the Part drops
//! the copy. While card work runs, a prompt or learn is open or the Part's
//! Sound rung is browsed, each channel's latest change waits.

use core::mem::MaybeUninit;
use core::sync::atomic::Ordering;

use chimera_midi::value::{U7, U14};

use super::super::UiState;
use super::super::cc::MidiSlot;
use super::super::learn::Learn;
use crate::MidiChannel;
use crate::block::DiskCode;
use crate::hw::MAX_PARTS;
use crate::midi_in::{PROGRAM_IN, PROGRAM_PENDING, Port};
use crate::params::EngineType;
use crate::preset::Sound;
use crate::project::{
    Loads, Origin, PartFrom, PartId, PartSource, PartStatus, ReplaceGuard, SlotId, part_status,
};

/// A Part's recall copy: the edited Sound it left, and where it left it.
pub struct Recall {
    from: Option<Origin>,
    /// The Part's loads after the change that left it: any other load
    /// moves them, and the copy is dropped.
    loads: Loads,
    sound: Sound,
}

/// Each Part's recall copy, uninit till first use (`MidiSlot::recall`).
pub type Recalls = [MaybeUninit<Recall>; MAX_PARTS];

// `recalls` seeds each copy bitwise: nothing of a Sound is owned.
const _: () = assert!(!core::mem::needs_drop::<Sound>());

/// Each Part's recall copy, written on first use since the MIDI UI was
/// built: no copy, its Sound `seed`'s bits, copied in place, so no frame
/// holds a Sound (a `Sound::init` built one on the UI's stack, +1.3 KB).
/// Inlined into its one caller: out of line, the outliner left a call to
/// it in bank 1.
#[inline(always)]
fn recalls<'a>(slot: &'a mut MidiSlot, seed: &Sound) -> &'a mut [Recall; MAX_PARTS] {
    let all = slot.recall;
    let m = slot.ui();
    if !m.recall_ready {
        for i in 0..MAX_PARTS {
            // SAFETY: `all` is `take`'s, this slot's alone, and reached
            // only here; each field is written whole before `recall_ready`
            // says it is; `seed` is a valid Sound with no drop glue (the
            // assert above), so its bits are one too.
            unsafe {
                let r = (*all)[i].as_mut_ptr();
                core::ptr::addr_of_mut!((*r).from).write(None);
                core::ptr::addr_of_mut!((*r).loads).write(Loads::NONE);
                core::ptr::copy_nonoverlapping(seed, core::ptr::addr_of_mut!((*r).sound), 1);
            }
        }
        m.recall_ready = true;
    }
    // SAFETY: written above, or by an earlier call since the build that
    // set `recall_ready`; the `&mut` borrows the slot, so it is the one.
    unsafe { &mut *all.cast::<[Recall; MAX_PARTS]>() }
}

/// A waiting change: bank << 7 | program, the top bit set.
const WAITS: u16 = 0x8000;

/// What `bank`, `program` loads; `None`: no Sound there.
fn source(bank: u8, program: u8) -> Option<PartFrom> {
    match bank {
        0 => SlotId::new(program).map(PartFrom::Slot),
        1 => EngineType::PLAYABLE
            .into_iter()
            .find(|e| e.disk_code() == program)
            .map(PartFrom::Init),
        _ => None,
    }
}

fn from_of(o: Origin) -> PartFrom {
    match o {
        Origin::Slot { slot, .. } => PartFrom::Slot(slot),
        Origin::Init(e) => PartFrom::Init(e),
    }
}

impl UiState {
    /// Before this pass's messages: the pipes learn PROGRAM IN.
    pub(super) fn programs_published(&mut self) {
        self.cc.heard = 0;
        PROGRAM_IN.store(self.midi.program_in(), Ordering::Relaxed);
    }

    /// The channels applied by an earlier pass go free (the audio's hold),
    /// their loads published since: not one a change was heard on this
    /// pass, or one still waits on, whose pipe marked it again.
    fn programs_free(&mut self) {
        let waiting = (0..16).fold(0u16, |m, c| m | u16::from(self.cc.wait[c] != 0) << c);
        let free = self.cc.done & !self.cc.heard & !waiting;
        self.cc.done &= !free;
        PROGRAM_PENDING.fetch_and(!free, Ordering::Release);
    }

    /// A program change heard on `port`'s channel `ch`: its bank latched,
    /// the change waiting for `programs` (latest wins).
    pub(super) fn cc_program(&mut self, port: Port, ch: MidiChannel, bank: Option<U14>, p: U7) {
        let c = usize::from(ch.get() & 15);
        self.cc.heard |= 1 << c;
        if !self.midi.program_in() {
            self.cc.done |= 1 << c;
            return;
        }
        let b = &mut self.cc.banks[port as usize & 1][c];
        if let Some(v) = bank {
            *b = if v.msb().get() != 0 {
                v.msb().get()
            } else {
                v.lsb().get()
            };
        }
        self.cc.wait[c] = WAITS | u16::from(*b) << 7 | u16::from(p.get());
    }

    /// The waiting changes applied, unless the card, a prompt or learn is
    /// busy; a channel whose Part's Sound rung is open waits too.
    pub(super) fn programs(&mut self, busy: bool) {
        self.programs_free();
        // No recall copies (a build without the map, whose `hear` is
        // `deaf`): nothing to load into, so nothing loads.
        if self.cc.recall.is_null() {
            return;
        }
        // Learn holds them back; pointing doesn't (ADR 0288): it may last
        // long, and a load under it is said at the bind (`BINDING GONE`).
        if busy
            || self.modal.is_some()
            || matches!(self.learn, Learn::Armed { .. } | Learn::Sensing { .. })
        {
            return;
        }
        let browsed = self
            .loc
            .browse()
            .map(|(p, _)| self.project.part(p).mix.channel.get());
        for c in 0..16u8 {
            let w = self.cc.wait[usize::from(c)];
            if w & WAITS == 0 || browsed == Some(c) {
                continue;
            }
            self.cc.wait[usize::from(c)] = 0;
            self.cc.done |= 1 << c;
            if let Some(from) = source((w >> 7 & 0x7F) as u8, (w & 0x7F) as u8) {
                self.program_channel(c, from);
            }
        }
    }

    /// `from` into every Part on channel `c`; the active Part's pages
    /// follow its engine, unless the browser is open.
    fn program_channel(&mut self, c: u8, from: PartFrom) {
        if let PartFrom::Slot(s) = from
            && self.project.pool().get(s).is_none()
        {
            return;
        }
        let was = self.active_chain();
        let mut active = false;
        for p in PartId::ALL {
            if self.project.part(p).mix.channel.get() == c && self.program_part(p, from) {
                active |= p == self.active_part;
            }
        }
        if active && self.loc.browse().is_none() {
            self.project_replaced(was);
        }
        self.browser_dirty = true;
    }

    /// `from` into Part `p`; whether it loaded. The Sound it holds Clean
    /// or edited stays; Stale reloads.
    fn program_part(&mut self, p: PartId, from: PartFrom) -> bool {
        let part = self.project.part(p);
        let (out, loads) = (part.origin(), part.loads());
        let status = part_status(part, self.project.pool());
        if from_of(out) == from && !matches!(status, PartStatus::Stale(_)) {
            return false;
        }
        let pool = self.project.pool();
        let r = &mut recalls(self.cc, &self.project.part(p).sound)[p.index()];
        // The copy is `from`'s, nothing loaded the Part since, and the
        // slot still holds what it was edited from.
        let back = r.loads == loads
            && r.from.is_some_and(|o| {
                from_of(o) == from
                    && match o {
                        Origin::Slot { slot, crc, .. } => {
                            pool.get(slot).is_some_and(|e| e.crc() == crc)
                        }
                        Origin::Init(_) => true,
                    }
            });
        let edited = status == PartStatus::Edited;
        let src = PartSource { part: p, from };
        let loaded = if edited || back {
            let ok = self.project.program_load(src, &mut r.sound, back).is_ok();
            if ok {
                r.from = edited.then_some(out);
            }
            ok
        } else {
            match ReplaceGuard::check(&self.project, self.template, src) {
                Ok(c) => self.project.replace_part(c).is_ok(),
                Err(_) => false,
            }
        };
        r.loads = self.project.part(p).loads();
        loaded
    }
}
