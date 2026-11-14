//! Digitone-style voice allocation (instrument-core spec § Voice
//! allocation): pure bookkeeping over the `MAX_VOICES` pool, no DSP.
//!
//! 1. A Mono part owns one voice while it sounds; a new note retriggers it.
//!    Mono voices are never stolen.
//! 2. A Poly part re-striking a key it sounds, held or ringing, takes that
//!    voice back if the budget allows: a re-plucked string (the owner's
//!    UAT, 2026-09-30); if not, the rules below.
//!    Else a free voice, round-robin; with none free, a dying
//!    one, nearest the end of its fade (its note starts when the fade ends).
//! 3. Pool full: steal from any part the oldest released (note-off'd, tail
//!    ringing) non-mono voice; if none, the oldest held non-mono voice;
//!    refuse if every voice is mono.
//! 4. Over the CPU budget: steal one voice as in rule 3 if that makes room,
//!    else refuse (and steal nothing).
//! 5. Note-off releases the voice (`release`); the voice is free once its
//!    engine reports inactive (`release_finished`), so tails ring out.
//!
//! A Sound change re-costs sounding voices (`recost`); `shed` then marks
//! voices dying until the rest fit the budget (ADR 0027): tails first, oldest
//! first, then the newest held note. A dying voice keeps its slot while it
//! fades, is never stolen or shed again, and frees when the fade ends
//! (`release_finished`). Its cost counts in `sounding_cost` but not against a
//! note-on: it is gone within `Voice::FADE`.
//!
//! A voice taken for another Sound's note fades out on its old one first;
//! its slot bills the costlier of the two until the note starts
//! (`fade_from`, ADR 0091).

use crate::MidiNote;
use crate::hw::{Cost, MAX_VOICES, SampleBudget};
use crate::part::PartMode;

/// A voice's index in the pool: always `< MAX_VOICES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceIdx(u8);

impl VoiceIdx {
    pub const ALL: [VoiceIdx; MAX_VOICES] = {
        let mut all = [VoiceIdx(0); MAX_VOICES];
        let mut i = 0;
        while i < MAX_VOICES {
            all[i] = VoiceIdx(i as u8);
            i += 1;
        }
        all
    };

