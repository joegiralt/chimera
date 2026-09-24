//! Digitone-style voice allocation (instrument-core spec § Voice
//! allocation): pure bookkeeping over the `MAX_VOICES` pool, no DSP.
//!
//! 1. A Mono part owns one voice while it sounds; a new note retriggers it.
//!    Mono voices are never stolen.
//! 2. A Poly part takes a free voice, round-robin.
//! 3. Pool full: steal the oldest non-mono voice from any part; refuse if
//!    every voice is mono.
//! 4. Over the CPU budget: steal one voice as in rule 3 if that makes room,
//!    else refuse (and steal nothing).
//! 5. Note-off releases the voice (`release`); the voice is free once its
//!    engine reports inactive (`release_finished`), so tails ring out.
//!
//! A Sound change re-costs sounding voices (`recost`); `shed` then cuts the
//! newest voices until the pool is back within the budget.

use crate::hw::{Cost, AUDIO_CYCLE_BUDGET, MAX_VOICES};
use crate::part::PartMode;
use crate::MidiNote;

#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceSlot {
    part: Option<u8>,
    note: Option<MidiNote>,
    /// `Allocator::clock` at the last note-on: lower is older.
    age: u32,
    held: bool,
    mono: bool,
    cost: Cost,
}

impl VoiceSlot {
    pub fn part(&self) -> Option<u8> {
        self.part
    }

    pub fn note(&self) -> Option<MidiNote> {
        self.note
    }

    /// Key still down (no note-off yet).
    pub fn held(&self) -> bool {
        self.held
    }

    pub fn is_free(&self) -> bool {
        self.part.is_none()
    }

    /// Cycles/sample this voice costs now (0 when free).
    pub fn cost(&self) -> Cost {
        self.cost
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
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

impl Allocator {
    pub fn new() -> Self {
        Self { slots: [VoiceSlot::default(); MAX_VOICES], clock: 0, rr: 0, refused: 0 }
    }

    pub fn slots(&self) -> &[VoiceSlot; MAX_VOICES] {
        &self.slots
    }

    /// Notes refused since start (debug counter).
    pub fn refused(&self) -> u32 {
        self.refused
    }

    /// Sum of the costs of every allocated voice.
    pub fn sounding_cost(&self) -> Cost {
        self.slots.iter().map(|s| s.cost).sum()
    }

    /// Allocate a voice for `note` on `part`. `cost` is the voice's
    /// cycles/sample; `reserved` is what is spent outside the pool (the FX
    /// bus). The pool's own sounding cost is tracked here, per slot.
    pub fn note_on(&mut self, part: u8, mode: PartMode, note: MidiNote, cost: Cost, reserved: Cost) -> Alloc {
        let v = match self.pick(part, mode, cost, reserved) {
            Some(v) => v,
            None => {
                self.refused = self.refused.wrapping_add(1);
                return Alloc::Refused;
            }
        };
        self.clock = self.clock.wrapping_add(1);
        self.rr = (v + 1) % MAX_VOICES;
        self.slots[v] = VoiceSlot {
            part: Some(part),
            note: Some(note),
            age: self.clock,
            held: true,
            mono: mode == PartMode::Mono,
            cost,
        };
        Alloc::Voice(v)
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

    /// The voice now costs `cost` (its Part's Sound changed engine).
    pub fn recost(&mut self, voice: usize, cost: Cost) {
        if let Some(s) = self.slots.get_mut(voice).filter(|s| !s.is_free()) {
            s.cost = cost;
        }
    }

    /// If the pool plus `reserved` is over the budget, free the newest
    /// voice — non-mono first — and return it for the caller to silence.
    /// Call until `None`.
    pub fn shed(&mut self, reserved: Cost) -> Option<usize> {
        if reserved + self.sounding_cost() <= AUDIO_CYCLE_BUDGET {
            return None;
        }
        let v = (0..MAX_VOICES)
            .filter(|&v| !self.slots[v].is_free())
            .max_by_key(|&v| (!self.slots[v].mono, self.slots[v].age))?;
        self.slots[v] = VoiceSlot::default();
        Some(v)
    }

    /// The voice's engine went silent: free it if it was released.
    pub fn release_finished(&mut self, voice: usize) {
        if let Some(s) = self.slots.get_mut(voice)
            && !s.held
        {
            *s = VoiceSlot::default();
        }
    }

    fn pick(&self, part: u8, mode: PartMode, cost: Cost, reserved: Cost) -> Option<usize> {
        let fits = |freed: Cost| {
            let total = reserved.0 + self.sounding_cost().0 + cost.0;
            total.saturating_sub(freed.0) <= AUDIO_CYCLE_BUDGET.0
        };
        // Rule 1: a Mono part retriggers the voice it owns.
        if mode == PartMode::Mono
            && let Some(v) = self.slots.iter().position(|s| s.mono && s.part == Some(part))
        {
            return fits(self.slots[v].cost).then_some(v);
        }
        // Rule 2: a free voice, round-robin — if it fits the budget.
        let free = (0..MAX_VOICES).map(|i| (self.rr + i) % MAX_VOICES).find(|&v| self.slots[v].is_free());
        if let Some(v) = free
            && fits(Cost::ZERO)
        {
            return Some(v);
        }
        // Rules 3 and 4: steal the oldest non-mono voice if that makes room.
        let oldest = (0..MAX_VOICES)
            .filter(|&v| !self.slots[v].is_free() && !self.slots[v].mono)
            .min_by_key(|&v| self.slots[v].age)?;
        fits(self.slots[oldest].cost).then_some(oldest)
    }
}
