//! The gates (spec § 4): pure measurements over sample buffers, each
//! with its bar and its verdict.

use chimera_core::dsp::modulator::law::{B_TIME, speed_ranges};
use chimera_core::dsp::modulator::{EnvSlot, EnvType};
use chimera_core::modulation::{FixedCol, ModSource};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_hal::BLOCK_SIZE;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use super::super::phrase::Hit;
use super::super::source::{Script, Source};
use super::SR;
use super::render::{LATE, block};
use crate::listen::qa_takes;

pub use super::super::source::{LOUDNESS_LU, REFERENCE_LUFS};
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
pub(super) const DC_HZ: f64 = 10.0;
/// Harmonic over inharmonic energy, at C6 and C7.
pub const ALIAS_DB: f32 = 40.0;
/// Silence, and how long past the release it may take to get there.
pub const TAIL_DBFS: f32 = -90.0;
/// At or under this a measured peak is digital silence (`db`'s floor).
pub(super) const SILENT_FLOOR_DB: f32 = -200.0;
pub const TAIL_GRACE_S: f32 = 2.0;
/// MODAL rings on past note-off (Rings): its own limit.
pub const MODAL_TAIL_S: f32 = 10.0;
/// The window the tail is judged over, at its limit.
pub(super) const TAIL_WINDOW_S: f32 = 0.1;
/// The tap must move the spectrum at least this (RMS over the ERB bands).
pub const TAP_DB: f32 = 6.0;
/// Two Sounds within this on every ERB band are samey.
pub const SAMEY_DB: f32 = 3.0;
pub const ERB_BANDS: usize = 32;
/// The ERB bands span this.
pub(super) const ERB_LO_HZ: f32 = 50.0;
pub(super) const ERB_HI_HZ: f32 = 20_000.0;
/// The aliasing renders' notes: C6 and C7.
pub const ALIAS_NOTES: [u8; 2] = [84, 96];

/// A gate's outcome.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Pass,
    Fail,
    /// Not measured against the bar, and why: the owner's ear decides.
    Exempt(String),
}

/// As `report.txt` writes it: `PASS`, `FAIL`, `EXEMPT (<why>)`.
impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verdict::Pass => f.write_str("PASS"),
            Verdict::Fail => f.write_str("FAIL"),
            Verdict::Exempt(why) => write!(f, "EXEMPT ({why})"),
        }
    }
}

pub(super) fn verdict(ok: bool) -> Verdict {
    if ok { Verdict::Pass } else { Verdict::Fail }
}

pub(super) fn db(x: f32) -> f32 {
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

/// No NaN or infinity in `x`: the value is the count of samples that are
/// not finite.
pub fn finite(x: &[f32]) -> (f32, Verdict) {
    let bad = x.iter().filter(|v| !v.is_finite()).count();
    (bad as f32, verdict(bad == 0))
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
pub(super) fn spectrum(x: &[f32], windowed: bool) -> (Vec<f64>, f64) {
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
pub(super) const SIDEBAND_HZ: f64 = 25.0;

/// Harmonic over inharmonic energy, dB, for a note at `f0`: a bin is
/// harmonic within `SIDEBAND_HZ` plus 0.3 % (5 cents of detune; at least
/// 8 bins, the window's main lobe) of a multiple of `f0 / 2`, so a sub an
/// octave down counts as harmonic. An alias folds back between them.
pub(super) fn harmonic_db(x: &[f32], f0: f32) -> f32 {
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
pub(super) fn erb_rate(f: f32) -> f32 {
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
