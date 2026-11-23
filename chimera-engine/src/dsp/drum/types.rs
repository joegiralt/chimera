// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! DRUM's types (ADR 0265): every index, value, code and mode a newtype or
//! an enum, built clamped or as an `Option`, never by a panic.

use chimera_midi::value::U7;

use super::sound::PadSound;
use crate::MidiNote;
use crate::block::DiskCode;
use crate::dsp::tape_engine::params::Retrig;
use crate::dsp::{Unit, clampf};
use crate::hw::Cost;

/// Pads in a kit.
const PADS: u8 = 12;

/// A pad's bytes (`Pad::to_bytes`).
pub const PAD_BYTES: usize = 13;
pub type PadBytes = [u8; PAD_BYTES];

/// One of the kit's 12 pads, 0..=11 (C0–B0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PadId(u8);

impl PadId {
    pub const ALL: [PadId; PADS as usize] = {
        let mut a = [PadId(0); PADS as usize];
        let mut i = 0;
        while i < PADS {
            a[i as usize] = PadId(i);
            i += 1;
        }
        a
    };

    pub const fn new(i: u8) -> Option<PadId> {
        if i < PADS { Some(PadId(i)) } else { None }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// 1..=12, as shown.
    pub const fn number(self) -> u8 {
        self.0 + 1
    }

    /// The next pad, 12 wrapping to 1.
    pub const fn next(self) -> PadId {
        PadId((self.0 + 1) % PADS)
    }

    /// The previous pad, 1 wrapping to 12.
    pub const fn prev(self) -> PadId {
        PadId((self.0 + PADS - 1) % PADS)
    }
}

/// A set of pads, bit `i` pad `i`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadSet(u16);

impl PadSet {
    pub const EMPTY: PadSet = PadSet(0);

    pub const fn with(self, p: PadId) -> PadSet {
        PadSet(self.0 | 1 << p.0)
    }

    pub const fn contains(self, p: PadId) -> bool {
        self.0 & 1 << p.0 != 0
    }

    pub const fn union(self, o: PadSet) -> PadSet {
        PadSet(self.0 | o.0)
    }

    /// The bits, for an atomic word or a key.
    pub const fn bits(self) -> u16 {
        self.0
    }

    /// `bits` back, the bits past the pads dropped.
    pub const fn from_bits(b: u16) -> PadSet {
        PadSet(b & ((1 << PADS) - 1))
    }

    /// In pad order.
    pub fn iter(self) -> impl Iterator<Item = PadId> {
        PadId::ALL.into_iter().filter(move |&p| self.contains(p))
    }
}

/// A machine knob: A–F on the machine's page, G and H on MORE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Knob {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
}

impl Knob {
    pub const ALL: [Knob; 8] = [
        Knob::A,
        Knob::B,
        Knob::C,
        Knob::D,
        Knob::E,
        Knob::F,
        Knob::G,
        Knob::H,
    ];
    /// The machine page's six.
    pub const MAIN: [Knob; 6] = [Knob::A, Knob::B, Knob::C, Knob::D, Knob::E, Knob::F];
    /// MORE's two.
    pub const MORE: [Knob; 2] = [Knob::G, Knob::H];
}

/// A knob's stored position, 0..=127.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KnobValue(U7);

impl KnobValue {
    pub const ZERO: KnobValue = KnobValue(U7::ZERO);

    pub const fn new(v: U7) -> KnobValue {
        KnobValue(v)
    }

    pub const fn get(self) -> U7 {
        self.0
    }

    /// 0..=1, both ends exact: `k / 127`.
    pub fn unit(self) -> Unit {
        Unit::new(f32::from(self.0.get()) / 127.0)
    }
}

/// A pad's eight knob positions, reached by `Knob` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnobSet([KnobValue; 8]);

impl KnobSet {
    pub const ZERO: KnobSet = KnobSet([KnobValue::ZERO; 8]);

    pub const fn new(v: [KnobValue; 8]) -> KnobSet {
        KnobSet(v)
    }

    pub const fn get(&self, k: Knob) -> KnobValue {
        self.0[k as usize]
    }

    pub const fn set(&mut self, k: Knob, v: KnobValue) {
        self.0[k as usize] = v;
    }
}

/// What kernel a machine runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kernel {
    Tone,
    Metal,
    Burst,
    Plain,
}

