//! Desktop audio: the same `Instrument` the firmware runs (ADR 0013),
//! fed by `NoteSources` (the keyboard, and midir when feature `midi` is on)
//! and an `AudioShared` published through a `TripleBuffer`, summed from
//! three DAC pairs to the speakers.

use chimera_core::audio_out::to_dac;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::hw::{BLOCK_SIZE, CPU_HZ_REV_V, DAC_PAIRS, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, DacOut, Instrument};
use chimera_core::note_queue::{
    NoteDrain, NoteEvent, NoteKind, NoteProducer, NoteSources, SourceId,
};
use chimera_core::preset::Performance;
use chimera_core::project::{LOAD_LINK, LoadGate, LoadLink};
use chimera_core::scope::{ScopeFrame, ScopeWriter};
use chimera_core::triple::{TripleBuffer, Writer};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::time::{Duration, Instant};

#[cfg(feature = "midi")]
const N_SOURCES: usize = 2;
#[cfg(not(feature = "midi"))]
const N_SOURCES: usize = 1;

const KEYS: SourceId<N_SOURCES> = SourceId::new(0);
#[cfg(feature = "midi")]
const MIDI: SourceId<N_SOURCES> = SourceId::new(1);

/// Shared with the audio callback: `solo` 0 = all pairs, 1..=3 = only that
/// DAC pair; `clamped` counts mixdown frames the speakers' clamp held.
struct SharedState {
    solo: AtomicU8,
    clamped: AtomicU32,
}

pub struct DesktopAudio {
    _stream: Stream,
    shared: Arc<SharedState>,
    /// The computer keyboard's note source.
    keys: NoteProducer<'static>,
    shared_audio: Writer<AudioShared>,
    clamp_log: ClampLog,
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
        // Leaked like the triple buffer: one per process, and the producers
        // it splits into must outlive the threads they move to.
        let notes: &'static NoteSources<N_SOURCES> = Box::leak(Box::new(NoteSources::new()));
        let (mut producers, mut drain) = notes.split().expect("note sources split once");
        let keys = producers.take(KEYS).expect("keyboard producer taken once");
        let shared = Arc::new(SharedState {
            solo: AtomicU8::new(0),
            clamped: AtomicU32::new(0),
        });
        let audio = Arc::clone(&shared);

