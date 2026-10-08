//! `status`: firmware, project, Part and where the UI is.

use core::fmt::{self, Write};

use super::PROTOCOL;
use crate::project::ProjectStatus;
use crate::ui::UiState;
use crate::ui::about_page::{BUILD, VERSION, write_build_rev};
use crate::ui::nav::{MixPage, Rung};
use chimera_hal::store::{Sweep, SweepSeen, SweepStats};

/// The footer's word for `s`.
pub fn state_word(s: ProjectStatus) -> &'static str {
    match s {
        ProjectStatus::Pristine => "NEW",
        ProjectStatus::Saved => "SAVED",
        ProjectStatus::Modified => "MODIFIED",
    }
}

/// The status body, without the terminal line.
pub fn write_status(ui: &UiState, w: &mut impl Write) -> fmt::Result {
    write!(w, "firmware {VERSION} ")?;
    for c in BUILD.chars() {
        w.write_char(c.to_ascii_lowercase())?;
    }
    w.write_str("\nbuild ")?;
    write_build_rev(w)?;
    writeln!(w, "\nprotocol {PROTOCOL}")?;
    writeln!(w, "project {}", ui.project().meta().name().as_str())?;
    writeln!(w, "state {}", state_word(ui.project_status()))?;
    writeln!(w, "part {}", ui.active_part.index() + 1)?;
    w.write_str("at ")?;
    write_at(ui, w)?;
    w.write_char('\n')
}

/// `Location` in ASCII, ` > ` between levels; SETTINGS' breadcrumb in full.
fn write_at(ui: &UiState, w: &mut impl Write) -> fmt::Result {
    match ui.location().rung() {
        Rung::Pages(p, _) => write!(w, "PART {} > {}", p.index() + 1, ui.page_title().as_str()),
        Rung::Mixer(p, m) => {
            let page = match m {
                MixPage::Part => "PART",
                MixPage::Sends => "SENDS",
            };
            write!(w, "MIXER {} > {page}", p.index() + 1)
        }
        Rung::Fx(..) => write!(w, "FX > {}", ui.page_title().as_str()),
        Rung::Sound(p) => write!(w, "SOUND {}", p.index() + 1),
        Rung::Orbit(_) => write!(w, "ORBIT > {}", ui.page_title().as_str()),
        Rung::Settings(at) => {
            for (i, c) in ui.crumbs_at(at).parts().iter().enumerate() {
                if i > 0 {
                    w.write_str(" > ")?;
                }
                write!(w, "{c}")?;
            }
            Ok(())
        }
    }
}

/// `status`'s `sweep` line: the boot's lost-cluster sweep (ADR 0094), its
/// mode, what it found or the doubt that stopped it, the volume, and each
/// phase's time.
pub fn write_sweep(mode: Sweep, seen: &SweepSeen, w: &mut impl Write) -> fmt::Result {
    let mode = match mode {
        Sweep::DryRun => "dry-run",
        Sweep::Free => "free",
    };
    let stats: &SweepStats = match seen {
        Ok(s) => {
            let healed = if s.healed { "yes" } else { "no" };
            write!(
                w,
                "sweep {mode} lost {} freed {} healed {healed}",
                s.lost, s.freed
            )?;
            &s.stats
        }
        Err((d, stats)) => {
            write!(w, "sweep {mode} doubt {}", d.word())?;
            stats
        }
    };
    writeln!(
        w,
        " spc {} clusters {} fat {} compare {}ms mark {}ms count {}ms",
        stats.spc, stats.clusters, stats.fat_bytes, stats.compare_ms, stats.mark_ms, stats.count_ms
    )
}