/// A pad's machine. Codes are permanent (ADR 0045): later machines take
/// the next free code, and a code is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Machine {
    ToneKick,
    ToneSnare,
    ToneTom,
    ToneClap,
    ToneCowbell,
    ToneCymbal,
    MetalHat,
    MetalJingle,
    BodyShaker,
    PlainSine,
    PlainNoise,
    PlainImpulse,
}

impl Machine {
    pub const ALL: [Machine; 12] = [
        Machine::ToneKick,
        Machine::ToneSnare,
        Machine::ToneTom,
        Machine::ToneClap,
        Machine::ToneCowbell,
        Machine::ToneCymbal,
        Machine::MetalHat,
        Machine::MetalJingle,
        Machine::BodyShaker,
        Machine::PlainSine,
        Machine::PlainNoise,
        Machine::PlainImpulse,
    ];

    /// Its place in `ALL`: MACHINE's value.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// It has a PITCH a TONAL note moves; CLAP, SHAKER and NOISE play the
    /// same hit on every note (ADR 0270; `KernelParams::transposed`).
    pub const fn pitched(self) -> bool {
        !matches!(
            self,
            Machine::ToneClap | Machine::BodyShaker | Machine::PlainNoise
        )
    }

    /// The disk code, fixed here by hand.
    pub const fn code(self) -> u8 {
        match self {
            Machine::ToneKick => 1,
            Machine::ToneSnare => 2,
            Machine::ToneTom => 3,
            Machine::ToneClap => 4,
            Machine::ToneCowbell => 5,
            Machine::ToneCymbal => 6,
            Machine::MetalHat => 7,
            Machine::MetalJingle => 8,
            Machine::BodyShaker => 9,
            Machine::PlainSine => 10,
            Machine::PlainNoise => 11,
            Machine::PlainImpulse => 12,
        }
    }

    /// METAL JINGLE's shaking is the burst's particles gating the metal
    /// bank; the bank is its sound, so it is a metal machine.
    pub const fn kernel(self) -> Kernel {
        match self {
            Machine::ToneKick | Machine::ToneSnare | Machine::ToneTom => Kernel::Tone,
            Machine::ToneClap | Machine::BodyShaker => Kernel::Burst,
            Machine::ToneCowbell
            | Machine::ToneCymbal
            | Machine::MetalHat
            | Machine::MetalJingle => Kernel::Metal,
            Machine::PlainSine | Machine::PlainNoise | Machine::PlainImpulse => Kernel::Plain,
        }
    }

    /// Cycles/sample of its kernel (#951): a hit's `EngineSlot::cost`.
    /// Each is INIT DRUM's pad struck alone on the unit, whole voice over
    /// idle × 1.1 by ADR 0056's count, less the 83 the rest of INIT DRUM's
    /// bill holds (chain, AMP, pads' mods). Unit: `stats` load_pct 80 ms
    /// after a vel-127 strike, 1 % = 100, rev V at 480 MHz, 9c2cfd1d,
    /// 2026-11-14. load_pct averages its window, so a hit that ended in it
    /// reads low (*): those, and the machines INIT DRUM doesn't play, are
    /// placed from desktop release ns per block against measured
    /// machines, erring high. COWBELL and IMPULSE are re-read on eb442a48,
    /// 2026-11-16, at the top of their spread (ADR 0275): their pulses
    /// rendering as square runs; a sample at a time they bill
    /// `Ticking::cost` (ADR 0287).
    pub const fn cost(self) -> Cost {
        Cost(match self {
            Machine::ToneKick => 797,     // 800 (pad 1)
            Machine::ToneSnare => 797,    // 800 (pad 2)
            Machine::ToneTom => 687,      // 700 (pads 5–7)
            Machine::ToneClap => 577,     // 600 (pad 4)
            Machine::ToneCowbell => 687, // 700 (pad 11, BUMP 0 and the plateau after BUMP; ADR 0287)
            Machine::ToneCymbal => 1457, // 1400 (pad 10)
            Machine::MetalHat => 1457,   // 1400 (pad 9, open; closed pad 8 ends inside the window)
            Machine::MetalJingle => 1677, // est. 1600: unmeasured; desktop 7,000 ns, 1.12 × HAT's 6,250
            Machine::BodyShaker => 687, // est. 700: pad 12 read 300 *; desktop 1,370 ns, CLAP's 1,360 (600)
            Machine::PlainSine => 577, // est. 600: unmeasured; desktop 1,100 ns, under CLAP's 1,360 (600)
            Machine::PlainNoise => 577, // est. 600: unmeasured; desktop 1,090 ns, under CLAP's 1,360 (600)
            Machine::PlainImpulse => 577, // 600 (pad 3, DECAY 1.12 s head; plateau 500; ADR 0275)
        })
    }
}

