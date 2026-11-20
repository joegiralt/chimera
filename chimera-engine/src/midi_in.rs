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
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering};

use chimera_midi::ctl::{Controllers, Ctl, DataValue, Dir, FreeCc, Heard, Sel};
use chimera_midi::sense::Sensing;
use chimera_midi::stream::{self, Event, FaultCounts, Parsed, Parser, RealTime as Rt, Voice};
use chimera_midi::sysex::{self, DumpWho};
use chimera_midi::usb::Cable;
use chimera_midi::value::{Channel, U7, U14};

use crate::hw::MAX_PARTS;
use crate::midi_map::codec::{self, Bounds, MapRecord};
use crate::midi_map::{KeptMap, MidiMap};
use crate::note_queue::{
    ChannelMode, ChannelModeEvent, Expr, ExprEvent, InEvent, MAX_NOTE_SOURCES, NoteKind,
    NoteProducer, NoteQueue, READY_LEN, Ready, RealTime, SustainEvent,
};
use crate::storage::{RecordBuf, RecordTag, RecordWriter};
use crate::{MidiChannel, MidiNote};
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
    /// A SysEx body waits in the port's `SYSEX_IN` (#846).
    SysEx,
    /// A note-on on MIDI CONFIG › CONTROL's SEQUENCER channel: ORBIT's KEY
    /// (ADR 0254).
    Key(MidiNote),
    /// Reset All Controllers on this channel: expression back to 127 (ADR
    /// 0263). Its own event, so learn never hears it as a CC.
    Reset(Channel),
    /// A parameter dump request (#984, ADR 0298): whose values it asks.
    Dump(DumpWho),
}

const T_CC: u32 = 0;
const T_COARSE: u32 = 1;
const T_FINE: u32 = 2;
const T_STEP: u32 = 3;
const T_PROGRAM: u32 = 4;
const T_PRESSURE: u32 = 5;
const T_IDENTITY: u32 = 6;
const T_LOST: u32 = 7;
const T_SYSEX: u32 = 8;
const T_KEY: u32 = 9;
const T_RESET: u32 = 10;
const T_DUMP: u32 = 11;
// Every tag its own (ADR 0254: KEY's 8 met SYSEX's on the merge).
const _: () = {
    let t = [
        T_CC, T_COARSE, T_FINE, T_STEP, T_PROGRAM, T_PRESSURE, T_IDENTITY, T_LOST, T_SYSEX, T_KEY,
        T_RESET, T_DUMP,
    ];
    let mut i = 0;
    while i < t.len() {
        let mut j = i + 1;
        while j < t.len() {
            assert!(t[i] != t[j], "two CtrlEvent tags alike");
            j += 1;
        }
        i += 1;
    }
};

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
            CtrlEvent::SysEx => (T_SYSEX, 0, 0),
            CtrlEvent::Key(k) => (T_KEY, 0, u32::from(k.get())),
            CtrlEvent::Reset(c) => (T_RESET, c.get(), 0),
            CtrlEvent::Dump(who) => (T_DUMP, 0, u32::from(who.wire().get())),
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
            T_SYSEX => Some(CtrlEvent::SysEx),
            T_KEY => Some(CtrlEvent::Key(MidiNote::new((p & 0x7F) as u8)?)),
            T_RESET => Some(CtrlEvent::Reset(c?)),
            T_DUMP => DumpWho::from_wire(u7(p)?).map(CtrlEvent::Dump),
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
    #[inline]
    pub fn push(&self, e: CtrlEvent) -> bool {
        self.tx().push(e)
    }

    /// The producer's side, its size a value.
    #[inline(always)]
    fn tx(&self) -> RingTx<'_> {
        RingTx {
            slots: &self.slots,
            head: &self.head,
            tail: &self.tail,
            dropped: &self.dropped,
        }
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

/// A ring's producer side with its size a value, not a type: DIN's ring
/// and USB's push through one parser (`PipeSinks`, ADR 0251). `slots`'
/// length is the ring's `N`, a power of two.
struct RingTx<'a> {
    slots: &'a [[AtomicU32; 2]],
    head: &'a AtomicU32,
    tail: &'a AtomicU32,
    dropped: &'a AtomicU32,
}

