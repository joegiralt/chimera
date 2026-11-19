//! `ed`: the browser editor's namespace. `ed hello` here; the rest follow.
//! Bank 2's, behind `EdDoor` (`crate::bank2`, ADR 0251): `bank2.x` links
//! this module there, so bank 1 keeps only `EdLine` and the door's stub.

use core::fmt::{self, Write};

use super::{EdLine, PROTOCOL, WORD_MAX, Word, Words};
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::about_page::{DIRTY, HASH, VERSION};
use chimera_engine::block::DiskCode;
use chimera_engine::params::EngineType;

mod addr;
mod describe;
mod edits;
mod pages;
mod since;
mod value;
pub use addr::{EdAddr, EdBlock, PadBlock, SoundBlock};
pub use describe::{
    MODELS, chain, env_variant, filter_variant, fx_variant, lfo_variant, machine_variant,
    modal_variant, op_variant, page_at, pages, tape_variant,
};
#[cfg(any(test, feature = "test-support"))]
pub use edits::StoredRefusal;
pub use edits::{EdState, Pushed, SetRefusal};
pub use since::{
    EdSince, Gen, PANELS_MAX as ED_PANELS, SLOTS as ED_SLOTS, panels_used as ed_panels_used,
    slots_used as ed_slots_used,
};
#[cfg(any(test, feature = "test-support"))]
pub use value::stored;

/// A choice's option by its number, `#<n>` (`opt`'s index): a name with a
/// space (`GATE EXT`) is one word this way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptIndex(u8);

impl OptIndex {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.new"))]
    pub const fn new(n: u8) -> OptIndex {
        OptIndex(n)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.get"))]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A value to set: a number, or a word (a choice's name).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdValue {
    /// A decimal, and the word it was typed as: a choice's name may be one
    /// (CRSE's `1.00`).
    Num(f32, Word),
    Name(Word),
    /// `#<n>`: option `n` of a choice.
    Index(OptIndex),
}

// `parse` only builds `Num` from a finite number, so `==` is total.
impl Eq for EdValue {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdArg {
    Hello,
    Describe {
        engine: EngineType,
        page: u8,
    },
    Get(EdAddr),
    Set(EdAddr, EdValue),
    /// The editor's last answer's number; `None`: it holds nothing.
    Since(Option<Gen>),
    /// The Part to follow.
    Part(PartId),
}

/// Why a line is no `ed` command: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdRefusal {
    /// `ERR ed takes <USAGE>`.
    Usage,
    /// `ed part` with no Part's number: `ERR ed part takes 1-6`.
    NoSuchPart,
}

#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.engine"))]
fn engine(w: &[u8]) -> Option<EngineType> {
    EngineType::ALL
        .into_iter()
        .find(|e| e.disk_ident().as_bytes().eq_ignore_ascii_case(w))
}

/// A decimal (`chimera_theory`'s, not core's float parse: that is 17 KB),
/// or a choice's name of at most `WORD_MAX`. `inf` and `nan` are refused,
/// never names: `Num` is always finite.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.value"))]
fn value(w: &[u8]) -> Option<EdValue> {
    if let Some(n) = w.strip_prefix(b"#") {
        let n = chimera_theory::tuning::uint(n)?;
        return u8::try_from(n).ok().map(|n| EdValue::Index(OptIndex(n)));
    }
    if let Some(n) = chimera_theory::tuning::decimal(w) {
        return Some(EdValue::Num(n, Word::new(w)));
    }
    let bare = match w {
        [b'+' | b'-', rest @ ..] => rest,
        _ => w,
    };
    let not_finite = [&b"inf"[..], b"infinity", b"nan"]
        .iter()
        .any(|n| bare.eq_ignore_ascii_case(n));
    (w.len() <= WORD_MAX && !not_finite).then(|| EdValue::Name(Word::new(w)))
}

impl EdArg {
    pub const USAGE: &'static str = "hello, describe, get, set, since or part";

