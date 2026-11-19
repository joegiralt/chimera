//! The mod matrix over `ed` (ADR 0289, slice 2): a route is a cell,
//! `<SRC>:<DEST>` (`LFO1:OP_C.CRSE`, `ENV1:FILTER.CUTOFF`, `OP5:FILTER.RESO`),
//! or FLT › MOD's jack knobs, `FM3IN` and `QMIN`, resolved in the UI pass
//! as the knob resolves them. `ed route <cell> <amount|off>` sets or
//! removes it through the grid's own writes (`ui::ed_cue::route`); `ed
//! describe MTX-<ENGINE> 0` is the matrix's grammar from its tables; `since`'s
//! ROUTES section reports every present cell, diffed by (source,
//! destination), never by column: the registry's columns shift.
//!
//! ```text
//! page 0 MTX of 1
//! src <SRC> "<label>"           (ROW_ORDER's order, OP5 and OP6 on ALGO)
//! col <FREQ|Q|VCA|FX> <addr>
//! amount -127 127
//! dests 16
//! ```

use core::fmt::{self, Write};

use super::EdAddr;
use super::addr::EdBlock;
use super::lit::{lit, uint};
use crate::addr::ParamAddr;
use crate::modulation::{FixedCol, MAX_MOD_DESTS, ModSource, ModState};
use crate::taps::{TapOp, TapRoutes};
use crate::ui::filter_panel::{JackSlot, RouteView};
use crate::ui::fmt::FmtBuf;
use crate::ui::mod_grid::{self, ROW_ORDER, RowSource};
use chimera_engine::params::EngineType;

lit!(
    /// A tap's source token: `OP5`, `OP6` (a `TapOp`'s tag has a space).
    OP = "OP"
);
lit!(
    /// FM3 IN's knob's cell, the jack param's ident.
    FM3IN = "FM3IN"
);
lit!(
    /// QM IN's.
    QMIN = "QMIN"
);
lit!(
    /// `ed route`'s word for removing a route.
    OFF = "off"
);
lit!(
    /// MTX's one page's head.
    MTX_PAGE = "page 0 MTX of 1\n"
);
lit!(
    /// A source row.
    SRC = "src "
);
lit!(
    /// A fixed column.
    COL = "col "
);
lit!(
    /// An amount's range and the registry's columns.
    AMOUNTS = "amount -127 127\ndests "
);
lit!(
    /// A route's line.
    ROUTE = "route "
);
lit!(
    /// A route gone.
    UNROUTE = "unroute "
);
lit!(
    /// ROUTES' header, whole.
    ROUTES = "routes "
);
lit!(
    /// A FLT › MOD knob's kind: a route amount.
    KIND_ROUTE = " route\n"
);
lit!(
    /// KEYBOARD's: OFF, HALF, FULL.
    KIND_KEYBD = " keybd\n"
);

/// A route's source: a matrix source, or on ALGO an operator's tap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSrc {
    Mod(ModSource),
    Tap(TapOp),
}

impl RouteSrc {
    /// The grid row it is.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.row"))]
    pub fn row(self) -> RowSource {
        match self {
            RouteSrc::Mod(s) => RowSource::Mod(s.index()),
            RouteSrc::Tap(t) => RowSource::Tap(t),
        }
    }

    /// `ENV1`…`TIMBRE` (the source's frozen `disk_ident`), `OP5`, `OP6`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.src_parse"))]
    fn parse(w: &[u8]) -> Option<RouteSrc> {
        if let Some(s) = ModSource::ALL
            .into_iter()
            .find(|s| s.disk_ident().as_bytes() == w)
        {
            return Some(RouteSrc::Mod(s));
        }
        let (head, n) = w.split_at_checked(OP.bytes().len())?;
        if head != OP.bytes() {
            return None;
        }
        match n {
            [b'5'] => Some(RouteSrc::Tap(TapOp::Op5)),
            [b'6'] => Some(RouteSrc::Tap(TapOp::Op6)),
            _ => None,
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.src_write"))]
    fn write(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            RouteSrc::Mod(s) => w.write_str(s.disk_ident()),
            RouteSrc::Tap(t) => {
                w.write_str(OP.get())?;
                w.write_char(match t {
                    TapOp::Op5 => '5',
                    TapOp::Op6 => '6',
                })
            }
        }
    }
}

/// A route's destination: a fixed column, or a registry param of the
/// Sound that the matrix may modulate. Only `RouteCell::parse` and the
/// Sound's own routes make one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteDest {
    Fixed(FixedCol),
    Reg(EdAddr),
}

