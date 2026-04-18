use chimera_core::dsp::fm::{FmEngine, FmParams};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use std::sync::atomic::{AtomicPtr, AtomicU8, Ordering};
use std::sync::Arc;

/// MIDI note command sent from UI thread to audio thread.
const NOTE_NONE: u8 = 0;
const NOTE_ON_FLAG: u8 = 0x80;

/// Shared state between UI and audio threads.
struct SharedState {
    /// Double-buffered FM params: UI writes to inactive, swaps pointer.
    fm_params: AtomicPtr<FmParams>,
    /// Simple note trigger: 0 = no change, 0x80|note = note on, note = note off
    note_cmd: AtomicU8,
    /// Velocity for note on
    velocity: AtomicU8,
}

pub struct DesktopAudio {
    _stream: Stream,
    shared: Arc<SharedState>,
    /// UI-side param buffers (double-buffered)
    param_bufs: Box<[FmParams; 2]>,
    active_buf: usize,
}

impl DesktopAudio {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = device.default_output_config().expect("no output config");
        let sample_rate = config.sample_rate().0;

        // Double-buffered params
        let mut param_bufs = Box::new([FmParams::default(), FmParams::default()]);
        let initial_ptr = &mut param_bufs[0] as *mut FmParams;

        let shared = Arc::new(SharedState {
            fm_params: AtomicPtr::new(initial_ptr),
            note_cmd: AtomicU8::new(NOTE_NONE),
            velocity: AtomicU8::new(100),
        });
        let shared_clone = Arc::clone(&shared);

        let mut engine = FmEngine::new();

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Read latest params (lock-free)
                    let params_ptr = shared_clone.fm_params.load(Ordering::Acquire);
                    // SAFETY: pointer is always valid — points to one of the two
                    // param_bufs entries owned by DesktopAudio. The UI thread only
                    // writes to the inactive buffer and swaps the pointer atomically.
                    let params = unsafe { &*params_ptr };

                    // Check for note commands
                    let cmd = shared_clone.note_cmd.swap(NOTE_NONE, Ordering::Relaxed);
                    if cmd & NOTE_ON_FLAG != 0 {
                        let note = cmd & 0x7F;
                        let vel = shared_clone.velocity.load(Ordering::Relaxed);
                        engine.update_params(params, sample_rate);
                        engine.note_on(note, vel, sample_rate);
                    } else if cmd > 0 {
                        engine.note_off();
                    }

                    // Update engine params
                    engine.update_params(params, sample_rate);

                    // Render in blocks of BLOCK_SIZE, then scatter to output
                    let mut block = [0.0f32; chimera_hal::BLOCK_SIZE];
                    let mut block_pos = chimera_hal::BLOCK_SIZE; // force render on first sample

                    for sample in data.iter_mut() {
                        if block_pos >= chimera_hal::BLOCK_SIZE {
                            engine.render(&mut block, &params.op_env, sample_rate);
                            block_pos = 0;
                        }
                        *sample = block[block_pos] * 0.5;
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

    /// Update FM parameters from the UI thread (lock-free swap).
    pub fn update_params(&mut self, params: &FmParams) {
        // Write to inactive buffer
        let inactive = 1 - self.active_buf;
        self.param_bufs[inactive] = *params;
        // Swap pointer atomically
        let ptr = &mut self.param_bufs[inactive] as *mut FmParams;
        self.shared.fm_params.store(ptr, Ordering::Release);
        self.active_buf = inactive;
    }

    /// Trigger a note on.
    pub fn note_on(&self, note: u8, velocity: u8) {
        self.shared.velocity.store(velocity, Ordering::Relaxed);
        self.shared
            .note_cmd
            .store(NOTE_ON_FLAG | (note & 0x7F), Ordering::Relaxed);
    }

    /// Trigger a note off.
    pub fn note_off(&self) {
        self.shared.note_cmd.store(1, Ordering::Relaxed);
    }
}
