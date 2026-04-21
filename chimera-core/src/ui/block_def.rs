use crate::ui::page::{CellIcon, PageLayout, ValFmt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VizType {
    None,
    FilterResponse,
    Adsr,
    AlgorithmDiagram,
    EqResponse,
    LpgResponse,
    WaveformPreview,
    Logo,
    ModalPeaks,
    DriveClip,
    WaveFold,
    EffectsFlow,
    MixerLevels,
    RoutingMatrix,
    CompressorCurve,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamSlot {
    pub label: &'static str,
    pub format: ValFmt,
    pub icon: CellIcon,
}

#[derive(Clone, Copy, Debug)]
pub struct BlockDef {
    pub name: &'static str,
    pub short: &'static str,
    pub layout: PageLayout,
    pub viz: VizType,
    pub params: [ParamSlot; 6],
}

#[derive(Clone, Copy, Debug)]
pub struct ChainBlock {
    pub def: &'static BlockDef,
    pub sub_pages: &'static [&'static BlockDef],
}

impl ChainBlock {
    pub fn active_def(&self, sub_page: usize) -> &'static BlockDef {
        if self.sub_pages.is_empty() || sub_page == 0 {
            self.def
        } else {
            self.sub_pages.get(sub_page - 1).copied().unwrap_or(self.def)
        }
    }

    pub fn sub_page_count(&self) -> usize {
        if self.sub_pages.is_empty() {
            0
        } else {
            1 + self.sub_pages.len()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ChainDef2 {
    pub name: &'static str,
    pub blocks: &'static [ChainBlock],
}

impl ChainDef2 {
    pub fn block_at(&self, node: usize) -> Option<&'static ChainBlock> {
        self.blocks.get(node)
    }

    pub fn active_def(&self, node: usize, sub_page: usize) -> Option<&'static BlockDef> {
        self.block_at(node).map(|b| b.active_def(sub_page))
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }
}
