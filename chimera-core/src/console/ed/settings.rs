// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SETTINGS (ADR 0289, slice 3). Its pages: the tree's leaves with a
//! param, in tree order, but the read-only ones and DEMO's, each with its
//! crumbs, a static built from `settings::tree::ROOT` at compile time, in
//! bank 2: the editor lays SETTINGS out as the box's lists from the
//! crumbs, with no list of its own. And `since`'s SETTINGS lines: the
//! project's and each Part's state and engine, the toast, the listing.

use core::fmt::{self, Write};

use super::lit::{QUOTE, QUOTE_SHUT, UNQUOTE, lit, text, uint};
use crate::project::{Line, PartId, PartStatus, ProjectStatus};
use crate::storage::{FileError, Generation};
use crate::ui::UiState;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::block_registry::DEMO_BLOCKS;
use crate::ui::settings::listing::Listing;
use crate::ui::settings::tree::{Kind, ROOT, Row};

/// A leaf the editor describes: its list's crumb, its own, and its page.
#[derive(Clone, Copy)]
pub struct Leaf {
    pub list: &'static str,
    pub row: &'static str,
    pub def: &'static BlockDef,
}

/// A leaf with something to edit: a cell bound to a param. A leaf of
/// legacy cells alone (ABOUT, AUDIO LOAD, POOL) is read-only
/// (`OnePage::read_only`, a test holds the two equal).
const fn edits(def: &BlockDef) -> bool {
    let mut i = 0;
    while i < def.params.len() {
        if let SlotBinding::Param(_) = def.params[i].binding {
            return true;
        }
        i += 1;
    }
    false
}

/// One of DIAGNOSTICS › DEMO's pages, by id.
const fn demo(def: &BlockDef) -> bool {
    let mut i = 0;
    while i < DEMO_BLOCKS.len() {
        if DEMO_BLOCKS[i].def.id == def.id {
            return true;
        }
        i += 1;
    }
    false
}

/// The leaves under `rows` from the `n`th on, written into `out` while it
/// has room; how many there are.
const fn fill(rows: &'static [Row], list: &'static str, out: &mut [Leaf], mut n: usize) -> usize {
    let mut i = 0;
    while i < rows.len() {
        let r = &rows[i];
        match r.kind {
            Kind::List(sub) => n = fill(sub, r.crumb, out, n),
            Kind::Leaf(p) => {
                let def = p.def();
                if edits(def) && !demo(def) {
                    if n < out.len() {
                        out[n] = Leaf {
                            list,
                            row: r.crumb,
                            def,
                        };
                    }
                    n += 1;
                }
            }
            Kind::Screen(_) | Kind::Act(_) | Kind::Later(_) => {}
        }
        i += 1;
    }
    n
}

/// The tree's top rows.
const TOP: &[Row] = match ROOT.kind {
    Kind::List(rows) => rows,
    Kind::Leaf(_) | Kind::Screen(_) | Kind::Act(_) | Kind::Later(_) => &[],
};

/// How many leaves SETTINGS describes.
const COUNT: usize = {
    let mut none: [Leaf; 0] = [];
    fill(TOP, ROOT.crumb, &mut none, 0)
};

/// SETTINGS' pages, in tree order.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.settings.leaves"))]
pub static LEAVES: [Leaf; COUNT] = {
    let mut out = [Leaf {
        list: "",
        row: "",
        def: &crate::ui::block_registry::SYS_THEME,
    }; COUNT];
    fill(TOP, ROOT.crumb, &mut out, 0);
    out
};

/// `def`'s leaf, if SETTINGS describes it.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.settings.leaf_of")
)]
pub(super) fn leaf_of(def: &BlockDef) -> Option<&'static Leaf> {
    LEAVES.iter().find(|l| core::ptr::eq(l.def, def))
}

/// `crumb "<LIST>" "<ROW>"`: the crumbs the box's breadcrumb shows.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.settings.crumb"))]
pub(super) fn crumb(l: &Leaf, w: &mut dyn Write) -> fmt::Result {
    w.write_str(CRUMB.get())?;
    text(w, l.list)?;
    w.write_str(QUOTE_SHUT.get())?;
    w.write_char('"')?;
    text(w, l.row)?;
    w.write_str(UNQUOTE.get())
}

lit!(
    /// A SETTINGS page's crumbs' line head, its list's quote open.
    CRUMB = "crumb \""
);

