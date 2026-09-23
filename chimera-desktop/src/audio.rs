use chimera_core::dsp::chorus::JunoChorus;
use chimera_core::dsp::delay::TapeDelay;
use chimera_core::dsp::reverb::Reverb;
use chimera_core::dsp::voice::Voice;
use chimera_core::params::ParamSnapshot;
use chimera_core::{MidiNote, Velocity};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

const NOTE_NONE: u8 = 0;
const NOTE_ON_FLAG: u8 = 0x80;

struct SharedState {
    params: AtomicPtr<ParamSnapshot>,
    note_cmd: AtomicU8,
    velocity: AtomicU8,
}

pub struct DesktopAudio {
    _stream: Stream,
    shared: Arc<SharedState>,
    param_bufs: Box<[ParamSnapshot; 2]>,
    active_buf: usize,
}

impl DesktopAudio {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = device.default_output_config().expect("no output config");
        let sample_rate = config.sample_rate().0;

        let mut param_bufs = Box::new([ParamSnapshot::default(), ParamSnapshot::default()]);
        let initial_ptr = &mut param_bufs[0] as *mut ParamSnapshot;

        let shared = Arc::new(SharedState {
            params: AtomicPtr::new(initial_ptr),
            note_cmd: AtomicU8::new(NOTE_NONE),
            velocity: AtomicU8::new(100),
        });
        let shared_clone = Arc::clone(&shared);

        let mut voice = Box::new(Voice::new(sample_rate));
        let mut chorus = Box::new(JunoChorus::new());
        let mut delay = Box::new(TapeDelay::new());
        let mut reverb = Box::new(Reverb::new());
        let mut block = [0.0f32; chimera_hal::BLOCK_SIZE];
        let mut block_pos: usize = chimera_hal::BLOCK_SIZE;

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let params_ptr = shared_clone.params.load(Ordering::Acquire);
                    // SAFETY: pointer always valid — points into param_bufs owned
                    // by DesktopAudio. UI writes inactive buffer, swaps atomically.
                    let params = unsafe { &*params_ptr };

                    let cmd = shared_clone.note_cmd.swap(NOTE_NONE, Ordering::Relaxed);
                    if cmd & NOTE_ON_FLAG != 0 {
                        let vel = shared_clone.velocity.load(Ordering::Relaxed);
                        // Both were stored from a MidiNote/Velocity, so these always succeed.
                        if let (Some(note), Some(vel)) = (MidiNote::new(cmd & 0x7F), Velocity::new(vel)) {
                            voice.note_on(note, vel, params);
                        }
                    } else if cmd > 0 {
                        voice.note_off();
                    }

                    let empty_mod = chimera_core::modulation::ModState::new();
                    for sample in data.iter_mut() {
                        if block_pos >= chimera_hal::BLOCK_SIZE {
                            voice.render(&mut block, params, &empty_mod);
                            // Effects chain: chorus → delay → reverb (Digitone II style)
                            chorus.process(&mut block, &params.chorus, sample_rate);
                            delay.process(&mut block, &params.delay, sample_rate);
                            reverb.process(&mut block, &params.reverb);
                            block_pos = 0;
                        }
                        *sample = libm::tanhf(block[block_pos] * 0.7);
                        block_pos += 1;
                    }
                },
                |err| eprintln!("audio error: {}", err),
                None,
            )
            .expect("failed to build audio stream");

        stream.play().expect("failed to play stream");

        Self {
            _stream: stream,
            shared,
            param_bufs,
            active_buf: 0,
        }
    }

    /// Push full param snapshot to audio thread (lock-free swap).
    pub fn update_params(&mut self, params: &ParamSnapshot) {
        let inactive = 1 - self.active_buf;
        self.param_bufs[inactive] = params.clone();
        let ptr = &mut self.param_bufs[inactive] as *mut ParamSnapshot;
        self.shared.params.store(ptr, Ordering::Release);
        self.active_buf = inactive;
    }

    pub fn note_on(&self, note: MidiNote, velocity: Velocity) {
        self.shared.velocity.store(velocity.get(), Ordering::Relaxed);
        self.shared
            .note_cmd
            .store(NOTE_ON_FLAG | note.get(), Ordering::Relaxed);
    }

    pub fn note_off(&self) {
        self.shared.note_cmd.store(1, Ordering::Relaxed);
    }
}
