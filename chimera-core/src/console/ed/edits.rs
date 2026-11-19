//! `ed set <addr> <value>`: the value resolved here, bank 2's, queued for
//! the shell's UI pass, which applies it as a knob turn (`UiState::edit_to`,
//! ADR 0112's glide): never a snap, never a zipper. A pad's lands on that
//! pad, whichever the box has selected (ADR 0285); a Part's on that Part,
//! whichever the box shows (ADR 0289).

use core::fmt::{self, Write};

use super::addr::EdBlock;
use super::schema::Hashes;
use super::value::{engine_of, find};
use super::{EdAddr, EdValue, OptIndex};
use crate::addr::BlockRead;
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::kinds::OpKind;
use crate::project::Line;
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::page::ValFmt;
use crate::ui::{ed_cue, held, view};
use chimera_engine::params::EngineType;

/// Sets waiting for the next UI pass. A lap of a browser's sliders is a
/// handful; more than this is the host outrunning the pass.
pub const CAP: usize = 16;

/// What a set was taken for, so it lands there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Taken {
    /// The followed Part's Sound (or kit, or a pad), on `engine`: a Part
    /// that changed engine since drops it.
    Sound { part: PartId, engine: EngineType },
    /// Part `n`'s mix or sends, whichever Part the box shows.
    Part(PartId),
    /// A bus: the mixer's, the same from any Part.
    Bus,
}

impl Taken {
    /// What a set of `at` is taken for while following `part` on
    /// `engine`; `None` for a block no set reaches yet.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.of"))]
    fn of(at: EdBlock, part: PartId, engine: EngineType) -> Option<Taken> {
        match at {
            EdBlock::Sound(_) | EdBlock::Pad(..) => Some(Taken::Sound { part, engine }),
            EdBlock::Part(n, _) => Some(Taken::Part(n)),
            EdBlock::Bus(_) => Some(Taken::Bus),
            EdBlock::Global(_) | EdBlock::Orbit(_) => None,
        }
    }

    /// The Part it lands on now; `None`: it is dropped.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.lands"))]
    fn lands(self, ui: &UiState) -> Option<PartId> {
        match self {
            Taken::Sound { part, engine } => {
                (ui.project().part(part).sound.engine() == engine).then_some(part)
            }
            Taken::Part(n) => Some(n),
            Taken::Bus => Some(ui.active_part),
        }
    }
}

#[derive(Clone, Copy)]
struct Pending {
    taken: Taken,
    addr: EdAddr,
    to: To,
}

impl Pending {
    /// A slot nothing is queued in (`len` says so).
    const SPARE: Pending = Pending {
        taken: Taken::Bus,
        addr: EdAddr::SPARE,
        to: To::Num(0.0),
    };
}

/// A set's value: a choice's option, or a continuous param's number.
#[derive(Clone, Copy, Debug, PartialEq)]
enum To {
    Opt(OptIndex),
    Num(f32),
}

impl To {
    /// As the cell stores it.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.stored"))]
    fn stored(self) -> f32 {
        match self {
            To::Opt(i) => f32::from(i.get()),
            To::Num(v) => v,
        }
    }
}

/// What `EdState::push` made of a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    Queued,
    Full,
}

/// What the console keeps for `ed`, which the shell owns: the sets taken
/// and not yet applied by the UI pass (as `Hands` queues `Gesture`s, a fixed
/// array), and `ed since`'s copy of what it last reported.
pub struct EdState {
    items: [Pending; CAP],
    len: u8,
    /// `ed part`'s Part, for the same pass: the last asked wins.
    part: Option<PartId>,
    /// `ed since`'s copy and number (`since.rs`).
    pub(super) since: super::EdSince,
    /// `ed schema`'s hashes, made once a boot.
    pub(super) hashes: Hashes,
}

