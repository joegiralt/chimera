//! QA before UAT (spec § 4): each factory Sound played headlessly through
//! the real `Instrument` (as `loudness_test` plays a scene: Part 1, P1,
//! after the limiter), measured dry (its sends at 0) by pure gates over
//! the sample buffers; the listening renders carry its sends through the
//! FX bus at NEW's settings, the first of them a phrase (a
//! listening render, never a gate). Then `report.txt` and the owner's
//! `listening.html` with the renders, all in the ledger folder. `write`
//! is the only I/O.

use std::fmt::Write as _;
use std::path::{Component, Path, PathBuf};

use chimera_core::block::DiskCode;
use chimera_core::dsp::fx_bus::{FX_SENDS, FxBus, FxParams};
use chimera_core::dsp::limiter::OUTPUT_GAIN;
use chimera_core::dsp::modulator::law::{B_TIME, speed_ranges};
use chimera_core::dsp::modulator::{EnvSlot, EnvType};
use chimera_core::dsp::rev::sync::Tempo;
use chimera_core::hw::MAX_PARTS;
use chimera_core::hw::SampleBudget;
use chimera_core::instrument::{AudioShared, DacBlocks, Instrument, MixState, mix_parts};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::note_queue::{NoteEvent, NoteKind};
use chimera_core::params::EngineType;
use chimera_core::part::PartMode;
use chimera_core::preset::Sound;
use chimera_core::project::{PartId, Performance};
use chimera_core::scope::{ScopeWriter, scope_buffer};
use chimera_core::source_table::GateView;
use chimera_core::storage::FactoryId;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use super::about::{self, Fact};
use super::project::{self, Demo, DemoId, Setting};
use super::source::{self, Badge, Phrase, Round, Script, Source, patch, to_sound, trimmed};
use crate::listen::{self, Player, Row, Section, qa_takes};

const SR: f32 = SAMPLE_RATE as f32;

pub use super::source::{LOUDNESS_LU, REFERENCE_LUFS};
/// How far a level pinned by ear may move.
pub const PIN_LU: f32 = 1.0;
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
/// A phrase's length, in sixteenths: four bars of 4/4 (`sweep`'s twelve).
pub const PHRASE_STEPS: u8 = 64;

/// The bars `p` spans, in sixteenths.
pub const fn phrase_steps(p: Phrase) -> u8 {
    match p {
        Phrase::Sweep => SWEEP_STEPS,
        _ => PHRASE_STEPS,
    }
}

/// When `p`'s last note ends, s.
pub fn phrase_secs(p: Phrase) -> f32 {
    let end = phrase_notes(p)
        .iter()
        .map(|x| x.at + x.len)
        .max()
        .unwrap_or(0);
    end as f32 * SIXTEENTH_S
}

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

/// `sweep`'s length, in sixteenths: twelve bars, 24 s at 120 BPM.
pub const SWEEP_STEPS: u8 = 192;
/// The slowest swell `sweeps` takes: an ENV's rise, s.
pub const SWEEP_MIN_RISE_S: f32 = 1.0;
/// The fastest LFO a `sweep` Sound may swell with (owner: a very slow
/// sweep, 0.05 to 0.1 Hz).
pub const SWEEP_MAX_HZ: f32 = 0.1;
/// The least route depth that is heard as a swell (±16 ≈ 1.3 octaves).
pub const SWEEP_MIN_DEPTH: u8 = 16;

/// In the manner of Radiohead – "Everything In Its Right Place" (harmony
/// and contour only, not transcribed), written fresh: three open-voiced
/// chords in a minor key, i7 – ♭VImaj7 – ♭VII7 (from C: Cm7, A♭maj7,
/// B♭7), each struck whole and held four bars
/// (7.9 s) over its root, so a slow envelope on the cutoff swells each one
/// open and closed. Every voice of a chord starts its envelope together:
/// honest as played, whatever the voices did before. Common tones (the
/// fifth and the minor third above the octave) are re-struck at each change.
pub const SWEEP_PHRASE: [PhraseNote; 15] = [
    // i7
    n(-12, 0, 63, 96),
    n(0, 0, 63, 92),
    n(7, 0, 63, 88),
    n(10, 0, 63, 90),
    n(15, 0, 63, 94),
    // ♭VImaj7, softer
    n(-16, 64, 63, 84),
    n(-4, 64, 63, 80),
    n(7, 64, 63, 76),
    n(12, 64, 63, 78),
    n(15, 64, 63, 82),
    // ♭VII7, the loudest: velocity opens it further
    n(-14, 128, 63, 112),
    n(-2, 128, 63, 108),
    n(5, 128, 63, 104),
    n(8, 128, 63, 106),
    n(14, 128, 63, 110),
];

/// How a `sweep` Sound swells its filter (`sweep`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Swell {
    /// A SLOW envelope: its rise and its fall, s.
    Env { rise_s: f32, fall_s: f32 },
    /// A free-running LFO, Hz.
    Lfo { hz: f32 },
}

/// Every built `sweep` Sound's swell (`sweep`).
pub fn sweeps(all: &[Source]) -> Result<(), String> {
    all.iter()
        .filter(|s| !s.is_placeholder() && s.phrase == Some(Phrase::Sweep))
        .try_for_each(|s| sweep(s).map(|_| ()))
}

