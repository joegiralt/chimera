//! MIDI in, split at the source (MIDI map spec § 1): notes, bend, the
//! pedal, channel mode and real-time go to the note queue; every other
//! controller to the control ring, which the UI drains. A turning knob never
//! fills the note queue, and only the UI edits the Sound.
//!
//! `DinPipe` runs both parser layers in bank 2, behind `MidiDoor`: a shell
//! hands its bytes to `feed` (one door call per ISR entry, spec § 7.2).
//! Closed, MIDI in is deaf, notes included (spec § 13): no bank-1 parser.

use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU32, Ordering};

use chimera_midi::ctl::{Controllers, Ctl, DataValue, Dir, FreeCc, Heard, Sel};
use chimera_midi::sense::Sensing;
use chimera_midi::stream::{Event, FaultCounts, Parsed, Parser, RealTime as Rt};
use chimera_midi::sysex;
use chimera_midi::value::{Channel, U7, U14};

use crate::MidiChannel;
use crate::midi_map::MidiMap;
use crate::midi_map::codec::{self, Bounds, MapRecord};
use crate::note_queue::{ChannelMode, ChannelModeEvent, InEvent, NoteProducer, RealTime};
use crate::storage::RecordBuf;

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

/// What `UiState::hear_ccs` takes: a plain CC (a test's) or a control
/// event off the ring, of which it hears `Ctl::Cc` only for now.
pub trait HeardCc: Copy {
    fn cc_event(self) -> Option<CcEvent>;
}

impl HeardCc for CcEvent {
    fn cc_event(self) -> Option<CcEvent> {
        Some(self)
    }
}

impl HeardCc for CtrlEvent {
    fn cc_event(self) -> Option<CcEvent> {
        match self {
            CtrlEvent::Ctl(channel, Ctl::Cc { cc, value }) => Some(CcEvent {
                channel,
                cc: Cc(cc.get()),
                value: value.get(),
            }),
            _ => None,
        }
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

pub const CTRL_RING_LEN: usize = 64;
const _: () = assert!(CTRL_RING_LEN.is_power_of_two());

/// One producer (an ISR or a MIDI thread), one consumer (the UI); head and
/// tail free-running, the slot `counter % CTRL_RING_LEN`, the same
/// `Acquire`/`Release` pairing as `note_queue`'s. Two words a slot, written
/// whole before `tail` publishes them, so a full ring drops whole events and a reader never sees
/// half of one (an NRPN's address and value travel together).
pub struct CtrlRing {
    slots: [[AtomicU32; 2]; CTRL_RING_LEN],
    /// Next event to pop (the consumer's only).
    head: AtomicU32,
    /// Next slot to push (the producer's only).
    tail: AtomicU32,
    dropped: AtomicU32,
}

impl Default for CtrlRing {
    fn default() -> Self {
        Self::new()
    }
}

impl CtrlRing {
    pub const fn new() -> Self {
        Self {
            slots: [const { [AtomicU32::new(0), AtomicU32::new(0)] }; CTRL_RING_LEN],
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
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) as usize >= CTRL_RING_LEN {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let slot = &self.slots[tail as usize % CTRL_RING_LEN];
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
        let slot = &self.slots[head as usize % CTRL_RING_LEN];
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

    /// Events dropped because the ring was full.
    pub fn dropped(&self) -> u32 {
        self.dropped.load(Ordering::Relaxed)
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
    /// Events a sink refused.
    pub refused: u16,
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
            },
        }
    }

    pub fn feed<S: Sinks + ?Sized>(&mut self, bytes: &[u8], out: &mut S) {
        for &b in bytes {
            let Some(parsed) = self.parser.push(b) else {
                continue;
            };
            let mut sink = Counted {
                out: &mut *out,
                refused: &mut self.stats.refused,
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
                            self.stats.programs_dropped =
                                self.stats.programs_dropped.saturating_add(1);
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
            let mut sink = Counted {
                out,
                refused: &mut self.stats.refused,
            };
            for c in 0..16 {
                sink.mode(Channel::clamped(c), ChannelMode::NotesOff);
            }
            sink.ctrl(CtrlEvent::SensingLost);
        }
    }

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
pub struct DinSinks<'a, 'q> {
    pub notes: &'a mut NoteProducer<'q>,
    pub ctrl: &'a CtrlRing,
}

impl Sinks for DinSinks<'_, '_> {
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
/// empty map and nothing is skipped (spec § 6.4).
#[inline(always)]
pub fn decode_map(r: MapRecord, p: &[u8], m: Option<&mut MidiMap>) -> Result<u16, Bounds> {
    #[cfg(feature = "bank2")]
    return crate::bank2::DOOR_CELL
        .with(|d| (d.midi.decode_map)(r, p, m))
        .unwrap_or(Ok(0));
    #[cfg(not(feature = "bank2"))]
    codec::decode(r, p, m)
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

/// A sink that counts refusals.
struct Counted<'a, S: Sinks + ?Sized> {
    out: &'a mut S,
    refused: &'a mut u16,
}

impl<S: Sinks + ?Sized> Counted<'_, S> {
    fn note(&mut self, e: InEvent) {
        if !self.out.note(e) {
            *self.refused = self.refused.saturating_add(1);
        }
    }

    fn ctrl(&mut self, e: CtrlEvent) {
        if !self.out.ctrl(e) {
            *self.refused = self.refused.saturating_add(1);
        }
    }

    fn mode(&mut self, channel: Channel, mode: ChannelMode) {
        self.note(InEvent::Mode(ChannelModeEvent { channel, mode }));
    }
}
