//! A kit's voice groups (ADR 0280): each choke group in use counts once,
//! each pad with choke OFF once. DRUM plays in the 8-voice pool by
//! keeping a kit to 8 groups, so a choke edit never grows a kit past
//! them; a kit from before the bound loads and plays as it is. The UI's,
//! linked into bank 2 with its door (`ui::kit_groups`).

use crate::hw::MAX_VOICES;

use super::types::{ChokeGroup, Kit, PadId};

/// The choke groups (A–D) a kit has in use, a bit each; OFF is no group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChokeSet(u8);

const _: () = assert!(ChokeGroup::ALL.len() <= u8::BITS as usize);

impl ChokeSet {
    pub const EMPTY: ChokeSet = ChokeSet(0);

    /// With `c` in it; OFF adds nothing.
    pub const fn with(self, c: ChokeGroup) -> ChokeSet {
        match c {
            ChokeGroup::Off => self,
            c => ChokeSet(self.0 | 1 << c.index()),
        }
    }

    /// How many groups.
    pub const fn len(self) -> u8 {
        self.0.count_ones() as u8
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A kit's chokes, a pad each: what a held CHOKE was judged against
/// (`ui::held::ChokeHold`). Equal while no pad's choke has changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitChokes([ChokeGroup; PadId::ALL.len()]);

impl KitChokes {
    /// A plain loop: `array::map`'s instance would be core's and link
    /// into bank 1.
    pub fn of(kit: &Kit) -> KitChokes {
        let mut c = [ChokeGroup::Off; PadId::ALL.len()];
        let mut i = 0;
        while i < c.len() {
            c[i] = kit.pad(PadId::ALL[i]).choke();
            i += 1;
        }
        KitChokes(c)
    }
}

/// A kit's voice groups: 1..=12 by construction, the pads' choke groups
/// in use and the pads with choke OFF.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct VoiceGroups(u8);

// One group fits a voice; a kit of only OFF pads has a group a pad.
const _: () = assert!(MAX_VOICES >= 1 && MAX_VOICES <= PadId::ALL.len());

impl VoiceGroups {
    /// The most a choke edit may take a kit to: one group a voice.
    pub const MAX: VoiceGroups = VoiceGroups(MAX_VOICES as u8);

    /// `kit`'s groups.
    pub fn of(kit: &Kit) -> VoiceGroups {
        Self::count(kit, None)
    }

    /// The groups of `kit`, with `with`'s pad in its choke group if given.
    /// Every pad is in one group or OFF, so 1..=12.
    #[inline(never)]
    #[optimize(size)]
    fn count(kit: &Kit, with: Option<(PadId, ChokeGroup)>) -> VoiceGroups {
        let (mut groups, mut off) = (ChokeSet::EMPTY, 0u8);
        for p in PadId::ALL {
            let c = match with {
                Some((q, c)) if q == p => c,
                _ => kit.pad(p).choke(),
            };
            match c {
                ChokeGroup::Off => off += 1,
                c => groups = groups.with(c),
            }
        }
        VoiceGroups(groups.len() + off)
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