/// How a hit's pulses render its next block (`Pulse::sweep`, ADR 0287):
/// as square runs, or a sample at a time while a sweep moves their pitch,
/// while their width floors at it, or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PulsePace {
    Runs,
    Swept,
    Floored,
    FlooredSwept,
}

impl PulsePace {
    pub const fn of(swept: bool, floored: bool) -> PulsePace {
        match (swept, floored) {
            (false, false) => PulsePace::Runs,
            (true, false) => PulsePace::Swept,
            (false, true) => PulsePace::Floored,
            (true, true) => PulsePace::FlooredSwept,
        }
    }
}

/// A pulse machine's bills while its pulses render a sample at a time
/// (#991). COWBELL's squares never floor (`MAX_INC` < ½).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ticking {
    CowbellSwept,
    ImpulseSwept,
    ImpulseFloored,
    ImpulseFlooredSwept,
}

impl Ticking {
    pub const ALL: [Ticking; 4] = [
        Ticking::CowbellSwept,
        Ticking::ImpulseSwept,
        Ticking::ImpulseFloored,
        Ticking::ImpulseFlooredSwept,
    ];

    pub const fn machine(self) -> Machine {
        match self {
            Ticking::CowbellSwept => Machine::ToneCowbell,
            Ticking::ImpulseSwept | Ticking::ImpulseFloored | Ticking::ImpulseFlooredSwept => {
                Machine::PlainImpulse
            }
        }
    }

    /// Cycles/sample, by `Machine::cost`'s formula (ADR 0287). COWBELL:
    /// BUMP 127's worst block on the unit, `peak_pct` 32 % over the 12 %
    /// idle, eb442a48, 2026-11-16 (ADR 0275). IMPULSE, estimated erring
    /// high, pending the unit: SWEEP 64's head window (700, 37 ms of sweep
    /// in 85 ms over a 500 plateau) puts its sweep near 950, × 1.33 (COWBELL's
    /// worst block over its window); floored, desktop ns put it near 2,000,
    /// with SWEEP 127 near 2,700.
    pub const fn cost(self) -> Cost {
        Cost(match self {
            Ticking::CowbellSwept => 2117, // 2000 (pad 11, BUMP 127, worst block)
            Ticking::ImpulseSwept => 1347, // est. 1300 (SWEEP 64): 1.33 × 950
            Ticking::ImpulseFloored => 2117, // est. 2000 (2 kHz +12, WIDTH 127)
            Ticking::ImpulseFlooredSwept => 2887, // est. 2700 (the same, SWEEP 127)
        })
    }
}

/// What a DRUM hit's kernel is billed (ADR 0287): its machine, or a pulse
/// machine's while its pulses render a sample at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelBill {
    Machine(Machine),
    Ticking(Ticking),
}

/// `KernelBill::ALL`'s length.
const BILLS: usize = Machine::ALL.len() + Ticking::ALL.len();

impl KernelBill {
    pub const ALL: [KernelBill; BILLS] = {
        let mut a = [KernelBill::Machine(Machine::ToneKick); BILLS];
        let mut i = 0;
        while i < BILLS {
            a[i] = if i < Machine::ALL.len() {
                KernelBill::Machine(Machine::ALL[i])
            } else {
                KernelBill::Ticking(Ticking::ALL[i - Machine::ALL.len()])
            };
            i += 1;
        }
        a
    };

    /// Its place in `ALL`.
    pub const fn index(self) -> usize {
        match self {
            KernelBill::Machine(m) => m.index(),
            KernelBill::Ticking(t) => Machine::ALL.len() + t as usize,
        }
    }

    /// `m`'s bill while its pulses render at `pace`. A COWBELL's squares
    /// never floor; were one to, it bills its sweep, its one per-sample
    /// path.
    pub const fn of(m: Machine, pace: PulsePace) -> KernelBill {
        match (m, pace) {
            (_, PulsePace::Runs) => KernelBill::Machine(m),
            (
                Machine::ToneCowbell,
                PulsePace::Swept | PulsePace::Floored | PulsePace::FlooredSwept,
            ) => KernelBill::Ticking(Ticking::CowbellSwept),
            (Machine::PlainImpulse, PulsePace::Swept) => KernelBill::Ticking(Ticking::ImpulseSwept),
            (Machine::PlainImpulse, PulsePace::Floored) => {
                KernelBill::Ticking(Ticking::ImpulseFloored)
            }
            (Machine::PlainImpulse, PulsePace::FlooredSwept) => {
                KernelBill::Ticking(Ticking::ImpulseFlooredSwept)
            }
            (
                Machine::ToneKick
                | Machine::ToneSnare
                | Machine::ToneTom
                | Machine::ToneClap
                | Machine::ToneCymbal
                | Machine::MetalHat
                | Machine::MetalJingle
                | Machine::BodyShaker
                | Machine::PlainSine
                | Machine::PlainNoise,
                PulsePace::Swept | PulsePace::Floored | PulsePace::FlooredSwept,
            ) => KernelBill::Machine(m),
        }
    }

