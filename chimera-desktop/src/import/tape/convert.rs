//! WAV to 12-bit samples (TAPE spec § 6 steps 1–4 and 6): read, clean, pitch,
//! resample, quantise. Pure functions over `f32` buffers; the packing is
//! `chimera-core`'s `pack12`, so host and unit share the bytes.

use chimera_core::dsp::clampf;
use rubato::audioadapter_buffers::owned::InterleavedOwned;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};
use std::f64::consts::PI;
use std::io::Cursor;

/// Which channel of a stereo file becomes the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// `(L + R) / 2` (every channel's mean).
    Mix,
    L,
    R,
}

/// A `smpl` chunk's root and first loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Smpl {
    /// MIDI note, 0–127.
    pub unity: u8,
    /// Above `unity`, in 1/2^32 of a semitone.
    pub fraction: u32,
    /// The first loop's start and end frames, as the file has them.
    pub loop_frames: Option<(u32, u32)>,
}

/// One channel at the file's rate, full scale ±1.
#[derive(Clone, Debug, PartialEq)]
pub struct Mono {
    pub rate: u32,
    pub samples: Vec<f32>,
    pub smpl: Option<Smpl>,
}

#[derive(Debug)]
pub enum WavError {
    /// `hound` refused the file.
    Read(hound::Error),
    /// A float sample NaN, infinite or past ±`FLOAT_LIMIT`.
    NotAudio,
    /// A sample rate of 0.
    NoRate,
}

impl From<hound::Error> for WavError {
    fn from(e: hound::Error) -> Self {
        WavError::Read(e)
    }
}

/// A float file past this is not audio (and its mix and high-pass would
/// overflow); NaN fails the same test.
const FLOAT_LIMIT: f32 = 1e6;

/// PCM 8/16/24/32-bit or 32-bit float, any channel count; the `smpl` chunk by
/// a RIFF walk (`hound` skips it).
pub fn read_wav(bytes: &[u8], ch: Channel) -> Result<Mono, WavError> {
    let mut r = hound::WavReader::new(Cursor::new(bytes))?;
    let spec = r.spec();
    if spec.sample_rate == 0 {
        return Err(WavError::NoRate);
    }
    let all: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 0.5f32.powi(i32::from(spec.bits_per_sample) - 1);
            r.samples::<i32>()
                .map(|s| s.map(|s| s as f32 * scale))
                .collect::<Result<_, _>>()?
        }
    };
    if !all.iter().all(|s| s.abs() <= FLOAT_LIMIT) {
        return Err(WavError::NotAudio);
    }
    let n = usize::from(spec.channels).max(1);
    let pick = |f: &[f32]| match ch {
        Channel::Mix => f.iter().sum::<f32>() / n as f32,
        Channel::L => f[0],
        Channel::R => f[1.min(n - 1)],
    };
    Ok(Mono {
        rate: spec.sample_rate,
        samples: all.chunks_exact(n).map(pick).collect(),
        smpl: smpl(bytes),
    })
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The first `smpl` chunk: 36 B, then 24 B a loop. A unity note past 127 is
/// no root.
fn smpl(b: &[u8]) -> Option<Smpl> {
    if b.get(0..4)? != b"RIFF" || b.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12usize;
    while let Some(id) = b.get(at..at + 4) {
        let len = le32(b, at + 4)? as usize;
        let body = at + 8;
        if id == b"smpl" {
            let c = b.get(body..body.checked_add(len)?)?;
            let unity = u8::try_from(le32(c, 12)?).ok().filter(|&u| u <= 127)?;
            let loops = le32(c, 28)?;
            let loop_frames = if loops > 0 {
                le32(c, 36 + 8).zip(le32(c, 36 + 12))
            } else {
                None
            };
            return Some(Smpl {
                unity,
                fraction: le32(c, 16)?,
                loop_frames,
            });
        }
        at = body.checked_add(len)?.checked_add(len & 1)?;
    }
    None
}

/// 5 Hz high-pass, silence under −60 dBFS trimmed from both ends, the mean
/// taken out, peak to −0.5 dBFS.
pub fn clean(m: &mut Mono) {
    let x = &mut m.samples;
    // One-pole DC blocker primed on the first sample, so an offset is no step.
    let r = (-std::f32::consts::TAU * 5.0 / m.rate as f32).exp();
    let (mut x1, mut y1) = (x.first().copied().unwrap_or(0.0), 0.0f32);
    for s in x.iter_mut() {
        let y = *s - x1 + r * y1;
        (x1, y1) = (*s, y);
        *s = y;
    }
    let floor = 10f32.powf(-60.0 / 20.0);
    let Some(first) = x.iter().position(|s| s.abs() >= floor) else {
        x.clear();
        return;
    };
    let last = x.iter().rposition(|s| s.abs() >= floor).unwrap_or(first);
    x.truncate(last + 1);
    x.drain(..first);
    // The blocker's tail leaves a little DC; what is left is exact.
    let mean = (x.iter().map(|&s| f64::from(s)).sum::<f64>() / x.len() as f64) as f32;
    let peak = x.iter().fold(0f32, |a, s| a.max((s - mean).abs()));
    let gain = if peak > 0.0 {
        10f32.powf(-0.5 / 20.0) / peak
    } else {
        0.0
    };
    x.iter_mut().for_each(|s| *s = (*s - mean) * gain);
}

/// The rates a source may have (descriptor 4 000–48 000 Hz).
pub const RATES: [u32; 6] = [48_000, 32_000, 24_000, 16_000, 12_000, 8_000];

