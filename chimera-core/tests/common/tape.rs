//! Sources for TAPE's tests: `held` bytes as the arena holds them (a
//! descriptor, then packed 12-bit samples) and the signals put in them.
#![allow(dead_code)]

use chimera_core::dsp::clampf;
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::dsp::tape_engine::wave;
use chimera_core::project::SourceId;
use chimera_core::source_table::{AudioToken, GateView, SourceTable};
use chimera_core::storage::twelve::pack12;

use super::XorShift;

/// `x` in [−1, 1] as a 12-bit sample, rounded; full scale clips to 2047.
pub fn to12(x: f32) -> i16 {
    clampf((x * 2048.0).round(), -2048.0, 2047.0) as i16
}

/// One source in a table of its own, read as the audio reads the pool: a
/// voice's `GateView` for the tests. The table and its bytes are leaked
/// (`reader` wants them `'static`): build one a test, not one a case.
pub struct OneSource {
    tok: AudioToken,
    table: &'static SourceTable,
    held: u32,
    rate: u16,
    pub id: SourceId,
}

impl OneSource {
    /// `held` (a descriptor, then its samples) as source 0.
    pub fn new(held: &[u8]) -> OneSource {
        Self::with(held, &[]).0
    }

    /// `held` as source 0 and, beside it in the same table, `second` as
    /// source 1 (if not empty): its id.
    pub fn with(held: &[u8], second: &[u8]) -> (OneSource, SourceId) {
        let table: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
        let bytes: &'static mut [u8] = Box::leak([held, second].concat().into_boxed_slice());
        table.attach_for_test(bytes.as_mut_ptr());
        let rate = |b: &[u8]| Descriptor::from_bytes(b).map_or(0, |d| d.rate().get());
        let (id, id2) = (SourceId::ALL[0], SourceId::ALL[1]);
        table.publish_for_test(id, 0, held.len() as u32, rate(held));
        if !second.is_empty() {
            table.publish_for_test(id2, held.len() as u32, second.len() as u32, rate(second));
        }
        let rate = rate(held);
        let one = OneSource {
            tok: table.reader().expect("its own table"),
            table,
            held: held.len() as u32,
            rate,
            id,
        };
        (one, id2)
    }

    /// A block's view, as the audio begins one.
    pub fn view(&mut self) -> GateView<'_> {
        self.tok.begin_block()
    }

    /// The audio's token, for a shell's whole block (`run_block_ported`).
    #[allow(dead_code)]
    pub fn token(&mut self) -> &mut AudioToken {
        &mut self.tok
    }

    /// The row gone: retired and compacted, as the pool leaves it.
    pub fn clear(&self) {
        self.table.clear_for_test(self.id);
    }

    /// The row back, as it was.
    pub fn publish(&self) {
        self.table
            .publish_for_test(self.id, 0, self.held, self.rate);
    }

    /// A compaction's gate closed on the source: the next view moves it.
    pub fn close(&self) {
        self.table.close_gate_for_test(self.id.bit());
    }

    /// The gate open again.
    pub fn open(&self) {
        self.table.open_gate_for_test();
    }
}

/// `samples` packed after a `Plain` descriptor at `rate`, held whole.
pub fn held_plain(samples: &[f32], rate: u16) -> Vec<u8> {
    let d = Descriptor::whole(Shape::Plain, 1, rate).expect("a descriptor");
    let s: Vec<i16> = samples.iter().map(|&x| to12(x)).collect();
    let mut held = d.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * s.len().div_ceil(2), 0);
    pack12(&s, &mut held[DESC_LEN..]);
    held
}

/// `frames` of a full-scale sine `period` frames long. The samples don't
/// depend on `_rate`: a source holds frames, and its rate plays them.
pub fn sine(_rate: u16, period: f32, frames: u32) -> Vec<f32> {
    (0..frames)
        .map(|i| libm::sinf(core::f32::consts::TAU * i as f32 / period))
        .collect()
}

/// `secs` of a 0.5 saw at 220 Hz with 50 ms of seeded white noise at
/// 0.5 from a quarter of the way in: a pitched tone and a transient.
pub fn saw_with_burst(rate: u16, secs: f32) -> Vec<f32> {
    let n = (rate as f32 * secs) as usize;
    let (from, len) = (n / 4, rate as usize / 20);
    let mut rng = XorShift(0x5A3B_0001);
    (0..n)
        .map(|i| {
            let ph = (i as f32 * 220.0 / rate as f32).fract();
            let noise = if (from..from + len).contains(&i) {
                rng.next_u32() as i32 as f32 / i32::MAX as f32 * 0.5
            } else {
                0.0
            };
            (ph - 0.5) + noise
        })
        .collect()
}