impl RouteDest {
    /// The box's address it routes into.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.dest_addr"))]
    pub fn addr(self) -> ParamAddr {
        match self {
            RouteDest::Fixed(c) => c.addr(),
            RouteDest::Reg(a) => a.param(),
        }
    }

    /// The destination `a` is: its fixed column, or a Sound's modulatable
    /// param; `None` for anything else.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.dest_of"))]
    fn of(a: ParamAddr) -> Option<RouteDest> {
        if let Some(c) = FixedCol::of(a) {
            return Some(RouteDest::Fixed(c));
        }
        let e = EdAddr::sound(a)?;
        a.modulatable().then_some(RouteDest::Reg(e))
    }
}

/// A route the editor names: a matrix cell, or a FLT › MOD jack knob's
/// cell (whatever FM3 IN or QM IN names when the UI pass resolves it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteCell {
    Matrix { src: RouteSrc, dest: RouteDest },
    Jack(JackSlot),
}

impl RouteCell {
    /// `<SRC>:<DEST>`, `FM3IN` or `QMIN`. A tap reaches only a fixed
    /// column (`MatrixState::is_x`).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.parse"))]
    pub fn parse(w: &[u8]) -> Option<RouteCell> {
        if w == FM3IN.bytes() {
            return Some(RouteCell::Jack(JackSlot::Fm3));
        }
        if w == QMIN.bytes() {
            return Some(RouteCell::Jack(JackSlot::Qm));
        }
        let colon = w.iter().position(|&b| b == b':')?;
        let (s, d) = (&w[..colon], &w[colon + 1..]);
        let src = RouteSrc::parse(s)?;
        let a = EdAddr::parse(d)?;
        let dest = match a.at() {
            EdBlock::Sound(_) => RouteDest::of(a.param())?,
            EdBlock::Pad(..)
            | EdBlock::Part(..)
            | EdBlock::Bus(_)
            | EdBlock::Global(_)
            | EdBlock::Orbit(_) => return None,
        };
        match (src, dest) {
            (RouteSrc::Tap(_), RouteDest::Reg(_)) => None,
            (RouteSrc::Tap(_) | RouteSrc::Mod(_), _) => Some(RouteCell::Matrix { src, dest }),
        }
    }

    /// Whether a Part on `engine` has the cell: a tap's only on ALGO.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.on"))]
    pub fn on(self, engine: EngineType) -> bool {
        match self {
            RouteCell::Matrix {
                src: RouteSrc::Tap(_),
                ..
            } => engine == EngineType::Algo,
            RouteCell::Matrix {
                src: RouteSrc::Mod(_),
                ..
            }
            | RouteCell::Jack(_) => true,
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.write"))]
    pub fn write(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            RouteCell::Jack(JackSlot::Fm3) => w.write_str(FM3IN.get()),
            RouteCell::Jack(JackSlot::Qm) => w.write_str(QMIN.get()),
            RouteCell::Matrix { src, dest } => {
                src.write(w)?;
                w.write_char(':')?;
                match dest {
                    RouteDest::Reg(a) => write_addr(a, w),
                    RouteDest::Fixed(c) => match EdAddr::sound(c.addr()) {
                        Some(a) => write_addr(a, w),
                        None => Err(fmt::Error),
                    },
                }
            }
        }
    }

    /// The cell FLT › MOD's knob `r` views: a source's into FREQ, NOTE's
    /// (KEYBOARD), or a jack's.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.of_view"))]
    pub fn of_view(r: RouteView) -> RouteCell {
        let freq = RouteDest::Fixed(FixedCol::Freq);
        match r {
            RouteView::Freq(s) => RouteCell::Matrix {
                src: RouteSrc::Mod(s),
                dest: freq,
            },
            RouteView::Keybd => RouteCell::Matrix {
                src: RouteSrc::Mod(ModSource::Note),
                dest: freq,
            },
            RouteView::Jack(j) => RouteCell::Jack(j),
        }
    }
}

/// An address as `<BLOCK>.<IDENT>`: its `Display` would put a
/// `Formatter`'s `Write` in bank 1.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.route.write_addr")
)]
fn write_addr(a: EdAddr, w: &mut dyn Write) -> fmt::Result {
    write!(w, "{a}")
}

