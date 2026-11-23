// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A parameter's editor address: `<block>.<ident>`, e.g. `OP_C.CRSE`,
//! `KIT.TONAL`, `PAD3_FLT.CUTOFF`, `PART3.LEVEL`, `CHORUS.MIX`. A Sound's
//! block is its `disk_ident`; the kit's is `KIT`; a pad's is `PAD<n>` and a
//! suffix naming the part of the pad (ADR 0285); a Part's is `PART<n>` or
//! `PART<n>_SND`; a bus's, a global's and ORBIT's are their `disk_ident`s,
//! or `TEMPO` and `RANDOM` (ADR 0289). Every token is frozen once shipped.
//! The active Part (`PART`) and the selected ring are never addressed.

use core::fmt;

use chimera_engine::addr::{BlockRef, ParamAddr};
use chimera_engine::block::ParamId;
use chimera_engine::dsp::drum::sound::{PadFilter, RouteId};
use chimera_engine::dsp::drum::types::PadId;
use chimera_engine::orbit::RingIdx;
use chimera_engine::params::{EngineType, OutParams};
use chimera_engine::project::PartId;

use super::lit::lit;
use super::schema::Round;

/// The kit's own block's token: it has no `disk_ident` (a kit is stored
/// as its own records, ADR 0265).
const KIT: &str = "KIT";
lit!(
    /// A pad's tokens start with this, then its number.
    pub(super) PAD = "PAD"
);
lit!(
    /// A Part's tokens and its round's prefix start with this, then its
    /// number.
    pub(super) PART = "PART"
);
lit!(
    /// The suffix of a Part's sends' token, `PART3_SND`.
    SND = "_SND"
);
lit!(
    /// ORBIT's BPM's token: it has no `disk_ident` (its own record).
    TEMPO = "TEMPO"
);
lit!(
    /// RANDOMIZE's token: never stored, so no `disk_ident`.
    RANDOM = "RANDOM"
);

/// A block of the Part's Sound, or its kit's own: one with a token, never
/// a pad's (that is `EdBlock::Pad`), a bus's or the mix's. Only `new`
/// makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundBlock(BlockRef);

impl SoundBlock {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.new"))]
    pub fn new(b: BlockRef) -> Option<SoundBlock> {
        ((b.in_sound() || b == BlockRef::Kit) && token(b).is_some()).then_some(SoundBlock(b))
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.get"))]
    pub const fn get(self) -> BlockRef {
        self.0
    }
}

/// The block's token. One call site for `disk_ident` in this module, so the
/// compiler inlines it here, into bank 2, and not a shared copy in bank 1.
#[inline(never)]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.token"))]
fn token(b: BlockRef) -> Option<&'static str> {
    match b {
        BlockRef::Kit => Some(KIT),
        BlockRef::Tempo => Some(TEMPO.get()),
        BlockRef::Random => Some(RANDOM.get()),
        BlockRef::Modal
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Filter
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
        | BlockRef::PartMix(_)
        | BlockRef::Theme
        | BlockRef::OrbitChords
        | BlockRef::OrbitRing(_)
        | BlockRef::Midi
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::VoiceFx
        | BlockRef::Pad
        | BlockRef::PadFilter
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_) => b.disk_ident(),
    }
}

/// The part of a pad a block is: its own params, or its sound's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadBlock {
    Pad,
    Filter,
    Amp,
    Env,
    Lfo,
    Route(RouteId),
}

impl PadBlock {
    /// A pad's blocks, in slot order.
    pub const ALL: [PadBlock; 9] = [
        PadBlock::Pad,
        PadBlock::Filter,
        PadBlock::Amp,
        PadBlock::Env,
        PadBlock::Lfo,
        PadBlock::Route(RouteId::ALL[0]),
        PadBlock::Route(RouteId::ALL[1]),
        PadBlock::Route(RouteId::ALL[2]),
        PadBlock::Route(RouteId::ALL[3]),
    ];

