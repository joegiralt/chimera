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