/// A route's amount, −127..=127, as the box stores it (`i8`, never −128).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Amount(i8);

impl Amount {
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.route.amount_new")
    )]
    pub fn new(a: i8) -> Option<Amount> {
        (a != i8::MIN).then_some(Amount(a))
    }

    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.route.amount_get")
    )]
    pub const fn get(self) -> i8 {
        self.0
    }

    /// `42`, `+42` or `-42`.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.route.amount_parse")
    )]
    fn parse(w: &[u8]) -> Option<Amount> {
        let (neg, digits) = match w {
            [b'-', rest @ ..] => (true, rest),
            [b'+', rest @ ..] => (false, rest),
            _ => (false, w),
        };
        let n = i32::try_from(chimera_theory::tuning::uint(digits)?).ok()?;
        let n = if neg { -n } else { n };
        Amount::new(i8::try_from(n).ok()?)
    }
}

/// What `ed route` sets a cell to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteTo {
    Amount(Amount),
    /// The route removed: the grid's MIX + MINUS.
    Off,
}

impl RouteTo {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.to_parse"))]
    pub fn parse(w: &[u8]) -> Option<RouteTo> {
        if w == OFF.bytes() {
            return Some(RouteTo::Off);
        }
        Amount::parse(w).map(RouteTo::Amount)
    }
}

/// `ed describe MTX-<engine> 0`: the matrix's grammar from its tables.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.describe"))]
pub(super) fn describe(engine: EngineType, w: &mut dyn Write) -> fmt::Result {
    w.write_str(MTX_PAGE.get())?;
    for s in sources(engine) {
        w.write_str(SRC.get())?;
        s.write(w)?;
        w.write_str(" \"")?;
        w.write_str(match s {
            RouteSrc::Mod(m) => m.tag(),
            RouteSrc::Tap(t) => t.tag(),
        })?;
        w.write_str("\"\n")?;
    }
    for c in FixedCol::ALL {
        w.write_str(COL.get())?;
        w.write_str(c.tag())?;
        w.write_char(' ')?;
        match EdAddr::sound(c.addr()) {
            Some(a) => write_addr(a, w)?,
            None => return Err(fmt::Error),
        }
        w.write_char('\n')?;
    }
    w.write_str(AMOUNTS.get())?;
    uint(w, MAX_MOD_DESTS as u32)?;
    w.write_char('\n')
}

/// `engine`'s sources in the grid's rows' order: `ROW_ORDER`, then the
/// taps on ALGO.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.sources"))]
fn sources(engine: EngineType) -> impl Iterator<Item = RouteSrc> {
    let taps: &[TapOp] = match engine {
        EngineType::Algo => &TapOp::ALL,
        EngineType::Modal | EngineType::Tape | EngineType::Va | EngineType::Drum => &[],
    };
    ROW_ORDER
        .into_iter()
        .filter_map(|i| ModSource::ALL.get(i).copied().map(RouteSrc::Mod))
        .chain(taps.iter().map(|&t| RouteSrc::Tap(t)))
}

