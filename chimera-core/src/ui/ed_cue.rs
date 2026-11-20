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

/// What a queued act made of a UI pass: done (or dropped), or kept for
/// the next pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pass {
    Done,
    Wait,
}

/// What a Part's Sound was when an act that takes several passes was
/// queued for it: the Part's Sound loads (a library or INIT load, a
/// program change) and the project's load. Either moved, the act was for
/// a Sound that left, and is dropped (as pointing ends, ADR 0288). The
/// project's load epoch is kept by its low byte, as `Loads` counts: an
/// act lives a few passes, never 256 loads, and its copy sits in ITCM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LoadMark {
    loads: crate::project::Loads,
    project: u8,
}

/// Part `p`'s Sound as it is loaded now.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.load_mark"))]
pub(crate) fn load_mark(ui: &UiState, p: PartId) -> LoadMark {
    LoadMark {
        loads: ui.project.part(p).loads(),
        project: ui.orbit.epoch() as u8,
    }
}

/// A column of the mod grid: a fixed one, or a registry destination's.
#[derive(Clone, Copy)]
struct GridCol(usize);

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
/// `ModState` and taps synced. Done once the cell holds `to`, or can't:
/// a registry full when priming says so in the focus band, as the box.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.route"))]
pub(crate) fn route(ui: &mut UiState, at: RouteAt, to: Option<i8>) -> Pass {
    use super::PrimeStatus;
    use super::mod_grid::{RowSource, dest_label};
    let part = ui.active_part;
    let (row, GridCol(col)): (RowSource, GridCol) = match at {
        RouteAt::Fixed(r, c) => (r, GridCol(c.index())),
        RouteAt::Jack(j) => {
            let (r, c) = super::filter_panel::RouteView::Jack(j).cell(&ui.project.part(part).sound);
            (r, GridCol(c.index()))
        }
        RouteAt::Reg(r, a) => match col_of(&ui.matrix_state, a) {
            Some(c) => (r, c),
            // Nothing to remove.
            None if to.is_none() => return Pass::Done,
            None => {
                let sound = ui.project.edit_part(part).sound;
                if let Err(e) = sound.dest_registry.add(a, dest_label(a)) {
                    ui.prime_status = Some(PrimeStatus::from(e));
                    return Pass::Done;
                }
                ui.matrix_state
                    .rebuild_dests_from_registry(&sound.dest_registry);
                ui.matrix_state.load_amounts(&sound.mod_state);
                match col_of(&ui.matrix_state, a) {
                    Some(c) => (r, c),
                    None => return Pass::Done,
                }
            }
        },
    };
    let m = &mut ui.matrix_state;
    let pass = match to {
        None => {
            if m.is_present(row.index(), col) {
                m.clear(row.index(), col);
            }
            Pass::Done
        }
        Some(t) => {
            let now = m.amount(row.index(), col);
            let step = i16::from(chimera_hal::encoder_accel(u8::MAX));
            let next = (i16::from(t) - i16::from(now)).clamp(-step, step) + i16::from(now);
            let next = i8::try_from(next).unwrap_or(t);
            if next != now || !m.is_present(row.index(), col) {
                m.set(row.index(), col, next);
            }
            match next == t {
                true => Pass::Done,
                false => Pass::Wait,
            }
        }
    };
    ui.sync_mod_state(part);
    pass
}

