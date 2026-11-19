//! What an editor's set does on the box beside its write (ADR 0285): a set
//! on a cell ends any choice held dimmed there, as a turn that lands does,
//! and a set the box refuses says why in a toast, as a CC's refusal does.
//! Bank 2's: `bank2.x` links this module with `console::ed`.

use super::UiState;
use super::held::Held;
use super::settings::prompt;
use crate::addr::ParamAddr;
use crate::dsp::drum::types::PadId;
use crate::project::{Line, PartId};

/// A set on `a` of Part `at` (on pad `pad`, a pad's) reached the cell: a
/// choice held there is over. A held CHOKE is its pad's: another pad's
/// set leaves it.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.landed"))]
pub(crate) fn landed(ui: &mut UiState, at: PartId, a: ParamAddr, pad: Option<PadId>) {
    let on = |h: Held| {
        h.addr() == a
            && match h {
                Held::Choke(c) => pad == Some(c.pad),
                Held::Sound(_) | Held::Voices(_) => true,
            }
    };
    if ui.held.is_some_and(|(p, h)| p == at && on(h)) {
        ui.held = None;
    }
}

/// A set on `a` the box refused, for `why`: the hold over, the reason
/// shown once while its toast is up.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.refused"))]
pub(crate) fn refused(ui: &mut UiState, at: PartId, a: ParamAddr, pad: Option<PadId>, why: Line) {
    landed(ui, at, a, pad);
    if !ui.toast.shows(&why) {
        ui.error(why);
    }
}

/// The formant budget's words, a CC's (`formant_full`).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.cue.formant_full")
)]
pub(crate) fn formant_full() -> Line {
    Line::new(prompt::FORMANT_FULL)
}

/// Where `ed route` writes on the active Part's grid (ADR 0289): a cell's
/// row and its column, a fixed one or a registry destination's (primed
/// first when it isn't, as MIX + PLUS then a turn would).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RouteAt {
    Fixed(super::mod_grid::RowSource, crate::modulation::FixedCol),
    Reg(super::mod_grid::RowSource, ParamAddr),
    /// FLT › MOD's jack knob: the cell its jack names now.
    Jack(super::filter_panel::JackSlot),
}

/// One UI pass of `ed route` on the active Part: the cell moved toward
/// `to` by at most a hand's fastest turn (`encoder_accel`'s top step),
/// never a jump, or removed (`None`, the grid's MIX + MINUS). Through the
/// grid's own writes (`MatrixState::set`, `clear`), then the Part's
/// `ModState` and taps synced. `true` once the cell holds `to`, or can't:
/// a registry full when priming says so in the focus band, as the box.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.route"))]
pub(crate) fn route(ui: &mut UiState, at: RouteAt, to: Option<i8>) -> bool {
    use super::PrimeStatus;
    use super::mod_grid::{RowSource, dest_label};
    let part = ui.active_part;
    let (row, col): (RowSource, usize) = match at {
        RouteAt::Fixed(r, c) => (r, c.index()),
        RouteAt::Jack(j) => {
            let (r, c) = super::filter_panel::RouteView::Jack(j).cell(&ui.project.part(part).sound);
            (r, c.index())
        }
        RouteAt::Reg(r, a) => match col_of(&ui.matrix_state, a) {
            Some(c) => (r, c),
            // Nothing to remove.
            None if to.is_none() => return true,
            None => {
                let sound = ui.project.edit_part(part).sound;
                if let Err(e) = sound.dest_registry.add(a, dest_label(a)) {
                    ui.prime_status = Some(PrimeStatus::from(e));
                    return true;
                }
                ui.matrix_state
                    .rebuild_dests_from_registry(&sound.dest_registry);
                ui.matrix_state.load_amounts(&sound.mod_state);
                match col_of(&ui.matrix_state, a) {
                    Some(c) => (r, c),
                    None => return true,
                }
            }
        },
    };
    let m = &mut ui.matrix_state;
    let done = match to {
        None => {
            if m.is_present(row.index(), col) {
                m.clear(row.index(), col);
            }
            true
        }
        Some(t) => {
            let now = m.amount(row.index(), col);
            let step = i16::from(chimera_hal::encoder_accel(u8::MAX));
            let next = (i16::from(t) - i16::from(now)).clamp(-step, step) + i16::from(now);
            let next = i8::try_from(next).unwrap_or(t);
            if next != now || !m.is_present(row.index(), col) {
                m.set(row.index(), col, next);
            }
            next == t
        }
    };
    ui.sync_mod_state(part);
    done
}

