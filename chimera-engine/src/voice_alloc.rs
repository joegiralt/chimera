// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Digitone-style voice allocation (instrument-core spec § Voice
//! allocation): pure bookkeeping over the `MAX_VOICES` pool, no DSP.
//!
//! 1. A Mono part owns one voice while it sounds; a new note retriggers it.
//!    Mono voices are never stolen.
//! 2. A Poly part re-striking a key it sounds, held or ringing, takes that
//!    voice back if the budget allows: a re-plucked string (UAT,
//!    2026-09-30); if not, the rules below.
//!    Else a free voice, round-robin; with none free, a dying
//!    one, nearest the end of its fade (its note starts when the fade ends).
//! 3. Pool full: steal from any part the oldest released (note-off'd, tail
//!    ringing) non-mono voice; if none, the oldest held non-mono voice;
//!    refuse if every voice is mono.
//! 4. Over the CPU budget: steal one voice as in rule 3 if that makes room,
//!    else refuse (and steal nothing).
//!    A DRUM hit whose pad has live voices of its own (STACK) steals only
//!    from them (`pick_hit`, ADR 0280).
//!    A Part at its MAX VOICES cap takes no free voice: rules 3 and 4 steal
//!    only its own. A Part with no more held notes than its RESERVE is
//!    guarded: another Part's note never steals those held notes, and a
//!    shed takes them last; its released tails are fair game (ADR 0281).
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
use crate::project::PartId;
use crate::voice_limit::{MaxVoices, Reserved, Shares, VoiceCount};

/// A voice's index in the pool: always `< MAX_VOICES`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

    /// `n` voices on, round the pool.
    pub const fn wrapping_add(self, n: usize) -> VoiceIdx {
        VoiceIdx(((self.0 as usize + n) % MAX_VOICES) as u8)
    }
}

/// A set of voices, a bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceSet(u8);

const _: () = assert!(MAX_VOICES <= u8::BITS as usize);

impl VoiceSet {
    pub const EMPTY: VoiceSet = VoiceSet(0);

    /// Every voice of the pool.
    pub const ALL: VoiceSet = {
        let (mut s, mut i) = (VoiceSet::EMPTY, 0);
        while i < MAX_VOICES {
            s = s.with(VoiceIdx::ALL[i]);
            i += 1;
        }
        s
    };

    pub const fn with(self, v: VoiceIdx) -> VoiceSet {
        VoiceSet(self.0 | 1 << v.0)
    }

    pub const fn contains(self, v: VoiceIdx) -> bool {
        self.0 & 1 << v.0 != 0
    }

    /// The voices in both.
    pub const fn and(self, o: VoiceSet) -> VoiceSet {
        VoiceSet(self.0 & o.0)
    }

    /// The voices in either.
    pub const fn or(self, o: VoiceSet) -> VoiceSet {
        VoiceSet(self.0 | o.0)
    }

    /// These voices but `o`'s.
    pub const fn without(self, o: VoiceSet) -> VoiceSet {
        VoiceSet(self.0 & !o.0)
    }

    /// How many: the set's bits, at most the pool's (`VoiceCount::of_pool_bits`).
    pub const fn count(self) -> VoiceCount {
        VoiceCount::of_pool_bits(self.0)
    }
}

/// Which voices rules 3 and 4 may steal (ADR 0280).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Steal {
    /// Any: the oldest voice of the pool.
    Any,
    /// Only these: a DRUM hit's pad's live voices (a STACK pad's).
    Only(VoiceSet),
}

impl Steal {
    /// With voice `v` the hit's own: from `Any`, only it.
    pub const fn mine(self, v: VoiceIdx) -> Steal {
        match self {
            Steal::Any => Steal::Only(VoiceSet::EMPTY.with(v)),
            Steal::Only(s) => Steal::Only(s.with(v)),
        }
    }

    /// The voices it may take.
    pub const fn pool(self) -> VoiceSet {
        match self {
            Steal::Any => VoiceSet::ALL,
            Steal::Only(s) => s,
        }
    }
}

/// A key's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Down,
    Up,
}

/// What a booked voice plays (ADR 0281): a note, which counts toward its
/// Part's RESERVE while its key is down, or a DRUM hit, which counts until
/// its voice finishes, its pad's note-off or not (2026-11-18 rulings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strike {
    Note(Key),
    Hit(Key),
}

