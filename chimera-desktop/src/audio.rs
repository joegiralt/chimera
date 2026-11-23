// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Desktop audio: the same `Instrument` the firmware runs (ADR 0013),
//! fed by `NoteSources` (the keyboard, and midir when feature `midi` is on)
//! and an `AudioShared` published through a `TripleBuffer`, summed from
//! three DAC pairs to the speakers.

use chimera_core::audio_out::to_dac;
use chimera_core::dsp::algo::engine::FormantViews;
use chimera_core::dsp::drum::view::KitWriter;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::knobs::KnobPublisher;
use chimera_core::hw::{BLOCK_SIZE, CPU_HZ_REV_V, DAC_PAIRS, SAMPLE_RATE, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, DacOut, Instrument, NoteOrigin};
use chimera_core::note_queue::{
    InEvent, NoteDrain, NoteEvent, NoteKind, NoteProducer, NoteSources, SourceId,
};
use chimera_core::orbit::door::OrbitPorts;
use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
use chimera_core::project::{LOAD_LINK, LoadGate, LoadLink, Project};
use chimera_core::scope::{ScopeFrame, ScopeWriter};
use chimera_core::source_table::{AudioToken, SOURCE_TABLE};
use chimera_core::stream::layout::StreamFile;
use chimera_core::stream::port::{NoPorts, StreamCard, Streams};
use chimera_core::stream::registry::StreamRegistry;
use chimera_core::stream::ring::AudioSide;
use chimera_core::stream::stats::StreamStats;
use chimera_core::stream::units::AudioBlocks;
use chimera_core::triple::{TripleBuffer, Writer};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::Ms;
use chimera_hal::card_bus::CardBus;
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
/// The MIDI port's controllers (MIDI map spec § 1): its thread pushes, the
/// UI thread drains.
#[cfg(feature = "midi")]
pub static MIDI_CTRL: chimera_core::midi_in::CtrlRing = chimera_core::midi_in::CtrlRing::new();
/// DIN OUT's bytes (spec § 9): the UI thread pushes, `--midi-out`'s
/// thread sends them.
#[cfg(feature = "midi")]
pub static MIDI_OUT: chimera_core::out_ring::OutRing = chimera_core::out_ring::OutRing::new();

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
    /// The UI side of the knobs: every publish's delta (ADR 0115).
    knobs: Box<KnobPublisher>,
    /// The kits' publisher to the audio's view (ADR 0265).
    kits: KitWriter,
    clamp_log: ClampLog,
    #[cfg(feature = "midi")]
    _midi: Option<midir::MidiInputConnection<()>>,
}

