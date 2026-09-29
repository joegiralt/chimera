//! A lone `Voice` with the sympathetic pool it borrows from, behind the
//! voice's pre-pool signatures: the suites that drive one voice directly.

use std::ops::{Deref, DerefMut};

use chimera_core::dsp::modal::SymPool;
use chimera_core::dsp::voice::Voice;
use chimera_core::modulation::ModState;
use chimera_core::params::ParamSnapshot;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::BLOCK_SIZE;

pub struct Rig {
    pub voice: Box<Voice>,
    pub pool: Box<SymPool>,
}

impl Rig {
    pub fn new(sample_rate: u32) -> Self {
        let mut raw = Box::<Voice>::new_uninit();
        Voice::init_in_place(&mut raw, sample_rate, VoiceIdx::ALL[0]);
        Self {
            // SAFETY: `init_in_place` built a valid voice in the box.
            voice: unsafe { raw.assume_init() },
            pool: SymPool::boxed(),
        }
    }

    pub fn note_on(&mut self, note: MidiNote, velocity: Velocity, params: &ParamSnapshot) -> bool {
        self.voice.note_on(note, velocity, params, &mut self.pool)
    }

    pub fn note_off(&mut self) {
        self.voice.note_off(&mut self.pool);
    }

    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], params: &ParamSnapshot, mods: &ModState) {
        self.voice.render(out, params, mods, &mut self.pool);
    }
}

impl Deref for Rig {
    type Target = Voice;
    fn deref(&self) -> &Voice {
        &self.voice
    }
}

impl DerefMut for Rig {
    fn deref_mut(&mut self) -> &mut Voice {
        &mut self.voice
    }
}