impl EdState {
    /// Sets the queue holds.
    pub const CAP: usize = CAP;

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.new"))]
    pub const fn new() -> Self {
        EdState {
            items: [Pending::SPARE; CAP],
            len: 0,
            part: None,
            since: super::EdSince::new(),
            hashes: Hashes::new(),
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.push"))]
    fn push(&mut self, taken: Taken, addr: EdAddr, to: To) -> Pushed {
        match self.items.get_mut(usize::from(self.len)) {
            None => Pushed::Full,
            Some(slot) => {
                *slot = Pending { taken, addr, to };
                self.len += 1;
                Pushed::Queued
            }
        }
    }

    /// Follow `p` on the next UI pass, as its key would.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.follow"))]
    pub fn follow(&mut self, p: PartId) {
        self.part = Some(p);
    }

    /// Every queued set, oldest first, as a knob turn on the Part it was
    /// taken for (a Part that has changed engine since drops its Sound's
    /// sets), then the Part to follow: the sets were taken for theirs.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.apply"))]
    pub fn apply(&mut self, ui: &mut UiState) {
        let mut i = 0;
        while i < usize::from(self.len) {
            if let Some(&p) = self.items.get(i)
                && let Some(at) = p.taken.lands(ui)
            {
                turn(ui, at, p);
            }
            i += 1;
        }
        self.len = 0;
        if let Some(p) = self.part.take() {
            ui.follow_part(p);
        }
    }
}

/// `p` as a knob turn. A pad's is the selected pad's turn with its pad
/// selected for it, and the box's selection put back: the box never sees
/// it move, and the edit takes the knob's whole path (ADR 0280's CHOKE
/// check with it).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.turn"))]
fn turn(ui: &mut UiState, at: PartId, p: Pending) {
    let (a, pad) = (p.addr.param(), p.addr.pad());
    if let Some(why) = refusal(ui, at, p) {
        // To the value it has: no change, the hold kept, nothing said.
        if !unchanged(ui, at, p) {
            ed_cue::refused(ui, at, a, pad, why);
        }
        return;
    }
    let to = p.to.stored();
    match pad {
        None => {
            ui.edit_to(at, a, to);
        }
        Some(n) => {
            let was = ui.project().selected_pad(at);
            ui.project_mut().select_pad(at, n);
            ui.edit_to(at, a, to);
            ui.project_mut().select_pad(at, was);
        }
    }
    ed_cue::landed(ui, at, a, pad);
}

/// `p` sets its Sound cell on Part `at` to the value stored there already.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.unchanged"))]
fn unchanged(ui: &UiState, at: PartId, p: Pending) -> bool {
    let a = p.addr.param();
    let params = &ui.project().part(at).sound.params;
    params
        .block(a.block)
        .is_some_and(|b| b.get(a.param) == p.to.stored())
}

/// Why the box refuses `p` before its write, as its toast reads: TAPE's
/// MODEL or DIRECTION onto what a streamed source can't play (the turn's
/// and a CC's check), or an operator kind or algorithm past the formant
/// budget (a CC's words). CHOKE refuses in `edit_in`, with its own toast
/// (ADR 0280); `None`: the write goes ahead.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.refusal"))]
fn refusal(ui: &UiState, at: PartId, p: Pending) -> Option<Line> {
    // Each cell refused here is a choice: a number never reaches one.
    let To::Opt(i) = p.to else {
        return None;
    };
    let a = p.addr.param();
    let sound = &ui.project().part(at).sound;
    let src = ui.streams(at);
    if let Some(t) = held::TapeTurn::of(a, p.to.stored()) {
        return held::Pick::Tape(t).why(sound, src).map(held::reason);
    }
    let pick = match held::applies(a, false)? {
        held::Cell::Wave(op) => held::Pick::Wave(op, OpKind::clamped(i.get())),
        held::Cell::Alg(side) => held::Pick::Alg(side, AlgoId::clamped(i.get())),
    };
    pick.why(sound, src).map(|_| ed_cue::formant_full())
}

/// Why `ed set` can't take a value: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetRefusal {
    /// `#n` past the choice's last option.
    NoSuchOption,
    /// A name or `#n` on a continuous param.
    TakesANumber,
    /// A word that names none of the choice's options.
    TakesAName,
}

impl SetRefusal {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.words"))]
    pub const fn words(self) -> &'static str {
        match self {
            SetRefusal::NoSuchOption => "has no such option",
            SetRefusal::TakesANumber => "takes a number",
            SetRefusal::TakesAName => "takes one of its names",
        }
    }
}

impl Default for EdState {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.default"))]
    fn default() -> Self {
        Self::new()
    }
}

/// `a`'s format as the Part shows it now: its cell's where a page shows
/// it, else its spec's (as `ed get` reads it).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.fmt_of"))]
fn fmt_of(ui: &UiState, a: EdAddr) -> Option<ValFmt> {
    let own = ui.cell_own();
    match find(ui, &own, a) {
        Some((def, i, ctx)) => Some(view::view(def, i, &ctx).fmt()),
        None => Some(a.param().spec()?.fmt),
    }
}

/// The stored value `val` means for `a`, or why not: a choice's name (its
/// own, matched exactly, the typed word kept), its option number, or a
/// number.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.resolve"))]
fn resolve(ui: &UiState, a: EdAddr, val: EdValue) -> Result<To, SetRefusal> {
    let names = fmt_of(ui, a).and_then(super::describe::names);
    match (val, names) {
        (EdValue::Index(n), Some(names)) => match names.get(usize::from(n.get())) {
            Some(_) => Ok(To::Opt(n)),
            None => Err(SetRefusal::NoSuchOption),
        },
        (EdValue::Index(_), None) => Err(SetRefusal::TakesANumber),
        (EdValue::Num(_, word) | EdValue::Name(word), Some(names)) => {
            let mut i = 0;
            while i < names.len() {
                if names[i].as_bytes() == word.as_bytes() {
                    return u8::try_from(i)
                        .map(|n| To::Opt(OptIndex::new(n)))
                        .map_err(|_| SetRefusal::NoSuchOption);
                }
                i += 1;
            }
            Err(SetRefusal::TakesAName)
        }
        (EdValue::Num(n, _), None) => Ok(To::Num(n)),
        (EdValue::Name(_), None) => Err(SetRefusal::TakesANumber),
    }
}

/// The answer, `OK queued` or `ERR`: whole, for `answer_in`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.set"))]
pub(super) fn set(
    addr: EdAddr,
    val: EdValue,
    ui: &UiState,
    q: Option<&mut EdState>,
    w: &mut dyn Write,
) -> fmt::Result {
    let part = ui.active_part;
    let engine = engine_of(ui);
    let Some(q) = q else {
        return w.write_str("ERR ed set is not in this build\n");
    };
    let taken = Taken::of(addr.at(), part, engine);
    let Some(taken) = taken.filter(|_| q.since.lists(engine, addr)) else {
        return writeln!(w, "ERR ed set has no {addr} here");
    };
    let to = match resolve(ui, addr, val) {
        Ok(to) => to,
        Err(why) => return writeln!(w, "ERR ed set {addr} {}", why.words()),
    };
    match q.push(taken, addr, to) {
        Pushed::Full => w.write_str("ERR busy\n"),
        Pushed::Queued => w.write_str("OK queued\n"),
    }
}