impl DesktopAudio {
    pub fn new(
        scope: Writer<ScopeFrame>,
        orbit: Writer<OrbitView>,
        formant: Writer<FormantViews>,
        streams: Option<DeskStreams>,
    ) -> Self {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("no output device");
        let config = stereo_48k(&device).unwrap_or_else(|| {
            let c = device.default_output_config().expect("no output config");
            eprintln!(
                "no 48 kHz stereo f32 output; resampling 48 kHz to {} Hz",
                c.sample_rate().0
            );
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

        let sources = SOURCE_TABLE.reader().expect("the source reader, once");
        // The engine runs at the unit's rate whatever the device's (#239):
        // MODAL's blocker, BODY and release, its pitch floor, the delays'
        // ranges, all as the firmware hears them.
        let mut engine = Engine::new(SAMPLE_RATE, orbit, sources);
        let mut rate = Resample::new(SAMPLE_RATE, sample_rate);
        engine.streams = streams;
        let mut formant = formant;
        let mut block_pos = BLOCK_SIZE;
        let mut scope = ScopeWriter::new(scope);

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    flush_denormals();
                    let shared = shared_reader.read();
                    let solo = audio.solo.load(Ordering::Relaxed);
                    // A load's waits scale with this period (ADR 0099).
                    let frames = (data.len() / channels) as u64;
                    let us = frames * 1_000_000 / u64::from(sample_rate);
                    engine.sources.set_period_us(us as u32);
                    for frame in data.chunks_mut(channels) {
                        let (l, r) = rate.frame(|| {
                            if block_pos >= BLOCK_SIZE {
                                engine.block(&LOAD_LINK, &mut drain, shared, &mut scope);
                                let inst = &*engine.inst;
                                formant.publish(|v| inst.formant_views_into(v));
                                block_pos = 0;
                            }
                            let (lr, clamped) = stereo_frame(engine.dac.out(), solo, block_pos);
                            if clamped {
                                audio.clamped.fetch_add(1, Ordering::Relaxed);
                            }
                            block_pos += 1;
                            lr
                        });
                        match frame {
                            [mono] => *mono = 0.5 * (l + r),
                            [fl, fr, rest @ ..] => {
                                (*fl, *fr) = (l, r);
                                rest.fill(0.0);
                            }
                            [] => {}
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
            #[cfg(feature = "midi")]
            _midi: connect_midi(producers.take(MIDI).expect("MIDI producer taken once")),
            shared,
            keys,
            shared_audio,
            knobs: Box::new(KnobPublisher::new()),
            kits: KitWriter::take().expect("kit writer once"),
            clamp_log: ClampLog::default(),
        }
    }

    /// Push the Performance, tagged with the load `epoch`, and ORBIT's
    /// settings (`orbit` writes every field) to the audio thread through the
    /// triple buffer, and report any mixdown frames clamped since the last
    /// call.
    pub fn update(&mut self, project: &Project, epoch: u32, orbit: impl FnOnce(&mut AudioShared)) {
        let knobs = &mut self.knobs;
        self.shared_audio.publish(|b| {
            b.update_from_project(project, epoch, knobs);
            orbit(b);
        });
        self.kits.publish_all(project.kits());
        let clamped = self.shared.clamped.swap(0, Ordering::Relaxed);
        if let Some(n) = self.clamp_log.note(clamped, Instant::now()) {
            eprintln!("speakers: {n} frames of the pairs' sum clamped at full scale");
        }
    }

    /// The pads struck since the last call, handed to `ui`.
    pub fn take_hits(&mut self, ui: &mut chimera_core::ui::UiState) {
        ui.take_hits(&mut self.kits);
    }

    /// The audio thread has taken the last `update`.
    #[cfg(feature = "midi")]
    pub fn taken(&self) -> bool {
        self.shared_audio.taken()
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
    runner: Box<OrbitRunner>,
    view: Writer<OrbitView>,
    /// The source table's one reader (ADR 0087).
    sources: AudioToken,
    /// The card model, when on.
    streams: Option<DeskStreams>,
}

impl Engine {
    /// `sources`: its table's one reader (`SOURCE_TABLE`'s for the sim).
    fn new(sample_rate: u32, view: Writer<OrbitView>, sources: AudioToken) -> Self {
        Self {
            inst: Box::new(Instrument::new(
                sample_rate,
                SampleBudget::for_cpu(CPU_HZ_REV_V),
            )),
            fx: Box::new(FxBus::new()),
            dac: DacBlocks::new(),
            gate: LoadGate::new(),
            runner: Box::new(OrbitRunner::new(
                core::num::NonZeroU32::new(sample_rate).expect("sample rate"),
            )),
            view,
            sources,
            streams: None,
        }
    }

    /// One block, as the firmware renders each half: the gate, then only
    /// when it opens the drain (live notes to any capture and then the
    /// Parts, real-time to ORBIT's clock) and ORBIT, then the render and
    /// ORBIT's report. With the card model on, each streamed voice reads
    /// its ring (`card_stream`), as `engine::render_half` does on the chip.
    fn block<const N: usize>(
        &mut self,
        link: &LoadLink,
        drain: &mut NoteDrain<'_, N>,
        shared: &AudioShared,
        scope: &mut ScopeWriter,
    ) {
        match self.streams.as_mut() {
            Some(st) => {
                let reg = st.reg;
                let frames = |id| reg.get(id).map(StreamFile::frames);
                let now = AudioBlocks::new(st.blocks.load(Ordering::Relaxed));
                let mut ports = Streams {
                    audio: &mut st.audio,
                    card: StreamCard {
                        frames: &frames,
                        now,
                        bus: st.bus,
                        stats: st.stats,
                        // The card thread polls.
                        pend: || {},
                        loud: core::cell::Cell::new(false),
                    },
                };
                Self::run(
                    (&mut self.inst, &mut self.fx, &mut self.dac, &mut self.gate),
                    (&mut self.runner, &mut self.sources),
                    (link, drain, shared, scope),
                    &mut ports,
                );
                ports.answer_quiet(Ms(st.t0.elapsed().as_millis() as u32));
                st.bus.set_streams_active(self.inst.streaming());
                st.blocks.fetch_add(1, Ordering::Relaxed);
            }
            None => Self::run(
                (&mut self.inst, &mut self.fx, &mut self.dac, &mut self.gate),
                (&mut self.runner, &mut self.sources),
                (link, drain, shared, scope),
                &mut NoPorts,
            ),
        }
        let runner = &self.runner;
        self.view.publish(|v| runner.fill_view(v));
    }

    /// `block`'s notes and render, every voice's stream through `ports`.
    #[allow(clippy::type_complexity)]
    fn run<const N: usize, P: OrbitPorts>(
        (inst, fx, dac, gate): (&mut Instrument, &mut FxBus, &mut DacBlocks, &mut LoadGate),
        (runner, sources): (&mut OrbitRunner, &mut AudioToken),
        (link, drain, shared, scope): (
            &LoadLink,
            &mut NoteDrain<'_, N>,
            &AudioShared,
            &mut ScopeWriter,
        ),
        ports: &mut P,
    ) {
        // TAPE's voices read the pool through the block's view.
        inst.run_block_ported(
            sources,
            (fx, dac, scope),
            shared,
            ports,
            |inst, reads, ports| {
                if gate.before_block(link, inst, shared, Some(&mut *runner)) {
                    let now = runner.now();
                    drain.drain_held(&HOLD, |src, ev| match ev {
                        // Capture sees the raw key, before any snap.
                        InEvent::Note(n) => {
                            runner.live(n);
                            inst.handle_streamed(n, shared, NoteOrigin::Live, reads, ports);
                        }
                        InEvent::Bend(b) => inst.bend(b),
                        InEvent::Sustain(p) => inst.sustain(p),
                        InEvent::Mode(m) => inst.channel_mode(m),
                        InEvent::Expr(e) => inst.expr(e),
                        InEvent::Rt(rt) => runner.realtime(src.index(), rt, now),
                    });
                    runner.block_ported(inst, shared, reads, ports);
                }
            },
        );
    }
}

/// The program-change hold (ADR 0224).
static HOLD: chimera_core::note_queue::ProgramHold = chimera_core::note_queue::ProgramHold::new();

/// The card model's side of the audio (`card_stream`): the rings' audio
/// side, the registry the UI fills, the bus and counters both share.
pub struct DeskStreams {
    pub audio: AudioSide<'static>,
    pub reg: &'static StreamRegistry,
    pub bus: &'static CardBus,
    pub stats: &'static StreamStats,
    /// Blocks rendered: the card's deadlines' clock.
    pub blocks: &'static AtomicU32,
    /// The quiet's clock.
    pub t0: Instant,
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
fn connect_midi(notes: NoteProducer<'static>) -> Option<midir::MidiInputConnection<()>> {
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
    let mut port = crate::midi::Port::new(notes, &MIDI_CTRL);
    input
        .connect(
            &ports[i],
            "chimera-in",
            move |_stamp, bytes, _| port.bytes(bytes),
            (),
        )
        .map_err(|e| eprintln!("MIDI connect failed: {e}"))
        .ok()
}

/// Opens the MIDI output port whose name contains `wanted` and starts the
/// thread that sends `MIDI_OUT`'s messages to it, as the firmware's TX
/// interrupt sends them to DIN OUT. `false`: no such port.
#[cfg(feature = "midi")]
pub fn connect_midi_out(wanted: &str) -> bool {
    let Ok(output) =
        midir::MidiOutput::new("chimera").map_err(|e| eprintln!("MIDI out unavailable: {e}"))
    else {
        return false;
    };
    let ports = output.ports();
    let names: Vec<String> = ports
        .iter()
        .map(|p| output.port_name(p).unwrap_or_default())
        .collect();
    let Some(i) = crate::midi::pick_port(&names, Some(wanted)) else {
        eprintln!("no MIDI output port matches {wanted:?}");
        return false;
    };
    let mut conn = match output.connect(&ports[i], "chimera-out") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("MIDI out connect failed: {e}");
            return false;
        }
    };
    eprintln!("MIDI out: {}", names[i]);
    std::thread::spawn(move || {
        let mut line = crate::midi::Line::default();
        loop {
            let sent = crate::midi::drain_out(&MIDI_OUT, &mut line, |m| {
                if let Err(e) = conn.send(m) {
                    eprintln!("MIDI out: {e}");
                }
            });
            if sent == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    });
    true
}

/// Subnormals flush to zero on this thread, as the firmware's FPSCR FZ
/// and DN have them: f32 state (the reverb's allpasses, its dampers)
/// decays to exactly 0 on both, never sits in subnormals on the host.
/// Every callback, as it costs two instructions and cpal may move threads.
#[inline(always)]
fn flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    {
        // FTZ (bit 15) and DAZ (bit 6).
        const FTZ_DAZ: u32 = 0x8040;
        let mut csr: u32 = 0;
        // SAFETY: STMXCSR and LDMXCSR read and write this thread's SSE
        // control register through `csr`, a live local; setting FTZ and
        // DAZ changes only subnormal results and operands, and keeps the
        // rounding mode and exception masks as they were.
        unsafe {
            core::arch::asm!("stmxcsr [{0}]", in(reg) &mut csr, options(nostack));
            csr |= FTZ_DAZ;
            core::arch::asm!("ldmxcsr [{0}]", in(reg) &csr, options(nostack, readonly));
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // FZ (bit 24).
        // SAFETY: MRS and MSR read and write this thread's FPCR; setting
        // FZ changes only subnormal results and operands, and keeps the
        // rounding mode and traps as they were.
        unsafe {
            let fpcr: u64;
            core::arch::asm!("mrs {0}, fpcr", out(reg) fpcr, options(nomem, nostack));
            core::arch::asm!("msr fpcr, {0}", in(reg) fpcr | (1 << 24), options(nomem, nostack));
        }
    }
}

/// The engine's frames at `from` Hz to the device's `to`: linear between
/// neighbours, a frame late, with no anti-alias filter, so don't judge tone
/// on a 44.1 kHz device. At equal rates each frame passes untouched.
struct Resample {
    step: f64,
    frac: f64,
    /// The frames either side of the read, `None` before the first.
    pair: Option<((f32, f32), (f32, f32))>,
}

impl Resample {
    fn new(from: u32, to: u32) -> Self {
        Self {
            step: f64::from(from) / f64::from(to),
            frac: 0.0,
            pair: None,
        }
    }

    /// The next device frame, `pull` giving the engine's next.
    fn frame(&mut self, mut pull: impl FnMut() -> (f32, f32)) -> (f32, f32) {
        if self.step == 1.0 {
            return pull();
        }
        let (mut a, mut b) = self.pair.unwrap_or_else(|| (pull(), pull()));
        while self.frac >= 1.0 {
            self.frac -= 1.0;
            (a, b) = (b, pull());
        }
        let t = self.frac as f32;
        self.frac += self.step;
        self.pair = Some((a, b));
        (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
    }
}

#[cfg(test)]
mod tests {
    /// #239: at the unit's rate the frames pass as they are; to 44.1 kHz
    /// a 1 kHz tone stays 1 kHz, its level kept.
    #[test]
    fn the_engine_rate_reaches_any_device_rate() {
        let mut n = 0u32;
        let mut same = Resample::new(48_000, 48_000);
        for i in 0..100 {
            let f = same.frame(|| {
                n += 1;
                (n as f32, -(n as f32))
            });
            assert_eq!(f, ((i + 1) as f32, -((i + 1) as f32)));
        }
        let mut r = Resample::new(48_000, 44_100);
        let mut k = 0u32;
        let tone = |k: u32| (core::f32::consts::TAU * 1000.0 * k as f32 / 48_000.0).sin();
        let y: Vec<f32> = (0..44_100)
            .map(|_| {
                r.frame(|| {
                    k += 1;
                    (tone(k - 1), 0.0)
                })
                .0
            })
            .collect();
        let ups = y.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((999..=1001).contains(&ups), "{ups} cycles a second");
        let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.99 && peak <= 1.0, "{peak}");
    }

    /// A reader of a table of its own: the sim's is taken once.
    fn test_reader() -> chimera_core::source_table::AudioToken {
        let t = Box::leak(Box::new(chimera_core::source_table::SourceTable::new()));
        t.reader().unwrap()
    }

    use super::*;
    use chimera_core::preset::Performance;

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
        let mut e = Engine::new(SAMPLE_RATE, orbit_writer(), test_reader());
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

    /// Task 9's desktop QA without a window: `boot` over a card holding a
    /// saved project, over no card and over a fresh card. Each gives the
    /// toast the shell would draw, and a note held on the selected Part's
    /// channel sounds; on NEW, sample for sample as `Project::boxed`'s
    /// own Part plays it.
    #[test]
    fn each_card_boots_a_project_that_plays() {
        use chimera_core::project::test_support::{full, same};
        use chimera_core::project::{ProjectStatus, SaveTo, new_project_id, project_status};
        use chimera_core::storage::{Card, SystemSync};
        use chimera_core::ui::UiState;
        use chimera_core::ui::busy::ToastStep;
        use chimera_desktop::store::DirStore;

        let base = std::env::temp_dir().join(format!("chimera-qa-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (saved, fresh) = (base.join("saved"), base.join("fresh"));
        for d in [&saved, &fresh] {
            std::fs::create_dir_all(d).unwrap();
        }
        // The relaunch root: `full()` saved, so SYSTEM names it.
        let (want, _) = full();
        {
            let mut store = DirStore::new(saved.clone());
            let mut card = Card::new();
            let (mut sync, mut set, _) = SystemSync::boot(&mut card, &mut store);
            let mut ui = Box::new(UiState::new());
            *ui.project_mut() = *full().0;
            let file = new_project_id(&mut card, &mut store).out.unwrap();
            ui.save_project(
                &mut card,
                &mut store,
                &mut sync,
                &mut set,
                SaveTo::Fresh(file),
            );
            assert_eq!(
                project_status(ui.project(), ui.template()),
                ProjectStatus::Saved
            );
        }
        let cases = [
            (saved.clone(), None),
            (base.join("absent"), Some("NO CARD")),
            (fresh.clone(), Some("NEW PROJECT")),
        ];
        for (root, toast) in cases {
            let mut ui = Box::new(UiState::new());
            let mut card = Card::new();
            crate::boot(&mut ui, &mut card, &mut DirStore::new(root.clone()));
            let shown = match ui.step_toast(0) {
                ToastStep::Show(t) => Some(t),
                _ => None,
            };
            assert_eq!(shown.as_ref().map(|t| t.as_str()), toast, "{root:?}");
            let ch = ui.project().part(ui.active_part).mix.channel;
            let played = held_note(ui.project().perf(), ch);
            assert!(played.iter().any(|s| s.abs() > 1e-3), "{root:?}: silent");
            if toast.is_some() {
                let new = chimera_core::project::Project::boxed().0;
                assert_eq!(played, held_note(new.perf(), ch), "{root:?}");
            } else {
                same(ui.project(), &want);
                assert_eq!(
                    project_status(ui.project(), ui.template()),
                    ProjectStatus::Saved
                );
            }
        }
        let _ = std::fs::remove_dir_all(base);
    }

    fn orbit_writer() -> Writer<OrbitView> {
        Box::leak(Box::new(chimera_core::orbit::runner::orbit_buffer()))
            .split()
            .0
    }

    /// Task 5's pre-flight: ORBIT's init scene playing for 20 s renders the
    /// same twice, every pair's block bit for bit (the runner holds no
    /// wall-clock or allocator state the render could depend on).
    #[test]
    fn desktop_render_is_repeatable() {
        let render = || {
            let sources: &'static NoteSources<1> = Box::leak(Box::new(NoteSources::new()));
            let (_producers, mut drain) = sources.split().unwrap();
            let mut e = Engine::new(SAMPLE_RATE, orbit_writer(), test_reader());
            let (w, _unread) = Box::leak(Box::new(chimera_core::scope::scope_buffer())).split();
            let mut scope = ScopeWriter::new(w);
            let link = LoadLink::new();
            let mut shared = Box::new(AudioShared::default());
            shared
                .orbit
                .push(chimera_core::orbit::channel::Cmd::TogglePlay);
            let mut sums = Vec::new();
            for _ in 0..(20 * SAMPLE_RATE as usize) / BLOCK_SIZE {
                e.block(&link, &mut drain, &shared, &mut scope);
                // FNV-1a over every pair's bits.
                let mut h = 0xcbf2_9ce4_8422_2325_u64;
                for x in e.dac.out().iter().flatten() {
                    h = (h ^ x.to_bits() as u64).wrapping_mul(0x0100_0000_01b3);
                }
                sums.push(h);
            }
            sums
        };
        let (a, b) = (render(), render());
        assert!(a.windows(2).any(|w| w[0] != w[1]), "something rendered");
        assert!(a == b, "two runs differ");
    }

    /// 100 ms of the speakers' left channel with middle C held on `ch`
    /// from the first block, `perf` published.
    fn held_note(perf: &Performance, ch: MidiChannel) -> Vec<f32> {
        let sources: &'static NoteSources<1> = Box::leak(Box::new(NoteSources::new()));
        let (mut producers, mut drain) = sources.split().unwrap();
        let mut keys = producers.take(SourceId::new(0)).unwrap();
        let mut e = Engine::new(SAMPLE_RATE, orbit_writer(), test_reader());
        let (w, _unread) = Box::leak(Box::new(chimera_core::scope::scope_buffer())).split();
        let mut scope = ScopeWriter::new(w);
        let link = LoadLink::new();
        let mut shared = Box::new(AudioShared::default());
        shared.update_from(perf, link.epoch());
        keys.push(NoteEvent {
            channel: ch,
            note: MidiNote::new(60).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        });
        let mut out = Vec::new();
        for _ in 0..(SAMPLE_RATE as usize / 10).div_ceil(BLOCK_SIZE) {
            e.block(&link, &mut drain, &shared, &mut scope);
            out.extend((0..BLOCK_SIZE).map(|i| stereo_frame(e.dac.out(), 0, i).0.0));
        }
        out
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
