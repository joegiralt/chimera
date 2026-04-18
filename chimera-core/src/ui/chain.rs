use chimera_hal::{ButtonId, ButtonState, Controls};

/// A node in a chain (horizontal position).
#[derive(Clone, Copy, Debug)]
pub struct NodeDef {
    /// Full name shown in header
    pub name: &'static str,
    /// 3-char abbreviation for dungeon map
    pub short: &'static str,
    /// Vertical sub-pages at this node (empty = no branches)
    pub sub_pages: &'static [&'static str],
}

/// A chain definition (one per button 1-6).
#[derive(Clone, Copy, Debug)]
pub struct ChainDef {
    pub name: &'static str,
    pub nodes: &'static [NodeDef],
}

// --- Chain definitions matching design spec ---

pub static VOICE_CHAIN: ChainDef = ChainDef {
    name: "VOICE",
    nodes: &[
        NodeDef {
            name: "Engine",
            short: "ENG",
            sub_pages: &["FM-A", "FM-B", "FM-C", "MODAL", "VA"],
        },
        NodeDef {
            name: "Drive",
            short: "DRV",
            sub_pages: &[],
        },
        NodeDef {
            name: "Filter",
            short: "FLT",
            sub_pages: &[],
        },
        NodeDef {
            name: "Folder",
            short: "FLD",
            sub_pages: &[],
        },
        NodeDef {
            name: "VCA",
            short: "VCA",
            sub_pages: &[],
        },
        NodeDef {
            name: "Effects",
            short: "EFX",
            sub_pages: &[],
        },
    ],
};

pub static MIX_CHAIN: ChainDef = ChainDef {
    name: "MIX",
    nodes: &[
        NodeDef {
            name: "Mixer",
            short: "MIX",
            sub_pages: &["CH 1", "CH 2", "CH 3", "CH 4"],
        },
        NodeDef {
            name: "Routing",
            short: "RTG",
            sub_pages: &[],
        },
        NodeDef {
            name: "Drive",
            short: "DRV",
            sub_pages: &[],
        },
        NodeDef {
            name: "Comp",
            short: "CMP",
            sub_pages: &[],
        },
        NodeDef {
            name: "Global FX",
            short: "GFX",
            sub_pages: &[],
        },
    ],
};

pub static ENVELOPE_CHAIN: ChainDef = ChainDef {
    name: "ENV",
    nodes: &[
        NodeDef {
            name: "Amp",
            short: "AMP",
            sub_pages: &[],
        },
        NodeDef {
            name: "Filter",
            short: "FLT",
            sub_pages: &[],
        },
        NodeDef {
            name: "Aux",
            short: "AUX",
            sub_pages: &[],
        },
    ],
};

pub static DEMO_CHAIN: ChainDef = ChainDef {
    name: "DEMO",
    nodes: &[
        NodeDef {
            name: "Waves",
            short: "WAV",
            sub_pages: &[],
        },
        NodeDef {
            name: "Shapes",
            short: "SHP",
            sub_pages: &[],
        },
        NodeDef {
            name: "Motion",
            short: "MOT",
            sub_pages: &[],
        },
    ],
};

/// All defined chains, indexed by button number (0-based).
pub static CHAINS: [Option<&ChainDef>; 6] = [
    Some(&VOICE_CHAIN),
    Some(&MIX_CHAIN),
    Some(&ENVELOPE_CHAIN),
    None,
    None,
    Some(&DEMO_CHAIN),
];

/// 2D navigation position within the chain system.
#[derive(Clone, Copy, Debug)]
pub struct ChainNav {
    /// Which chain (0-5, maps to buttons 1-6)
    pub chain: usize,
    /// Horizontal position within chain
    pub node: usize,
    /// Vertical sub-page at current node
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
            chain: 0,
            node: 0,
            sub_page: 0,
        }
    }

    /// Get the current chain definition, if it exists.
    pub fn chain_def(&self) -> Option<&'static ChainDef> {
        CHAINS.get(self.chain).copied().flatten()
    }

    /// Get the current node definition.
    pub fn node_def(&self) -> Option<&'static NodeDef> {
        self.chain_def()
            .and_then(|c| c.nodes.get(self.node))
    }

    /// Process control input and update navigation state.
    /// Returns true if position changed.
    pub fn handle_input(&mut self, controls: &impl Controls) -> bool {
        let prev = *self;

        // Button 1-6: jump to chain head
        let chain_buttons = [
            ButtonId::B1,
            ButtonId::B2,
            ButtonId::B3,
            ButtonId::B4,
            ButtonId::B5,
            ButtonId::B6,
        ];
        for (i, &btn) in chain_buttons.iter().enumerate() {
            if controls.button_state(btn) == ButtonState::Pressed
                && let Some(Some(_)) = CHAINS.get(i) {
                    if self.chain == i {
                        // Same button = snap home
                        self.node = 0;
                        self.sub_page = 0;
                    } else {
                        self.chain = i;
                        self.node = 0;
                        self.sub_page = 0;
                    }
                }
        }

        // Left/Right: horizontal navigation
        if controls.button_state(ButtonId::Minus) == ButtonState::Pressed
            && self.node > 0 {
                self.node -= 1;
                self.sub_page = 0;
            }
        if controls.button_state(ButtonId::Plus) == ButtonState::Pressed
            && let Some(chain) = self.chain_def()
                && self.node + 1 < chain.nodes.len() {
                    self.node += 1;
                    self.sub_page = 0;
                }

        // Up/Down: vertical sub-page navigation
        if controls.button_state(ButtonId::Seq) == ButtonState::Pressed
            && self.sub_page > 0 {
                self.sub_page -= 1;
            }
        if controls.button_state(ButtonId::Edit) == ButtonState::Pressed
            && let Some(node) = self.node_def()
                && !node.sub_pages.is_empty() && self.sub_page + 1 < node.sub_pages.len() {
                    self.sub_page += 1;
                }

        // Return whether position changed
        self.chain != prev.chain || self.node != prev.node || self.sub_page != prev.sub_page
    }
}