/// FLT › MOD's knob line: `route "<label>" <cell> <route|keybd>`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.knob_line"))]
pub(super) fn knob_line(label: &str, r: RouteView, w: &mut dyn Write) -> fmt::Result {
    w.write_str(ROUTE.get())?;
    w.write_char('"')?;
    w.write_str(label)?;
    w.write_str("\" ")?;
    RouteCell::of_view(r).write(w)?;
    w.write_str(match r {
        RouteView::Keybd => KIND_KEYBD.get(),
        RouteView::Freq(_) | RouteView::Jack(_) => KIND_ROUTE.get(),
    })
}

/// A Part's routes as last reported: its matrix and taps.
#[derive(Clone)]
pub(super) struct Routes {
    pub(super) mods: ModState,
    pub(super) taps: TapRoutes,
}

impl Routes {
    /// Every present cell, with its amount, in the grid's order: each
    /// source's fixed columns, its registry columns, then the taps.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.each"))]
    pub(super) fn each(
        &self,
        engine: EngineType,
        mut f: impl FnMut(RouteCell, i8) -> fmt::Result,
    ) -> fmt::Result {
        let m = &self.mods;
        for s in sources(engine) {
            let RouteSrc::Mod(src) = s else {
                continue;
            };
            let i = src.index();
            for c in FixedCol::ALL {
                if m.fixed_present(c) & 1 << i != 0 {
                    f(cell(s, RouteDest::Fixed(c)), m.fixed_amount(i, c))?;
                }
            }
            for d in 0..m.num_dests() {
                if m.present(d) & 1 << i != 0
                    && let Some(dest) = RouteDest::of(m.dest(d))
                {
                    f(cell(s, dest), m.amount(i, d))?;
                }
            }
        }
        if engine == EngineType::Algo {
            for t in TapOp::ALL {
                for c in FixedCol::ALL {
                    if let Some(a) = self.taps.get(t, c) {
                        f(cell(RouteSrc::Tap(t), RouteDest::Fixed(c)), a)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Cell `c`'s amount, if it is present.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.get"))]
    pub(super) fn get(&self, c: RouteCell) -> Option<i8> {
        let RouteCell::Matrix { src, dest } = c else {
            return None;
        };
        let m = &self.mods;
        match (src, dest) {
            (RouteSrc::Tap(t), RouteDest::Fixed(f)) => self.taps.get(t, f),
            (RouteSrc::Tap(_), RouteDest::Reg(_)) => None,
            (RouteSrc::Mod(s), RouteDest::Fixed(f)) => {
                let i = s.index();
                (m.fixed_present(f) & 1 << i != 0).then(|| m.fixed_amount(i, f))
            }
            (RouteSrc::Mod(s), RouteDest::Reg(a)) => {
                let i = s.index();
                let d = m.find(a.param())?;
                (m.present(d) & 1 << i != 0).then(|| m.amount(i, d))
            }
        }
    }
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.cell"))]
fn cell(src: RouteSrc, dest: RouteDest) -> RouteCell {
    RouteCell::Matrix { src, dest }
}

/// `route <cell> <amount> "<text>"`, the box's text (`fmt_amount`).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.line"))]
pub(super) fn line(c: RouteCell, a: i8, w: &mut dyn Write) -> fmt::Result {
    w.write_str(ROUTE.get())?;
    c.write(w)?;
    w.write_char(' ')?;
    if a < 0 {
        w.write_char('-')?;
    }
    uint(w, u32::from(a.unsigned_abs()))?;
    let mut text = FmtBuf::new();
    mod_grid::fmt_amount(&mut text, a);
    w.write_str(" \"")?;
    w.write_str(text.as_str())?;
    w.write_str("\"\n")
}

/// `unroute <cell>`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.unline"))]
pub(super) fn unline(c: RouteCell, w: &mut dyn Write) -> fmt::Result {
    w.write_str(UNROUTE.get())?;
    c.write(w)?;
    w.write_char('\n')
}

/// `routes <n>`, ROUTES whole.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.route.header"))]
pub(super) fn header(part: crate::project::PartId, w: &mut dyn Write) -> fmt::Result {
    w.write_str(ROUTES.get())?;
    uint(w, u32::from(part.number()))?;
    w.write_char('\n')
}
