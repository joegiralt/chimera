//! A kit's voice groups (ADR 0280): each choke group in use counts once,
//! each pad with choke OFF once. DRUM plays in the 8-voice pool by
//! keeping a kit to 8 groups, so a choke edit never grows a kit past
//! them; a kit from before the bound loads and plays as it is. The UI's,
//! linked into bank 2 with its door (`ui::kit_groups`).

use crate::hw::MAX_VOICES;

use super::types::{ChokeGroup, Kit, PadId};

/// A kit's voice groups: 1..=12 (a kit has 12 pads).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VoiceGroups(u8);

impl VoiceGroups {
    /// The most a choke edit may take a kit to: one group a voice.
    pub const MAX: VoiceGroups = VoiceGroups(MAX_VOICES as u8);

    /// `kit`'s groups.
    pub fn of(kit: &Kit) -> VoiceGroups {
        Self::count(kit, None)
    }

    /// The groups of `kit`, with `with`'s pad in its choke group if given:
    /// a bit a choke group (A–D), the OFF pads counted apart.
    #[inline(never)]
    #[optimize(size)]
    fn count(kit: &Kit, with: Option<(PadId, ChokeGroup)>) -> VoiceGroups {
        let (mut groups, mut off) = (0u8, 0u8);
        for p in PadId::ALL {
            let c = match with {
                Some((q, c)) if q == p => c,
                _ => kit.pad(p).choke(),
            };
            match c {
                ChokeGroup::Off => off += 1,
                c => groups |= 1 << c.index(),
            }
        }
        VoiceGroups(groups.count_ones() as u8 + off)
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Past `MAX`: a kit from before the bound.
    pub const fn over(self) -> bool {
        self.0 > Self::MAX.0
    }

    /// `kit`'s groups with pad `p` in `c`: `Ok` where the edit may land
    /// (within `MAX`, or no more than the kit has now), else `Err` with
    /// the count it would make.
    #[optimize(size)]
    pub fn choke(kit: &Kit, p: PadId, c: ChokeGroup) -> Result<VoiceGroups, VoiceGroups> {
        let to = Self::count(kit, Some((p, c)));
        if to.over() && to > Self::of(kit) {
            Err(to)
        } else {
            Ok(to)
        }
    }
}