/// The swell a `sweep` Source plays its held chords through, on
/// FILTER.CUTOFF at ±`SWEEP_MIN_DEPTH` or more: either ENV 2 or 3, type A at
/// SLOW, rising `SWEEP_MIN_RISE_S` or longer, sustaining at 50 % or under,
/// its rise and fall spanning half to all of the shortest chord; or a SINE
/// or TRIANGLE, CLASSIC, FREE LFO at `SWEEP_MAX_HZ` or slower whose cycle
/// the phrase holds, with an ENV 1 release that bridges the phrase's
/// longest gap (so a re-struck voice keeps its LFO). Else the id line fails.
pub fn sweep(s: &Source) -> Result<Swell, String> {
    use chimera_core::dsp::lfo::LfoShape;
    use chimera_core::dsp::modulator::{EnvSpeed, LfoType};
    let snd = patch(s)?;
    let notes = phrase_notes(Phrase::Sweep);
    let secs = |steps: u8| steps as f32 * SIXTEENTH_S;
    let chord = secs(notes.iter().map(|x| x.len).min().unwrap_or(0));
    let played = phrase_secs(Phrase::Sweep);
    // A pitch's longest wait from its note-off to its next strike.
    let gap = notes
        .iter()
        .filter_map(|x| {
            notes
                .iter()
                .find(|y| y.step == x.step && y.at > x.at)
                .map(|y| secs(y.at.saturating_sub(x.at + x.len)))
        })
        .fold(0.0, f32::max);
    let env1 = &snd.params.envelopes[EnvSlot::Env1.index()];
    let release = match env1.env_type {
        EnvType::A => speed_ranges(env1.speed).dec_rel.at(env1.release),
        EnvType::B => B_TIME.at(env1.func.fall),
    };
    let mut why = Vec::new();
    for (src, rs) in &s.mods {
        let Some(amt) = rs
            .iter()
            .find(|r| r.dest == FixedCol::Freq.addr())
            .map(|r| r.amount)
        else {
            continue;
        };
        let name = src.name();
        if amt.unsigned_abs() < SWEEP_MIN_DEPTH {
            why.push(format!("{name}: depth {amt:+} under ±{SWEEP_MIN_DEPTH}"));
            continue;
        }
        let lfo = match src {
            ModSource::Env2 | ModSource::Env3 => {
                let slot = src.env_slot().expect("an ENV source");
                let e = &snd.params.envelopes[slot.index()];
                if e.env_type != EnvType::A || e.speed != EnvSpeed::Slow {
                    why.push(format!("{name}: not a SLOW type A envelope"));
                    continue;
                }
                let r = speed_ranges(e.speed);
                let (rise_s, fall_s) = (r.attack.at(e.attack), r.dec_rel.at(e.decay));
                if rise_s < SWEEP_MIN_RISE_S {
                    why.push(format!("{name}: rises in {rise_s:.1} s"));
                } else if e.sustain > 0.5 {
                    why.push(format!("{name}: sustains at {:.0} %", e.sustain * 100.0));
                } else if !(chord / 2.0..=chord).contains(&(rise_s + fall_s)) {
                    why.push(format!(
                        "{name}: rise and fall {:.1} s, not within the {chord:.1} s chord's half to whole",
                        rise_s + fall_s
                    ));
                } else {
                    return Ok(Swell::Env { rise_s, fall_s });
                }
                continue;
            }
            ModSource::Lfo1 => 0,
            ModSource::Lfo2 => 1,
            ModSource::Lfo3 => 2,
            _ => continue,
        };
        let l = &snd.params.lfos[lfo];
        let smooth = matches!(
            LfoShape::from_u8(l.shape),
            LfoShape::Sine | LfoShape::Triangle
        );
        if l.lfo_type != LfoType::Classic || l.sync != 0 || !smooth {
            why.push(format!("{name}: not a FREE SINE or TRIANGLE"));
        } else if l.rate > SWEEP_MAX_HZ {
            why.push(format!(
                "{name}: {} Hz is faster than {SWEEP_MAX_HZ} Hz",
                l.rate
            ));
        } else if 1.0 / l.rate > played {
            why.push(format!(
                "{name}: its cycle ({:.1} s) is longer than the phrase ({played} s)",
                1.0 / l.rate
            ));
        } else if release < gap {
            why.push(format!(
                "{name}: ENV 1's release ({release:.2} s) does not bridge the phrase's {gap} s gap"
            ));
        } else {
            return Ok(Swell::Lfo { hz: l.rate });
        }
    }
    Err(format!(
        "{}: a sweep Sound needs a slow swell on FILTER.CUTOFF (a SLOW ENV 2 or 3 over each chord, \
         or a FREE SINE or TRIANGLE LFO at {SWEEP_MAX_HZ} Hz or slower){}{}",
        s.at(s.id_line),
        if why.is_empty() { "" } else { ": " },
        why.join("; ")
    ))
}

/// A phrase's notes.
pub const fn phrase_notes(p: Phrase) -> &'static [PhraseNote] {
    match p {
        Phrase::Bass => &BASS_PHRASE,
        Phrase::Lead => &LEAD_PHRASE,
        Phrase::Pad => &PAD_PHRASE,
        Phrase::Arp => &ARP_PHRASE,
        Phrase::Wonky => &WONKY_PHRASE,
        Phrase::Sweep => &SWEEP_PHRASE,
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
    let channel = perf.part(PartId::ALL[0]).mix.channel;
    play_on(&perf, channel, plan)
}

/// `plan` played on `channel` of `perf`, every Part and the FX bus as
/// set: the Parts on that channel sound.
fn play_on(perf: &Performance, channel: MidiChannel, plan: &Plan) -> Take {
    let shared = Box::new(AudioShared::from_performance(perf));
    let mut inst = Box::new(Instrument::new(
        SAMPLE_RATE,
        SampleBudget::for_cpu(u32::MAX),
    ));
    let mut fx = Box::new(FxBus::new());
    let ev = |note: u8, kind| NoteEvent {
        channel,
        note: MidiNote::new(note).expect("a MIDI note"),
        kind,
    };
    SCOPE.with_borrow_mut(|scope| {
        metered(block(plan.secs), |b, dac| {
            for h in plan.hits.iter().filter(|h| block(h.off) == b) {
                inst.handle_with(ev(h.note, NoteKind::Off), &shared, &GateView::empty());
            }
            for h in plan.hits.iter().filter(|h| block(h.on) == b) {
                let v = Velocity::new(h.vel).expect("a velocity");
                inst.handle_with(ev(h.note, NoteKind::On(v)), &shared, &GateView::empty());
            }
            inst.render_with(&mut fx, dac, &shared, scope, &GateView::empty());
        })
    })
}

