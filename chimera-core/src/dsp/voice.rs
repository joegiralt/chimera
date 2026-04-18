use chimera_hal::BLOCK_SIZE;

use crate::dsp::drive::Drive;
use crate::dsp::envelope::Envelope;
use crate::dsp::filter::SvfFilter;
use crate::dsp::fm::FmEngine;
use crate::dsp::wavefolder::Wavefolder;
use crate::params::ParamSnapshot;

/// Complete voice signal chain:
/// [FM Engine] → [Drive] → [Filter] → [Wavefolder] → [VCA]
pub struct Voice {
    pub engine: FmEngine,
    drive: Drive,
    filter: SvfFilter,
    folder: Wavefolder,
    amp_env: Envelope,
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
            engine: FmEngine::new(),
            drive: Drive::new(),
            filter: SvfFilter::new(),
            folder: Wavefolder::new(),
            amp_env: Envelope::new(),
            active: false,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ParamSnapshot, sample_rate: u32) {
        self.engine.update_params(&params.fm, sample_rate);
        self.engine.note_on(note, velocity, sample_rate);
        self.amp_env.note_on(velocity as f32 / 127.0);
        self.active = true;
    }

    pub fn note_off(&mut self) {
        self.engine.note_off();
        self.amp_env.note_off();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Render one block through the full signal chain.
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

        // Update engine params (ratios, algorithm, etc.)
        self.engine.update_params(&params.fm, sample_rate);

        // 1. FM Engine → raw oscillator output
        self.engine
            .render(output, &params.fm.op_env, sample_rate);

        // 2. Drive (pre-filter saturation)
        self.drive.process(output, &params.drive);

        // 3. Filter (SVF, 8 modes)
        self.filter.process(output, &params.filter, sample_rate);

        // 4. Wavefolder (post-filter)
        self.folder.process(output, &params.folder);

        // 5. VCA (amp envelope + velocity + volume)
        let volume = params.volume.value;
        for sample in output.iter_mut() {
            let env = self.amp_env.process(&params.envelopes[0], sample_rate);
            *sample *= env * volume;
        }

        // Check if voice is done
        if !self.engine.is_active() && !self.amp_env.is_active() {
            self.active = false;
        }
    }
}
