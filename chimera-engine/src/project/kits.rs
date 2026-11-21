//! Each Part's kit (ADR 0265): beside the Parts, not in them. The Parts
//! live in the UI's state in D2, which is full; the kits are an ITCM
//! static on the chip (ADR 0266), lent to the project with the pool's
//! arena.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::units::Millis;

use crate::dsp::drum::types::{Kit, PadId, PadSet};
use crate::hw::MAX_PARTS;

use super::ids::PartId;
use super::kit_library::KitMark;

/// One `Kit` a Part, reached by `PartId` only, each Part's selected
/// pad (what its pad cells edit), lit pads and PAD FOLLOW's hold. The selection, the lights and the hold are the UI's,
/// never saved; they sit here, in ITCM, since D2 is full. With them, each
/// kit's mark and a library load's scratch kit (ADR 0305), never saved
/// either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kits {
    /// Each Part's, then a library load's scratch kit (`SCRATCH`): the
    /// kit of the library Sound in the pool's scratch, which only `adopt`
    /// reads. In the one array, so the one loop builds it; no `PartId`
    /// reaches it.
    kits: [Kit; MAX_PARTS + 1],
    selected: [PadId; MAX_PARTS],
    lights: [PadLights; MAX_PARTS],
    hold: [FollowHold; MAX_PARTS],
    marks: [KitMark; MAX_PARTS],
}

/// The scratch kit's place in `Kits::kits`.
const SCRATCH: usize = MAX_PARTS;

crate::in_place::field_list!(Kits => Kits { kits, selected, lights, hold, marks });

/// A Part's lit pads: those its voices sound, and those struck since the
/// UI's last frame, so a hit too short to see sounding still flashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadLights {
    struck: PadSet,
    sounding: PadSet,
}

impl PadLights {
    pub const DARK: PadLights = PadLights {
        struck: PadSet::EMPTY,
        sounding: PadSet::EMPTY,
    };

    /// The lit pads.
    pub fn lit(&self) -> PadSet {
        self.struck.union(self.sounding)
    }

    pub fn is_lit(&self, pad: PadId) -> bool {
        self.lit().contains(pad)
    }
}

/// PAD FOLLOW IDLE's hold (ADR 0268): how long, on the UI's clock, a live
/// hit still leaves the pad where the player's last edit left it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FollowHold {
    left_ms: u16,
}

impl FollowHold {
    /// Hits move the pad.
    pub const FREE: FollowHold = FollowHold { left_ms: 0 };
    /// Just edited: the pad stays 2 s.
    pub const AFTER_EDIT: FollowHold = FollowHold { left_ms: 2_000 };

    /// `dt` on.
    pub fn after(self, dt: Millis) -> FollowHold {
        let dt = u16::try_from(dt.get()).unwrap_or(u16::MAX);
        FollowHold {
            left_ms: self.left_ms.saturating_sub(dt),
        }
    }

    pub const fn holds(self) -> bool {
        self.left_ms > 0
    }
}

impl Kits {
    pub const DEFAULT: Kits = Kits {
        kits: [Kit::DEFAULT; MAX_PARTS + 1],
        selected: [PadId::ALL[0]; MAX_PARTS],
        lights: [PadLights::DARK; MAX_PARTS],
        hold: [FollowHold::FREE; MAX_PARTS],
        marks: [KitMark::NONE; MAX_PARTS],
    };

    /// Every kit `Kit::DEFAULT`, built a kit at a time: no whole `Kits`
    /// on the stack; pad 1 selected, none lit.
    #[optimize(size)]
    pub fn init_in_place(slot: &mut MaybeUninit<Kits>) -> &mut Kits {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; kit `i` is inside `kits`;
        // every kit, `selected`, `lights`, `hold` and `marks`
        // (`field_list!` above) are written once before
        // `assume_init_mut`.
        unsafe {
            let kits = addr_of_mut!((*p).kits).cast::<Kit>();
            for i in 0..=SCRATCH {
                kits.add(i).write(Kit::init());
            }
            addr_of_mut!((*p).selected).write([PadId::ALL[0]; MAX_PARTS]);
            addr_of_mut!((*p).lights).write([PadLights::DARK; MAX_PARTS]);
            addr_of_mut!((*p).hold).write([FollowHold::FREE; MAX_PARTS]);
            addr_of_mut!((*p).marks).write([KitMark::NONE; MAX_PARTS]);
            slot.assume_init_mut()
        }
    }

