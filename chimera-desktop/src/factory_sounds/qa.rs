//! QA before UAT (spec § 4): each factory Sound played headlessly through
//! the real `Instrument` (as `loudness_test` plays a scene: Part 1, P1,
//! after the limiter), measured dry (its sends at 0) by pure gates over
//! the sample buffers; the listening renders carry its sends through the
//! FX bus at NEW's settings, the first of them a four-bar phrase (a
//! listening render, never a gate). Then `report.txt` and the owner's
//! `listening.html` with the renders, all in the ledger folder. `write`
//! is the only I/O.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use chimera_core::block::DiskCode;
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus};
use chimera_core::dsp::limiter::OUTPUT_GAIN;
use chimera_core::dsp::modulator::law::{B_TIME, speed_ranges};
use chimera_core::dsp::modulator::{EnvSlot, EnvType};
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::storage::FactoryId;
use chimera_core::{MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use super::about::{self, Fact};
use super::source::{Badge, Phrase, Script, Source, patch, to_sound, trimmed};
use crate::listen::{self, CLICK_RATIO, Player, Row, qa_takes};

const SR: f32 = SAMPLE_RATE as f32;

/// ADR 0109's reference (ADR 0063's at the master gain): ALGO INIT's C4
/// at velocity 100 on P1.
pub const REFERENCE_LUFS: f32 = -11.0;
/// The loudness gate's half-width.
pub const LOUDNESS_LU: f32 = 2.0;
/// Gain reduction a single note may take, and the chord `stab`.
pub const LIMIT_SINGLE_DB: f32 = 1.0;
pub const LIMIT_CHORD_DB: f32 = 3.0;
/// DC on a held note against the note, per channel (`dc_db`): an offset
/// of 1 % of the RMS. Its step at note-on and note-off sits 40 dB under
/// the note (masked; subsonic) and costs under 0.1 dB of headroom; a wave
/// a fold or crush shifts by 3 % of its RMS reads -30 dB. The nine VA
/// Sounds' held notes read -43 (GLIDE BASS at v127) to -108 dB.
pub const DC_REL_DB: f32 = -40.0;
/// What counts as DC: energy under this.
const DC_HZ: f64 = 10.0;
/// Harmonic over inharmonic energy, at C6 and C7.
pub const ALIAS_DB: f32 = 40.0;
/// Silence, and how long past the release it may take to get there.
pub const TAIL_DBFS: f32 = -90.0;
/// At or under this a measured peak is digital silence (`db`'s floor).
const SILENT_FLOOR_DB: f32 = -200.0;
pub const TAIL_GRACE_S: f32 = 2.0;
/// MODAL rings on past note-off (Rings): its own limit.
pub const MODAL_TAIL_S: f32 = 10.0;
/// The window the tail is judged over, at its limit.
const TAIL_WINDOW_S: f32 = 0.1;
/// The tap must move the spectrum at least this (RMS over the ERB bands).
pub const TAP_DB: f32 = 6.0;
/// Two Sounds within this on every ERB band are samey.
pub const SAMEY_DB: f32 = 3.0;
pub const ERB_BANDS: usize = 32;
/// The ERB bands span this.
const ERB_LO_HZ: f32 = 50.0;
const ERB_HI_HZ: f32 = 20_000.0;
/// The aliasing renders' notes: C6 and C7.
pub const ALIAS_NOTES: [u8; 2] = [84, 96];
/// After the last note-off, each script's tail (but `long`'s 2 s).
const TAIL_S: f32 = 1.5;
/// The level render holds the home note this long (`loudness_test`).
const LEVEL_HOLD_S: f32 = 1.0;
/// The limiter's lookahead: the output is a block late.
const LATE: usize = BLOCK_SIZE;

/// One note of a script: seconds on and off, the note and its velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub on: f32,
    pub off: f32,
    pub note: u8,
    pub vel: u8,
}

/// A script's notes and its length; `mode` overrides the Source's demo.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub hits: Vec<Hit>,
    pub secs: f32,
    pub mode: Option<PartMode>,
}

fn hit(on: f32, off: f32, note: u8, vel: u8) -> Hit {
    Hit { on, off, note, vel }
}

/// Up two octaves of a major seventh and back: `texture`'s sixteen notes.
const TEXTURE: [u8; 16] = [0, 4, 7, 11, 12, 16, 19, 23, 24, 23, 19, 16, 12, 11, 7, 4];

/// Spec § 4's render scripts, velocity 100 unless noted.
pub fn plan(s: Script, home: u8) -> Plan {
    let one = |held: f32, note: u8, tail: f32| Plan {
        hits: vec![hit(0.0, held, note, 100)],
        secs: held + tail,
        mode: None,
    };
    match s {
        Script::Bass => one(0.3, home, TAIL_S),
        Script::Short => one(0.08, 60, TAIL_S),
        Script::Long => one(2.0, home, 2.0),
        Script::Stab => Plan {
            hits: [48, 52, 55, 59].map(|n| hit(0.0, 0.25, n, 100)).to_vec(),
            secs: 0.25 + TAIL_S,
            mode: None,
        },
        // Sixteenths at 120 BPM, velocity 40 rising to 127.
        Script::Texture => Plan {
            hits: TEXTURE
                .iter()
                .enumerate()
                .map(|(i, &d)| {
                    let on = i as f32 * 0.125;
                    let vel = 40 + (87.0 * i as f32 / 15.0).round() as u8;
                    hit(on, on + 0.1, home.saturating_add(d).min(127), vel)
                })
                .collect(),
            secs: 2.0 + TAIL_S,
            mode: None,
        },
        // C2, G2, C3, each overlapping the next, on a MONO Part.
        Script::Glide => Plan {
            hits: vec![
                hit(0.0, 0.5, 36, 100),
                hit(0.4, 0.9, 43, 100),
                hit(0.8, 1.4, 48, 100),
            ],
            secs: 1.4 + TAIL_S,
            mode: Some(PartMode::Mono),
        },
    }
}

/// A sixteenth at the phrases' 120 BPM, s.
pub const SIXTEENTH_S: f32 = 0.125;
/// A phrase's length, in sixteenths: four bars of 4/4.
pub const PHRASE_STEPS: u8 = 64;
/// After a phrase's last note-off: a pad's release rings out.
const PHRASE_TAIL_S: f32 = 3.0;

