//! Notes and MIDI real-time (ORBIT spec § 6.1) from the input threads to the
//! audio thread, in the order they came (instrument-core spec
//! § Threading): a fixed 64-event single-producer single-consumer ring.
//! Lock-free and allocation-free; each event is packed into one `AtomicU32`,
//! so there is no `unsafe`. A full queue drops the event and counts it. The
//! last `NOTE_OFF_RESERVE` slots take only note-offs, so a queue held through a
//! project load's card reads (ADR 0099) still takes the note-offs: a
//! dropped note-off would leave a note stuck.
//!
//! Ordering: the Cortex-M7 is single-core, so there is no cache-coherency
//! problem to solve here — every access sees the same physical memory.
//! What `Acquire`/`Release` still buys us is a *reordering* barrier, for
//! both the compiler and the core's out-of-order pipeline: without it,
//! the slot write in `push` could be observed after the `tail` bump that
//! advertises it, or the slot read in `pop` could be hoisted past the
//! `head` bump that frees the slot for reuse, and the producer and
//! consumer (an interrupt handler and a foreground loop, or two real
//! cores on other hardware) would disagree about which slot is safe to
//! touch. A `Release` store paired with an `Acquire` load of the *same*
//! atomic forms a synchronizes-with edge, which makes everything the
//! writer did before the `Release` visible to the reader after the
//! matching `Acquire`. Concretely: `push`'s `Relaxed` slot store
//! happens-before `pop`'s `Relaxed` slot load because it is sequenced
//! before `push`'s `Release` store to `tail`, which `pop`'s `Acquire`
//! load of `tail` synchronizes with. The same reasoning runs the other
//! way for `head`, so the producer never reuses a slot the consumer is
//! still reading. `Relaxed` is sufficient for the slot accesses
//! themselves and for each side reading back its *own* index, since only
//! one thread ever writes `head` and only one thread ever writes `tail`.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use chimera_midi::stream::{ChannelMode as StreamMode, Voice};
use chimera_midi::value::U7;

use crate::{MidiChannel, MidiNote, Velocity};

pub const NOTE_QUEUE_LEN: usize = 64;
const _: () = assert!(NOTE_QUEUE_LEN.is_power_of_two());
/// Free slots kept for note-offs: any other event (a note-on, real-time)
/// that would leave fewer is dropped.
pub const NOTE_OFF_RESERVE: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    On(Velocity),
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    pub channel: MidiChannel,
    pub note: MidiNote,
    pub kind: NoteKind,
}

impl NoteEvent {
    /// Bits 0..4 channel, 4..11 note, 11..18 velocity (0 = note-off).
    fn pack(self) -> u32 {
        let vel = match self.kind {
            NoteKind::On(v) => v.get(),
            NoteKind::Off => 0,
        };
        self.channel.get() as u32 | ((self.note.get() as u32) << 4) | ((vel as u32) << 11)
    }

    fn unpack(bits: u32) -> Option<Self> {
        Some(Self {
            channel: MidiChannel::new((bits & 0xF) as u8)?,
            note: MidiNote::new(((bits >> 4) & 0x7F) as u8)?,
            kind: match Velocity::new(((bits >> 11) & 0x7F) as u8) {
                Some(v) => NoteKind::On(v),
                None => NoteKind::Off,
            },
        })
    }
}

/// System real-time: the MIDI clock and its transport (spec § 6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealTime {
    Clock,
    Start,
    Continue,
    Stop,
}

impl RealTime {
    const ALL: [RealTime; 4] = [
        RealTime::Clock,
        RealTime::Start,
        RealTime::Continue,
        RealTime::Stop,
    ];
}

/// A channel's pitch-bend wheel: -8192..=8191, 0 at rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BendEvent {
    pub channel: MidiChannel,
    pub value: i16,
}

/// A channel's sustain pedal (CC 64): down at 64 and above.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SustainEvent {
    pub channel: MidiChannel,
    pub down: bool,
}

/// The channel-mode messages (CC 120, 121, 123) a Part obeys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelMode {
    /// CC 120: fade everything out now.
    SoundOff,
    /// CC 121: bend to centre, pedal up.
    ResetControllers,
    /// CC 123: let go of every held key (the pedal still holds).
    NotesOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelModeEvent {
    pub channel: MidiChannel,
    pub mode: ChannelMode,
}

/// The hand's expression a matrix source reads (WHEEL, PRESSURE).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expr {
    /// The mod wheel, CC 1.
    Wheel(U7),
    /// Channel pressure.
    Pressure(U7),
    /// Poly pressure on one key.
    Poly(MidiNote, U7),
    /// CC 74, timbre (MPE's third dimension, ADR 0252).
    Timbre(U7),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExprEvent {
    pub channel: MidiChannel,
    pub expr: Expr,
}