    /// The costliest bill a hit of `m` can run up, at any knobs and pitch:
    /// a Part's kit bill (`KitBill::kernel`).
    pub const fn worst(m: Machine) -> KernelBill {
        let (mut i, mut w) = (0, KernelBill::Machine(m));
        while i < Ticking::ALL.len() {
            let t = Ticking::ALL[i];
            if t.machine() as usize == m as usize && t.cost().0 > w.cost().0 {
                w = KernelBill::Ticking(t);
            }
            i += 1;
        }
        w
    }

    pub const fn machine(self) -> Machine {
        match self {
            KernelBill::Machine(m) => m,
            KernelBill::Ticking(t) => t.machine(),
        }
    }

    /// Cycles/sample: a hit's `EngineSlot::cost`.
    pub const fn cost(self) -> Cost {
        match self {
            KernelBill::Machine(m) => m.cost(),
            KernelBill::Ticking(t) => t.cost(),
        }
    }

    /// The costliest `cost`: a DRUM hit's bill with no kit to read.
    pub const MAX_COST: Cost = {
        let (mut i, mut m) = (0, Cost::ZERO);
        while i < BILLS {
            m = m.max(KernelBill::ALL[i].cost());
            i += 1;
        }
        m
    };
}

const _: () = {
    let mut i = 0;
    while i < BILLS {
        assert!(KernelBill::ALL[i].index() == i);
        i += 1;
    }
};

impl DiskCode for Machine {
    fn disk_code(self) -> u8 {
        self.code()
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Machine::ToneKick => "TONE_KICK",
            Machine::ToneSnare => "TONE_SNARE",
            Machine::ToneTom => "TONE_TOM",
            Machine::ToneClap => "TONE_CLAP",
            Machine::ToneCowbell => "TONE_COWBELL",
            Machine::ToneCymbal => "TONE_CYMBAL",
            Machine::MetalHat => "METAL_HAT",
            Machine::MetalJingle => "METAL_JINGLE",
            Machine::BodyShaker => "BODY_SHAKER",
            Machine::PlainSine => "PLAIN_SINE",
            Machine::PlainNoise => "PLAIN_NOISE",
            Machine::PlainImpulse => "PLAIN_IMPULSE",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Some(match c {
            1 => Machine::ToneKick,
            2 => Machine::ToneSnare,
            3 => Machine::ToneTom,
            4 => Machine::ToneClap,
            5 => Machine::ToneCowbell,
            6 => Machine::ToneCymbal,
            7 => Machine::MetalHat,
            8 => Machine::MetalJingle,
            9 => Machine::BodyShaker,
            10 => Machine::PlainSine,
            11 => Machine::PlainNoise,
            12 => Machine::PlainImpulse,
            _ => return None,
        })
    }
}

/// A pad's MACHINE (ADR 0318): a machine, or NONE: no machine, so a
/// strike plays nothing and takes no voice, and its kit's bill skips it.
#[derive(Clone, Copy, Debug, Eq)]
pub enum PadMachine {
    Machine(Machine),
    None,
}

impl PadMachine {
    /// MACHINE's values: `Machine::ALL`'s, in its order, then NONE, so no
    /// machine's value moves (ADR 0318).
    pub const ALL: [PadMachine; Machine::ALL.len() + 1] = {
        let mut a = [PadMachine::None; Machine::ALL.len() + 1];
        let mut i = 0;
        while i < Machine::ALL.len() {
            a[i] = PadMachine::Machine(Machine::ALL[i]);
            i += 1;
        }
        a
    };

    /// NONE's disk code: the next free one, never reused (ADR 0045).
    pub const NONE_CODE: u8 = 13;

    /// NONE's name: its ident, option, cell and header.
    pub const NONE_NAME: &'static str = "NONE";