/// One note of a phrase: semitones from the Sound's home note, its start
/// and length in sixteenths, its velocity. A note that runs past the next
/// one's start overlaps it: on a MONO Part, a legato slide (glide).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhraseNote {
    pub step: i8,
    pub at: u8,
    pub len: u8,
    pub vel: u8,
}

const fn n(step: i8, at: u8, len: u8, vel: u8) -> PhraseNote {
    PhraseNote { step, at, len, vel }
}

/// In the manner of Giorgio Moroder / Donna Summer – "I Feel Love" and
/// Human League – "Don't You Want Me" (rhythm/contour only, not
/// transcribed): the bouncing root-and-octave sixteenths of Eurodisco and
/// synth-pop, written fresh. A minor i–i–♭VI–♭VII, syncopated pushes on
/// the off-sixteenths, octave jumps, seven notes held into the next (slides
/// on MONO), and accents from 68 to 124 so velocity routes move.
pub const BASS_PHRASE: [PhraseNote; 37] = [
    // i
    n(0, 0, 2, 120),
    n(12, 2, 1, 70),
    n(0, 3, 2, 100),
    n(0, 6, 1, 84),
    n(12, 7, 3, 110),
    n(7, 9, 2, 90),
    n(10, 11, 1, 72),
    n(12, 12, 3, 104),
    n(0, 14, 2, 96),
    // i, answered
    n(0, 16, 2, 124),
    n(12, 18, 1, 68),
    n(0, 19, 1, 92),
    n(3, 20, 2, 100),
    n(0, 22, 1, 80),
    n(12, 23, 2, 112),
    n(10, 25, 3, 88),
    n(7, 27, 2, 96),
    n(12, 28, 1, 70),
    n(0, 29, 3, 108),
    // ♭VI
    n(-4, 32, 2, 120),
    n(8, 34, 1, 72),
    n(-4, 35, 2, 98),
    n(-4, 38, 1, 82),
    n(8, 39, 3, 112),
    n(3, 41, 2, 90),
    n(-4, 43, 1, 76),
    n(8, 44, 2, 104),
    n(-4, 46, 2, 94),
    // ♭VII, sliding home
    n(-2, 48, 2, 122),
    n(10, 50, 1, 70),
    n(-2, 51, 2, 100),
    n(5, 54, 1, 86),
    n(10, 55, 3, 114),
    n(12, 57, 2, 96),
    n(10, 59, 1, 74),
    n(7, 60, 3, 100),
    n(0, 62, 2, 118),
];

/// In the manner of Harold Faltermeyer – "Axel F" and Jan Hammer –
/// "Crockett's Theme" (rhythm/contour only, not transcribed): a singable
/// minor synth lead answered by itself, ending on a long held top note,
/// written fresh. Long and short notes, three legato slides, and a held
/// note of thirteen sixteenths (1.6 s) where vibrato and filter motion
/// show.
pub const LEAD_PHRASE: [PhraseNote; 21] = [
    n(7, 0, 3, 100),
    n(10, 3, 1, 84),
    n(12, 4, 6, 110),
    n(10, 10, 2, 90),
    n(7, 12, 4, 96),
    n(5, 16, 2, 92),
    n(7, 18, 1, 80),
    // A slide up from the minor third to the fourth, then down home.
    n(3, 19, 5, 104),
    n(5, 23, 3, 96),
    n(7, 26, 2, 90),
    n(0, 28, 4, 100),
    n(12, 32, 2, 112),
    n(15, 34, 2, 104),
    n(14, 36, 1, 86),
    // A slide down from the octave to the minor seventh.
    n(12, 37, 3, 98),
    n(10, 39, 2, 92),
    n(12, 41, 3, 108),
    // A slide up from the fifth into the held top note.
    n(7, 44, 5, 96),
    n(12, 48, 13, 116),
    n(10, 61, 1, 80),
    n(7, 62, 2, 90),
];

/// In the manner of Tears for Fears – "Mad World" and Ultravox – "Vienna"
/// (rhythm/contour only, not transcribed): slow minor changes, a chord a
/// bar, voiced open with common tones re-struck at each change, written
/// fresh. i – ♭VImaj7 – ♭III – ♭VII, each held its full bar.
pub const PAD_PHRASE: [PhraseNote; 16] = [
    n(0, 0, 16, 92),
    n(7, 0, 16, 88),
    n(12, 0, 16, 88),
    n(15, 0, 16, 84),
    n(-4, 16, 16, 92),
    n(7, 16, 16, 86),
    n(12, 16, 16, 88),
    n(15, 16, 16, 84),
    n(-9, 32, 16, 92),
    n(7, 32, 16, 86),
    n(10, 32, 16, 88),
    n(15, 32, 16, 84),
    n(-2, 48, 16, 94),
    n(5, 48, 16, 88),
    n(10, 48, 16, 88),
    n(14, 48, 16, 86),
];

/// In the manner of Plaid – the broken, interlocking arpeggios of "Not for
/// Threes" and "Rest Proof Clockwork" (rhythm/contour only, not
/// transcribed), written fresh: a minor-ninth arpeggio grouped in threes
/// across the 4/4 (3+3+3+2…), pushed onto the off-sixteenths and tied over
/// the beat, with a rest on bar 2's downbeat. i – i – ♭VI – ♭VII climbing
/// home to the octave-plus-fifth. Twenty-five short notes, no overlaps, so
/// each note's envelope and velocity (70 to 120) are heard alone.
pub const ARP_PHRASE: [PhraseNote; 25] = [
    // i, in threes, pushed across beat 4
    n(0, 0, 2, 110),
    n(7, 3, 1, 84),
    n(14, 6, 2, 100),
    n(10, 9, 1, 80),
    n(15, 11, 3, 112),
    n(12, 14, 1, 76),
    // i, answered off the downbeat
    n(0, 17, 1, 92),
    n(7, 19, 2, 104),
    n(10, 22, 1, 78),
    n(14, 23, 3, 108),
    n(7, 27, 1, 82),
    n(3, 29, 2, 100),
    n(5, 31, 1, 70),
    // ♭VI
    n(-4, 32, 2, 112),
    n(3, 35, 1, 84),
    n(10, 38, 2, 100),
    n(7, 41, 1, 80),
    n(12, 43, 3, 114),
    n(8, 46, 1, 76),
    // ♭VII, climbing home
    n(-2, 49, 1, 92),
    n(5, 51, 2, 104),
    n(10, 54, 1, 80),
    n(14, 55, 3, 110),
    n(17, 59, 1, 86),
    n(19, 61, 3, 120),
];