/// What a note queue carries: notes, the performance controls the Parts
/// take (bend, sustain, channel mode, wheel and pressure) and real-time, in
/// the order they came.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InEvent {
    Note(NoteEvent),
    Bend(BendEvent),
    Sustain(SustainEvent),
    Mode(ChannelModeEvent),
    Rt(RealTime),
    Expr(ExprEvent),
}

impl From<NoteEvent> for InEvent {
    fn from(n: NoteEvent) -> Self {
        InEvent::Note(n)
    }
}

impl From<RealTime> for InEvent {
    fn from(rt: RealTime) -> Self {
        InEvent::Rt(rt)
    }
}

/// Bits 18 up: 0 a note, 1 + the `RealTime`'s index, then bend, sustain,
/// mode, wheel, pressure, poly pressure and timbre. Below them a note's 18
/// bits.
const KIND_SHIFT: u32 = 18;
const KIND_BEND: usize = RealTime::ALL.len() + 1;
const KIND_SUSTAIN: usize = KIND_BEND + 1;
const KIND_MODE: usize = KIND_SUSTAIN + 1;
const KIND_WHEEL: usize = KIND_MODE + 1;
const KIND_PRESSURE: usize = KIND_WHEEL + 1;
const KIND_POLY: usize = KIND_PRESSURE + 1;
const KIND_TIMBRE: usize = KIND_POLY + 1;
const _: () = assert!(KIND_TIMBRE < 1 << (32 - KIND_SHIFT));
/// The CC that is the sustain pedal.
const CC_SUSTAIN: u8 = 64;
/// The CC that is the mod wheel.
const CC_WHEEL: u8 = 1;
/// The CC that is timbre (MPE's, and the MIDI spec's brightness).
const CC_TIMBRE: u8 = 74;
/// Free slots an expression event leaves: a flood of pressure takes half the
/// queue at most, so it never crowds out a note-on.
const EXPR_RESERVE: usize = NOTE_QUEUE_LEN / 2;

impl InEvent {
    /// The note path's share of a layer-1 voice message: notes, bend and
    /// the pedal (CC 64). Everything else is layer 2's.
    pub fn from_event(channel: MidiChannel, v: &Voice) -> Option<Self> {
        Some(match *v {
            Voice::NoteOn { key, vel } => InEvent::Note(NoteEvent {
                channel,
                note: key,
                kind: NoteKind::On(vel),
            }),
            Voice::NoteOff { key, .. } => InEvent::Note(NoteEvent {
                channel,
                note: key,
                kind: NoteKind::Off,
            }),
            Voice::PitchBend(b) => InEvent::Bend(BendEvent {
                channel,
                value: b.get(),
            }),
            Voice::Control { cc, value } if cc.get() == CC_SUSTAIN => {
                InEvent::Sustain(SustainEvent {
                    channel,
                    down: value.get() >= 64,
                })
            }
            _ => return None,
        })
    }

    /// A matrix source's share of a voice message: the wheel (CC 1),
    /// channel and poly pressure, timbre (CC 74). CC 1, CC 74 and channel
    /// pressure reach the map too (layer 2's), so learn still sees them.
    pub fn expr(channel: MidiChannel, v: &Voice) -> Option<Self> {
        let expr = match *v {
            Voice::Control { cc, value } if cc.get() == CC_WHEEL => Expr::Wheel(value),
            Voice::Control { cc, value } if cc.get() == CC_TIMBRE => Expr::Timbre(value),
            Voice::ChannelPressure(value) => Expr::Pressure(value),
            Voice::PolyPressure { key, value } => Expr::Poly(key, value),
            _ => return None,
        };
        Some(InEvent::Expr(ExprEvent { channel, expr }))
    }

    /// Channel mode: 120 sound off, 121 reset, 123-127 notes off (the Omni
    /// and Mono/Poly switches release held keys, as the Parts have no
    /// modes); 122 local control is nothing here.
    pub fn from_mode(channel: MidiChannel, m: StreamMode) -> Option<Self> {
        let mode = match m {
            StreamMode::AllSoundOff => ChannelMode::SoundOff,
            StreamMode::ResetAllControllers => ChannelMode::ResetControllers,
            StreamMode::LocalControl(_) => return None,
            _ => ChannelMode::NotesOff,
        };
        Some(InEvent::Mode(ChannelModeEvent { channel, mode }))
    }

