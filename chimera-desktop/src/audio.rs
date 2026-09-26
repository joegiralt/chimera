//! Desktop audio: the same `Instrument` the firmware will run (ADR 0013),
//! fed by `NoteSources` (the keyboard, and midir when feature `midi` is on)
//! and an `AudioShared` published through a `TripleBuffer`, summed from
//! three DAC pairs to the speakers.

use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{BLOCK_SIZE, CPU_HZ_REV_V, DAC_PAIRS, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind, NoteSources, SourceId};
use chimera_core::preset::Performance;
use chimera_core::scope::{ScopeFrame, ScopeWriter};
use chimera_core::triple::{TripleBuffer, Writer};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(feature = "midi")]
const N_SOURCES: usize = 2;
#[cfg(not(feature = "midi"))]
const N_SOURCES: usize = 1;

const KEYS: SourceId<N_SOURCES> = SourceId::new(0);
#[cfg(feature = "midi")]
const MIDI: SourceId<N_SOURCES> = SourceId::new(1);

/// Everything the audio callback shares with the UI thread.
struct SharedState {
    notes: NoteSources<N_SOURCES>,
    /// 0 = all pairs, 1..=3 = only that DAC pair.
    solo: AtomicU8,
}

pub struct DesktopAudio {
    _stream: Stream,
    shared: Arc<SharedState>,
    shared_audio: Writer<AudioShared>,
    #[cfg(feature = "midi")]
    _midi: Option<midir::MidiInputConnection<()>>,
}

impl DesktopAudio {
    pub fn new(scope: Writer<ScopeFrame>) -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = stereo_48k(&device).unwrap_or_else(|| {
            let c = device.default_output_config().expect("no output config");
            eprintln!("no 48 kHz stereo f32 output; using {} Hz (Modal pitch floor and delay range assume 48 kHz)", c.sample_rate().0);
            c
        });
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        let (shared_audio, mut shared_reader) = Box::leak(Box::new(TripleBuffer::new(
            AudioShared::default(),
            AudioShared::default(),
            AudioShared::default(),
        )))
        .split();
        let shared = Arc::new(SharedState {
            notes: NoteSources::new(),
            solo: AtomicU8::new(0),
        });
        let audio = Arc::clone(&shared);

        let mut inst = Box::new(Instrument::new(
            sample_rate,
            SampleBudget::for_cpu(CPU_HZ_REV_V),
        ));
        let mut fx = Box::new(FxBus::new());
        let mut dac: DacOut = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
        let mut block_pos = BLOCK_SIZE;
        let mut scope = ScopeWriter::new(scope);

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let shared = shared_reader.read();
                    // This is each source queue's only consumer: the UI/main
                    // thread pushes to KEYS, the MIDI callback thread to
                    // MIDI (see `note_on`/`note_off` and `connect_midi`).
                    audio.notes.drain(|ev| inst.handle(ev, shared));
                    let solo = audio.solo.load(Ordering::Relaxed);
                    for frame in data.chunks_mut(channels) {
                        if block_pos >= BLOCK_SIZE {
                            inst.render(&mut fx, &mut dac, shared, &mut scope);
                            block_pos = 0;
                        }
                        let (l, r) = stereo_frame(&dac, solo, block_pos);
                        let (l, r) = (libm::tanhf(l * 0.7), libm::tanhf(r * 0.7));
                        match frame {
                            [mono] => *mono = 0.5 * (l + r),
                            [fl, fr, rest @ ..] => {
                                (*fl, *fr) = (l, r);
                                rest.fill(0.0);
                            }
                            [] => {}
                        }
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
            #[cfg(feature = "midi")]
            _midi: connect_midi(Arc::clone(&shared)),
            shared,
            shared_audio,
        }
    }

    /// Push the Performance to the audio thread through the triple buffer.
    pub fn update(&mut self, perf: &Performance) {
        self.shared_audio.publish(|b| b.update_from(perf));
    }

    /// Push a note-on onto the KEYS queue. Callers: the UI/main thread only
    /// — each source's queue is single-producer/single-consumer, and the
    /// audio callback is the sole consumer of all of them.
    pub fn note_on(&self, channel: MidiChannel, note: MidiNote, velocity: Velocity) {
        self.shared.notes.source(KEYS).push(NoteEvent {
            channel,
            note,
            kind: NoteKind::On(velocity),
        });
    }

    /// Push a note-off onto the queue. Callers: the UI/main thread only —
    /// see `note_on`.
    pub fn note_off(&self, channel: MidiChannel, note: MidiNote) {
        self.shared.notes.source(KEYS).push(NoteEvent {
            channel,
            note,
            kind: NoteKind::Off,
        });
    }

    /// Hear only DAC pair `pair` (1..=3), or all of them (0).
    pub fn solo(&self, pair: u8) {
        self.shared
            .solo
            .store(pair.min(DAC_PAIRS as u8), Ordering::Relaxed);
    }
}