    /// Its place in `ALL`: MACHINE's value.
    pub const fn index(self) -> usize {
        match self {
            PadMachine::Machine(m) => m.index(),
            PadMachine::None => Machine::ALL.len(),
        }
    }

    pub const fn from_index(i: usize) -> Option<PadMachine> {
        if i < Self::ALL.len() {
            Some(Self::ALL[i])
        } else {
            None
        }
    }

    /// The machine it runs; `None` on NONE.
    pub const fn machine(self) -> Option<Machine> {
        match self {
            PadMachine::Machine(m) => Some(m),
            PadMachine::None => None,
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            PadMachine::Machine(m) => m.code(),
            PadMachine::None => Self::NONE_CODE,
        }
    }
}

/// By MACHINE's value: one byte compared, its layout's (the derive's
/// match cost bank 1 96 B in the UI's context compare).
impl PartialEq for PadMachine {
    fn eq(&self, o: &PadMachine) -> bool {
        self.index() == o.index()
    }
}

impl From<Machine> for PadMachine {
    fn from(m: Machine) -> PadMachine {
        PadMachine::Machine(m)
    }
}

impl DiskCode for PadMachine {
    fn disk_code(self) -> u8 {
        self.code()
    }

    fn disk_ident(self) -> &'static str {
        match self {
            PadMachine::Machine(m) => m.disk_ident(),
            PadMachine::None => Self::NONE_NAME,
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            Self::NONE_CODE => Some(PadMachine::None),
            c => Machine::from_disk_code(c).map(PadMachine::Machine),
        }
    }
}

/// A pad's choke group: a hit cuts the sounding pads of its group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChokeGroup {
    Off,
    A,
    B,
    C,
    D,
}

impl ChokeGroup {
    pub const ALL: [ChokeGroup; 5] = [
        ChokeGroup::Off,
        ChokeGroup::A,
        ChokeGroup::B,
        ChokeGroup::C,
        ChokeGroup::D,
    ];

    /// The place in `ALL`: the CHOKE cell's value.
    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(i: usize) -> Option<ChokeGroup> {
        Self::ALL.get(i).copied()
    }

    /// `delta` along `ALL`, saturating at its ends. Inlined: bank 2's
    /// CHOKE turn (`ui::kit_groups`) is its caller.
    #[inline(always)]
    pub fn step(self, delta: i8) -> ChokeGroup {
        let last = Self::ALL.len() - 1;
        Self::ALL[self
            .index()
            .saturating_add_signed(isize::from(delta))
            .min(last)]
    }

    /// The end of `ALL` the way of `delta`.
    #[inline(always)]
    pub fn end(delta: i8) -> ChokeGroup {
        if delta > 0 {
            Self::ALL[Self::ALL.len() - 1]
        } else {
            Self::ALL[0]
        }
    }
}

impl DiskCode for ChokeGroup {
    fn disk_code(self) -> u8 {
        match self {
            ChokeGroup::Off => 0,
            ChokeGroup::A => 1,
            ChokeGroup::B => 2,
            ChokeGroup::C => 3,
            ChokeGroup::D => 4,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            ChokeGroup::Off => "OFF",
            ChokeGroup::A => "A",
            ChokeGroup::B => "B",
            ChokeGroup::C => "C",
            ChokeGroup::D => "D",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Some(match c {
            0 => ChokeGroup::Off,
            1 => ChokeGroup::A,
            2 => ChokeGroup::B,
            3 => ChokeGroup::C,
            4 => ChokeGroup::D,
            _ => return None,
        })
    }
}

/// A pad's pan, −64 (left) ..= 63 (right), 0 the centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pan(i8);

impl Pan {
    pub const CENTRE: Pan = Pan(0);

    pub const fn new(p: i8) -> Option<Pan> {
        if p >= -64 && p <= 63 {
            Some(Pan(p))
        } else {
            None
        }
    }

    /// `p` held to −64..=63.
    pub const fn clamped(p: i8) -> Pan {
        Pan(if p < -64 {
            -64
        } else if p > 63 {
            63
        } else {
            p
        })
    }

    pub const fn get(self) -> i8 {
        self.0
    }
}

/// Semitones from a pad's tuned pitch, −60..=67: MIDI notes 0..=127 over
/// middle C (60), where TONAL tunes its pad (ADR 0270).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Transpose(i8);

impl Transpose {
    pub const ZERO: Transpose = Transpose(0);
    pub const MIN: Transpose = Transpose(-(MIDDLE_C as i8));
    pub const MAX: Transpose = Transpose(127 - MIDDLE_C as i8);