    /// The command, or the refusal it gets. `Since`'s `None` is no
    /// failure: 0 is the editor holding nothing.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.parse"))]
    pub fn parse(mut words: Words<'_>) -> Result<Self, EdRefusal> {
        let Some(sub) = words.next() else {
            return Err(EdRefusal::Usage);
        };
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        if is(b"part") {
            let n = words.next().and_then(chimera_theory::tuning::uint);
            let p = n
                .and_then(PartId::from_number)
                .ok_or(EdRefusal::NoSuchPart)?;
            return match words.next() {
                None => Ok(EdArg::Part(p)),
                Some(_) => Err(EdRefusal::Usage),
            };
        }
        EdArg::parse_rest(sub, words).ok_or(EdRefusal::Usage)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.parse_rest"))]
    fn parse_rest(sub: &[u8], mut words: Words<'_>) -> Option<Self> {
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        let got = if is(b"hello") {
            EdArg::Hello
        } else if is(b"since") {
            let n = chimera_theory::tuning::uint(words.next()?)?;
            EdArg::Since(Gen::new(n))
        } else if is(b"describe") {
            let engine = engine(words.next()?)?;
            let page = chimera_theory::tuning::uint(words.next()?)?
                .try_into()
                .ok()?;
            EdArg::Describe { engine, page }
        } else if is(b"get") {
            EdArg::Get(EdAddr::parse(words.next()?)?)
        } else if is(b"set") {
            let a = EdAddr::parse(words.next()?)?;
            EdArg::Set(a, value(words.next()?)?)
        } else {
            return None;
        };
        words.next().is_none().then_some(got)
    }
}

/// The whole answer, `OK` or `ERR` included: `EdDoor::answer`'s entry.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.answer_in"))]
pub(crate) fn answer_in(
    line: &EdLine,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    match EdArg::parse(line.words()) {
        Err(EdRefusal::Usage) => writeln!(w, "ERR ed takes {}", EdArg::USAGE),
        Err(EdRefusal::NoSuchPart) => writeln!(
            w,
            "ERR ed part takes {}-{}",
            PartId::FIRST.number(),
            PartId::LAST.number()
        ),
        Ok(EdArg::Hello) => hello(ui, w).and_then(|()| w.write_str("OK\n")),
        Ok(EdArg::Describe { engine, page }) => match describe::describe(engine, page, w) {
            Some(done) => done.and_then(|()| w.write_str("OK\n")),
            None => writeln!(
                w,
                "ERR ed describe {} has {} pages",
                engine.disk_ident(),
                pages(engine)
            ),
        },
        Ok(EdArg::Get(a)) => match value::get(a, ui, w) {
            Some(done) => done.and_then(|()| w.write_str("OK\n")),
            None => writeln!(w, "ERR ed get has no {} here", a),
        },
        Ok(EdArg::Set(a, v)) => edits::set(a, v, ui, q, w),
        Ok(EdArg::Since(held)) => match q {
            Some(st) => st.since.answer(held, ui, &mut st.refused, w),
            None => writeln!(w, "ERR ed since is not in this build"),
        },
        Ok(EdArg::Part(p)) => match q {
            None => w.write_str("ERR ed part is not in this build\n"),
            Some(q) => {
                q.follow(p);
                w.write_str("OK\n")
            }
        },
    }
}

/// `ed hello <protocol> <build> part <n> <engine>`: the build is `status`'s
/// as one word, `0.3.0-a1b2c3d4`, `+` after a dirty tree's hash.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.hello"))]
fn hello(ui: &UiState, w: &mut dyn Write) -> fmt::Result {
    let dirty = if DIRTY { "+" } else { "" };
    let engine = ui.project().part(ui.active_part).sound.engine();
    writeln!(
        w,
        "ed hello {PROTOCOL} {VERSION}-{HASH}{dirty} part {} {}",
        ui.active_part.number(),
        engine.disk_ident()
    )
}
