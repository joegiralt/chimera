// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Streamed admission's work (slice 1 § 5.5, #595): a TAPE note's bill,
//! the pick that steals what the card needs for it, and the block's
//! re-bill. With `bank2` it is bank 2's (`bank2.x` globs this module):
//! bank 1 reaches it only through `AdmitDoor`, from `Instrument`'s
//! wrappers, and keeps the plain pick for every note that doesn't stream.

use core::sync::atomic::Ordering::Relaxed;

use chimera_hal::card_bus::CardFault;
use chimera_theory::Note;

use super::{AudioShared, Instrument, NoteOrigin, Paid};
use crate::MidiNote;
use crate::hw::{Cost, MAX_VOICES};
use crate::params::EngineType;
use crate::part::PartMode;
use crate::project::{PartId, SourceId};
use crate::source_table::GateView;
use crate::stream::admit::{
    Admit, Bill, Raise, ReadRatio, STREAM_BUDGET_BPS, admit_on, raise_room,
};
use crate::stream::stats::StreamStats;
use crate::voice_alloc::{VoiceIdx, VoiceSet};

/// A note-on as the pick sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PickAt {
    pub(crate) part: PartId,
    pub(crate) mode: PartMode,
    pub(crate) note: MidiNote,
    pub(crate) origin: NoteOrigin,
}

/// A note asking the card: the key struck, the note it plays (snapped or
/// harmonised), and its source.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BillAt {
    pub(crate) key: MidiNote,
    pub(crate) note: MidiNote,
    pub(crate) source: Option<SourceId>,
}

