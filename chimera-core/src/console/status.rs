//! `status`: firmware, project, Part and where the UI is.

use core::fmt::{self, Write};

use super::PROTOCOL;
use crate::board::BoardId;
use crate::project::ProjectStatus;
use crate::ui::UiState;
use crate::ui::about_page::{BUILD, VERSION, write_build_rev};
use crate::ui::nav::{MixPage, Rung};
use chimera_hal::store::{DOUBT_HALT, Doubt, Mark, Sweep, SweepRun, SweepStats, Then, Trigger};

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

/// The `board` line (ADR 0140): the board and the strap it was read
/// from, PD0 first; an unknown strap is wired as the LQFP176.
pub fn write_board(b: BoardId, w: &mut impl Write) -> fmt::Result {
    let s = b.straps();
    let bit = |i: u8| if s >> i & 1 == 1 { '1' } else { '0' };
    writeln!(
        w,
        "board {} straps=0b{}{}{}{}",
        b.word(),
        bit(3),
        bit(2),
        bit(1),
        bit(0)
    )
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

/// The `sweep` line, `status`'s (ADR 0094): skipped and the mark that let
/// it; or the mode, why it ran, what it found or the doubt that stopped
/// it, the volume, each phase's time, and what became of the mark. A boot
/// that stopped sweeping for doubts adds a `warn` line.
pub fn write_sweep(run: &SweepRun, w: &mut impl Write) -> fmt::Result {
    let (mode, why, seen, then) = match run {
        SweepRun::Skipped(m) => {
            w.write_str("sweep skipped mark ")?;
            write_mark(*m, w)?;
            w.write_char('\n')?;
            return warn(run, w);
        }
        SweepRun::Ran {
            mode,
            why,
            seen,
            then,
        } => (mode, why, seen, then),
    };
    let mode = match mode {
        Sweep::DryRun => "dry-run",
        Sweep::Free => "free",
    };
    write!(w, "sweep {mode} why ")?;
    match why {
        Trigger::Owed(m) => write_mark(*m, w)?,
        Trigger::Unread => w.write_str("unread")?,
        Trigger::Asked => w.write_str("asked")?,
    }
    w.write_char(' ')?;
    let stats: &SweepStats = match seen {
        Ok(s) => {
            let yes = |b| if b { "yes" } else { "no" };
            write!(
                w,
                "lost {} freed {} stale {} healed {}",
                s.lost,
                s.freed,
                yes(s.stale),
                yes(s.healed)
            )?;
            &s.stats
        }
        Err((d, stats)) => {
            write!(w, "doubt {}", d.word())?;
            if let Doubt::TooMany(lost) = d {
                write!(w, " lost {lost}")?;
            }
            stats
        }
    };
    write!(
        w,
        " spc {} clusters {} fat {} compare {}ms mark {}ms count {}ms then ",
        stats.spc, stats.clusters, stats.fat_bytes, stats.compare_ms, stats.mark_ms, stats.count_ms
    )?;
    match then {
        Then::Cleared => w.write_str("cleared")?,
        Then::Kept => w.write_str("kept")?,
        Then::Failed => w.write_str("failed")?,
        Then::Doubted(n) => write!(w, "doubted {n}")?,
    }
    w.write_char('\n')?;
    warn(run, w)
}

/// The boot's warning once the sweep stopped for doubts.
fn warn(run: &SweepRun, w: &mut impl Write) -> fmt::Result {
    if !run.halted() {
        return Ok(());
    }
    writeln!(
        w,
        "warn sweep stopped: {DOUBT_HALT} doubting boots in a row; check the card on a computer, then run sweep"
    )
}

fn write_mark(m: Mark, w: &mut impl Write) -> fmt::Result {
    match m {
        Mark::Clean => w.write_str("clean"),
        Mark::Dirty => w.write_str("dirty"),
        Mark::Missing => w.write_str("missing"),
        Mark::Unused => w.write_str("unused"),
        Mark::Doubted(n) => write!(w, "doubted {n}"),
        Mark::Asked(_) => w.write_str("asked"),
    }
}
