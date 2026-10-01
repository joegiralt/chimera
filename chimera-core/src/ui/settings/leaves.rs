//! One chain per SETTINGS leaf page, from the existing System defs.

use crate::addr::BlockRef;
use crate::part::PartParams;
use crate::project::PartId;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::block_registry::{SYS_ABOUT, SYS_AUDIO, SYS_THEME, SYS_TUNING, SYS_UPDATES};
use crate::ui::page::PageLayout;

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

static CHANNELS_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&CHANNELS)];
static OUTPUTS_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&OUTPUTS)];
static TUNING_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_TUNING)];
static THEME_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_THEME)];
static UPDATES_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_UPDATES)];
static ABOUT_BLOCKS: [ChainBlock; 1] = [ChainBlock::with_subs(&SYS_ABOUT, &[&SYS_AUDIO])];

const fn leaf(name: &'static str, blocks: &'static [ChainBlock]) -> ChainDef2 {
    ChainDef2::new(name, blocks, &[])
}

pub static CHANNELS_LEAF: ChainDef2 = leaf("Channels", &CHANNELS_BLOCKS);
pub static OUTPUTS_LEAF: ChainDef2 = leaf("Outputs", &OUTPUTS_BLOCKS);
pub static TUNING_LEAF: ChainDef2 = leaf("Tuning", &TUNING_BLOCKS);
pub static THEME_LEAF: ChainDef2 = leaf("Theme", &THEME_BLOCKS);
pub static UPDATES_LEAF: ChainDef2 = leaf("Updates", &UPDATES_BLOCKS);
pub static ABOUT_LEAF: ChainDef2 = leaf("About", &ABOUT_BLOCKS);
