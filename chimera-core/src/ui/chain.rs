use chimera_hal::{ButtonId, ButtonState, Controls};
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2};
use crate::ui::block_registry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainId {
    Part(usize),    // 0-5 (B1-B6)
    Mixer(usize),   // 0-5 (MIX + B1-B6, but MIX+B6 = Demo)
    System,         // MENU
    Demo,           // MIX + B6
}

#[derive(Clone, Copy, Debug)]
pub struct ChainNav {
    pub chain_id: ChainId,
    pub node: usize,
    pub sub_page: usize,
}

impl Default for ChainNav {
    fn default() -> Self {
        Self::new()
    }
}

impl ChainNav {
    pub const fn new() -> Self {
        Self {
            chain_id: ChainId::Part(0),
            node: 0,
            sub_page: 0,
        }
    }

    /// Get the chain definition for the current ChainId.
    pub fn active_chain(&self) -> &'static ChainDef2 {
        match self.chain_id {
            // All parts currently default to FM_POLY_CHAIN.
            // In the future each Part will have its own chain.
            ChainId::Part(_) => &block_registry::FM_POLY_CHAIN,
            ChainId::Mixer(_) => &block_registry::MIXER_CHANNEL_CHAIN,
            ChainId::System => &block_registry::SYSTEM_CHAIN,
            ChainId::Demo => &block_registry::DEMO_CHAIN,
        }
    }

    /// Resolve the full position (chain + node + sub_page) to a BlockDef.
    pub fn active_block_def(&self) -> &'static BlockDef {
        let chain = self.active_chain();
        chain
            .active_def(self.node, self.sub_page)
            .unwrap_or(chain.blocks[0].def)
    }

    /// Get the current ChainBlock (node with sub-page info).
    pub fn active_chain_block(&self) -> Option<&'static ChainBlock> {
        self.active_chain().block_at(self.node)
    }

    /// Process control input and update navigation state.
    /// Returns true if position changed.
    pub fn handle_input(&mut self, controls: &impl Controls) -> bool {
        let prev_chain_id = self.chain_id;
        let prev_node = self.node;
        let prev_sub = self.sub_page;

        let mix_held = matches!(
            controls.button_state(ButtonId::Mix),
            ButtonState::Pressed | ButtonState::Held
        );

        // MENU = System
        if controls.button_state(ButtonId::Menu) == ButtonState::Pressed {
            if self.chain_id == ChainId::System {
                self.node = 0;
                self.sub_page = 0;
            } else {
                self.chain_id = ChainId::System;
                self.node = 0;
                self.sub_page = 0;
            }
        }

        // B1-B6: Part or Mixer depending on MIX modifier
        let chain_buttons = [
            ButtonId::B1,
            ButtonId::B2,
            ButtonId::B3,
            ButtonId::B4,
            ButtonId::B5,
            ButtonId::B6,
        ];
        for (i, &btn) in chain_buttons.iter().enumerate() {
            if controls.button_state(btn) == ButtonState::Pressed {
                let target = if mix_held {
                    if i == 5 {
                        ChainId::Demo
                    } else {
                        ChainId::Mixer(i)
                    }
                } else {
                    ChainId::Part(i)
                };

                if self.chain_id == target {
                    // Same chain pressed again = snap home
                    self.node = 0;
                    self.sub_page = 0;
                } else {
                    self.chain_id = target;
                    self.node = 0;
                    self.sub_page = 0;
                }
            }
        }

        // Left/Right: horizontal navigation
        if controls.button_state(ButtonId::Minus) == ButtonState::Pressed && self.node > 0 {
            self.node -= 1;
            self.sub_page = 0;
        }
        if controls.button_state(ButtonId::Plus) == ButtonState::Pressed {
            let chain = self.active_chain();
            if self.node + 1 < chain.len() {
                self.node += 1;
                self.sub_page = 0;
            }
        }

        // Up/Down: vertical sub-page navigation
        if controls.button_state(ButtonId::Seq) == ButtonState::Pressed && self.sub_page > 0 {
            self.sub_page -= 1;
        }
        if controls.button_state(ButtonId::Edit) == ButtonState::Pressed {
            if let Some(block) = self.active_chain_block() {
                let count = block.sub_page_count();
                if count > 0 && self.sub_page + 1 < count {
                    self.sub_page += 1;
                }
            }
        }

        // Return whether position changed
        self.chain_id != prev_chain_id
            || self.node != prev_node
            || self.sub_page != prev_sub
    }
}