    /// The selected pad's block this one is, on the named pad.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.block"))]
    pub const fn block(self) -> BlockRef {
        match self {
            PadBlock::Pad => BlockRef::Pad,
            PadBlock::Filter => BlockRef::PadFilter,
            PadBlock::Amp => BlockRef::PadAmp,
            PadBlock::Env => BlockRef::PadEnv,
            PadBlock::Lfo => BlockRef::PadLfo,
            PadBlock::Route(r) => BlockRef::PadRoute(r),
        }
    }

    /// The pad block `b` names; `None` for a block no pad has.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.of"))]
    pub const fn of(b: BlockRef) -> Option<PadBlock> {
        Some(match b {
            BlockRef::Pad => PadBlock::Pad,
            BlockRef::PadFilter => PadBlock::Filter,
            BlockRef::PadAmp => PadBlock::Amp,
            BlockRef::PadEnv => PadBlock::Env,
            BlockRef::PadLfo => PadBlock::Lfo,
            BlockRef::PadRoute(r) => PadBlock::Route(r),
            BlockRef::Modal
            | BlockRef::Algo
            | BlockRef::AlgoOp(_)
            | BlockRef::Filter
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
            | BlockRef::PartMix(_)
            | BlockRef::Theme
            | BlockRef::OrbitChords
            | BlockRef::OrbitRing(_)
            | BlockRef::Tempo
            | BlockRef::Midi
            | BlockRef::Random
            | BlockRef::TapeEngine
            | BlockRef::Va
            | BlockRef::VoiceFx
            | BlockRef::Kit => return None,
        })
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.suffix"))]
    fn stem(self) -> &'static str {
        match self {
            PadBlock::Pad => "",
            PadBlock::Filter => PAD_FLT.get(),
            PadBlock::Amp => PAD_AMP.get(),
            PadBlock::Env => PAD_ENV.get(),
            PadBlock::Lfo => PAD_LFO.get(),
            PadBlock::Route(_) => PAD_RT.get(),
        }
    }

    /// A route's number, 1–4, after its stem.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.addr.route_number")
    )]
    fn route_number(self) -> Option<u8> {
        match self {
            PadBlock::Route(r) => Some(r.number()),
            PadBlock::Pad | PadBlock::Filter | PadBlock::Amp | PadBlock::Env | PadBlock::Lfo => {
                None
            }
        }
    }

    /// Its suffix: the stem, then a route's number (`_FLT`, `_RT2`).
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.addr.write_suffix")
    )]
    fn write_suffix(self, w: &mut dyn fmt::Write) -> fmt::Result {
        w.write_str(self.stem())?;
        match self.route_number() {
            Some(n) => super::lit::uint(w, u32::from(n)),
            None => Ok(()),
        }
    }

    /// Whether `s` is its suffix.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.is_suffix"))]
    fn is_suffix(self, s: &[u8]) -> bool {
        let Some(rest) = after(self.stem().as_bytes(), s) else {
            return false;
        };
        match self.route_number() {
            None => rest.is_empty(),
            Some(n) => matches!(rest, [d] if *d == b'0' + n),
        }
    }
}

lit!(PAD_FLT = "_FLT");
lit!(PAD_AMP = "_AMP");
lit!(PAD_ENV = "_ENV");
lit!(PAD_LFO = "_LFO");
lit!(
    /// A pad's route's suffix, before its number: `_RT1` … `_RT4`.
    PAD_RT = "_RT"
);

/// A Part's own block: its mix (`PART<n>`, `PartParams`) or its Sound's
/// sends (`PART<n>_SND`, OUT's SEND1–SEND3 alone).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartBlock {
    Mix,
    Sends,
}

impl PartBlock {
    pub const ALL: [PartBlock; 2] = [PartBlock::Mix, PartBlock::Sends];

    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.addr.part_suffix")
    )]
    fn suffix(self) -> &'static str {
        match self {
            PartBlock::Mix => "",
            PartBlock::Sends => SND.get(),
        }
    }
}

/// A shared bus of the mixer: the Performance's FX (`CHORUS DELAY REVERB
/// TAPE COMP`, TAPE the master reel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusBlock {
    Chorus,
    Delay,
    Reverb,
    Tape,
    Comp,
}

