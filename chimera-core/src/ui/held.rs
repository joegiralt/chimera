//! A WAVE or ALG A/B choice the formant budget refuses (ADR 0151), a
//! TAPE machine or WALK DIRECTION a streamed source can't play, or a pad's
//! CHOKE that makes a 9th voice group (ADR 0280): the turn lands on it, the
//! cell shows it dimmed with the reason in the focus band, and nothing is
//! written.

use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::ParamId;
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::kinds::{Linked, OpKind, Why, why_kind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::drum::groups::{KitChokes, VoiceGroups};
use crate::dsp::drum::params::PadParams;
use crate::dsp::drum::types::{ChokeGroup, PadId};
use crate::dsp::tape_engine::{Direction, NeedsRam, TapeEngineParams, TapeMachine};
use crate::part::PartParams;
use crate::preset::Sound;
use crate::project::Line;
use crate::voice_limit::{Asked, Edge, MaxVoices, Refusal, Reserve, Seen};

/// The choice a turn stopped on, typed by its cell: the cell is the
/// choice's, so the two can't disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    /// A Sound's cell the formant budget or a streamed source refuses.
    Sound(Pick),
    /// A pad's CHOKE that makes a 9th voice group (ADR 0280).
    Choke(ChokeHold),
    /// A Part's MAX VOICES or RESERVE that can't be honoured (ADR 0281).
    Voices(VoicesHold),
}

/// Where a TAPE Sound's source plays from: RAM plays every machine and
/// direction, a streamed one refuses some (`NeedsRam`). No source: RAM's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    InRam,
    Streamed,
}

/// A Sound's held choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Operator `Op`'s WAVE.
    Wave(Op, OpKind),
    /// ALG A or B.
    Alg(AlgSide, AlgoId),
    /// TAPE's MODEL or DIRECTION.
    Tape(TapeTurn),
}

/// Which ALG cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlgSide {
    A,
    B,
}

impl AlgSide {
    pub const fn param(self) -> ParamId {
        match self {
            AlgSide::A => AlgoParams::ALG_A,
            AlgSide::B => AlgoParams::ALG_B,
        }
    }
}

/// A Sound's cell a turn can hold a refused choice on (ADR 0151).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Wave(Op),
    Alg(AlgSide),
}

/// A TAPE MODEL or DIRECTION choice: its own type, the cell with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapeTurn {
    Model(TapeMachine),
    Direction(Direction),
}

impl TapeTurn {
    /// Cell `a`'s (MODEL or DIRECTION) at stored value `v`; `None` off
    /// them.
    pub fn of(a: ParamAddr, v: f32) -> Option<TapeTurn> {
        let i = (v as usize).min(Direction::ALL.len() - 1);
        match a {
            MODEL => Some(TapeTurn::Model(TapeMachine::ALL[i])),
            DIRECTION => Some(TapeTurn::Direction(Direction::ALL[i])),
            _ => None,
        }
    }

    pub const fn addr(self) -> ParamAddr {
        match self {
            TapeTurn::Model(_) => MODEL,
            TapeTurn::Direction(_) => DIRECTION,
        }
    }

    /// The cell's stored value.
    pub const fn code(self) -> u8 {
        match self {
            TapeTurn::Model(m) => m.index(),
            TapeTurn::Direction(d) => d as u8,
        }
    }

    /// Turned `delta` along its list, saturating; with `shift`, to its end
    /// that way.
    pub fn step(self, delta: i8, shift: bool) -> TapeTurn {
        let last = Direction::ALL.len() - 1;
        let to = match (shift, delta > 0) {
            (true, true) => last,
            (true, false) => 0,
            _ => usize::from(self.code())
                .saturating_add_signed(isize::from(delta))
                .min(last),
        };
        match self {
            TapeTurn::Model(_) => TapeTurn::Model(TapeMachine::ALL[to]),
            TapeTurn::Direction(_) => TapeTurn::Direction(Direction::ALL[to]),
        }
    }
}

// MODEL and DIRECTION list four each: one index walks both.
const _: () = assert!(TapeMachine::ALL.len() == Direction::ALL.len());

/// A Part's MAX VOICES or RESERVE cell (ADR 0281).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceCell {
    Max,
    Reserve,
}

impl VoiceCell {
    pub const fn addr(self) -> ParamAddr {
        match self {
            VoiceCell::Max => MAX_VOICES,
            VoiceCell::Reserve => RESERVE,
        }
    }

