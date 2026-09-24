//! The playable instrument (instrument-core spec § Audio path, § Threading):
//! what the audio thread reads from the UI, and the voice pool that renders
//! every Part into the three DAC pairs.

use core::mem::size_of;

use crate::dsp::fx_bus::{FxBus, FxParams};
use crate::hw::{AXI_SRAM, FB_BYTES, MAX_PARTS, UI_RESERVE};
use crate::modulation::ModState;
use crate::params::ParamSnapshot;
use crate::part::PartParams;
use crate::preset::{Performance, SoundPool};

/// Everything the port places in AXI SRAM (ADR 0014): framebuffer, UI,
/// Performance, SoundPool, both `AudioShared` copies and the FX bus.
pub const AXI_RESIDENT: usize = FB_BYTES
    + UI_RESERVE
    + size_of::<Performance>()
    + size_of::<SoundPool>()
    + 2 * size_of::<AudioShared>()
    + size_of::<FxBus>();
const _: () = assert!(AXI_RESIDENT <= AXI_SRAM);

/// One Part as the audio thread sees it.
#[derive(Clone, Debug)]
pub struct PartAudio {
    pub params: ParamSnapshot,
    pub mod_state: ModState,
    pub mix: PartParams,
}

/// The Performance state the audio needs, double-buffered by the platform
/// (one pointer swap per UI frame). The Sound names, pool and UI stay behind.
#[derive(Clone, Debug)]
pub struct AudioShared {
    pub parts: [PartAudio; MAX_PARTS],
    pub fx: FxParams,
}

impl Default for AudioShared {
    fn default() -> Self {
        Self::from_performance(&Performance::new())
    }
}

impl AudioShared {
    pub fn from_performance(perf: &Performance) -> Self {
        Self {
            parts: core::array::from_fn(|i| {
                let p = &perf.parts[i];
                PartAudio { params: p.sound.params.clone(), mod_state: p.sound.mod_state.clone(), mix: p.mix }
            }),
            fx: perf.fx,
        }
    }

    /// Overwrite with `perf` in place (the UI's per-frame copy into the back
    /// buffer; no allocation).
    pub fn update_from(&mut self, perf: &Performance) {
        for (dst, src) in self.parts.iter_mut().zip(&perf.parts) {
            dst.params.clone_from(&src.sound.params);
            dst.mod_state.clone_from(&src.sound.mod_state);
            dst.mix = src.mix;
        }
        self.fx = perf.fx;
    }
}
