//! MIDI in, split at the source (MIDI map spec § 1): notes, bend, the
//! pedal, channel mode and real-time go to the note queue; every other
//! controller to the control ring, which the UI drains. A turning knob never
//! fills the note queue, and only the UI edits the Sound.
//!
//! `DinPipe` runs both parser layers in bank 2, behind `MidiDoor`: a shell
//! hands its bytes to `feed` (one door call per ISR entry, spec § 7.2).
//! USB's shell feeds its own slot the same way, and `lost` on detach.
//! Closed, MIDI in is deaf, notes included (spec § 13): no bank-1 parser.

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU32, Ordering};

use chimera_midi::ctl::{Controllers, Ctl, DataValue, Dir, FreeCc, Heard, Sel};
use chimera_midi::sense::Sensing;
use chimera_midi::stream::{Event, FaultCounts, Parsed, Parser, RealTime as Rt};
use chimera_midi::sysex;
use chimera_midi::value::{Channel, U7, U14};

use crate::MidiChannel;
use crate::midi_map::codec::{self, Bounds, MapRecord};
use crate::midi_map::{KeptMap, MidiMap};
use crate::note_queue::{
    ChannelMode, ChannelModeEvent, InEvent, NoteKind, NoteProducer, RealTime, SustainEvent,
};
use crate::storage::{RecordBuf, RecordTag, RecordWriter};
use chimera_hal::store::StoreError;

/// A controller number, 0..=127.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cc(u8);

/// Bank select (0, 32), RPN/NRPN (6, 38, 96–101), sustain (64) and
/// channel mode (120–127): never mapped (spec § 4).
const RESERVED: [u8; 19] = [
    0, 6, 32, 38, 64, 96, 97, 98, 99, 100, 101, 120, 121, 122, 123, 124, 125, 126, 127,
];

/// `RESERVED` as bits, so a test is one shift.
const RESERVED_MASK: u128 = {
    let (mut m, mut i) = (0, 0);
    while i < RESERVED.len() {
        m |= 1u128 << RESERVED[i];
        i += 1;
    }
    m
};

