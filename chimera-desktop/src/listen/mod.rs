//! The TAPE listening rig (spec § 9, Desktop QA): a source held as the
//! importer holds it, played offline through a model under a macro script,
//! checked (peaks, the click rule, NaN) before the owner hears it, and laid
//! out on a page with a transport per render.

use std::fmt::Write as _;
use std::mem::MaybeUninit;

use crate::import::tape::convert::{Mono, Pitch, clean, quantise_tpdf, resample, yin};
use crate::import::tape::transwave::{DEFAULT_TOP, MAX_FRAMES, cut_cycles, round12, transwave};
use chimera_core::block::{Block, DiskCode, ParamId};
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, MIN_SOURCE_FRAMES, Shape, SourceView,
};
use chimera_core::dsp::tape_engine::{
    StretchType, TapeEngine, TapeEngineParams as P, TapeModel, Wow,
};
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::storage::twelve::pack12;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

/// After the note-off, how long a Chimera source rings on.
const TAIL_S: f32 = 1.0;
/// The dither's seed: a source rebuilds bit for bit.
const SEED: u64 = 0x7A9E;

/// A factory Sound held on `note` for `secs`, then `TAIL_S` of its release,
/// rendered offline through `Instrument` at 48 kHz: DAC pair 1, `(L + R)/2`.
/// Slots past the factory bank are NEW's INIT Sounds, by `EngineType::ALL`
/// (as `template::new_slot`): `FACTORY_LEN + 1` is INIT MODAL.
pub fn chimera_source(slot: usize, note: u8, secs: f32) -> Mono {
    chimera_render(slot, note, secs, &|_, _| {})
}

/// A transwave's Chimera source: factory slot 6 (MORPH PAD, ALGO) with
/// MORPH's base swept 0 → 127 across the hold (its LFO 1 rides on it), as
/// `chimera_source` renders it.
pub fn chimera_morph_sweep(note: u8, secs: f32) -> Mono {
    chimera_render(6, note, secs, &|t, p| {
        p.algo.morph = (127.0 * t.min(1.0)) as u8
    })
}

/// `chimera_source`, with `each(t, params)` run before every block, `t`
/// the hold's share gone (0 to 1, past 1 in the tail).
fn chimera_render(
    slot: usize,
    note: u8,
    secs: f32,
    each: &dyn Fn(f32, &mut chimera_core::params::ParamSnapshot),
) -> Mono {
    let sound = factory_sound(slot)
        .or_else(|| {
            let e = EngineType::ALL.get(slot.checked_sub(FACTORY_LEN)?)?;
            Some(Sound::init(*e))
        })
        .expect("a factory or INIT slot");
    render_sound(&sound, &[note], secs, TAIL_S, each)
}

/// `sound`'s `notes` held for `secs`, then `tail` of their release,
/// rendered offline through `Instrument` at 48 kHz: DAC pair 1,
/// `(L + R)/2`; `each(t, params)` before every block, `t` the hold's share
/// gone.
pub fn render_sound(
    sound: &Sound,
    notes: &[u8],
    secs: f32,
    tail: f32,
    each: &dyn Fn(f32, &mut chimera_core::params::ParamSnapshot),
) -> Mono {
    let mut shared = Box::<AudioShared>::default();
    shared.parts[0].params = sound.params.clone();
    shared.parts[0].mod_state = sound.mod_state.clone();
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(CPU_HZ_REV_V),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    let mut scope = ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0);
    let ev = |note, kind| NoteEvent {
        channel: MidiChannel::clamped(0),
        note: MidiNote::new(note).expect("a MIDI note"),
        kind,
    };
    let blocks = |s: f32| (s * SAMPLE_RATE as f32) as usize / BLOCK_SIZE;
    for &n in notes {
        inst.handle_with(
            ev(n, NoteKind::On(Velocity::DEFAULT)),
            &shared,
            &GateView::empty(),
        );
    }
    let mut samples = Vec::new();
    for b in 0..blocks(secs + tail) {
        if b == blocks(secs) {
            for &n in notes {
                inst.handle_with(ev(n, NoteKind::Off), &shared, &GateView::empty());
            }
        }
        each(b as f32 / blocks(secs) as f32, &mut shared.parts[0].params);
        inst.render_with(&mut fx, &mut dac, &shared, &mut scope, &GateView::empty());
        let lr = &dac.out()[0];
        samples.extend(lr.chunks(2).map(|f| 0.5 * (f[0] + f[1])));
    }
    Mono {
        rate: SAMPLE_RATE,
        samples,
        smpl: None,
    }
}

/// `m` as the importer holds it at `rate`: cleaned, resampled, pitched by
/// YIN, dithered to 12 bits, then the descriptor and the packed samples.
/// An empty or silent `m` holds no source, nor one under
/// `MIN_SOURCE_FRAMES` (64) at `rate`: an error.
pub fn held_from(m: &Mono, rate: u32) -> Result<(Vec<u8>, Pitch), String> {
    let mut c = m.clone();
    clean(&mut c);
    if c.samples.is_empty() {
        return Err("no audio: empty or silent".into());
    }
    let x =
        resample(&c.samples, c.rate, rate).map_err(|_| format!("can't resample {} Hz", c.rate))?;
    if x.len() < MIN_SOURCE_FRAMES as usize {
        return Err(format!(
            "too short: {} frames at {rate} Hz, a source needs {MIN_SOURCE_FRAMES}",
            x.len()
        ));
    }
    let pitch = yin(&x, rate);
    let q = quantise_tpdf(&x, SEED);
    let desc = Descriptor {
        shape: Shape::Plain,
        count: 1,
        rate: u16::try_from(rate).expect("a rate under 65 536"),
        head_frames: 0,
    };
    let mut held = desc.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * q.len() / 2, 0);
    pack12(&q, &mut held[DESC_LEN..]);
    Ok((held, pitch))
}

