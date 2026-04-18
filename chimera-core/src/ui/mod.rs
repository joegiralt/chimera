pub mod animation;
pub mod cell;
pub mod chain;
pub mod dungeon_map;
pub mod fmt;
pub mod page;
pub mod perf;
pub mod renderer;
pub mod theme;

use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId};

use crate::params::{EngineType, ParamSnapshot};
use chain::ChainNav;
use page::PageId;
use perf::PerfStats;
use renderer::Renderer;

/// Top-level UI state. Owns navigation, parameters, and display animation.
/// Portable across desktop and hardware — only depends on HAL traits.
pub struct UiState {
    pub nav: ChainNav,
    pub params: ParamSnapshot,
    pub renderer: Renderer,
    page: PageId,
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

impl UiState {
    pub fn new() -> Self {
        let nav = ChainNav::new();
        let params = ParamSnapshot::default();
        let page = PageId::from_nav(&nav);
        let mut renderer = Renderer::new();
        renderer.snap_to_current(page, &params);

        Self {
            nav,
            params,
            renderer,
            page,
        }
    }

    /// Current page id.
    pub fn page(&self) -> PageId {
        self.page
    }

    /// Process one frame of input: navigation + encoder deltas.
    pub fn handle_input(&mut self, controls: &impl Controls) {
        // Navigation
        let nav_changed = self.nav.handle_input(controls);
        if nav_changed {
            self.page = PageId::from_nav(&self.nav);
            self.renderer.snap_to_current(self.page, &self.params);

            // Switch engine type based on which engine sub-page is active
            self.params.engine = match self.page {
                PageId::EngineFmA | PageId::EngineFmB | PageId::EngineFmC => EngineType::Fm,
                PageId::EngineModal1 | PageId::EngineModal2 => EngineType::Modal,
                PageId::EngineVa => EngineType::Va,
                _ => self.params.engine, // keep current
            };
        }

        // Encoder deltas -> parameter changes
        let shift = matches!(
            controls.button_state(ButtonId::Mix),
            ButtonState::Pressed | ButtonState::Held
        );

        let encoder_ids = [
            EncoderId::A,
            EncoderId::B,
            EncoderId::C,
            EncoderId::D,
            EncoderId::E,
            EncoderId::F,
        ];
        for (i, &enc) in encoder_ids.iter().enumerate() {
            let delta = controls.encoder_delta(enc);
            if delta != 0 {
                if shift {
                    self.page
                        .snap_encoder(i, delta, &mut self.params);
                } else {
                    self.page.apply_encoder(i, delta, &mut self.params);
                }
            }
        }
    }

    /// Advance animations. Call at 30fps.
    pub fn update(&mut self) {
        self.renderer.update(self.page, &self.params);
    }

    /// Render full screen to a display.
    pub fn render<D>(&self, display: &mut D, perf: &PerfStats)
    where
        D: embedded_graphics::draw_target::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
    {
        self.renderer
            .draw(display, &self.nav, self.page, perf);
    }
}
