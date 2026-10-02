//! `status`: firmware, project, Part and where the UI is.

use core::fmt::{self, Write};

use super::PROTOCOL;
use crate::project::ProjectStatus;
use crate::ui::UiState;
use crate::ui::about_page::{BUILD, VERSION};
use crate::ui::nav::{MixPage, Rung};

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
    let name = ui.page_name();
    let name = name.as_ref().map_or("", |n| n.as_str());
    match ui.location().rung() {
        Rung::Pages(p, _) => write!(w, "PART {} > {name}", p.index() + 1),
        Rung::Mixer(p, m) => {
            let page = match m {
                MixPage::Part => "PART",
                MixPage::Sends => "SENDS",
            };
            write!(w, "MIXER {} > {page}", p.index() + 1)
        }
        Rung::Fx(..) => write!(w, "FX > {name}"),
        Rung::Sound(p) => write!(w, "SOUND {}", p.index() + 1),
        Rung::Settings(_) => {
            let Some(crumbs) = ui.crumbs() else {
                return Ok(());
            };
            for (i, c) in crumbs.parts().iter().enumerate() {
                if i > 0 {
                    w.write_str(" > ")?;
                }
                write!(w, "{c}")?;
            }
            Ok(())
        }
    }
}