/// In the manner of Jega – the rubbery, lurching basslines of "Spectrum" –
/// and Proswell – a melodic bass that stutters then slides (rhythm/contour
/// only, not transcribed), written fresh. A minor line around i – i – ♭VI –
/// ♭VII with wide leaps (octave, minor ninth down to the fifth, a tritone),
/// accents landing off the beat (velocities 68 to 124) and five notes held
/// into the next: slides on a MONO Part.
pub const WONKY_PHRASE: [PhraseNote; 30] = [
    n(0, 0, 3, 112),
    n(0, 3, 2, 74),
    n(12, 5, 1, 120),
    // A slide down from the minor seventh to the fifth.
    n(10, 6, 4, 88),
    n(7, 9, 3, 96),
    n(-5, 13, 2, 124),
    n(-2, 15, 1, 70),
    n(0, 16, 2, 100),
    n(3, 19, 1, 118),
    n(0, 20, 2, 80),
    n(15, 22, 1, 90),
    // A slide down from the ninth to the octave.
    n(13, 23, 4, 100),
    n(12, 26, 2, 84),
    // The tritone, accented, sliding to the fourth.
    n(6, 29, 3, 116),
    n(5, 31, 1, 68),
    n(-4, 32, 3, 110),
    n(8, 35, 1, 72),
    n(-4, 37, 2, 122),
    n(-1, 40, 1, 76),
    // A slide down from the minor third to the flat second.
    n(3, 41, 4, 94),
    n(1, 44, 2, 104),
    n(-4, 47, 1, 70),
    n(-2, 48, 2, 108),
    n(10, 50, 1, 74),
    n(-2, 51, 3, 120),
    n(5, 54, 1, 82),
    // A slide from the fifth home.
    n(7, 55, 5, 98),
    n(0, 59, 2, 124),
    n(12, 61, 1, 80),
    n(0, 62, 2, 104),
];

/// A phrase's notes.
pub const fn phrase_notes(p: Phrase) -> &'static [PhraseNote] {
    match p {
        Phrase::Bass => &BASS_PHRASE,
        Phrase::Lead => &LEAD_PHRASE,
        Phrase::Pad => &PAD_PHRASE,
        Phrase::Arp => &ARP_PHRASE,
        Phrase::Wonky => &WONKY_PHRASE,
    }
}

/// `p` from `home` at 120 BPM, on the Sound's demo mode (so a MONO Part
/// glides), then `PHRASE_TAIL_S`.
pub fn phrase_plan(p: Phrase, home: u8) -> Plan {
    let hits: Vec<Hit> = phrase_notes(p)
        .iter()
        .map(|x| {
            let note = (home as i16 + x.step as i16).clamp(0, 127) as u8;
            let at = |steps: u8| steps as f32 * SIXTEENTH_S;
            hit(at(x.at), at(x.at + x.len), note, x.vel)
        })
        .collect();
    let end = hits.iter().map(|h| h.off).fold(0.0, f32::max);
    Plan {
        hits,
        secs: end + PHRASE_TAIL_S,
        mode: None,
    }
}

/// The block an instant falls in.
fn block(t: f32) -> usize {
    (t * SR / BLOCK_SIZE as f32).round() as usize
}

/// A played plan: P1 after the limiter (interleaved stereo), and the most
/// gain reduction the limiter took in any block, dB (0 or more).
struct Take {
    stereo: Vec<f32>,
    reduced_db: f32,
}

impl Take {
    fn mono(&self) -> Vec<f32> {
        self.stereo.chunks(2).map(|f| 0.5 * (f[0] + f[1])).collect()
    }
}

thread_local! {
    /// The scope every `play` writes (the UI's, unread here): one buffer per
    /// thread, allocated once, not one leaked per render.
    static SCOPE: std::cell::RefCell<ScopeWriter> =
        std::cell::RefCell::new(ScopeWriter::new(Box::leak(Box::new(scope_buffer())).split().0));
}

/// `plan` played by `snd` on Part 1 in `mode`, offline at 48 kHz through
/// `Instrument`: what `loudness_test`'s scenes measure.
fn play(snd: &Sound, mode: PartMode, plan: &Plan) -> Take {
    let mut perf = Performance::new();
    let e = perf.edit(PartId::ALL[0]);
    **e.sound = snd.clone();
    e.mix.mode = plan.mode.unwrap_or(mode);
    let shared = Box::new(AudioShared::from_performance(&perf));
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    let (mut fx, mut dac) = (Box::new(FxBus::new()), Box::new(DacBlocks::new()));
    SCOPE.with_borrow_mut(|scope| {
        let channel = shared.parts[0].mix.channel;
        let ev = |note: u8, kind| NoteEvent {
            channel,
            note: MidiNote::new(note).expect("a MIDI note"),
            kind,
        };
        let energy = |p: &[f32]| p.iter().map(|v| v * v).sum::<f32>();
        let mut t = Take {
            stereo: Vec::new(),
            reduced_db: 0.0,
        };
        let mut prev_in = 0.0f32;
        for b in 0..block(plan.secs) {
            for h in plan.hits.iter().filter(|h| block(h.off) == b) {
                inst.handle_with(ev(h.note, NoteKind::Off), &shared, &GateView::empty());
            }
            for h in plan.hits.iter().filter(|h| block(h.on) == b) {
                let v = Velocity::new(h.vel).expect("a velocity");
                inst.handle_with(ev(h.note, NoteKind::On(v)), &shared, &GateView::empty());
            }
            inst.render_with(&mut fx, &mut dac, &shared, scope, &GateView::empty());
            let out = &dac.out()[0];
            // The block out against the block in before it, over the rest gain.
            if prev_in > 1e-12 {
                let gr = 20.0
                    * ((energy(out) / prev_in).sqrt() / OUTPUT_GAIN)
                        .max(1e-9)
                        .log10();
                t.reduced_db = t.reduced_db.max(-gr);
            }
            prev_in = energy(&dac.input()[0]);
            t.stereo.extend_from_slice(out);
        }
        t
    })
}

/// The listening render: the Source's Sound played under `s` at 48 kHz,
/// mono `(L + R)/2`, its sends (if any) through the FX bus at NEW's
/// settings (`Performance::new`).
pub fn render(src: &Source, s: Script) -> Result<Vec<f32>, String> {
    Ok(play(&to_sound(src)?, src.demo, &plan(s, src.home)).mono())
}