impl RingTx<'_> {
    /// `CtrlRing::push`.
    #[inline(always)]
    fn push(&self, e: CtrlEvent) -> bool {
        let n = self.slots.len();
        let tail = self.tail.load(Ordering::Relaxed);
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) as usize >= n {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let slot = &self.slots[tail as usize & (n - 1)];
        let [a, b] = e.pack();
        slot[0].store(a, Ordering::Relaxed);
        slot[1].store(b, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
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

/// Each Part's MIDI channel as the UI last saw it: a USB Part cable plays
/// on it (#840, ADR 0243). The UI stores, USB's poll reads; until the UI's
/// first pass, the defaults (Part n on channel n). Each held plus one, 0
/// the default: all zero, so `.bss`, no image in bank 1.
pub struct PartChannels([AtomicU8; MAX_PARTS]);

pub static PART_CHANNELS: PartChannels = PartChannels::new();

const _: () = assert!(MAX_PARTS + 1 == chimera_midi::usb_class::CABLES as usize);

impl PartChannels {
    pub const fn new() -> Self {
        Self([const { AtomicU8::new(0) }; MAX_PARTS])
    }

    /// Part `part`'s channel is `ch`.
    #[inline(always)]
    pub fn set(&self, part: usize, ch: Channel) {
        if let Some(a) = self.0.get(part) {
            a.store(ch.get() + 1, Ordering::Relaxed);
        }
    }

    /// Cable `c`'s Part's channel: cable n is Part n's; `None` for cable 0
    /// and any past the Parts.
    #[inline(always)]
    pub fn of_cable(&self, c: Cable) -> Option<Channel> {
        let i = usize::from(c.get()).checked_sub(1)?;
        let v = self.0.get(i)?.load(Ordering::Relaxed);
        Some(Channel::clamped(v.checked_sub(1).unwrap_or(i as u8)))
    }
}

impl Default for PartChannels {
    fn default() -> Self {
        Self::new()
    }
}

/// MIDI CONFIG › CONTROL as the UI last judged it (ADR 0254): AUTO's
/// channel and the focused Part's, and SEQUENCER's, each only where no
/// Part's own channel takes it. The UI stores, the pipes read. Bytes 0-2:
/// AUTO, its Part's, SEQUENCER's, each the channel plus one, 0 none; all
/// zero, so `.bss`.
pub struct Routes(AtomicU32);

pub static ROUTES: Routes = Routes(AtomicU32::new(0));

impl Routes {
    /// `auto`: AUTO's channel and the focused Part's; `seq`: SEQUENCER's.
    #[inline(always)]
    pub fn set(&self, auto: Option<(Channel, Channel)>, seq: Option<Channel>) {
        let b = |c: Option<Channel>| c.map_or(0, |c| u32::from(c.get()) + 1);
        let (a, to) = auto.unzip();
        self.0
            .store(b(a) | b(to) << 8 | b(seq) << 16, Ordering::Relaxed);
    }

    /// AUTO's channel and the focused Part's, from one load.
    #[inline(always)]
    pub fn auto(&self) -> Option<(Channel, Channel)> {
        let v = self.0.load(Ordering::Relaxed);
        let at = |byte: u32| Channel::new(((v >> (8 * byte)) as u8).checked_sub(1)?);
        Some((at(0)?, at(1)?))
    }

    #[inline(always)]
    pub fn seq(&self) -> Option<Channel> {
        let v = (self.0.load(Ordering::Relaxed) >> 16) as u8;
        Channel::new(v.checked_sub(1)?)
    }
}

/// A channel's bit in a `u16` of channels.
#[inline(always)]
fn bit(c: Channel) -> u16 {
    1 << (c.get() & 15)
}

/// AUTO's keys: every key's with the MIDI UI, none without it (the bench
/// builds, deaf, whose AXI has no room): there AUTO is never routed.
const AUTO_KEYS: usize = if cfg!(feature = "controller-maps") {
    128
} else {
    0
};

/// AUTO's held keys and pedals (ADR 0254), as Elektron's auto channel: a
/// note-on or pedal-down plays the focused Part's channel at once; a
/// note-off, a held key's poly pressure and a pedal-up on the channel they
/// came on reach the channels their note-on or pedal-down went to, even
/// once AUTO has moved or is OFF. Checked before AUTO's route.
struct AutoLatch {
    /// The keys held through AUTO, a bit each.
    held: u128,
    /// Each held key's source channel (high nibble) and Part channel (low).
    keys: [u8; AUTO_KEYS],
    /// The channels a pedal-down went to, a bit each, and the channel the
    /// pedal came on.
    pedals: u16,
    pedal_from: u8,
}

impl AutoLatch {
    const NONE: Self = Self {
        held: 0,
        keys: [0; AUTO_KEYS],
        pedals: 0,
        pedal_from: 0,
    };

    /// Key `k`'s source and Part channels, if held through AUTO.
    fn latched(&self, k: u8) -> Option<(u8, u8)> {
        let v = *self
            .keys
            .get(usize::from(k))
            .filter(|_| self.held >> k & 1 != 0)?;
        Some((v >> 4, v & 15))
    }

    /// The Part channels of the keys held from channel `src`, a bit each,
    /// let go if `clear`.
    fn held_from(&mut self, src: u8, clear: bool) -> u16 {
        let mut to = 0;
        for k in 0..128u8 {
            if let Some((_, ch)) = self.latched(k).filter(|l| l.0 == src) {
                to |= 1 << ch;
                if clear {
                    self.held &= !(1 << k);
                }
            }
        }
        to
    }

    /// The channels `v`, heard on `c`, reaches, a bit each.
    fn voice(&mut self, c: Channel, v: &Voice) -> u16 {
        let src = c.get() & 15;
        // What AUTO latched answers first, wherever AUTO is now.
        match *v {
            Voice::NoteOff { key, .. } | Voice::PolyPressure { key, .. } => {
                if let Some((_, to)) = self.latched(key.get()).filter(|l| l.0 == src) {
                    if matches!(v, Voice::NoteOff { .. }) {
                        self.held &= !(1 << key.get());
                    }
                    return 1 << to;
                }
            }
            Voice::Control { cc, value }
                if cc.get() == 64
                    && value.get() < 64
                    && self.pedals != 0
                    && self.pedal_from == src =>
            {
                let now = ROUTES.auto().and_then(|(a, to)| (a == c).then_some(to));
                return core::mem::take(&mut self.pedals) | now.map_or(bit(c), bit);
            }
            _ => {}
        }
        let Some(to) = ROUTES.auto().and_then(|(a, to)| (a == c).then_some(to)) else {
            return bit(c);
        };
        match *v {
            Voice::NoteOn { key, .. } => {
                if let Some(k) = self.keys.get_mut(usize::from(key.get())) {
                    *k = src << 4 | to.get();
                    self.held |= 1 << key.get();
                }
            }
            Voice::Control { cc, value } if cc.get() == 64 && value.get() >= 64 => {
                self.pedals |= bit(to);
                self.pedal_from = src;
            }
            _ => {}
        }
        bit(to)
    }

    /// The channels the channel mode `m`, heard on `c`, reaches: the
    /// focus's (or `c`'s) and every channel AUTO holds something on from
    /// `c`, each let go of what `m` ends.
    fn mode(&mut self, c: Channel, m: &stream::ChannelMode) -> u16 {
        let src = c.get() & 15;
        let now = ROUTES.auto().and_then(|(a, to)| (a == c).then_some(to));
        let pedals = if self.pedal_from == src {
            self.pedals
        } else {
            0
        };
        let notes = !matches!(
            m,
            stream::ChannelMode::ResetAllControllers | stream::ChannelMode::LocalControl(_)
        );
        let to = now.map_or(bit(c), bit) | self.held_from(src, notes) | pedals;
        if matches!(m, stream::ChannelMode::ResetAllControllers) && pedals != 0 {
            self.pedals = 0;
        }
        to
    }
}

/// Where a pipe's events go; `false`: refused (full), the event is lost.
pub trait Sinks {
    fn note(&mut self, e: InEvent) -> bool;
    fn ctrl(&mut self, e: CtrlEvent) -> bool;
    /// A manufacturer's SysEx body for the UI's SysEx bindings (#846);
    /// a sink that keeps none takes it and drops it.
    fn sysex(&mut self, _body: &[u8]) -> bool {
        true
    }
}

/// The longest SysEx body the UI hears: a parameter message, not a dump.
pub const SX_LEN: usize = chimera_midi::sxmap::MAX_LEN;
/// Bodies a port holds for the UI: a lap's worth of an editor's knob.
pub const SX_SLOTS: usize = 4;
const _: () = assert!(
    SX_SLOTS.is_power_of_two(),
    "`SxRing` indexes by `% SX_SLOTS`"
);

/// One port's SysEx bodies for the UI, as `CtrlRing`: one producer, one
/// consumer, a slot's length then its bytes, published by `tail`. Full,
/// the newest is dropped.
pub struct SxRing {
    slots: [[AtomicU32; 1 + SX_LEN / 4]; SX_SLOTS],
    head: AtomicU32,
    tail: AtomicU32,
}

impl Default for SxRing {
    fn default() -> Self {
        Self::new()
    }
}

impl SxRing {
    pub const fn new() -> Self {
        Self {
            slots: [const { [const { AtomicU32::new(0) }; 1 + SX_LEN / 4] }; SX_SLOTS],
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
        }
    }

    /// Producer side only: `false` if full or longer than `SX_LEN`.
    pub fn push(&self, body: &[u8]) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if body.len() > SX_LEN
            || tail.wrapping_sub(self.head.load(Ordering::Acquire)) as usize >= SX_SLOTS
        {
            return false;
        }
        let slot = &self.slots[tail as usize % SX_SLOTS];
        slot[0].store(body.len() as u32, Ordering::Relaxed);
        for (w, c) in slot[1..].iter().zip(body.chunks(4)) {
            let mut b = [0; 4];
            b[..c.len()].copy_from_slice(c);
            w.store(u32::from_le_bytes(b), Ordering::Relaxed);
        }
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// Consumer side only: the oldest body into `out`, its length.
    pub fn pop(&self, out: &mut [u8; SX_LEN]) -> Option<usize> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let slot = &self.slots[head as usize % SX_SLOTS];
        let n = (slot[0].load(Ordering::Relaxed) as usize).min(SX_LEN);
        for (c, w) in out.chunks_mut(4).zip(&slot[1..]) {
            c.copy_from_slice(&w.load(Ordering::Relaxed).to_le_bytes());
        }
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(n)
    }
}

/// Each port's SysEx bodies, by `Port`: global, as `played`'s; the UI
/// drains a port's on its `CtrlEvent::SysEx`.
pub static SYSEX_IN: [SxRing; Port::COUNT] = [SxRing::new(), SxRing::new()];

/// What a pipe lost or ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PipeStats {
    pub faults: FaultCounts,
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
/// A SysEx body's room: an MTS bulk dump with its bank, 407 B (#217),
/// with `tuning`; without it (the bench builds, for their AXI) the 32 B a
/// controller's messages need.
pub const SYSEX_LEN: usize = if cfg!(feature = "tuning") { 408 } else { 32 };

pub struct DinPipe {
    parser: Parser<SYSEX_LEN>,
    ctl: Controllers,
    sense: Sensing,
    stats: PipeStats,
    /// A bit a channel whose NotesOff `lost` owes the queue.
    owed: u16,
    pedals: Pedals,
    expr: ExprOwed,
    auto: AutoLatch,
}

/// Each channel's wheel, pressure and timbre the queue refused, latest
/// wins: owed until it takes them, so the last value always lands (a
/// dropped final 0 would hold a source up). Poly pressure, per key, is not
/// kept.
struct ExprOwed {
    /// Bit `3 × channel + kind`: kind 0 the wheel, 1 the pressure, 2 the
    /// timbre (ADR 0252).
    bits: u64,
    v: [[u8; 3]; 16],
}

impl ExprOwed {
    const NONE: Self = Self {
        bits: 0,
        v: [[0; 3]; 16],
    };

    /// `e` taken (`ok`) or refused: a refused wheel, pressure or timbre is
    /// owed, a taken one clears what was. Owed values go ahead of anything
    /// later (`settle`), so a reset after one still has the last word.
    fn heard(&mut self, e: InEvent, ok: bool) {
        let InEvent::Expr(ExprEvent { channel, expr }) = e else {
            return;
        };
        let (k, v) = match expr {
            Expr::Wheel(v) => (0, v),
            Expr::Pressure(v) => (1, v),
            Expr::Timbre(v) => (2, v),
            Expr::Poly(..) => return,
        };
        let c = usize::from(channel.get()) & 15;
        let bit = 1 << (3 * c + k);
        if ok {
            self.bits &= !bit;
        } else {
            (self.bits, self.v[c][k]) = (self.bits | bit, v.get());
        }
    }

    /// The lowest owed event.
    fn next(&self) -> Option<InEvent> {
        let i = self.bits.trailing_zeros() as usize;
        let v = U7::new(*self.v.get(i / 3)?.get(i % 3)?)?;
        let expr = match i % 3 {
            0 => Expr::Wheel(v),
            1 => Expr::Pressure(v),
            _ => Expr::Timbre(v),
        };
        Some(InEvent::Expr(ExprEvent {
            channel: Channel::clamped((i / 3) as u8),
            expr,
        }))
    }
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
                refused: 0,
                ctrl_refused: 0,
            },
            owed: 0,
            pedals: Pedals { down: 0, up: 0 },
            expr: ExprOwed::NONE,
            auto: AutoLatch::NONE,
        }
    }

    /// Empty, it queues what `lost` still owes: the poll's retry.
    pub fn feed<S: Sinks + ?Sized>(&mut self, bytes: &[u8], out: &mut S) {
        self.run(bytes, out, None);
    }

    /// `feed`, keeping `held` (USB's) by what the queue takes. One copy:
    /// the door's and USB's entries share it.
    #[inline(never)]
    fn run<S: Sinks + ?Sized>(&mut self, bytes: &[u8], out: &mut S, mut held: Option<&mut Held>) {
        Counted {
            out: &mut *out,
            stats: &mut self.stats,
            owed: &mut self.owed,
            pedals: &mut self.pedals,
            expr: &mut self.expr,
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
                expr: &mut self.expr,
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
                        self.auto = AutoLatch::NONE;
                        // What a busy queue refuses is owed, as `lost`'s: a
                        // NotesOff for a refused SoundOff, a pedal-up for a
                        // refused ResetControllers (no note sticks). The
                        // other controllers it would reset (bend, mod) are
                        // not owed: they stay where they were.
                        for c in 0..16 {
                            let (channel, bit) = (Channel::clamped(c), 1 << c);
                            if !sink.mode(channel, ChannelMode::SoundOff) {
                                *sink.owed |= bit;
                            }
                            if !sink.mode(channel, ChannelMode::ResetControllers) {
                                sink.pedals.up |= sink.pedals.down & bit;
                                sink.pedals.down &= !bit;
                            }
                        }
                    }
                },
                Parsed::Event(Event::Voice(c, v)) => {
                    // MIDI CONFIG › CONTROL (ADR 0254): SEQUENCER's notes
                    // are ORBIT's KEY, never played; AUTO's play the focus.
                    if ROUTES.seq() == Some(c)
                        && matches!(v, Voice::NoteOn { .. } | Voice::NoteOff { .. })
                    {
                        if let Voice::NoteOn { key, .. } = v {
                            sink.ctrl(CtrlEvent::Key(key));
                        }
                        continue;
                    }
                    let mut to = self.auto.voice(c, &v);
                    while to != 0 {
                        let c = Channel::clamped(to.trailing_zeros() as u8);
                        to &= to - 1;
                        // The wheel and pressure feed their matrix sources; the
                        // wheel and channel pressure reach the map too.
                        if let Some(e) = InEvent::expr(c, &v) {
                            sink.note(e);
                        }
                        match InEvent::from_event(c, &v) {
                            Some(e) => sink.note(e),
                            None => match self.ctl.hear(c, v) {
                                Heard::Ctl(ctl) => {
                                    // Queued, a program change holds its
                                    // channel's notes till the UI applies it
                                    // (#515).
                                    if sink.ctrl(CtrlEvent::Ctl(c, ctl))
                                        && matches!(ctl, Ctl::Program { .. })
                                        && PROGRAM_IN.load(Ordering::Relaxed)
                                    {
                                        PROGRAM_PENDING
                                            .fetch_or(1 << (c.get() & 15), Ordering::Release);
                                    }
                                }
                                Heard::Note | Heard::Consumed => {}
                            },
                        }
                    }
                }
                Parsed::Event(Event::Mode(c, m)) => {
                    let mut to = self.auto.mode(c, &m);
                    while to != 0 {
                        let c = Channel::clamped(to.trailing_zeros() as u8);
                        to &= to - 1;
                        if let Some(e) = InEvent::from_mode(c, m) {
                            sink.note(e);
                        }
                        // GM resets expression with the rest (ADR 0263).
                        if matches!(m, stream::ChannelMode::ResetAllControllers) {
                            sink.ctrl(CtrlEvent::Reset(c));
                        }
                    }
                }
                Parsed::Event(Event::SysEx(s)) => {
                    // MTS retunes the MTS table at once (ADR 0240).
                    if let Some(m) = sysex::mts(s, U7::ZERO) {
                        m.each(|k, o| crate::tuning::set_mts(k.get(), o));
                    }
                    if sysex::request(s, U7::ZERO).is_some()
                        && let Some(dev) = s.bytes().get(1).and_then(|&d| U7::new(d))
                    {
                        sink.ctrl(CtrlEvent::Identity { dev });
                    } else if let Some(who) = sysex::dump_request(s) {
                        // Chimera's own (#984): never a binding's.
                        sink.ctrl(CtrlEvent::Dump(who));
                    } else if !matches!(s.bytes().first(), Some(0x7E | 0x7F) | None)
                        && s.bytes().len() <= SX_LEN
                    {
                        // A manufacturer's message: a SysEx binding's, maybe;
                        // a longer one (a dump) is none's.
                        sink.sysex(s.bytes());
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
    #[inline(never)]
    fn lost<S: Sinks + ?Sized>(&mut self, out: &mut S, mut held: Option<&mut Held>) {
        let told = self.pedals.down != 0
            || held
                .as_deref()
                .is_none_or(|h| h.keys.iter().any(|&k| k != 0));
        self.owed = u16::MAX;
        self.auto = AutoLatch::NONE;
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
            expr: &mut self.expr,
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
    /// The port's `SYSEX_IN` ring, where a UI drains it (`midi-map`);
    /// `None`, a manufacturer's SysEx is dropped (#846).
    pub sysex: Option<&'a SxRing>,
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

    /// The marker first: refused, no body waits that no marker drains.
    #[inline]
    fn sysex(&mut self, body: &[u8]) -> bool {
        let Some(sx) = self.sysex else {
            return true;
        };
        self.ctrl.push(CtrlEvent::SysEx) && sx.push(body)
    }
}

impl<'q, const N: usize> DinSinks<'_, 'q, N> {
    /// The same sinks, the ring's size a value.
    #[inline(always)]
    fn erased(&mut self) -> PipeSinks<'_, 'q> {
        PipeSinks {
            notes: &mut *self.notes,
            ctrl: self.ctrl.tx(),
            sysex: self.sysex,
        }
    }
}

/// `DinSinks` of either ring: the one type the door's and USB's entries
/// run the pipe on, so its parser links once, not once a ring size (ADR
/// 0251).
struct PipeSinks<'a, 'q> {
    notes: &'a mut NoteProducer<'q>,
    ctrl: RingTx<'a>,
    sysex: Option<&'a SxRing>,
}

impl Sinks for PipeSinks<'_, '_> {
    #[inline]
    fn note(&mut self, e: InEvent) -> bool {
        self.notes.push(e)
    }

    #[inline]
    fn ctrl(&mut self, e: CtrlEvent) -> bool {
        self.ctrl.push(e)
    }

    /// `DinSinks::sysex`: the marker first, then the body.
    #[inline]
    fn sysex(&mut self, body: &[u8]) -> bool {
        let Some(sx) = self.sysex else {
            return true;
        };
        self.ctrl.push(CtrlEvent::SysEx) && sx.push(body)
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

    /// NotesOff or pedal-ups `lost` still owes the queue, or a refused
    /// wheel, pressure or timbre (`ExprOwed`): an empty `feed` retries them. Field
    /// reads, so bank 1's shells may poll it (#884).
    #[inline(always)]
    pub fn owes(&self) -> bool {
        self.pipe()
            .is_some_and(|p| p.owed != 0 || p.pedals.up != 0 || p.expr.bits != 0)
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

    /// What its pipe owes (`PipeSlot::owes`).
    #[inline(always)]
    pub fn owes(&self) -> bool {
        self.slot.owes()
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

    /// One whole channel message (a Part cable's, ADR 0243) through the
    /// pipe's controllers and sinks past its parser, so cable 0's SysEx
    /// or running status around it stands.
    #[inline(always)]
    pub fn feed_msg<S: Sinks + ?Sized>(&mut self, msg: &[u8], out: &mut S) {
        let p = self.slot.built();
        let kept = core::mem::replace(&mut p.parser, Parser::new());
        p.run(msg, out, Some(&mut self.held));
        p.parser = kept;
        p.stats.faults = p.parser.faults();
    }

    /// The host went away (`DinPipe::lost`): every key and pedal let go.
    #[inline(always)]
    pub fn lost<S: Sinks + ?Sized>(&mut self, out: &mut S) {
        self.slot.built().lost(out, Some(&mut self.held));
    }
}

/// SETTINGS › MIDI CONFIG › SYNC's PROGRAM IN as the UI last saw it: a
/// pipe holds a channel's notes for a program change only while it is on
/// (ADR 0224).
pub static PROGRAM_IN: AtomicBool = AtomicBool::new(false);

/// A bit a channel whose program change the UI has yet to apply: set by a
/// pipe as it queues one, cleared by the UI the pass after it applied it
/// (its load published) or by the drain's timeout (`NoteHold`).
pub static PROGRAM_PENDING: AtomicU16 = AtomicU16::new(0);

/// Events a hold keeps.
pub const HOLD_LEN: usize = 16;
/// Blocks a program change holds its channel at most: 40 ms (owner
/// decision 7, #515).
pub const HOLD_BLOCKS: u32 = (40 * crate::hw::SAMPLE_RATE / 1000) / crate::hw::BLOCK_SIZE as u32;

/// The audio's hold (`NoteDrain::drain`): a pending channel's events, in
/// order, each with its source, till the UI applies its program change.
/// Each block's step moves what may play into `ready` (`ProgramHold`'s), a
/// ring a source the drain empties instead of the source's queue that
/// block, so the drain stays one loop and bank 1 keeps one copy of the
/// Parts' note handling. In `ProgramHold`, a static, so no frame holds
/// it; the audio thread's alone, atomics only so a static can hold it.
pub struct NoteHold {
    /// Packed events (`InEvent::pack`) and their sources.
    at: [AtomicU32; HOLD_LEN],
    src: [AtomicU8; HOLD_LEN],
    len: AtomicU8,
    /// Blocks some channel has been pending.
    age: AtomicU32,
}

impl Default for NoteHold {
    fn default() -> Self {
        Self::new()
    }
}

/// The channel an event plays on; real-time has none. Inlined into
/// `NoteHold::step`, so it is bank 2's.
#[inline(always)]
fn channel_of(e: InEvent) -> Option<u8> {
    Some(match e {
        InEvent::Note(n) => n.channel.get(),
        InEvent::Bend(b) => b.channel.get(),
        InEvent::Sustain(p) => p.channel.get(),
        InEvent::Mode(m) => m.channel.get(),
        InEvent::Expr(x) => x.channel.get(),
        InEvent::Rt(_) => return None,
    })
}

impl NoteHold {
    pub const fn new() -> Self {
        Self {
            at: [const { AtomicU32::new(0) }; HOLD_LEN],
            src: [const { AtomicU8::new(0) }; HOLD_LEN],
            len: AtomicU8::new(0),
            age: AtomicU32::new(0),
        }
    }

    /// Nothing held: a drain with no channel pending needs no door.
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len.load(Ordering::Relaxed) == 0
    }

    /// One block's sort (bank 2's, the door's `hold`), `ready` empty: what
    /// waits on a channel no longer pending first, then each source's
    /// queue, a pending channel's events kept, as many as `ready` has
    /// room for (the rest stay queued for the next block's sort: the drain
    /// plays only `ready`). A full hold lets its oldest go ahead of the
    /// new one, never drops it. Past `HOLD_BLOCKS` every pending bit is
    /// cleared.
    fn step(&self, queues: &[NoteQueue], ready: &[Ready]) {
        let mut pend = PROGRAM_PENDING.load(Ordering::Acquire);
        let mut age = if pend == 0 {
            0
        } else {
            self.age.load(Ordering::Relaxed) + 1
        };
        if age > HOLD_BLOCKS {
            PROGRAM_PENDING.fetch_and(!pend, Ordering::Relaxed);
            (pend, age) = (0, 0);
        }
        self.age.store(age, Ordering::Relaxed);
        let held = |e: InEvent| channel_of(e).is_some_and(|c| pend >> (c & 15) & 1 != 0);
        // The hold, unpacked: what fails to unpack was never packed.
        let mut at = [(0u8, InEvent::Rt(RealTime::Clock)); HOLD_LEN];
        let mut len = 0;
        for i in 0..usize::from(self.len.load(Ordering::Relaxed)).min(HOLD_LEN) {
            if let Some(e) = InEvent::unpack(self.at[i].load(Ordering::Relaxed)) {
                at[len] = (self.src[i].load(Ordering::Relaxed), e);
                len += 1;
            }
        }
        let mut room = [READY_LEN; MAX_NOTE_SOURCES];
        let mut k = 0;
        for r in 0..len {
            let (s, e) = at[r];
            let si = usize::from(s);
            if !held(e) && ready.get(si).is_some_and(|r| r.push(e)) {
                room[si] -= 1;
            } else {
                at[k] = (s, e);
                k += 1;
            }
        }
        len = k;
        for (i, q) in queues.iter().enumerate().take(ready.len()) {
            // Full, the hold lets its oldest go to make room: only while
            // that one's source has Ready room, else this queue waits.
            while room[i] > 0
                && (len < HOLD_LEN || room[usize::from(at[0].0)] > 0)
                && let Some(e) = q.pop()
            {
                if !held(e) {
                    ready[i].push(e);
                    room[i] -= 1;
                    continue;
                }
                if len == HOLD_LEN {
                    let (s, old) = at[0];
                    at.copy_within(1.., 0);
                    len -= 1;
                    ready[usize::from(s)].push(old);
                    room[usize::from(s)] -= 1;
                }
                at[len] = (i as u8, e);
                len += 1;
            }
        }
        for (j, &(s, e)) in at.iter().enumerate().take(len) {
            self.at[j].store(e.pack(), Ordering::Relaxed);
            self.src[j].store(s, Ordering::Relaxed);
        }
        self.len.store(len as u8, Ordering::Relaxed);
    }

    pub(crate) fn door_hold(h: &NoteHold, queues: &[NoteQueue], ready: &[Ready]) {
        h.step(queues, ready);
    }
}

/// `NoteHold::step` through bank 2's door with `bank2`; whether it ran.
/// Closed, nothing: the drain plays the queues as they are.
#[inline(always)]
pub fn hold(h: &NoteHold, queues: &[NoteQueue], ready: &[Ready]) -> bool {
    #[cfg(feature = "bank2")]
    return crate::bank2::DOOR_CELL
        .with(|d| (d.midi.mpe)(crate::mpe::Call::Hold(h, queues, ready)))
        .is_some();
    #[cfg(not(feature = "bank2"))]
    {
        NoteHold::door_hold(h, queues, ready);
        true
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

/// MIDI in's entries in bank 2's door: the pipe's two layers, the map's
/// codec, and MPE's zone reads for the audio (ADR 0252).
#[repr(C)]
pub struct MidiDoor {
    pub feed: MidiFeed,
    pub idle: MidiIdle,
    pub decode_map: MapDecode,
    pub encode_map: MapEncode,
    pub mpe: fn(crate::mpe::Call<'_>),
}

impl MidiDoor {
    pub const DOOR: MidiDoor = MidiDoor {
        feed: DinPipe::door_feed,
        idle: DinPipe::door_idle,
        decode_map: codec::decode,
        encode_map: codec::encode,
        mpe: crate::mpe::kern::call,
    };
}

/// The door's entries: `feed` and `idle` on a shell's slot and sinks,
/// `DinPipe`'s so `bank2.x`'s glob places them.
impl DinPipe {
    fn door_feed(slot: &mut PipeSlot, bytes: &[u8], out: &mut DinSinks<'_, '_>) {
        slot.built().feed(bytes, &mut out.erased());
    }

    fn door_idle(slot: &mut PipeSlot, out: &mut DinSinks<'_, '_>) {
        slot.built().idle(&mut out.erased());
    }

    /// `door_feed` on USB's ring, for a caller already in bank 2 (USB's
    /// poll): no door call. Not generic, so it builds at this crate's
    /// level as the door's: built at the board's, the machine outliner put
    /// a call into bank 2 in a bank-1 outline.
    #[inline(never)]
    pub fn usb_feed(slot: &mut UsbSlot, bytes: &[u8], out: &mut DinSinks<'_, '_, CTRL_USB_LEN>) {
        slot.feed(bytes, &mut out.erased());
    }

    /// `UsbSlot::lost`, as `usb_feed`.
    #[inline(never)]
    pub fn usb_lost(slot: &mut UsbSlot, out: &mut DinSinks<'_, '_, CTRL_USB_LEN>) {
        slot.lost(&mut out.erased());
    }

    /// `UsbSlot::feed_msg`, as `usb_feed`.
    #[inline(never)]
    pub fn usb_feed_msg(slot: &mut UsbSlot, msg: &[u8], out: &mut DinSinks<'_, '_, CTRL_USB_LEN>) {
        slot.feed_msg(msg, &mut out.erased());
    }

    /// `usb_feed_msg` on DIN's ring size, as `usb_feed_short`.
    #[inline(never)]
    pub fn usb_feed_msg_short(slot: &mut UsbSlot, msg: &[u8], out: &mut DinSinks<'_, '_>) {
        slot.feed_msg(msg, &mut out.erased());
    }

    /// `usb_feed` on DIN's ring size: builds without the map.
    #[inline(never)]
    pub fn usb_feed_short(slot: &mut UsbSlot, bytes: &[u8], out: &mut DinSinks<'_, '_>) {
        slot.feed(bytes, &mut out.erased());
    }

    /// `usb_lost` on DIN's ring size, as `usb_feed_short`.
    #[inline(never)]
    pub fn usb_lost_short(slot: &mut UsbSlot, out: &mut DinSinks<'_, '_>) {
        slot.lost(&mut out.erased());
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
    expr: &'a mut ExprOwed,
    held: Option<&'a mut Held>,
}

impl<S: Sinks + ?Sized> Counted<'_, S> {
    fn note(&mut self, e: InEvent) {
        self.push(e);
    }

    /// Refused while NotesOff are owed, so none overtakes them; `false`:
    /// refused. A refused wheel or pressure is owed, not counted
    /// (`ExprOwed`).
    fn push(&mut self, e: InEvent) -> bool {
        let ok = self.settle() && self.out.note(e);
        self.expr.heard(e, ok);
        if ok {
            self.pedals.heard(e);
            if let Some(h) = self.held.as_deref_mut() {
                h.heard(e);
            }
            true
        } else {
            if !matches!(e, InEvent::Expr(_)) {
                self.stats.refused = self.stats.refused.saturating_add(1);
            }
            false
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
        while let Some(e) = self.expr.next() {
            if !self.out.note(e) {
                return false;
            }
            self.expr.heard(e, true);
        }
        true
    }

    fn ctrl(&mut self, e: CtrlEvent) -> bool {
        let took = self.out.ctrl(e);
        if !took {
            self.stats.ctrl_refused = self.stats.ctrl_refused.saturating_add(1);
        }
        took
    }

    fn sysex(&mut self, body: &[u8]) {
        if !self.out.sysex(body) {
            self.stats.ctrl_refused = self.stats.ctrl_refused.saturating_add(1);
        }
    }

    fn mode(&mut self, channel: Channel, mode: ChannelMode) -> bool {
        self.push(InEvent::Mode(ChannelModeEvent { channel, mode }))
    }
}