impl Cc {
    pub const fn new(n: u8) -> Option<Self> {
        if n <= 127 { Some(Self(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn is_reserved(self) -> bool {
        RESERVED_MASK >> self.0 & 1 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CcEvent {
    pub channel: MidiChannel,
    pub cc: Cc,
    /// 0..=127.
    pub value: u8,
}

/// What `UiState::hear_ccs` takes: a control event off the ring, or a
/// test's plain CC, which may name a number no binding can take (learn
/// says so).
pub trait HeardCc: Copy {
    fn heard(self) -> HeardEv;
}

/// The port a control message came in on, and an identity reply goes
/// out of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    Din,
    Usb,
}

impl Port {
    pub const ALL: [Port; 2] = [Port::Din, Port::Usb];
    pub const COUNT: usize = Port::ALL.len();
}

/// One message for the UI, either way it came.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeardEv {
    Ctrl(Port, CtrlEvent),
    Cc(CcEvent),
}

impl HeardCc for CcEvent {
    fn heard(self) -> HeardEv {
        HeardEv::Cc(self)
    }
}

/// DIN's, untagged.
impl HeardCc for CtrlEvent {
    fn heard(self) -> HeardEv {
        HeardEv::Ctrl(Port::Din, self)
    }
}

impl HeardCc for (Port, CtrlEvent) {
    fn heard(self) -> HeardEv {
        HeardEv::Ctrl(self.0, self.1)
    }
}

/// What the UI hears from a source beyond the note path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrlEvent {
    Ctl(Channel, Ctl),
    /// A Device Inquiry; `dev` is the device byte it named (7F or ours).
    Identity {
        dev: U7,
    },
    /// Active Sensing armed, then silent past the timeout.
    SensingLost,
}

const T_CC: u32 = 0;
const T_COARSE: u32 = 1;
const T_FINE: u32 = 2;
const T_STEP: u32 = 3;
const T_PROGRAM: u32 = 4;
const T_PRESSURE: u32 = 5;
const T_IDENTITY: u32 = 6;
const T_LOST: u32 = 7;

fn sel_bits(s: Sel) -> u32 {
    match s {
        Sel::Rpn(v) => u32::from(v.get()) << 1,
        Sel::Nrpn(v) => u32::from(v.get()) << 1 | 1,
    }
}

fn sel_of(bits: u32) -> Option<Sel> {
    let v = U14::new((bits >> 1 & 0x3FFF) as u16)?;
    Some(if bits & 1 != 0 {
        Sel::Nrpn(v)
    } else {
        Sel::Rpn(v)
    })
}

impl CtrlEvent {
    /// Two words: `[tag << 4 | channel, payload]`.
    pub fn pack(self) -> [u32; 2] {
        let (tag, ch, pay) = match self {
            CtrlEvent::Ctl(c, ctl) => {
                let (tag, pay) = match ctl {
                    Ctl::Cc { cc, value } => {
                        (T_CC, u32::from(cc.get()) | u32::from(value.get()) << 8)
                    }
                    Ctl::Data {
                        sel,
                        value: DataValue::Coarse(v),
                    } => (T_COARSE, sel_bits(sel) | u32::from(v.get()) << 16),
                    Ctl::Data {
                        sel,
                        value: DataValue::Fine(v),
                    } => (T_FINE, sel_bits(sel) | u32::from(v.get()) << 16),
                    Ctl::Step { sel, dir } => {
                        (T_STEP, sel_bits(sel) | u32::from(dir == Dir::Down) << 16)
                    }
                    Ctl::Program { bank, program } => (
                        T_PROGRAM,
                        u32::from(program.get())
                            | bank.map_or(0, |b| 1 << 7 | u32::from(b.get()) << 8),
                    ),
                    Ctl::Pressure(v) => (T_PRESSURE, u32::from(v.get())),
                };
                (tag, c.get(), pay)
            }
            CtrlEvent::Identity { dev } => (T_IDENTITY, 0, u32::from(dev.get())),
            CtrlEvent::SensingLost => (T_LOST, 0, 0),
        };
        [tag << 4 | u32::from(ch), pay]
    }

    pub fn unpack([head, p]: [u32; 2]) -> Option<Self> {
        let c = Channel::new((head & 15) as u8);
        let u7 = |v: u32| U7::new((v & 0xFF) as u8);
        let ctl = |ctl| Some(CtrlEvent::Ctl(c?, ctl));
        match head >> 4 {
            T_CC => ctl(Ctl::Cc {
                cc: FreeCc::new((p & 0xFF) as u8)?,
                value: u7(p >> 8)?,
            }),
            T_COARSE => ctl(Ctl::Data {
                sel: sel_of(p)?,
                value: DataValue::Coarse(u7(p >> 16)?),
            }),
            T_FINE => ctl(Ctl::Data {
                sel: sel_of(p)?,
                value: DataValue::Fine(U14::new((p >> 16) as u16)?),
            }),
            T_STEP => ctl(Ctl::Step {
                sel: sel_of(p)?,
                dir: if p >> 16 & 1 != 0 { Dir::Down } else { Dir::Up },
            }),
            T_PROGRAM => ctl(Ctl::Program {
                bank: if p & 0x80 != 0 {
                    Some(U14::new((p >> 8) as u16)?)
                } else {
                    None
                },
                program: u7(p & 0x7F)?,
            }),
            T_PRESSURE => ctl(Ctl::Pressure(u7(p)?)),
            T_IDENTITY => Some(CtrlEvent::Identity { dev: u7(p)? }),
            T_LOST => Some(CtrlEvent::SensingLost),
            _ => None,
        }
    }
}

/// DIN's ring: 3 bytes a millisecond fill few slots a UI lap.
pub const CTRL_RING_LEN: usize = 64;
/// USB's: a host's burst lands whole in a few milliseconds; 3,780 B (420
/// note pairs, each with a CC) dropped 160 CCs from 64 slots on the unit.
pub const CTRL_USB_LEN: usize = 512;

/// One producer (an ISR or a MIDI thread), one consumer (the UI); head and
/// tail free-running, the slot `counter % N` (a power of two), the same
/// `Acquire`/`Release` pairing as `note_queue`'s. Two words a slot, written
/// whole before `tail` publishes them, so a full ring drops whole events and a reader never sees
/// half of one (an NRPN's address and value travel together).
pub struct CtrlRing<const N: usize = CTRL_RING_LEN> {
    slots: [[AtomicU32; 2]; N],
    /// Next event to pop (the consumer's only).
    head: AtomicU32,
    /// Next slot to push (the producer's only).
    tail: AtomicU32,
    dropped: AtomicU32,
}

impl<const N: usize> Default for CtrlRing<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> CtrlRing<N> {
    pub const fn new() -> Self {
        const { assert!(N.is_power_of_two()) };
        Self {
            slots: [const { [AtomicU32::new(0), AtomicU32::new(0)] }; N],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dropped: AtomicU32::new(0),
        }
    }

    /// Producer side only. `false`: full, the whole event dropped. `head`
    /// `Acquire` pairs with `pop`'s `Release`; the `Release` on `tail`
    /// publishes the slot.
    pub fn push(&self, e: CtrlEvent) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) as usize >= N {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let slot = &self.slots[tail as usize % N];
        let [a, b] = e.pack();
        slot[0].store(a, Ordering::Relaxed);
        slot[1].store(b, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer side only. `tail` `Acquire` pairs with `push`'s `Release`;
    /// the `Release` on `head` frees the slot.
    pub fn pop(&self) -> Option<CtrlEvent> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let slot = &self.slots[head as usize % N];
        let bits = [
            slot[0].load(Ordering::Relaxed),
            slot[1].load(Ordering::Relaxed),
        ];
        self.head.store(head.wrapping_add(1), Ordering::Release);
        CtrlEvent::unpack(bits)
    }

    /// Consumer side: every event waiting, oldest first.
    pub fn drain(&self) -> impl Iterator<Item = CtrlEvent> + '_ {
        core::iter::from_fn(|| self.pop())
    }

    /// Consumer side: every event waiting dropped unread, as a build with
    /// no MIDI map drains it (`tail` `Acquire` as `pop`'s).
    #[inline]
    pub fn clear(&self) {
        self.head
            .store(self.tail.load(Ordering::Acquire), Ordering::Release);
    }

    /// Events dropped because the ring was full.
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// MIDI in's control rings, one a `Port`: DIN's at DIN's rate, USB's at
/// `U` slots, a host's burst. Each port's poll pushes its own; the UI is
/// the one consumer.
pub struct CtrlPorts<const U: usize = CTRL_USB_LEN> {
    pub din: CtrlRing,
    pub usb: CtrlRing<U>,
}

impl<const U: usize> Default for CtrlPorts<U> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const U: usize> CtrlPorts<U> {
    pub const fn new() -> Self {
        Self {
            din: CtrlRing::new(),
            usb: CtrlRing::new(),
        }
    }

    /// Consumer side: every event waiting, DIN's then USB's, each tagged
    /// with its port. A `from_fn`, not a `Chain`, whose overrides a `dyn`
    /// caller would link.
    #[inline(always)]
    pub fn drain(&self) -> impl Iterator<Item = (Port, CtrlEvent)> + '_ {
        let mut din = true;
        core::iter::from_fn(move || {
            if din {
                match self.din.pop() {
                    Some(e) => return Some((Port::Din, e)),
                    None => din = false,
                }
            }
            self.usb.pop().map(|e| (Port::Usb, e))
        })
    }

    /// Consumer side: both dropped unread.
    #[inline(always)]
    pub fn clear(&self) {
        self.din.clear();
        self.usb.clear();
    }
}

/// Where a pipe's events go; `false`: refused (full), the event is lost.
pub trait Sinks {
    fn note(&mut self, e: InEvent) -> bool;
    fn ctrl(&mut self, e: CtrlEvent) -> bool;
}

/// What a pipe lost or ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipeStats {
    pub faults: FaultCounts,
    /// Program Changes parsed and dropped (Phase 1 acts on none).
    pub programs_dropped: u16,
    /// Events the note queue refused.
    pub refused: u16,
    /// Events the control ring refused, apart: a ring nobody drains fills
    /// without a note lost.
    pub ctrl_refused: u16,
}

impl PipeStats {
    /// Every fault layer 1 counted.
    pub fn fault_total(&self) -> u32 {
        let f = self.faults;
        [
            f.orphan,
            f.interrupted,
            f.sysex_cut,
            f.sysex_overflow,
            f.undefined,
        ]
        .iter()
        .map(|&n| u32::from(n))
        .sum()
    }
}

/// One source's byte stream, both parser layers: the note path to the note
/// queue, every other controller to the control ring.
pub struct DinPipe {
    parser: Parser<32>,
    ctl: Controllers,
    sense: Sensing,
    stats: PipeStats,
    /// A bit a channel whose NotesOff `lost` owes the queue.
    owed: u16,
    pedals: Pedals,
}

/// The pedals a source holds on the note queue, and the pedal-ups `lost`
/// owes it.
struct Pedals {
    /// A bit a channel whose pedal the queue took down: cleared by its
    /// pedal-up or Reset All Controllers.
    down: u16,
    /// A bit a channel whose pedal-up the queue has yet to take, after
    /// every owed NotesOff.
    up: u16,
}

impl Pedals {
    fn heard(&mut self, e: InEvent) {
        match e {
            InEvent::Sustain(p) => {
                let bit = 1 << (p.channel.get() & 15);
                if p.down {
                    self.down |= bit;
                } else {
                    self.down &= !bit;
                }
            }
            InEvent::Mode(m) if m.mode == ChannelMode::ResetControllers => {
                self.down &= !(1 << (m.channel.get() & 15));
            }
            _ => {}
        }
    }
}

/// The keys USB's host holds on the note queue. USB's alone (`UsbSlot`):
/// only its poll reads them, so DIN's pipe carries none.
struct Held {
    /// A bit a key, a word a channel: set by a note-on the queue took,
    /// cleared by its note-off, NotesOff or SoundOff. A count a channel
    /// would be fooled by a stray note-off (256 B against 16).
    keys: [u128; 16],
}

impl Held {
    /// A channel's word; masked, so no bounds check's panic data lands in
    /// bank 1's anonymous constants.
    fn keys(&mut self, c: MidiChannel) -> &mut u128 {
        &mut self.keys[usize::from(c.get()) & 15]
    }