/// `render` with the Sound's sends at 0: what the gates measure, so levels
/// compare with ADR 0109's whatever the effects add.
pub fn dry(src: &Source, s: Script) -> Result<Vec<f32>, String> {
    Ok(play(&dried(&to_sound(src)?), src.demo, &plan(s, src.home)).mono())
}

/// `snd` with its sends (ADR 0123) at 0.
fn dried(snd: &Sound) -> Sound {
    let mut d = snd.clone();
    d.params.out.sends = [0.0; FX_SENDS];
    d
}

/// The effects `snd`'s sends feed, in `FxBus` order, by their full names.
pub fn heard_through(snd: &Sound) -> Vec<&'static str> {
    ["CHORUS", "DELAY", "REVERB"]
        .into_iter()
        .zip(snd.params.out.sends)
        .filter(|&(_, s)| s > 0.0)
        .map(|(n, _)| n)
        .collect()
}

/// A gate's outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
    /// Not measured against the bar, and why: the owner's ear decides.
    Exempt(String),
}

fn verdict(ok: bool) -> Verdict {
    if ok { Verdict::Pass } else { Verdict::Fail }
}

fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// The home note at v100 held 1 s, LUFS: −11 ± 2 LU, unless set by ear.
pub fn loudness(lufs: f32, by_ear: Option<&str>) -> Verdict {
    match by_ear {
        Some(why) => Verdict::Exempt(format!("level by ear: {why}")),
        None => verdict((lufs - REFERENCE_LUFS).abs() <= LOUDNESS_LU),
    }
}

/// The most gain reduction the limiter took under `script` (`None`: the
/// level render): 1 dB for a single note, 3 dB for `stab`.
pub fn limiter(reduced_db: f32, script: Option<Script>) -> Verdict {
    verdict(reduced_db <= limiter_bar(script))
}

/// The limiter gate's bar under `script`.
pub fn limiter_bar(script: Option<Script>) -> f32 {
    if matches!(script, Some(Script::Stab)) {
        LIMIT_CHORD_DB
    } else {
        LIMIT_SINGLE_DB
    }
}

/// `listen::qa`'s click ratio over the whole render (note-offs and edges
/// included), each note-on in `onsets` judged as a take's onset; finite
/// and sounding. The value is the click ratio.
pub fn clicks(x: &[f32], onsets: &[usize]) -> (f32, Verdict) {
    let q = qa_takes(x, onsets);
    (
        q.click_ratio,
        verdict(q.passes() && q.click_ratio <= CLICK_RATIO),
    )
}

/// DC against the signal: each channel's energy under `DC_HZ` (and in the
/// window's main lobe, 4 bins) over frames `from..to` of interleaved stereo
/// (the held part), against the window's whole energy, dB; the worse
/// channel. A Blackman-Harris window (sidelobes at −92 dB) keeps a partial
/// cycle of a note from reading as an offset. Turning a Sound down, or a
/// long silent tail, changes nothing: what counts is the offset as a share
/// of what is playing.
pub fn dc_db(stereo: &[f32], from: usize, to: usize) -> f32 {
    let frames = stereo.len() / 2;
    let (from, to) = (from.min(frames), to.min(frames));
    (0..2)
        .map(|c| {
            let x: Vec<f32> = stereo[2 * from..2 * to]
                .iter()
                .skip(c)
                .step_by(2)
                .copied()
                .collect();
            let (p, bin) = spectrum(&x, true);
            let k = ((DC_HZ / bin) as usize).max(4).min(p.len() - 1);
            // One side of the spectrum: each bin but 0 counts twice.
            let side = |r: &[f64]| r.iter().sum::<f64>() * 2.0;
            let low = p[0] + side(&p[1..=k]);
            let all = p[0] + side(&p[1..]);
            (10.0 * (low.max(1e-24) / all.max(1e-24)).log10()) as f32
        })
        .fold(f32::NEG_INFINITY, f32::max)
}

/// `dc` over the span `hits` hold, the first note-on to the last
/// note-off, in a render of them (a block late).
pub fn held_dc(stereo: &[f32], hits: &[Hit]) -> (f32, Verdict) {
    let on = hits.iter().map(|h| block(h.on)).min().unwrap_or(0);
    let off = hits.iter().map(|h| block(h.off)).max().unwrap_or(0);
    dc(stereo, on * BLOCK_SIZE + LATE, off * BLOCK_SIZE + LATE)
}

/// No audible DC offset: `dc_db` at most `DC_REL_DB`.
pub fn dc(stereo: &[f32], from: usize, to: usize) -> (f32, Verdict) {
    let v = dc_db(stereo, from, to);
    (v, verdict(v.is_finite() && v <= DC_REL_DB))
}

/// `x`'s power spectrum under a 4-term Blackman-Harris window (sidelobes
/// at −92 dB), with its bin width in Hz.
fn spectrum(x: &[f32], windowed: bool) -> (Vec<f64>, f64) {
    let n = x.len().max(1);
    let w = |i: usize| {
        if !windowed {
            return 1.0;
        }
        let p = std::f64::consts::TAU * i as f64 / n as f64;
        0.358_75 - 0.488_29 * p.cos() + 0.141_28 * (2.0 * p).cos() - 0.011_68 * (3.0 * p).cos()
    };
    let mut buf: Vec<Complex<f64>> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| Complex::new(v as f64 * w(i), 0.0))
        .collect();
    buf.resize(n, Complex::new(0.0, 0.0));
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    let p = buf[..n / 2 + 1]
        .iter()
        .map(|c| c.norm_sqr() / (n as f64 * n as f64))
        .collect();
    (p, SR as f64 / n as f64)
}

/// A modulator's sidebands (an LFO on CUTOFF or pitch, up to 25 Hz) sit
/// this close to their harmonic: not aliases.
const SIDEBAND_HZ: f64 = 25.0;

