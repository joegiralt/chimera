//! WAV to 12-bit samples (TAPE spec § 6 steps 1–4 and 6): read, clean, pitch,
//! resample, quantise. Pure functions over `f32` buffers; the packing is
//! `chimera-core`'s `pack12`, so host and unit share the bytes.

use chimera_core::dsp::clampf;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};
use std::f64::consts::PI;
use std::io::Cursor;
use std::ops::RangeInclusive;

/// Which channel of a stereo file becomes the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// `(L + R) / 2`; past two channels, the rest are ignored.
    Mix,
    L,
    R,
}

/// A `smpl` chunk's root and first loop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Smpl {
    /// MIDI note; `None` when the file's is past 127.
    pub unity: Option<u8>,
    /// Above `unity`, in 1/2^32 of a semitone.
    pub fraction: u32,
    /// The first loop's start and end frames, kept only when
    /// `start <= end < frames`.
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
    /// No `RIFF`/`WAVE` with a whole `fmt ` and a `data` chunk.
    NotWav,
    /// `hound` refused the file.
    Read(hound::Error),
    /// A float sample NaN, infinite or past ±`FLOAT_LIMIT`.
    NotAudio,
    /// A rate outside `WAV_RATES`.
    BadRate,
}

impl From<hound::Error> for WavError {
    fn from(e: hound::Error) -> Self {
        WavError::Read(e)
    }
}

/// The rates a file may have: past these it is not audio, and 1 Hz into
/// 48 kHz would be a 48 000-fold allocation.
pub const WAV_RATES: RangeInclusive<u32> = 1_000..=768_000;

/// A float file past this is not audio (and its mix and high-pass would
/// overflow); NaN fails the same test.
const FLOAT_LIMIT: f32 = 1e6;

/// The chunks a WAV is read from.
struct Riff<'a> {
    fmt: &'a [u8],
    data: &'a [u8],
    smpl: Option<&'a [u8]>,
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Walks the chunks honouring pad bytes (`hound` doesn't: an odd chunk before
/// `data` refuses the file). `data` is cut to the bytes present, in whole
/// frames, so a recorder's truncated file still reads.
fn walk(b: &[u8]) -> Option<Riff<'_>> {
    if b.get(0..4)? != b"RIFF" || b.get(8..12)? != b"WAVE" {
        return None;
    }
    let (mut fmt, mut data, mut smpl) = (None, None, None);
    let mut at = 12usize;
    while let (Some(id), Some(len)) = (b.get(at..at + 4), le32(b, at + 4)) {
        let body = at + 8;
        let len = len as usize;
        let whole = b.get(body..body.saturating_add(len));
        match id {
            b"fmt " if fmt.is_none() => fmt = whole,
            b"data" if data.is_none() => {
                data = Some(&b[body.min(b.len())..b.len().min(body.saturating_add(len))])
            }
            b"smpl" if smpl.is_none() => smpl = whole,
            _ => {}
        }
        let Some(next) = body.checked_add(len).and_then(|n| n.checked_add(len & 1)) else {
            break;
        };
        at = next;
    }
    let fmt: &[u8] = fmt?;
    let data: &[u8] = data?;
    let align = usize::from(u16::from_le_bytes(fmt.get(12..14)?.try_into().ok()?)).max(1);
    Some(Riff {
        fmt,
        data: &data[..data.len() / align * align],
        smpl,
    })
}

/// `RIFF`, `fmt ` and `data` alone, every size true: what `hound` reads.
fn canonical(r: &Riff) -> Vec<u8> {
    let pad = r.fmt.len() & 1;
    let riff = 4 + 8 + r.fmt.len() + pad + 8 + r.data.len();
    let mut out = Vec::with_capacity(8 + riff);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(riff as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&(r.fmt.len() as u32).to_le_bytes());
    out.extend_from_slice(r.fmt);
    out.extend(std::iter::repeat_n(0, pad));
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(r.data.len() as u32).to_le_bytes());
    out.extend_from_slice(r.data);
    out
}