    /// The default kits on the heap, leaked (host tests and tools).
    #[cfg(any(test, feature = "test-support", feature = "host-project"))]
    #[optimize(size)]
    pub fn leaked() -> &'static mut Kits {
        alloc::boxed::Box::leak(alloc::boxed::Box::new(Kits::DEFAULT))
    }

    #[optimize(size)]
    pub fn kit(&self, p: PartId) -> &Kit {
        &self.kits[p.index()]
    }

    #[optimize(size)]
    pub fn kit_mut(&mut self, p: PartId) -> &mut Kit {
        &mut self.kits[p.index()]
    }

    /// What Part `p`'s kit is weighed against (ADR 0305).
    #[inline(always)]
    pub(in crate::project) fn mark(&self, p: PartId) -> KitMark {
        self.marks[p.index()]
    }

    #[inline(always)]
    pub(in crate::project) fn set_mark(&mut self, p: PartId, m: KitMark) {
        self.marks[p.index()] = m;
    }

    /// A library load's scratch kit, for the load to write.
    #[optimize(size)]
    pub(in crate::project) fn scratch_mut(&mut self) -> &mut Kit {
        &mut self.kits[SCRATCH]
    }

    /// The scratch kit into Part `p`'s: a library load's adopt.
    #[inline(always)]
    pub(in crate::project) fn land(&mut self, p: PartId) {
        self.kits[p.index()] = self.kits[SCRATCH];
    }

    /// Part `p`'s selected pad.
    #[optimize(size)]
    pub fn selected(&self, p: PartId) -> PadId {
        self.selected[p.index()]
    }

    /// Inlined as `Project::select_pad` is (ADR 0285).
    #[inline(always)]
    pub fn select(&mut self, p: PartId, pad: PadId) {
        self.selected[p.index()] = pad;
    }

    /// Part `p`'s lit pads.
    #[optimize(size)]
    pub fn lights(&self, p: PartId) -> PadLights {
        self.lights[p.index()]
    }

    /// A live hit on Part `p`'s `pad`: lit until the next frame at least.
    #[optimize(size)]
    pub fn light(&mut self, p: PartId, pad: PadId) {
        let l = &mut self.lights[p.index()];
        l.struck = l.struck.with(pad);
    }

    /// Part `p`'s pads its voices sound, as the audio last said.
    #[optimize(size)]
    pub fn sounding(&mut self, p: PartId, pads: PadSet) {
        self.lights[p.index()].sounding = pads;
    }

    /// One UI frame, `frame` long, on: the hits before it lit only while
    /// they sound, and each PAD FOLLOW hold that much nearer its end.
    #[optimize(size)]
    pub fn fade(&mut self, frame: Millis) {
        for l in &mut self.lights {
            l.struck = PadSet::EMPTY;
        }
        for h in &mut self.hold {
            *h = h.after(frame);
        }
    }

    /// Part `p`'s PAD FOLLOW hold: what IDLE reads.
    #[optimize(size)]
    pub fn hold(&self, p: PartId) -> FollowHold {
        self.hold[p.index()]
    }

    /// A pad edit on Part `p`: IDLE's hits there leave the pad a while.
    #[optimize(size)]
    pub fn edited(&mut self, p: PartId) {
        self.hold[p.index()] = FollowHold::AFTER_EDIT;
    }

    /// Every kit (the scratch too) back to `Kit::DEFAULT` and marked so,
    /// pad 1 selected, none lit: NEW.
    #[optimize(size)]
    pub fn reset(&mut self) {
        self.kits.iter_mut().for_each(Kit::reset);
        self.marks = [KitMark::NONE; MAX_PARTS];
        self.selected = [PadId::ALL[0]; MAX_PARTS];
        self.lights = [PadLights::DARK; MAX_PARTS];
        self.hold = [FollowHold::FREE; MAX_PARTS];
    }
}
