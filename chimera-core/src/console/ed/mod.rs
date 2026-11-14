//! `ed`: the browser editor's namespace. `ed hello` here; the rest follow.

use core::fmt::{self, Write};

use super::answer::Text;
use super::{Arg, Out, PROTOCOL, Stalled, Unit, Word, Words};
use crate::ui::about_page::write_build_rev;
use chimera_engine::block::DiskCode;
use chimera_engine::params::EngineType;

mod addr;
pub use addr::EdAddr;

/// A value to set: a number, or a word (a choice's name).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdValue {
    Num(f32),
    Name(Word),
}

// `parse` only builds `Num` from a finite number, so `==` is total.
impl Eq for EdValue {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdArg {
    Hello,
    Describe { engine: EngineType, page: u8 },
    Get(EdAddr),
    Set(EdAddr, EdValue),
    Watch,
    Unwatch,
}

fn engine(w: &[u8]) -> Option<EngineType> {
    EngineType::ALL
        .into_iter()
        .find(|e| e.disk_ident().as_bytes().eq_ignore_ascii_case(w))
}

fn value(w: &[u8]) -> EdValue {
    match core::str::from_utf8(w).ok().and_then(|s| s.parse::<f32>().ok()) {
        Some(n) if n.is_finite() => EdValue::Num(n),
        _ => EdValue::Name(Word::new(w)),
    }
}

impl Arg for EdArg {
    const USAGE: &'static str = "hello, describe, get, set, watch or unwatch";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let sub = words.next()?;
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        let got = if is(b"hello") {
            EdArg::Hello
        } else if is(b"watch") {
            EdArg::Watch
        } else if is(b"unwatch") {
            EdArg::Unwatch
        } else if is(b"describe") {
            let engine = engine(words.next()?)?;
            let page = core::str::from_utf8(words.next()?).ok()?.parse().ok()?;
            EdArg::Describe { engine, page }
        } else if is(b"get") {
            EdArg::Get(EdAddr::parse(words.next()?)?)
        } else if is(b"set") {
            let a = EdAddr::parse(words.next()?)?;
            EdArg::Set(a, value(words.next()?))
        } else {
            return None;
        };
        words.next().is_none().then_some(got)
    }
}

impl EdArg {
    const fn sub(self) -> &'static str {
        match self {
            EdArg::Hello => "hello",
            EdArg::Describe { .. } => "describe",
            EdArg::Get(_) => "get",
            EdArg::Set(..) => "set",
            EdArg::Watch => "watch",
            EdArg::Unwatch => "unwatch",
        }
    }
}

/// The whole answer, `OK` or `ERR` included.
pub fn answer(arg: EdArg, unit: &mut impl Unit, out: &mut impl Out) -> Result<(), Stalled> {
    let mut t = Text(out);
    let wrote = match arg {
        EdArg::Hello => hello(unit, &mut t).and_then(|()| t.write_str("OK\n")),
        other => writeln!(t, "ERR ed {} is not in this build", other.sub()),
    };
    wrote.map_err(|fmt::Error| Stalled)
}

/// `ed hello <protocol> <build> part <n> <engine>`: the build is `status`'s.
fn hello(unit: &impl Unit, w: &mut impl Write) -> fmt::Result {
    let ui = unit.ui();
    write!(w, "ed hello {PROTOCOL} ")?;
    write_build_rev(w)?;
    let engine = ui.project().part(ui.active_part).sound.engine();
    writeln!(w, " part {} {}", ui.active_part.index() + 1, engine.disk_ident())
}