/// A 48 kHz stereo f32 config if the device has one (hardware parity).
fn stereo_48k(device: &cpal::Device) -> Option<cpal::SupportedStreamConfig> {
    device
        .supported_output_configs()
        .ok()?
        .find(|c| {
            c.channels() >= 2
                && c.sample_format() == cpal::SampleFormat::F32
                && (c.min_sample_rate().0..=c.max_sample_rate().0).contains(&SAMPLE_RATE)
        })
        .map(|c| c.with_sample_rate(cpal::SampleRate(SAMPLE_RATE)))
}

/// Frame `i` of the three pairs summed to one stereo pair, or only pair
/// `solo` (1..=3) when `solo` is not 0.
fn stereo_frame(dac: &DacOut, solo: u8, i: usize) -> (f32, f32) {
    let mut l = 0.0;
    let mut r = 0.0;
    for (p, pair) in dac.iter().enumerate() {
        if solo == 0 || solo as usize == p + 1 {
            l += pair[2 * i];
            r += pair[2 * i + 1];
        }
    }
    (l, r)
}

/// Opens a MIDI input port for the app's lifetime, keeping the returned
/// connection alive keeps it open. `None` (no device, or only "Midi
/// Through") means the computer keyboard is the only note source.
#[cfg(feature = "midi")]
fn connect_midi(shared: Arc<SharedState>) -> Option<midir::MidiInputConnection<()>> {
    use chimera_hal::midi::MidiParser;
    let input = midir::MidiInput::new("chimera")
        .map_err(|e| eprintln!("MIDI unavailable: {e}"))
        .ok()?;
    let ports = input.ports();
    let names: Vec<String> = ports
        .iter()
        .map(|p| input.port_name(p).unwrap_or_default())
        .collect();
    let wanted = std::env::var("CHIMERA_MIDI_PORT").ok();
    let Some(i) = crate::midi::pick_port(&names, wanted.as_deref()) else {
        eprintln!("no MIDI input port; playing from the keyboard only");
        return None;
    };
    eprintln!("MIDI in: {}", names[i]);
    let mut parser = MidiParser::new();
    input
        .connect(
            &ports[i],
            "chimera-in",
            move |_stamp, bytes, _| {
                for &b in bytes {
                    if let Some(ev) = parser.feed(b).and_then(NoteEvent::from_midi) {
                        shared.notes.source(MIDI).push(ev);
                    }
                }
            },
            (),
        )
        .map_err(|e| eprintln!("MIDI connect failed: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dac() -> DacOut {
        let mut d = [[0.0; BLOCK_SIZE * 2]; DAC_PAIRS];
        for (p, pair) in d.iter_mut().enumerate() {
            pair[0] = (p + 1) as f32; // L of frame 0
            pair[1] = 10.0 * (p + 1) as f32; // R of frame 0
        }
        d
    }

    #[test]
    fn pairs_sum_to_stereo() {
        assert_eq!(stereo_frame(&dac(), 0, 0), (6.0, 60.0));
    }

    #[test]
    fn solo_hears_one_pair() {
        assert_eq!(stereo_frame(&dac(), 2, 0), (2.0, 20.0));
        assert_eq!(stereo_frame(&dac(), 3, 0), (3.0, 30.0));
    }
}
