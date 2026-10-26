//! Layer 1: bytes to MIDI events. Pure, no allocation, never panics.

use crate::value::{Bend, Channel, Key, U7, U14, Velocity};

/// A control-change number that is not Channel Mode: 0..=119.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ctrl(u8);

impl Ctrl {
    pub const fn new(n: u8) -> Option<Self> {
        if n <= 119 { Some(Self(n)) } else { None }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    NoteOff {
        key: Key,
        vel: U7,
    },
    /// `9n kk 00` is returned as `NoteOff`.
    NoteOn {
        key: Key,
        vel: Velocity,
    },
    PolyPressure {
        key: Key,
        value: U7,
    },
    Control {
        cc: Ctrl,
        value: U7,
    },
    Program(U7),
    ChannelPressure(U7),
    /// LSB first, centred on 0x2000.
    PitchBend(Bend),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelMode {
    AllSoundOff,
    ResetAllControllers,
    LocalControl(bool),
    AllNotesOff,
    OmniOff,
    OmniOn,
    MonoOn(U7),
    PolyOn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Common {
    TimeCode(U7),
    SongPosition(U14),
    SongSelect(U7),
    TuneRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealTime {
    Clock,
    Start,
    Continue,
    Stop,
    ActiveSensing,
    Reset,
}

/// A complete SysEx body between F0 and F7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SysEx<'a>(&'a [u8]);

impl<'a> SysEx<'a> {
    pub const fn bytes(&self) -> &'a [u8] {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A data byte with no running status.
    Orphan,
    /// A status byte cut a channel or common message short.
    Interrupted,
    /// A non-real-time status ended a SysEx without F7.
    SysExCut,
    /// Longer than the buffer: the rest is ignored until F7 or a status.
    SysExOverflow,
    /// F4, F5 (they also cancel running status), F9, FD.
    Undefined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    Voice(Channel, Voice),
    Mode(Channel, ChannelMode),
    Common(Common),
    SysEx(SysEx<'a>),
    Fault(Fault),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parsed<'a> {
    RealTime(RealTime),
    Event(Event<'a>),
}

/// Per-kind fault tallies; each saturates at `u16::MAX`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FaultCounts {
    pub orphan: u16,
    pub interrupted: u16,
    pub sysex_cut: u16,
    pub sysex_overflow: u16,
    pub undefined: u16,
}

impl FaultCounts {
    fn bump(&mut self, f: Fault) {
        let c = match f {
            Fault::Orphan => &mut self.orphan,
            Fault::Interrupted => &mut self.interrupted,
            Fault::SysExCut => &mut self.sysex_cut,
            Fault::SysExOverflow => &mut self.sysex_overflow,
            Fault::Undefined => &mut self.undefined,
        };
        *c = c.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    NoteOff,
    NoteOn,
    Poly,
    Control,
    Program,
    ChanPress,
    Bend,
}

impl Kind {
    const fn two_bytes(self) -> bool {
        !matches!(self, Kind::Program | Kind::ChanPress)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VoiceStatus {
    kind: Kind,
    ch: Channel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommonKind {
    TimeCode,
    SongPosition,
    SongSelect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// No running status.
    Idle,
    /// `first` is held only for two-byte kinds.
    Running {
        status: VoiceStatus,
        first: Option<U7>,
    },
    /// No running status after it.
    Common {
        kind: CommonKind,
        first: Option<U7>,
    },
    SysEx {
        len: usize,
        over: bool,
    },
}

pub struct Parser<const SYSEX: usize> {
    state: State,
    sysex: [u8; SYSEX],
    faults: FaultCounts,
}

impl<const N: usize> Default for Parser<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// What a byte produced, before it is tied to the parser's buffer.
enum Out {
    None,
    Rt(RealTime),
    Ev(Event<'static>),
    SysEx(usize),
}

const fn mode_of(n: u8, v: U7) -> Option<ChannelMode> {
    Some(match n {
        120 => ChannelMode::AllSoundOff,
        121 => ChannelMode::ResetAllControllers,
        122 => ChannelMode::LocalControl(v.get() >= 64),
        123 => ChannelMode::AllNotesOff,
        124 => ChannelMode::OmniOff,
        125 => ChannelMode::OmniOn,
        126 => ChannelMode::MonoOn(v),
        127 => ChannelMode::PolyOn,
        _ => return None,
    })
}

fn voice_event(s: VoiceStatus, a: U7, b: U7) -> Event<'static> {
    // `a` is the first data byte, `b` the second (ignored by one-byte kinds).
    let key = Key::clamped(a.get());
    let v = match s.kind {
        Kind::NoteOff => Voice::NoteOff { key, vel: b },
        Kind::NoteOn => match Velocity::new(b.get()) {
            Some(vel) => Voice::NoteOn { key, vel },
            None => Voice::NoteOff { key, vel: b },
        },
        Kind::Poly => Voice::PolyPressure { key, value: b },
        Kind::Control => match Ctrl::new(a.get()) {
            Some(cc) => Voice::Control { cc, value: b },
            None => {
                return match mode_of(a.get(), b) {
                    Some(m) => Event::Mode(s.ch, m),
                    None => Event::Fault(Fault::Undefined),
                };
            }
        },
        Kind::Program => Voice::Program(a),
        Kind::ChanPress => Voice::ChannelPressure(a),
        Kind::Bend => Voice::PitchBend(Bend::from_u14(U14::from_parts(b, a))),
    };
    Event::Voice(s.ch, v)
}

impl<const N: usize> Parser<N> {
    pub const fn new() -> Self {
        Self {
            state: State::Idle,
            sysex: [0; N],
            faults: FaultCounts {
                orphan: 0,
                interrupted: 0,
                sysex_cut: 0,
                sysex_overflow: 0,
                undefined: 0,
            },
        }
    }

    pub fn faults(&self) -> FaultCounts {
        self.faults
    }

    fn fault(&mut self, f: Fault) -> Out {
        self.faults.bump(f);
        Out::Ev(Event::Fault(f))
    }

    /// Feed one byte. Real-time bytes are returned at once and leave the state alone.
    pub fn push(&mut self, byte: u8) -> Option<Parsed<'_>> {
        let out = if byte >= 0xF8 {
            self.realtime(byte)
        } else if byte >= 0x80 {
            self.status(byte)
        } else {
            self.data(byte)
        };
        match out {
            Out::None => None,
            Out::Rt(r) => Some(Parsed::RealTime(r)),
            Out::Ev(e) => Some(Parsed::Event(e)),
            Out::SysEx(len) => Some(Parsed::Event(Event::SysEx(SysEx(&self.sysex[..len])))),
        }
    }

    fn realtime(&mut self, byte: u8) -> Out {
        match byte {
            0xF8 => Out::Rt(RealTime::Clock),
            0xFA => Out::Rt(RealTime::Start),
            0xFB => Out::Rt(RealTime::Continue),
            0xFC => Out::Rt(RealTime::Stop),
            0xFE => Out::Rt(RealTime::ActiveSensing),
            0xFF => Out::Rt(RealTime::Reset),
            _ => self.fault(Fault::Undefined), // F9, FD
        }
    }

    fn status(&mut self, byte: u8) -> Out {
        // What this byte breaks, if anything.
        let broke = match self.state {
            State::SysEx { over, .. } if byte != 0xF7 => (!over).then_some(Fault::SysExCut),
            State::Running { first: Some(_), .. } | State::Common { first: Some(_), .. } => {
                Some(Fault::Interrupted)
            }
            State::Common { first: None, .. } => Some(Fault::Interrupted),
            _ => None,
        };
        let ended = if let State::SysEx { len, over } = self.state {
            (byte == 0xF7).then_some((len, over))
        } else {
            None
        };
        if let Some(f) = broke {
            self.faults.bump(f);
        }
        let lead = broke.map(|f| Out::Ev(Event::Fault(f))).unwrap_or(Out::None);

        if byte < 0xF0 {
            let ch = Channel::clamped(byte & 15);
            let kind = match byte >> 4 {
                0x8 => Kind::NoteOff,
                0x9 => Kind::NoteOn,
                0xA => Kind::Poly,
                0xB => Kind::Control,
                0xC => Kind::Program,
                0xD => Kind::ChanPress,
                _ => Kind::Bend,
            };
            self.state = State::Running {
                status: VoiceStatus { kind, ch },
                first: None,
            };
            return lead;
        }
        self.state = State::Idle;
        let own = match byte {
            0xF0 => {
                self.state = State::SysEx {
                    len: 0,
                    over: false,
                };
                Out::None
            }
            0xF1 => self.begin(CommonKind::TimeCode),
            0xF2 => self.begin(CommonKind::SongPosition),
            0xF3 => self.begin(CommonKind::SongSelect),
            0xF6 => Out::Ev(Event::Common(Common::TuneRequest)),
            0xF7 => match ended {
                Some((len, false)) => Out::SysEx(len),
                _ => Out::None, // stray, or the tail of an overflowed SysEx
            },
            _ => {
                // F4, F5
                self.faults.bump(Fault::Undefined);
                Out::Ev(Event::Fault(Fault::Undefined))
            }
        };
        // A complete message wins over the fault that came with it (counted only).
        match (own, lead) {
            (Out::None, lead) => lead,
            (Out::Ev(Event::Fault(_)), lead @ Out::Ev(_)) => lead,
            (own, _) => own,
        }
    }

    fn begin(&mut self, kind: CommonKind) -> Out {
        self.state = State::Common { kind, first: None };
        Out::None
    }

    fn data(&mut self, byte: u8) -> Out {
        let b = U7::new(byte).unwrap_or(U7::MAX);
        match self.state {
            State::Idle => self.fault(Fault::Orphan),
            State::Running { status, first } => {
                if !status.kind.two_bytes() {
                    return Out::Ev(voice_event(status, b, b));
                }
                match first {
                    None => {
                        self.state = State::Running {
                            status,
                            first: Some(b),
                        };
                        Out::None
                    }
                    Some(a) => {
                        self.state = State::Running {
                            status,
                            first: None,
                        };
                        Out::Ev(voice_event(status, a, b))
                    }
                }
            }
            State::Common { kind, first } => match (kind, first) {
                (CommonKind::SongPosition, None) => {
                    self.state = State::Common {
                        kind,
                        first: Some(b),
                    };
                    Out::None
                }
                (CommonKind::SongPosition, Some(lsb)) => {
                    self.state = State::Idle;
                    Out::Ev(Event::Common(Common::SongPosition(U14::from_parts(b, lsb))))
                }
                (CommonKind::TimeCode, _) => {
                    self.state = State::Idle;
                    Out::Ev(Event::Common(Common::TimeCode(b)))
                }
                (CommonKind::SongSelect, _) => {
                    self.state = State::Idle;
                    Out::Ev(Event::Common(Common::SongSelect(b)))
                }
            },
            State::SysEx { len, over } => {
                if over {
                    Out::None
                } else if len == N {
                    self.state = State::SysEx { len, over: true };
                    self.fault(Fault::SysExOverflow)
                } else {
                    self.sysex[len] = byte;
                    self.state = State::SysEx { len: len + 1, over };
                    Out::None
                }
            }
        }
    }
}