/// The grid column of registry destination `a`: `MatrixState::col_of`'s
/// walk, here in bank 2 (its `find` would be a generic bank 1 holds).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.col_of"))]
fn col_of(m: &super::mod_grid::MatrixState, a: ParamAddr) -> Option<usize> {
    let mut d = 0;
    while d < m.num_dests {
        if m.dests
            .get(d)
            .copied()
            .flatten()
            .is_some_and(|x| x.addr == a)
        {
            return Some(super::mod_grid::FIXED_COLS + d);
        }
        d += 1;
    }
    None
}

/// A global block's values as the box holds them: SETTINGS › THEME, MIDI
/// CONFIG, RANDOMIZE's (none while bank 2 is closed, as its turn reaches
/// nothing then: ADR 0232).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.global"))]
pub(crate) fn global(
    ui: &UiState,
    g: crate::console::GlobalBlock,
) -> Option<chimera_engine::addr::BlockView<'_>> {
    use crate::console::GlobalBlock;
    use chimera_engine::addr::BlockView;
    Some(BlockView::Plain(match g {
        GlobalBlock::Theme => &ui.theme,
        GlobalBlock::Midi => &ui.midi,
        GlobalBlock::Random => {
            crate::bank2::ui_door(|_| ())?;
            &ui.random
        }
    }))
}

/// The box's toast up now, its words: `since`'s `toast` line.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.toast"))]
pub(crate) fn toast(ui: &UiState) -> Option<&Line> {
    ui.toast.text()
}

/// The listed entry `id` at `generation`, from a listing that is the card
/// as last read (as MANAGE's `chosen`): an `ed project load`'s witness.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.listed"))]
pub(crate) fn listed(
    ui: &UiState,
    id: crate::storage::ProjectId,
    generation: crate::storage::Generation,
) -> Option<crate::project::ProjectEntry> {
    use super::settings::listing::Validity;
    if ui.listing.validity() != Validity::Current {
        return None;
    }
    let mut i = 0;
    while let Some(e) = ui.listing.entry(i) {
        if e.id == id && e.generation == Some(generation) {
            return Some(e);
        }
        i += 1;
    }
    None
}

/// One UI pass of `ed projects` (`None`) or an `ed project` command: the
/// box's own card job queued for `card_work`, under BUSY, once the card is
/// free (`false` until then, so the act waits and the answer never does).
/// Checked again here: a save with no file of its own, a load whose
/// listing moved or that has edits under it without `discard`, write
/// nothing (`true`: the act is done).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.card"))]
pub(crate) fn card(ui: &mut UiState, c: Option<crate::console::ProjectCmd>) -> bool {
    use super::settings::job::Job;
    use crate::console::{OverEdits, ProjectCmd};
    use crate::project::{ProjectSource, ReplaceGuard};
    if ui.card_pending() {
        return false;
    }
    let job = match c {
        None => Job::Relist(None),
        Some(ProjectCmd::Save) => match ui.project.meta().file() {
            Some(_) => Job::QuickSave(None),
            None => return true,
        },
        Some(ProjectCmd::SaveAs(n)) => Job::Relist(Some(n.get())),
        Some(ProjectCmd::Load {
            id,
            generation,
            over,
        }) => {
            let Some(e) = listed(ui, id, generation) else {
                return true;
            };
            if let Some(n) = super::settings::listing::refusal(&e) {
                ui.show_note(n);
                return true;
            }
            let src = ProjectSource::File {
                id: e.id,
                vol: e.vol,
            };
            match (ReplaceGuard::check(&ui.project, ui.template, src), over) {
                (Ok(go), OverEdits::Refuse | OverEdits::Discard) => Job::Load(go),
                (Err(ask), OverEdits::Discard) => {
                    let yes = chimera_engine::confirm::Said::editor_discard();
                    Job::Load(ask.into_pending().confirm(&ui.project, yes))
                }
                (Err(_), OverEdits::Refuse) => return true,
            }
        }
    };
    ui.job = Some(job);
    true
}

