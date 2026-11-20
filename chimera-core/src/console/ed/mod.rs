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
mod exact;
mod lit;
#[cfg(feature = "midi-map")]
mod map;
mod orbit;
mod pages;
mod project;
mod route;
mod schema;
mod settings;
mod since;
mod value;
pub use addr::{
    BusBlock, EdAddr, EdBlock, GlobalBlock, OrbitBlock, PadBlock, PartBlock, SoundBlock,
};
pub use describe::{
    MODELS, chain, env_variant, filter_variant, fx_variant, lfo_variant, machine_variant,
    modal_variant, op_variant, page_at, pages, reverb_variant, tape_variant,
};
#[cfg(any(test, feature = "test-support"))]
pub use edits::StoredRefusal;
pub use edits::{EdState, Pushed, REFUSED as ED_REFUSED, SetRefusal};
#[cfg(feature = "midi-map")]
pub(crate) use map::{KeepMap, RowIx};
pub(crate) use orbit::{
    NotNow as OrbitNotNow, OrbitCmd, Seen as OrbitSeen, check as orbit_check, holds as orbit_holds,
};
pub use pages::schema_pages;
pub use project::{Discard as EdDiscard, EdName, OverEdits, ProjectCmd};
pub use schema::{Round, Schema};
pub use since::{
    EdSince, Gen, MIXER_SLOTS as ED_MIXER_SLOTS, PANELS_MAX as ED_PANELS,
    SETTINGS_SLOTS as ED_SETTINGS_SLOTS, SLOTS as ED_SLOTS, panels_used as ed_panels_used,
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
    /// `ed projects`: the card listed again, for `since`'s listing.
    Projects,
    /// `ed project save|saveas|load …`.
    Project(project::ProjectCmd),
    /// `ed map add|set|rm|pick|keep|save|rename …`.
    #[cfg(feature = "midi-map")]
    Map(map::MapCmd),
    /// `ed maps`: the card's controller maps listed, for `since`.
    #[cfg(feature = "midi-map")]
    Maps,
    /// `ed learn <addr>|off`.
    #[cfg(feature = "midi-map")]
    Learn(map::LearnCmd),
    /// `ed orbit play|stop`, `ed scene …`, `ed chord|scale|step|pool …`.
    Orbit(orbit::OrbitCmd),
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
    /// `ed project` that is none of its forms, or a name NAMING couldn't
    /// type.
    Project(project::ProjectRefusal),
    /// `ed map` that is none of its forms.
    #[cfg(feature = "midi-map")]
    Map(map::MapRefusal),
    /// `ed learn` with no address or `off`.
    #[cfg(feature = "midi-map")]
    Learn,
    /// An ORBIT command that is none of its family's forms.
    Orbit(orbit::Family),
}

/// A decimal (`exact::read`'s, correctly rounded, #1038; not core's float
/// parse: that is 17 KB), or a choice's name of at most `WORD_MAX`. `inf` and `nan` are refused,
/// never names: `Num` is always finite.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.value"))]
fn value(w: &[u8]) -> Option<EdValue> {
    if let Some(n) = w.strip_prefix(b"#") {
        let n = chimera_theory::tuning::uint(n)?;
        return u8::try_from(n).ok().map(|n| EdValue::Index(OptIndex(n)));
    }
    if let Some(n) = exact::read(w) {
        return Some(EdValue::Num(n, Word::new(w)));
    }
    let bare = match w {
        [b'+' | b'-', rest @ ..] => rest,
        _ => w,
    };
    let not_finite = [INF.bytes(), INFINITY.bytes(), NAN.bytes()]
        .iter()
        .any(|n| bare.eq_ignore_ascii_case(n));
    (w.len() <= WORD_MAX && !not_finite).then(|| EdValue::Name(Word::new(w)))
}