impl Strike {
    /// The key is still down (no note-off yet).
    pub const fn held(self) -> bool {
        matches!(self, Strike::Note(Key::Down) | Strike::Hit(Key::Down))
    }

    /// It counts toward its Part's RESERVE and is guarded within it.
    pub const fn reserves(self) -> bool {
        match self {
            Strike::Note(k) => matches!(k, Key::Down),
            Strike::Hit(_) => true,
        }
    }

    /// Its slot frees when its voice goes quiet, its key down or not: a
    /// released note, or a DRUM hit, which ends with its voice (the
    /// owner's ruling, 2026-11-18). A held note's voice keeps its slot.
    pub const fn ends_with_voice(self) -> bool {
        match self {
            Strike::Note(k) => matches!(k, Key::Up),
            Strike::Hit(_) => true,
        }
    }

    /// After its note-off.
    pub const fn released(self) -> Strike {
        match self {
            Strike::Note(_) => Strike::Note(Key::Up),
            Strike::Hit(_) => Strike::Hit(Key::Up),
        }
    }
}

/// What a booked slot plays: its Part, its note and how it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Booking {
    part: PartId,
    note: MidiNote,
    strike: Strike,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VoiceSlot {
    /// What it plays; `None` while free.
    booking: Option<Booking>,
    /// `Allocator::clock` at the last note-on: lower is older.
    age: u32,
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
    pub fn part(&self) -> Option<PartId> {
        self.booking.map(|b| b.part)
    }

    pub fn note(&self) -> Option<MidiNote> {
        self.booking.map(|b| b.note)
    }

    /// `Allocator`'s clock at the note-on: lower is older. The sympathetic
    /// pool ranks its slots by the same ages.
    pub fn age(&self) -> u32 {
        self.age
    }

    /// Key still down (no note-off yet).
    pub fn held(&self) -> bool {
        self.booking.is_some_and(|b| b.strike.held())
    }

    /// What it plays: a note or a DRUM hit, and its key; `None` while free.
    pub fn strike(&self) -> Option<Strike> {
        self.booking.map(|b| b.strike)
    }

    /// It counts toward its Part's RESERVE (`Strike::reserves`).
    fn reserves(&self) -> bool {
        self.booking.is_some_and(|b| b.strike.reserves())
    }

    /// Shed and fading out.
    pub fn dying(&self) -> bool {
        self.dying.is_some()
    }

    pub fn is_free(&self) -> bool {
        self.booking.is_none()
    }

    /// Cycles/sample this voice costs now (0 when free): its note's, or
    /// while the note it took fades out, the costlier of the two.
    pub fn cost(&self) -> Cost {
        (self.cost + self.glide).max(self.fading)
    }

    /// The glide's extra on `cost` (admission's): for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn glide(&self) -> Cost {
        self.glide
    }

    /// `cost` without the glide: what shedding counts.
    fn shed_cost(&self) -> Cost {
        self.cost.max(self.fading)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alloc {
    Voice(VoiceIdx),
    Refused,
}

pub struct Allocator {
    slots: [VoiceSlot; MAX_VOICES],
    clock: u32,
    /// Next slot to try for a free voice.
    rr: VoiceIdx,
    refused: u32,
    /// Counts sheds: a lower `dying` is nearer the end of its fade.
    sheds: u32,
    budget: SampleBudget,
    /// Each Part's MAX VOICES and RESERVE (ADR 0281).
    shares: Shares,
}

impl Allocator {
    pub fn new(budget: SampleBudget) -> Self {
        Self {
            slots: [VoiceSlot::default(); MAX_VOICES],
            clock: 0,
            rr: VoiceIdx::ALL[0],
            refused: 0,
            sheds: 0,
            budget,
            shares: Shares::AUTO,
        }
    }

    /// The Parts' MAX VOICES and RESERVE the next picks and sheds honour
    /// (ADR 0281): once a snapshot, before its note-ons.
    pub fn set_shares(&mut self, s: Shares) {
        self.shares = s;
    }

    /// Voice `v`'s slot.
    pub fn slot(&self, v: VoiceIdx) -> &VoiceSlot {
        &self.slots[v.index()]
    }

    /// Part `part`'s live voices: booked, not dying. Out of line, as
    /// `guarded`: bank 1's (ADR 0281).
    #[inline(never)]
    #[optimize(size)]
    fn live(&self, part: PartId) -> VoiceSet {
        let mut s = VoiceSet::EMPTY;
        for v in VoiceIdx::ALL {
            let slot = self.slot(v);
            if slot.part() == Some(part) && slot.dying.is_none() {
                s = s.with(v);
            }
        }
        s
    }

    /// Part `part`'s note may take no more voices: it is at its cap.
    fn capped(&self, part: PartId) -> bool {
        match self.shares.ask(part).max {
            MaxVoices::Auto => false,
            m @ MaxVoices::Cap(_) => m.reached(self.live(part).count()),
        }
    }

    /// Part `part`'s voices that count toward its reserve: live notes with
    /// their key down, and live DRUM hits (`Strike::reserves`, ADR 0281).
    #[inline(never)]
    #[optimize(size)]
    fn reserving(&self, part: PartId) -> VoiceSet {
        let mut s = VoiceSet::EMPTY;
        for v in VoiceIdx::ALL {
            let slot = self.slot(v);
            if slot.part() == Some(part) && slot.dying.is_none() && slot.reserves() {
                s = s.with(v);
            }
        }
        s
    }

    /// The reserving voices (`reserving`) of every Part but `but` with no
    /// more of them than its reserve: what another Part's note may not
    /// steal (ADR 0281). A released note's tail is never guarded; a DRUM
    /// hit is until it ends. Empty with no reserve set.
    #[inline(never)]
    #[optimize(size)]
    fn guarded(&self, but: Option<PartId>) -> VoiceSet {
        let mut g = VoiceSet::EMPTY;
        if self.shares.reserved() == Reserved::NONE {
            return g;
        }
        for v in VoiceIdx::ALL {
            let s = self.slot(v);
            if let Some(q) = s
                .part()
                .filter(|&q| s.dying.is_none() && s.reserves() && Some(q) != but)
                && self.shares.ask(q).reserve.guards(self.reserving(q).count())
            {
                g = g.with(v);
            }
        }
        g
    }

    /// Rule 3's victim in `pool`: the oldest booked, live voice whose
    /// `mono` is as asked, a tail before a held note; the first of equals.
    #[inline(never)]
    fn oldest(&self, pool: VoiceSet, mono: bool) -> Option<VoiceIdx> {
        let mut best: Option<(VoiceIdx, (bool, u32))> = None;
        for v in VoiceIdx::ALL {
            let s = self.slot(v);
            if !pool.contains(v) || s.is_free() || s.mono != mono || s.dying() {
                continue;
            }
            let key = (s.held(), s.age);
            if best.is_none_or(|(_, b)| key < b) {
                best = Some((v, key));
            }
        }
        best.map(|(v, _)| v)
    }

    /// The voices a note on `part` may steal (ADR 0281): at its cap only
    /// its own, else any but another Part's guarded ones.
    pub fn stealable(&self, part: PartId) -> VoiceSet {
        if self.capped(part) {
            self.live(part)
        } else {
            self.card_stealable(part)
        }
    }

    /// The voices whose stream a note on `part` may stop for card bandwidth
    /// (ADR 0281): any but another Part's guarded ones, at the cap too. The
    /// cap limits only how many voices the Part sounds (ADR 0281, 2026-11-18).
    pub fn card_stealable(&self, part: PartId) -> VoiceSet {
        VoiceSet::ALL.without(self.guarded(Some(part)))
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
        part: PartId,
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

    /// Books `note` on `pick`'s voice `v`, whatever it held: a note, its
    /// key down.
    pub fn book(&mut self, v: VoiceIdx, part: PartId, mode: PartMode, note: MidiNote, cost: Cost) {
        self.book_as(v, (part, mode), note, cost, Strike::Note(Key::Down));
    }

    /// Books DRUM hit `key` on `pick_hit`'s voice `v`: it counts toward
    /// its Part's RESERVE until it ends (ADR 0281).
    pub fn book_hit(
        &mut self,
        v: VoiceIdx,
        part: PartId,
        mode: PartMode,
        key: MidiNote,
        cost: Cost,
    ) {
        self.book_as(v, (part, mode), key, cost, Strike::Hit(Key::Down));
    }

    fn book_as(
        &mut self,
        v: VoiceIdx,
        (part, mode): (PartId, PartMode),
        note: MidiNote,
        cost: Cost,
        strike: Strike,
    ) {
        let mono = mode == PartMode::Mono;
        self.clock = self.next_age();
        self.rr = v.wrapping_add(1);
        self.slots[v.index()] = VoiceSlot {
            booking: Some(Booking { part, note, strike }),
            age: self.clock,
            mono,
            dying: None,
            cost,
            glide: Cost::ZERO,
            fading: Cost::ZERO,
        };
    }

    /// Key up on `voice`: it is no longer held (its tail keeps the slot
    /// until `release_finished`). No-op for a free or already released
    /// voice. The caller picks the voice — the
    /// Instrument matches note and note-on channel, which a (part, note)
    /// lookup cannot tell apart when one Part holds a key from two channels.
    pub fn release(&mut self, voice: VoiceIdx) {
        if let Some(b) = self
            .slots
            .get_mut(voice.index())
            .and_then(|s| s.booking.as_mut())
        {
            b.strike = b.strike.released();
        }
    }

    /// A held `voice`'s note moved to `note` (a re-snap): its age and the
    /// rest stand. No-op for a free voice.
    pub fn renote(&mut self, voice: VoiceIdx, note: MidiNote) {
        if let Some(b) = self
            .slots
            .get_mut(voice.index())
            .and_then(|s| s.booking.as_mut())
        {
            b.note = note;
        }
    }

    /// `voice` was taken from a note of another Sound, which costs
    /// `fading` until it has faded out and the booked note starts; then
    /// `Cost::ZERO` (ADR 0091). No-op for a free voice.
    pub fn fade_from(&mut self, voice: VoiceIdx, fading: Cost) {
        if let Some(s) = self.slots.get_mut(voice.index()).filter(|s| !s.is_free()) {
            s.fading = fading;
        }
    }

    /// The voice now costs `cost` (its Part's Sound changed).
    pub fn recost(&mut self, voice: VoiceIdx, cost: Cost) {
        self.recost_with_glide(voice, cost, Cost::ZERO);
    }

    /// `glide` is a knob glide's extra: it counts toward admission, not
    /// shedding (ADR 0117).
    pub fn recost_with_glide(&mut self, voice: VoiceIdx, cost: Cost, glide: Cost) {
        if let Some(s) = self.slots.get_mut(voice.index()).filter(|s| !s.is_free()) {
            (s.cost, s.glide) = (cost, glide);
        }
    }

    /// If the voices not yet dying, plus `reserved`, are over the budget,
    /// mark one dying and return it for the caller to fade out: non-mono
    /// first, then the oldest tail, then the newest held note. Call until
    /// `None`. Dying voices still count in `sounding_cost`.
    pub fn shed(&mut self, reserved: Cost) -> Option<VoiceIdx> {
        if reserved + self.steady_live_cost() <= self.budget.as_cost() {
            return None;
        }
        self.shed_one(|_| true)
    }

    /// Mark the voice `shed` would pick among those `sounds` accepts dying,
    /// whatever the budget, and return it: the load net's (ADR 0273), which
    /// takes only a voice whose fade frees something. `None` once no booked
    /// voice is both live and accepted.
    pub fn shed_one(&mut self, sounds: impl Fn(VoiceIdx) -> bool) -> Option<VoiceIdx> {
        // A Part at or below its reserve last (ADR 0281).
        let guarded = self.guarded(None);
        let v = VoiceIdx::ALL
            .into_iter()
            .filter(|&v| !self.slot(v).is_free() && !self.slot(v).dying())
            .filter(|&v| sounds(v))
            .min_by_key(|&v| {
                let s = self.slot(v);
                (
                    guarded.contains(v),
                    s.mono,
                    s.held(),
                    if s.held() { u32::MAX - s.age } else { s.age },
                )
            })?;
        self.sheds = self.sheds.wrapping_add(1);
        self.slots[v.index()].dying = Some(self.sheds);
        Some(v)
    }

    /// A project load (ADR 0046): every booked slot dying, as if shed, so
    /// none is re-struck, retriggered or stolen while it fades. Not counted:
    /// nothing was refused.
    pub fn kill_all(&mut self) {
        for v in VoiceIdx::ALL {
            self.kill(v);
        }
    }

    /// One slot dying, as `kill_all` does (CC 120).
    pub fn kill(&mut self, voice: VoiceIdx) {
        if let Some(s) = self
            .slots
            .get_mut(voice.index())
            .filter(|s| !s.is_free() && !s.dying())
        {
            self.sheds = self.sheds.wrapping_add(1);
            s.dying = Some(self.sheds);
        }
    }

    /// The voice's engine went silent: free it if it was released or shed,
    /// or played a DRUM hit, which ends with its voice whatever its pad's
    /// key (ADR 0281, 2026-11-18): a later note-off finds
    /// nothing held.
    pub fn release_finished(&mut self, voice: VoiceIdx) {
        if let Some(s) = self.slots.get_mut(voice.index())
            && (s.dying() || s.strike().is_some_and(Strike::ends_with_voice))
        {
            *s = VoiceSlot::default();
        }
    }

    /// The voice `note_on` would book, without booking it: rules 1–4.
    pub fn pick(
        &self,
        part: PartId,
        mode: PartMode,
        note: MidiNote,
        cost: Cost,
        reserved: Cost,
    ) -> Option<VoiceIdx> {
        self.pick_where(part, mode, note, (cost, reserved), &|_| true, Steal::Any)
    }

    /// A DRUM hit's voice (ADR 0280): no re-strike; rule 3 within `steal`,
    /// a STACK pad's own live voices (`Steal::Only`), its oldest tail
    /// first, never another pad's.
    #[inline(always)]
    pub fn pick_hit(
        &self,
        part: PartId,
        mode: PartMode,
        note: MidiNote,
        cost: Cost,
        reserved: Cost,
        steal: Steal,
    ) -> Option<VoiceIdx> {
        self.pick_where(part, mode, note, (cost, reserved), &|_| false, steal)
    }

    /// `pick`, where rule 2 re-strikes only voices `restrike` accepts: a key
    /// held by another origin is a voice of its own (`Instrument::handle_from`).
    /// Rules 3 and 4 steal within `steal` (ADR 0280) and what the Part's
    /// limits leave stealable (`stealable`, ADR 0281).
    pub fn pick_where(
        &self,
        part: PartId,
        mode: PartMode,
        note: MidiNote,
        (cost, reserved): (Cost, Cost),
        restrike: &dyn Fn(VoiceIdx) -> bool,
        steal: Steal,
    ) -> Option<VoiceIdx> {
        let fits = |freed: Cost| {
            let total = reserved.0 + self.live_cost().0 + cost.0;
            total.saturating_sub(freed.0) <= self.budget.as_cost().0
        };
        // Rule 1: a Mono part retriggers the voice it owns.
        if mode == PartMode::Mono
            && let Some(v) = VoiceIdx::ALL.into_iter().find(|&v| {
                let s = self.slot(v);
                s.mono && !s.dying() && s.part() == Some(part)
            })
        {
            return fits(self.slot(v).cost()).then_some(v);
        }
        // Rule 2: the key's own voice, re-struck (if `restrike` accepts it),
        // if it fits (else on to the rest); else a free voice, round-robin,
        // else the dying one nearest the end of its fade — if it fits.
        if mode != PartMode::Mono
            && let Some(v) = VoiceIdx::ALL.into_iter().find(|&v| {
                let s = self.slot(v);
                !s.mono
                    && !s.dying()
                    && s.part() == Some(part)
                    && s.note() == Some(note)
                    && restrike(v)
            })
            && fits(self.slot(v).cost())
        {
            return Some(v);
        }
        // At its cap a Part steals its own, free voices or not (ADR 0281).
        let capped = self.capped(part);
        let free = (0..MAX_VOICES)
            .map(|i| self.rr.wrapping_add(i))
            .find(|&v| self.slot(v).is_free())
            .or_else(|| {
                VoiceIdx::ALL
                    .into_iter()
                    .filter_map(|v| self.slot(v).dying.map(|d| (d, v)))
                    .min_by_key(|&(d, _)| d)
                    .map(|(_, v)| v)
            });
        if !capped
            && let Some(v) = free
            && fits(Cost::ZERO)
        {
            return Some(v);
        }
        // Rules 3 and 4: steal the oldest non-mono voice — tails before held
        // notes — if that makes room: at the cap the Part's own, else none
        // of a guarded Part's (ADR 0281).
        let pool = steal.pool().and(self.stealable(part));
        // At the cap with no other voice of its own, its mono one (MODE
        // switched while it rang): a capped Part always reuses its own.
        let v = self
            .oldest(pool, false)
            .or_else(|| capped.then(|| self.oldest(pool, true)).flatten())?;
        fits(self.slot(v).cost()).then_some(v)
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