/// Part `p`'s controller map's rows as it plays them (ADR 0226): empty
/// before the MIDI UI first ran, as MIDI MAP reads it.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.layer"))]
pub(crate) fn layer(ui: &UiState, p: PartId) -> crate::midi_map::MidiMap {
    match ui.cc.built() {
        Some(m) => *m.ctl.layer(p.index()),
        None => crate::midi_map::MidiMap::EMPTY,
    }
}

/// `m` as Part `p`'s own rows: MIDI MAP's write, a Sound edit (`set_map`).
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.set_own"))]
pub(crate) fn set_own(ui: &mut UiState, p: PartId, m: crate::midi_map::MidiMap) {
    ui.set_map(p, m);
}

/// Row `i` of Part `p`'s controller map, `was`, made `to`: MIDI MAP's
/// turn on a layer row (`Layers::edit`), the map on the card changed by
/// the next card work. `false`: another job waits, nothing changed; the
/// box says CARD BUSY, as it does to a hand.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.layer_edit"))]
pub(crate) fn layer_edit(
    ui: &mut UiState,
    p: PartId,
    i: usize,
    was: crate::midi_map::Binding,
    to: crate::midi_map::Binding,
) -> bool {
    let took = ui.cc.ui().ctl.edit(p, i, was, to);
    match took {
        true => ui.cc.reset_part(p),
        false => ui.error(Line::new(chimera_hal::store::StoreError::Busy.message())),
    }
    took
}

/// MIDI MAP's card job `j` (`ctl_ask`): a save, a rename, a row's drop.
/// One waits at a time: another waiting, the box says CARD BUSY.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.ctl_ask"))]
pub(crate) fn ctl_ask(ui: &mut UiState, j: super::midi::layer::CtlJob) {
    ui.ctl_ask_in(j);
}

/// The map job waiting for the card, if any.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.ctl_job"))]
pub(crate) fn ctl_job(ui: &UiState) -> Option<super::midi::layer::CtlJob> {
    ui.cc.built().and_then(|m| m.ctl.job)
}

/// The card's maps as last listed, `None` before a listing.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.maps"))]
pub(crate) fn maps(ui: &UiState) -> Option<&[Option<crate::name::SoundName>]> {
    ui.cc.built().and_then(|m| m.ctl.listed())
}

/// KEEP MAP on Part `p` (ADR 0253).
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.keeps"))]
pub(crate) fn keeps(ui: &UiState, p: PartId) -> bool {
    ui.keeps_map(p)
}

/// KEEP MAP on Part `p` made `on`: MIX + PLUS on row 0, only when it
/// differs, so a second ask changes nothing.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.keep"))]
pub(crate) fn keep(ui: &mut UiState, p: PartId, on: bool) {
    if ui.keeps_map(p) != on {
        ui.keep_map(p);
    }
}

/// The editor asked for the card's maps (`ed maps`): the next card work
/// lists them, as MIDI MAP opening does, nothing on screen moving. One
/// editor, one ask: a flag, not a queue. RAM's (`.bss`), not a section of
/// bank 2's.
#[cfg(feature = "midi-map")]
static MAPS_ASKED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// `ed maps` in the UI pass: the listing asked of the next card work,
/// once no map job waits (`false` until then: the act waits).
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.ask_maps"))]
pub(crate) fn ask_maps(ui: &UiState) -> bool {
    if ctl_job(ui).is_some() {
        return false;
    }
    MAPS_ASKED.store(true, core::sync::atomic::Ordering::Relaxed);
    true
}

/// The door's `ctl_due`: the layers' card work (`ctl_due_in`), or the
/// editor's listing asked.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.ctl_due"))]
pub(crate) fn ctl_due(ui: &UiState) -> bool {
    ui.ctl_due_in() || MAPS_ASKED.load(core::sync::atomic::Ordering::Relaxed)
}

