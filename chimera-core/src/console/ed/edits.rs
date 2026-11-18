//! `ed set <addr> <value>`: the value resolved here, bank 2's, queued for
//! the shell's UI pass, which applies it as a knob turn (`UiState::edit_to`,
//! ADR 0112's glide): never a snap, never a zipper. A pad's lands on that
//! pad, whichever the box has selected (ADR 0285).

use core::fmt::{self, Write};

use super::value::{engine_of, find};
use super::{EdAddr, EdValue};
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::page::ValFmt;
use crate::ui::view;
use chimera_engine::params::EngineType;

/// Sets waiting for the next UI pass. A lap of a browser's sliders is a
/// handful; more than this is the host outrunning the pass.
pub const CAP: usize = 16;

#[derive(Clone, Copy)]
struct Pending {
    part: PartId,
    /// The engine the set was taken for: a Part that changed engine since
    /// drops it.
    engine: EngineType,
    addr: EdAddr,
    to: f32,
}

impl Pending {
    /// A slot nothing is queued in (`len` says so).
    const SPARE: Pending = Pending {
        part: PartId::ALL[0],
        engine: EngineType::Algo,
        addr: EdAddr::SPARE,
        to: 0.0,
    };
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
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.push"))]
    fn push(&mut self, part: PartId, engine: EngineType, addr: EdAddr, to: f32) -> Pushed {
        match self.items.get_mut(usize::from(self.len)) {
            None => Pushed::Full,
            Some(slot) => {
                *slot = Pending {
                    part,
                    engine,
                    addr,
                    to,
                };
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
    /// taken for (a Part that has changed engine since drops its sets),
    /// then the Part to follow: the sets were taken for theirs.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.edits.apply"))]
    pub fn apply(&mut self, ui: &mut UiState) {
        let mut i = 0;
        while i < usize::from(self.len) {
            if let Some(&p) = self.items.get(i)
                && ui.project().part(p.part).sound.engine() == p.engine
            {
                turn(ui, p);
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
fn turn(ui: &mut UiState, p: Pending) {
    match p.addr.pad() {
        None => {
            ui.edit_to(p.part, p.addr.param(), p.to);
        }
        Some(n) => {
            let was = ui.project().selected_pad(p.part);
            ui.project_mut().select_pad(p.part, n);
            ui.edit_to(p.part, p.addr.param(), p.to);
            ui.project_mut().select_pad(p.part, was);
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
fn resolve(ui: &UiState, a: EdAddr, val: EdValue) -> Result<f32, &'static str> {
    let names = fmt_of(ui, a).and_then(|f| super::describe::names(f));
    match (val, names) {
        (EdValue::Index(n), Some(names)) => match names.get(usize::from(n.get())) {
            Some(_) => Ok(f32::from(n.get())),
            None => Err("has no such option"),
        },
        (EdValue::Index(_), None) => Err("takes a number"),
        (EdValue::Num(_, word) | EdValue::Name(word), Some(names)) => {
            let mut i = 0;
            while i < names.len() {
                if names[i].as_bytes() == word.as_bytes() {
                    return Ok(i as f32);
                }
                i += 1;
            }
            Err("takes one of its names")
        }
        (EdValue::Num(n, _), None) => Ok(n),
        (EdValue::Name(_), None) => Err("takes a number"),
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
    if !q.since.described(engine).has(addr) {
        return writeln!(w, "ERR ed set has no {addr} here");
    }
    let to = match resolve(ui, addr, val) {
        Ok(to) => to,
        Err(why) => return writeln!(w, "ERR ed set {addr} {why}"),
    };
    match q.push(part, engine, addr, to) {
        Pushed::Full => w.write_str("ERR busy\n"),
        Pushed::Queued => w.write_str("OK queued\n"),
    }
}
