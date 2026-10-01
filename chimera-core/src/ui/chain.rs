use crate::params::EngineType;
use crate::ui::block_def::{BlockDef, ChainBlock, ChainDef2};
use crate::ui::block_registry;
use chimera_hal::{ButtonId, ButtonState, Controls, PART_BUTTONS};

/// Moved to `ui::nav`; re-exported until `UiState` runs it (Task 8).
pub use super::nav::chain_def_for;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainId {
    Part(usize),  // 0-5 (B1-B6)
    Mixer(usize), // 0-5 (MIX + B1-B6, but MIX+B6 = Demo)
    System,       // MENU
    Demo,         // MIX + B6
}

#[derive(Clone, Copy, Debug)]
pub struct ChainNav {
    pub chain_id: ChainId,
    pub node: usize,
    pub sub_page: usize,
    /// The active Part's engine (resolves ChainId::Part to its chain def).
    pub engine: EngineType,
    /// The last Part sound page left, with the engine it was on.
    sound_left: SoundPage,
    /// The mixer page last used, for every Part (ADR 0057).
    mix_page: (usize, usize),
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
            engine: EngineType::Algo,
            sound_left: SoundPage {
                part: 0,
                engine: EngineType::Algo,
                node: 0,
                sub_page: 0,
            },
            mix_page: (block_registry::MIXER_HOME, 0),
        }
    }

    /// Move to `to`: the mixer on its last page (PART only from another
    /// mixer: from outside it opens SENDS), Part n's sound pages on the
    /// page left when coming from its mixer, anything else home (ADR 0057).
    pub fn go(&mut self, to: ChainId) {
        let from = self.chain_id;
        match from {
            ChainId::Part(part) => {
                self.sound_left = SoundPage {
                    part,
                    engine: self.engine,
                    node: self.node,
                    sub_page: self.sub_page,
                }
            }
            ChainId::Mixer(_) => self.mix_page = (self.node, self.sub_page),
            ChainId::System | ChainId::Demo => {}
        }
        let left = self.sound_left;
        (self.node, self.sub_page) = match (from, to) {
            (ChainId::Mixer(_), ChainId::Mixer(_)) => self.mix_page,
            (_, ChainId::Mixer(_)) if self.mix_page.0 == block_registry::MIXER_PART => {
                (block_registry::MIXER_HOME, 0)
            }
            (_, ChainId::Mixer(_)) => self.mix_page,
            (ChainId::Mixer(m), ChainId::Part(n)) if m == n && left.part == n => {
                self.engine = left.engine;
                (left.node, left.sub_page)
            }
            _ => (0, 0),
        };
        self.chain_id = to;
    }

    /// The active Part's engine: a Part page kept for another engine, or
    /// a position off the chain, goes home.
    pub fn set_engine(&mut self, engine: EngineType) {
        if matches!(self.chain_id, ChainId::Part(_)) && engine != self.engine {
            (self.node, self.sub_page) = (0, 0);
        }
        self.engine = engine;
        let subs = self.active_chain_block().map(|b| b.sub_page_count().max(1));
        if subs.is_none_or(|n| self.sub_page >= n) {
            (self.node, self.sub_page) = (0, 0);
        }
    }

    /// Get the chain definition for the current ChainId.
    /// For `Part(_)`, resolves via the stored `engine` (set by UiState from the active part).
    pub fn active_chain(&self) -> &'static ChainDef2 {
        match self.chain_id {
            ChainId::Part(_) => chain_def_for(self.engine),
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

        // MENU = System (again: home)
        if controls.button_state(ButtonId::Menu) == ButtonState::Pressed {
            self.go(ChainId::System);
        }

        // B<n>: toggles Part n's sound and mixer; MIX + B<n>: its mixer,
        // MIX + B6 the demo.
        for (i, &btn) in PART_BUTTONS.iter().enumerate() {
            if controls.button_state(btn) == ButtonState::Pressed {
                self.go(match (mix_held, i) {
                    (true, 5) => ChainId::Demo,
                    (true, _) => ChainId::Mixer(i),
                    (false, _) => next_on_part_button(self.chain_id, i),
                });
            }
        }

        // Left/Right: horizontal navigation (only when MIX is not held)
        if !mix_held {
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
        }

        // Up/Down: vertical sub-page navigation
        if controls.button_state(ButtonId::Seq) == ButtonState::Pressed && self.sub_page > 0 {
            self.sub_page -= 1;
        }
        if controls.button_state(ButtonId::Edit) == ButtonState::Pressed
            && let Some(block) = self.active_chain_block()
        {
            let count = block.sub_page_count();
            if count > 0 && self.sub_page + 1 < count {
                self.sub_page += 1;
            }
        }

        // Return whether position changed
        self.chain_id != prev_chain_id || self.node != prev_node || self.sub_page != prev_sub
    }
}

/// A Part sound page, and the engine whose chain it is on.
#[derive(Clone, Copy, Debug)]
struct SoundPage {
    part: usize,
    engine: EngineType,
    node: usize,
    sub_page: usize,
}

/// Where B<n> goes from `from`: Part n's sound pages and its mixer swap;
/// from anywhere else, its sound pages (ADR 0057).
pub const fn next_on_part_button(from: ChainId, n: usize) -> ChainId {
    match from {
        ChainId::Part(p) if p == n => ChainId::Mixer(n),
        _ => ChainId::Part(n),
    }
}
