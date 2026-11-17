//! `ed`: the browser editor's namespace. `ed hello` here; the rest follow.
//! Bank 2's, behind `EdDoor` (`crate::bank2`, ADR 0251): `bank2.x` links
//! this module there, so bank 1 keeps only `EdLine` and the door's stub.

use core::fmt::{self, Write};

use super::{Arg, EdLine, PROTOCOL, WORD_MAX, Word, Words};
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::about_page::{DIRTY, HASH, VERSION};
use chimera_engine::block::DiskCode;
use chimera_engine::params::EngineType;

mod addr;
mod describe;
mod edits;
mod since;
mod value;
pub use addr::EdAddr;
pub use describe::{env_variant, filter_variant, fx_variant, lfo_variant, op_variant, pages};
pub use edits::{EdState, Pushed};
pub use since::{EdSince, Gen, SLOTS as ED_SLOTS, slots_used as ed_slots_used};

/// A value to set: a number, or a word (a choice's name).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdValue {
    /// A decimal, and the word it was typed as: a choice's name may be one
    /// (CRSE's `1.00`).
    Num(f32, Word),
    Name(Word),
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
    /// The Part to follow; `None`: the word was no Part's number.
    Part(Option<PartId>),
}

fn engine(w: &[u8]) -> Option<EngineType> {
    EngineType::ALL
        .into_iter()
        .find(|e| e.disk_ident().as_bytes().eq_ignore_ascii_case(w))
}

/// A decimal (`chimera_theory`'s, not core's float parse: that is 17 KB),
/// or a choice's name of at most `WORD_MAX`. `inf` and `nan` are refused,
/// never names: `Num` is always finite.
fn value(w: &[u8]) -> Option<EdValue> {
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

impl Arg for EdArg {
    const USAGE: &'static str = "hello, describe, get, set, since or part";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let sub = words.next()?;
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        let got = if is(b"hello") {
            EdArg::Hello
        } else if is(b"since") {
            let n = chimera_theory::tuning::uint(words.next()?)?;
            EdArg::Since(Gen::new(n))
        } else if is(b"part") {
            // Numbered from 1, as the box's keys are.
            let n = words.next().and_then(chimera_theory::tuning::uint);
            let p = n.and_then(|n| u8::try_from(n).ok()?.checked_sub(1));
            EdArg::Part(p.and_then(PartId::new))
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
pub(crate) fn answer_in(
    line: &EdLine,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    match EdArg::parse(line.words()) {
        None => writeln!(w, "ERR ed takes {}", EdArg::USAGE),
        Some(EdArg::Hello) => hello(ui, w).and_then(|()| w.write_str("OK\n")),
        Some(EdArg::Describe { engine, page }) => match describe::describe(engine, page, w) {
            Some(done) => done.and_then(|()| w.write_str("OK\n")),
            None => writeln!(
                w,
                "ERR ed describe {} has {} pages",
                engine.disk_ident(),
                pages(engine)
            ),
        },
        Some(EdArg::Get(a)) => match value::get(a, ui, w) {
            Some(done) => done.and_then(|()| w.write_str("OK\n")),
            None => writeln!(w, "ERR ed get has no {} here", a),
        },
        Some(EdArg::Set(a, v)) => edits::set(a, v, ui, q, w),
        Some(EdArg::Since(held)) => match q {
            Some(st) => st.since.answer(held, ui, w),
            None => writeln!(w, "ERR ed since is not in this build"),
        },
        Some(EdArg::Part(p)) => match (p, q) {
            (None, _) => writeln!(w, "ERR ed part takes 1-{}", PartId::ALL.len()),
            (Some(_), None) => w.write_str("ERR ed part is not in this build\n"),
            (Some(p), Some(q)) => {
                q.follow(p);
                w.write_str("OK\n")
            }
        },
    }
}

/// `ed hello <protocol> <build> part <n> <engine>`: the build is `status`'s
/// as one word, `0.3.0-a1b2c3d4`, `+` after a dirty tree's hash.
fn hello(ui: &UiState, w: &mut dyn Write) -> fmt::Result {
    let dirty = if DIRTY { "+" } else { "" };
    let engine = ui.project().part(ui.active_part).sound.engine();
    writeln!(
        w,
        "ed hello {PROTOCOL} {VERSION}-{HASH}{dirty} part {} {}",
        ui.active_part.index() + 1,
        engine.disk_ident()
    )
}
