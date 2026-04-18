use chimera_hal::BLOCK_SIZE;

use crate::dsp::drive::Drive;
use crate::dsp::envelope::Envelope;
use crate::dsp::filter::SvfFilter;
use crate::dsp::fm::FmEngine;
use crate::dsp::modal::ModalEngine;
use crate::dsp::wavefolder::Wavefolder;
use crate::params::{EngineType, ParamSnapshot};

/// Complete voice signal chain:
/// [Engine (FM/Modal/VA)] → [Drive] → [Filter] → [Wavefolder] → [VCA]
pub struct Voice {
    pub fm: FmEngine,
    pub modal: ModalEngine,
    drive: Drive,
    filter: SvfFilter,
    folder: Wavefolder,
    amp_env: Envelope,
    active_engine: EngineType,
    active: bool,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

impl Voice {
    pub fn new() -> Self {
        Self {
            fm: FmEngine::new(),
            modal: ModalEngine::new(),
            drive: Drive::new(),
            filter: SvfFilter::new(),
            folder: Wavefolder::new(),
            amp_env: Envelope::new(),
            active_engine: EngineType::Fm,
            active: false,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ParamSnapshot, sample_rate: u32) {
        self.active_engine = params.engine;
        match self.active_engine {
            EngineType::Fm => {
                self.fm.update_params(&params.fm, sample_rate);
                self.fm.note_on(note, velocity, sample_rate);
            }
            EngineType::Modal => {
                self.modal.note_on(note, velocity, &params.modal, sample_rate);
            }
            EngineType::Va => {
                // VA engine not yet implemented — fall back to FM
                self.fm.update_params(&params.fm, sample_rate);
                self.fm.note_on(note, velocity, sample_rate);
            }
        }
        self.amp_env.note_on(velocity as f32 / 127.0);
        self.active = true;
    }

    pub fn note_off(&mut self) {
        match self.active_engine {
            EngineType::Fm | EngineType::Va => self.fm.note_off(),
            EngineType::Modal => self.modal.note_off(),
        }
        self.amp_env.note_off();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ParamSnapshot,
        sample_rate: u32,
    ) {
        if !self.active {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        // 1. Engine → raw oscillator output
        match self.active_engine {
            EngineType::Fm | EngineType::Va => {
                self.fm.update_params(&params.fm, sample_rate);
                self.fm.render(output, &params.fm.op_env, sample_rate);
            }
            EngineType::Modal => {
                self.modal.render(output, &params.modal, sample_rate);
            }
        }

        // 2. Drive
        self.drive.process(output, &params.drive);

        // 3. Filter
        self.filter.process(output, &params.filter, sample_rate);

        // 4. Wavefolder
        self.folder.process(output, &params.folder);

        // 5. VCA
        let volume = params.volume.value;
        for sample in output.iter_mut() {
            let env = self.amp_env.process(&params.envelopes[0], sample_rate);
            *sample *= env * volume;
        }

        // Check if done
        let engine_done = match self.active_engine {
            EngineType::Fm | EngineType::Va => !self.fm.is_active(),
            EngineType::Modal => !self.modal.is_active(),
        };
        if engine_done && !self.amp_env.is_active() {
            self.active = false;
        }
    }
}