impl BusBlock {
    pub const ALL: [BusBlock; 5] = [
        BusBlock::Chorus,
        BusBlock::Delay,
        BusBlock::Reverb,
        BusBlock::Tape,
        BusBlock::Comp,
    ];

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.bus_block"))]
    pub const fn block(self) -> BlockRef {
        match self {
            BusBlock::Chorus => BlockRef::Chorus,
            BusBlock::Delay => BlockRef::Delay,
            BusBlock::Reverb => BlockRef::Reverb,
            BusBlock::Tape => BlockRef::Tape,
            BusBlock::Comp => BlockRef::Comp,
        }
    }
}

/// A block the UI holds for the whole box: SETTINGS › THEME, MIDI CONFIG,
/// and the Part's RANDOMIZE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalBlock {
    Theme,
    Midi,
    Random,
}

impl GlobalBlock {
    pub const ALL: [GlobalBlock; 3] = [GlobalBlock::Theme, GlobalBlock::Midi, GlobalBlock::Random];

    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.addr.global_block")
    )]
    pub const fn block(self) -> BlockRef {
        match self {
            GlobalBlock::Theme => BlockRef::Theme,
            GlobalBlock::Midi => BlockRef::Midi,
            GlobalBlock::Random => BlockRef::Random,
        }
    }
}

/// ORBIT's: the chord ring, ring *n* of the live scene, the tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitBlock {
    Chords,
    Ring(RingIdx),
    Tempo,
}

impl OrbitBlock {
    pub const ALL: [OrbitBlock; 7] = [
        OrbitBlock::Chords,
        OrbitBlock::Ring(RingIdx::ALL[0]),
        OrbitBlock::Ring(RingIdx::ALL[1]),
        OrbitBlock::Ring(RingIdx::ALL[2]),
        OrbitBlock::Ring(RingIdx::ALL[3]),
        OrbitBlock::Ring(RingIdx::ALL[4]),
        OrbitBlock::Tempo,
    ];

    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.addr.orbit_block")
    )]
    pub const fn block(self) -> BlockRef {
        match self {
            OrbitBlock::Chords => BlockRef::OrbitChords,
            OrbitBlock::Ring(r) => BlockRef::OrbitRing(r),
            OrbitBlock::Tempo => BlockRef::Tempo,
        }
    }
}

/// Where an address's value lives: the followed Part's Sound (or kit), one
/// pad, one Part's mix or sends, a bus, a global, or ORBIT's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdBlock {
    Sound(SoundBlock),
    Pad(PadId, PadBlock),
    Part(PartId, PartBlock),
    Bus(BusBlock),
    Global(GlobalBlock),
    Orbit(OrbitBlock),
}

