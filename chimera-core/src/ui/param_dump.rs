//! The parameter dump (#984, ADR 0298): a controller's request answered
//! with the Part's Sound, each writable cell its engine reads as the NRPN
//! PARAM OUT sends for it (ADR 0216's address and law, ADR 0258's bytes),
//! on the Part's channel, out of the port that asked, then the end
//! message. A walk of the chart kept between passes: each pass sends
//! what fits half the port's ring, so DIN is paced by its own UART and
//! the echo and PARAM OUT, which go first, always find room.
//!
//! Bank 2's, an S4 group; called only from bank 2's `ui::midi`. Plain
//! loops, no iterator chains: an adaptor's own symbol would be bank 1's.

use chimera_midi::out::{Running, Writer};
use chimera_midi::sysex::{DumpWho, PartNo, dump_end};

use crate::addr::{BlockRef, ParamAddr};
use crate::midi_in::Port;
use crate::midi_map::Mappable;
use crate::nrpn::NrpnAddr;
use crate::out_ring::{OUT_RING_LEN, OutPorts, OutRing};
use crate::params::EngineType;
use crate::project::{Loads, PartId, Project};

/// The dump leaves this much of a port's ring to the echo and PARAM OUT.
const KEPT_FREE: usize = OUT_RING_LEN / 2;

/// The longest message: an NRPN with its fine value, status included.
const MSG_MAX: usize = 9;

/// Where a walk of the Sound's cells stands: a block of `BlockRef::ALL`
/// and a spec of it, next to send.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Cursor {
    block: u8,
    spec: u8,
}

impl Cursor {
    const START: Self = Self { block: 0, spec: 0 };
}

/// A dump under way: whose, where its walk stands, and the Sound it
/// reads (a load or another engine since starts it again).
#[derive(Clone, Copy)]
struct Walk {
    part: PartId,
    at: Cursor,
    loads: Loads,
    engine: EngineType,
}

/// The dumps asked for, one a `Port`: a new ask on a port replaces its
/// last.
#[derive(Clone, Copy)]
pub struct ParamDump {
    walks: [Option<Walk>; Port::COUNT],
}

/// What a pass of a walk came to.
enum Pass {
    /// The ring had no room: more next pass.
    Waits,
    /// The end message is sent.
    Done,
}

impl ParamDump {
    pub const REST: Self = Self {
        walks: [None; Port::COUNT],
    };

    /// A request heard on `port`: the Part it names starts its dump
    /// there. A number or a channel with no Part asks nothing.
    #[inline(never)]
    pub fn ask(&mut self, port: Port, who: DumpWho, project: &Project, editing: PartId) {
        let Some(part) = named(who, project, editing) else {
            return;
        };
        self.walks[port as usize] = Some(Walk::of(part, project));
    }

    /// MIDI CONFIG › ECHO is off: nothing goes out, and a request heard
    /// meanwhile is never answered.
    pub fn silence(&mut self) {
        self.walks = [None; Port::COUNT];
    }

    /// One pass: each port's dump goes on while its ring has room.
    #[inline(never)]
    pub fn pump(&mut self, project: &Project, out: &OutPorts) {
        let rings = [Some(out.din), out.usb];
        let mut i = 0;
        while i < Port::COUNT {
            if let (Some(w), Some(ring)) = (self.walks[i].as_mut(), rings[i])
                && matches!(w.pass(project, ring), Pass::Done)
            {
                self.walks[i] = None;
            }
            i += 1;
        }
    }
}

/// The Part `who` names: by number, the lowest on a channel (the one
/// whose echo a shared control hears, `echo::value`), or `editing`.
fn named(who: DumpWho, project: &Project, editing: PartId) -> Option<PartId> {
    match who {
        DumpWho::Editing => Some(editing),
        DumpWho::Part(n) => PartId::ALL.get(usize::from(n.get())).copied(),
        DumpWho::Channel(c) => {
            let mut i = 0;
            while i < PartId::ALL.len() {
                let p = PartId::ALL[i];
                if project.part(p).mix.channel == c {
                    return Some(p);
                }
                i += 1;
            }
            None
        }
    }
}

impl Walk {
    fn of(part: PartId, project: &Project) -> Self {
        let p = project.part(part);
        Self {
            part,
            at: Cursor::START,
            loads: p.loads(),
            engine: p.sound.engine(),
        }
    }

    /// The walk's next messages into `ring`, whole, while they leave
    /// `KEPT_FREE` of it; from the top again if the Sound was replaced, or
    /// the ring lost what it held (USB's poll drops all of it).
    fn pass(&mut self, project: &Project, ring: &OutRing) -> Pass {
        let now = Walk::of(self.part, project);
        if now.loads != self.loads || now.engine != self.engine || ring.losing() {
            *self = now;
        }
        if ring.losing() {
            return Pass::Waits;
        }
        let ch = project.part(self.part).mix.channel;
        let mut run = Running::NONE;
        while let Some((a, n)) = self.cell() {
            let mut b = [0u8; MSG_MAX];
            let mut w = Writer::after(&mut b, run);
            // A cell with no value to send (none today) is passed over.
            let ok = super::param_out::data(project, self.part, a)
                .is_some_and(|d| w.nrpn(ch, n.get(), d));
            let (len, r) = (w.len(), w.running());
            if ok {
                if !fits(ring, len) || !ring.push_msg(&b[..len]) {
                    return Pass::Waits;
                }
                run = r;
            }
            self.at.spec += 1;
        }
        let mut b = [0u8; MSG_MAX];
        let mut w = Writer::new(&mut b);
        let ended = PartNo::new(self.part.index() as u8).is_some_and(|p| dump_end(p, &mut w));
        let len = w.len();
        if ended && !(fits(ring, len) && ring.push_msg(&b[..len])) {
            return Pass::Waits;
        }
        Pass::Done
    }

    /// The cell at the cursor, moved on to it: the next writable stored
    /// param of a Sound's block that the engine reads, with its NRPN.
    /// `None`: the walk is over.
    fn cell(&mut self) -> Option<(ParamAddr, NrpnAddr)> {
        while let Some(&block) = BlockRef::ALL.get(usize::from(self.at.block)) {
            let specs = block.specs();
            while let Some(spec) = specs.get(usize::from(self.at.spec)) {
                let a = ParamAddr::new(block, spec.id);
                if block.in_sound()
                    && let Some(m) = Mappable::new(a)
                    && m.live(self.engine)
                    && let Some(n) = NrpnAddr::of(a)
                {
                    return Some((a, n));
                }
                self.at.spec += 1;
            }
            self.at = Cursor {
                block: self.at.block + 1,
                spec: 0,
            };
        }
        None
    }
}

/// `len` more bytes leave `ring` its kept half.
fn fits(ring: &OutRing, len: usize) -> bool {
    ring.held() + len <= OUT_RING_LEN - KEPT_FREE
}