    pub const fn new(s: i8) -> Option<Transpose> {
        if s >= Self::MIN.0 && s <= Self::MAX.0 {
            Some(Transpose(s))
        } else {
            None
        }
    }

    /// `n` over middle C: total, as any MIDI note is in range.
    const fn from_middle_c(n: MidiNote) -> Transpose {
        Transpose(n.get() as i8 - MIDDLE_C as i8)
    }

    pub const fn get(self) -> i8 {
        self.0
    }
}

/// Middle C, where TONAL's pad plays at its tuned pitch.
const MIDDLE_C: u8 = 60;

/// A kit's TONAL (ADR 0270): OFF repeats the kit every octave; a pad
/// plays chromatically from C1 up, tuned at middle C. Codes: 0 OFF, else
/// the pad's number 1..=12 (the `Kit` record's last byte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tonal {
    Off,
    Pad(PadId),
}

impl Tonal {
    pub const fn code(self) -> u8 {
        match self {
            Tonal::Off => 0,
            Tonal::Pad(p) => p.number(),
        }
    }

    pub const fn from_code(c: u8) -> Option<Tonal> {
        match c {
            0 => Some(Tonal::Off),
            _ => match PadId::new(c - 1) {
                Some(p) => Some(Tonal::Pad(p)),
                None => None,
            },
        }
    }
}

/// A pad's compact params and its sound (ADR 0269), copied into the
/// voice at the hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pad {
    machine: PadMachine,
    knobs: KnobSet,
    level: U7,
    pan: Pan,
    choke: ChokeGroup,
    retrig: Retrig,
    sound: PadSound,
}

impl Pad {
    pub const fn new(
        machine: Machine,
        knobs: KnobSet,
        level: U7,
        pan: Pan,
        choke: ChokeGroup,
        retrig: Retrig,
    ) -> Pad {
        Pad {
            machine: PadMachine::Machine(machine),
            knobs,
            level,
            pan,
            choke,
            retrig,
            sound: PadSound::DEFAULT,
        }
    }

    /// The pad with `sound`.
    pub const fn with_sound(self, sound: PadSound) -> Pad {
        Pad { sound, ..self }
    }

    pub const fn sound(&self) -> &PadSound {
        &self.sound
    }

    pub fn sound_mut(&mut self) -> &mut PadSound {
        &mut self.sound
    }

    pub const fn machine(&self) -> PadMachine {
        self.machine
    }

    pub const fn knobs(&self) -> &KnobSet {
        &self.knobs
    }

    pub const fn level(&self) -> U7 {
        self.level
    }

    pub const fn pan(&self) -> Pan {
        self.pan
    }

    pub const fn choke(&self) -> ChokeGroup {
        self.choke
    }

    pub const fn retrig(&self) -> Retrig {
        self.retrig
    }

    pub fn set_machine(&mut self, m: PadMachine) {
        self.machine = m;
    }

    pub fn knobs_mut(&mut self) -> &mut KnobSet {
        &mut self.knobs
    }

    pub fn set_level(&mut self, l: U7) {
        self.level = l;
    }

    pub fn set_pan(&mut self, p: Pan) {
        self.pan = p;
    }

    pub fn set_choke(&mut self, c: ChokeGroup) {
        self.choke = c;
    }

    pub fn set_retrig(&mut self, r: Retrig) {
        self.retrig = r;
    }

    /// The pad as 13 bytes: machine, knobs A–H, level, pan, choke, RETRIG.
    /// The `Kit` record's layout, frozen (ADR 0045), and the kit view's.
    pub fn to_bytes(&self) -> PadBytes {
        let mut b = [0; PAD_BYTES];
        b[0] = self.machine.disk_code();
        for (o, k) in b[1..9].iter_mut().zip(Knob::ALL) {
            *o = self.knobs.get(k).get().get();
        }
        b[9] = self.level.get();
        b[10] = self.pan.get() as u8;
        b[11] = self.choke.disk_code();
        b[12] = self.retrig.disk_code();
        b
    }

