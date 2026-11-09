//! MPE zones (ADR 0252): a lower zone, master channel 1 and its members
//! from channel 2 up, and an upper zone, master 16 and its members down
//! from 15. A zone's member channels play the Parts on its master channel,
//! each note's bend, pressure and timbre its own. MIDI CONFIG › MPE sets
//! them, and so does an MPE Configuration Message (RPN 6 on a master).
//!
//! Bank 2's (`bank2.x` links the module), behind `MidiDoor`: bank 1 keeps
//! the inlined accessors and builders SETTINGS' cells write through, and
//! the door stubs (`master_of`, `own`). Closed, MIDI in is deaf, so no
//! note comes on a member channel.

use crate::MidiChannel;
use crate::dsp::voice::Own;
use crate::hw::MAX_VOICES;

/// The most member channels a zone has: every channel but its master.
pub const MAX_MEMBERS: u8 = 15;
/// The most a member channel bends, semitones (the MPE spec's).
pub const MAX_ZONE_BEND: u8 = 96;
/// A zone's member bend after a configuration message: ±48, the spec's.
pub const DEFAULT_ZONE_BEND: u8 = 48;

/// Both zones in a word, so `MidiSettings` copies and compares in words:
/// bits 0–3 the lower zone's member count (0 the zone is off), 4–7 the
/// upper's, 8–14 and 15–21 their members' bend in semitones, each XOR
/// `DEFAULT_ZONE_BEND`, so 0 is both zones off at the spec's ±48.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Zones(u32);

/// The two masters, channels 1 and 16.
pub const LOWER_MASTER: MidiChannel = MidiChannel::clamped(0);
pub const UPPER_MASTER: MidiChannel = MidiChannel::clamped(15);

const D: u32 = DEFAULT_ZONE_BEND as u32;

impl Zones {
    pub const OFF: Zones = Zones(0);

    #[inline(always)]
    pub const fn lower(self) -> u8 {
        (self.0 & 15) as u8
    }

    #[inline(always)]
    pub const fn upper(self) -> u8 {
        (self.0 >> 4 & 15) as u8
    }

    #[inline(always)]
    pub const fn lower_bend(self) -> u8 {
        ((self.0 >> 8 & 127) ^ D) as u8
    }

    #[inline(always)]
    pub const fn upper_bend(self) -> u8 {
        ((self.0 >> 15 & 127) ^ D) as u8
    }

    /// `lower` and `upper` members (each at most 15, not checked against
    /// each other: `with_lower` and `with_upper` keep them apart) and their
    /// bends (at most 96).
    #[inline(always)]
    pub const fn new(lower: u8, upper: u8, lower_bend: u8, upper_bend: u8) -> Zones {
        const fn m(v: u8, max: u8) -> u32 {
            (if v > max { max } else { v }) as u32
        }
        Zones(
            m(lower, MAX_MEMBERS)
                | m(upper, MAX_MEMBERS) << 4
                | (m(lower_bend, MAX_ZONE_BEND) ^ D) << 8
                | (m(upper_bend, MAX_ZONE_BEND) ^ D) << 15,
        )
    }

    /// The lower zone set to `n` members, the upper shrunk out of its way
    /// (the spec's rule: zones never overlap; 15 leaves no upper zone).
    pub fn with_lower(self, n: u8) -> Zones {
        let lower = n.min(MAX_MEMBERS);
        let upper = self.upper().min((MAX_MEMBERS - 1).saturating_sub(lower));
        Zones::new(lower, upper, self.lower_bend(), self.upper_bend())
    }

    /// `with_lower`'s mirror.
    pub fn with_upper(self, n: u8) -> Zones {
        let upper = n.min(MAX_MEMBERS);
        let lower = self.lower().min((MAX_MEMBERS - 1).saturating_sub(upper));
        Zones::new(lower, upper, self.lower_bend(), self.upper_bend())
    }

    pub fn with_lower_bend(self, b: u8) -> Zones {
        Zones::new(self.lower(), self.upper(), b, self.upper_bend())
    }