        let mut engine = Engine::new(sample_rate);
        let mut block_pos = BLOCK_SIZE;
        let mut scope = ScopeWriter::new(scope);

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let shared = shared_reader.read();
                    let solo = audio.solo.load(Ordering::Relaxed);
                    for frame in data.chunks_mut(channels) {
                        if block_pos >= BLOCK_SIZE {
                            engine.block(&LOAD_LINK, &mut drain, shared, &mut scope);
                            block_pos = 0;
                        }
                        let ((l, r), clamped) = stereo_frame(engine.dac.out(), solo, block_pos);
                        if clamped {
                            audio.clamped.fetch_add(1, Ordering::Relaxed);
                        }
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
            _midi: connect_midi(producers.take(MIDI).expect("MIDI producer taken once")),
            shared,
            keys,
            shared_audio,
            clamp_log: ClampLog::default(),
        }
    }

    /// Push the Performance, tagged with the load `epoch`, to the audio
    /// thread through the triple buffer, and report any mixdown frames
    /// clamped since the last call.
    pub fn update(&mut self, perf: &Performance, epoch: u32) {
        self.shared_audio.publish(|b| b.update_from(perf, epoch));
        let clamped = self.shared.clamped.swap(0, Ordering::Relaxed);
        if let Some(n) = self.clamp_log.note(clamped, Instant::now()) {
            eprintln!("speakers: {n} frames of the pairs' sum clamped at full scale");
        }
    }

    /// Push a note-on onto the keyboard's queue.
    pub fn note_on(&mut self, channel: MidiChannel, note: MidiNote, velocity: Velocity) {
        self.keys.push(NoteEvent {
            channel,
            note,
            kind: NoteKind::On(velocity),
        });
    }

    /// Push a note-off onto the keyboard's queue.
    pub fn note_off(&mut self, channel: MidiChannel, note: MidiNote) {
        self.keys.push(NoteEvent {
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

/// What the callback renders with: the firmware's `Engine`, less the queue
/// and the snapshot it is handed.
struct Engine {
    inst: Box<Instrument>,
    fx: Box<FxBus>,
    dac: DacBlocks,
    /// Holds the note queues through a project load (ADR 0046).
    gate: LoadGate,
}

impl Engine {
    fn new(sample_rate: u32) -> Self {
        Self {
            inst: Box::new(Instrument::new(
                sample_rate,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            fx: Box::new(FxBus::new()),
            dac: DacBlocks::new(),
            gate: LoadGate::new(),
        }
    }

    /// One block, as the firmware renders each half: the gate, the drain
    /// only when it opens, then the render.
    fn block<const N: usize>(
        &mut self,
        link: &LoadLink,
        drain: &mut NoteDrain<'_, N>,
        shared: &AudioShared,
        scope: &mut ScopeWriter,
    ) {
        let Self {
            inst,
            fx,
            dac,
            gate,
        } = self;
        if gate.before_block(link, inst, shared) {
            drain.drain(|ev| inst.handle(ev, shared));
        }
        inst.render(fx, dac, shared, scope);
    }
}

/// The mixdown's clamped frames, reported at most once per `EVERY`.
#[derive(Default)]
struct ClampLog {
    pending: u32,
    last: Option<Instant>,
}

impl ClampLog {
    const EVERY: Duration = Duration::from_secs(1);

    /// Add `n` clamped frames seen by `now`; the count to report, if one is
    /// due.
    fn note(&mut self, n: u32, now: Instant) -> Option<u32> {
        self.pending = self.pending.saturating_add(n);
        let due = self
            .last
            .is_none_or(|t| now.duration_since(t) >= Self::EVERY);
        if self.pending == 0 || !due {
            return None;
        }
        self.last = Some(now);
        Some(core::mem::take(&mut self.pending))
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

/// Frame `i` of the three pairs as their DACs play them (`to_dac`, the
/// firmware's final stage: ADR 0050), summed to one stereo pair, or only
/// pair `solo` (1..=3) when `solo` is not 0. The sum goes through the same
/// stage again, so the speakers never get more than full scale; the flag
/// says it clamped, which only several busy pairs together can make it do.
fn stereo_frame(dac: &DacOut, solo: u8, i: usize) -> ((f32, f32), bool) {
    let mut l = 0.0;
    let mut r = 0.0;
    for (p, pair) in dac.iter().enumerate() {
        if solo == 0 || solo as usize == p + 1 {
            l += to_dac(pair[2 * i]).level();
            r += to_dac(pair[2 * i + 1]).level();
        }
    }
    let clamped = l.abs() > 1.0 || r.abs() > 1.0;
    ((to_dac(l).level(), to_dac(r).level()), clamped)
}

/// Opens a MIDI input port for the app's lifetime, keeping the returned
/// connection alive keeps it open. `None` (no device, or only "Midi
/// Through") means the computer keyboard is the only note source.
#[cfg(feature = "midi")]
fn connect_midi(mut notes: NoteProducer<'static>) -> Option<midir::MidiInputConnection<()>> {
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
                        notes.push(ev);
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
            pair[0] = 0.1 * (p + 1) as f32; // L of frame 0
            pair[1] = -0.15 * (p + 1) as f32; // R of frame 0
        }
        d
    }

    /// What pair `p`'s DAC plays for frame 0.
    fn played(p: usize) -> (f32, f32) {
        let d = dac();
        (to_dac(d[p][0]).level(), to_dac(d[p][1]).level())
    }

    /// The speakers get the pairs' sum through the same final stage.
    fn heard(l: f32, r: f32) -> (f32, f32) {
        (to_dac(l).level(), to_dac(r).level())
    }

    #[test]
    fn pairs_sum_to_stereo() {
        let (a, b, c) = (played(0), played(1), played(2));
        let want = heard(a.0 + b.0 + c.0, a.1 + b.1 + c.1);
        assert_eq!(stereo_frame(&dac(), 0, 0), (want, false));
    }

    #[test]
    fn solo_hears_one_pair() {
        assert_eq!(stereo_frame(&dac(), 2, 0), (played(1), false));
        assert_eq!(stereo_frame(&dac(), 3, 0), (played(2), false));
    }

    /// ADR 0050: the desktop clamps where the DACs do, no softer curve.
    #[test]
    fn each_pair_clamps_as_its_dac_does() {
        let mut d = dac();
        (d[1][0], d[1][1]) = (3.0, -0.5);
        assert_eq!(stereo_frame(&d, 2, 0), ((1.0, to_dac(-0.5).level()), false));
    }

    /// At most one report a second, carrying every frame since the last.
    #[test]
    fn the_clamp_log_reports_once_a_second_with_a_count() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut log = ClampLog::default();
        assert_eq!(log.note(0, at(0)), None, "nothing to report");
        assert_eq!(log.note(3, at(10)), Some(3));
        assert_eq!(log.note(5, at(500)), None, "too soon");
        assert_eq!(log.note(2, at(900)), None);
        assert_eq!(log.note(0, at(1010)), Some(7), "the held count, once due");
        assert_eq!(log.note(0, at(3000)), None);
    }

    /// ADR 0046: the gate steps once per block, not per callback, so one
    /// callback's blocks kill, fade and ack a load, and a note queued
    /// meanwhile waits for the publish.
    #[test]
    fn the_gate_steps_every_block() {
        let sources: &'static NoteSources<1> = Box::leak(Box::new(NoteSources::new()));
        let (mut producers, mut drain) = sources.split().unwrap();
        let mut keys = producers.take(SourceId::new(0)).unwrap();
        let mut e = Engine::new(SAMPLE_RATE);
        let (w, _unread) = Box::leak(Box::new(chimera_core::scope::scope_buffer())).split();
        let mut scope = ScopeWriter::new(w);
        let mut shared = AudioShared::default();
        let link = LoadLink::new();
        let _swap = link.bump_for_test();
        let ch = MidiChannel::new(0).unwrap();
        let note = MidiNote::new(60).unwrap();
        keys.push(NoteEvent {
            channel: ch,
            note,
            kind: NoteKind::On(Velocity::DEFAULT),
        });
        for _ in 0..3 {
            e.block(&link, &mut drain, &shared, &mut scope);
        }
        assert!(link.acked(link.epoch()), "kill, fade, ack: three blocks");
        assert!(e.inst.quiet(), "the note waits in the queue");
        shared.epoch = link.epoch();
        e.block(&link, &mut drain, &shared, &mut scope);
        assert!(!e.inst.quiet(), "it plays once published");
    }

    /// Three pairs near full scale sum past 1.0: the speakers still get
    /// at most full scale, and the frame says it was clamped.
    #[test]
    fn the_mixdown_never_passes_full_scale_unreported() {
        let d = [[0.8; BLOCK_SIZE * 2]; DAC_PAIRS];
        assert_eq!(stereo_frame(&d, 0, 0), ((1.0, 1.0), true));
        assert!(!stereo_frame(&d, 1, 0).1, "one pair alone is in range");
    }
}