/// `blocks` blocks, each rendered into the DAC by `render`, kept as P1
/// after the limiter, with the most gain reduction it took.
fn metered(blocks: usize, mut render: impl FnMut(usize, &mut DacBlocks)) -> Take {
    let mut dac = Box::new(DacBlocks::new());
    let energy = |p: &[f32]| p.iter().map(|v| v * v).sum::<f32>();
    let mut t = Take {
        stereo: Vec::new(),
        reduced_db: 0.0,
    };
    let mut prev_in = 0.0f32;
    for b in 0..blocks {
        render(b, &mut dac);
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

/// The home note at v100 held 1 s, LUFS: −11 ± 2 LU passes; a level
/// pinned by ear `(lufs, why)` instead holds within `PIN_LU` of its value
/// (marked for the ear), and fails once it moves, even into the band.
pub fn loudness(lufs: f32, pin: Option<(f32, &str)>) -> Verdict {
    match pin {
        None => verdict((lufs - REFERENCE_LUFS).abs() <= LOUDNESS_LU),
        Some((p, why)) if (lufs - p).abs() <= PIN_LU => {
            Verdict::Exempt(format!("level by ear {p:.1}: {why}"))
        }
        Some(_) => Verdict::Fail,
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
/// and sounding. The value is the click ratio. A Sound `exempt` by
/// `listen::click_exempt` is left to the ear for the ratio (Exempt, with
/// its reason), never for being finite and sounding.
pub fn clicks(x: &[f32], onsets: &[usize], exempt: Option<&str>) -> (f32, Verdict) {
    let q = qa_takes(x, onsets);
    let v = match exempt {
        Some(why) if q.sounds() => Verdict::Exempt(why.into()),
        Some(_) => Verdict::Fail,
        None => verdict(q.passes()),
    };
    (q.click_ratio, v)
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

    /// The bar its limiter reading is flagged over: a pad's or sweep's
    /// chords (as `stab`) 3 dB, else 1 dB.
    pub fn bar(self) -> f32 {
        match self {
            Audition::Phrase(Phrase::Pad | Phrase::Sweep) => LIMIT_CHORD_DB,
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
    /// Approved in this round, but the Sound has changed since: NEW again.
    pub changed: Option<Round>,
    /// What it is (`about`), from the built Sound.
    pub about: Vec<Fact>,
    /// An APPROVED Sound's renders against `--baseline` (`compare`).
    pub baseline: Option<Baseline>,
    /// Its routed FREE LFOs (`about::free_lfos`), for a #489 cause.
    pub free_lfos: Vec<String>,
}

/// A render whose residual against its baseline is over this was changed
/// by the engine, not by its Source: re-listen.
pub const AUDIO_CHANGED_DB: f32 = -40.0;
/// The page's line for it; the badge stays.
pub const AUDIO_CHANGED: &str = "AUDIO CHANGED BY ENGINE UPDATE: re-listen";

/// A residual at or under this still nulls: the same waveform, moved
/// (coherent drift), and its size says how far. Near 0 dB the takes no
/// longer null at all (a re-phased LFO decorrelates them), so the size
/// says nothing. Between the two clusters #495 left: -6 to +3 dB, and
/// -12.6 to -39.4 dB. It measures coherent drift, not timbre.
pub const COHERENT_DB: f32 = -11.0;
/// What the page and `report.txt` say the residual measures.
pub const RESIDUAL_NOTE: &str = "AUDIO CHANGED compares each render with the baseline's: its \
     residual measures coherent drift, not timbre (near 0 dB the takes no longer null at all).";
/// The bisect evidence beside the baseline WAVs: `S<id>` and, for each
/// engine change its residual bisects to, `#<issue> <dB>`; `#` comments.
pub const CAUSES: &str = "causes.txt";

/// An engine change a Sound's residual bisects to (`CAUSES`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineChange {
    /// #485: the VA sub flips at the shaped saw's edge.
    SubFlip,
    /// #489: FREE LFOs play their Part's clock (ADR 0135).
    LfoClock,
}

impl EngineChange {
    fn parse(w: &str) -> Option<Self> {
        match w {
            "#485" => Some(Self::SubFlip),
            "#489" => Some(Self::LfoClock),
            _ => None,
        }
    }

    /// Its page line at `db`, naming the Sound's `free_lfos` for #489.
    fn line(self, db: f32, free_lfos: &[String]) -> String {
        match self {
            Self::SubFlip => {
                format!("TIMBRE CHANGED {db:.1} dB: #485 the sub flips at the shaped saw's edge")
            }
            Self::LfoClock => {
                let (lfos, verb) = match free_lfos {
                    [] => ("its LFOs".to_string(), "run"),
                    [one] => (one.clone(), "runs"),
                    many => (many.join(" and "), "run"),
                };
                format!(
                    "LFO MOTION CHANGED {db:.1} dB: #489 FREE {lfos} now {verb} on the Part clock"
                )
            }
        }
    }
}

/// `text` (a `CAUSES` file) for the Sound `id`: each change and its
/// residual. A bad line fails with its number.
pub fn causes(text: &str, id: u32) -> Result<Vec<(EngineChange, f32)>, String> {
    let mut out = Vec::new();
    for (n, l) in text.lines().enumerate() {
        // `#` starts a comment only at the line's start: `#485` is a word.
        if l.trim_start().starts_with('#') {
            continue;
        }
        let words: Vec<&str> = l.split_whitespace().collect();
        let bad = |why: &str| Err(format!("{CAUSES}:{}: {why}", n + 1));
        let Some((first, rest)) = words.split_first() else {
            continue;
        };
        let Some(sid) = first.strip_prefix('S').and_then(|v| v.parse::<u32>().ok()) else {
            return bad("an S<id> first");
        };
        if rest.is_empty() || rest.len() % 2 != 0 {
            return bad("each change with its residual: #<issue> <dB>");
        }
        let mut these = Vec::new();
        for p in rest.chunks(2) {
            let (Some(c), Ok(db)) = (EngineChange::parse(p[0]), p[1].parse::<f32>()) else {
                return bad(&format!("{} {}: not #485 or #489 and a dB", p[0], p[1]));
            };
            these.push((c, db));
        }
        if sid == id {
            out = these;
        }
    }
    Ok(out)
}

/// A Sound's renders against the baseline's.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseline {
    /// The render with the largest residual, and it, dB.
    pub worst: Option<(Audition, f32)>,
    /// Renders with no baseline WAV: not compared.
    pub missing: Vec<Audition>,
    /// What its change bisects to (`CAUSES`), each with its residual.
    pub causes: Vec<(EngineChange, f32)>,
}

impl Baseline {
    /// The worst render, when it is over `AUDIO_CHANGED_DB`.
    pub fn changed(&self) -> Option<(Audition, f32)> {
        self.worst.filter(|&(_, db)| db > AUDIO_CHANGED_DB)
    }

    /// Changed, and some of it still nulls (`COHERENT_DB`): heard first.
    pub fn coherent(&self) -> bool {
        let still = |db: f32| db > AUDIO_CHANGED_DB && db <= COHERENT_DB;
        match (self.changed(), self.causes.as_slice()) {
            (None, _) => false,
            (Some((_, db)), []) => still(db),
            (Some(_), c) => c.iter().any(|&(_, db)| still(db)),
        }
    }

    /// The page's flagged lines: `AUDIO_CHANGED`, then each cause.
    pub fn lines(&self, free_lfos: &[String]) -> Vec<String> {
        if self.changed().is_none() {
            return Vec::new();
        }
        std::iter::once(AUDIO_CHANGED.to_string())
            .chain(self.causes.iter().map(|&(c, db)| c.line(db, free_lfos)))
            .collect()
    }
}

/// `new − old` against `old`, dB (energy over energy: the RMS ratio), the
/// two aligned sample for sample as written and the shorter padded with
/// silence. Identical: −∞.
pub fn residual_db(new: &[f32], old: &[f32]) -> f32 {
    let at = |x: &[f32], i: usize| x.get(i).copied().unwrap_or(0.0) as f64;
    let diff: f64 = (0..new.len().max(old.len()))
        .map(|i| (at(new, i) - at(old, i)).powi(2))
        .sum();
    if diff == 0.0 {
        return f32::NEG_INFINITY;
    }
    let base: f64 = old.iter().map(|&v| (v as f64).powi(2)).sum();
    (10.0 * (diff / base).log10()) as f32
}

/// An APPROVED Sound's renders, as `write` writes them, against the WAVs
/// of the same names in `dir` (`--baseline`), into `r.baseline`; any other
/// Sound is not compared. A missing WAV is noted, not flagged.
pub fn compare(r: &mut Report, dir: &Path) -> Result<(), String> {
    if r.badge != Badge::Approved {
        r.baseline = None;
        return Ok(());
    }
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    let c = dir.join(CAUSES);
    let mut b = Baseline {
        worst: None,
        missing: Vec::new(),
        causes: if c.is_file() {
            causes(&read(&c)?, r.id.get()).map_err(|e| format!("{}/{e}", dir.display()))?
        } else {
            Vec::new()
        },
    };
    let unit = |v: i16| v as f32 / 32_767.0;
    for x in &r.renders {
        let p = dir.join(r.file(x.audition));
        if !p.is_file() {
            b.missing.push(x.audition);
            continue;
        }
        let err = |e: hound::Error| format!("{}: {e}", p.display());
        let w = hound::WavReader::open(&p).map_err(err)?;
        if w.spec().channels != 2 || w.spec().bits_per_sample != 16 {
            return Err(format!("{}: not 16-bit stereo", p.display()));
        }
        let old: Vec<f32> = w
            .into_samples::<i16>()
            .map(|v| v.map(unit))
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let new: Vec<f32> = listen::wav_samples(&x.stereo, 2).map(unit).collect();
        let db = residual_db(&new, &old);
        if b.worst.is_none_or(|(_, w)| db > w) {
            b.worst = Some((x.audition, db));
        }
    }
    r.baseline = Some(b);
    Ok(())
}

impl Report {
    pub fn passes(&self) -> bool {
        self.gates.iter().all(|g| g.verdict != Verdict::Fail)
    }

    fn wav(&self, a: Audition) -> String {
        format!("renders/{}", self.file(a))
    }

    fn file(&self, a: Audition) -> String {
        format!("S{}-{}.wav", self.id.get(), a.slug())
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
    let crc = source::approval_crc(src)?;
    let with = heard_through(&wet);
    let snd = dried(&wet);
    let within = tail_within(src, &snd);
    let exempt = listen::click_exempt(&snd);
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
        (
            lufs,
            loudness(lufs, src.level.as_ref().map(|p| (p.lufs, p.why.as_str()))),
        ),
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
        gate(Check::Clicks(s), clicks(&mono, &onsets, exempt));
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
        badge: src.badge(crc),
        changed: src.changed(crc),
        about: about::of(src)?,
        baseline: None,
        free_lfos: about::free_lfos(&wet),
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

/// `text` with each Source's `trim` centred on −11 LUFS (`fmt --level`):
/// measured, trimmed, measured again (the limiter bends a loud first
/// guess), then `backed_off` while a limiter gate would fail. Only Sounds shown NEW: placeholders, approved Sounds and
/// levels pinned by ear keep theirs.
pub fn levelled(text: &str, all: &[Source]) -> Result<String, String> {
    let mut trims = Vec::new();
    for s in all.iter().filter(|s| s.level.is_none()) {
        if s.badge(source::approval_crc(s)?) != Badge::New {
            continue;
        }
        let mut t = s.clone();
        for _ in 0..2 {
            t.trim = level_trim(&t, level_lufs(&t)?)?;
        }
        t.trim = backed_off(t.trim, |trim| {
            let mut u = t.clone();
            u.trim = trim;
            over_limit(&u)
        })?;
        println!("{} trim {:+.1} dB", s.name.as_str(), t.trim);
        trims.push((s, t.trim));
    }
    Ok(source::set_trims(text, &trims))
}

/// How far `backed_off` may go: 6 dB.
const BACK_OFF_STEPS: u8 = 60;

/// `trim` stepped down 0.1 dB at a time while `over` it (a limiter gate
/// would fail): the smallest cut that passes. Refused past 6 dB.
pub fn backed_off(
    trim: f32,
    mut over: impl FnMut(f32) -> Result<bool, String>,
) -> Result<f32, String> {
    let tenths = (trim * 10.0).round();
    for k in 0..=BACK_OFF_STEPS {
        let t = (tenths - k as f32) / 10.0 + 0.0;
        if !over(t)? {
            return Ok(t);
        }
    }
    Err(format!(
        "trim {trim:+.1}: still over the limiter's bar {} dB down",
        BACK_OFF_STEPS / 10
    ))
}

/// Some limiter gate on `src` fails: the level render's, or a script's
/// (`gates`, dry).
pub fn over_limit(src: &Source) -> Result<bool, String> {
    let snd = dried(&to_sound(src)?);
    let level = Plan {
        hits: vec![hit(0.0, LEVEL_HOLD_S, src.home, 100)],
        secs: LEVEL_HOLD_S + 0.05,
        mode: None,
    };
    let mut takes = vec![(None, level)];
    takes.extend(src.plays.iter().map(|&s| (Some(s), plan(s, src.home))));
    Ok(takes
        .iter()
        .any(|(s, p)| limiter(play(&snd, src.demo, p).reduced_db, *s) == Verdict::Fail))
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
        if let Some(round) = r.changed {
            let _ = writeln!(o, "  {}", changed_line(round));
        }
        if let Some(b) = &r.baseline {
            match (b.changed(), b.worst) {
                (Some((a, db)), _) => {
                    let _ = writeln!(o, "  {AUDIO_CHANGED} (worst {db:.1} dB: {})", a.slug());
                    for l in b.lines(&r.free_lfos).iter().skip(1) {
                        let _ = writeln!(o, "  {l}");
                    }
                }
                (None, Some((a, db))) if db.is_finite() => {
                    let _ = writeln!(o, "  baseline: worst residual {db:.1} dB: {}", a.slug());
                }
                (None, Some(_)) => {
                    let _ = writeln!(o, "  baseline: identical");
                }
                (None, None) => {}
            }
            for &a in &b.missing {
                let _ = writeln!(o, "  baseline: no {}, not compared", r.file(a));
            }
        }
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
    for r in reports {
        if let Some((a, db)) = r.baseline.as_ref().and_then(Baseline::changed) {
            let _ = writeln!(
                o,
                "audio changed by engine update: S{} {}, worst {db:.1} dB: {}",
                r.id.get(),
                r.name,
                a.slug()
            );
        }
    }
    if any_changed(reports) {
        let _ = writeln!(o, "{RESIDUAL_NOTE}");
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

/// Some Sound's audio changed since the baseline.
fn any_changed(reports: &[Report]) -> bool {
    reports
        .iter()
        .any(|r| r.baseline.as_ref().and_then(Baseline::changed).is_some())
}

/// An approved Sound whose CRC no longer matches.
fn changed_line(round: Round) -> String {
    format!("CHANGED SINCE {round}: re-listen")
}

/// The owner's page: every Sound that passes, badged APPROVED or NEW,
/// then each placeholder, then each passing demo project under DEMO
/// PROJECTS; a Sound or demo a gate failed is named in a flagged OFF THE
/// PAGE row. Per Sound its name, description, demo mode,
/// LUFS, `edits`, exemptions and samey pairs to judge by ear, what it is
/// (`about`), and a player per render with its LUFS and peak.
pub fn page(
    reports: &[Report],
    samey: &[(usize, usize)],
    placeholders: &[Source],
    demos: &[DemoReport],
) -> String {
    let mut sounds: Vec<((bool, Badge), Row)> = reports
        .iter()
        .enumerate()
        .filter(|(_, r)| r.passes())
        .map(|(i, r)| {
            let mut lines = vec![(r.note.clone(), false)];
            if let Some(round) = r.changed {
                lines.push((changed_line(round), true));
            }
            if let Some(b) = &r.baseline {
                lines.extend(b.lines(&r.free_lfos).into_iter().map(|l| (l, true)));
            }
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
                section: if r.private {
                    Section::Private
                } else {
                    Section::Sounds
                },
            };
            let coherent = r.baseline.as_ref().is_some_and(Baseline::coherent);
            ((!coherent, r.badge), row)
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
            section: Section::Sounds,
        };
        ((true, Badge::Placeholder), row)
    }));
    // A coherent engine change first (heard first), then APPROVED, NEW and
    // the placeholders; Source order within each.
    sounds.sort_by_key(|(b, _)| *b);
    let mut rows: Vec<Row> = sounds.into_iter().map(|(_, r)| r).collect();
    rows.extend(demos.iter().filter(|d| d.passes()).map(demo_row));
    let off_line = |what: String| {
        (
            format!("Off the page (a gate failed, see report.txt): {what}"),
            true,
        )
    };
    let off: Vec<(String, bool)> = reports
        .iter()
        .filter(|r| !r.passes())
        .map(|r| off_line(format!("S{} {}", r.id.get(), r.name)))
        .chain(
            demos
                .iter()
                .filter(|d| !d.passes())
                .map(|d| off_line(format!("{} {}", d.id.stem(), d.name))),
        )
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
                section: Section::Sounds,
            },
        );
    }
    let intro = format!(
        "Every Sound that is ready: APPROVED (passed your listening round), NEW (not yet heard), \
         and the PLACEHOLDERS still to come. Each plays over a phrase (BASS, LEAD, PAD, ARP or \
         WONKY, four bars; SWEEP, twelve, three held chords each swelling the filter open and \
         closed; on its demo mode), then under its render scripts, offline through the real \
         Instrument (Part 1, P1); a render marked WITH plays its sends through NEW's effects. The \
         phrase is for your ear, not a gate. Under each Sound, what it is, read from the Sound \
         itself. Every gate passed (measured dry) or is marked for your ear: the lines in colour.{}{}",
        if demos.is_empty() {
            ""
        } else {
            " Under DEMO PROJECTS, each effects demo: every Part's STAB and SHORT on its \
             channel, then the HATS and SNARE stimuli, through the project's own effects and \
             each setting its \"what to try\" invites, offline through the real Instrument and \
             mixer."
        },
        if any_changed(reports) {
            format!(" {RESIDUAL_NOTE}")
        } else {
            String::new()
        }
    );
    listen::rows_page("Factory Sounds · listening", &intro, &rows)
}

/// Into `dir` (checked as a ledger): `report.txt`, naming each Sound or
/// demo a failing gate keeps off the page; with `wav`, each passing
/// Sound's renders as `renders/S<id>-<script>.wav` and each passing
/// demo's as `renders/P<id>-<setting>-<take>.wav` (a failing one's old
/// renders removed), and `listening.html`. A failing run without `wav`
/// removes an old page. True when every Sound and demo passes.
pub fn write(
    dir: &Path,
    reports: &[Report],
    samey: &[(usize, usize)],
    placeholders: &[Source],
    demos: &[DemoReport],
    wav: bool,
) -> Result<bool, String> {
    let dir = checked_ledger(dir)?;
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    std::fs::create_dir_all(&dir).map_err(|e| err(&dir, e))?;
    let pass = reports.iter().all(Report::passes) && demos.iter().all(DemoReport::passes);
    let mut txt = report_text(reports, samey);
    txt.push_str(&demo_text(demos));
    for r in reports.iter().filter(|r| !r.passes()) {
        let _ = writeln!(
            txt,
            "off the page: S{} {} (a gate failed)",
            r.id.get(),
            r.name
        );
    }
    for d in demos.iter().filter(|d| !d.passes()) {
        let _ = writeln!(
            txt,
            "off the page: {} {} (a gate failed)",
            d.id.stem(),
            d.name
        );
    }
    let path = dir.join("report.txt");
    std::fs::write(&path, txt).map_err(|e| err(&path, e))?;
    let html = dir.join("listening.html");
    if wav {
        let r = dir.join("renders");
        std::fs::create_dir_all(&r).map_err(|e| err(&r, e))?;
        // A Sound or demo off the page leaves no old render to pass as
        // current.
        let unrender = |stale: String| -> Result<(), String> {
            for e in std::fs::read_dir(&r).map_err(|e| err(&r, e))? {
                let p = e.map_err(|e| err(&r, e))?.path();
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&stale) && n.ends_with(".wav"))
                {
                    std::fs::remove_file(&p).map_err(|e| err(&p, e))?;
                }
            }
            Ok(())
        };
        for rep in reports {
            if rep.passes() {
                for x in &rep.renders {
                    listen::write_wav_stereo(&dir.join(rep.wav(x.audition)), &x.stereo)?;
                }
            } else {
                unrender(format!("S{}-", rep.id.get()))?;
            }
        }
        for d in demos {
            if d.passes() {
                for x in &d.renders {
                    listen::write_wav_stereo(&r.join(d.file(x)), &x.stereo)?;
                }
            } else {
                unrender(format!("{}-", d.id.stem()))?;
            }
        }
        std::fs::write(&html, page(reports, samey, placeholders, demos))
            .map_err(|e| err(&html, e))?;
    } else if !pass {
        // An old page could show a now failing Sound as passed.
        match std::fs::remove_file(&html) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(err(&html, e)),
            _ => {}
        }
    }
    Ok(pass)
}