/// PCM 8/16/24/32-bit or 32-bit float, any channel count; the `smpl` chunk
/// read by the same walk (`hound` skips it).
pub fn read_wav(bytes: &[u8], ch: Channel) -> Result<Mono, WavError> {
    let riff = walk(bytes).ok_or(WavError::NotWav)?;
    // Before hound, which may refuse a 0 rate as something else.
    if !le32(riff.fmt, 4).is_some_and(|r| WAV_RATES.contains(&r)) {
        return Err(WavError::BadRate);
    }
    let mut r = hound::WavReader::new(Cursor::new(canonical(&riff)))?;
    let spec = r.spec();
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
    let pick = |f: &[f32]| match (ch, n) {
        (_, 1) | (Channel::L, _) => f[0],
        (Channel::R, _) => f[1],
        (Channel::Mix, _) => (f[0] + f[1]) / 2.0,
    };
    let samples: Vec<f32> = all.chunks_exact(n).map(pick).collect();
    let smpl = riff.smpl.and_then(|c| smpl(c, samples.len()));
    Ok(Mono {
        rate: spec.sample_rate,
        samples,
        smpl,
    })
}

/// A `smpl` chunk's body: 36 B, then 24 B a loop.
fn smpl(c: &[u8], frames: usize) -> Option<Smpl> {
    let unity = u8::try_from(le32(c, 12)?).ok().filter(|&u| u <= 127);
    let fraction = le32(c, 16)?;
    let loop_frames = le32(c, 36 + 8)
        .zip(le32(c, 36 + 12))
        .filter(|&(s, e)| le32(c, 28) > Some(0) && s <= e && (e as usize) < frames);
    Some(Smpl {
        unity,
        fraction,
        loop_frames,
    })
}

/// The 5 Hz high-pass: one pole, τ = 1/(2π · 5) ≈ 31.8 ms, primed on the first
/// sample so an offset is no step.
pub fn high_pass(x: &mut [f32], rate: u32) {
    let r = (-std::f32::consts::TAU * 5.0 / rate as f32).exp();
    let (mut x1, mut y1) = (x.first().copied().unwrap_or(0.0), 0.0f32);
    for s in x.iter_mut() {
        let y = *s - x1 + r * y1;
        (x1, y1) = (*s, y);
        *s = y;
    }
}

/// The 5 Hz high-pass; silence more than 60 dB under the file's own peak
/// trimmed from both ends, so a quiet file keeps its tail; peak to −0.5 dBFS.
pub fn clean(m: &mut Mono) {
    high_pass(&mut m.samples, m.rate);
    let x = &mut m.samples;
    let peak = x.iter().fold(0f32, |a, s| a.max(s.abs()));
    let floor = peak * 10f32.powf(-60.0 / 20.0);
    // Under 1e-9 (−180 dBFS) is silence: a subnormal peak overflows the gain.
    let Some(first) = x
        .iter()
        .position(|s| s.abs() > floor)
        .filter(|_| peak >= 1e-9)
    else {
        x.clear();
        return;
    };
    let last = x.iter().rposition(|s| s.abs() > floor).unwrap_or(first);
    x.truncate(last + 1);
    x.drain(..first);
    let gain = 10f32.powf(-0.5 / 20.0) / peak;
    x.iter_mut().for_each(|s| *s *= gain);
}

/// The rates a source may have (descriptor 4 000–48 000 Hz).
pub const RATES: [u32; 6] = [48_000, 32_000, 24_000, 16_000, 12_000, 8_000];

/// A resample `rubato` refused: a 0 rate, or a ratio it can't build.
#[derive(Debug, PartialEq, Eq)]
pub struct BadRatio;

