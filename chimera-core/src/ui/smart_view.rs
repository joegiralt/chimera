//! SMART EQ's viz band, bank 1's side (ADR 0296): what the band shows,
//! read from the audio's live cuts (`SMART_VIEW`) and the Parts' roles,
//! its dirty key, and the one call through the UI door that paints it
//! (`smart_eq::paint_in`, bank 2's). Closed, nothing is drawn.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::hw::MAX_PARTS;
use crate::part_eq::smart::{CutView, Role, SMART_VIEW};
use crate::project::PartId;

/// Which of the two views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shows {
    /// PRT › EQ: this Part's four bands.
    Part(PartId),
    /// MST › SMART EQ: every Part, this one lit.
    All(PartId),
}

/// What the band draws: read by bank 1 from the audio's view and the
/// project's roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub shows: Shows,
    pub cuts: [CutView; MAX_PARTS],
    pub roles: [Role; MAX_PARTS],
}

impl Shown {
    /// The audio's live cuts now, with `roles`.
    pub fn now(shows: Shows, roles: [Role; MAX_PARTS]) -> Shown {
        Shown {
            shows,
            cuts: PartId::ALL.map(|p| SMART_VIEW.read(p)),
            roles,
        }
    }

    /// The band's dirty key.
    pub fn key(&self) -> u32 {
        let (tag, p) = match self.shows {
            Shows::Part(p) => (0u32, p),
            Shows::All(p) => (1, p),
        };
        let roles = self
            .roles
            .iter()
            .fold(0u32, |k, r| k << 2 | u32::from(r.code()));
        let mut k = tag << 31 ^ (p.index() as u32) << 28 ^ roles;
        for c in self.cuts {
            k = k.rotate_left(7) ^ c.word();
        }
        k
    }
}

/// The band through the UI door; closed, blank.
pub fn paint<D>(d: &mut D, s: &Shown)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut ink = crate::ui::orbit::door::Ink::new(d);
    crate::bank2::ui_door(|u| u.smart_eq(&mut ink, s));
}