    pub const fn new(i: usize) -> Option<Self> {
        if i < MAX_VOICES {
            Some(Self(i as u8))
        } else {
            None
        }
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A set of voices, a bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceSet(u8);

const _: () = assert!(MAX_VOICES <= u8::BITS as usize);

impl VoiceSet {
    pub const EMPTY: VoiceSet = VoiceSet(0);

    pub const fn with(self, v: VoiceIdx) -> VoiceSet {
        VoiceSet(self.0 | 1 << v.0)
    }

    pub const fn contains(self, v: VoiceIdx) -> bool {
        self.0 & 1 << v.0 != 0
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceSlot {
    part: Option<u8>,
    note: Option<MidiNote>,
    /// `Allocator::clock` at the last note-on: lower is older.
    age: u32,
    held: bool,
    mono: bool,
    /// Shed, fading out, freed by `release_finished`: the shed's order.
    dying: Option<u32>,
    cost: Cost,
    /// A knob glide's extra on `cost`: billed to admission, never shed
    /// on (ADR 0117).
    glide: Cost,
    /// What the voice's last note, taken for this one, costs while it
    /// fades out before this one starts (ADR 0091).
    fading: Cost,
}

impl VoiceSlot {
    pub fn part(&self) -> Option<u8> {
        self.part
    }

    pub fn note(&self) -> Option<MidiNote> {
        self.note
    }

    /// `Allocator`'s clock at the note-on: lower is older. The sympathetic
    /// pool ranks its slots by the same ages.
    pub fn age(&self) -> u32 {
        self.age
    }

    /// Key still down (no note-off yet).
    pub fn held(&self) -> bool {
        self.held
    }

    /// Shed and fading out.
    pub fn dying(&self) -> bool {
        self.dying.is_some()
    }

    pub fn is_free(&self) -> bool {
        self.part.is_none()
    }

    /// Cycles/sample this voice costs now (0 when free): its note's, or
    /// while the note it took fades out, the costlier of the two.
    pub fn cost(&self) -> Cost {
        (self.cost + self.glide).max(self.fading)
    }

    /// `cost` without the glide: what shedding counts.
    fn shed_cost(&self) -> Cost {
        self.cost.max(self.fading)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alloc {
    Voice(usize),
    Refused,
}

pub struct Allocator {
    slots: [VoiceSlot; MAX_VOICES],
    clock: u32,
    /// Next slot to try for a free voice.
    rr: usize,
    refused: u32,
    /// Counts sheds: a lower `dying` is nearer the end of its fade.
    sheds: u32,
    budget: SampleBudget,
}

impl Allocator {
    pub fn new(budget: SampleBudget) -> Self {
        Self {
            slots: [VoiceSlot::default(); MAX_VOICES],
            clock: 0,
            rr: 0,
            refused: 0,
            sheds: 0,
            budget,
        }
    }

    pub fn budget(&self) -> SampleBudget {
        self.budget
    }

    pub fn slots(&self) -> &[VoiceSlot; MAX_VOICES] {
        &self.slots
    }

    /// Notes refused since start, and notes shed before they sounded
    /// (debug counter).
    pub fn refused(&self) -> u32 {
        self.refused
    }

    /// A note was shed before it sounded (ADR 0027): count it as refused.
    pub fn dropped_unheard(&mut self) {
        self.refused = self.refused.wrapping_add(1);
    }

    /// Sum of the costs of every allocated voice.
    pub fn sounding_cost(&self) -> Cost {
        self.slots.iter().map(VoiceSlot::cost).sum()
    }

    /// `reserved` and every allocated voice, as a share of the budget,
    /// saturating.
    pub fn cost_pct(&self, reserved: Cost) -> u8 {
        let booked = reserved.0.saturating_add(self.sounding_cost().0);
        booked
            .saturating_mul(100)
            .checked_div(self.budget.as_cost().0)
            .map_or(u8::MAX, |p| p.min(u32::from(u8::MAX)) as u8)
    }

    /// `sounding_cost` without the dying voices.
    fn live_cost(&self) -> Cost {
        self.slots
            .iter()
            .filter(|s| s.dying.is_none())
            .map(VoiceSlot::cost)
            .sum()
    }

    /// `live_cost` without the knob glides: a turn never sheds.
    fn steady_live_cost(&self) -> Cost {
        self.slots
            .iter()
            .filter(|s| s.dying.is_none())
            .map(VoiceSlot::shed_cost)
            .sum()
    }

    /// Allocate a voice for `note` on `part`. `cost` is the voice's
    /// cycles/sample; `reserved` is what is spent outside the pool (the FX
    /// bus). The pool's own sounding cost is tracked here, per slot.
    pub fn note_on(
        &mut self,
        part: u8,
        mode: PartMode,
        note: MidiNote,
        cost: Cost,
        reserved: Cost,
    ) -> Alloc {
        match self.pick(part, mode, note, cost, reserved) {
            Some(v) => {
                self.book(v, part, mode, note, cost);
                Alloc::Voice(v)
            }
            None => self.refuse(),
        }
    }

    /// A note the budget gave no voice: counted, `Refused`.
    pub fn refuse(&mut self) -> Alloc {
        self.refused = self.refused.wrapping_add(1);
        Alloc::Refused
    }

    /// The age `book` gives the next note.
    fn next_age(&self) -> u32 {
        self.clock.wrapping_add(1)
    }

    /// Books `note` on `pick`'s voice `v`, whatever it held.
    pub fn book(&mut self, v: usize, part: u8, mode: PartMode, note: MidiNote, cost: Cost) {
        let mono = mode == PartMode::Mono;
        self.clock = self.next_age();
        self.rr = (v + 1) % MAX_VOICES;
        self.slots[v] = VoiceSlot {
            part: Some(part),
            note: Some(note),
            age: self.clock,
            held: true,
            mono,
            dying: None,
            cost,
            glide: Cost::ZERO,
            fading: Cost::ZERO,
        };
    }

    /// Key up on `voice`: it is no longer held (its tail keeps the slot
    /// until `release_finished`). No-op for a free or already released
    /// voice, or an index out of range. The caller picks the voice — the
    /// Instrument matches note and note-on channel, which a (part, note)
    /// lookup cannot tell apart when one Part holds a key from two channels.
    pub fn release(&mut self, voice: usize) {
        if let Some(s) = self.slots.get_mut(voice) {
            s.held = false;
        }
    }

    /// A held `voice`'s note moved to `note` (a re-snap): its age and the
    /// rest stand. No-op for a free voice or an index out of range.
    pub fn renote(&mut self, voice: usize, note: MidiNote) {
        if let Some(s) = self.slots.get_mut(voice).filter(|s| !s.is_free()) {
            s.note = Some(note);
        }
    }

    /// `voice` was taken from a note of another Sound, which costs
    /// `fading` until it has faded out and the booked note starts; then
    /// `Cost::ZERO` (ADR 0091). No-op for a free voice.
    pub fn fade_from(&mut self, voice: usize, fading: Cost) {
        if let Some(s) = self.slots.get_mut(voice).filter(|s| !s.is_free()) {
            s.fading = fading;
        }
    }

    /// The voice now costs `cost` (its Part's Sound changed).
    pub fn recost(&mut self, voice: usize, cost: Cost) {
        self.recost_with_glide(voice, cost, Cost::ZERO);
    }

    /// `glide` is a knob glide's extra: it counts toward admission, not
    /// shedding (ADR 0117).
    pub fn recost_with_glide(&mut self, voice: usize, cost: Cost, glide: Cost) {
        if let Some(s) = self.slots.get_mut(voice).filter(|s| !s.is_free()) {
            (s.cost, s.glide) = (cost, glide);
        }
    }

    /// If the voices not yet dying, plus `reserved`, are over the budget,
    /// mark one dying and return it for the caller to fade out: non-mono
    /// first, then the oldest tail, then the newest held note. Call until
    /// `None`. Dying voices still count in `sounding_cost`.
    pub fn shed(&mut self, reserved: Cost) -> Option<usize> {
        if reserved + self.steady_live_cost() <= self.budget.as_cost() {
            return None;
        }
        self.shed_one()
    }

    /// Mark the voice `shed` would pick dying, whatever the budget, and
    /// return it: the load net's (ADR 0272). `None` once every booked
    /// voice is dying or the pool is empty.
    pub fn shed_one(&mut self) -> Option<usize> {
        let v = (0..MAX_VOICES)
            .filter(|&v| !self.slots[v].is_free() && !self.slots[v].dying())
            .min_by_key(|&v| {
                let s = &self.slots[v];
                (
                    s.mono,
                    s.held,
                    if s.held { u32::MAX - s.age } else { s.age },
                )
            })?;
        self.sheds = self.sheds.wrapping_add(1);
        self.slots[v].dying = Some(self.sheds);
        Some(v)
    }

    /// A project load (ADR 0046): every booked slot dying, as if shed, so
    /// none is re-struck, retriggered or stolen while it fades. Not counted:
    /// nothing was refused.
    pub fn kill_all(&mut self) {
        for v in 0..MAX_VOICES {
            self.kill(v);
        }
    }

    /// One slot dying, as `kill_all` does (CC 120).
    pub fn kill(&mut self, voice: usize) {
        if let Some(s) = self
            .slots
            .get_mut(voice)
            .filter(|s| !s.is_free() && !s.dying())
        {
            self.sheds = self.sheds.wrapping_add(1);
            s.dying = Some(self.sheds);
        }
    }

    /// The voice's engine went silent: free it if it was released or shed.
    pub fn release_finished(&mut self, voice: usize) {
        if let Some(s) = self.slots.get_mut(voice)
            && (!s.held || s.dying())
        {
            *s = VoiceSlot::default();
        }
    }

    /// The voice `note_on` would book, without booking it: rules 1–4.
    pub fn pick(
        &self,
        part: u8,
        mode: PartMode,
        note: MidiNote,
        cost: Cost,
        reserved: Cost,
    ) -> Option<usize> {
        self.pick_where(part, mode, note, cost, reserved, &|_| true)
    }

    /// `pick`, where rule 2 re-strikes only voices `restrike` accepts: a key
    /// held by another origin is a voice of its own (`Instrument::handle_from`).
    pub fn pick_where(
        &self,
        part: u8,
        mode: PartMode,
        note: MidiNote,
        cost: Cost,
        reserved: Cost,
        restrike: &dyn Fn(usize) -> bool,
    ) -> Option<usize> {
        let fits = |freed: Cost| {
            let total = reserved.0 + self.live_cost().0 + cost.0;
            total.saturating_sub(freed.0) <= self.budget.as_cost().0
        };
        // Rule 1: a Mono part retriggers the voice it owns.
        if mode == PartMode::Mono
            && let Some(v) = self
                .slots
                .iter()
                .position(|s| s.mono && !s.dying() && s.part == Some(part))
        {
            return fits(self.slots[v].cost()).then_some(v);
        }
        // Rule 2: the key's own voice, re-struck (if `restrike` accepts it),
        // if it fits (else on to
        // the rest); else a free voice, round-robin, else the dying one
        // nearest the end of its fade — if it fits.
        if mode != PartMode::Mono
            && let Some(v) = self.slots.iter().enumerate().position(|(v, s)| {
                !s.mono && !s.dying() && s.part == Some(part) && s.note == Some(note) && restrike(v)
            })
            && fits(self.slots[v].cost())
        {
            return Some(v);
        }
        let free = (0..MAX_VOICES)
            .map(|i| (self.rr + i) % MAX_VOICES)
            .find(|&v| self.slots[v].is_free())
            .or_else(|| {
                (0..MAX_VOICES)
                    .filter_map(|v| self.slots[v].dying.map(|d| (d, v)))
                    .min()
                    .map(|(_, v)| v)
            });
        if let Some(v) = free
            && fits(Cost::ZERO)
        {
            return Some(v);
        }
        // Rules 3 and 4: steal the oldest non-mono voice — tails before held
        // notes — if that makes room.
        let oldest = (0..MAX_VOICES)
            .filter(|&v| !self.slots[v].is_free() && !self.slots[v].mono && !self.slots[v].dying())
            .min_by_key(|&v| (self.slots[v].held, self.slots[v].age))?;
        fits(self.slots[oldest].cost()).then_some(oldest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_idx_all_counts_up() {
        for (i, v) in VoiceIdx::ALL.iter().enumerate() {
            assert_eq!(v.index(), i);
        }
    }
}