/// What bank 1 may call of admission (ADR 0137): bank 2's with `bank2`.
#[repr(C)]
pub struct AdmitDoor {
    bill: fn(&Instrument, PartId, &AudioShared, &GateView<'_>, BillAt) -> Option<Paid>,
    #[allow(clippy::type_complexity)]
    pick: fn(
        &mut Instrument,
        PickAt,
        (Cost, bool),
        Paid,
        (Option<&StreamStats>, CardFault),
    ) -> Option<(VoiceIdx, Option<Paid>)>,
    refresh: fn(&mut Instrument, &AudioShared, &GateView<'_>, Option<&StreamStats>),
    loop_extra: fn(&Instrument, PartId, &AudioShared, &GateView<'_>, BillAt) -> Cost,
    clear_avail: fn(&Instrument) -> usize,
    admit_sym: fn(&mut Instrument, VoiceIdx, &crate::params::ParamSnapshot, MidiNote) -> bool,
    strike_ready: StrikeReady,
    ringing_kits: fn(&Instrument) -> [crate::dsp::drum::sound::KitBill; crate::hw::MAX_PARTS],
    follows: fn(&Instrument) -> [Cost; MAX_VOICES],
    fade_replaced: fn(&mut Instrument, &AudioShared),
    ramps: fn(&AudioShared) -> super::part_sound::Ramps,
}

/// `AdmitDoor::strike_ready`'s entry.
type StrikeReady =
    fn(&mut Instrument, VoiceIdx, &crate::params::ParamSnapshot, MidiNote, super::Waiting) -> bool;

impl AdmitDoor {
    #[inline(always)]
    pub(crate) fn clear_avail(&self, i: &Instrument) -> usize {
        (self.clear_avail)(i)
    }

    #[inline(always)]
    pub(crate) fn admit_sym(
        &self,
        i: &mut Instrument,
        v: VoiceIdx,
        params: &crate::params::ParamSnapshot,
        note: MidiNote,
    ) -> bool {
        (self.admit_sym)(i, v, params, note)
    }

    #[inline(always)]
    pub(crate) fn strike_ready(
        &self,
        i: &mut Instrument,
        v: VoiceIdx,
        params: &crate::params::ParamSnapshot,
        note: MidiNote,
        w: super::Waiting,
    ) -> bool {
        (self.strike_ready)(i, v, params, note, w)
    }

    #[inline(always)]
    pub(crate) fn fade_replaced(&self, i: &mut Instrument, shared: &AudioShared) {
        (self.fade_replaced)(i, shared)
    }

    #[inline(always)]
    pub(crate) fn ramps(&self, shared: &AudioShared) -> super::part_sound::Ramps {
        (self.ramps)(shared)
    }

    #[inline(always)]
    pub(crate) fn follows(&self, i: &Instrument) -> [Cost; MAX_VOICES] {
        (self.follows)(i)
    }

    #[inline(always)]
    pub(crate) fn ringing_kits(
        &self,
        i: &Instrument,
    ) -> [crate::dsp::drum::sound::KitBill; crate::hw::MAX_PARTS] {
        (self.ringing_kits)(i)
    }

    #[inline(always)]
    pub(crate) fn bill(
        &self,
        i: &Instrument,
        p: PartId,
        shared: &AudioShared,
        src: &GateView<'_>,
        at: BillAt,
    ) -> Option<Paid> {
        (self.bill)(i, p, shared, src, at)
    }

    #[inline(always)]
    pub(crate) fn pick(
        &self,
        i: &mut Instrument,
        at: PickAt,
        costs: (Cost, bool),
        new: Paid,
        ctx: (Option<&StreamStats>, CardFault),
    ) -> Option<(VoiceIdx, Option<Paid>)> {
        (self.pick)(i, at, costs, new, ctx)
    }

    #[inline(always)]
    pub(crate) fn refresh(
        &self,
        i: &mut Instrument,
        shared: &AudioShared,
        src: &GateView<'_>,
        stats: Option<&StreamStats>,
    ) {
        (self.refresh)(i, shared, src, stats)
    }

    #[inline(always)]
    pub(crate) fn loop_extra(
        &self,
        i: &Instrument,
        p: PartId,
        shared: &AudioShared,
        src: &GateView<'_>,
        at: BillAt,
    ) -> Cost {
        (self.loop_extra)(i, p, shared, src, at)
    }
}

impl Instrument {
    /// The door's table: every entry is this module's, so `bank2.x`'s
    /// `10instrument9admission` glob links it into bank 2.
    pub const ADMIT: AdmitDoor = AdmitDoor {
        bill: Self::bill_in,
        pick: Self::pick_streamed,
        refresh: Self::refresh_in,
        loop_extra: Self::loop_in,
        clear_avail: Self::clear_avail_in,
        admit_sym: Self::admit_sym_in,
        strike_ready: Self::strike_ready_in,
        ringing_kits: Self::ringing_kits_in,
        follows: Self::follows_in,
        fade_replaced: Self::fade_replaced_in,
        ramps: Self::ramps_in,
    };

    /// What key `key`'s note `note` on Part `p`, reading `source`, asks of
    /// the card (slice 1 § 5.5): its bill at the fastest it can read, from
    /// PITCH, TUNING, every PITCH and FINE route at full swing, the bend's
    /// range and wow; `None` off the card, or while `source` moves. A glide
    /// steal can start from the Part's highest sounding note; a snapping
    /// Part's key can re-snap up to `snap_reach_up` over itself.
    fn bill_in(
        &self,
        p: PartId,
        shared: &AudioShared,
        src: &GateView<'_>,
        BillAt { key, note, source }: BillAt,
    ) -> Option<Paid> {
        use crate::dsp::tape_engine::read::Descriptor;
        use crate::stream::admit::bill;
        if shared.parts[p.index()].params.engine() != EngineType::Tape {
            return None;
        }
        let d = Descriptor::from_bytes(src.bytes(source?)?).filter(Descriptor::is_streamed)?;
        let ratio = self.reach(p, shared, key, note);
        Some(Paid {
            bill: bill(d.rate(), ratio),
            ratio,
        })
    }

    /// What key `key`'s note `note` on Part `p` can read at its fastest,
    /// over its source's own speed: `bill_in`'s reach.
    fn reach(&self, p: PartId, shared: &AudioShared, key: MidiNote, note: MidiNote) -> ReadRatio {
        use crate::addr::{BlockRef, ParamAddr};
        use crate::dsp::engines::pitch_reach_by;
        use crate::dsp::lfo::LfoParams;
        use crate::dsp::modulator::LfoSlot;
        use crate::dsp::tape_engine::{TapeEngineParams, ratio_max, tape_runs};
        use crate::modulation::{ModSource, amount_scale};
        use crate::stream::admit::WOW_CENTS;
        use crate::stream::units::Cents;
        use chimera_theory::snap_reach_up;
        let part = &shared.parts[p.index()];
        let mut top = note;
        if part.params.pitch.steal_glide().is_some() {
            for s in self.alloc.slots().iter().filter(|s| s.part() == Some(p)) {
                top = top.max(s.note().unwrap_or(top));
            }
        }
        // HELD may turn to RE-SNAP mid-note: any snapping Part's key may.
        if part.mix.snap.target().is_some() {
            let k = key.get();
            let up = Note::new(k).map_or(11, snap_reach_up);
            top = top.max(MidiNote::clamped(k.saturating_add(up)));
        }
        // The reach in use (#698): each route by its source's live swing
        // (an LFO by its DEPTH and DEPTH's routes, the wheel's where it sits,
        // so INIT's wheel vibrato at rest reaches nothing), wow only while
        // TAPE runs, and the bend the wheel holds now, up. A wheel or DEPTH
        // moving up re-bills it (`refresh_in`).
        let mods = &part.mod_state;
        let wheel = self.knobs[p.index()].0.wheel();
        let lfo_depth = |s: usize| {
            let slot = match ModSource::ALL.get(s) {
                Some(ModSource::Lfo1) => LfoSlot::Lfo1,
                Some(ModSource::Lfo2) => LfoSlot::Lfo2,
                Some(ModSource::Lfo3) => LfoSlot::Lfo3,
                _ => return 1.0,
            };
            let depth = ParamAddr::new(BlockRef::Lfo(slot), LfoParams::DEPTH);
            let routed = mods.find(depth).map_or(0.0, |d| {
                (0..mods.num_sources())
                    .map(|r| {
                        let reach = if r == ModSource::Wheel.index() {
                            wheel
                        } else {
                            1.0
                        };
                        amount_scale(mods.amount(r, d)).abs() * reach
                    })
                    .sum()
            });
            crate::dsp::clampf(part.params.lfos[slot.index()].depth + routed, 0.0, 1.0)
        };
        let routes = pitch_reach_by(mods, lfo_depth);
        let tape = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::TAPE);
        let wow = if tape_runs(part.params.tape.tape, mods.moves_addr(tape)) {
            WOW_CENTS.get()
        } else {
            0.0
        };
        let bend = Cents::new(self.bend_up(p, part) * 100.0);
        let cents = (part.params.pitch.semitones() + self.tune) * 100.0 + routes + wow;
        ratio_max(&part.params.tape, top, Cents::new(cents), bend)
    }

    /// What key `key`'s note `note` on Part `p` bills over its TAPE model's
    /// for its loop (`heads::loop_extra`, ADR 0211): the loop as the note
    /// lays it out on `source`, at the fastest it reads. A SIZE, START, END
    /// or XFADE route can make it any loop: billed the shortest, faded
    /// equal-power. Nothing off TAPE, or with no source to read.
    fn loop_in(
        &self,
        p: PartId,
        shared: &AudioShared,
        src: &GateView<'_>,
        BillAt { key, note, source }: BillAt,
    ) -> Cost {
        use crate::addr::{BlockRef, ParamAddr};
        use crate::dsp::tape_engine::heads::{
            Bounds, Sizing, loop_extra, shortest, window_end_min, window_start,
        };
        use crate::dsp::tape_engine::read::SourceView;
        use crate::dsp::tape_engine::{INC_MAX, TapeEngineParams, Way};
        let part = &shared.parts[p.index()];
        let t = &part.params.tape;
        let way = t.way();
        if part.params.engine() != EngineType::Tape
            || !matches!(way, Way::Loop | Way::Tron | Way::Ping)
        {
            return Cost::ZERO;
        }
        let Some(v) = source.and_then(|s| src.bytes(s)).and_then(SourceView::new) else {
            return Cost::ZERO;
        };
        let rate = v.desc().rate();
        let ratio = self.reach(p, shared, key, note).get();
        let inc = (f32::from(rate) / crate::hw::SAMPLE_RATE as f32 * ratio).min(INC_MAX);
        let frames = v.file_frames();
        let floor = shortest(rate, inc);
        let routed = [
            TapeEngineParams::SIZE,
            TapeEngineParams::START,
            TapeEngineParams::END,
            TapeEngineParams::XFADE,
        ]
        .iter()
        .any(|&q| {
            part.mod_state
                .moves_addr(ParamAddr::new(BlockRef::TapeEngine, q))
        });
        let (len, fade) = if routed {
            let len = floor.min(frames.get());
            (len, len / 2)
        } else {
            let ws = window_start(t.start, frames);
            let we = window_end_min(ws, t.end, frames.get(), floor);
            let xfade = if way == Way::Ping { 0.0 } else { t.xfade };
            let sizing = Sizing {
                size: t.size,
                place: 0.0,
                xfade,
            };
            let b = Bounds::of_min(ws.min(we), ws.max(we), sizing, rate, floor);
            (b.frames(), b.lx())
        };
        loop_extra(way, len, fade, rate, inc)
    }

    /// Part `p`'s bend up, semitones: the wheel's glided reach or where the
    /// wheel now sits, the higher; 0 bent down.
    fn bend_up(&self, p: PartId, part: &super::PartAudio) -> f32 {
        let w = self.wheel[usize::from(part.mix.channel.get())].max(0);
        let target = f32::from(w) / 8191.0 * f32::from(part.mix.bend);
        self.knobs[p.index()].0.bend().max(target).max(0.0)
    }

    /// Admits a streamed note billed `new` onto voice `on`'s ring (`None`:
    /// a voice the note takes whole), against every
    /// other voice streaming now (`admit_on`): a dying voice's bill counts
    /// until it is freed and it is never a victim (#764); `Steal(w)`
    /// frees one live voice, released first, the oldest (umbrella open
    /// question 2), for the note to take behind its fade. Under a card
    /// fault nothing streams: refused, no voice stolen. Only a voice that
    /// isn't another Part's guarded one goes (`Allocator::card_stealable`,
    /// ADR 0281), at the note's Part's cap too: the cap limits voices, not
    /// bandwidth.
    fn admit_stream(
        &self,
        new: Bill,
        on: Option<VoiceIdx>,
        part: PartId,
        fault: CardFault,
    ) -> Admit {
        if fault != CardFault::Clear {
            return Admit::Refuse;
        }
        let others = VoiceIdx::ALL
            .iter()
            .zip(&self.streaming)
            .filter(|&(&v, _)| Some(v) != on)
            .filter_map(|(&v, s)| Some((v, s.billed()?, s.age)));
        let here = on
            .and_then(|v| self.streaming[v.index()].billed())
            .unwrap_or(Bill::ZERO);
        admit_on(
            new,
            here,
            others,
            STREAM_BUDGET_BPS,
            self.card_victims(part),
        )
    }

    /// The voices a card steal for Part `part` may take: its stealable
    /// ones, none dying (#764).
    fn card_victims(&self, part: PartId) -> VoiceSet {
        let dying = VoiceIdx::ALL
            .into_iter()
            .filter(|&v| self.alloc.slot(v).dying())
            .fold(VoiceSet::EMPTY, VoiceSet::with);
        self.alloc.card_stealable(part).without(dying)
    }

    /// Admits voice `v`'s bill `new` mid-note (#595): it fits beside every
    /// other voice's bill, a dying one's too (#764). Else released voices
    /// go, the first `Age::rank` gives, each fading out as a shed one does,
    /// until the live voices leave room; the note holds at its cap until
    /// their fades end and a later block admits it. A held note is never
    /// cut, nor a dying one taken; if even every released voice wouldn't
    /// make room, none goes. Only voices that aren't another Part's
    /// guarded ones go (`Allocator::card_stealable`, ADR 0281). False: it
    /// doesn't fit now.
    fn admit_mid(&mut self, v: VoiceIdx, new: Bill) -> bool {
        let Some(part) = self.alloc.slot(v).part() else {
            return false;
        };
        let alloc = &self.alloc;
        let others = VoiceIdx::ALL
            .iter()
            .zip(&self.streaming)
            .filter(|&(&w, _)| w != v)
            .filter_map(|(&w, s)| Some((w, s.billed()?, s.age, alloc.slot(w).dying())));
        match raise_room(new, others, STREAM_BUDGET_BPS, self.card_victims(part)) {
            Raise::Fits => true,
            Raise::Hold => false,
            Raise::Wait(gone) => {
                for w in VoiceIdx::ALL.into_iter().filter(|&w| gone.contains(w)) {
                    self.alloc.kill(w);
                    self.shed(w);
                }
                false
            }
        }
    }

    /// Voice `v`'s note, paid for, now reaches `new` (#595): re-admitted
    /// (`admit_mid`), its cap following; or it holds at its cap, counted
    /// once a rise.
    fn raise(&mut self, v: VoiceIdx, new: Paid, stats: Option<&StreamStats>) {
        let st = self.streaming[v.index()];
        let Some(old) = st.paid.filter(|old| new.ratio > old.ratio) else {
            return;
        };
        let up = Paid {
            bill: new.bill.max(old.bill),
            ratio: new.ratio,
        };
        let i = v.index();
        if self.admit_mid(v, up.bill) {
            (self.streaming[i].paid, self.streaming[i].capped) = (Some(up), false);
        } else if !st.capped {
            self.streaming[i].capped = true;
            if let Some(s) = stats {
                s.held_at_cap.fetch_add(1, Relaxed);
            }
        }
    }

    /// Once a block, before the voices render (#595): each sounding streamed
    /// note's bill, as its Part's PITCH, TUNING, bend range, routes and SNAP
    /// now reach, re-admitted when it rose (`raise`). A note whose source
    /// moved at note-on is admitted once it reads, released voices going,
    /// or refused, counted, and freed. Eight voices' bills at most.
    fn refresh_in(
        &mut self,
        shared: &AudioShared,
        src: &GateView<'_>,
        stats: Option<&StreamStats>,
    ) {
        for vi in VoiceIdx::ALL {
            let v = vi.index();
            let s = *self.alloc.slot(vi);
            let (Some(q), Some(note)) = (s.part(), s.note()) else {
                continue;
            };
            let voice = &self.voices[v];
            if s.dying() || !voice.is_active() || voice.fading() || self.waiting[v].is_some() {
                continue;
            }
            let at = BillAt {
                key: self.keys[v],
                note,
                source: voice.reads()[0],
            };
            let Some(new) = self.bill_in(q, shared, src, at) else {
                continue;
            };
            if self.streaming[v].paid.is_some() {
                self.raise(vi, new, stats);
            } else if self.admit_mid(vi, new.bill) {
                self.streaming[v].paid = Some(new);
            } else {
                if let Some(s) = stats {
                    s.late_refused.fetch_add(1, Relaxed);
                }
                self.alloc.kill(vi);
                self.shed(vi);
            }
        }
    }

    /// The voice a streamed note-on books (`Allocator::pick_where`), and
    /// the card's room for it (`admit_on`, #764): on the picked voice's own
    /// ring (a free or dying one, the note's own, or any at the Part's cap)
    /// if it fits; else, a note free to move (not a MONO retrigger or a
    /// re-strike, its Part under its cap), behind a fade already running
    /// (a cut's, a shed's) that has room, else behind one live victim it
    /// steals, the top-ranked; else refused, nothing stolen. A STEAL GLIDE
    /// retrigger carries on the held note, admitted mid-note (`raise`).
    /// Returns what the note is paid; `None`: refused, counted.
    fn pick_streamed(
        &mut self,
        at: PickAt,
        (cost, glides): (Cost, bool),
        new: Paid,
        (stats, fault): (Option<&StreamStats>, CardFault),
    ) -> Option<(VoiceIdx, Option<Paid>)> {
        if let Some(vi) = self.pick_one(at, cost) {
            let v = vi.index();
            let s = *self.alloc.slot(vi);
            let replaces = !s.dying()
                && s.part() == Some(at.part)
                && (at.mode == PartMode::Mono || s.note() == Some(at.note));
            if replaces && glides && self.voices[v].is_active() && self.streaming[v].paid.is_some()
            {
                self.raise(vi, new, stats);
                return Some((vi, self.streaming[v].paid));
            }
            // At its cap a Part keeps to the voice the allocator gave it
            // (ADR 0281): taking another's would be one over.
            let capped = self.alloc.capped(at.part);
            // A free or dying voice, the note's own, or a capped Part's: the
            // note's ring. A live one the allocator would steal stays
            // billed, so the card's own victim rule picks (`admit_on`).
            let ring = (s.is_free() || s.dying() || replaces || capped).then_some(vi);
            let first = self.admit_stream(new.bill, ring, at.part, fault);
            if first == Admit::Ok {
                return Some((vi, Some(new)));
            }
            // A note bound to its own voice (a MONO retrigger, a re-strike)
            // can't move: its old note would hold on beside it, two voices.
            let moves = !replaces && !capped;
            // Behind a fade already running (a cut's, a shed's), the note
            // shares that ring's bill: no steal (#764, #706).
            let behind = VoiceIdx::ALL.into_iter().filter(|_| moves).find(|&d| {
                self.alloc.slot(d).dying()
                    && self.admit_stream(new.bill, Some(d), at.part, fault) == Admit::Ok
            });
            if let Some(d) = behind {
                return Some((d, Some(new)));
            }
            // Else one live victim, the note behind its fade: one ring, one
            // bill. Not for a note that can't move: the victim's bill stays
            // until its fade ends, so refused, nothing stolen.
            if let Admit::Steal(w) = first
                && moves
            {
                self.alloc.kill(w);
                self.shed(w);
                return Some((w, Some(new)));
            }
        }
        self.alloc.refuse();
        None
    }
}