    /// A pad from `to_bytes`' bytes, `d` the pad's default: an unknown
    /// machine is `d`'s with its knobs (a newer firmware's machine), an
    /// unknown choke or RETRIG code `d`'s, a byte past its range held to
    /// it; the sound is `d`'s (not in these bytes). Total: any bytes are a
    /// pad.
    pub fn from_bytes(b: PadBytes, d: &Pad) -> Pad {
        let [m, knobs @ .., level, pan, choke, retrig] = b;
        let u7 = |v: u8| U7::new(v).unwrap_or(U7::MAX);
        let (machine, knobs) = match PadMachine::from_disk_code(m) {
            Some(m) => (m, KnobSet::new(knobs.map(|k| KnobValue::new(u7(k))))),
            None => (d.machine, d.knobs),
        };
        Pad {
            machine,
            knobs,
            level: u7(level),
            pan: Pan::clamped(pan as i8),
            choke: ChokeGroup::from_disk_code(choke).unwrap_or(d.choke),
            retrig: Retrig::from_disk_code(retrig).unwrap_or(d.retrig),
            sound: d.sound,
        }
    }
}

/// A Part's kit: one `Pad` per `PadId`, and its TONAL (ADR 0270).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kit {
    pads: [Pad; PADS as usize],
    tonal: Tonal,
}

impl Kit {
    /// DRUM INIT's kit: the Rytm's layout on slice 1's machines, each at
    /// its own defaults, CUT on its own re-strike (2026-11-10 ruling 9).
    /// Pads 1–12: KICK, SNARE, IMPULSE (the rim's stand-in), CLAP, three
    /// TOMs (low, mid, high by PITCH), HAT closed (short DECAY) and open,
    /// CYMBAL, COWBELL, SHAKER. Chokes as the Rytm's pairs: A 3+4, B 6+7,
    /// C 8+9, D 10+11.
    pub const DEFAULT: Kit = {
        let mut pads = [Kit::init_pad(PadId(0)); PADS as usize];
        let mut i = 1;
        while i < PADS {
            pads[i as usize] = Kit::init_pad(PadId(i));
            i += 1;
        }
        Kit {
            pads,
            tonal: Tonal::Off,
        }
    };

    /// `DEFAULT`'s pad `p`, built from its row: one machine, choke and
    /// knob a pad, not `DEFAULT`'s 80 B pads copied.
    const fn init_pad(p: PadId) -> Pad {
        use ChokeGroup::{A, B, C, D, Off};
        use Machine::*;
        // TOM's PITCH 50 (113 Hz) the mid, 30 and 70 a fourth-odd either
        // side; HAT's DECAY 20 (40 ms) closed, 80 (0.3 s) open.
        let (m, choke, knob) = match p.0 {
            0 => (ToneKick, Off, None),
            1 => (ToneSnare, Off, None),
            2 => (PlainImpulse, A, None),
            3 => (ToneClap, A, None),
            4 => (ToneTom, Off, Some((Knob::A, 30))),
            5 => (ToneTom, B, Some((Knob::A, 50))),
            6 => (ToneTom, B, Some((Knob::A, 70))),
            7 => (MetalHat, C, Some((Knob::B, 20))),
            8 => (MetalHat, C, Some((Knob::B, 80))),
            9 => (ToneCymbal, D, None),
            10 => (ToneCowbell, D, None),
            _ => (BodyShaker, Off, None),
        };
        let mut knobs = KnobSet::defaults(m);
        if let Some((k, v)) = knob {
            knobs.set(
                k,
                KnobValue(match U7::new(v) {
                    Some(u) => u,
                    None => panic!("a knob position is 0..=127"),
                }),
            );
        }
        Pad::new(m, knobs, U7::MAX, Pan::CENTRE, choke, Retrig::Cut)
    }

    /// `DEFAULT`'s pad `p`, built at run time: the firmware keeps no
    /// copy of `DEFAULT` (964 B) in flash.
    #[inline(never)]
    #[optimize(size)]
    pub fn default_pad(p: PadId) -> Pad {
        Kit::init_pad(p)
    }

    /// Back to `DEFAULT`, in place (`default_pad`).
    #[optimize(size)]
    pub fn reset(&mut self) {
        for p in PadId::ALL {
            self.pads[p.index()] = Kit::default_pad(p);
        }
        self.tonal = Tonal::Off;
    }

    /// `DEFAULT`, built at run time (`default_pad`).
    #[optimize(size)]
    pub fn init() -> Kit {
        Kit {
            pads: PadId::ALL.map(Kit::default_pad),
            tonal: Tonal::Off,
        }
    }

    pub const fn pad(&self, p: PadId) -> &Pad {
        &self.pads[p.index()]
    }

    pub fn pad_mut(&mut self, p: PadId) -> &mut Pad {
        &mut self.pads[p.index()]
    }

    pub const fn tonal(&self) -> Tonal {
        self.tonal
    }