    fn heard(&mut self, e: InEvent) {
        match e {
            InEvent::Note(n) => {
                let k = self.keys(n.channel);
                let bit = 1u128 << n.note.get();
                match n.kind {
                    NoteKind::On(_) => *k |= bit,
                    NoteKind::Off => *k &= !bit,
                }
            }
            // Pedals only (`Pedals::heard`): the keys stay down.
            InEvent::Mode(m) if m.mode == ChannelMode::ResetControllers => {}
            InEvent::Mode(m) => *self.keys(m.channel) = 0,
            _ => {}
        }
    }
}

impl Default for DinPipe {
    fn default() -> Self {
        Self::new()
    }
}

impl DinPipe {
    pub const fn new() -> Self {
        Self {
            parser: Parser::new(),
            ctl: Controllers::new(),
            sense: Sensing::Off,
            stats: PipeStats {
                faults: FaultCounts {
                    orphan: 0,
                    interrupted: 0,
                    sysex_cut: 0,
                    sysex_overflow: 0,
                    undefined: 0,
                },
                programs_dropped: 0,
                refused: 0,
                ctrl_refused: 0,
            },
            owed: 0,
            pedals: Pedals { down: 0, up: 0 },
        }
    }

    /// Empty, it queues what `lost` still owes: the poll's retry.
    pub fn feed<S: Sinks + ?Sized>(&mut self, bytes: &[u8], out: &mut S) {
        self.run(bytes, out, None);
    }