/// The door's `ctl_work`: the layers' (`ctl_work_in`: a job, a stale
/// layer, MIDI MAP's listing), and once they are done the editor's
/// listing, as MIDI MAP's is made. No card: the layers' empties it.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.ctl_work"))]
pub(crate) fn ctl_work(
    ui: &mut UiState,
    s: &mut dyn chimera_hal::store::Store,
    r: Option<&crate::storage::Ready>,
) {
    use crate::midi_map::ctl::{self, MAX_MAPS};
    use chimera_hal::store::StoreError;
    use core::sync::atomic::Ordering;
    let asked = MAPS_ASKED.load(Ordering::Relaxed);
    let (Some(r), true, false) = (r, asked, ui.ctl_due_in()) else {
        if r.is_none() {
            MAPS_ASKED.store(false, Ordering::Relaxed);
        }
        return ui.ctl_work_in(s, r);
    };
    let c = &mut ui.cc.ui().ctl;
    c.list = [None; MAX_MAPS];
    let got = ctl::list(s, r, &mut c.list);
    c.more = matches!(got, Ok((_, true)));
    c.listed = !matches!(got, Err(StoreError::Busy));
    if c.listed {
        MAPS_ASKED.store(false, Ordering::Relaxed);
    }
}

/// Where `ed learn` arms (ADR 0289): a page of the Part's own chain, or of
/// its mixer (its pages, the FX's).
#[cfg(feature = "midi-map")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LearnOn {
    Own,
    Mixer,
}

/// `ed learn` in the UI pass: the box taken to page `def` of Part `p`'s
/// (its own chain, or its mixer), slot `slot` focused and learn armed
/// there, as MIX + SEQ arms on the focused cell; `None`, learn ended, as
/// MIX + SEQ ends it. Never over pointing (MIX + SEQ does nothing then).
/// A move waits behind a load or a card job (`false`), as `ed part`'s
/// key does; a page the Part's chain no longer holds arms nothing.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.learn"))]
pub(crate) fn learn(
    ui: &mut UiState,
    at: Option<(PartId, LearnOn, &'static super::block_def::BlockDef, usize)>,
) -> bool {
    use super::learn::Learn;
    use super::nav::{Location, PartChain};
    if let Learn::Pointing(_) = ui.learn {
        return true;
    }
    let Some((p, on, def, slot)) = at else {
        if let Learn::Armed { .. } | Learn::Sensing { .. } = ui.learn {
            ui.learn = Learn::Off;
        }
        return true;
    };
    if ui.src.loading() || ui.lib.busy() || ui.job.is_some() {
        return false;
    }
    let chain = match on {
        LearnOn::Own => PartChain::of(&ui.project.part(p).sound).def(),
        LearnOn::Mixer => &super::block_registry::MIXER_CHANNEL_CHAIN,
    };
    let Some(page) = locate(chain, def.id) else {
        return true;
    };
    if ui.modal.take().is_some() {
        ui.closed();
    }
    ui.idle.touch(false, true);
    ui.go(match on {
        LearnOn::Own => Location::own_page(p, page),
        LearnOn::Mixer => Location::mixer_page(p, page),
    });
    ui.focus.touch(def.id, slot);
    ui.learn = Learn::Armed {
        part: p,
        note: None,
    };
    true
}

/// The page of `chain` showing def `id`: `ChainDef2::locate`'s walk, here
/// in bank 2 (its iterator adaptors would be instances bank 1 holds).
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.locate"))]
fn locate(chain: &super::block_def::ChainDef2, id: u16) -> Option<super::block_def::PageAt> {
    let mut node = 0;
    while let Some(b) = chain.block_at(node) {
        if b.def.id == id {
            return chain.page(node, 0);
        }
        let mut k = 0;
        while let Some(d) = b.sub_pages.get(k) {
            if d.id == id {
                return chain.page(node, k + 1);
            }
            k += 1;
        }
        node += 1;
    }
    None
}

/// What learn says in the focus band now, under the focused cell's label:
/// `since`'s `learn` line quotes it.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.learn_note"))]
pub(crate) fn learn_note(ui: &UiState) -> Option<crate::ui::fmt::FmtBuf> {
    let v = super::view::view(ui.page_def(), ui.focused_slot(), &ui.ctx());
    ui.band_status().map(|s| s.text(v.label()))
}

/// The focused cell's param, if a control may drive it: what learn arms on.
#[cfg(feature = "midi-map")]
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.cue.learn_target")
)]
pub(crate) fn learn_target(ui: &UiState) -> Option<crate::midi_map::Mappable> {
    ui.learn_target()
}