/// `m` as a `Transwave` of `frames` frames, as the importer will hold one
/// (spec § 6 step 5): cleaned; whole cycles at its YIN period cut from
/// across it (`cut_cycles`, at most 32), `frames` of them taken evenly;
/// each built into levels (nominal 256 down to 16, stored oversampled); the
/// whole set's peak to −0.5 dBFS, rounded to 12 bits: a fixed single-cycle
/// table gains nothing from dither, and its noise would eat the levels'
/// 60 dB. An unpitched or too short `m` is an error.
pub fn held_transwave(m: &Mono, frames: usize) -> Result<(Vec<u8>, f32), String> {
    let mut c = m.clone();
    clean(&mut c);
    let Pitch::Pitched { period, .. } = yin(&c.samples, c.rate) else {
        return Err("unpitched: no cycle to cut".into());
    };
    let cycles = cut_cycles(&c.samples, period, MAX_FRAMES);
    if cycles.is_empty() {
        return Err("too short for a cycle".into());
    }
    let n = frames.clamp(1, cycles.len());
    let pick = |k: usize| {
        if n > 1 {
            k * (cycles.len() - 1) / (n - 1)
        } else {
            0
        }
    };
    let picked: Vec<Vec<f32>> = (0..n).map(|k| cycles[pick(k)].clone()).collect();
    let q = round12(&transwave(&picked, DEFAULT_TOP)).ok_or("silent cycles")?;
    let desc = Descriptor {
        shape: Shape::Transwave,
        count: n as u8,
        rate: 48_000,
        head_frames: 0,
    };
    let mut held = desc.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * q.len() / 2, 0);
    pack12(&q, &mut held[DESC_LEN..]);
    Ok((held, period * SAMPLE_RATE as f32 / c.rate as f32))
}

/// The source the A/B compares against: `m` cleaned, at 48 kHz.
pub fn dry(m: &Mono) -> Vec<f32> {
    let mut c = m.clone();
    clean(&mut c);
    resample(&c.samples, c.rate, SAMPLE_RATE).expect("a source rate")
}

/// The note that plays a source at its own speed: its root, or C4.
pub fn root_note(pitch: Pitch) -> u8 {
    match pitch {
        Pitch::Pitched { root_cents, .. } => ((root_cents as u32 + 50) / 100).min(127) as u8,
        Pitch::Unpitched => 60,
    }
}

/// Breakpoints `(seconds, param, value)`: each param linear between its
/// own, in time order, and held before the first and after the last. A
/// param the script never names stays neutral. It ends at its last
/// breakpoint. Then the seconds a fresh note-on comes, past the first (a
/// take: the head back at START, MODEL and TYPE latched again), and each
/// note's semitones over the render's, the first's first (none: 0).
pub struct Script(
    pub &'static [(f32, ParamId, f32)],
    pub &'static [f32],
    pub &'static [u8],
);

impl Script {
    pub fn secs(&self) -> f32 {
        self.0.iter().fold(0.0, |a, k| a.max(k.0))
    }

    fn at(&self, id: ParamId, t: f32) -> Option<f32> {
        let mut prev: Option<(f32, f32)> = None;
        for &(tk, _, v) in self.0.iter().filter(|k| k.1 == id) {
            if t < tk {
                return Some(match prev {
                    None => v,
                    Some((t0, v0)) => v0 + (v - v0) * (t - t0) / (tk - t0),
                });
            }
            prev = Some((tk, v));
        }
        prev.map(|(_, v)| v)
    }

    /// Every scripted param at `t`.
    pub fn apply(&self, p: &mut P, t: f32) {
        for &(_, id, _) in self.0 {
            if let Some(v) = self.at(id, t) {
                p.write(id, v);
            }
        }
    }
}

/// 2 s held at neutral; POSITION, then MOTION, then SIZE each swept
/// 0 → 1 → 0 over 3 s (SIZE glides from neutral 1 to 0 in the 250 ms
/// before its sweep, and back after); then TAPE and AGE glide to 0.5 and
/// hold 3 s.
static SWEEP: [(f32, ParamId, f32); 16] = [
    (2.0, P::POSITION, 0.0),
    (3.5, P::POSITION, 1.0),
    (5.0, P::POSITION, 0.0),
    (5.0, P::MOTION, 0.0),
    (6.5, P::MOTION, 1.0),
    (8.0, P::MOTION, 0.0),
    (7.75, P::SIZE, 1.0),
    (8.0, P::SIZE, 0.0),
    (9.5, P::SIZE, 1.0),
    (11.0, P::SIZE, 0.0),
    (11.25, P::SIZE, 1.0),
    (11.25, P::TAPE, 0.0),
    (11.5, P::TAPE, 0.5),
    (14.5, P::TAPE, 0.5),
    (11.25, P::AGE, 0.0),
    (11.5, P::AGE, 0.5),
];

/// What the page says the script does.
pub const SWEEP_LEGEND: &str = "0–2 s neutral · 2–5 s POSITION 0→1→0 · 5–8 s MOTION 0→1→0 · \
    8–11 s SIZE 0→1→0 · 11.5–14.5 s TAPE and AGE at 0.5";

/// SCRUB's: 2 s at neutral, where the tape stands still and is silent;
/// then the loop takes 0.3 of the window and runs at the note's speed
/// (MOTION 0.5) over 250 ms, as POSITION drags the finger 0 → 1 → 0; MOTION
/// runs 0.5 → −1 → 0.5, through a stop into reverse; SIZE and TAPE and AGE
/// as `SWEEP`'s.
static SCRUB_SWEEP: [(f32, ParamId, f32); 20] = [
    (2.0, P::POSITION, 0.0),
    (3.5, P::POSITION, 1.0),
    (5.0, P::POSITION, 0.0),
    (2.0, P::MOTION, 0.0),
    (2.25, P::MOTION, 0.5),
    (5.0, P::MOTION, 0.5),
    (6.5, P::MOTION, -1.0),
    (8.0, P::MOTION, 0.5),
    (2.0, P::SIZE, 1.0),
    (2.25, P::SIZE, 0.3),
    (7.75, P::SIZE, 0.3),
    (8.0, P::SIZE, 0.0),
    (9.5, P::SIZE, 1.0),
    (11.0, P::SIZE, 0.0),
    (11.25, P::SIZE, 0.3),
    (11.25, P::TAPE, 0.0),
    (11.5, P::TAPE, 0.5),
    (14.5, P::TAPE, 0.5),
    (11.25, P::AGE, 0.0),
    (11.5, P::AGE, 0.5),
];