    /// `feed`, keeping `held` (USB's) by what the queue takes.
    fn run<S: Sinks + ?Sized>(&mut self, bytes: &[u8], out: &mut S, mut held: Option<&mut Held>) {
        Counted {
            out: &mut *out,
            stats: &mut self.stats,
            owed: &mut self.owed,
            pedals: &mut self.pedals,
            held: held.as_deref_mut(),
        }
        .settle();
        for &b in bytes {
            let Some(parsed) = self.parser.push(b) else {
                continue;
            };
            let mut sink = Counted {
                out: &mut *out,
                stats: &mut self.stats,
                owed: &mut self.owed,
                pedals: &mut self.pedals,
                held: held.as_deref_mut(),
            };
            match parsed {
                Parsed::RealTime(rt) => match rt {
                    Rt::Clock => sink.note(InEvent::Rt(RealTime::Clock)),
                    Rt::Start => sink.note(InEvent::Rt(RealTime::Start)),
                    Rt::Continue => sink.note(InEvent::Rt(RealTime::Continue)),
                    Rt::Stop => sink.note(InEvent::Rt(RealTime::Stop)),
                    Rt::ActiveSensing => self.sense.heard(rt),
                    Rt::Reset => {
                        self.ctl = Controllers::new();
                        for c in 0..16 {
                            let channel = Channel::clamped(c);
                            sink.mode(channel, ChannelMode::SoundOff);
                            sink.mode(channel, ChannelMode::ResetControllers);
                        }
                    }
                },
                Parsed::Event(Event::Voice(c, v)) => match InEvent::from_event(c, &v) {
                    Some(e) => sink.note(e),
                    None => match self.ctl.hear(c, v) {
                        Heard::Ctl(Ctl::Program { .. }) => {
                            sink.stats.programs_dropped =
                                sink.stats.programs_dropped.saturating_add(1);
                        }
                        Heard::Ctl(ctl) => sink.ctrl(CtrlEvent::Ctl(c, ctl)),
                        Heard::Note | Heard::Consumed => {}
                    },
                },
                Parsed::Event(Event::Mode(c, m)) => {
                    if let Some(e) = InEvent::from_mode(c, m) {
                        sink.note(e);
                    }
                }
                Parsed::Event(Event::SysEx(s)) => {
                    if sysex::request(s, U7::ZERO).is_some()
                        && let Some(dev) = s.bytes().get(1).and_then(|&d| U7::new(d))
                    {
                        sink.ctrl(CtrlEvent::Identity { dev });
                    }
                }
                Parsed::Event(Event::Common(_) | Event::Fault(_)) => {}
            }
        }
        self.stats.faults = self.parser.faults();
    }