impl EdArg {
    /// Never shown: `EdLine` takes every line, and `ed`'s refusals are
    /// bank 2's (`answer_in`, `ED_USAGE`), so bank 1 holds no list.
    pub const USAGE: &'static str = "a command";

    /// The command, or the refusal it gets. `Since`'s `None` is no
    /// failure: 0 is the editor holding nothing.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.parse"))]
    pub fn parse(mut words: Words<'_>) -> Result<Self, EdRefusal> {
        let Some(sub) = words.next() else {
            return Err(EdRefusal::Usage);
        };
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        if is(PART_WORD.bytes()) {
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
        if is(PROJECT_WORD.bytes()) {
            return project::ProjectCmd::parse(words)
                .map(EdArg::Project)
                .map_err(EdRefusal::Project);
        }
        #[cfg(feature = "midi-map")]
        if is(MAP_WORD.bytes()) {
            return map::MapCmd::parse(words)
                .map(EdArg::Map)
                .map_err(EdRefusal::Map);
        }
        #[cfg(feature = "midi-map")]
        if is(LEARN_WORD.bytes()) {
            return map::LearnCmd::parse(words)
                .map(EdArg::Learn)
                .ok_or(EdRefusal::Learn);
        }
        if let Some(f) = orbit::Family::of(sub) {
            return f.parse(words).map(EdArg::Orbit).ok_or(EdRefusal::Orbit(f));
        }
        EdArg::parse_rest(sub, words).ok_or(EdRefusal::Usage)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.mod.parse_rest"))]
    fn parse_rest(sub: &[u8], mut words: Words<'_>) -> Option<Self> {
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        #[cfg(feature = "midi-map")]
        if is(MAPS_WORD.bytes()) {
            return words.next().is_none().then_some(EdArg::Maps);
        }
        let got = if is(HELLO.bytes()) {
            EdArg::Hello
        } else if is(PROJECTS_WORD.bytes()) {
            EdArg::Projects
        } else if is(SINCE.bytes()) {
            let n = chimera_theory::tuning::uint(words.next()?)?;
            EdArg::Since(Gen::new(n))
        } else if is(DESCRIBE.bytes()) {
            let schema = Schema::parse(words.next()?)?;
            let page = chimera_theory::tuning::uint(words.next()?)?
                .try_into()
                .ok()?;
            EdArg::Describe { schema, page }
        } else if is(SCHEMA.get().trim_end().as_bytes()) {
            EdArg::Schema(Schema::parse(words.next()?)?)
        } else if is(GET.bytes()) {
            EdArg::Get(EdAddr::parse(words.next()?)?)
        } else if is(SET.bytes()) {
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
        Err(EdRefusal::Usage) => {
            w.write_str(TAKES.get())?;
            w.write_str(ED_USAGE.get())?;
            w.write_char('\n')
        }
        Err(EdRefusal::Orbit(f)) => f.usage(w),
        Err(EdRefusal::Route) => w.write_str(ROUTE_USAGE.get()),
        Err(EdRefusal::Project(project::ProjectRefusal::Usage)) => {
            w.write_str(project::USAGE.get())
        }
        Err(EdRefusal::Project(project::ProjectRefusal::Name)) => {
            w.write_str(project::NAME_USAGE.get())
        }
        #[cfg(feature = "midi-map")]
        Err(EdRefusal::Map(r)) => r.write(w),
        #[cfg(feature = "midi-map")]
        Err(EdRefusal::Learn) => w.write_str(map::LEARN_USAGE.get()),
        Err(EdRefusal::NoSuchPart) => {
            w.write_str(PART_TAKES.get())?;
            lit::uint(w, u32::from(PartId::FIRST.number()))?;
            w.write_char('-')?;
            lit::uint(w, u32::from(PartId::LAST.number()))?;
            w.write_char('\n')
        }
        Ok(EdArg::Hello) => hello(ui, w).and_then(|()| w.write_str("OK\n")),
        Ok(EdArg::Describe { schema, page }) => match describe::describe(schema, page, w) {
            Some(done) => done.and_then(|()| w.write_str("OK\n")),
            None => {
                w.write_str(DESCRIBE_ERR.get())?;
                schema.write(w)?;
                w.write_str(HAS.get())?;
                lit::uint(w, u32::from(schema_pages(schema)))?;
                w.write_str(PAGES.get())
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
            None => {
                w.write_str(GET_HAS_NO.get())?;
                a.write(w)?;
                w.write_str(lit::HERE.get())
            }
        },
        Ok(EdArg::Set(a, v)) => edits::set(a, v, ui, q, w),
        Ok(EdArg::Since(held)) => match q {
            Some(st) => st.since.answer(held, ui, &mut st.refused, w),
            None => w.write_str(SINCE_UNBUILT.get()),
        },
        Ok(EdArg::Route(c, to)) => edits::route(c, to, ui, q, w),
        Ok(EdArg::Projects) => edits::projects(q, w),
        Ok(EdArg::Project(c)) => edits::project(c, ui, q, w),
        #[cfg(feature = "midi-map")]
        Ok(EdArg::Map(c)) => edits::map(c, ui, q, w),
        #[cfg(feature = "midi-map")]
        Ok(EdArg::Maps) => edits::maps(q, w),
        #[cfg(feature = "midi-map")]
        Ok(EdArg::Learn(c)) => edits::learn(c, ui, q, w),
        Ok(EdArg::Orbit(c)) => edits::orbit(c, ui, q, w),
        Ok(EdArg::Part(p)) => match q {
            None => w.write_str(PART_UNBUILT.get()),
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
    let engine = ui.project().part(ui.active_part).sound.engine();
    w.write_str(HELLO_HEAD.get())?;
    lit::uint(w, u32::from(PROTOCOL))?;
    w.write_char(' ')?;
    w.write_str(VERSION)?;
    w.write_char('-')?;
    w.write_str(HASH)?;
    if DIRTY {
        w.write_char('+')?;
    }
    w.write_str(HELLO_PART.get())?;
    lit::uint(w, u32::from(ui.active_part.number()))?;
    w.write_char(' ')?;
    w.write_str(engine.disk_ident())?;
    w.write_char('\n')
}

lit!(INF = "inf");
lit!(INFINITY = "infinity");
lit!(NAN = "nan");
lit!(PART_WORD = "part");
lit!(PROJECTS_WORD = "projects");
lit!(PROJECT_WORD = "project");
#[cfg(feature = "midi-map")]
lit!(MAP_WORD = "map");
#[cfg(feature = "midi-map")]
lit!(MAPS_WORD = "maps");
#[cfg(feature = "midi-map")]
lit!(LEARN_WORD = "learn");
lit!(HELLO = "hello");
lit!(SINCE = "since");
lit!(DESCRIBE = "describe");
lit!(GET = "get");
lit!(SET = "set");
lit!(
    /// The usage refusal's head.
    TAKES = "ERR ed takes "
);
lit!(
    /// `ed`'s commands, the usage refusal's tail.
    ED_USAGE = "hello, schema, describe, get, set, route, since, part, projects, project, map, maps, learn, orbit, scene, chord, scale, step or pool"
);
lit!(PART_TAKES = "ERR ed part takes ");
lit!(DESCRIBE_ERR = "ERR ed describe ");
lit!(HAS = " has ");
lit!(PAGES = " pages\n");
lit!(GET_HAS_NO = "ERR ed get has no ");
lit!(SINCE_UNBUILT = "ERR ed since is not in this build\n");
lit!(PART_UNBUILT = "ERR ed part is not in this build\n");
lit!(
    /// `hello`'s answer's head, before the protocol.
    HELLO_HEAD = "ed hello "
);
lit!(
    /// After the build, before the followed Part's number.
    HELLO_PART = " part "
);