    /// A note packs as `NoteEvent::pack`, kind 0.
    fn pack(self) -> u32 {
        match self {
            InEvent::Note(n) => n.pack(),
            InEvent::Rt(rt) => (rt as u32 + 1) << KIND_SHIFT,
            InEvent::Bend(b) => {
                let v = (b.value.clamp(-8192, 8191) + 8192) as u32;
                b.channel.get() as u32 | (v << 4) | ((KIND_BEND as u32) << KIND_SHIFT)
            }
            InEvent::Mode(m) => {
                m.channel.get() as u32 | ((m.mode as u32) << 4) | ((KIND_MODE as u32) << KIND_SHIFT)
            }
            InEvent::Sustain(p) => {
                p.channel.get() as u32
                    | ((p.down as u32) << 4)
                    | ((KIND_SUSTAIN as u32) << KIND_SHIFT)
            }
            // A note's layout: channel, key, value.
            InEvent::Expr(e) => {
                let (kind, key, v) = match e.expr {
                    Expr::Wheel(v) => (KIND_WHEEL, 0, v),
                    Expr::Pressure(v) => (KIND_PRESSURE, 0, v),
                    Expr::Poly(k, v) => (KIND_POLY, k.get(), v),
                    Expr::Timbre(v) => (KIND_TIMBRE, 0, v),
                };
                e.channel.get() as u32
                    | (key as u32) << 4
                    | (v.get() as u32) << 11
                    | (kind as u32) << KIND_SHIFT
            }
        }
    }

    fn unpack(bits: u32) -> Option<Self> {
        match (bits >> KIND_SHIFT) as usize {
            0 => NoteEvent::unpack(bits).map(InEvent::Note),
            KIND_BEND => Some(InEvent::Bend(BendEvent {
                channel: MidiChannel::new((bits & 0xF) as u8)?,
                value: ((bits >> 4) & 0x3FFF) as i16 - 8192,
            })),
            KIND_SUSTAIN => Some(InEvent::Sustain(SustainEvent {
                channel: MidiChannel::new((bits & 0xF) as u8)?,
                down: bits & 0x10 != 0,
            })),
            KIND_MODE => Some(InEvent::Mode(ChannelModeEvent {
                channel: MidiChannel::new((bits & 0xF) as u8)?,
                mode: match (bits >> 4) & 3 {
                    0 => ChannelMode::SoundOff,
                    1 => ChannelMode::ResetControllers,
                    2 => ChannelMode::NotesOff,
                    _ => return None,
                },
            })),
            k @ (KIND_WHEEL | KIND_PRESSURE | KIND_POLY | KIND_TIMBRE) => {
                let v = U7::new(((bits >> 11) & 0x7F) as u8)?;
                Some(InEvent::Expr(ExprEvent {
                    channel: MidiChannel::new((bits & 0xF) as u8)?,
                    expr: match k {
                        KIND_WHEEL => Expr::Wheel(v),
                        KIND_PRESSURE => Expr::Pressure(v),
                        KIND_TIMBRE => Expr::Timbre(v),
                        _ => Expr::Poly(MidiNote::new(((bits >> 4) & 0x7F) as u8)?, v),
                    },
                }))
            }
            k => RealTime::ALL.get(k - 1).copied().map(InEvent::Rt),
        }
    }
}

/// One producer and one consumer, each a handle `split` hands out once.
/// Head and tail are free-running counters; the slot is
/// `counter % NOTE_QUEUE_LEN`.
pub struct NoteQueue {
    slots: [AtomicU32; NOTE_QUEUE_LEN],
    /// Next event to pop (written by the consumer only).
    head: AtomicU32,
    /// Next slot to push (written by the producer only).
    tail: AtomicU32,
    dropped: AtomicU32,
    split: AtomicBool,
}

/// The queue's only writer.
pub struct NoteProducer<'a>(&'a NoteQueue);

/// The queue's only reader.
pub struct NoteConsumer<'a>(&'a NoteQueue);

impl NoteProducer<'_> {
    /// `false`: the queue was full and the event was dropped. Inlined into
    /// each pipe in bank 2: out of line, it lands in bank 1.
    #[inline(always)]
    pub fn push(&mut self, ev: impl Into<InEvent>) -> bool {
        self.0.push(ev.into())
    }
}

impl NoteConsumer<'_> {
    pub fn pop(&mut self) -> Option<InEvent> {
        self.0.pop()
    }
}

impl Default for NoteQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteQueue {
    pub const fn new() -> Self {
        Self {
            slots: [const { AtomicU32::new(0) }; NOTE_QUEUE_LEN],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dropped: AtomicU32::new(0),
            split: AtomicBool::new(false),
        }
    }

    /// The producer and consumer ends, once.
    pub fn split(&self) -> Option<(NoteProducer<'_>, NoteConsumer<'_>)> {
        (!self.split.swap(true, Ordering::AcqRel))
            .then_some((NoteProducer(self), NoteConsumer(self)))
    }