    /// The receiver timeout fired: if Active Sensing was armed, every Part
    /// lets go and the UI hears it.
    pub fn idle<S: Sinks + ?Sized>(&mut self, out: &mut S) {
        if self.sense.timed_out() {
            self.lost(out, None);
        }
    }

    /// The source went away (DIN's sensing timed out, USB's bus reset or
    /// suspend): every Part lets go and the UI hears it (USB's only when it
    /// held a key or pedal: a re-enumeration says nothing); then every pedal
    /// the source has down is lifted. NotesOff and pedal-ups a full
    /// queue refuses are owed, each channel's once, and go ahead of
    /// anything fed after (`feed`); a refusal counts in `refused` (#865),
    /// as no byte may follow to count it.
    fn lost<S: Sinks + ?Sized>(&mut self, out: &mut S, mut held: Option<&mut Held>) {
        let told = self.pedals.down != 0
            || held
                .as_deref()
                .is_none_or(|h| h.keys.iter().any(|&k| k != 0));
        self.owed = u16::MAX;
        self.pedals.up |= self.pedals.down;
        self.pedals.down = 0;
        if let Some(h) = held.as_deref_mut() {
            h.keys = [0; 16];
        }
        let mut sink = Counted {
            out,
            stats: &mut self.stats,
            owed: &mut self.owed,
            pedals: &mut self.pedals,
            held,
        };
        if !sink.settle() {
            sink.stats.refused = sink.stats.refused.saturating_add(1);
        }
        if told {
            sink.ctrl(CtrlEvent::SensingLost);
        }
    }

    /// A field read, so bank 1's ISR may call it: armed, it sets the
    /// receiver timeout's interrupt.
    #[inline(always)]
    pub fn sensing_armed(&self) -> bool {
        self.sense.armed()
    }

    /// As of the last `feed`. A field read, so bank 1 may call it closed.
    #[inline(always)]
    pub fn stats(&self) -> PipeStats {
        self.stats
    }
}

/// A shell's sinks: its note queue's producer and its control ring. The
/// door's pipe pushes to them directly, so the pushes run in bank 2 with it
/// (no vtable, nothing of them left in bank 1's stub).
pub struct DinSinks<'a, 'q, const N: usize = CTRL_RING_LEN> {
    pub notes: &'a mut NoteProducer<'q>,
    pub ctrl: &'a CtrlRing<N>,
}

impl<const N: usize> Sinks for DinSinks<'_, '_, N> {
    #[inline]
    fn note(&mut self, e: InEvent) -> bool {
        self.notes.push(e)
    }

