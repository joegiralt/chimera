//! Layer 2: controller meaning. RPN/NRPN selection, data entry, step,
//! bank latch, per channel. Pure, no allocation, never panics.

use crate::stream::Voice;
use crate::value::{Channel, U7, U14};

/// The active register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sel {
    Rpn(U14),
    Nrpn(U14),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataValue {
    /// CC 6 alone.
    Coarse(U7),
    /// CC 38: `msb << 7 | lsb`.
    Fine(U14),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

/// A CC layer 2 passes through: 0..=119 minus 0, 6, 32, 38, 64 and 96..=101.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FreeCc(u8);

impl FreeCc {
    pub const fn new(n: u8) -> Option<Self> {
        match n {
            0 | 6 | 32 | 38 | 64 | 96..=101 | 120..=255 => None,
            _ => Some(Self(n)),
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctl {
    Cc {
        cc: FreeCc,
        value: U7,
    },
    Data {
        sel: Sel,
        value: DataValue,
    },
    /// CC 96 / 97: one step, the data byte ignored.
    Step {
        sel: Sel,
        dir: Dir,
    },
    Program {
        bank: Option<U14>,
        program: U7,
    },
    Pressure(U7),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heard {
    /// Hand the `Voice` to the note path untouched.
    Note,
    Ctl(Ctl),
    Consumed,
}

/// CC 0 / CC 32 halves seen since the last Program Change.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct BankLatch {
    msb: Option<U7>,
    lsb: Option<U7>,
}

impl BankLatch {
    /// Either half alone is accepted; the missing one is 0.
    fn take(&mut self) -> Option<U14> {
        let l = core::mem::take(self);
        if l.msb.is_none() && l.lsb.is_none() {
            return None;
        }
        Some(U14::from_parts(
            l.msb.unwrap_or(U7::ZERO),
            l.lsb.unwrap_or(U7::ZERO),
        ))
    }
}

#[derive(Clone, Copy, Debug)]
struct ChannelCtl {
    rpn: (U7, U7),
    nrpn: (U7, U7),
    sel: Option<Sel>,
    msb: Option<U7>,
    bank: BankLatch,
}

const NULL: (U7, U7) = (U7::MAX, U7::MAX);

impl ChannelCtl {
    const fn new() -> Self {
        Self {
            rpn: NULL,
            nrpn: NULL,
            sel: None,
            msb: None,
            bank: BankLatch {
                msb: None,
                lsb: None,
            },
        }
    }

    fn select(&mut self, nrpn: bool) {
        self.msb = None;
        let (r, make): ((U7, U7), fn(U14) -> Sel) = if nrpn {
            (self.nrpn, Sel::Nrpn)
        } else {
            (self.rpn, Sel::Rpn)
        };
        self.sel = (r != NULL).then(|| make(U14::from_parts(r.0, r.1)));
    }

    fn control(&mut self, n: u8, v: U7) -> Heard {
        match n {
            0 => self.bank.msb = Some(v),
            32 => self.bank.lsb = Some(v),
            98 => {
                self.nrpn.1 = v;
                self.select(true);
            }
            99 => {
                self.nrpn.0 = v;
                self.select(true);
            }
            100 => {
                self.rpn.1 = v;
                self.select(false);
            }
            101 => {
                self.rpn.0 = v;
                self.select(false);
            }
            6 | 38 => {
                let Some(sel) = self.sel else {
                    return Heard::Consumed;
                };
                let value = if n == 6 {
                    self.msb = Some(v);
                    DataValue::Coarse(v)
                } else {
                    DataValue::Fine(U14::from_parts(self.msb.unwrap_or(U7::ZERO), v))
                };
                return Heard::Ctl(Ctl::Data { sel, value });
            }
            96 | 97 => {
                let Some(sel) = self.sel else {
                    return Heard::Consumed;
                };
                let dir = if n == 96 { Dir::Up } else { Dir::Down };
                return Heard::Ctl(Ctl::Step { sel, dir });
            }
            64 => return Heard::Note,
            _ => {
                return match FreeCc::new(n) {
                    Some(cc) => Heard::Ctl(Ctl::Cc { cc, value: v }),
                    None => Heard::Consumed,
                };
            }
        }
        Heard::Consumed
    }
}

/// One source's controller state: 16 channels.
pub struct Controllers {
    ch: [ChannelCtl; 16],
}

impl Default for Controllers {
    fn default() -> Self {
        Self::new()
    }
}

impl Controllers {
    pub const fn new() -> Self {
        Self {
            ch: [ChannelCtl::new(); 16],
        }
    }

    pub fn hear(&mut self, ch: Channel, v: Voice) -> Heard {
        let c = &mut self.ch[usize::from(ch.get() & 15)];
        match v {
            Voice::Control { cc, value } => c.control(cc.get(), value),
            Voice::Program(program) => Heard::Ctl(Ctl::Program {
                bank: c.bank.take(),
                program,
            }),
            Voice::ChannelPressure(p) => Heard::Ctl(Ctl::Pressure(p)),
            Voice::NoteOff { .. }
            | Voice::NoteOn { .. }
            | Voice::PolyPressure { .. }
            | Voice::PitchBend(_) => Heard::Note,
        }
    }
}