// ── the demo projects' renders (spec § 4, § 5) ──

/// What a demo render plays: a Part's script on its channel, or a
/// listening stimulus into the FX bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoTake {
    Part(PartId, Script),
    Hats,
    Snare,
}

impl DemoTake {
    /// Its WAV name's end: `part1-stab`, `hats`.
    fn slug(self) -> String {
        match self {
            DemoTake::Part(p, s) => format!("part{}-{}", p.index() + 1, s.word()),
            DemoTake::Hats => "hats".into(),
            DemoTake::Snare => "snare".into(),
        }
    }

    fn label(self) -> String {
        match self {
            DemoTake::Part(p, s) => format!("PART {} {}", p.index() + 1, s.word().to_uppercase()),
            DemoTake::Hats => "HATS".into(),
            DemoTake::Snare => "SNARE".into(),
        }
    }
}

/// What a demo gate checked, on which take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DemoCheck {
    /// The count of samples that are not finite.
    Finite(DemoTake),
    Clicks(DemoTake),
    Limiter(DemoTake),
}

impl DemoCheck {
    fn label(self) -> String {
        let (what, t) = match self {
            DemoCheck::Finite(t) => ("finite", t),
            DemoCheck::Clicks(t) => ("clicks", t),
            DemoCheck::Limiter(t) => ("limiter", t),
        };
        format!("{what} {}", t.label().to_lowercase())
    }