/// `rubato`'s windowed sinc (256 taps, Blackman-Harris²), cut-off 0.35 × `to`
/// when going down; ≥ 80 dB stop band from 0.5 × `to`. Delay compensated: the
/// output starts where the input does.
pub fn resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || x.is_empty() {
        return x.to_vec();
    }
    // Relative to the lower rate's Nyquist.
    let f_cutoff = (0.7 * to as f32 / from.min(to) as f32).min(0.95);
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: Some(f_cutoff),
        oversampling_factor: 256,
        interpolation: SincInterpolationType::Cubic,
        window: WindowFunction::BlackmanHarris2,
    };
    let ratio = f64::from(to) / f64::from(from);
    let mut rs = Async::<f32>::new_sinc(ratio, 1.0, &params, 1024, 1, FixedAsync::Input)
        .expect("a positive ratio and one channel");
    let input = InterleavedOwned::new_from(x.to_vec(), 1, x.len()).expect("one channel");
    rs.process_all(&input, x.len(), None)
        .expect("buffers sized by rubato")
        .take_data()
}

/// A source's pitch, at the rate YIN ran on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pitch {
    /// `period` in frames; `root_cents` in MIDI cents (6 000 = C4).
    Pitched {
        period: f32,
        root_cents: u16,
    },
    Unpitched,
}

/// The aperiodicity under which a source is pitched (spec § 6).
const APERIODIC: f64 = 0.2;

/// YIN (de Cheveigné & Kawahara 2002) over the loudest 100 ms: the
/// cumulative-mean-normalised difference, its first dip under 0.2 followed to
/// its floor, refined on the raw difference between lags.
pub fn yin(x: &[f32], rate: u32) -> Pitch {
    let w = (rate as usize / 10).clamp(1, x.len().max(1)).min(x.len());
    if w < 8 {
        return Pitch::Unpitched;
    }
    // The loudest window, by a running sum of squares.
    let (mut e, mut best, mut at) = (0f64, 0f64, 0usize);
    for i in 0..x.len() {
        e += f64::from(x[i]) * f64::from(x[i]);
        if i >= w {
            e -= f64::from(x[i - w]) * f64::from(x[i - w]);
        }
        if i + 1 >= w && e > best {
            (best, at) = (e, i + 1 - w);
        }
    }
    let seg = &x[at..at + w];
    let tmax = w / 2;
    let d: Vec<f64> = (0..=tmax)
        .map(|t| {
            let diff = seg[..w - tmax].iter().zip(&seg[t..]).map(|(a, b)| a - b);
            diff.map(|e| f64::from(e * e)).sum()
        })
        .collect();
    let mut cmnd = vec![1f64; tmax + 1];
    let mut sum = 0f64;
    for t in 1..=tmax {
        sum += d[t];
        cmnd[t] = if sum > 0.0 {
            d[t] * t as f64 / sum
        } else {
            1.0
        };
    }
    let Some(mut t) = (2..tmax).find(|&t| cmnd[t] < APERIODIC) else {
        return Pitch::Unpitched;
    };
    while t + 1 < tmax && cmnd[t + 1] < cmnd[t] {
        t += 1;
    }
    let period = floor_of(&d, t);
    let cents = 6_900.0 + 1_200.0 * (f64::from(rate) / period / 440.0).log2();
    Pitch::Pitched {
        period: period as f32,
        root_cents: clampf(cents.round() as f32, 0.0, 12_700.0) as u16,
    }
}

/// Lanczos half-width, in lags.
const LANCZOS: i64 = 16;

/// The difference is an autocorrelation, so it is band-limited in lag: read
/// between lags by Lanczos, its floor near `t` found by ternary search. A
/// parabola through three lags sits 0.035 frame off on a saw at 220.5 Hz.
fn floor_of(d: &[f64], t: usize) -> f64 {
    let sinc = |x: f64| {
        if x == 0.0 {
            1.0
        } else {
            (PI * x).sin() / (PI * x)
        }
    };
    let at = |x: f64| {
        let i0 = x.floor() as i64;
        (i0 - LANCZOS + 1..=i0 + LANCZOS)
            .map(|i| {
                let u = x - i as f64;
                d[i.clamp(0, d.len() as i64 - 1) as usize] * sinc(u) * sinc(u / LANCZOS as f64)
            })
            .sum::<f64>()
    };
    let (mut lo, mut hi) = (t as f64 - 1.0, t as f64 + 1.0);
    for _ in 0..48 {
        let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if at(a) < at(b) { hi = b } else { lo = a }
    }
    0.5 * (lo + hi)
}

/// 12-bit with TPDF dither of ±1 LSB (seeded, so a source rebuilds bit for
/// bit), into −2048..=2047. An odd length repeats its last sample: a 0 pad
/// would click once a wrap when LOOP or PING-PONG plays the whole source.
pub fn quantise_tpdf(x: &[f32], seed: u64) -> Vec<i16> {
    let mut s = seed;
    // splitmix64, the top 24 bits as a uniform 0..1.
    let mut uniform = || {
        s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u32 << 24) as f32
    };
    let mut q: Vec<i16> = x
        .iter()
        .map(|&v| {
            let tpdf = uniform() - uniform();
            clampf((v * 2048.0 + tpdf).round(), -2048.0, 2047.0) as i16
        })
        .collect();
    if let Some(&last) = q.last().filter(|_| q.len() % 2 == 1) {
        q.push(last);
    }
    q
}