/// `project "<name>" <id|new> <clean|edited> gen <g|->`: the loaded
/// project, its file's id, whether it has edits since its save or load
/// (`status`, `project_status`'s), and its file's generation as the
/// listing read it (`-` unlisted or new).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.settings.project_line")
)]
pub(super) fn project_line(ui: &UiState, status: ProjectStatus, w: &mut dyn Write) -> fmt::Result {
    let meta = ui.project().meta();
    w.write_str(PROJECT.get())?;
    text(w, meta.name().as_str())?;
    w.write_str(QUOTE_SHUT.get())?;
    match meta.id() {
        Some(id) => uint(w, id.get())?,
        None => w.write_str(NEW.get())?,
    }
    w.write_char(' ')?;
    w.write_str(match status {
        ProjectStatus::Pristine | ProjectStatus::Saved => CLEAN.get(),
        ProjectStatus::Modified => EDITED.get(),
    })?;
    let l = ui.listing();
    let mut i = 0;
    let mut generation = None;
    while let Some(e) = l.entry(i) {
        if Some(e.file()) == meta.file() {
            generation = e.generation;
        }
        i += 1;
    }
    generation_word(generation, w)?;
    w.write_char('\n')
}

/// ` gen <g>`, or ` gen -` for none.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.settings.generation_word")
)]
fn generation_word(g: Option<Generation>, w: &mut dyn Write) -> fmt::Result {
    w.write_str(GEN.get())?;
    match g {
        Some(g) => uint(w, g.get()),
        None => w.write_char('-'),
    }
}

/// `sound <n> "<name>" <clean|edited|stale>`: Part `p`'s Sound against its
/// slot, `status` as the PART strip reads it (`part_status`).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.settings.sound_line")
)]
pub(super) fn sound_line(
    ui: &UiState,
    p: PartId,
    status: PartStatus,
    w: &mut dyn Write,
) -> fmt::Result {
    let part = ui.project().part(p);
    w.write_str(SOUND.get())?;
    uint(w, u32::from(p.number()))?;
    w.write_str(QUOTE.get())?;
    text(w, part.sound.name.as_str())?;
    w.write_str(QUOTE_SHUT.get())?;
    w.write_str(match status {
        PartStatus::Clean => CLEAN.get(),
        // The Sound's, the kit's or both (ADR 0305): one word on the wire.
        PartStatus::Edited(_) => EDITED.get(),
        PartStatus::Stale(_) => STALE.get(),
    })?;
    w.write_char('\n')
}

/// `engine <n> <ENGINE>`: Part `p`'s engine by its disk token, every
/// Part's, so the editor names a Part it doesn't follow (`part <n>
/// <ENGINE>` is the followed one's alone).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.settings.engine_line")
)]
pub(super) fn engine_line(ui: &UiState, p: PartId, w: &mut dyn Write) -> fmt::Result {
    use crate::block::DiskCode;
    w.write_str(ENGINE.get())?;
    uint(w, u32::from(p.number()))?;
    w.write_char(' ')?;
    w.write_str(ui.project().part(p).sound.engine().disk_ident())?;
    w.write_char('\n')
}

/// `toast "<text>"`.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.settings.toast_line")
)]
pub(super) fn toast_line(t: &Line, w: &mut dyn Write) -> fmt::Result {
    w.write_str(TOAST.get())?;
    text(w, t.as_str())?;
    w.write_str(UNQUOTE.get())
}

/// The listing whole: `projects <n>[ more]`, then `proj <id> "<name>"
/// <ok|bad|newer> gen <g|->` for each, in id order (at most `MAX_LISTED`).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.settings.listing")
)]
pub(super) fn listing(l: &Listing, w: &mut dyn Write) -> fmt::Result {
    w.write_str(PROJECTS.get())?;
    uint(w, u32::try_from(l.len()).map_err(|_| fmt::Error)?)?;
    if l.more() {
        w.write_str(MORE.get())?;
    }
    w.write_char('\n')?;
    let mut i = 0;
    while let Some(e) = l.entry(i) {
        i += 1;
        w.write_str(PROJ.get())?;
        uint(w, e.id.get())?;
        w.write_str(QUOTE.get())?;
        if let Some(n) = e.name {
            text(w, n.as_str())?;
        }
        w.write_str(QUOTE_SHUT.get())?;
        w.write_str(match e.err {
            None => OK.get(),
            Some(FileError::NeedsNewerFirmware) => NEWER.get(),
            Some(_) => BAD.get(),
        })?;
        generation_word(e.generation, w)?;
        w.write_char('\n')?;
    }
    Ok(())
}

lit!(
    /// The project's line's head, its name's quote open.
    PROJECT = "project \""
);
lit!(NEW = "new");
lit!(GEN = " gen ");
lit!(
    /// A Part's Sound's line's head.
    SOUND = "sound "
);
lit!(
    /// A Part's engine's line's head.
    ENGINE = "engine "
);
lit!(CLEAN = "clean");
lit!(EDITED = "edited");
lit!(STALE = "stale");
lit!(
    /// The toast's line's head, its text's quote open.
    TOAST = "toast \""
);
lit!(
    /// The listing's header's head.
    PROJECTS = "projects "
);
lit!(MORE = " more");
lit!(
    /// A listed project's line's head.
    PROJ = "proj "
);
lit!(OK = "ok");
lit!(NEWER = "newer");
lit!(BAD = "bad");