/// What the page says SCRUB's script does.
pub const SCRUB_LEGEND: &str = "0–2 s neutral (the tape stands still: silent) · 2–2.25 s to \
    SIZE 0.3, MOTION 0.5 (the note's speed) · 2–5 s POSITION (the finger) 0→1→0 · 5–8 s \
    MOTION 0.5→−1→0.5 · 8–11 s SIZE 0→1→0 · 11.5–14.5 s TAPE and AGE at 0.5";

/// TRANSWAVE's: 2 s on frame 0; POSITION scans the table 0 → 1 → 0;
/// then at POSITION ½ MOTION runs the scan up to 0.5 (4 Hz) and back, the
/// triangle half the table either way (SIZE ½); SIZE then widens it 0 → 1
/// → 0 under MOTION 0.25 (2 Hz); TAPE and AGE as `SWEEP`'s.
static WAVE_SWEEP: [(f32, ParamId, f32); 21] = [
    (2.0, P::POSITION, 0.0),
    (3.5, P::POSITION, 1.0),
    (4.75, P::POSITION, 0.5),
    (5.0, P::MOTION, 0.0),
    (6.5, P::MOTION, 0.5),
    (7.75, P::MOTION, 0.25),
    (4.75, P::SIZE, 1.0),
    (5.0, P::SIZE, 0.5),
    (7.75, P::SIZE, 0.5),
    (8.0, P::SIZE, 0.0),
    (9.5, P::SIZE, 1.0),
    (11.0, P::SIZE, 0.0),
    (11.0, P::MOTION, 0.25),
    (11.25, P::MOTION, 0.0),
    (11.25, P::SIZE, 1.0),
    (11.25, P::POSITION, 0.5),
    (11.25, P::TAPE, 0.0),
    (11.5, P::TAPE, 0.5),
    (14.5, P::TAPE, 0.5),
    (11.25, P::AGE, 0.0),
    (11.5, P::AGE, 0.5),
];

/// STRETCH's SIZE for a D-TIME of 60 ms: `log(6) / log(25)`.
const D_TIME_60: f32 = 0.556_641;

/// STRETCH's: 2 s frozen at START (MOTION 0) at D-TIME 250 ms; then the
/// same passage from START, a fresh take each, at 0.5×, 1× and 2× its time
/// (MOTION 0.25, 0.5, 1), pitch fixed, D-TIME 60 ms; then at 0.5× again at
/// D-TIME 10 ms (metallic) and 250 ms (tremolo).
static STRETCH_SWEEP: [(f32, ParamId, f32); 12] = [
    (1.99, P::MOTION, 0.0),
    (2.0, P::MOTION, 0.25),
    (4.99, P::MOTION, 0.25),
    (5.0, P::MOTION, 0.5),
    (7.99, P::MOTION, 0.5),
    (8.0, P::MOTION, 1.0),
    (10.99, P::MOTION, 1.0),
    (11.0, P::MOTION, 0.25),
    (14.5, P::MOTION, 0.25),
    (1.99, P::SIZE, 1.0),
    (2.0, P::SIZE, D_TIME_60),
    (10.99, P::SIZE, D_TIME_60),
];

/// STRETCH's SIZE after 11 s: D-TIME 10 ms, then 250 ms from 12.75 s.
static STRETCH_SIZE_TAIL: [(f32, ParamId, f32); 4] = [
    (11.0, P::SIZE, 0.0),
    (12.74, P::SIZE, 0.0),
    (12.75, P::SIZE, 1.0),
    (14.5, P::SIZE, 1.0),
];

/// STRETCH's takes.
static STRETCH_TAKES: [f32; 5] = [2.0, 5.0, 8.0, 11.0, 12.75];

/// STRETCH's breakpoints, whole.
static STRETCH_KEYS: [(f32, ParamId, f32); 16] = {
    let mut k = [(0.0, P::SIZE, 0.0); 16];
    let mut i = 0;
    while i < 12 {
        k[i] = STRETCH_SWEEP[i];
        i += 1;
    }
    while i < 16 {
        k[i] = STRETCH_SIZE_TAIL[i - 12];
        i += 1;
    }
    k
};

/// What the page says STRETCH's script does.
pub const STRETCH_LEGEND: &str = "Each take from START, pitch fixed · 0–2 s frozen (MOTION 0), \
    D-TIME 250 ms · 2–5 s 0.5× time · 5–8 s 1× · 8–11 s 2× (D-TIME 60 ms) · 11–12.75 s 0.5× \
    at D-TIME 10 ms (metallic) · 12.75–14.5 s 0.5× at 250 ms (tremolo)";

/// What the page says TRANSWAVE's script does.
pub const WAVE_LEGEND: &str = "16 frames cut from the source · 0–2 s frame 0 · 2–4.75 s \
    POSITION scans 0→1→½ · 5–7.75 s MOTION 0→0.5→0.25 (a 4 Hz scan, SIZE ½) · 8–11 s SIZE \
    0→1→0 at 2 Hz · 11.5–14.5 s TAPE and AGE at 0.5";

/// The macro script `model` is heard under, and what the page says of it.
pub fn script(model: TapeModel) -> Script {
    match model {
        TapeModel::Scrub => Script(&SCRUB_SWEEP, &[], &[]),
        TapeModel::Transwave => Script(&WAVE_SWEEP, &[], &[]),
        TapeModel::Stretch => Script(&STRETCH_KEYS, &STRETCH_TAKES, &[]),
        TapeModel::Oneshot => Script(&ONESHOT_KEYS, &ONESHOT_TAKES, &[]),
        TapeModel::Tron => Script(&TRON_KEYS, &[10.0], &[0, 12]),
        TapeModel::Loop | TapeModel::PingPong => Script(&SWEEP, &[], &[]),
    }
}

