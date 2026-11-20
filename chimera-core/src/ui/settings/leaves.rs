//! One chain per SETTINGS leaf page, from the existing System defs.

use crate::addr::BlockRef;
use crate::part::PartParams;
use crate::project::PartId;
use crate::randomize::Randomize;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2, ParamSlot, VizType};
use crate::ui::block_registry::{
    DEMO_BLOCKS, SYS_ABOUT, SYS_AUDIO, SYS_ECHO, SYS_FOLLOW, SYS_MAP, SYS_MPE, SYS_POOL, SYS_PUSH,
    SYS_SAVER, SYS_SYNC, SYS_TEXT, SYS_THEME, SYS_TUNING,
};
use crate::ui::midi_settings::MidiSettings;
use crate::ui::page::{PageId, PageLayout};

/// One cell per Part, `P1`–`P6`, each Part's `param`: the mixer's own
/// value, so editing either edits both.
const fn per_part(param: crate::block::ParamId) -> [ParamSlot; 6] {
    static LABELS: [&str; 6] = ["P1", "P2", "P3", "P4", "P5", "P6"];
    let mut slots = [ParamSlot::EMPTY; 6];
    let mut i = 0;
    while i < 6 {
        slots[i] =
            ParamSlot::param(BlockRef::PartMix(PartId::ALL[i]), param).with_label(&LABELS[i]);
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

/// MIDI CONFIG › CONTROL CHANNELS (ADR 0254): MIXER, SEQUENCER and AUTO,
/// each OFF or a channel. One a Part's own channel takes reads dimmed.
/// D: BUILT-IN, each Part's GM CCs on its own channel (ADR 0263).
pub static CONTROL: BlockDef = BlockDef {
    // Never used before: a new top, past MPE's 113 (#896), one more
    // focus slot (`MAX_PAGES`) each.
    id: 114,
    name: "Control",
    short: "CTL",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Midi, MidiSettings::MIXER),
        ParamSlot::param(BlockRef::Midi, MidiSettings::SEQUENCER),
        ParamSlot::param(BlockRef::Midi, MidiSettings::AUTO),
        ParamSlot::param(BlockRef::Midi, MidiSettings::BUILT_IN),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
    ],
};

/// AUDIO ROUTING › USB AUDIO (ADR 0260): USB IN, the host's playback
/// into the main out (Elektron's USB TO MAIN), OFF to 0 dB.
#[cfg(feature = "usb-audio")]
pub static USB_AUDIO: BlockDef = BlockDef {
    // Never used before: a new top, past CONTROL's 114, one more focus
    // slot (`MAX_PAGES`).
    id: 115,
    name: "USB Audio",
    short: "USB",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Midi, MidiSettings::USB_IN),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
    ],
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

/// PART > LIVE SNAP: the active Part's SNAP and HELD (spec § 5.2,
/// owner Q6), bound as the mixer's PART page binds its cells.
pub static LIVE_SNAP: BlockDef = BlockDef {
    id: 85,
    name: "Live Snap",
    short: "SNP",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Part, PartParams::SNAP),
        ParamSlot::param(BlockRef::Part, PartParams::HELD),
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
        ParamSlot::EMPTY,
    ],
};

/// PART › RANDOMIZE (ADR 0232): four amounts and the seed. A turn of any
/// rolls the active Part's Sound.
pub static RANDOM: BlockDef = BlockDef {
    id: 112,
    name: "Randomize",
    short: "RND",
    layout: PageLayout::CellGrid,
    viz: VizType::None,
    params: [
        ParamSlot::param(BlockRef::Random, Randomize::OSC),
        ParamSlot::param(BlockRef::Random, Randomize::ENV),
        ParamSlot::param(BlockRef::Random, Randomize::TONE),
        ParamSlot::param(BlockRef::Random, Randomize::MOD),
        ParamSlot::param(BlockRef::Random, Randomize::SEED),
        ParamSlot::EMPTY,
    ],
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
static CONTROL_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&CONTROL)];
static SYNC_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_SYNC)];
static ECHO_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_ECHO)];
static MPE_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_MPE)];
static OUTPUTS_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&OUTPUTS)];
#[cfg(feature = "usb-audio")]
static USB_AUDIO_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&USB_AUDIO)];
static TUNING_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_TUNING)];
static THEME_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_THEME)];
static SAVER_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_SAVER)];
static MAP_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_MAP)];
static TEXT_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_TEXT)];
static FOLLOW_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_FOLLOW)];
static ALGO_PAGES_BLOCKS: [ChainBlock; 1] =
    [ChainBlock::page(&crate::ui::block_registry::SYS_ALGO_PAGES)];
static PUSH_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_PUSH)];
static ABOUT_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_ABOUT)];
static AUDIO_LOAD_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_AUDIO)];
static LIVE_SNAP_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&LIVE_SNAP)];
static RANDOM_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&RANDOM)];
static POOL_BLOCKS: [ChainBlock; 1] = [ChainBlock::page(&SYS_POOL)];

pub static CHANNELS_LEAF: OnePage = OnePage::new("Channels", &CHANNELS_BLOCKS);
pub static CONTROL_LEAF: OnePage = OnePage::new("Control", &CONTROL_BLOCKS);
pub static SYNC_LEAF: OnePage = OnePage::new("Sync", &SYNC_BLOCKS);
pub static ECHO_LEAF: OnePage = OnePage::new("Echo", &ECHO_BLOCKS);
pub static MPE_LEAF: OnePage = OnePage::new("MPE", &MPE_BLOCKS);
pub static OUTPUTS_LEAF: OnePage = OnePage::new("Outputs", &OUTPUTS_BLOCKS);
#[cfg(feature = "usb-audio")]
pub static USB_AUDIO_LEAF: OnePage = OnePage::new("USB Audio", &USB_AUDIO_BLOCKS);
pub static TUNING_LEAF: OnePage = OnePage::new("Tuning", &TUNING_BLOCKS);
pub static THEME_LEAF: OnePage = OnePage::new("Theme", &THEME_BLOCKS);
pub static SAVER_LEAF: OnePage = OnePage::new("Screensaver", &SAVER_BLOCKS);
pub static MAP_LEAF: OnePage = OnePage::new("Chain Map", &MAP_BLOCKS);
pub static TEXT_LEAF: OnePage = OnePage::new("Text Size", &TEXT_BLOCKS);
pub static FOLLOW_LEAF: OnePage = OnePage::new("Pad Follow", &FOLLOW_BLOCKS);
pub static ALGO_PAGES_LEAF: OnePage = OnePage::new("Algo Pages", &ALGO_PAGES_BLOCKS);
pub static PUSH_LEAF: OnePage = OnePage::new("Push Encoders", &PUSH_BLOCKS);
pub static ABOUT_LEAF: OnePage = OnePage::new("About", &ABOUT_BLOCKS);
pub static AUDIO_LOAD_LEAF: OnePage = OnePage::new("Audio Load", &AUDIO_LOAD_BLOCKS);
pub static LIVE_SNAP_LEAF: OnePage = OnePage::new("Live Snap", &LIVE_SNAP_BLOCKS);
pub static RANDOM_LEAF: OnePage = OnePage::new("Randomize", &RANDOM_BLOCKS);
pub static POOL_LEAF: OnePage = OnePage::new("Pool", &POOL_BLOCKS);

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
