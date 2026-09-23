use chimera_hal::BLOCK_SIZE;

use crate::block::apply_offset;
use crate::dsp::drive::Drive;
use crate::dsp::engine_fm::FmEngine;
use crate::dsp::envelope::Envelope;
use crate::dsp::filter::SvfFilter;
use crate::dsp::lfo::Lfo;
use crate::dsp::modal::ModalEngine;
use crate::dsp::pizza::{PizzaOsc, PizzaParams};
use crate::dsp::wavefolder::Wavefolder;
use crate::mod_path::ParamPath;
use crate::modulation::{ModState, MAX_MOD_SOURCES};
use crate::params::{DriveParams, EngineType, FilterParams, ParamSnapshot};

/// Complete voice signal chain:
/// [Engine (Pizza/Modal)] → [Drive] → [Filter] → [Wavefolder]
/// Modulators: Envelope + LFO
pub struct Voice {
    pub pizza: PizzaOsc,
    pub modal: ModalEngine,
    pub fm: FmEngine,
    drive: Drive,
    filter: SvfFilter,
    folder: Wavefolder,
    amp_env: Envelope,
    pub lfo: Lfo,
    active_engine: EngineType,
    active: bool,
    last_note: u8,
    last_velocity: u8,
    last_sample_rate: u32,
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
            fm: FmEngine::new(),
            drive: Drive::new(),
            filter: SvfFilter::new(),
            folder: Wavefolder::new(),
            amp_env: Envelope::new(),
            lfo: Lfo::new(),
            active_engine: EngineType::Pizza,
            active: false,
            last_note: 69,
            last_velocity: 100,
            last_sample_rate: 48000,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ParamSnapshot, sample_rate: u32) {
        self.active_engine = params.engine;
        self.last_note = note;
        self.last_velocity = velocity;
        self.last_sample_rate = sample_rate;
        let freq = crate::dsp::note_to_freq(note);
        match self.active_engine {
            EngineType::Pizza => {
                self.pizza.note_on(freq, sample_rate);
            }
            EngineType::Fm => {
                self.fm.note_on_params(note, velocity as f32 / 127.0, &params.fm, sample_rate as f32);
            }
            EngineType::Va => {
                // VA — silence placeholder
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
            EngineType::Fm => self.fm.note_off(),
            EngineType::Va => { /* VA — silence placeholder */ }
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
        mod_state: &ModState,
        sample_rate: u32,
    ) {
        // Auto-retrigger if engine type changed (e.g., user loaded FM patch)
        if self.active && params.engine != self.active_engine {
            self.note_on(self.last_note, self.last_velocity, params, self.last_sample_rate);
        }

        if !self.active {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        // Compute modulator source values
        let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
        // Source 0 = Envelope
        if mod_state.num_sources > 0 {
            mod_values[0] = self.amp_env.current_level();
        }
        // Source 1 = LFO
        if mod_state.num_sources > 1 {
            mod_values[1] = self.lfo.process(&params.lfo, sample_rate);
        }

        // Modulated param copies
        let mut mod_pizza = params.pizza;
        let mut mod_drive = params.drive;
        let mut mod_filter = params.filter;
        let mut mod_folder = params.folder;

        // Apply mod offsets — block indices match the chain: 0=Pizza, 1=Drive, 2=Filter, 3=Folder
        apply_offset(&mut mod_pizza, PizzaParams::SHAPE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 0, param: 0 }));
        apply_offset(&mut mod_pizza, PizzaParams::CRUSH, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 0, param: 1 }));
        apply_offset(&mut mod_pizza, PizzaParams::LEVEL, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 0, param: 2 }));

        apply_offset(&mut mod_drive, DriveParams::DRIVE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 1, param: 0 }));
        apply_offset(&mut mod_drive, DriveParams::TONE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 1, param: 1 }));

        // Filter
        apply_offset(&mut mod_filter, FilterParams::CUTOFF, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 2, param: 0 }));
        apply_offset(&mut mod_filter, FilterParams::RESONANCE, mod_state.compute_offset(&mod_values, ParamPath::Block { block: 2, param: 1 }));

        // Folder
        mod_folder.fold.apply_mod_offset(mod_state.compute_offset(&mod_values, ParamPath::Block { block: 3, param: 0 }));
        mod_folder.symmetry.apply_mod_offset(mod_state.compute_offset(&mod_values, ParamPath::Block { block: 3, param: 1 }));

        // 1. Engine → raw oscillator output
        match self.active_engine {
            EngineType::Pizza => {
                self.pizza.render(output, &mod_pizza, sample_rate);
            }
            EngineType::Fm => {
                let mut mod_fm = params.fm;
                // Apply modulation offsets to FM operator params
                for op in 0..4u8 {
                    // Level (param index 2 in FmOpParams)
                    let offset = mod_state.compute_offset(&mod_values, ParamPath::FmOp { op, param: 2 });
                    if offset != 0.0 {
                        mod_fm.operators[op as usize].level.apply_mod_offset(offset);
                    }
                    // Could add more modulatable FM params here in the future:
                    // coarse (1), feedback (3), detune (4), etc.
                }
                self.fm.render_params(output, &mod_fm);
            }
            EngineType::Va => {
                // VA — render silence
                for s in output.iter_mut() {
                    *s = 0.0;
                }
            }
            EngineType::Modal => {
                self.modal.render(output, &params.modal, sample_rate);
            }
        }

        // 2. Drive
        self.drive.process(output, &mod_drive);

        // 3. Filter
        self.filter.process(output, &mod_filter, sample_rate);

        // 4. Wavefolder
        self.folder.process(output, &mod_folder);

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

        // 6. Scope — capture end-of-chain for oscilloscope display
        crate::scope::write_samples(output);

        // Check if done
        self.active = match self.active_engine {
            EngineType::Pizza => self.amp_env.is_active(),
            EngineType::Fm => !self.fm.is_idle(),
            EngineType::Va => false, // VA — silence placeholder
            EngineType::Modal => self.modal.is_active(),
        };
    }
}