pub fn legend(model: TapeModel) -> &'static str {
    match model {
        TapeModel::Scrub => SCRUB_LEGEND,
        TapeModel::Transwave => WAVE_LEGEND,
        TapeModel::Stretch => STRETCH_LEGEND,
        TapeModel::Oneshot => ONESHOT_LEGEND,
        TapeModel::Tron => TRON_LEGEND,
        TapeModel::Loop | TapeModel::PingPong => SWEEP_LEGEND,
    }
}

/// ONESHOT's: the whole window once; then a take every 0.5 s, SLICE's knob
/// stepping slices 1 → 16 (the note fixed); from 10 s the whole window
/// with END before START, a take a second, TAPE and AGE at 0.5 from 11.5 s.
static ONESHOT_KEYS: [(f32, ParamId, f32); 15] = [
    (2.0, P::SLICE, 0.0),
    (2.25, P::SLICE, 2.0),
    (9.75, P::SLICE, 17.0),
    (9.99, P::SLICE, 17.0),
    (10.0, P::SLICE, 0.0),
    (9.99, P::START, 0.0),
    (10.0, P::START, 1.0),
    (9.99, P::END, 1.0),
    (10.0, P::END, 0.0),
    (11.25, P::TAPE, 0.0),
    (11.5, P::TAPE, 0.5),
    (14.5, P::TAPE, 0.5),
    (11.25, P::AGE, 0.0),
    (11.5, P::AGE, 0.5),
    (14.5, P::AGE, 0.5),
];

static ONESHOT_TAKES: [f32; 20] = [
    2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5, 9.0, 9.5, 10.0, 11.0, 12.0,
    13.0, 14.0,
];

pub const ONESHOT_LEGEND: &str = "0–2.5 s the whole window once · 2.5–10 s a take each 0.5 s, \
    SLICE 1→16 by the knob · 10–14.5 s END before START (reversed), a take a second · \
    11.5–14.5 s TAPE and AGE at 0.5";

/// ONESHOT at SLICE NOTE: 16 notes from C4, 0.5 s apart, each its slice
/// at ROOT's pitch.
static ONESHOT_NOTE_KEYS: [(f32, ParamId, f32); 1] = [(8.0, P::TAPE, 0.0)];
static ONESHOT_NOTE_TAKES: [f32; 15] = [
    0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0, 5.5, 6.0, 6.5, 7.0, 7.5,
];
static ONESHOT_NOTE_NOTES: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

/// TRON's: held 10 s at ROOT, then 10 s an octave up; LENGTH 8 s, so the
/// tape runs out (or END comes) before each key's 10 s.
static TRON_KEYS: [(f32, ParamId, f32); 1] = [(20.0, P::TAPE, 0.0)];

pub const TRON_LEGEND: &str = "0–10 s held at ROOT · 10–20 s held an octave up: each key \
    its own 8 s of tape (END may come first)";

/// How a machine is heard besides its plain script (spec § 2 Machines):
/// END before START, WOW WARBLE, SLICE NOTE.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    Plain,
    Reversed,
    Warble,
    SliceNote,
}

impl Way {
    /// The ways `m` is rendered.
    pub fn of(m: TapeModel) -> &'static [Way] {
        match m {
            TapeModel::Loop => &[Way::Plain, Way::Reversed, Way::Warble],
            TapeModel::Stretch => &[Way::Plain, Way::Reversed],
            TapeModel::Oneshot => &[Way::Plain, Way::SliceNote],
            _ => &[Way::Plain],
        }
    }

    /// Added to a row's slug and title.
    pub fn slug(self) -> &'static str {
        match self {
            Way::Plain => "",
            Way::Reversed => "-rev",
            Way::Warble => "-warble",
            Way::SliceNote => "-note",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Way::Plain => "",
            Way::Reversed => " · END before START",
            Way::Warble => " · WOW WARBLE, TAPE 0.5 from 11.5 s",
            Way::SliceNote => " · SLICE NOTE, 16 notes from C4",
        }
    }

    /// `model`'s params before the script: END before START swaps the
    /// window (REVERSE's renders, #369), WARBLE in place of the capstan,
    /// SLICE NOTE.
    pub fn base(self, model: TapeModel, kind: StretchType) -> P {
        let p = P {
            model,
            kind,
            ..P::default()
        };
        match self {
            Way::Plain => p,
            Way::Reversed => P {
                start: 1.0,
                end: 0.0,
                ..p
            },
            Way::Warble => P {
                wow: Wow::Warble,
                ..p
            },
            Way::SliceNote => P { slice: 1, ..p },
        }
    }

    pub fn script(self, model: TapeModel) -> Script {
        match self {
            Way::SliceNote => Script(&ONESHOT_NOTE_KEYS, &ONESHOT_NOTE_TAKES, &ONESHOT_NOTE_NOTES),
            _ => script(model),
        }
    }

    /// The note it plays on: SLICE NOTE's start at C4, slice 1.
    pub fn note(self, root: u8) -> u8 {
        if self == Way::SliceNote { 60 } else { root }
    }
}