    /// The cell `a` is, if it is one of them.
    pub fn of(a: ParamAddr) -> Option<VoiceCell> {
        match a {
            MAX_VOICES => Some(VoiceCell::Max),
            RESERVE => Some(VoiceCell::Reserve),
            _ => None,
        }
    }
}

/// A MAX VOICES or RESERVE choice: its own type, the cell with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceTurn {
    Max(MaxVoices),
    Reserve(Reserve),
}

impl VoiceTurn {
    /// Cell `c`'s choice in `a`.
    pub const fn of(c: VoiceCell, a: Asked) -> VoiceTurn {
        match c {
            VoiceCell::Max => VoiceTurn::Max(a.max),
            VoiceCell::Reserve => VoiceTurn::Reserve(a.reserve),
        }
    }

    pub const fn cell(self) -> VoiceCell {
        match self {
            VoiceTurn::Max(_) => VoiceCell::Max,
            VoiceTurn::Reserve(_) => VoiceCell::Reserve,
        }
    }

    /// The cell's stored value.
    pub const fn code(self) -> u8 {
        match self {
            VoiceTurn::Max(m) => m.code(),
            VoiceTurn::Reserve(r) => r.code(),
        }
    }

    /// `a` with this choice in its cell.
    pub const fn on(self, a: Asked) -> Asked {
        match self {
            VoiceTurn::Max(max) => Asked { max, ..a },
            VoiceTurn::Reserve(reserve) => Asked { reserve, ..a },
        }
    }

    /// Turned `delta` along its list, saturating; with `shift`, to its end
    /// that way.
    pub const fn step(self, delta: i8, shift: bool) -> VoiceTurn {
        match (self, shift) {
            (VoiceTurn::Max(_), true) => VoiceTurn::Max(MaxVoices::ended(Edge::of(delta))),
            (VoiceTurn::Reserve(_), true) => VoiceTurn::Reserve(Reserve::ended(Edge::of(delta))),
            (VoiceTurn::Max(m), false) => VoiceTurn::Max(m.stepped(delta)),
            (VoiceTurn::Reserve(r), false) => VoiceTurn::Reserve(r.stepped(delta)),
        }
    }
}

/// A held MAX VOICES or RESERVE, and what it was judged against: the
/// Part's own pair and the other Parts' reserves. A change to them (a
/// load, another Part's RESERVE) drops it; any other edit (a CC on any
/// Part) leaves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicesHold {
    pub to: VoiceTurn,
    pub seen: Seen,
}

/// A held CHOKE: the group, the kit's chokes at the hold and the pad
/// selected then, whose CHOKE it is. The kit's chokes changed since (a
/// kit loaded in, a CC on another pad) drop it; any other edit (another
/// Part's, a pedal's) leaves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChokeHold {
    pub to: ChokeGroup,
    pub kit: KitChokes,
    pub pad: PadId,
}

impl Held {
    /// The cell it is held on.
    pub fn addr(self) -> ParamAddr {
        match self {
            Held::Sound(Pick::Wave(op, _)) => {
                ParamAddr::new(BlockRef::AlgoOp(op), AlgoOpParams::WAVE)
            }
            Held::Sound(Pick::Alg(side, _)) => ParamAddr::new(BlockRef::Algo, side.param()),
            Held::Sound(Pick::Tape(t)) => t.addr(),
            Held::Choke(_) => CHOKE,
            Held::Voices(v) => v.to.cell().addr(),
        }
    }

    /// The cell's enum code: what its text shows.
    pub fn code(self) -> u8 {
        match self {
            Held::Sound(Pick::Wave(_, k)) => k.code(),
            Held::Sound(Pick::Alg(_, a)) => a.get(),
            Held::Sound(Pick::Tape(t)) => t.code(),
            Held::Choke(c) => c.to.index() as u8,
            Held::Voices(v) => v.to.code(),
        }
    }
}

/// Why a choice is refused: the formant budget, a streamed source, or
/// the kit's voice groups (the count the choke would make).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    Formant(Why),
    NeedsRam(NeedsRam),
    Groups(VoiceGroups),
    Voices(Refusal),
}

impl From<Why> for Reason {
    fn from(w: Why) -> Reason {
        Reason::Formant(w)
    }
}

/// A held choice and why it is refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub held: Held,
    pub why: Reason,
}

