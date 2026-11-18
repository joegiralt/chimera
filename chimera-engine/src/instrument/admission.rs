//! Streamed admission's work (slice 1 § 5.5, #595): a TAPE note's bill,
//! the pick that steals what the card needs for it, and the block's
//! re-bill. With `bank2` it is bank 2's (`bank2.x` globs this module):
//! bank 1 reaches it only through `AdmitDoor`, from `Instrument`'s
//! wrappers, and keeps the plain pick for every note that doesn't stream.

use core::sync::atomic::Ordering::Relaxed;

use chimera_hal::card_bus::CardFault;
use chimera_theory::Note;

use chimera_hal::units::BytesPerSec;

use super::{AudioShared, Instrument, NoteOrigin, Paid};
use crate::MidiNote;
use crate::hw::{Cost, MAX_VOICES};
use crate::params::EngineType;
use crate::part::PartMode;
use crate::project::{PartId, SourceId};
use crate::source_table::GateView;
use crate::stream::admit::{Admit, Age, Bill, ReadRatio, STREAM_BUDGET_BPS, admit};
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
    ) -> Option<(usize, Option<Paid>)>,
    refresh: fn(&mut Instrument, &AudioShared, &GateView<'_>, Option<&StreamStats>),
    loop_extra: fn(&Instrument, PartId, &AudioShared, &GateView<'_>, BillAt) -> Cost,
}

impl AdmitDoor {
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
    ) -> Option<(usize, Option<Paid>)> {
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

    /// Admits a streamed note billed `new` against the voices streaming
    /// now, but those `gone` (stolen already, or replaced by the note):
    /// `Steal(v)` frees one more, released first, the oldest (umbrella open
    /// question 2). A voice still fading counts until it is freed; a shed
    /// one goes first. Under a card fault nothing streams: refused, no
    /// voice stolen. Only a voice the note's Part may steal goes
    /// (`Allocator::stealable`: its own at its cap, never a guarded Part's,
    /// ADR 0281).
    fn admit_stream(&self, new: Bill, gone: VoiceSet, part: PartId, fault: CardFault) -> Admit {
        if fault == CardFault::Read {
            return Admit::Refuse;
        }
        let playing = VoiceIdx::ALL
            .iter()
            .zip(&self.streaming)
            .filter_map(|(&v, s)| {
                let b = s.billed().filter(|_| !gone.contains(v))?;
                Some((v, b, self.age_now(v, s.age)))
            });
        admit(new, playing, STREAM_BUDGET_BPS, self.alloc.stealable(part))
    }

    /// Voice `v`'s age as a steal sees it: shed or cut this block (dying
    /// before its block's `age_stream`), the first to go.
    fn age_now(&self, v: VoiceIdx, age: Age) -> Age {
        if self.alloc.slots()[v.index()].dying() {
            Age::DYING
        } else {
            age
        }
    }

    /// Admits voice `v`'s bill `new` mid-note (#595): only released voices
    /// go, each fading out as a shed one does; a held note is never cut.
    /// Only voices `v`'s Part may steal go (ADR 0281). False: it doesn't
    /// fit, nothing stolen.
    fn admit_mid(&mut self, v: VoiceIdx, new: Bill) -> bool {
        let mut gone = VoiceSet::EMPTY.with(v);
        let Some(part) = self.alloc.slot(v).part() else {
            return false;
        };
        // Each steal adds a voice to `gone`: at most one a voice.
        for _ in 0..=MAX_VOICES {
            let st = &self.streaming;
            let others = || {
                VoiceIdx::ALL
                    .iter()
                    .zip(st)
                    .filter(|&(&w, _)| !gone.contains(w))
                    .filter_map(|(&w, s)| Some((w, s.billed()?, self.age_now(w, s.age))))
            };
            let held = others()
                .filter(|o| !o.2.released())
                .fold(0u32, |a, o| a.saturating_add(o.1.get()));
            let released = others().filter(|o| o.2.released());
            let room = BytesPerSec::new(STREAM_BUDGET_BPS.get().saturating_sub(held));
            match admit(new, released, room, self.alloc.stealable(part)) {
                Admit::Ok => return true,
                Admit::Steal(w) => {
                    gone = gone.with(w);
                    self.alloc.kill(w.index());
                    self.shed(w.index());
                }
                Admit::Refuse => return false,
            }
        }
        false
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
            let s = self.alloc.slots()[v];
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
                self.alloc.kill(v);
                self.shed(v);
            }
        }
    }

    /// The voice a streamed note-on books (`Allocator::pick_where`), once
    /// its admission has freed what the card needs: each voice it steals
    /// fades out through the allocator, as a shed one does, and the pick
    /// is asked again, so the note takes a stolen voice when it needs one.
    /// A Mono retrigger or a re-strike replaces its voice's stream; a STEAL
    /// GLIDE one carries on the held note, so it is admitted mid-note
    /// (`raise`). Returns what the note is paid; `None`: refused, counted.
    fn pick_streamed(
        &mut self,
        at: PickAt,
        (cost, glides): (Cost, bool),
        new: Paid,
        (stats, fault): (Option<&StreamStats>, CardFault),
    ) -> Option<(usize, Option<Paid>)> {
        let mut gone = VoiceSet::EMPTY;
        // Each steal adds a voice to `gone`: at most one a voice.
        for _ in 0..=MAX_VOICES {
            let Some(vi) = self.pick_one(at, cost).and_then(VoiceIdx::new) else {
                break;
            };
            let v = vi.index();
            let s = self.alloc.slots()[v];
            let replaces = !s.dying()
                && s.part() == Some(at.part)
                && (at.mode == PartMode::Mono || s.note() == Some(at.note));
            if replaces && glides && self.voices[v].is_active() && self.streaming[v].paid.is_some()
            {
                self.raise(vi, new, stats);
                return Some((v, self.streaming[v].paid));
            }
            let ask = if replaces { gone.with(vi) } else { gone };
            match self.admit_stream(new.bill, ask, at.part, fault) {
                Admit::Ok => return Some((v, Some(new))),
                Admit::Steal(w) => {
                    gone = gone.with(w);
                    self.alloc.kill(w.index());
                    self.shed(w.index());
                }
                Admit::Refuse => break,
            }
        }
        self.alloc.refuse();
        None
    }
}