/// `held` played by `model` (TYPE `kind`) on `note` under `s`, from a
/// fresh engine: the engine's own output, 48 kHz. Before each take the
/// output fades out over `RELEASE` samples, as a VCA letting go would, and
/// the take is a fresh note on the idle voice (`hold`, then `note_on`).
/// Panics on a `held` with no source in it.
pub fn render(base: P, held: &[u8], pitch: Pitch, note: u8, s: &Script) -> Vec<f32> {
    let v = SourceView::new(held).expect("a held source");
    let mut p = base;
    let key = |k: usize| {
        note.saturating_add(s.2.get(k).copied().unwrap_or(0))
            .min(127)
    };
    if let Pitch::Pitched { period, root_cents } = pitch {
        (p.root, p.period) = (f32::from(root_cents), period);
    }
    s.apply(&mut p, 0.0);
    let mut slot = MaybeUninit::uninit();
    let e = TapeEngine::init_in_place(&mut slot);
    e.hold();
    e.note_on(key(0), &p, &v, SAMPLE_RATE);
    let mut take = 0;
    let blocks = (s.secs() * SAMPLE_RATE as f32) as usize / BLOCK_SIZE;
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut blk = [0.0; BLOCK_SIZE];
    let at = |b: usize| (b * BLOCK_SIZE) as f32 / SAMPLE_RATE as f32;
    for b in 0..blocks {
        s.apply(&mut p, at(b));
        if b > 0 && s.1.iter().any(|&t| at(b - 1) < t && t <= at(b)) {
            let n0 = out.len();
            for (i, y) in out[n0.saturating_sub(RELEASE)..].iter_mut().enumerate() {
                *y *= 1.0 - (i + 1) as f32 / RELEASE as f32;
            }
            e.hold();
            take += 1;
            e.note_on(key(take), &p, &v, SAMPLE_RATE);
        }
        e.render(&mut blk, &p, &v, SAMPLE_RATE);
        out.extend_from_slice(&blk);
    }
    out
}

/// A take's release: 10 ms.
const RELEASE: usize = 480;

/// The samples `render` starts a fresh note at under `s`: 0, and each
/// take's block.
pub fn take_starts(s: &Script) -> Vec<usize> {
    let at = |b: usize| (b * BLOCK_SIZE) as f32 / SAMPLE_RATE as f32;
    let blocks = (s.secs() * SAMPLE_RATE as f32) as usize / BLOCK_SIZE;
    let mut v = vec![0];
    v.extend(
        (1..blocks)
            .filter(|&b| s.1.iter().any(|&t| at(b - 1) < t && t <= at(b)))
            .map(|b| b * BLOCK_SIZE),
    );
    v
}

/// Claude's QA of a render (QA before UAT).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Qa {
    pub peak_dbfs: f32,
    /// The worst sample's `isolation` under either detector (`qa`).
    pub click_ratio: f32,
    /// The sample it scored at.
    pub click_at: usize,
    pub non_finite: usize,
}

/// A render passes at most this `click_ratio`: tuned on the LOOP renders
/// (≤ 1.92) and the test clicks (≥ 4).
pub const CLICK_RATIO: f32 = 3.0;

/// A render must be louder than this: silence is a failed render.
pub const SILENT_DBFS: f32 = -80.0;

/// Detector values at or under this are not clicks (−60 dBFS).
const FLOOR: f32 = 1e-3;

/// How far a click looks for its peers: ±21 ms, a period down to 47 Hz.
const REACH: usize = 1_024;

/// Samples about a candidate that its own click spreads into.
const OWN: usize = 2;

/// Block length of `isolation`'s running maxima.
const BLOCK: usize = 64;

/// The worst ratio of a sample of `r` (over `FLOOR`) to the largest of
/// its peers within ±`REACH`, the sample's own ±`OWN` left out. A periodic
/// edge has peers as large one period away; a click has none. With the
/// sample it is at.
fn isolation(r: &[f32], skip: &[bool]) -> (f32, usize) {
    let a: Vec<f32> = r.iter().map(|v| v.abs()).collect();
    let blocks: Vec<f32> = a
        .chunks(BLOCK)
        .map(|c| c.iter().fold(0f32, |m, &v| m.max(v)))
        .collect();
    // The largest of `a[lo..hi]`: loose ends sample by sample, whole blocks
    // by their maxima.
    let max = |lo: usize, hi: usize| {
        let (mut m, mut i) = (0f32, lo);
        while i < hi {
            if i % BLOCK == 0 && i + BLOCK <= hi {
                m = m.max(blocks[i / BLOCK]);
                i += BLOCK;
            } else {
                m = m.max(a[i]);
                i += 1;
            }
        }
        m
    };
    let n = a.len();
    (0..n)
        .filter(|&i| a[i] > FLOOR && !skip[i])
        .map(|i| {
            let left = max(i.saturating_sub(REACH), i.saturating_sub(OWN));
            let right = max((i + OWN + 1).min(n), (i + REACH + 1).min(n));
            (a[i] / left.max(right).max(f32::MIN_POSITIVE), i)
        })
        .fold((0.0, 0), |m, r| if r.0 > m.0 { r } else { m })
}

/// Two detectors, each judged by `isolation`: the second difference
/// `x[i+1] − 2x[i] + x[i−1]`, which finds a small click in a smooth wave;
/// and the residual from a 5-point running median, which leaves a
/// band-limited edge (monotone) alone and finds a click riding on one.
/// A median of the second difference over a window (the first idea) failed
/// a clean saw at 60–270×: one edge in a window looks like a click.
pub fn qa(x: &[f32]) -> Qa {
    qa_takes(x, &[])
}

/// How long an onset is judged by its step, not its bend: 2 ms, the
/// engine's 1 ms note-on ramp and the attack's first cycle of a high note.
const ONSET: usize = 96;

/// An onset's steepest step may be this many times the material's own
/// after it (`REACH`), or this share of its level (twice the engine's 1 ms
/// smoothstep ramp's steepest, 1.5/48), before it is a click.
const ONSET_STEP: f32 = 3.0;
const ONSET_RAMP: f32 = 1.0 / 16.0;