/// TAPE's MODEL: on a streamed source a turn holds a machine it refuses.
pub const MODEL: ParamAddr = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL);
/// WALK's DIRECTION: likewise REVERSE and PING-PONG (ADR 0203).
pub const DIRECTION: ParamAddr = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODE);

/// A pad's CHOKE: a turn holds a 9th voice group (ADR 0280).
pub const CHOKE: ParamAddr = ParamAddr::new(BlockRef::Pad, PadParams::CHOKE);

/// A Part's MAX VOICES and RESERVE: a turn holds what can't be honoured
/// (ADR 0281).
pub const MAX_VOICES: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::MAX_VOICES);
pub const RESERVE: ParamAddr = ParamAddr::new(BlockRef::Part, PartParams::RESERVE);

/// The cell `a` is, if it is one this applies to (ALG A/B not with MIX).
pub fn applies(a: ParamAddr, shift: bool) -> Option<Cell> {
    match a.block {
        BlockRef::AlgoOp(op) => (a.param == AlgoOpParams::WAVE).then_some(Cell::Wave(op)),
        BlockRef::Algo if shift => None,
        BlockRef::Algo => match a.param {
            AlgoParams::ALG_A => Some(Cell::Alg(AlgSide::A)),
            AlgoParams::ALG_B => Some(Cell::Alg(AlgSide::B)),
            _ => None,
        },
        BlockRef::Modal
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::Filter
        | BlockRef::VoiceFx
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch
        | BlockRef::Chorus
        | BlockRef::Delay
        | BlockRef::Reverb
        | BlockRef::Tape
        | BlockRef::Comp
        | BlockRef::Part
        | BlockRef::Theme
        | BlockRef::Midi
        | BlockRef::PartMix(_)
        | BlockRef::OrbitChords
        | BlockRef::OrbitRing(_)
        | BlockRef::Tempo
        | BlockRef::Random
        | BlockRef::Pad
        | BlockRef::Kit
        | BlockRef::PadFilter
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_) => None,
    }
}

impl Pick {
    /// Why `sound`, its source at `src`, refuses it, or `None` where it
    /// would land.
    pub fn why(self, sound: &Sound, src: Source) -> Option<Reason> {
        let p = &sound.params.algo;
        let why = match self {
            Pick::Tape(t) => {
                if src == Source::InRam {
                    return None;
                }
                // Bank 2's (`ui::walk`): closed, nothing is held.
                let tape = &sound.params.tape;
                return crate::bank2::ui_door(|u| u.tape_held(tape, t))
                    .flatten()
                    .map(Reason::NeedsRam);
            }
            Pick::Wave(op, k) => why_kind(p.kinds(), p.linked(), op.index(), k),
            Pick::Alg(side, to) => {
                let (a, b) = p.algs();
                let l = match side {
                    AlgSide::A => Linked::of(to, b),
                    AlgSide::B => Linked::of(a, to),
                };
                (!p.kinds().fit(l)).then_some(Why::Links)
            }
        };
        why.map(Reason::Formant)
    }
}

/// The focus band's reason: 216 px at the house tracking.
pub fn reason(w: impl Into<Reason>) -> Line {
    let w = match w.into() {
        Reason::Formant(w) => w,
        Reason::NeedsRam(n) => {
            let mut l = Line::new(crate::ui::source_cell::needs_ram_words(n));
            let _ = core::fmt::Write::write_str(&mut l, " · STREAMED");
            return l;
        }
        Reason::Groups(n) => {
            return match crate::bank2::groups(super::kit_groups::Ask::Why(n)) {
                Some(super::kit_groups::Said::Line(l)) => l,
                _ => Line::new(""),
            };
        }
        Reason::Voices(r) => {
            // Bank 2's words (`part_voices`): closed, none.
            let mut l = None;
            crate::bank2::voices(super::part_voices::Ask::Why(r, &mut l));
            return l.unwrap_or_else(|| Line::new(""));
        }
    };
    match w {
        Why::Modulated(op) => {
            let mut l = Line::new("OP ");
            let _ = core::fmt::Write::write_fmt(
                &mut l,
                format_args!("{} MODULATED · OVER BUDGET", op + 1),
            );
            l
        }
        Why::Full => Line::new("BUDGET FULL · OTHER FORMANTS"),
        Why::Links => Line::new("LINKS A FORMANT · OVER BUDGET"),
    }
}