/// A `Transwave` of `count` frames at stored top `top` (nominal levels
/// `top / 2` down to 16, each at `wave::stored_log2`'s length), frame `f`'s
/// sine-phase harmonic `h` at amplitude `amp(f, h)`: each nominal level `N`
/// keeps harmonics `1..=wave::harmonics`, as the importer's do, rounded to
/// 12 bits.
pub fn held_transwave(top: u32, count: u8, amp: impl Fn(usize, u32) -> f32) -> Vec<u8> {
    let d = Descriptor::whole(Shape::Transwave, count, 48_000).expect("a descriptor");
    let mut s: Vec<i16> = Vec::new();
    for f in 0..count as usize {
        let mut l = wave::nominal_top(top).trailing_zeros();
        while l >= wave::MIN_LOG2 {
            let n = 1u64 << wave::stored_log2(l);
            let parts: Vec<(u64, f32)> = (1..=u64::from(wave::harmonics(l)))
                .map(|h| (h, amp(f, h as u32)))
                .filter(|&(_, a)| a != 0.0)
                .collect();
            s.extend((0..n).map(|i| {
                let x = parts.iter().fold(0.0f64, |y, &(h, a)| {
                    let w = core::f64::consts::TAU * (h * i % n) as f64;
                    y + a as f64 * (w / n as f64).sin()
                });
                to12(x as f32)
            }));
            l -= 1;
        }
    }
    assert_eq!(s.len(), count as usize * wave::frame_len(top) as usize);
    let mut held = d.to_bytes().to_vec();
    held.resize(DESC_LEN + 3 * s.len() / 2, 0);
    pack12(&s, &mut held[DESC_LEN..]);
    held
}

/// A 4-frame transwave from a saw to a square (frame 0 a saw, frame 3 a
/// square, the two between blends), stored top 512.
pub fn saw_to_square() -> Vec<u8> {
    held_transwave(512, 4, |f, h| {
        let (saw, square) = (
            0.5 / h as f32,
            if h % 2 == 1 { 0.6 / h as f32 } else { 0.0 },
        );
        let k = f as f32 / 3.0;
        saw + (square - saw) * k
    })
}

/// In-place iterative radix-2 FFT; `re.len()` is a power of two.
pub fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -core::f64::consts::TAU / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// Power per bin, 0..=n/2, of `x` (a power of two long) under a 4-term
/// Blackman-Harris window: sidelobes 92 dB down, a main lobe ±4 bins.
pub fn power_bh(x: &[f32]) -> Vec<f64> {
    let n = x.len();
    let w = |i: usize| {
        let t = core::f64::consts::TAU * i as f64 / n as f64;
        0.35875 - 0.48829 * t.cos() + 0.14128 * (2.0 * t).cos() - 0.01168 * (3.0 * t).cos()
    };
    let mut re: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(i, &v)| v as f64 * w(i))
        .collect();
    let mut im = vec![0.0; n];
    fft(&mut re, &mut im);
    (0..=n / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect()
}

/// A machine as a test plays it: its model, and END before START or not.
/// The old REVERSE is `Mach::rev(TapeModel::Play)` (#369).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mach {
    pub model: chimera_core::dsp::tape_engine::TapeModel,
    pub mode: chimera_core::dsp::tape_engine::PlayMode,
    pub rev: bool,
}

impl Mach {
    pub const fn fwd(model: chimera_core::dsp::tape_engine::TapeModel) -> Mach {
        Mach {
            model,
            mode: chimera_core::dsp::tape_engine::PlayMode::Loop,
            rev: false,
        }
    }

    pub const fn rev(model: chimera_core::dsp::tape_engine::TapeModel) -> Mach {
        Mach {
            model,
            mode: chimera_core::dsp::tape_engine::PlayMode::Loop,
            rev: true,
        }
    }

    /// PLAY in MODE PING.
    pub const fn ping() -> Mach {
        Mach {
            model: chimera_core::dsp::tape_engine::TapeModel::Play,
            mode: chimera_core::dsp::tape_engine::PlayMode::Ping,
            rev: false,
        }
    }

    /// `p` on this machine: its START and END swapped when reversed.
    pub fn params(
        self,
        p: chimera_core::dsp::tape_engine::TapeEngineParams,
    ) -> chimera_core::dsp::tape_engine::TapeEngineParams {
        let p = chimera_core::dsp::tape_engine::TapeEngineParams {
            model: self.model,
            mode: self.mode,
            ..p
        };
        if self.rev {
            chimera_core::dsp::tape_engine::TapeEngineParams {
                start: p.end,
                end: p.start,
                ..p
            }
        } else {
            p
        }
    }
}