impl EdBlock {
    /// The box's block for it: the knob's, on the Part or pad it names.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.block.1"))]
    pub const fn block(self) -> BlockRef {
        match self {
            EdBlock::Sound(s) => s.get(),
            EdBlock::Pad(_, p) => p.block(),
            EdBlock::Part(_, PartBlock::Mix) => BlockRef::Part,
            EdBlock::Part(_, PartBlock::Sends) => BlockRef::Out,
            EdBlock::Bus(b) => b.block(),
            EdBlock::Global(g) => g.block(),
            EdBlock::Orbit(o) => o.block(),
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.pad"))]
    pub const fn pad(self) -> Option<PadId> {
        match self {
            EdBlock::Pad(n, _) => Some(n),
            EdBlock::Sound(_)
            | EdBlock::Part(..)
            | EdBlock::Bus(_)
            | EdBlock::Global(_)
            | EdBlock::Orbit(_) => None,
        }
    }

    /// The Part it names, a Part's block's.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.part"))]
    pub const fn part(self) -> Option<PartId> {
        match self {
            EdBlock::Part(n, _) => Some(n),
            EdBlock::Sound(_)
            | EdBlock::Pad(..)
            | EdBlock::Bus(_)
            | EdBlock::Global(_)
            | EdBlock::Orbit(_) => None,
        }
    }

    /// Whether the editor reaches it while following a Part on `engine`:
    /// a Sound's block or a pad's by the engine; a Part's, the buses and
    /// the globals always (the master reel with `master-tape`); ORBIT's
    /// with `orbit`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.reachable"))]
    pub fn reachable(self, engine: EngineType) -> bool {
        match self {
            EdBlock::Sound(s) => in_engine(engine, s.get()),
            EdBlock::Pad(..) => engine == EngineType::Drum,
            EdBlock::Part(..) => true,
            EdBlock::Bus(BusBlock::Tape) => cfg!(feature = "master-tape"),
            EdBlock::Bus(
                BusBlock::Chorus | BusBlock::Delay | BusBlock::Reverb | BusBlock::Comp,
            ) => true,
            EdBlock::Global(GlobalBlock::Theme | GlobalBlock::Midi | GlobalBlock::Random) => true,
            EdBlock::Orbit(OrbitBlock::Chords | OrbitBlock::Ring(_) | OrbitBlock::Tempo) => {
                cfg!(feature = "orbit")
            }
        }
    }

    /// The ids it holds, when not its block's whole table: a pad's filter
    /// its six, a Part's sends SEND1–SEND3.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.only"))]
    fn only(self) -> Option<&'static [ParamId]> {
        match self {
            EdBlock::Pad(_, PadBlock::Filter) => Some(&PadFilter::IDS),
            EdBlock::Part(_, PartBlock::Sends) => Some(&OutParams::SENDS),
            EdBlock::Pad(..)
            | EdBlock::Sound(_)
            | EdBlock::Part(_, PartBlock::Mix)
            | EdBlock::Bus(_)
            | EdBlock::Global(_)
            | EdBlock::Orbit(_) => None,
        }
    }

    /// Its params' slots.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.width"))]
    pub fn width(self) -> usize {
        match self.only() {
            Some(ids) => ids.len(),
            None => self.block().specs().len(),
        }
    }

    /// Param `i` of its slots.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.id_at"))]
    pub fn id_at(self, i: usize) -> Option<ParamId> {
        match self.only() {
            Some(ids) => ids.get(i).copied(),
            None => self.block().specs().get(i).map(|s| s.id),
        }
    }

    /// The slot `id` takes in it: `None` for a param it doesn't hold.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.slot"))]
    pub fn slot(self, id: ParamId) -> Option<usize> {
        let mut i = 0;
        while let Some(at) = self.id_at(i) {
            if at == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// Both halves are frozen literals: the block's token and
/// `ParamSpec::ident`. Only a param the block holds is an address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdAddr {
    at: EdBlock,
    id: ParamId,
}

impl EdAddr {
    /// An empty queue slot's: ALGO's first param, which it holds.
    pub(super) const SPARE: EdAddr = EdAddr {
        at: EdBlock::Sound(SoundBlock(BlockRef::Algo)),
        id: ParamId(0),
    };

    /// `id` of `at`, if `at` holds it.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.new.1"))]
    pub fn new(at: EdBlock, id: ParamId) -> Option<EdAddr> {
        at.slot(id).map(|_| EdAddr { at, id })
    }

    /// A param of the Sound (or the kit): never a pad's.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.sound"))]
    pub fn sound(a: ParamAddr) -> Option<EdAddr> {
        EdAddr::new(EdBlock::Sound(SoundBlock::new(a.block)?), a.param)
    }

    /// The selected pad's param `a`, on pad `n`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.on_pad"))]
    pub fn on_pad(n: PadId, a: ParamAddr) -> Option<EdAddr> {
        EdAddr::new(EdBlock::Pad(n, PadBlock::of(a.block)?), a.param)
    }

    /// `a` as a cell on a page of round `r` shows it: a pad's param is
    /// pad `n`'s on its round, the active Part's mix and OUT's sends Part
    /// `n`'s on its, a mirror (`PartMix`) its own Part's; a bus, a global
    /// and ORBIT's their own; any other the Sound's.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.shown"))]
    pub fn shown(a: ParamAddr, r: Round) -> Option<EdAddr> {
        let at = match (a.block, r) {
            (b, Round::Pad(n)) if b.is_pad() => return EdAddr::on_pad(n, a),
            (BlockRef::Part, Round::Part(n)) => EdBlock::Part(n, PartBlock::Mix),
            (BlockRef::Out, Round::Part(n)) => EdBlock::Part(n, PartBlock::Sends),
            (BlockRef::PartMix(n), _) => EdBlock::Part(n, PartBlock::Mix),
            (b, _) => return EdAddr::new(block_of(b)?, a.param),
        };
        EdAddr::new(at, a.param)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.at"))]
    pub const fn at(self) -> EdBlock {
        self.at
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.id"))]
    pub const fn id(self) -> ParamId {
        self.id
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.pad.1"))]
    pub const fn pad(self) -> Option<PadId> {
        self.at.pad()
    }

    /// The box's address of it, as its knob turns it: the selected pad's
    /// for a pad's, the active Part's mix or OUT for a Part's (`edit_to`
    /// names the Part).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.param"))]
    pub const fn param(self) -> ParamAddr {
        ParamAddr::new(self.at.block(), self.id)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.parse"))]
    pub fn parse(word: &[u8]) -> Option<EdAddr> {
        let dot = word.iter().position(|&b| b == b'.')?;
        let (block, ident) = (&word[..dot], &word[dot + 1..]);
        if ident.is_empty() {
            return None;
        }
        let at = block_named(block)?;
        let spec = at
            .block()
            .specs()
            .iter()
            .find(|s| s.ident.as_bytes() == ident)?;
        EdAddr::new(at, spec.id)
    }
}

/// The block a box's `b` is when named by its own token: the Sound's, a
/// bus, a global or ORBIT's; `None` for one only a round names (a pad's,
/// the active Part's) or the mirror.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.block_of"))]
fn block_of(b: BlockRef) -> Option<EdBlock> {
    Some(match b {
        BlockRef::Chorus => EdBlock::Bus(BusBlock::Chorus),
        BlockRef::Delay => EdBlock::Bus(BusBlock::Delay),
        BlockRef::Reverb => EdBlock::Bus(BusBlock::Reverb),
        BlockRef::Tape => EdBlock::Bus(BusBlock::Tape),
        BlockRef::Comp => EdBlock::Bus(BusBlock::Comp),
        BlockRef::Theme => EdBlock::Global(GlobalBlock::Theme),
        BlockRef::Midi => EdBlock::Global(GlobalBlock::Midi),
        BlockRef::Random => EdBlock::Global(GlobalBlock::Random),
        BlockRef::OrbitChords => EdBlock::Orbit(OrbitBlock::Chords),
        BlockRef::OrbitRing(r) => EdBlock::Orbit(OrbitBlock::Ring(r)),
        BlockRef::Tempo => EdBlock::Orbit(OrbitBlock::Tempo),
        BlockRef::Part
        | BlockRef::PartMix(_)
        | BlockRef::Pad
        | BlockRef::PadFilter
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_) => return None,
        BlockRef::Modal
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Filter
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::VoiceFx
        | BlockRef::Kit => EdBlock::Sound(SoundBlock::new(b)?),
    })
}

/// Every block named by its own token, in `BlockRef::ALL` order, then
/// the kit, the tempo and RANDOMIZE. A static in bank 2: a `const` walked
/// by value would be anonymous rodata, bank 1's.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.named"))]
static NAMED: [BlockRef; BlockRef::ALL.len() + 3] = {
    let mut v = [BlockRef::Kit; BlockRef::ALL.len() + 3];
    let mut i = 0;
    while i < BlockRef::ALL.len() {
        v[i] = BlockRef::ALL[i];
        i += 1;
    }
    v[i + 1] = BlockRef::Tempo;
    v[i + 2] = BlockRef::Random;
    v
};

/// The Sound's blocks an engine's editor shows: the other engines' params
/// sit in the Sound too, and are no part of it. The kit's is DRUM's. A
/// block no `SoundBlock` holds is no engine's.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.in_engine"))]
fn in_engine(engine: EngineType, b: BlockRef) -> bool {
    match b {
        BlockRef::Algo | BlockRef::AlgoOp(_) => engine == EngineType::Algo,
        BlockRef::Modal => engine == EngineType::Modal,
        BlockRef::TapeEngine => engine == EngineType::Tape,
        BlockRef::Va => engine == EngineType::Va,
        BlockRef::Kit => engine == EngineType::Drum,
        BlockRef::Filter
        | BlockRef::VoiceFx
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch => true,
        BlockRef::Pad
        | BlockRef::PadFilter
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_)
        | BlockRef::Chorus
        | BlockRef::Delay
        | BlockRef::Reverb
        | BlockRef::Tape
        | BlockRef::Comp
        | BlockRef::Part
        | BlockRef::PartMix(_)
        | BlockRef::Theme
        | BlockRef::OrbitChords
        | BlockRef::OrbitRing(_)
        | BlockRef::Tempo
        | BlockRef::Midi
        | BlockRef::Random => false,
    }
}

/// The block `name` is the token of.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.addr.block_named")
)]
fn block_named(name: &[u8]) -> Option<EdBlock> {
    if let Some(rest) = after(PAD.bytes(), name) {
        return pad_named(rest);
    }
    if let Some(rest) = after(PART.bytes(), name) {
        return part_named(rest);
    }
    let b = *NAMED
        .iter()
        .find(|&&b| token(b).is_some_and(|t| t.as_bytes() == name))?;
    block_of(b)
}

/// What follows `prefix` in `name`, if it starts so: core's
/// `strip_prefix` is a generic bank 1 would hold the copy of.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.after"))]
fn after<'a>(prefix: &[u8], name: &'a [u8]) -> Option<&'a [u8]> {
    let (head, rest) = name.split_at_checked(prefix.len())?;
    (head == prefix).then_some(rest)
}

/// A number written without a leading zero, and what follows it.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.numbered"))]
fn numbered(rest: &[u8]) -> Option<(u32, &[u8])> {
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let (num, suffix) = rest.split_at(digits);
    if num.first() == Some(&b'0') {
        return None;
    }
    Some((chimera_theory::tuning::uint(num)?, suffix))
}

/// `<n>` or `<n>_SND` after `PART`: `n` 1–6.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.part_named"))]
fn part_named(rest: &[u8]) -> Option<EdBlock> {
    let (n, suffix) = numbered(rest)?;
    let part = PartId::from_number(n)?;
    let b = match suffix {
        b"" => PartBlock::Mix,
        s if s == SND.bytes() => PartBlock::Sends,
        _ => return None,
    };
    Some(EdBlock::Part(part, b))
}

/// `<n><suffix>` after `PAD`: `n` 1–12, written without a leading zero.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.pad_named"))]
fn pad_named(rest: &[u8]) -> Option<EdBlock> {
    let (n, suffix) = numbered(rest)?;
    let pad = PadId::ALL
        .into_iter()
        .find(|p| u32::from(p.number()) == n)?;
    let part = PadBlock::ALL.into_iter().find(|p| p.is_suffix(suffix))?;
    Some(EdBlock::Pad(pad, part))
}

impl EdBlock {
    /// Its token, written with no `write!`: a format's pieces are bank 1's.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.write"))]
    pub fn write(&self, w: &mut dyn fmt::Write) -> fmt::Result {
        match *self {
            EdBlock::Sound(s) => w.write_str(token(s.get()).ok_or(fmt::Error)?),
            EdBlock::Pad(n, p) => {
                w.write_str(PAD.get())?;
                super::lit::uint(w, u32::from(n.number()))?;
                p.write_suffix(w)
            }
            EdBlock::Part(n, p) => {
                w.write_str(PART.get())?;
                super::lit::uint(w, u32::from(n.number()))?;
                w.write_str(p.suffix())
            }
            EdBlock::Bus(_) | EdBlock::Global(_) | EdBlock::Orbit(_) => {
                w.write_str(token(self.block()).ok_or(fmt::Error)?)
            }
        }
    }
}

impl EdAddr {
    /// `<BLOCK>.<IDENT>`, written with no `write!`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.addr.write.1"))]
    pub fn write(&self, w: &mut dyn fmt::Write) -> fmt::Result {
        let spec = self.param().spec().ok_or(fmt::Error)?;
        self.at.write(w)?;
        w.write_char('.')?;
        w.write_str(spec.ident)
    }
}

impl fmt::Display for EdBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f)
    }
}

impl fmt::Display for EdAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write(f)
    }
}
