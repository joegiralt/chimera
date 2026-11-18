//! A parameter's editor address: `<block>.<ident>`, e.g. `OP_C.CRSE`,
//! `KIT.TONAL`, `PAD3_FLT.CUTOFF`. A Sound's block is its `disk_ident`;
//! the kit's is `KIT`; a pad's is `PAD<n>` and a suffix naming the part of
//! the pad (ADR 0285). Every token is frozen once shipped.

use core::fmt;

use chimera_engine::addr::{BlockRef, ParamAddr};
use chimera_engine::block::ParamId;
use chimera_engine::dsp::drum::sound::{PadFilter, RouteId};
use chimera_engine::dsp::drum::types::PadId;

/// The kit's own block's token: it has no `disk_ident` (a kit is stored
/// as its own records, ADR 0265).
const KIT: &str = "KIT";
/// A pad's tokens start with this, then its number.
const PAD: &[u8] = b"PAD";

/// A block of the Part's Sound, or its kit's own: one with a token, never
/// a pad's (that is `EdBlock::Pad`). Only `new` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundBlock(BlockRef);

impl SoundBlock {
    pub fn new(b: BlockRef) -> Option<SoundBlock> {
        (!b.is_pad() && token(b).is_some()).then_some(SoundBlock(b))
    }

    pub const fn get(self) -> BlockRef {
        self.0
    }
}

/// The block's token. One call site for `disk_ident` in this module, so the
/// compiler inlines it here, into bank 2, and not a shared copy in bank 1.
#[inline(never)]
fn token(b: BlockRef) -> Option<&'static str> {
    match b {
        BlockRef::Kit => Some(KIT),
        _ => b.disk_ident(),
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

    const fn suffix(self) -> &'static str {
        match self {
            PadBlock::Pad => "",
            PadBlock::Filter => "_FLT",
            PadBlock::Amp => "_AMP",
            PadBlock::Env => "_ENV",
            PadBlock::Lfo => "_LFO",
            PadBlock::Route(r) => ["_RT1", "_RT2", "_RT3", "_RT4"][r.index()],
        }
    }
}

/// Where an address's value lives: the Part's Sound (or kit), or one pad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdBlock {
    Sound(SoundBlock),
    Pad(PadId, PadBlock),
}

impl EdBlock {
    pub const fn block(self) -> BlockRef {
        match self {
            EdBlock::Sound(s) => s.get(),
            EdBlock::Pad(_, p) => p.block(),
        }
    }

    pub const fn pad(self) -> Option<PadId> {
        match self {
            EdBlock::Sound(_) => None,
            EdBlock::Pad(n, _) => Some(n),
        }
    }

    /// Its params' slots: a pad's filter holds only `PadFilter::IDS`.
    pub fn width(self) -> usize {
        match self {
            EdBlock::Pad(_, PadBlock::Filter) => PadFilter::IDS.len(),
            EdBlock::Pad(..) | EdBlock::Sound(_) => self.block().specs().len(),
        }
    }

    /// Param `i` of its slots.
    pub fn id_at(self, i: usize) -> Option<ParamId> {
        match self {
            EdBlock::Pad(_, PadBlock::Filter) => PadFilter::IDS.get(i).copied(),
            EdBlock::Pad(..) | EdBlock::Sound(_) => self.block().specs().get(i).map(|s| s.id),
        }
    }

    /// The slot `id` takes in it: `None` for a param it doesn't hold.
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
    pub fn new(at: EdBlock, id: ParamId) -> Option<EdAddr> {
        at.slot(id).map(|_| EdAddr { at, id })
    }

    /// A param of the Sound (or the kit): never a pad's.
    pub fn sound(a: ParamAddr) -> Option<EdAddr> {
        EdAddr::new(EdBlock::Sound(SoundBlock::new(a.block)?), a.param)
    }

    /// The selected pad's param `a`, on pad `n`.
    pub fn on_pad(n: PadId, a: ParamAddr) -> Option<EdAddr> {
        EdAddr::new(EdBlock::Pad(n, PadBlock::of(a.block)?), a.param)
    }

    /// `a` as a cell on pad `n`'s page shows it: a pad's param is pad
    /// `n`'s, any other the Sound's.
    pub fn shown(a: ParamAddr, pad: Option<PadId>) -> Option<EdAddr> {
        match (pad, a.block.is_pad()) {
            (Some(n), true) => EdAddr::on_pad(n, a),
            (None, true) => None,
            (_, false) => EdAddr::sound(a),
        }
    }

    pub const fn at(self) -> EdBlock {
        self.at
    }

    pub const fn id(self) -> ParamId {
        self.id
    }

    pub const fn pad(self) -> Option<PadId> {
        self.at.pad()
    }

    /// The box's address of it: the selected pad's, for a pad's.
    pub const fn param(self) -> ParamAddr {
        ParamAddr::new(self.at.block(), self.id)
    }

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

/// The block `name` is the token of.
fn block_named(name: &[u8]) -> Option<EdBlock> {
    if let Some(rest) = name.strip_prefix(PAD) {
        return pad_named(rest);
    }
    if name == KIT.as_bytes() {
        return Some(EdBlock::Sound(SoundBlock(BlockRef::Kit)));
    }
    let b = BlockRef::ALL
        .into_iter()
        .find(|&b| token(b).is_some_and(|t| t.as_bytes() == name))?;
    Some(EdBlock::Sound(SoundBlock::new(b)?))
}

/// `<n><suffix>` after `PAD`: `n` 1–12, written without a leading zero.
fn pad_named(rest: &[u8]) -> Option<EdBlock> {
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    let (num, suffix) = rest.split_at(digits);
    if num.first() == Some(&b'0') {
        return None;
    }
    let n = chimera_theory::tuning::uint(num)?;
    let pad = PadId::ALL
        .into_iter()
        .find(|p| u32::from(p.number()) == n)?;
    let part = PadBlock::ALL
        .into_iter()
        .find(|p| p.suffix().as_bytes() == suffix)?;
    Some(EdBlock::Pad(pad, part))
}

impl fmt::Display for EdBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            EdBlock::Sound(s) => f.write_str(token(s.get()).ok_or(fmt::Error)?),
            EdBlock::Pad(n, p) => write!(f, "PAD{}{}", n.number(), p.suffix()),
        }
    }
}

impl fmt::Display for EdAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let spec = self.param().spec().ok_or(fmt::Error)?;
        write!(f, "{}.{}", self.at, spec.ident)
    }
}