    /// Producer side. `false`: the queue was full and the event was dropped,
    /// or it wasn't a note-off (or pedal up) and only `NOTE_OFF_RESERVE` slots were free.
    ///
    /// Ordering: `tail` is read back `Relaxed` since only this thread ever
    /// writes it. `head` is `Acquire`, pairing with `pop`'s `Release` store
    /// to `head`, so a slot the consumer just finished reading is visible
    /// as free before we overwrite it. The slot `store` is `Relaxed`; it
    /// is published by the `Release` store to `tail` right after it, which
    /// is what `pop`'s `Acquire` load of `tail` synchronizes with, so the
    /// slot write always happens-before the matching read in `pop`.
    /// Inlined into the pipes in bank 2 (`NoteProducer::push`).
    #[inline(always)]
    fn push(&self, ev: InEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        // A velocity-0 note-on is an `Off` already (`unpack`, `from_event`).
        // A pedal up and a channel-mode message are let through as well:
        // dropped, they leave notes stuck.
        let note_off = matches!(
            ev,
            InEvent::Note(NoteEvent {
                kind: NoteKind::Off,
                ..
            }) | InEvent::Sustain(SustainEvent { down: false, .. })
                | InEvent::Mode(_)
        );
        let limit = NOTE_QUEUE_LEN
            - match ev {
                _ if note_off => 0,
                InEvent::Expr(_) => EXPR_RESERVE,
                _ => NOTE_OFF_RESERVE,
            };
        if tail.wrapping_sub(head) as usize >= limit {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.slots[tail as usize % NOTE_QUEUE_LEN].store(ev.pack(), Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer side (the audio thread).
    ///
    /// Ordering: symmetric with `push`. `head` is read back `Relaxed`
    /// (only this thread writes it); `tail` is `Acquire`, pairing with
    /// `push`'s `Release` store to `tail`, so the slot write in `push` is
    /// guaranteed visible before the `Relaxed` slot load here. The
    /// `Release` store to `head` afterward is what `push`'s `Acquire` load
    /// of `head` pairs with, so the producer never reuses this slot before
    /// this read of it has completed.
    fn pop(&self) -> Option<InEvent> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let bits = self.slots[head as usize % NOTE_QUEUE_LEN].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        InEvent::unpack(bits)
    }

    /// Events dropped because the queue was full.
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}

pub const MAX_NOTE_SOURCES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceId<const N: usize>(usize);

impl<const N: usize> SourceId<N> {
    pub const fn new(index: usize) -> Self {
        assert!(index < N, "note source index out of range");
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0
    }
}

/// One `NoteQueue` per note source. `split` hands out one producer per
/// source and the one `NoteDrain`, which empties every queue in source
/// order (source 0 first) so a caller processing note-offs before note-ons
/// within a block sees a deterministic, if not timestamped, order.
pub struct NoteSources<const N: usize> {
    queues: [NoteQueue; N],
    split: AtomicBool,
}

/// Every source's consumer.
pub struct NoteDrain<'a, const N: usize>(&'a NoteSources<N>);

/// The sources' producers, each taken once by its `SourceId`.
pub struct NoteProducers<'a, const N: usize>([Option<NoteProducer<'a>>; N]);

impl<'a, const N: usize> NoteProducers<'a, N> {
    /// Source `id`'s producer; `None` once taken.
    pub fn take(&mut self, id: SourceId<N>) -> Option<NoteProducer<'a>> {
        self.0[id.0].take()
    }
}

impl<const N: usize> NoteDrain<'_, N> {
    /// Every event, each with the source it came from.
    pub fn drain(&mut self, mut f: impl FnMut(SourceId<N>, InEvent)) {
        for (i, q) in self.0.queues.iter().enumerate() {
            while let Some(ev) = q.pop() {
                f(SourceId(i), ev);
            }
        }
    }
}

impl<const N: usize> Default for NoteSources<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> NoteSources<N> {
    pub const fn new() -> Self {
        const {
            assert!(N > 0);
            assert!(N <= MAX_NOTE_SOURCES);
        }
        Self {
            queues: [const { NoteQueue::new() }; N],
            split: AtomicBool::new(false),
        }
    }

    /// The producers, taken by `SourceId`, and the drain; once. The
    /// queues are private, so this is their only split.
    pub fn split(&self) -> Option<(NoteProducers<'_, N>, NoteDrain<'_, N>)> {
        if self.split.swap(true, Ordering::AcqRel) {
            return None;
        }
        Some((
            NoteProducers(core::array::from_fn(|i| {
                Some(NoteProducer(&self.queues[i]))
            })),
            NoteDrain(self),
        ))
    }

    pub fn drops(&self) -> [u32; N] {
        core::array::from_fn(|i| self.queues[i].dropped())
    }
}