    pub fn set_tonal(&mut self, t: Tonal) {
        self.tonal = t;
    }
}

/// What a note means to a kit Part (ADR 0270): with TONAL OFF the kit
/// repeats every octave, each pad at its tuned pitch; with TONAL on a
/// pad, C0–B0 (notes 0–11) strike the kit and C1 up play that pad, at its
/// tuned pitch on middle C.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KitNote {
    Pad(PadId),
    Tonal(PadId, Transpose),
}

impl KitNote {
    pub const fn new(n: MidiNote, tonal: Tonal) -> KitNote {
        match tonal {
            Tonal::Pad(p) if n.get() >= PADS => KitNote::Tonal(p, Transpose::from_middle_c(n)),
            _ => KitNote::Pad(PadId(n.get() % PADS)),
        }
    }

    /// The pad struck and how far from its tuned pitch.
    pub const fn pad(self) -> (PadId, Transpose) {
        match self {
            KitNote::Pad(p) => (p, Transpose::ZERO),
            KitNote::Tonal(p, t) => (p, t),
        }
    }
}

/// A struck pad, as a voice takes it: which pad, its params at the hit,
/// and how far from its tuned pitch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumHit {
    pad: PadId,
    machine: Machine,
    params: Pad,
    pitch: Transpose,
}

impl DrumHit {
    /// `params` struck on `pad`; `None` on a NONE pad, which plays no hit
    /// (ADR 0318).
    pub const fn new(pad: PadId, params: Pad, pitch: Transpose) -> Option<DrumHit> {
        match params.machine {
            PadMachine::Machine(machine) => Some(DrumHit {
                pad,
                machine,
                params,
                pitch,
            }),
            PadMachine::None => None,
        }
    }

    /// The machine it plays: its pad's at the hit.
    pub const fn machine(&self) -> Machine {
        self.machine
    }

    pub const fn pad(&self) -> PadId {
        self.pad
    }

    pub const fn params(&self) -> Pad {
        self.params
    }

    pub const fn pitch(&self) -> Transpose {
        self.pitch
    }

    /// Its pad's sound, its choices as struck, borrowed.
    pub const fn sound(&self) -> &super::sound::PadSound {
        self.params.sound()
    }

    /// Its pad's sound, as a ringing hit follows it (`DrumEngine::follow`).
    pub(crate) fn sound_mut(&mut self) -> &mut super::sound::PadSound {
        self.params.sound_mut()
    }
}

/// `dsp`'s, lifted for the Parts' EQ (ADR 0284).
pub use crate::dsp::Hz;

/// A time in seconds, 0..=60; NaN is 0. A decay's T60, a sweep's tau.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Seconds(f32);

impl Seconds {
    pub const fn new(v: f32) -> Seconds {
        Seconds(clampf(v, 0.0, 60.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A frequency or time ratio, 0..=16; NaN is 0.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Ratio(f32);

impl Ratio {
    pub const fn new(v: f32) -> Ratio {
        Ratio(clampf(v, 0.0, 16.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A linear level factor, 0..=16; NaN is 0.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Gain(f32);

impl Gain {
    pub const fn new(v: f32) -> Gain {
        Gain(clampf(v, 0.0, 16.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A pitch offset in fractional semitones, −48..=48; NaN is −48.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Semis(f32);

impl Semis {
    pub const fn new(v: f32) -> Semis {
        Semis(clampf(v, -48.0, 48.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A rate of events a second (grains, jingles), 0..=48 000; NaN is 0.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct PerSecond(f32);

impl PerSecond {
    pub const fn new(v: f32) -> PerSecond {
        PerSecond(clampf(v, 0.0, 48_000.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

/// The rate a kernel runs at, in Hz; never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRate(u32);

impl SampleRate {
    /// The DACs' rate.
    pub const HW: SampleRate = SampleRate(crate::hw::SAMPLE_RATE);

    pub const fn new(hz: u32) -> Option<SampleRate> {
        if hz > 0 { Some(SampleRate(hz)) } else { None }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub fn hz(self) -> f32 {
        self.0 as f32
    }
}

/// A hit's noise and jitter seed: a new one each hit, as the prototypes
/// take theirs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitSeed(u32);

impl HitSeed {
    pub const fn new(s: u32) -> HitSeed {
        HitSeed(s)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// The next hit's seed: one step of the prototypes' LCG.
    pub const fn next(self) -> HitSeed {
        HitSeed(self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223))
    }
}