/// `qa`, told where `takes` start (fresh notes, the render's first
/// included): a take's onset from silence has no peers before it, and its
/// attack (the engine's declicked ramp, then the source's own) bends more
/// than anything after it, so curvature alone flags it (#385). The first
/// `ONSET` samples of each take are judged instead by their steepest step,
/// against the material's own over the `REACH` after or the declick ramp's
/// at the onset's level: a step at an onset still fails.
pub fn qa_takes(x: &[f32], takes: &[usize]) -> Qa {
    let finite = |s: &&f32| s.is_finite();
    let peak = x.iter().filter(finite).fold(0f32, |a, s| a.max(s.abs()));
    let y: Vec<f32> = x
        .iter()
        .map(|&s| if s.is_finite() { s } else { 0.0 })
        .collect();
    let n = y.len();
    let d2: Vec<f32> = (0..n)
        .map(|i| match (i.checked_sub(1), y.get(i + 1)) {
            (Some(p), Some(&q)) => q - 2.0 * y[i] + y[p],
            _ => 0.0,
        })
        .collect();
    let med: Vec<f32> = (0..n)
        .map(|i| {
            let (Some(lo), Some(_)) = (i.checked_sub(2), y.get(i + 2)) else {
                return 0.0;
            };
            let mut w: [f32; 5] = y[lo..lo + 5].try_into().expect("five samples");
            w.sort_unstable_by(f32::total_cmp);
            y[i] - w[2]
        })
        .collect();
    let mut skip = vec![false; n];
    let mut onset = (0f32, 0usize);
    for &t in takes.iter().filter(|&&t| t < n) {
        let end = (t + ONSET).min(n);
        skip[t..end].fill(true);
        let step = |lo: usize, hi: usize| {
            (lo.max(1)..hi.min(n))
                .map(|i| (y[i] - y[i - 1]).abs())
                .fold(0f32, f32::max)
        };
        let level = y[t..end].iter().fold(0f32, |m, v| m.max(v.abs()));
        let bar = (ONSET_STEP * step(end, end + REACH))
            .max(ONSET_RAMP * level)
            .max(FLOOR);
        // On `CLICK_RATIO`'s scale: the bar passes at it.
        let r = step(t, end) / bar * CLICK_RATIO;
        if r > onset.0 {
            onset = (r, t);
        }
    }
    let (a, b) = (isolation(&d2, &skip), isolation(&med, &skip));
    let (click_ratio, click_at) = [a, b, onset]
        .into_iter()
        .fold((0f32, 0), |m, r| if r.0 > m.0 { r } else { m });
    Qa {
        peak_dbfs: 20.0 * peak.log10(),
        click_ratio,
        click_at,
        non_finite: n - x.iter().filter(finite).count(),
    }
}

impl Qa {
    /// Finite, sounding, at or under 0 dBFS, and click-free.
    pub fn passes(&self) -> bool {
        self.non_finite == 0
            && self.peak_dbfs > SILENT_DBFS
            && self.peak_dbfs <= 0.0
            && self.click_ratio <= CLICK_RATIO
    }
}

/// A source's row on the page.
#[derive(Clone, Debug)]
pub struct Src {
    /// The file names' middle: `<model>-<slug>-{dry,wet}.wav`.
    pub slug: String,
    pub title: String,
    pub author: String,
    pub licence: String,
    pub url: Option<String>,
}

/// A table's `key = "value"` lines, in order.
pub type Table = Vec<(String, String)>;

/// `file`'s `[[kind]]` tables of `key = "value"` lines, `#` comments and
/// blank lines, each with its kind; anything else is an error, so a
/// malformed file fails loudly.
pub fn tables(toml: &str, file: &str) -> Result<Vec<(String, Table)>, String> {
    let mut tables: Vec<(String, Table)> = Vec::new();
    for (n, line) in toml.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let bad = || format!("{file}:{}: not key = \"value\": {l}", n + 1);
        let key =
            |k: &str| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if let Some(kind) = l.strip_prefix("[[").and_then(|l| l.strip_suffix("]]")) {
            if !key(kind) {
                return Err(bad());
            }
            tables.push((kind.into(), Vec::new()));
            continue;
        }
        let (k, v) = l.split_once('=').ok_or_else(bad)?;
        let v = v.trim();
        let v = v
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .filter(|v| !v.contains('"'))
            .ok_or_else(bad)?;
        let k = k.trim();
        if !key(k) {
            return Err(bad());
        }
        tables
            .last_mut()
            .ok_or_else(bad)?
            .1
            .push((k.into(), v.into()));
    }
    Ok(tables)
}

/// `listen.toml`: `[[source]]` tables (`tables`). Each needs `file`;
/// `slug` defaults to the file's stem.
pub fn credits(toml: &str) -> Result<Vec<(String, Src)>, String> {
    let tables = tables(toml, "listen.toml")?;
    if let Some((k, _)) = tables.iter().find(|t| t.0 != "source") {
        return Err(format!("listen.toml: [[{k}]], not [[source]]"));
    }
    tables
        .into_iter()
        .map(|(_, t)| {
            let get = |k: &str| t.iter().find(|e| e.0 == k).map(|e| e.1.clone());
            let file = get("file").ok_or("listen.toml: a [[source]] without file")?;
            let stem = file.trim_end_matches(".wav").to_lowercase();
            let src = Src {
                slug: get("slug").unwrap_or(stem),
                title: get("title").unwrap_or_else(|| file.clone()),
                author: get("author").unwrap_or_default(),
                licence: get("licence").unwrap_or_default(),
                url: get("url"),
            };
            Ok((file, src))
        })
        .collect()
}

/// `model`'s name in file names and on the command line: `loop`,
/// `pingpong`, `transwave`….
pub fn model_slug(model: TapeModel) -> String {
    model.disk_ident().to_lowercase().replace('_', "")
}