/// Harmonic over inharmonic energy, dB, for a note at `f0`: a bin is
/// harmonic within `SIDEBAND_HZ` plus 0.3 % (5 cents of detune; at least
/// 8 bins, the window's main lobe) of a multiple of `f0 / 2`, so a sub an
/// octave down counts as harmonic. An alias folds back between them.
fn harmonic_db(x: &[f32], f0: f32) -> f32 {
    let (p, bin) = spectrum(x, true);
    let g = f0 as f64 / 2.0;
    let (mut h, mut inh) = (0.0f64, 0.0f64);
    for (k, &e) in p.iter().enumerate() {
        let f = k as f64 * bin;
        if !(20.0..=ERB_HI_HZ as f64).contains(&f) {
            continue;
        }
        let near = (f / g).round() * g;
        if near > 0.0 && (f - near).abs() <= (SIDEBAND_HZ + 0.003 * near).max(8.0 * bin) {
            h += e;
        } else {
            inh += e;
        }
    }
    (10.0 * (h.max(1e-30) / inh.max(1e-30)).log10()) as f32
}

/// For a harmonic Sound, `x` (a held note at `f0`) keeps inharmonic
/// energy 40 dB under the harmonic. A Source marked `harmonic = "no"`
/// (ALGO's and MODAL's default: bells, modes) is exempt and marked for
/// listening.
pub fn aliasing(x: &[f32], f0: f32, harmonic: bool) -> (f32, Verdict) {
    if !harmonic {
        return (
            f32::NAN,
            Verdict::Exempt("inharmonic: for listening".into()),
        );
    }
    let v = harmonic_db(x, f0);
    (v, verdict(v >= ALIAS_DB))
}

/// The peak, dBFS, over `TAIL_WINDOW_S` from `within_s` after the
/// note-off at sample `off`: silent (−90 dBFS) there. A render that ends
/// before the window fails.
pub fn tail(x: &[f32], off: usize, within_s: f32) -> (f32, Verdict) {
    let from = off + (within_s * SR) as usize;
    let to = from + (TAIL_WINDOW_S * SR) as usize;
    let Some(w) = x.get(from..to) else {
        return (f32::NAN, Verdict::Fail);
    };
    let peak = w.iter().fold(0f32, |m, v| m.max(v.abs()));
    let v = if peak.is_finite() { db(peak) } else { f32::NAN };
    (v, verdict(v <= TAIL_DBFS))
}

/// How long after note-off the Sound must fall silent: MODAL 10 s; any
/// other the longest release (A: its SPEED's range; B: FALL) among the
/// ENVs routed into the VCA (any slot may be, as the voice reads it),
/// ENV 1's when none is, plus 2 s.
pub fn tail_within(s: &Source, snd: &Sound) -> f32 {
    if s.engine == EngineType::Modal {
        return MODAL_TAIL_S;
    }
    let m = &snd.mod_state;
    let routed = |e: EnvSlot| {
        let i = ModSource::of_env(e).index();
        m.fixed_present(FixedCol::Vca) & (1 << i) != 0 && m.fixed_amount(i, FixedCol::Vca) != 0
    };
    let release = |e: EnvSlot| {
        let p = &snd.params.envelopes[e.index()];
        match p.env_type {
            EnvType::A => speed_ranges(p.speed).dec_rel.at(p.release),
            EnvType::B => B_TIME.at(p.func.fall),
        }
    };
    let longest = EnvSlot::ALL
        .into_iter()
        .filter(|&e| routed(e))
        .map(release)
        .reduce(f32::max)
        .unwrap_or_else(|| release(EnvSlot::Env1));
    longest + TAIL_GRACE_S
}

/// ERB-rate (Glasberg and Moore): `21.4 · log10(1 + 0.00437 f)`.
fn erb_rate(f: f32) -> f32 {
    21.4 * (1.0 + 0.004_37 * f).log10()
}

/// `x`'s energy in 32 bands equally spaced in ERB-rate from 50 Hz to
/// 20 kHz, dB (floor −120): the spectral metric `tap` and `samey` use.
pub fn erb_bands(x: &[f32]) -> [f32; ERB_BANDS] {
    let (p, bin) = spectrum(x, false);
    let (lo, hi) = (erb_rate(ERB_LO_HZ), erb_rate(ERB_HI_HZ));
    let mut e = [0f64; ERB_BANDS];
    for (k, &v) in p.iter().enumerate() {
        let f = (k as f64 * bin) as f32;
        if !(ERB_LO_HZ..ERB_HI_HZ).contains(&f) {
            continue;
        }
        let b = ((erb_rate(f) - lo) / (hi - lo) * ERB_BANDS as f32) as usize;
        e[b.min(ERB_BANDS - 1)] += v;
    }
    e.map(|v| (10.0 * v.max(1e-12).log10()) as f32)
}

/// RMS over the bands of their difference, dB.
pub fn spectral_error(a: &[f32; ERB_BANDS], b: &[f32; ERB_BANDS]) -> f32 {
    let ss: f32 = a.iter().zip(b).map(|(x, y)| (x - y).powi(2)).sum();
    (ss / ERB_BANDS as f32).sqrt()
}

/// The vocal Sounds' tap must be audible: 6 dB of spectral error between
/// the render with it and without.
pub fn tap(err_db: f32) -> Verdict {
    verdict(err_db >= TAP_DB)
}

/// Every pair `(i, j)`, `i < j`, within 3 dB on all 32 bands: reported
/// for a by-ear verdict.
pub fn samey(bands: &[[f32; ERB_BANDS]]) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    for i in 0..bands.len() {
        for j in i + 1..bands.len() {
            if bands[i]
                .iter()
                .zip(&bands[j])
                .all(|(a, b)| (a - b).abs() <= SAMEY_DB)
            {
                v.push((i, j));
            }
        }
    }
    v
}

/// What a listening render plays: the Sound's phrase, or a render script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audition {
    Phrase(Phrase),
    Script(Script),
}

impl Audition {
    /// Its WAV's name: `phrase-bass`, or the script's word.
    fn slug(self) -> String {
        match self {
            Audition::Phrase(p) => format!("phrase-{}", p.word()),
            Audition::Script(s) => s.word().into(),
        }
    }

    /// The player's label: `BASS PHRASE`, `LONG`.
    fn label(self) -> String {
        match self {
            Audition::Phrase(p) => format!("{} PHRASE", p.word().to_uppercase()),
            Audition::Script(s) => s.word().to_uppercase(),
        }
    }

    /// The bar its limiter reading is flagged over: a pad's chords (as
    /// `stab`) 3 dB, else 1 dB.
    pub fn bar(self) -> f32 {
        match self {
            Audition::Phrase(Phrase::Pad) => LIMIT_CHORD_DB,
            Audition::Phrase(_) => LIMIT_SINGLE_DB,
            Audition::Script(s) => limiter_bar(Some(s)),
        }
    }
}