    pub fn with_upper_bend(self, b: u8) -> Zones {
        Zones::new(self.lower(), self.upper(), self.lower_bend(), b)
    }

    /// The most a note of a Part with `mix` bends up, semitones: its BEND,
    /// and on a zone's master its members' range on top (a high note's
    /// bill, `HighNote`).
    #[inline(always)]
    pub fn reach(self, mix: &crate::part::PartParams) -> u8 {
        let members = if mix.channel == LOWER_MASTER && self.lower() > 0 {
            self.lower_bend()
        } else if mix.channel == UPPER_MASTER && self.upper() > 0 {
            self.upper_bend()
        } else {
            0
        };
        mix.bend.saturating_add(members)
    }

    /// `ch`'s zone if it is a member channel: its master and bend range.
    /// Inlined into each caller (all bank 2's): out of line, the machine
    /// outliner shares a call to it from bank 1.
    #[inline(always)]
    pub fn member(self, ch: MidiChannel) -> Option<(MidiChannel, u8)> {
        let c = ch.get();
        if c >= 1 && c <= self.lower() {
            Some((LOWER_MASTER, self.lower_bend()))
        } else if c < 15 && c + self.upper() >= 15 {
            Some((UPPER_MASTER, self.upper_bend()))
        } else {
            None
        }
    }

    /// The channel whose Parts play a note on `ch`: its zone's master for
    /// a member channel, else `ch`.
    pub fn master_of(self, ch: MidiChannel) -> MidiChannel {
        self.member(ch).map_or(ch, |(m, _)| m)
    }

    /// An MPE Configuration Message, `n` members, on `master`: that zone
    /// takes them and the spec's ±48 member bend; any other channel's
    /// changes nothing.
    pub fn configured(self, master: MidiChannel, n: u8) -> Zones {
        if master == LOWER_MASTER {
            self.with_lower(n).with_lower_bend(DEFAULT_ZONE_BEND)
        } else if master == UPPER_MASTER {
            self.with_upper(n).with_upper_bend(DEFAULT_ZONE_BEND)
        } else {
            self
        }
    }

    /// `ch`'s zone's member bend set to `semis` (RPN 0 on a member
    /// channel); a channel in no zone changes nothing.
    pub fn with_bend_on(self, ch: MidiChannel, semis: u8) -> Zones {
        match self.member(ch) {
            Some((m, _)) if m == LOWER_MASTER => self.with_lower_bend(semis),
            Some(_) => self.with_upper_bend(semis),
            None => self,
        }
    }
}

impl Default for Zones {
    fn default() -> Self {
        Self::OFF
    }
}

impl core::fmt::Debug for Zones {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Zones {{ lower: {}, upper: {}, lower_bend: {}, upper_bend: {} }}",
            self.lower(),
            self.upper(),
            self.lower_bend(),
            self.upper_bend()
        )
    }
}

/// What each voice's own expression is read from (`own`): the Instrument's
/// per-channel and per-voice state.
pub struct OwnIn<'a> {
    pub zones: Zones,
    /// The channel each voice's note came on.
    pub channel: &'a [MidiChannel; MAX_VOICES],
    /// Who struck it: ORBIT's notes have no expression.
    pub origin: &'a [crate::instrument::NoteOrigin; MAX_VOICES],
    /// Each channel's wheel, -8192..=8191.
    pub wheel: &'a [i16; 16],
    /// Each channel's mod wheel, pressure and timbre, 0..=127.
    pub expr: &'a [[u8; 3]; 16],
    /// Each voice's key's poly pressure, 0..=127.
    pub poly: &'a [u8; MAX_VOICES],
}

