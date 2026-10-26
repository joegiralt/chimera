//! The NRPN address space (ADR 0216), derived from the frozen disk identity
//! (ADR 0045), never a second table: MSB = the block's disk code, LSB = the
//! `ParamId`. `tests/fixtures/nrpn_v1.txt` pins the chart. Bank 2.

use core::fmt;

use chimera_midi::law::ValueLaw;
use chimera_midi::value::{U7, U14};

use crate::addr::{BlockRef, ParamAddr};
use crate::block::{Block, ParamId, ParamKind, ParamSpec};
use crate::midi_map::Mappable;
use crate::storage::{MAX_BLOCK_PARAMS, RETIRED, ValidAddr};
use crate::theme_settings::ThemeSettings;

// MSB 64..=126 is reserved for global, system and FX spaces, 127 never used
// (127/127 is the null): every disk code < 64, every ParamId < 128. The
// probe's restore holds a block's stored params.
const _: () = {
    let mut i = 0;
    while i < BlockRef::ALL.len() {
        let b = BlockRef::ALL[i];
        if let Some(c) = b.disk_code() {
            assert!(c < 64, "a disk code reaches MSB 64, the reserved spaces");
        }
        let specs = b.specs();
        assert!(specs.len() <= MAX_BLOCK_PARAMS);
        let mut j = 0;
        while j < specs.len() {
            assert!(specs[j].id.0 < 128, "a ParamId past an NRPN LSB");
            j += 1;
        }
        i += 1;
    }
    let mut k = 0;
    while k < RETIRED.len() {
        assert!(RETIRED[k].0 < 64 && RETIRED[k].1 < 128);
        k += 1;
    }
};

/// An NRPN number. Any number off the wire is one; `addr` says what it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NrpnAddr(U14);

impl NrpnAddr {
    pub const fn new(v: U14) -> Self {
        Self(v)
    }

    /// `a`'s address. `None`: its block has no disk code (PartMix, Tempo),
    /// or it isn't a stored param.
    pub const fn of(a: ParamAddr) -> Option<Self> {
        let Some(code) = a.block.disk_code() else {
            return None;
        };
        match a.spec() {
            Some(s) if s.stored => pack(code, a.param.0),
            _ => None,
        }
    }

    /// The stored param this names. `None`: retired, reserved or unknown.
    pub fn addr(self) -> Option<ParamAddr> {
        let b = BlockRef::from_disk_code(self.0.msb().get())?;
        ValidAddr::find(b, ParamId(self.0.lsb().get())).map(ValidAddr::addr)
    }

    pub const fn get(self) -> U14 {
        self.0
    }
}

const fn pack(code: u8, id: u8) -> Option<NrpnAddr> {
    match (U7::new(code), U7::new(id)) {
        (Some(m), Some(l)) => Some(NrpnAddr(U14::from_parts(m, l))),
        _ => None,
    }
}

/// The value law. A choice (`Enum`) is stored as its frozen code, so it is
/// `Code7`, unless a code is past 7 bits (`WIDE_CODES`): then `Index7`. A
/// continuous param is `Place14`; any other is discrete, `Index7` up to 128
/// values, `Index14` past (TUNE). A change of law is a renumber.
pub fn law(block: BlockRef, spec: &ParamSpec) -> ValueLaw {
    match spec.kind {
        ParamKind::Enum if WIDE_CODES.contains(&(block, spec.id)) => ValueLaw::Index7,
        ParamKind::Enum => ValueLaw::Code7,
        ParamKind::Continuous => ValueLaw::Place14,
        ParamKind::Stepped if spec.max - spec.min < 128.0 => ValueLaw::Index7,
        ParamKind::Stepped => ValueLaw::Index14,
    }
}

/// Choices with a code an MSB can't carry: THEME BLACK's −2 and −1 are 254
/// and 255. `nrpn_chart_test` checks the list is exactly these.
pub const WIDE_CODES: &[(BlockRef, ParamId)] = &[(BlockRef::Theme, ThemeSettings::BLACK)];