pub fn file_name(model: TapeModel, src: &Src, kind: &str) -> String {
    format!("{}-{}-{kind}.wav", model_slug(model), src.slug)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const PAGE: &str = include_str!("page.html");

/// The row's author, linked when its URL is http(s); an empty author shows
/// the URL.
fn credit(s: &Src) -> String {
    let url = s.url.as_deref().filter(|u| !u.is_empty());
    let name = match (s.author.as_str(), url) {
        ("", Some(u)) => u,
        (a, _) => a,
    };
    match url {
        Some(u) if u.starts_with("https://") || u.starts_with("http://") => format!(
            "<a href=\"{}\" rel=\"noopener noreferrer\">{}</a>",
            esc(u),
            esc(name)
        ),
        _ => esc(name),
    }
}

/// The listening page: a section per model, in `TapeModel::ALL`'s order,
/// a row per source, each with its transport.
pub fn page(rows: &[(TapeModel, &Src)]) -> String {
    let mut out = String::new();
    for m in TapeModel::ALL {
        let mut it = rows.iter().filter(|r| r.0 == m).peekable();
        if it.peek().is_none() {
            continue;
        }
        let name = m.disk_ident().replace('_', "-");
        let _ = write!(
            out,
            "<section><h2>{name}</h2><p class=\"legend\">{}</p>",
            esc(legend(m))
        );
        for &(m, s) in it {
            let credit = credit(s);
            let _ = write!(
                out,
                r#"<article class="track" data-dry="{dry}" data-wet="{wet}">
<div class="meta"><h3>{title}</h3><p>{credit} · {licence}</p></div>
<div class="transport">
<button type="button" data-act="play" aria-label="Play">▶</button>
<button type="button" data-act="stop" aria-label="Stop">■</button>
<button type="button" data-act="back" aria-label="Back 2 s">−2s</button>
<button type="button" data-act="fwd" aria-label="Forward 2 s">+2s</button>
<input type="range" data-act="seek" min="0" max="1000" value="0" aria-label="Seek">
<span class="time">0:00 / 0:00</span>
<button type="button" class="loop" data-act="loop" aria-pressed="false">LOOP</button>
<button type="button" class="ab" data-act="ab" aria-pressed="true" title="A/B: the dry source or the render, at the same place">WET</button>
</div>
</article>
"#,
                dry = esc(&file_name(m, s, "dry")),
                wet = esc(&file_name(m, s, "wet")),
                title = esc(&s.title),
                licence = esc(&s.licence),
            );
        }
        out.push_str("</section>\n");
    }
    PAGE.replace("<!--SECTIONS-->", &out)
}

/// The WAVs' ends fade over 10 ms, so a player stopping there never clicks.
const FADE: usize = 480;

/// `x` as a 48 kHz, 16-bit mono WAV, its end faded over `FADE`.
pub fn write_wav(path: &std::path::Path, x: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let err = |e: hound::Error| format!("{}: {e}", path.display());
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    let n = x.len();
    for (i, &s) in x.iter().enumerate() {
        let fade = ((n - i) as f32 / FADE as f32).min(1.0);
        let v = chimera_core::dsp::clampf(s * fade, -1.0, 1.0) * 32_767.0;
        w.write_sample(v.round() as i16).map_err(err)?;
    }
    w.finalize().map_err(err)
}

/// Ungated BS.1770 loudness of an interleaved stereo run at 48 kHz, as
/// `chimera-core`'s level tests take it (ADR 0063): the K-weighting's
/// shelf and high-pass, then −0.691 + 10·log10 of the sides' mean squares
/// summed.
pub fn lufs(x: &[f32]) -> f32 {
    let biquad = |b: [f64; 3], a: [f64; 2], s: &[f64]| -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        s.iter()
            .map(|&x0| {
                let y = b[0] * x0 + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
                (x2, x1, y2, y1) = (x1, x0, y1, y);
                y
            })
            .collect()
    };
    let k = |s: Vec<f64>| {
        let s = biquad(
            [
                1.535_124_859_586_97,
                -2.691_696_189_406_38,
                1.198_392_810_852_85,
            ],
            [-1.690_659_293_182_41, 0.732_480_774_215_85],
            &s,
        );
        biquad(
            [1.0, -2.0, 1.0],
            [-1.990_047_454_833_98, 0.990_072_250_366_21],
            &s,
        )
    };
    let ms = |s: Vec<f64>| s.iter().map(|v| v * v).sum::<f64>() / s.len().max(1) as f64;
    let l = k(x.iter().step_by(2).map(|&v| v as f64).collect());
    let r = k(x.iter().skip(1).step_by(2).map(|&v| v as f64).collect());
    (-0.691 + 10.0 * (ms(l) + ms(r)).max(1e-20).log10()) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(slug: &str) -> Src {
        Src {
            slug: slug.into(),
            title: slug.to_uppercase(),
            author: "Chimera".into(),
            licence: "CC0 1.0".into(),
            url: Some("https://example.org/a?b&c".into()),
        }
    }

    #[test]
    fn every_script_starts_neutral_and_runs_14_5_s() {
        for m in TapeModel::ALL {
            let s = script(m);
            // TRON holds a key 10 s, twice.
            let secs = if m == TapeModel::Tron { 20.0 } else { 14.5 };
            assert_eq!(s.secs(), secs, "{m:?}");
            let (mut a, mut b) = (P::default(), P::default());
            s.apply(&mut a, 0.0);
            s.apply(&mut b, 1.9);
            for p in [a, b] {
                assert_eq!(p, P::default(), "{m:?}: not neutral before 2 s");
            }
            assert!(s.1.iter().all(|&t| (2.0..s.secs()).contains(&t)), "{m:?}");
        }
    }

    /// STRETCH's takes: 0.5×, 1× and 2× at 60 ms, then 0.5× at 10 and
    /// 250 ms, each settled before its note-on.
    #[test]
    fn stretch_script_plays_its_takes() {
        use chimera_core::dsp::tape_engine::stretch::{d_time_s, time_speed};
        let s = script(TapeModel::Stretch);
        let want = [
            (0.5, 0.06),
            (1.0, 0.06),
            (2.0, 0.06),
            (0.5, 0.01),
            (0.5, 0.25),
        ];
        assert_eq!(s.1.len(), want.len());
        for (&t, (speed, d)) in s.1.iter().zip(want) {
            let mut p = P::default();
            s.apply(&mut p, t);
            assert!((time_speed(p.motion) - speed).abs() < 1e-6, "{t} s");
            assert!(
                (d_time_s(p.size) - d).abs() < 1e-4,
                "{t} s: {}",
                d_time_s(p.size)
            );
        }
    }

    #[test]
    fn page_lists_every_track() {
        let (a, b) = (src("tx-bass"), src("choir"));
        let rows = [(TapeModel::Loop, &a), (TapeModel::Loop, &b)];
        let html = page(&rows);
        for s in ["tx-bass", "choir"] {
            for kind in ["dry", "wet"] {
                let f = format!("loop-{s}-{kind}.wav");
                assert!(html.contains(&f), "{f} missing");
            }
        }
        for act in ["play", "stop", "seek", "back", "fwd", "loop", "ab"] {
            let n = html.matches(&format!("data-act=\"{act}\"")).count();
            assert_eq!(n, rows.len(), "{act} per row");
        }
        assert!(
            html.contains("https://example.org/a?b&amp;c"),
            "links escaped"
        );
        assert_eq!(
            html.matches("rel=\"noopener noreferrer\"").count(),
            rows.len()
        );
    }

    #[test]
    fn only_http_links() {
        let mut s = src("x");
        s.url = Some("javascript:alert(1)".into());
        let html = page(&[(TapeModel::Loop, &s)]);
        assert!(!html.contains("href=\"javascript"), "plain text");
        s.url = Some("https://example.org/f".into());
        s.author.clear();
        let html = page(&[(TapeModel::Loop, &s)]);
        assert!(
            html.contains(">https://example.org/f</a>"),
            "the URL stands in"
        );
    }

    #[test]
    fn credits_reads_and_refuses() {
        let ok = "# a\n\n[[source]]\nfile = \"A.wav\"\ntitle = \"T\"\nurl = \"https://e.org\"\n";
        let c = credits(ok).unwrap();
        assert_eq!((c[0].0.as_str(), c[0].1.slug.as_str()), ("A.wav", "a"));
        assert_eq!(c[0].1.title, "T");
        for bad in [
            "[[source]]\nfile = A.wav\n",
            "[[source]]\nfile = \"A.wav\" # x\n",
            "file = \"A.wav\"\n",
            "[[source]]\nfile = \"A.wav\"\n[sources]\n",
            "[[source]]\ntitle = \"T\"\n",
        ] {
            assert!(credits(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_empty_wav_is_an_error() {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut bytes = std::io::Cursor::new(Vec::new());
        hound::WavWriter::new(&mut bytes, spec)
            .unwrap()
            .finalize()
            .unwrap();
        let held = crate::import::tape::convert::read_wav(
            bytes.get_ref(),
            crate::import::tape::convert::Channel::Mix,
        )
        .map_err(|e| format!("{e:?}"))
        .and_then(|m| held_from(&m, 24_000));
        assert!(held.is_err(), "{held:?}");
    }

    fn sine(amp: f32) -> Vec<f32> {
        (0..48_000)
            .map(|i| amp * (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin())
            .collect()
    }

    /// A band-limited saw at 110 Hz, harmonics to 0.35 × 48 kHz.
    fn saw() -> Vec<f32> {
        (0..48_000)
            .map(|i| {
                (1..153)
                    .map(|k| {
                        let w = std::f32::consts::TAU * 110.0 * k as f32 / 48_000.0;
                        0.3 * (w * i as f32).sin() / k as f32
                    })
                    .sum()
            })
            .collect()
    }

    /// A source under 64 frames at the held rate is refused, by name.
    #[test]
    fn a_too_short_source_is_an_error() {
        let m = |n: usize| Mono {
            rate: 24_000,
            samples: (0..n).map(|i| 0.5 * (i as f32 * 0.3).sin()).collect(),
            smpl: None,
        };
        let e = held_from(&m(63), 24_000).unwrap_err();
        assert!(e.contains("too short"), "{e}");
        assert!(held_from(&m(200), 24_000).is_ok());
    }

    #[test]
    fn a_clean_saw_passes() {
        assert!(qa(&saw()).passes(), "{:?}", qa(&saw()));
    }

    #[test]
    fn a_click_on_saw_and_noise_fails() {
        let mut s = 1u32;
        let mut x: Vec<f32> = saw()
            .into_iter()
            .map(|v| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                v + 0.04 * (s as f32 / u32::MAX as f32 - 0.5)
            })
            .collect();
        assert!(qa(&x).passes(), "{:?}", qa(&x));
        x[24_100] += 0.6;
        x[24_101] += 0.6;
        assert!(!qa(&x).passes(), "{:?}", qa(&x));
    }

    #[test]
    fn a_small_click_on_a_sine_fails() {
        let mut x = sine(0.5);
        x[24_000] += 0.05;
        assert!(!qa(&x).passes(), "{:?}", qa(&x));
    }

    #[test]
    fn silence_fails() {
        assert!(!qa(&vec![0.0; 48_000]).passes());
        assert!(!qa(&sine(1e-5)).passes(), "−100 dBFS");
    }

    /// 0.1 s of silence, then a bass at 0.8 starting at its peak (a fresh
    /// take at START), under the engine's 1 ms smoothstep note-on ramp
    /// (`ramp`) or none.
    fn take(ramp: bool) -> Vec<f32> {
        let mut x = vec![0.0f32; 4_800];
        x.extend((0..43_200).map(|i| {
            let r = (i as f32 / 48.0).min(1.0);
            let g = if ramp { r * r * (3.0 - 2.0 * r) } else { 1.0 };
            g * 0.8 * (std::f32::consts::TAU * 65.4 * i as f32 / 48_000.0).cos()
        }));
        x
    }

    /// A take's onset from silence has no peers before it, so curvature
    /// alone flags a declicked attack (#385); told where takes start, the QA
    /// judges an onset by its step against the material's own slope after
    /// it.
    #[test]
    fn a_declicked_onset_passes_where_takes_start() {
        let x = take(true);
        assert!(!qa(&x).passes(), "{:?}", qa(&x));
        let q = qa_takes(&x, &[4_800]);
        assert!(q.passes(), "{q:?}");
    }

    #[test]
    fn a_step_at_an_onset_fails() {
        let x = take(false);
        let q = qa_takes(&x, &[4_800]);
        assert!(!q.passes(), "{q:?}");
    }

    #[test]
    fn qa_refuses_a_click() {
        let mut x: Vec<f32> = (0..48_000)
            .map(|i| 0.5 * (std::f32::consts::TAU * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        assert!(qa(&x).passes(), "a clean sine passes: {:?}", qa(&x));
        x[24_000] += 0.3;
        assert!(!qa(&x).passes(), "{:?}", qa(&x));
    }
}