/// A call through the door's one MPE entry (`MidiDoor::mpe`): one entry,
/// so the door's table keeps its room.
pub enum Call<'a> {
    /// `kern::master` into the channel.
    Master(Zones, &'a mut MidiChannel),
    /// `kern::own`.
    Own(&'a OwnIn<'a>, &'a mut [Own; MAX_VOICES]),
    /// `kern::write` on the zones.
    Write(&'a mut Zones, u8, u8),
}

/// The door's entries, through `kern::call`.
pub mod kern {
    use super::*;

    /// The door's one MPE entry.
    pub fn call(c: Call<'_>) {
        match c {
            Call::Master(z, ch) => *ch = master(z, *ch),
            Call::Own(i, out) => own(i, out),
            Call::Write(z, which, v) => *z = write(*z, which, v),
        }
    }

    /// `Zones::master_of` for a live note.
    pub fn master(z: Zones, ch: MidiChannel) -> MidiChannel {
        z.master_of(ch)
    }

    /// MIDI CONFIG › MPE's cell `which` (LOWER, L BEND, UPPER, U BEND) set
    /// to `v`: a zone grown into the other shrinks it.
    pub fn write(z: Zones, which: u8, v: u8) -> Zones {
        match which {
            0 => z.with_lower(v),
            1 => z.with_lower_bend(v),
            2 => z.with_upper(v),
            _ => z.with_upper_bend(v),
        }
    }

    /// Each voice's own expression targets: its key's poly pressure, and a
    /// live note on an MPE member channel that channel's bend over its
    /// zone's range, pressure (the higher) and timbre.
    pub fn own(i: &OwnIn<'_>, out: &mut [Own; MAX_VOICES]) {
        for v in 0..MAX_VOICES {
            out[v] = poly_only(i.poly[v]);
            let ch = i.channel[v];
            let live = i.origin[v] == crate::instrument::NoteOrigin::Live;
            let Some((_, range)) = i.zones.member(ch).filter(|_| live) else {
                continue;
            };
            let c = usize::from(ch.get()) & 15;
            let [_, press, timbre] = i.expr[c];
            let w = i.wheel[c];
            let unit = if w > 0 {
                f32::from(w) / 8191.0
            } else {
                f32::from(w) / 8192.0
            };
            out[v] = Own {
                bend: unit * f32::from(range),
                press: out[v].press.max(f32::from(press) / 127.0),
                timbre: f32::from(timbre) / 127.0,
            };
        }
    }
}

/// A voice's own expression with no MPE: its key's poly pressure.
#[inline(always)]
pub fn poly_only(poly: u8) -> Own {
    Own {
        bend: 0.0,
        press: f32::from(poly) / 127.0,
        timbre: 0.0,
    }
}

/// One call through bank 2's door with `bank2`; `false` closed (nothing
/// done).
#[inline(always)]
fn door(c: Call<'_>) -> bool {
    #[cfg(feature = "bank2")]
    return crate::bank2::DOOR_CELL.with(|d| (d.midi.mpe)(c)).is_some();
    #[cfg(not(feature = "bank2"))]
    {
        kern::call(c);
        true
    }
}

/// The channel whose Parts play a live note on `ch` (`kern::master`):
/// closed, `ch`.
#[inline(always)]
pub fn master_of(z: Zones, ch: MidiChannel) -> MidiChannel {
    let mut m = ch;
    door(Call::Master(z, &mut m));
    m
}

/// Each voice's own expression (`kern::own`) into `out`: closed, `false`
/// and `out` untouched (every voice its poly pressure alone, `poly_only`).
#[inline(always)]
pub fn own(i: &OwnIn<'_>, out: &mut [Own; MAX_VOICES]) -> bool {
    door(Call::Own(i, out))
}

/// MIDI CONFIG › MPE's cell `which` written (`kern::write`): closed, the
/// value stored as given, no shrink rule, so a SYSTEM loaded and saved
/// with the door closed keeps its zones (as #815's maps).
#[inline(always)]
pub fn write(z: Zones, which: u8, v: u8) -> Zones {
    let mut out = z;
    if !door(Call::Write(&mut out, which, v)) {
        let (l, u, lb, ub) = (z.lower(), z.upper(), z.lower_bend(), z.upper_bend());
        out = match which {
            0 => Zones::new(v, u, lb, ub),
            1 => Zones::new(l, u, v, ub),
            2 => Zones::new(l, v, lb, ub),
            _ => Zones::new(l, u, lb, v),
        };
    }
    out
}