/// The highest index of a discrete param (`max - min`): `place_of`'s `steps`.
pub fn steps(spec: &ParamSpec) -> u16 {
    libm::roundf(spec.max - spec.min) as u16
}

/// Phase 1 writes exactly ADR 0139's mappable set.
pub const fn writable(a: ParamAddr) -> Option<Mappable> {
    Mappable::new(a)
}

/// `Code7`'s lookup for `place_of`: the index at which `b` (the live block)
/// shows the value with frozen code `code`. Read through the block's own
/// decoder, so a code depending on another param (filter MODE on KIND) is
/// read in the block's context. `None`: no live value has the code.
///
/// The probe writes `b`; every stored param's raw value is written back
/// after, in spec order (the load order), by `write`, not through the
/// codes' load policy (TAPE SLICE loads as OFF), so `b` ends as it began.
pub fn index_of_code(b: &mut dyn Block, id: ParamId, code: U7) -> Option<u16> {
    let specs = b.specs();
    let min = specs.iter().find(|s| s.id == id)?.min;
    let mut saved = [0.0; MAX_BLOCK_PARAMS];
    for (s, x) in specs.iter().zip(saved.iter_mut()) {
        *x = b.get(s.id);
    }
    let at = b
        .set_enum_code(id, code.get())
        .then(|| libm::roundf(b.get(id) - min) as u16);
    for (s, &v) in specs.iter().zip(saved.iter()) {
        if s.stored {
            b.write(s.id, v);
        }
    }
    at
}

/// One line of the chart: a stored param, or a retired address that is
/// never reused.
#[derive(Clone, Copy, Debug)]
pub enum ChartLine {
    Live {
        at: NrpnAddr,
        block: BlockRef,
        spec: &'static ParamSpec,
    },
    Retired(NrpnAddr),
}

/// Every stored param of every block, in `BlockRef::ALL` and spec order,
/// then `storage::RETIRED`.
pub fn chart() -> impl Iterator<Item = ChartLine> {
    let live = BlockRef::ALL.into_iter().flat_map(|block| {
        ValidAddr::of_block(block).filter_map(move |v| {
            NrpnAddr::of(v.addr()).map(|at| ChartLine::Live {
                at,
                block,
                spec: v.spec(),
            })
        })
    });
    let gone = RETIRED
        .iter()
        .filter_map(|&(b, id)| pack(b, id).map(ChartLine::Retired));
    live.chain(gone)
}

const fn law_token(l: ValueLaw) -> &'static str {
    match l {
        ValueLaw::Place14 => "Place14",
        ValueLaw::Index7 => "Index7",
        ValueLaw::Index14 => "Index14",
        ValueLaw::Code7 => "Code7",
    }
}

impl ChartLine {
    pub const fn addr(&self) -> NrpnAddr {
        match self {
            ChartLine::Live { at, .. } | ChartLine::Retired(at) => *at,
        }
    }

    /// The line with its label and range, for the docs (`chimera-utils nrpn-chart`).
    pub const fn labelled(&self) -> Labelled<'_> {
        Labelled(self)
    }
}

/// `N <msb> <lsb> <BLOCK> <PARAM> <law> <W|R>`, or `X <msb> <lsb>`: the
/// fixture's frozen form.
impl fmt::Display for ChartLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let a = self.addr().get();
        let (m, l) = (a.msb().get(), a.lsb().get());
        match self {
            ChartLine::Live { block, spec, .. } => {
                let w = if writable(ParamAddr::new(*block, spec.id)).is_some() {
                    'W'
                } else {
                    'R'
                };
                write!(
                    f,
                    "N {m} {l} {} {} {} {w}",
                    block.disk_ident().unwrap_or("?"),
                    spec.ident,
                    law_token(law(*block, spec))
                )
            }
            ChartLine::Retired(_) => write!(f, "X {m} {l}"),
        }
    }
}

pub struct Labelled<'a>(&'a ChartLine);

impl fmt::Display for Labelled<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ChartLine::Live { spec, .. } => {
                write!(f, "{}  # {} {}..{}", self.0, spec.label, spec.min, spec.max)
            }
            ChartLine::Retired(_) => write!(f, "{}  # retired", self.0),
        }
    }
}
