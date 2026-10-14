//! What a factory Sound plays (spec § 4): the render scripts and the
//! phrases, as timed notes (`Plan`), and the swell a `sweep` Sound must
//! hold. Pure: no rendering, so the build checks a sweep without the QA.

use chimera_core::dsp::modulator::law::{B_TIME, speed_ranges};
use chimera_core::dsp::modulator::{EnvSlot, EnvType};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::part::PartMode;

use super::source::{Phrase, Script, Source, patch};

/// After the last note-off, each script's tail (but `long`'s 2 s).
const TAIL_S: f32 = 1.5;

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

pub(crate) fn hit(on: f32, off: f32, note: u8, vel: u8) -> Hit {
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
