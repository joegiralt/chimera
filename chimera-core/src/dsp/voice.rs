use chimera_hal::BLOCK_SIZE;

use crate::block::apply_offset;
use crate::dsp::drive::Drive;
use crate::dsp::engines::Engines;
use crate::dsp::envelope::Envelope;
use crate::dsp::filter::SvfFilter;
use crate::dsp::lfo::Lfo;
use crate::dsp::wavefolder::Wavefolder;
use crate::modulation::{ModState, MAX_MOD_SOURCES};
use crate::params::{EngineType, ParamSnapshot};
use crate::{MidiNote, Velocity};

/// Complete voice signal chain:
/// [Engine] → [Drive] → [Filter] → [Wavefolder] → [VCA]
/// Modulators: Envelope + LFO
pub struct Voice {
    engines: Engines,
    drive: Drive,
    filter: SvfFilter,
    folder: Wavefolder,
    amp_env: Envelope,
    pub lfo: Lfo,
    active_engine: EngineType,
    active: bool,
    last_note: MidiNote,
    last_velocity: Velocity,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new(chimera_hal::SAMPLE_RATE)
    }
}

impl Voice {
    /// The sample rate is stored once (spec §3), not passed per call.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            engines: Engines::new(sample_rate),
            drive: Drive::new(),
            filter: SvfFilter::new(),
            folder: Wavefolder::new(),
            amp_env: Envelope::new(),
            lfo: Lfo::new(),
            active_engine: EngineType::Pizza,
            active: false,
            last_note: MidiNote::A4,
            last_velocity: Velocity::DEFAULT,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.engines.sample_rate()
    }

    pub fn note_on(&mut self, note: MidiNote, velocity: Velocity, params: &ParamSnapshot) {
        self.active_engine = params.engine();
        self.last_note = note;
        self.last_velocity = velocity;
        self.engines.note_on(self.active_engine, note, velocity, params);
        self.amp_env.note_on(velocity.unit());
        self.active = true;
    }

    pub fn note_off(&mut self) {
        self.engines.note_off(self.active_engine);
        self.amp_env.note_off();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn render(&mut self, output: &mut [f32; BLOCK_SIZE], params: &ParamSnapshot, mod_state: &ModState) {
        let sample_rate = self.sample_rate();

        // Auto-retrigger if engine type changed (e.g., user loaded FM patch)
        if self.active && params.engine() != self.active_engine {
            self.note_on(self.last_note, self.last_velocity, params);
        }

        if !self.active {
            output.fill(0.0);
            return;
        }

        // Compute modulator source values
        let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
        // Source 0 = Envelope
        if mod_state.num_sources() > 0 {
            mod_values[0] = self.amp_env.current_level();
        }
        // Source 1 = LFO
        if mod_state.num_sources() > 1 {
            mod_values[1] = self.lfo.process(&params.lfo, sample_rate);
        }

        // Modulated copy (stack only): every routed destination gets its
        // offset through its block's spec (spec §4).
        let mut m = params.clone();
        for d in 0..mod_state.num_dests() {
            let off = mod_state.sum_for(d, &mod_values);
            if off != 0.0 {
                let a = mod_state.dest(d);
                apply_offset(m.block_mut(a.block), a.param, off);
            }
        }

        // 1. Engine → raw oscillator output
        self.engines.render(self.active_engine, output, &m);

        // 2. Drive
        self.drive.process(output, &m.drive);

        // 3. Filter
        self.filter.process(output, &m.filter, sample_rate);

        // 4. Wavefolder
        self.folder.process(output, &m.folder);

        // 5. VCA
        let volume = m.out.volume;
        if Engines::uses_amp_env(self.active_engine) {
            for sample in output.iter_mut() {
                let env = self.amp_env.process(&m.envelopes[0], sample_rate);
                *sample *= env * volume;
            }
        } else {
            for sample in output.iter_mut() {
                *sample *= volume;
            }
        }

        // 6. Scope — capture end-of-chain for oscilloscope display
        crate::scope::write_samples(output);

        // Check if done
        self.active = self.engines.is_active(self.active_engine, &self.amp_env);
    }
}
