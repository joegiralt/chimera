//! DRUM's types (ADR 0265): every index, value, code and mode a newtype or
//! an enum, built clamped or as an `Option`, never by a panic.

use chimera_midi::value::U7;

use super::sound::PadSound;
use crate::MidiNote;
use crate::block::DiskCode;
use crate::dsp::tape_engine::params::Retrig;
use crate::dsp::{Unit, clampf};

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
}

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
    machine: Machine,
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
            machine,
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

    pub const fn machine(&self) -> Machine {
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

    pub fn set_machine(&mut self, m: Machine) {
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
        let (machine, knobs) = match Machine::from_disk_code(m) {
            Some(m) => (m, KnobSet::new(knobs.map(|k| KnobValue::new(u7(k))))),
            None => (d.machine, d.knobs),
        };
        Pad::new(
            machine,
            knobs,
            u7(level),
            Pan::clamped(pan as i8),
            ChokeGroup::from_disk_code(choke).unwrap_or(d.choke),
            Retrig::from_disk_code(retrig).unwrap_or(d.retrig),
        )
        .with_sound(d.sound)
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
        use ChokeGroup::{A, B, C, D, Off};
        use Machine::*;
        const fn at(m: Machine, choke: ChokeGroup, knob: Option<(Knob, u8)>) -> Pad {
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
        // TOM's PITCH 50 (113 Hz) the mid, 30 and 70 a fourth-odd either
        // side; HAT's DECAY 20 (40 ms) closed, 80 (0.3 s) open.
        Kit {
            pads: [
                at(ToneKick, Off, None),
                at(ToneSnare, Off, None),
                at(PlainImpulse, A, None),
                at(ToneClap, A, None),
                at(ToneTom, Off, Some((Knob::A, 30))),
                at(ToneTom, B, Some((Knob::A, 50))),
                at(ToneTom, B, Some((Knob::A, 70))),
                at(MetalHat, C, Some((Knob::B, 20))),
                at(MetalHat, C, Some((Knob::B, 80))),
                at(ToneCymbal, D, None),
                at(ToneCowbell, D, None),
                at(BodyShaker, Off, None),
            ],
            tonal: Tonal::Off,
        }
    };

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
    params: Pad,
    pitch: Transpose,
}

impl DrumHit {
    pub const fn new(pad: PadId, params: Pad, pitch: Transpose) -> DrumHit {
        DrumHit { pad, params, pitch }
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
}

/// A frequency in Hz, 0..=24 000 (the rate's Nyquist); NaN is 0.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Hz(f32);

impl Hz {
    pub const fn new(v: f32) -> Hz {
        Hz(clampf(v, 0.0, 24_000.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

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
