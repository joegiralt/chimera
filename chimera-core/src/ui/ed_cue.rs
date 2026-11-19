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