    fn unit(self) -> &'static str {
        match self {
            DemoCheck::Finite(_) => "bad",
            DemoCheck::Clicks(_) => "x",
            DemoCheck::Limiter(_) => "dB GR",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DemoGate {
    /// The setting (`project::settings`) it played under.
    pub setting: usize,
    pub check: DemoCheck,
    pub value: f32,
    pub verdict: Verdict,
}

/// `chimera-utils stim`'s hats and snare, mono at 48 kHz.
#[derive(Clone, Debug)]
pub struct Stims {
    pub hats: Vec<f32>,
    pub snare: Vec<f32>,
}

/// One demo render, P1 as heard, interleaved stereo.
#[derive(Clone, Debug)]
pub struct DemoRender {
    pub setting: usize,
    pub take: DemoTake,
    pub stereo: Vec<f32>,
    pub lufs: f32,
    pub peak_dbfs: [f32; 2],
    pub limited_db: f32,
}

/// A demo project's QA: what it holds, its settings, gates and renders.
#[derive(Clone, Debug)]
pub struct DemoReport {
    pub id: DemoId,
    pub name: String,
    pub tries: String,
    /// `PART 1 · NYLON (APPROVED) · POLY · CH 1`.
    pub parts: Vec<String>,
    /// Each setting's effects heard, in full words.
    pub settings: Vec<String>,
    pub gates: Vec<DemoGate>,
    pub renders: Vec<DemoRender>,
}

impl DemoReport {
    pub fn passes(&self) -> bool {
        self.gates.iter().all(|g| g.verdict != Verdict::Fail)
    }

    fn file(&self, r: &DemoRender) -> String {
        format!("{}-{}-{}.wav", self.id.stem(), r.setting + 1, r.take.slug())
    }
}

/// A demo render's room past its last note, for a long reverb's tail.
const DEMO_TAIL_S: f32 = 4.0;

/// Each setting plays every Part's `stab` and `short`, then the hats and
/// the snare.
pub fn demo_takes(d: &Demo) -> Vec<DemoTake> {
    d.parts
        .iter()
        .flat_map(|p| [Script::Stab, Script::Short].map(|s| DemoTake::Part(p.part, s)))
        .chain([DemoTake::Hats, DemoTake::Snare])
        .collect()
}

/// A setting's effects, as the page names them.
fn heard_words(s: &Setting) -> String {
    if s.heard.is_empty() {
        return "no effect heard".into();
    }
    s.heard
        .iter()
        .map(|e| e.label())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// Where a stimulus starts sounding: after 10 ms or more under −80 dBFS.
fn onsets(x: &[f32]) -> Vec<usize> {
    let gap = (0.01 * SR) as usize;
    let mut quiet = gap;
    let mut out = Vec::new();
    for (i, v) in x.iter().enumerate() {
        if v.abs() < 1e-4 {
            quiet += 1;
            continue;
        }
        if quiet >= gap {
            out.push(i);
        }
        quiet = 0;
    }
    out
}

/// `x` straight onto Part 1's bus through the mixer (`mix_parts`): its
/// pair, its `sends` into the FX bus as `perf` sets it, then the master
/// section and the limiter. Block 0 is silent, so every gain lands first.
fn play_stim(fx_params: &FxParams, sends: [f32; FX_SENDS], x: &[f32], secs: f32) -> Take {
    let mut perf = Performance::new();
    perf.fx = *fx_params;
    let mut snd = Sound::neutral(EngineType::Va);
    snd.params.out.sends = sends;
    **perf.edit(PartId::ALL[0]).sound = snd;
    let shared = Box::new(AudioShared::from_performance(&perf));
    let mut fx = Box::new(FxBus::new());
    let mut mix = MixState::default();
    let mut buses = Box::new([[0.0f32; BLOCK_SIZE]; MAX_PARTS]);
    let mut sends_buf = [[0.0f32; BLOCK_SIZE]; FX_SENDS];
    metered(block(secs) + 1, |b, dac| {
        let from = b.saturating_sub(1) * BLOCK_SIZE;
        let mut written = [false; MAX_PARTS];
        written[0] = b > 0;
        buses[0] = core::array::from_fn(|i| {
            if b > 0 {
                x.get(from + i).copied().unwrap_or(0.0)
            } else {
                0.0
            }
        });
        mix_parts(
            &buses,
            &written,
            &mut sends_buf,
            &mut mix,
            &mut fx,
            &shared,
            Tempo::DEFAULT,
            SAMPLE_RATE,
            dac,
        );
    })
}

/// `x` without its trailing silence.
fn sounding(x: &[f32]) -> &[f32] {
    let end = x.iter().rposition(|v| *v != 0.0).map_or(0, |i| i + 1);
    &x[..end]
}

/// Every setting of `d` (`project::settings`) played: each Part's `stab`
/// and `short` on its channel through the project as built, then the hats
/// and the snare into the same FX bus at the Parts' most send per effect.
/// Gates: finite, clicks (left to the ear under REVERSE and GATE,
/// `Setting::click_exempt`), and the limiter at the chord bar (3 dB).
pub fn demo_gates(d: &Demo, sounds: &[Source], stims: &Stims) -> Result<DemoReport, String> {
    let project = project::build(d, sounds)?;
    let settings = project::settings(d, sounds)?;
    let mut sends = [0.0f32; FX_SENDS];
    let mut parts = Vec::new();
    let mut exempt = Vec::new();
    for p in &d.parts {
        let src = sounds
            .iter()
            .find(|s| s.name.as_str() == p.sound)
            .expect("project::build found it");
        let snd = to_sound(src)?;
        for (m, v) in sends.iter_mut().zip(snd.params.out.sends) {
            *m = m.max(v);
        }
        // Each Part's click exemption and home note.
        exempt.push((p.part, listen::click_exempt(&snd), src.home));
        parts.push(format!(
            "PART {} · {} ({}) · {} · CH {}",
            p.part.index() + 1,
            p.sound,
            src.badge(source::approval_crc(src)?).word(),
            p.mode.disk_ident(),
            p.channel.get() + 1
        ));
    }
    let mut perf = Performance::new();
    for p in PartId::ALL {
        let e = perf.edit(p);
        **e.sound = (*project.part(p).sound).clone();
        *e.mix = project.part(p).mix;
    }
    let mut gates = Vec::new();
    let mut renders = Vec::new();
    for (i, s) in settings.iter().enumerate() {
        perf.fx = s.fx;
        for take in demo_takes(d) {
            let (t, onset, ex) = match take {
                DemoTake::Part(part, script) => {
                    let (ex, home) = exempt
                        .iter()
                        .find(|e| e.0 == part)
                        .map_or((None, 60), |e| (e.1, e.2));
                    let mut p = plan(script, home);
                    p.secs += DEMO_TAIL_S;
                    let ch = perf.part(part).mix.channel;
                    let on: Vec<usize> = p
                        .hits
                        .iter()
                        .map(|h| block(h.on) * BLOCK_SIZE + LATE)
                        .collect();
                    (play_on(&perf, ch, &p), on, ex.or(s.click_exempt()))
                }
                DemoTake::Hats | DemoTake::Snare => {
                    let x = sounding(if take == DemoTake::Hats {
                        &stims.hats
                    } else {
                        &stims.snare
                    });
                    let secs = x.len() as f32 / SR + DEMO_TAIL_S;
                    // Block 0 silent, and the limiter's block late.
                    let on = onsets(x)
                        .into_iter()
                        .map(|o| o + BLOCK_SIZE + LATE)
                        .collect();
                    (play_stim(&s.fx, sends, x, secs), on, s.click_exempt())
                }
            };
            let bad = t.stereo.iter().filter(|v| !v.is_finite()).count();
            let mut gate = |check, (value, verdict)| {
                gates.push(DemoGate {
                    setting: i,
                    check,
                    value,
                    verdict,
                })
            };
            gate(DemoCheck::Finite(take), (bad as f32, verdict(bad == 0)));
            gate(DemoCheck::Clicks(take), clicks(&t.mono(), &onset, ex));
            gate(
                DemoCheck::Limiter(take),
                (t.reduced_db, verdict(t.reduced_db <= LIMIT_CHORD_DB)),
            );
            let limited_db = t.reduced_db;
            let r = heard(Audition::Script(Script::Stab), t, &[], None);
            renders.push(DemoRender {
                setting: i,
                take,
                lufs: r.lufs,
                peak_dbfs: r.peak_dbfs,
                limited_db,
                stereo: r.stereo,
            });
        }
    }
    Ok(DemoReport {
        id: d.id,
        name: d.name.as_str().into(),
        tries: d.tries.clone(),
        parts,
        settings: settings.iter().map(heard_words).collect(),
        gates,
        renders,
    })
}

/// `report.txt`'s demo lines: each project's gates by setting.
pub fn demo_text(demos: &[DemoReport]) -> String {
    let mut o = String::new();
    for d in demos {
        let ok = if d.passes() { "PASS" } else { "FAIL" };
        let _ = writeln!(o, "{} {}  {ok}", d.id.stem(), d.name);
        for (i, s) in d.settings.iter().enumerate() {
            let _ = writeln!(o, "  setting {}: {s}", i + 1);
            for g in d.gates.iter().filter(|g| g.setting == i) {
                let v = match &g.verdict {
                    Verdict::Pass => "PASS".to_string(),
                    Verdict::Fail => "FAIL".to_string(),
                    Verdict::Exempt(why) => format!("EXEMPT ({why})"),
                };
                let _ = writeln!(
                    o,
                    "    {:<22} {:>8.2} {:<6} {v}",
                    g.check.label(),
                    g.value,
                    g.check.unit()
                );
            }
        }
    }
    if !demos.is_empty() {
        let pass = demos.iter().all(DemoReport::passes);
        let _ = writeln!(
            o,
            "{} demo projects: {}",
            demos.len(),
            if pass { "all pass" } else { "FAIL" }
        );
    }
    o
}

/// A passing demo's row in the DEMO PROJECTS section.
fn demo_row(d: &DemoReport) -> Row {
    let mut lines = vec![(format!("What to try: {}", d.tries), false)];
    lines.push((d.id.stem(), false));
    lines.extend(d.parts.iter().map(|p| (p.clone(), false)));
    for g in &d.gates {
        if let Verdict::Exempt(why) = &g.verdict {
            lines.push((format!("{}: {why}", g.check.label()), true));
        }
    }
    Row {
        title: d.name.clone(),
        badge: None,
        lines,
        facts: Vec::new(),
        players: d
            .renders
            .iter()
            .map(|r| Player {
                label: format!("{} · {}", r.take.label(), d.settings[r.setting]),
                file: format!("renders/{}", d.file(r)),
                info: format!(
                    "{:.1} LUFS · peak L {:.1} · R {:.1} dBFS · limiter {:.1} dB",
                    r.lufs, r.peak_dbfs[0], r.peak_dbfs[1], r.limited_db
                ),
                flag: false,
            })
            .collect(),
        section: Section::Demos,
    }
}
