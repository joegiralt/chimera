//! One chain per SETTINGS leaf page, from the existing System defs.

use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::block_registry::{SYS_ABOUT, SYS_AUDIO, SYS_THEME, SYS_TUNING, SYS_UPDATES};
use crate::ui::page::PageLayout;

const EMPTY: ParamSlot = ParamSlot::EMPTY;

/// MIDI CONFIG > CHANNELS; Task 7 binds the slots.
pub static CHANNELS: BlockDef = BlockDef {
    id: 68,
    name: "Channels",
    short: "CHN",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [EMPTY; 6],
};

/// AUDIO ROUTING > OUTPUTS; Task 7 binds the slots.
pub static OUTPUTS: BlockDef = BlockDef {
    id: 69,
    name: "Outputs",
    short: "OUT",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [EMPTY; 6],
};

static CHANNELS_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&CHANNELS)];
static OUTPUTS_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&OUTPUTS)];
static TUNING_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_TUNING)];
static THEME_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_THEME)];
static UPDATES_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_UPDATES)];
static ABOUT_BLOCKS: [ChainBlock; 1] = [ChainBlock::with_subs(&SYS_ABOUT, &[&SYS_AUDIO])];

const fn leaf(name: &'static str, blocks: &'static [ChainBlock]) -> ChainDef2 {
    ChainDef2 {
        name,
        blocks,
        mod_sources: &[],
    }
}

pub static CHANNELS_LEAF: ChainDef2 = leaf("Channels", &CHANNELS_BLOCKS);
pub static OUTPUTS_LEAF: ChainDef2 = leaf("Outputs", &OUTPUTS_BLOCKS);
pub static TUNING_LEAF: ChainDef2 = leaf("Tuning", &TUNING_BLOCKS);
pub static THEME_LEAF: ChainDef2 = leaf("Theme", &THEME_BLOCKS);
pub static UPDATES_LEAF: ChainDef2 = leaf("Updates", &UPDATES_BLOCKS);
pub static ABOUT_LEAF: ChainDef2 = leaf("About", &ABOUT_BLOCKS);
