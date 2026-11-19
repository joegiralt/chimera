//! `ed`: the browser editor's namespace. `ed hello` here; the rest follow.
//! Bank 2's, behind `EdDoor` (`crate::bank2`, ADR 0251): `bank2.x` links
//! this module there, so bank 1 keeps only `EdLine` and the door's stub.

use core::fmt::{self, Write};

use super::{EdLine, PROTOCOL, WORD_MAX, Word, Words};
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::about_page::{DIRTY, HASH, VERSION};
use chimera_engine::block::DiskCode;
use chimera_engine::fmt_buf::Fnv64;
use lit::lit;

mod addr;
mod describe;
mod edits;
mod lit;
mod pages;
mod route;
mod schema;
mod since;
mod value;
pub use addr::{
    BusBlock, EdAddr, EdBlock, GlobalBlock, OrbitBlock, PadBlock, PartBlock, SoundBlock,
};
pub use describe::{
    MODELS, chain, env_variant, filter_variant, fx_variant, lfo_variant, machine_variant,
    modal_variant, op_variant, pages, reverb_variant, tape_variant,
};
pub use edits::{EdState, Pushed, SetRefusal};
pub use pages::schema_pages;
pub use schema::{Round, Schema};
pub use since::{
    EdSince, Gen, MIXER_SLOTS as ED_MIXER_SLOTS, PANELS_MAX as ED_PANELS, SLOTS as ED_SLOTS,
    panels_used as ed_panels_used, slots_used as ed_slots_used,
};

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
        schema: Schema,
        page: u8,
    },
    /// `ed schema <SCHEMA>`: its page count and hash.
    Schema(Schema),
    Get(EdAddr),
    Set(EdAddr, EdValue),
    /// The editor's last answer's number; `None`: it holds nothing.
    Since(Option<Gen>),
    /// The Part to follow.
    Part(PartId),
    /// `ed route <cell> <amount|off>`.
    Route(route::RouteCell, route::RouteTo),
}

/// Why a line is no `ed` command: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdRefusal {
    /// `ERR ed takes <USAGE>`.
    Usage,
    /// `ed part` with no Part's number: `ERR ed part takes 1-6`.
    NoSuchPart,
    /// `ed route` with no cell or amount: `ERR ed route takes <cell>
    /// <-127..127|off>`.
    Route,
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
    pub const USAGE: &'static str = "hello, schema, describe, get, set, route, since or part";

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
        if is(ROUTE_WORD.get().as_bytes()) {
            let c = words.next().and_then(route::RouteCell::parse);
            let to = words.next().and_then(route::RouteTo::parse);
            return match (c, to, words.next()) {
                (Some(c), Some(to), None) => Ok(EdArg::Route(c, to)),
                (Some(_) | None, Some(_) | None, Some(_) | None) => Err(EdRefusal::Route),
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
            let schema = Schema::parse(words.next()?)?;
            let page = chimera_theory::tuning::uint(words.next()?)?
                .try_into()
                .ok()?;
            EdArg::Describe { schema, page }
        } else if is(SCHEMA.get().trim_end().as_bytes()) {
            EdArg::Schema(Schema::parse(words.next()?)?)
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
        Err(EdRefusal::Route) => w.write_str(ROUTE_USAGE.get()),
        Err(EdRefusal::NoSuchPart) => writeln!(
            w,
            "ERR ed part takes {}-{}",
            PartId::FIRST.number(),
            PartId::LAST.number()
        ),
        Ok(EdArg::Hello) => hello(ui, w).and_then(|()| w.write_str("OK\n")),
        Ok(EdArg::Describe { schema, page }) => match describe::describe(schema, page, w) {
            Some(done) => done.and_then(|()| w.write_str("OK\n")),
            None => {
                w.write_str("ERR ed describe ")?;
                schema.write(w)?;
                writeln!(w, " has {} pages", schema_pages(schema))
            }
        },
        Ok(EdArg::Schema(s)) => {
            let hash = match q {
                Some(q) => q.hashes.of(s, || hash(s)),
                None => hash(s),
            };
            w.write_str(SCHEMA.get())?;
            s.write(w)?;
            w.write_char(' ')?;
            lit::uint(w, u32::from(schema_pages(s)))?;
            w.write_char(' ')?;
            hex16(hash, w)?;
            w.write_char('\n')?;
            w.write_str("OK\n")
        }
        Ok(EdArg::Get(a)) => match value::get(a, ui, w) {
            Some(done) => done.and_then(|()| w.write_str("OK\n")),
            None => writeln!(w, "ERR ed get has no {} here", a),
        },
        Ok(EdArg::Set(a, v)) => edits::set(a, v, ui, q, w),
        Ok(EdArg::Since(held)) => match q {
            Some(st) => st.since.answer(held, ui, w),
            None => writeln!(w, "ERR ed since is not in this build"),
        },
        Ok(EdArg::Route(c, to)) => edits::route(c, to, ui, q, w),
        Ok(EdArg::Part(p)) => match q {
            None => w.write_str("ERR ed part is not in this build\n"),
            Some(q) => {
                q.follow(p);
                w.write_str("OK\n")
            }
        },
    }
}

lit!(
    /// `ed schema`'s word and its answer's head.
    SCHEMA = "schema "
);

lit!(
    /// `ed route`'s word.
    ROUTE_WORD = "route"
);
lit!(
    /// `ed route`'s refusal.
    ROUTE_USAGE = "ERR ed route takes <cell> <-127..127|off>\n"
);

/// FNV-1a 64 of every byte `ed describe <s> 0..pages` writes, in order:
/// equal hashes are the same text, so an editor's cached schema can't
/// disagree with `describe`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.hash"))]
fn hash(s: Schema) -> u64 {
    let mut h = Fnv64::NEW;
    // A hasher never refuses a write.
    let _ = describe::describe_all(s, &mut h);
    h.0
}

/// `h` as sixteen lowercase hex digits (core's `LowerHex` would be bank
/// 1's to place).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.hex16"))]
fn hex16(h: u64, w: &mut dyn Write) -> fmt::Result {
    let mut i = 16;
    while i > 0 {
        i -= 1;
        let d = ((h >> (i * 4)) & 0xF) as u8;
        w.write_char(char::from(if d < 10 { b'0' + d } else { b'a' + d - 10 }))?;
    }
    Ok(())
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