/// `rubato`'s windowed sinc (256 taps, Blackman-Harris²), cut-off 0.35 × `to`
/// when going down; ≥ 80 dB stop band from 0.5 × `to`. Delay compensated: the
/// output starts where the input does. Never panics: a 0 rate is `BadRatio`.
pub fn resample(x: &[f32], from: u32, to: u32) -> Result<Vec<f32>, BadRatio> {
    if from == 0 || to == 0 {
        return Err(BadRatio);
    }
    if from == to || x.is_empty() {
        return Ok(x.to_vec());
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
        .map_err(|_| BadRatio)?;
    let input = InterleavedSlice::new(x, 1, x.len()).map_err(|_| BadRatio)?;
    Ok(rs
        .process_all(&input, x.len(), None)
        .map_err(|_| BadRatio)?
        .take_data())
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

/// How far above the deepest dip the chosen one may sit. YIN's absolute 0.1
/// takes the octave: a fundamental 14 dB under its second partial dips to
/// 0.078 at the partial's period.
const DIP: f64 = 0.05;

/// Above this (about C8, with room for 4.8 kHz at 24 kHz's 5 frames) a
/// source is unpitched.
const TOP_HZ: f64 = 5_000.0;

/// YIN (de Cheveigné & Kawahara 2002) over the loudest 100 ms: the
/// cumulative-mean-normalised difference, its first dip within `DIP` of the
/// deepest followed to its floor, pitched under `APERIODIC`, refined on the
/// raw difference between lags.
pub fn yin(x: &[f32], rate: u32) -> Pitch {
    let w = (rate as usize / 10).min(x.len());
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
    // sums[t] = d[1] + … + d[t]
    let sums: Vec<f64> = d
        .iter()
        .scan(-d[0], |a, &v| {
            *a += v;
            Some(*a)
        })
        .collect();
    let cmnd_at = |x: f64, dx: f64| {
        let sum = sums[(x as usize).min(tmax)];
        if sum > 0.0 { dx * x / sum } else { 1.0 }
    };
    let cmnd: Vec<f64> = (0..=tmax)
        .map(|t| if t == 0 { 1.0 } else { cmnd_at(t as f64, d[t]) })
        .collect();
    let deepest = cmnd[2..tmax].iter().copied().fold(f64::INFINITY, f64::min);
    let Some(mut t) = (2..tmax).find(|&t| cmnd[t] <= deepest + DIP) else {
        return Pitch::Unpitched;
    };
    while t + 1 < tmax && cmnd[t + 1] < cmnd[t] {
        t += 1;
    }
    if cmnd[t] >= APERIODIC {
        return Pitch::Unpitched;
    }
    // Integer lags miss a period of a few frames, so the dip lands on a
    // multiple: the shortest sub-multiple that dips as deep is the period.
    let p = floor_of(&d, t);
    let period = (2..)
        .map(|k| p / f64::from(k))
        .take_while(|&q| q >= 2.0)
        .map(|q| floor_of(&d, q.round() as usize))
        .filter(|&q| cmnd_at(q, lanczos(&d, q)) <= deepest + DIP)
        .last()
        .unwrap_or(p);
    if period < f64::from(rate) / TOP_HZ {
        return Pitch::Unpitched;
    }
    let cents = 6_900.0 + 1_200.0 * (f64::from(rate) / period / 440.0).log2();
    Pitch::Pitched {
        period: period as f32,
        root_cents: clampf(cents.round() as f32, 0.0, 12_700.0) as u16,
    }
}

/// Lanczos half-width, in lags.
const LANCZOS: i64 = 16;

/// `d` read between lags; even about lag 0 (`d[−i] = d[i]`), held at the top.
fn lanczos(d: &[f64], x: f64) -> f64 {
    let sinc = |x: f64| {
        if x == 0.0 {
            1.0
        } else {
            (PI * x).sin() / (PI * x)
        }
    };
    let i0 = x.floor() as i64;
    (i0 - LANCZOS + 1..=i0 + LANCZOS)
        .map(|i| {
            let u = x - i as f64;
            let j = i.unsigned_abs().min(d.len() as u64 - 1) as usize;
            d[j] * sinc(u) * sinc(u / LANCZOS as f64)
        })
        .sum()
}

/// The difference is an autocorrelation, so it is band-limited in lag: read
/// between lags by Lanczos, its floor near `t` found by ternary search. A
/// parabola through three lags sits 0.035 frame off on a saw at 220.5 Hz.
fn floor_of(d: &[f64], t: usize) -> f64 {
    let at = |x| lanczos(d, x);
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