/// What a gate checked.
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    Loudness,
    /// The level render (`None`) or a script's.
    Limiter(Option<Script>),
    Clicks(Script),
    /// A held home note at this velocity.
    Dc(u8),
    /// At this note.
    Aliasing(u8),
    Tail,
    Tap,
    /// The phrase render holds no NaN or infinity (the count of samples
    /// that are not finite).
    Finite(Phrase),
}

impl Check {
    fn label(&self) -> String {
        match self {
            Check::Loudness => "loudness".into(),
            Check::Limiter(None) => "limiter level".into(),
            Check::Limiter(Some(s)) => format!("limiter {}", s.word()),
            Check::Clicks(s) => format!("clicks {}", s.word()),
            Check::Dc(v) => format!("DC v{v}"),
            Check::Aliasing(n) => format!("aliasing C{}", n / 12 - 1),
            Check::Tail => "tail".into(),
            Check::Tap => "tap".into(),
            Check::Finite(p) => format!("finite {} phrase", p.word()),
        }
    }

    fn unit(&self) -> &'static str {
        match self {
            Check::Loudness => "LUFS",
            Check::Limiter(_) => "dB GR",
            Check::Clicks(_) => "x",
            Check::Dc(_) => "dB rel",
            Check::Aliasing(_) | Check::Tap => "dB",
            Check::Tail => "dBFS",
            Check::Finite(_) => "bad",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gate {
    pub check: Check,
    pub value: f32,
    pub verdict: Verdict,
}

/// One listening render, for the page: P1 as heard, interleaved stereo.
#[derive(Clone, Debug)]
pub struct Render {
    pub audition: Audition,
    pub stereo: Vec<f32>,
    pub lufs: f32,
    /// Left and right.
    pub peak_dbfs: [f32; 2],
    /// The effects heard on it (its sends), full words: empty when dry.
    pub with: Vec<&'static str>,
    /// A wet render's limiting, and always the phrase's, dB: information,
    /// not a gate (the gates measure dry).
    pub limited_db: Option<f32>,
}

/// A Sound's QA: every gate, every render, and its `long` render's ERB
/// bands (for `samey`).
#[derive(Clone, Debug)]
pub struct Report {
    pub id: FactoryId,
    pub name: String,
    pub note: String,
    pub demo: PartMode,
    pub edits: Option<String>,
    /// A port (it has a `from` line): the page's PRIVATE section.
    pub private: bool,
    pub gates: Vec<Gate>,
    pub renders: Vec<Render>,
    /// The dry `long` render's LUFS, beside the level figure.
    pub long_lufs: Option<f32>,
    pub bands: [f32; ERB_BANDS],
    pub badge: Badge,
    /// What it is (`about`), from the built Sound.
    pub about: Vec<Fact>,
}

impl Report {
    pub fn passes(&self) -> bool {
        self.gates.iter().all(|g| g.verdict != Verdict::Fail)
    }

    fn wav(&self, a: Audition) -> String {
        format!("renders/S{}-{}.wav", self.id.get(), a.slug())
    }

    fn lufs(&self) -> Option<f32> {
        self.gates
            .iter()
            .find(|g| g.check == Check::Loudness)
            .map(|g| g.value)
    }
}

/// A take as the page plays it, with its peaks and LUFS.
fn heard(audition: Audition, t: Take, with: &[&'static str], limited_db: Option<f32>) -> Render {
    let peak = |c: usize| {
        db(t.stereo
            .iter()
            .skip(c)
            .step_by(2)
            .fold(0f32, |m, v| m.max(v.abs())))
    };
    Render {
        audition,
        lufs: listen::lufs(&t.stereo),
        peak_dbfs: [peak(0), peak(1)],
        with: with.to_vec(),
        limited_db,
        stereo: t.stereo,
    }
}

fn hz(n: u8) -> f32 {
    440.0 * 2f32.powf((n as f32 - 69.0) / 12.0)
}

/// Every gate on `src`, through the renders its gates and `plays` need.
pub fn gates(src: &Source) -> Result<Report, String> {
    // Every gate plays dry; the page's renders carry the sends.
    let wet = to_sound(src)?;
    let with = heard_through(&wet);
    let snd = dried(&wet);
    let within = tail_within(src, &snd);
    let mut gates = Vec::new();
    let mut gate = |check, (value, verdict)| {
        gates.push(Gate {
            check,
            value,
            verdict,
        })
    };

    // The level render: the home note at v100, held 1 s, then its tail.
    let level = Plan {
        hits: vec![hit(0.0, LEVEL_HOLD_S, src.home, 100)],
        secs: LEVEL_HOLD_S + within + 2.0 * TAIL_WINDOW_S,
        mode: None,
    };
    let t = play(&snd, src.demo, &level);
    let held = (LEVEL_HOLD_S * SR) as usize;
    let lufs = listen::lufs(&t.stereo[2 * LATE..2 * (LATE + held)]);
    gate(
        Check::Loudness,
        (lufs, loudness(lufs, src.level_by_ear.as_deref())),
    );
    gate(
        Check::Limiter(None),
        (t.reduced_db, limiter(t.reduced_db, None)),
    );
    // DC where an offset would sit: the held home note, at v100 and at
    // v127 (velocity drives a fold or a bit crush harder).
    gate(Check::Dc(100), held_dc(&t.stereo, &level.hits));
    let hard = Plan {
        hits: vec![hit(0.0, LEVEL_HOLD_S, src.home, 127)],
        secs: LEVEL_HOLD_S + 0.05,
        mode: None,
    };
    gate(
        Check::Dc(127),
        held_dc(&play(&snd, src.demo, &hard).stereo, &hard.hits),
    );
    gate(Check::Tail, tail(&t.mono(), held + LATE, within));

    let mut renders = Vec::new();
    // The phrase first, as heard (its sends on): for the ear, not a gate,
    // but it must be finite.
    if let Some(ph) = src.phrase {
        let t = play(&wet, src.demo, &phrase_plan(ph, src.home));
        let bad = t.stereo.iter().filter(|v| !v.is_finite()).count();
        gate(Check::Finite(ph), (bad as f32, verdict(bad == 0)));
        let r = t.reduced_db;
        renders.push(heard(Audition::Phrase(ph), t, &with, Some(r)));
    }
    let mut dry_long = None;
    for &s in &src.plays {
        let p = plan(s, src.home);
        let t = play(&snd, src.demo, &p);
        let mono = t.mono();
        gate(
            Check::Limiter(Some(s)),
            (t.reduced_db, limiter(t.reduced_db, Some(s))),
        );
        let onsets: Vec<usize> = p
            .hits
            .iter()
            .map(|h| block(h.on) * BLOCK_SIZE + LATE)
            .collect();
        gate(Check::Clicks(s), clicks(&mono, &onsets));
        if s == Script::Long {
            dry_long = Some((mono, listen::lufs(&t.stereo)));
        }
        let (t, limited_db) = if with.is_empty() {
            (t, None)
        } else {
            let w = play(&wet, src.demo, &p);
            let r = w.reduced_db;
            (w, Some(r))
        };
        renders.push(heard(Audition::Script(s), t, &with, limited_db));
    }

    // `long`'s held part, past the attack.
    let held_part = |x: &[f32]| x[(0.25 * SR) as usize..(2.0 * SR) as usize].to_vec();
    let harmonic = src.harmonic;
    for n in ALIAS_NOTES {
        let x = if harmonic {
            held_part(&play(&snd, src.demo, &plan(Script::Long, n)).mono())
        } else {
            Vec::new()
        };
        gate(Check::Aliasing(n), aliasing(&x, hz(n), harmonic));
    }

    let (long, long_lufs) = match dry_long {
        Some(x) => x,
        None => {
            let t = play(&snd, src.demo, &plan(Script::Long, src.home));
            (t.mono(), listen::lufs(&t.stereo))
        }
    };
    if !src.taps.is_empty() {
        let mut bare = snd.clone();
        bare.params.taps = Default::default();
        let without = play(&bare, src.demo, &plan(Script::Long, src.home)).mono();
        let err = spectral_error(&erb_bands(&long), &erb_bands(&without));
        gate(Check::Tap, (err, tap(err)));
    }

    Ok(Report {
        id: src.id,
        name: src.name.as_str().into(),
        note: src.note.as_str().into(),
        demo: src.demo,
        edits: src.edits.clone(),
        private: src.from.is_some(),
        gates,
        renders,
        long_lufs: Some(long_lufs),
        bands: erb_bands(&long),
        badge: src.badge(),
        about: about::of(src)?,
    })
}

/// The `trim` that centres a Source measured at `lufs` on −11 LUFS, one
/// place after the point, refused (never clipped) when it would push OUT
/// LEVEL out of range: the error names the Source's trim line (or its id
/// line when it has none).
pub fn level_trim(src: &Source, lufs: f32) -> Result<f32, String> {
    let t = ((src.trim + REFERENCE_LUFS - lufs) * 10.0).round() / 10.0;
    let level = patch(src)?.params.out.volume;
    let line = if src.trim_line > 0 {
        src.trim_line
    } else {
        src.id_line
    };
    trimmed(level, t).map_err(|m| format!("{}:{line}: {} {m}", src.file, src.name.as_str()))?;
    Ok(t + 0.0) // never -0.0
}

/// The level render's LUFS (`fmt --level` centres it).
pub fn level_lufs(src: &Source) -> Result<f32, String> {
    let snd = dried(&to_sound(src)?);
    let level = Plan {
        hits: vec![hit(0.0, LEVEL_HOLD_S, src.home, 100)],
        secs: LEVEL_HOLD_S + 0.05,
        mode: None,
    };
    let t = play(&snd, src.demo, &level);
    let held = (LEVEL_HOLD_S * SR) as usize;
    Ok(listen::lufs(&t.stereo[2 * LATE..2 * (LATE + held)]))
}

/// The ledger folder, refused under `/tmp` (the scratchpad included) or
/// the system temp dir, or when not absolute or holding `..`: renders and
/// verdicts are durable (durable ledgers rule).
pub fn checked_ledger(p: &Path) -> Result<PathBuf, String> {
    let bad = |why: &str| Err(format!("ledger {}: {why}", p.display()));
    if !p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
        return bad("must be an absolute path without ..");
    }
    let tmp = [PathBuf::from("/tmp"), std::env::temp_dir()];
    if tmp.iter().any(|t| p.starts_with(t)) {
        return bad("never /tmp or the scratchpad");
    }
    Ok(p.to_path_buf())
}

const DEFAULT_LEDGER: &str = "~/dev/chimera/.superpowers/ledgers/showcase";

/// `$CHIMERA_SHOWCASE_LEDGER`, else the showcase ledger, checked.
pub fn ledger() -> Result<PathBuf, String> {
    let p = std::env::var_os("CHIMERA_SHOWCASE_LEDGER")
        .map_or_else(|| PathBuf::from(DEFAULT_LEDGER), PathBuf::from);
    checked_ledger(&p)
}

/// `report.txt`: each Sound's gates, the samey pairs, and the verdict.
pub fn report_text(reports: &[Report], samey: &[(usize, usize)]) -> String {
    let mut o = String::new();
    for r in reports {
        let ok = if r.passes() { "PASS" } else { "FAIL" };
        let private = if r.private { "  PRIVATE" } else { "" };
        let _ = writeln!(o, "S{} {}{private}  {ok}", r.id.get(), r.name);
        for g in &r.gates {
            let v = match &g.verdict {
                Verdict::Pass => "PASS".to_string(),
                Verdict::Fail => "FAIL".to_string(),
                Verdict::Exempt(why) => format!("EXEMPT ({why})"),
            };
            // A tail at the dB floor is silence, not a number.
            let value = if g.check == Check::Tail && g.value <= SILENT_FLOOR_DB {
                "silent".to_string()
            } else {
                format!("{:.2}", g.value)
            };
            let _ = writeln!(
                o,
                "  {:<18} {value:>8} {:<6} {v}",
                g.check.label(),
                g.check.unit()
            );
        }
    }
    for &(i, j) in samey {
        let _ = writeln!(
            o,
            "samey: {} and {}, within {SAMEY_DB} dB on all {ERB_BANDS} ERB bands: by ear",
            reports[i].name, reports[j].name
        );
    }
    let pass = reports.iter().all(Report::passes);
    let _ = writeln!(
        o,
        "{} Sounds: {}",
        reports.len(),
        if pass { "all pass" } else { "FAIL" }
    );
    o
}

/// The owner's page: every Sound that passes, badged APPROVED or NEW,
/// then each placeholder; a Sound a gate failed is named in a flagged
/// OFF THE PAGE row. Per Sound its name, description, demo mode,
/// LUFS, `edits`, exemptions and samey pairs to judge by ear, what it is
/// (`about`), and a player per render with its LUFS and peak.
pub fn page(reports: &[Report], samey: &[(usize, usize)], placeholders: &[Source]) -> String {
    let mut sounds: Vec<(Badge, Row)> = reports
        .iter()
        .enumerate()
        .filter(|(_, r)| r.passes())
        .map(|(i, r)| {
            let mut lines = vec![(r.note.clone(), false)];
            let lufs = r.lufs().map_or(String::new(), |l| {
                format!(" · level (1 s, dry) {l:.1} LUFS")
            });
            let long = r
                .long_lufs
                .map_or(String::new(), |l| format!(" · long (dry) {l:.1} LUFS"));
            lines.push((
                format!("S{} · {}{lufs}{long}", r.id.get(), r.demo.disk_ident()),
                false,
            ));
            if let Some(e) = &r.edits {
                lines.push((format!("edits: {e}"), false));
            }
            for g in &r.gates {
                if let Verdict::Exempt(why) = &g.verdict {
                    lines.push((format!("{}: {why}", g.check.label()), true));
                }
            }
            for &(a, b) in samey.iter().filter(|p| p.0 == i || p.1 == i) {
                let other = &reports[if a == i { b } else { a }].name;
                lines.push((format!("samey with {other}: by ear"), true));
            }
            let row = Row {
                title: r.name.clone(),
                badge: Some(r.badge.word()),
                lines,
                facts: r.about.clone(),
                players: r
                    .renders
                    .iter()
                    .map(|x| {
                        let bar = x.audition.bar();
                        let over = x.limited_db.is_some_and(|d| d > bar);
                        let limited = x.limited_db.map_or(String::new(), |d| {
                            let past = if over {
                                format!(", over the {bar} dB bar")
                            } else {
                                String::new()
                            };
                            format!(" · limiter {d:.1} dB{past}")
                        });
                        Player {
                            label: std::iter::once(x.audition.label())
                                .chain(x.with.iter().map(|w| format!("WITH {w}")))
                                .collect::<Vec<_>>()
                                .join(" "),
                            file: r.wav(x.audition),
                            info: format!(
                                "{:.1} LUFS · peak L {:.1} · R {:.1} dBFS{limited}",
                                x.lufs, x.peak_dbfs[0], x.peak_dbfs[1]
                            ),
                            flag: over,
                        }
                    })
                    .collect(),
                private: r.private,
            };
            (r.badge, row)
        })
        .collect();
    sounds.extend(placeholders.iter().map(|s| {
        let row = Row {
            title: s.name.as_str().into(),
            badge: Some(Badge::Placeholder.word()),
            lines: vec![
                (s.note.as_str().into(), false),
                (
                    format!("S{}: waits on the formant engine", s.id.get()),
                    false,
                ),
            ],
            facts: Vec::new(),
            players: Vec::new(),
            private: false,
        };
        (Badge::Placeholder, row)
    }));
    // APPROVED, then NEW, then the placeholders; Source order within each.
    sounds.sort_by_key(|(b, _)| *b);
    let mut rows: Vec<Row> = sounds.into_iter().map(|(_, r)| r).collect();
    let off: Vec<(String, bool)> = reports
        .iter()
        .filter(|r| !r.passes())
        .map(|r| {
            let l = format!(
                "Off the page (a gate failed, see report.txt): S{} {}",
                r.id.get(),
                r.name
            );
            (l, true)
        })
        .collect();
    if !off.is_empty() {
        rows.insert(
            0,
            Row {
                title: "OFF THE PAGE".into(),
                badge: None,
                lines: off,
                facts: Vec::new(),
                players: Vec::new(),
                private: false,
            },
        );
    }
    listen::rows_page(
        "Factory Sounds · listening",
        "Every Sound that is ready: APPROVED (passed your listening round), NEW (not yet heard), \
         and the PLACEHOLDERS still to come. Each plays over a four-bar phrase (BASS, LEAD, PAD, \
         ARP or WONKY, on its demo mode), then under its render scripts, offline through the real \
         Instrument (Part 1, P1); a render marked WITH plays its sends through NEW's effects. The \
         phrase is for your ear, not a gate. Under each Sound, what it is, read from the Sound \
         itself. Every gate passed (measured dry) or is marked for your ear: the lines in colour.",
        &rows,
    )
}

/// Into `dir` (checked as a ledger): `report.txt`, naming each Sound a
/// failing gate keeps off the page; with `wav`, each passing Sound's
/// renders as `renders/S<id>-<script>.wav` (a failing one's old renders
/// removed) and `listening.html`. A failing run without `wav` removes an
/// old page. True when every Sound passes.
pub fn write(
    dir: &Path,
    reports: &[Report],
    samey: &[(usize, usize)],
    placeholders: &[Source],
    wav: bool,
) -> Result<bool, String> {
    let dir = checked_ledger(dir)?;
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(&dir).map_err(|e| err(&dir, e))?;
    let pass = reports.iter().all(Report::passes);
    let mut txt = report_text(reports, samey);
    for r in reports.iter().filter(|r| !r.passes()) {
        let _ = writeln!(
            txt,
            "off the page: S{} {} (a gate failed)",
            r.id.get(),
            r.name
        );
    }
    let path = dir.join("report.txt");
    std::fs::write(&path, txt).map_err(|e| err(&path, e))?;
    let html = dir.join("listening.html");
    if wav {
        let r = dir.join("renders");
        std::fs::create_dir_all(&r).map_err(|e| err(&r, e))?;
        for rep in reports {
            if rep.passes() {
                for x in &rep.renders {
                    listen::write_wav_stereo(&dir.join(rep.wav(x.audition)), &x.stereo)?;
                }
                continue;
            }
            // A Sound off the page leaves no old render to pass as current.
            let stale = format!("S{}-", rep.id.get());
            for e in std::fs::read_dir(&r).map_err(|e| err(&r, e))? {
                let p = e.map_err(|e| err(&r, e))?.path();
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&stale) && n.ends_with(".wav"))
                {
                    std::fs::remove_file(&p).map_err(|e| err(&p, e))?;
                }
            }
        }
        std::fs::write(&html, page(reports, samey, placeholders)).map_err(|e| err(&html, e))?;
    } else if !pass {
        // An old page could show a now failing Sound as passed.
        match std::fs::remove_file(&html) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(&html, e)),
            _ => {}
        }
    }
    Ok(pass)
}