/// The grid column of registry destination `a`: `MatrixState::col_of`'s
/// walk, here in bank 2 (its `find` would be a generic bank 1 holds).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.col_of"))]
fn col_of(m: &super::mod_grid::MatrixState, a: ParamAddr) -> Option<GridCol> {
    let mut d = 0;
    while d < m.num_dests {
        if m.dests
            .get(d)
            .copied()
            .flatten()
            .is_some_and(|x| x.addr == a)
        {
            return Some(GridCol(super::mod_grid::FIXED_COLS + d));
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
/// free (it waits until then, and the answer never does). Checked again
/// here: a save with no file of its own, a load whose listing moved or
/// that has edits under it without `discard`, write nothing (done).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.card"))]
pub(crate) fn card(ui: &mut UiState, c: Option<crate::console::ProjectCmd>) -> Pass {
    use super::settings::job::Job;
    use crate::console::{OverEdits, ProjectCmd};
    use crate::project::{ProjectSource, ReplaceGuard};
    if ui.card_pending() {
        return Pass::Wait;
    }
    let job = match c {
        None => Job::Relist(None),
        Some(ProjectCmd::Save) => match ui.project.meta().file() {
            Some(_) => Job::QuickSave(None),
            None => return Pass::Done,
        },
        Some(ProjectCmd::SaveAs(n)) => Job::Relist(Some(n.get())),
        Some(ProjectCmd::Load {
            id,
            generation,
            over,
        }) => {
            let Some(e) = listed(ui, id, generation) else {
                return Pass::Done;
            };
            if let Some(n) = super::settings::listing::refusal(&e) {
                ui.show_note(n);
                return Pass::Done;
            }
            let src = ProjectSource::File {
                id: e.id,
                vol: e.vol,
            };
            match (ReplaceGuard::check(&ui.project, ui.template, src), over) {
                (Ok(go), OverEdits::Refuse | OverEdits::Discard(_)) => Job::Load(go),
                // The editor's yes, said with this load's witness.
                (Err(ask), OverEdits::Discard(d)) if d.said_of(generation) => {
                    let yes = chimera_engine::confirm::Said::editor_discard(&d);
                    Job::Load(ask.into_pending().confirm(&ui.project, yes))
                }
                (Err(_), OverEdits::Refuse | OverEdits::Discard(_)) => return Pass::Done,
            }
        }
    };
    ui.job = Some(job);
    Pass::Done
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
    i: crate::console::EdRowIx,
    was: crate::midi_map::Binding,
    to: crate::midi_map::Binding,
) -> bool {
    let took = ui.cc.ui().ctl.edit(p, i.index(), was, to);
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

/// `ed map rename`: Part `p`'s map, marked `was` when the answer checked
/// it, renamed `to` by the next card work, or nothing renamed if the Part
/// is on another map by then (the box says MAP CHANGED).
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.ctl_rename"))]
pub(crate) fn ctl_rename(
    ui: &mut UiState,
    p: PartId,
    was: super::midi::layer::MapMark,
    to: crate::name::SoundName,
) {
    ui.ctl_rename_of_in(p, was, to);
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

/// KEEP MAP on Part `p` made `to`: MIX + PLUS on row 0, only when it
/// differs, so a second ask changes nothing.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.keep"))]
pub(crate) fn keep(ui: &mut UiState, p: PartId, to: crate::console::EdKeepMap) {
    let on = match to {
        crate::console::EdKeepMap::On => true,
        crate::console::EdKeepMap::Off => false,
    };
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
/// once no map job waits (the act waits until then).
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.ask_maps"))]
pub(crate) fn ask_maps(ui: &UiState) -> Pass {
    if ctl_job(ui).is_some() {
        return Pass::Wait;
    }
    MAPS_ASKED.store(true, core::sync::atomic::Ordering::Relaxed);
    Pass::Done
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

/// A cell's slot on its page: one of its six, A–F.
#[cfg(feature = "midi-map")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CellSlot(u8);

#[cfg(feature = "midi-map")]
impl CellSlot {
    /// F's.
    const LAST: usize = 5;

    /// Slot `i` of a page, as `value::find` counts it; past F, none.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.cell_slot"))]
    pub(crate) fn new(i: usize) -> Option<CellSlot> {
        match i <= CellSlot::LAST {
            true => Some(CellSlot(i as u8)),
            false => None,
        }
    }
}

/// The cell `ed learn` arms on: its Part, the chain its page is on, the
/// page's def and the cell's slot there.
#[cfg(feature = "midi-map")]
#[derive(Clone, Copy)]
pub(crate) struct LearnAt {
    pub(crate) part: PartId,
    pub(crate) on: LearnOn,
    pub(crate) def: &'static super::block_def::BlockDef,
    pub(crate) slot: CellSlot,
}

/// `ed learn` in the UI pass: the box taken to `at`'s page of its Part's
/// (its own chain, or its mixer), its slot focused and learn armed there,
/// as MIX + SEQ arms on the focused cell; `None`, learn ended, as MIX +
/// SEQ ends it. Never over pointing (MIX + SEQ does nothing then). A
/// move waits behind a load or a card job, as `ed part`'s key does; a
/// page the Part's chain no longer holds arms nothing.
#[cfg(feature = "midi-map")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.learn"))]
pub(crate) fn learn(ui: &mut UiState, at: Option<LearnAt>) -> Pass {
    use super::learn::Learn;
    use super::nav::{Location, PartChain};
    if let Learn::Pointing(_) = ui.learn {
        return Pass::Done;
    }
    let Some(LearnAt {
        part: p,
        on,
        def,
        slot: CellSlot(slot),
    }) = at
    else {
        if let Learn::Armed { .. } | Learn::Sensing { .. } = ui.learn {
            ui.learn = Learn::Off;
        }
        return Pass::Done;
    };
    if ui.src.loading() || ui.lib.busy() || ui.job.is_some() {
        return Pass::Wait;
    }
    let chain = match on {
        LearnOn::Own => PartChain::of(&ui.project.part(p).sound).def(),
        LearnOn::Mixer => &super::block_registry::MIXER_CHANNEL_CHAIN,
    };
    let Some(page) = locate(chain, def.id) else {
        return Pass::Done;
    };
    if ui.modal.take().is_some() {
        ui.closed();
    }
    ui.idle.touch(false, true);
    ui.go(match on {
        LearnOn::Own => Location::own_page(p, page),
        LearnOn::Mixer => Location::mixer_page(p, page),
    });
    ui.focus.touch(def.id, usize::from(slot));
    ui.learn = Learn::Armed {
        part: p,
        note: None,
    };
    Pass::Done
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

/// `ed set` of an ORBIT address, in the UI pass: a scene's param through
/// `OrbitSong::edit` with a set to `v` (its write marks the ring touched
/// where the runner keeps its own copy, ADR 0082), on `live` and its slot
/// alike (ADR 0083), the ring the address names whatever ring the box
/// shows; the tempo through the box's own blocks, as F's turn. Then the
/// hand wins over its bindings, as a turn's (`cc_hand`).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.orbit_set"))]
pub(crate) fn orbit_set(ui: &mut UiState, a: ParamAddr, v: f32) {
    use crate::addr::{BlockRef, Blocks};
    let part = ui.active_part;
    match a.block {
        BlockRef::OrbitChords | BlockRef::OrbitRing(_) => {
            ui.project.orbit_mut().edit(&mut |s| {
                if let Some(mut b) = s.block_mut(a.block) {
                    b.set(a.param, v);
                }
            });
            // SEGS shrunk under an armed segment drops the capture.
            ui.orbit.fit(&ui.project.orbit().live().chords);
        }
        // The tempo, the box's own block (F's turn); no other block is
        // ORBIT's (`Taken::Orbit`).
        BlockRef::Tempo
        | BlockRef::Modal
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Filter
        | BlockRef::VoiceFx
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch
        | BlockRef::Chorus
        | BlockRef::Delay
        | BlockRef::Reverb
        | BlockRef::Tape
        | BlockRef::Comp
        | BlockRef::Part
        | BlockRef::Theme
        | BlockRef::Midi
        | BlockRef::PartMix(_)
        | BlockRef::Random
        | BlockRef::Pad
        | BlockRef::Kit
        | BlockRef::PadFilter
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_) => {
            ui.edit_to(part, a, v);
        }
    }
    ui.cc_hand(part, a);
}

/// One UI pass of an ORBIT command, through the box's own path for the
/// gesture, checked again first (a stale witness writes nothing, and a
/// refusal the box would toast is toasted). The transport waits until
/// every command before it is applied, so it reads the runner's transport
/// as it is; under SYNC EXT too, as B6 (ADR 0080).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.cue.orbit"))]
pub(crate) fn orbit(ui: &mut UiState, c: crate::console::OrbitCmd) -> Pass {
    use super::settings::prompt;
    use crate::console::{OrbitCmd, OrbitNotNow};
    use crate::orbit::scene::{OrbitSong, Refused};
    use crate::project::{Confirmed, SceneSlot};
    // The editor's yes, only while the slot reads the rev it sent.
    let confirmed =
        |song: &OrbitSong, s: crate::console::OrbitSeen| -> Result<Confirmed<SceneSlot>, Refused> {
            match crate::console::orbit_holds(song, s) {
                Ok(held) => Ok(held.confirmed()),
                Err(_) => Err(Refused::SlotChanged),
            }
        };
    if let OrbitCmd::Transport(t) = c {
        if !ui.orbit.idle() {
            return Pass::Wait;
        }
        if crate::console::orbit_check(c, ui).is_ok() && ui.orbit.status.running != t.runs() {
            ui.orbit.play_stop();
        }
        return Pass::Done;
    }
    let refused = match crate::console::orbit_check(c, ui) {
        Err(OrbitNotNow::SwitchPending) => Some(crate::orbit::scene::Refused::SwitchPending),
        Err(OrbitNotNow::SlotChanged | OrbitNotNow::Saved) => {
            Some(crate::orbit::scene::Refused::SlotChanged)
        }
        // Dimmed on the box: nothing said.
        Err(OrbitNotNow::Unbuilt | OrbitNotNow::NotPlaying | OrbitNotNow::NothingToReload) => {
            return Pass::Done;
        }
        Ok(()) => None,
    };
    if let Some(e) = refused {
        ui.error(Line::new(prompt::scene_refused(e)));
        return Pass::Done;
    }
    let song = ui.project.orbit_mut();
    let done = match c {
        OrbitCmd::Transport(_) => Ok(()),
        OrbitCmd::Queue(n) => {
            ui.orbit.queue(n, song);
            Ok(())
        }
        OrbitCmd::Save(n) => match song.save_to(n) {
            Ok(crate::orbit::scene::SceneSave::Saved(n)) => {
                ui.scene_saved(n);
                Ok(())
            }
            // Saved since: the editor's confirm names a rev.
            Ok(crate::orbit::scene::SceneSave::Ask(_)) => {
                Err(crate::orbit::scene::Refused::SlotChanged)
            }
            Err(e) => Err(e),
        },
        OrbitCmd::SaveOver(s) => match confirmed(song, s) {
            Ok(yes) => song.overwrite_scene(yes).map(|()| ui.scene_saved(s.slot())),
            Err(e) => Err(e),
        },
        OrbitCmd::Clear(s) => ui.relanding(|song| match confirmed(song, s) {
            Ok(yes) => song.clear_scene(yes),
            Err(e) => Err(e),
        }),
        OrbitCmd::Reload(s) => ui.relanding(|song| match confirmed(song, s) {
            Ok(yes) => song.reload_scene(yes),
            Err(e) => Err(e),
        }),
        OrbitCmd::Move(a, b) => match (confirmed(song, a), confirmed(song, b)) {
            (Ok(a), Ok(b)) => song.swap_scenes(a, b).map(|()| {
                // The runner plays `live` as the slot it moved to.
                ui.orbit.issue_scene(ui.project.orbit());
            }),
            (Err(e), Ok(_) | Err(_)) | (Ok(_), Err(e)) => Err(e),
        },
        OrbitCmd::Chord(seg, d) => {
            song.edit(&mut |s| {
                if let Some(at) = s.chords.degrees.get_mut(seg.index()) {
                    *at = d;
                }
            });
            Ok(())
        }
        // IN SCALE's toggles, a note at a time: CUSTOM, the root kept.
        OrbitCmd::Scale(m) => {
            let to = m.get();
            song.edit(&mut |s| {
                for i in chimera_theory::Interval::ALL {
                    if s.chords.set().contains(i) != to.contains(i) {
                        s.chords.toggle(i);
                    }
                }
            });
            Ok(())
        }
        OrbitCmd::Step(r, i, n) => {
            song.edit(&mut |s| {
                if let Some(ring) = s.rings.get_mut(usize::from(r.get())) {
                    if let Some(at) = ring.ring.notes.get_mut(i.index()) {
                        *at = n;
                    }
                    ring.touched = ring.touched.with(crate::orbit::scene::Touched::NOTES);
                }
            });
            Ok(())
        }
        OrbitCmd::Pool(r, p) => {
            song.edit(&mut |s| {
                if let Some(ring) = s.rings.get_mut(usize::from(r.get())) {
                    ring.ring.pool = p;
                    ring.touched = ring.touched.with(crate::orbit::scene::Touched::NOTES);
                }
            });
            Ok(())
        }
    };
    if let Err(e) = done {
        ui.error(Line::new(prompt::scene_refused(e)));
    }
    Pass::Done
}

#[cfg(all(test, feature = "midi-map"))]
mod tests {
    use super::CellSlot;

    /// A page has six slots: a seventh is no slot, never F again.
    #[test]
    fn a_slot_past_f_is_no_slot() {
        assert_eq!(CellSlot::new(0), Some(CellSlot(0)));
        assert_eq!(CellSlot::new(5), Some(CellSlot(5)));
        assert_eq!(CellSlot::new(6), None);
        assert_eq!(CellSlot::new(usize::MAX), None);
    }
}
