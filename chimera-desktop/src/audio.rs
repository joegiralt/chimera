use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Stream;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Desktop audio backend using cpal.
///
/// Note: This uses a simple AtomicU32 for frequency as a Phase 0 placeholder.
/// The spec calls for double-buffered ParamSnapshot with AtomicPtr swap —
/// that will be implemented when the full parameter model is wired up.
pub struct DesktopAudio {
    _stream: Stream,
    frequency: Arc<AtomicU32>,
}

impl DesktopAudio {
    pub fn new() -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = device.default_output_config().expect("no output config");

        let sample_rate = config.sample_rate().0 as f32;
        let frequency = Arc::new(AtomicU32::new(0));
        let freq_clone = Arc::clone(&frequency);

        let mut phase: f32 = 0.0;

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let freq = f32::from_bits(freq_clone.load(Ordering::Relaxed));
                    let phase_inc = freq / sample_rate;
                    for sample in data.iter_mut() {
                        if freq > 0.0 {
                            *sample = (phase * 2.0 * core::f32::consts::PI).sin() * 0.3;
                            phase += phase_inc;
                            if phase >= 1.0 {
                                phase -= 1.0;
                            }
                        } else {
                            *sample = 0.0;
                        }
                    }
                },
                |err| eprintln!("audio error: {}", err),
                None,
            )
            .expect("failed to build audio stream");

        stream.play().expect("failed to play stream");

        Self {
            _stream: stream,
            frequency,
        }
    }

    /// Set frequency in Hz. 0.0 = silence.
    pub fn set_frequency(&self, freq: f32) {
        self.frequency.store(freq.to_bits(), Ordering::Relaxed);
    }
}