    #[inline]
    fn ctrl(&mut self, e: CtrlEvent) -> bool {
        self.ctrl.push(e)
    }
}

/// A shell's pipe, built by the door on its first bytes: a `static` of it is
/// all zero (`.bss`), so bank 1 carries no image of `DinPipe::new` (~290 B)
/// and never runs its stores.
pub struct PipeSlot {
    ready: bool,
    pipe: MaybeUninit<DinPipe>,
}

impl Default for PipeSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl PipeSlot {
    pub const fn new() -> Self {
        Self {
            ready: false,
            pipe: MaybeUninit::uninit(),
        }
    }

    /// The pipe, once bytes have reached it.
    #[inline(always)]
    pub fn pipe(&self) -> Option<&DinPipe> {
        // SAFETY: `ready` is set only by `built`, after it writes `pipe`.
        self.ready.then(|| unsafe { self.pipe.assume_init_ref() })
    }

    /// As of the last `feed`; nothing counted before the first.
    #[inline(always)]
    pub fn stats(&self) -> PipeStats {
        self.pipe().map_or_else(PipeStats::default, DinPipe::stats)
    }

    /// Active Sensing armed as of the last `feed` or `idle`; nothing
    /// before the first. A field read, as `stats`.
    #[inline(always)]
    pub fn sensing_armed(&self) -> bool {
        self.pipe().is_some_and(DinPipe::sensing_armed)
    }

    /// The pipe, built first if it is not yet. Inlined into the door's
    /// entries, so `DinPipe::new` runs in bank 2.
    #[inline(always)]
    fn built(&mut self) -> &mut DinPipe {
        if !self.ready {
            self.pipe.write(DinPipe::new());
            self.ready = true;
        }
        // SAFETY: written above or by an earlier call, which set `ready`.
        unsafe { self.pipe.assume_init_mut() }
    }
}

/// USB's pipe, and the keys and pedals its host holds (`Held`): `lost`
/// lets go of them. Zero is empty, as `PipeSlot`.
pub struct UsbSlot {
    slot: PipeSlot,
    held: Held,
}

impl Default for UsbSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbSlot {
    pub const fn new() -> Self {
        Self {
            slot: PipeSlot::new(),
            held: Held { keys: [0; 16] },
        }
    }

    /// A key or pedal the queue took down that nothing has let go since.
    #[inline(always)]
    pub fn holds(&self) -> bool {
        self.slot.pipe().is_some_and(|p| p.pedals.down != 0)
            || self.held.keys.iter().any(|&k| k != 0)
    }

    /// NotesOff or pedal-ups `lost` still owes the queue. Field reads.
    #[inline(always)]
    pub fn owes(&self) -> bool {
        self.slot
            .pipe()
            .is_some_and(|p| p.owed != 0 || p.pedals.up != 0)
    }

    /// As of the last `feed`; nothing counted before the first.
    #[inline(always)]
    pub fn stats(&self) -> PipeStats {
        self.slot.stats()
    }

    /// `DinPipe::feed`, keeping what the host holds. Inlined into
    /// `DinPipe`'s entries, so it runs in bank 2 with them.
    #[inline(always)]
    pub fn feed<S: Sinks + ?Sized>(&mut self, bytes: &[u8], out: &mut S) {
        self.slot.built().run(bytes, out, Some(&mut self.held));
    }

