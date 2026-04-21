use chimera_hal::BLOCK_SIZE;

use crate::dsp::drive::Drive;
use crate::dsp::envelope::Envelope;
use crate::dsp::filter::SvfFilter;
use crate::dsp::modal::ModalEngine;
use crate::dsp::pizza::PizzaOsc;
use crate::dsp::wavefolder::Wavefolder;
use crate::params::{EngineType, ParamSnapshot};

/// Complete voice signal chain:
/// [Engine (Pizza/Modal)] → [Drive] → [Filter] → [Wavefolder] → [VCA]
pub struct Voice {
    pub pizza: PizzaOsc,
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
            pizza: PizzaOsc::new(),
            modal: ModalEngine::new(),
            drive: Drive::new(),
            filter: SvfFilter::new(),
            folder: Wavefolder::new(),
            amp_env: Envelope::new(),
            active_engine: EngineType::Pizza,
            active: false,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ParamSnapshot, sample_rate: u32) {
        self.active_engine = params.engine;
        let freq = crate::dsp::note_to_freq(note);
        match self.active_engine {
            EngineType::Pizza => {
                self.pizza.note_on(freq, sample_rate);
            }
            EngineType::Fm | EngineType::Va => {
                // FM/VA removed — silence placeholder
            }
            EngineType::Modal => {
                self.modal
                    .note_on(note, velocity, &params.modal, sample_rate);
            }
        }
        self.amp_env.note_on(velocity as f32 / 127.0);
        self.active = true;
    }

    pub fn note_off(&mut self) {
        match self.active_engine {
            EngineType::Pizza => self.pizza.note_off(),
            EngineType::Fm | EngineType::Va => { /* FM/VA removed */ }
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
            EngineType::Pizza => {
                self.pizza.render(output, &params.pizza, sample_rate);
            }
            EngineType::Fm | EngineType::Va => {
                // FM/VA removed — render silence
                for s in output.iter_mut() {
                    *s = 0.0;
                }
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

        // 5. VCA — amp envelope shapes the sound
        let volume = params.volume.value;
        match self.active_engine {
            EngineType::Modal => {
                // Modal: modes have natural decay. Just apply volume.
                for sample in output.iter_mut() {
                    *sample *= volume;
                }
            }
            _ => {
                // Pizza/FM/VA: amp envelope shapes the sound
                for sample in output.iter_mut() {
                    let env = self.amp_env.process(&params.envelopes[0], sample_rate);
                    *sample *= env * volume;
                }
            }
        }

        // Check if done
        self.active = match self.active_engine {
            EngineType::Pizza => self.amp_env.is_active(),
            EngineType::Fm | EngineType::Va => false, // FM/VA removed
            EngineType::Modal => self.modal.is_active(),
        };
    }
}
