//! One chain per SETTINGS leaf page, from the existing System defs.

use crate::addr::BlockRef;
use crate::part::PartParams;
use crate::project::PartId;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::block_registry::{DEMO_BLOCKS, SYS_ABOUT, SYS_AUDIO, SYS_THEME, SYS_TUNING};
use crate::ui::page::{PageId, PageLayout};

/// One cell per Part, `P1`–`P6`, each Part's `param`: the mixer's own
/// value, so editing either edits both.
const fn per_part(param: crate::block::ParamId) -> [ParamSlot; 6] {
    const LABELS: [&str; 6] = ["P1", "P2", "P3", "P4", "P5", "P6"];
    let mut slots = [ParamSlot::EMPTY; 6];
    let mut i = 0;
    while i < 6 {
        slots[i] = ParamSlot::param(BlockRef::PartMix(PartId::ALL[i]), param).with_label(LABELS[i]);
        i += 1;
    }
    slots
}

/// MIDI CONFIG > CHANNELS.
pub static CHANNELS: BlockDef = BlockDef {
    id: 68,
    name: "Channels",
    short: "CHN",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: per_part(PartParams::CHANNEL),
};

/// AUDIO ROUTING > OUTPUTS.
pub static OUTPUTS: BlockDef = BlockDef {
    id: 69,
    name: "Outputs",
    short: "OUT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: per_part(PartParams::OUTPUT),
};

/// A SETTINGS leaf: one page, no sub-pages, so no key in it steps a page.
/// A leaf with sub-pages fails the build:
///
/// ```compile_fail,E0080
/// use chimera_core::ui::block_def::ChainBlock;
/// use chimera_core::ui::block_registry::{SYS_ABOUT, SYS_AUDIO};
/// use chimera_core::ui::settings::leaves::OnePage;
/// static B: [ChainBlock; 1] = [ChainBlock::with_subs(&SYS_ABOUT, &[&SYS_AUDIO])];
/// static BAD: OnePage = OnePage::new("About", &B);
/// ```
#[derive(Debug)]
pub struct OnePage(ChainDef2);

impl OnePage {
    pub const fn new(name: &'static str, block: &'static [ChainBlock; 1]) -> OnePage {
        assert!(
            block[0].sub_pages.is_empty(),
            "a SETTINGS leaf has one page"
        );
        OnePage(ChainDef2::new(name, block, &[]))
    }

    pub const fn chain(&'static self) -> &'static ChainDef2 {
        &self.0
    }

    pub const fn def(&self) -> &'static BlockDef {
        self.0.blocks[0].def
    }

    /// Nothing to edit: no bound param, no legacy page table entry, and
    /// no matrix (its encoders edit the grid).
    pub fn read_only(&self) -> bool {
        let def = self.def();
        def.layout != PageLayout::Matrix
            && PageId::of_leaf(def)
                .is_some_and(|p| (0..def.params.len()).all(|i| p.binding(i).is_none()))
    }
}

static CHANNELS_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&CHANNELS)];
static OUTPUTS_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&OUTPUTS)];
static TUNING_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_TUNING)];
static THEME_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_THEME)];
static ABOUT_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_ABOUT)];
static AUDIO_LOAD_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_AUDIO)];

pub static CHANNELS_LEAF: OnePage = OnePage::new("Channels", &CHANNELS_BLOCKS);
pub static OUTPUTS_LEAF: OnePage = OnePage::new("Outputs", &OUTPUTS_BLOCKS);
pub static TUNING_LEAF: OnePage = OnePage::new("Tuning", &TUNING_BLOCKS);
pub static THEME_LEAF: OnePage = OnePage::new("Theme", &THEME_BLOCKS);
pub static ABOUT_LEAF: OnePage = OnePage::new("About", &ABOUT_BLOCKS);
pub static AUDIO_LOAD_LEAF: OnePage = OnePage::new("Audio Load", &AUDIO_LOAD_BLOCKS);

/// One leaf per DEMO page, in `DEMO_BLOCKS`' order.
pub static DEMO_LEAVES: [OnePage; DEMO_BLOCKS.len()] = {
    const NO: OnePage = OnePage::new("Demo", &ABOUT_BLOCKS);
    let mut l = [NO; DEMO_BLOCKS.len()];
    let mut i = 0;
    while i < l.len() {
        l[i] = OnePage::new(
            DEMO_BLOCKS[i].def.name,
            core::array::from_ref(&DEMO_BLOCKS[i]),
        );
        i += 1;
    }
    l
};