    /// The host went away (`DinPipe::lost`): every key and pedal let go.
    #[inline(always)]
    pub fn lost<S: Sinks + ?Sized>(&mut self, out: &mut S) {
        self.slot.built().lost(out, Some(&mut self.held));
    }
}

/// How USB left Configured (`chimera_midi::usb::Edge`).
pub use chimera_midi::usb::Left;

/// Whether leaving Configured lets every Part go (`lost`): a reset always;
/// a suspend only while USB holds a key or a pedal (`UsbSlot::holds`), so a host's
/// autosuspend never cuts notes held from DIN alone.
#[inline(always)]
pub fn lets_go(left: Left, holds: bool) -> bool {
    left == Left::Reset || holds
}

/// `DinPipe::feed` as the door carries it.
pub type MidiFeed = fn(&mut PipeSlot, &[u8], &mut DinSinks<'_, '_>);
/// `DinPipe::idle` as the door carries it.
pub type MidiIdle = fn(&mut PipeSlot, &mut DinSinks<'_, '_>);

/// `midi_map::codec::decode` as the door carries it.
pub type MapDecode = fn(MapRecord, &[u8], Option<&mut MidiMap>) -> Result<u16, Bounds>;
/// `midi_map::codec::encode` as the door carries it.
pub type MapEncode = fn(&MidiMap, &mut RecordBuf);

/// MIDI in's entries in bank 2's door: the pipe's two layers and the
/// map's codec.
#[repr(C)]
pub struct MidiDoor {
    pub feed: MidiFeed,
    pub idle: MidiIdle,
    pub decode_map: MapDecode,
    pub encode_map: MapEncode,
}

impl MidiDoor {
    pub const DOOR: MidiDoor = MidiDoor {
        feed: DinPipe::door_feed,
        idle: DinPipe::door_idle,
        decode_map: codec::decode,
        encode_map: codec::encode,
    };
}

/// The door's entries: `feed` and `idle` on a shell's slot and sinks,
/// `DinPipe`'s so `bank2.x`'s glob places them.
impl DinPipe {
    fn door_feed(slot: &mut PipeSlot, bytes: &[u8], out: &mut DinSinks<'_, '_>) {
        slot.built().feed(bytes, out);
    }

    fn door_idle(slot: &mut PipeSlot, out: &mut DinSinks<'_, '_>) {
        slot.built().idle(out);
    }

    /// `door_feed` on USB's ring, for a caller already in bank 2 (USB's
    /// poll): no door call. Not generic, so it builds at this crate's
    /// level as the door's: built at the board's, the machine outliner put
    /// a call into bank 2 in a bank-1 outline.
    #[inline(never)]
    pub fn usb_feed(slot: &mut UsbSlot, bytes: &[u8], out: &mut DinSinks<'_, '_, CTRL_USB_LEN>) {
        slot.feed(bytes, out);
    }

    /// `UsbSlot::lost`, as `usb_feed`.
    #[inline(never)]
    pub fn usb_lost(slot: &mut UsbSlot, out: &mut DinSinks<'_, '_, CTRL_USB_LEN>) {
        slot.lost(out);
    }

    /// `usb_feed` on DIN's ring size: builds without the map.
    #[inline(never)]
    pub fn usb_feed_short(slot: &mut UsbSlot, bytes: &[u8], out: &mut DinSinks<'_, '_>) {
        slot.feed(bytes, out);
    }

    /// `usb_lost` on DIN's ring size, as `usb_feed_short`.
    #[inline(never)]
    pub fn usb_lost_short(slot: &mut UsbSlot, out: &mut DinSinks<'_, '_>) {
        slot.lost(out);
    }
}

/// `bytes` through `slot`'s pipe into `out`, through bank 2's door with
/// `bank2`: closed, they are dropped, notes too (spec § 13).
#[inline(always)]
pub fn feed(slot: &mut PipeSlot, bytes: &[u8], out: &mut DinSinks<'_, '_>) {
    #[cfg(feature = "bank2")]
    crate::bank2::DOOR_CELL.with(|d| (d.midi.feed)(slot, bytes, out));
    #[cfg(not(feature = "bank2"))]
    DinPipe::door_feed(slot, bytes, out);
}

/// The receiver timed out (`DinPipe::idle`), as `feed`: closed, nothing.
#[inline(always)]
pub fn idle(slot: &mut PipeSlot, out: &mut DinSinks<'_, '_>) {
    #[cfg(feature = "bank2")]
    crate::bank2::DOOR_CELL.with(|d| (d.midi.idle)(slot, out));
    #[cfg(not(feature = "bank2"))]
    DinPipe::door_idle(slot, out);
}

/// A map record checked and, with `m`, decoded (`codec::decode`) through
/// bank 2's door with `bank2`: closed, it is not read, `m` keeps its
/// empty map and nothing is skipped (spec § 6.4); over a full record is
/// `Bounds`, as open, so the Part's `KeptMap` holds any it passes (#815).
#[inline(always)]
pub fn decode_map(r: MapRecord, p: &[u8], m: Option<&mut MidiMap>) -> Result<u16, Bounds> {
    #[cfg(feature = "bank2")]
    return crate::bank2::DOOR_CELL
        .with(|d| (d.midi.decode_map)(r, p, m))
        .unwrap_or(if p.len() > crate::midi_map::KEPT_LEN {
            Err(Bounds)
        } else {
            Ok(0)
        });
    #[cfg(not(feature = "bank2"))]
    codec::decode(r, p, m)
}

/// Record `tag`'s payload `p` onto `k` if it is a map record bank 2's
/// closed door leaves unread (#815).
#[inline(never)]
pub fn keep_map(k: &mut KeptMap, tag: RecordTag, p: &[u8]) {
    let r = match tag {
        RecordTag::MidiMap => MapRecord::Legacy,
        RecordTag::Bindings => MapRecord::Bindings,
        _ => return,
    };
    #[cfg(feature = "bank2")]
    if !crate::bank2::DOOR_CELL.is_open() {
        // Over a full record: `decode_map` refuses the file.
        let _ = k.keep(r, p);
    }
    #[cfg(not(feature = "bank2"))]
    let _ = (k, r, p);
}

/// `k`'s record, under its tag, into `w`.
pub fn put_kept(k: &KeptMap, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    match k.get() {
        Some((MapRecord::Legacy, p)) => w.put(RecordTag::MidiMap, p),
        Some((MapRecord::Bindings, p)) => w.put(RecordTag::Bindings, p),
        None => Ok(()),
    }
}

/// `m` as a `Bindings` payload into `out` (`codec::encode`), as
/// `decode_map`: closed, nothing.
#[inline(always)]
pub fn encode_map(m: &MidiMap, out: &mut RecordBuf) {
    #[cfg(feature = "bank2")]
    crate::bank2::DOOR_CELL.with(|d| (d.midi.encode_map)(m, out));
    #[cfg(not(feature = "bank2"))]
    codec::encode(m, out);
}

/// Whether `decode_map` and `encode_map` reach the codec: bank 2's door
/// is open, or this build has no bank 2.
#[inline(always)]
pub fn map_codec_open() -> bool {
    #[cfg(feature = "bank2")]
    return crate::bank2::DOOR_CELL.is_open();
    #[cfg(not(feature = "bank2"))]
    true
}

/// A sink that counts refusals, each sink's apart, settles what `lost`
/// owes, and keeps USB's `Held`.
struct Counted<'a, S: Sinks + ?Sized> {
    out: &'a mut S,
    stats: &'a mut PipeStats,
    owed: &'a mut u16,
    pedals: &'a mut Pedals,
    held: Option<&'a mut Held>,
}

impl<S: Sinks + ?Sized> Counted<'_, S> {
    /// Refused while NotesOff are owed, so none overtakes them.
    fn note(&mut self, e: InEvent) {
        if self.settle() && self.out.note(e) {
            self.pedals.heard(e);
            if let Some(h) = self.held.as_deref_mut() {
                h.heard(e);
            }
        } else {
            self.stats.refused = self.stats.refused.saturating_add(1);
        }
    }

    /// The owed NotesOff, then the owed pedal-ups (the voices a NotesOff
    /// leaves sustained go with the pedal), lowest channel first, until
    /// one is refused; `true`: none left.
    fn settle(&mut self) -> bool {
        for (bits, up) in [(&mut *self.owed, false), (&mut self.pedals.up, true)] {
            while *bits != 0 {
                let channel = Channel::clamped(bits.trailing_zeros() as u8);
                let e = if up {
                    InEvent::Sustain(SustainEvent {
                        channel,
                        down: false,
                    })
                } else {
                    InEvent::Mode(ChannelModeEvent {
                        channel,
                        mode: ChannelMode::NotesOff,
                    })
                };
                if !self.out.note(e) {
                    return false;
                }
                *bits &= *bits - 1;
            }
        }
        true
    }

    fn ctrl(&mut self, e: CtrlEvent) {
        if !self.out.ctrl(e) {
            self.stats.ctrl_refused = self.stats.ctrl_refused.saturating_add(1);
        }
    }

    fn mode(&mut self, channel: Channel, mode: ChannelMode) {
        self.note(InEvent::Mode(ChannelModeEvent { channel, mode }));
    }
}
