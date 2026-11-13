//! The burst kernel: noise under a schedule of bursts or grains, through a
//! CLEAN band-pass. TONE CLAP and BODY SHAKER (`Burst`, machines.rs:1203).

use crate::dsp::filter::FilterMode;
use crate::hw::BLOCK_SIZE;

use super::super::laws::{BurstParams, ClapCount, ClapParams, Hardness, ShakerParams};
use super::{Blk, Decay, FLOOR, Hiss, Hit, Lcg, Svf, per_sample, quiet};

/// The most claps CLAPS reaches.
const MAX_CLAPS: usize = ClapCount::MAX.get() as usize;

pub struct Burst {
    hiss: Hiss,
    rng: Lcg,
    bp: Svf,
    kind: Kind,
    n: u32,
    /// HARDNESS: the grains' one-pole low-pass coefficient (1 open), its
    /// state, and the dry grains' share over the band.
    lpk: f32,
    hard_lp: f32,
    wet: f32,
    gain: f32,
    vel: f32,
    quiet: bool,
}

enum Kind {
    Clap(Clap),
    Shaker(Shaker),
}

struct Clap {
    /// Burst onsets (samples), `count` of them; the tail starts at the last.
    onsets: [u32; MAX_CLAPS],
    count: ClapCount,
    env: f32,
    burst: f32,
    tail_at: u32,
    tail: Decay,
    tail_level: f32,
    /// ROOM TONE: the tail's one-pole low-pass coefficient and state.
    room: f32,
    room_lp: f32,
}

struct Shaker {
    /// Grains a sample at the shake's peak, its length (samples) and the
    /// peak's place.
    density: f32,
    len: f32,
    peak: f32,
    /// The latest grain, falling by `grain_k` a sample.
    grain: f32,
    grain_k: f32,
}

impl Burst {
    pub(super) fn new(p: BurstParams, h: Hit) -> Burst {
        match p {
            BurstParams::Clap(p) => Burst::clap(p, h),
            BurstParams::Shaker(p) => Burst::shaker(p, h),
        }
    }

    /// `Burst::clap`, machines.rs:1236.
    fn clap(p: ClapParams, h: Hit) -> Burst {
        let fs = h.sr.hz();
        let mut rng = Lcg::new(h.seed, 0x5eed);
        let count = p.claps;
        let spread = p.spread.get() * fs;
        let mut onsets = [0; MAX_CLAPS];
        let mut t = 0.0f32;
        for o in onsets.iter_mut().take(count.get().into()) {
            *o = t as u32;
            t += spread * (1.0 + 0.25 * rng.next());
        }
        // The last onset: `ClapCount` is never 0.
        let tail_at = onsets
            .get(usize::from(count.get().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        Burst {
            hiss: Hiss::new(h.seed),
            rng,
            bp: Svf::new(FilterMode::Bp12, p.tone, p.ring, h.sr),
            kind: Kind::Clap(Clap {
                onsets,
                count,
                env: 0.0,
                burst: libm::expf(-1.0 / (p.burst.get() * spread)),
                tail_at,
                tail: Decay::t60(p.decay, h.sr),
                tail_level: p.tail.get(),
                room: 1.0 - libm::expf(-core::f32::consts::TAU * p.room.get() / fs),
                room_lp: 0.0,
            }),
            n: 0,
            lpk: 1.0,
            hard_lp: 0.0,
            wet: 0.0,
            gain: 0.6,
            vel: h.vel.unit(),
            quiet: false,
        }
    }

    /// `Burst::shaker`, machines.rs:1276.
    fn shaker(p: ShakerParams, h: Hit) -> Burst {
        let fs = h.sr.hz();
        let mut rng = Lcg::new(h.seed, 0x5eed);
        // The prototype builds a clap at every knob ½ first: its five
        // onsets' jitter draws come before the grains'.
        for _ in 0..5 {
            rng.next();
        }
        let (wet, lpk) = match p.hardness {
            Hardness::Dry(w) => (w.get(), 1.0),
            Hardness::Dark(k) => (0.0, k.get()),
        };
        Burst {
            hiss: Hiss::new(h.seed),
            rng,
            bp: Svf::new(FilterMode::Bp12, p.tone, p.ring, h.sr),
            kind: Kind::Shaker(Shaker {
                density: p.grains.get() / fs,
                len: p.length.get() * fs,
                peak: p.shake.get(),
                grain: 1.0,
                grain_k: per_sample(p.grain, h.sr),
            }),
            n: 0,
            lpk,
            hard_lp: 0.0,
            wet,
            gain: 0.8,
            vel: h.vel.unit(),
            quiet: false,
        }
    }

    #[inline(never)]
    pub(super) fn render(&mut self, out: &mut Blk) {
        let mut x = [0.0; BLOCK_SIZE];
        self.hiss.block(&mut x);
        for s in x.iter_mut() {
            let e = match &mut self.kind {
                Kind::Shaker(k) => {
                    let u = self.n as f32 / k.len;
                    let shape = if u < k.peak {
                        u / k.peak
                    } else if u < 1.0 {
                        let d = (1.0 - u) / (1.0 - k.peak);
                        d * d
                    } else {
                        0.0
                    };
                    if self.rng.unit() < k.density * libm::sqrtf(shape) {
                        k.grain = 0.4 + 0.6 * self.rng.unit();
                    }
                    k.grain *= k.grain_k;
                    k.grain * shape
                }
                Kind::Clap(k) => {
                    // private issue #952
                    if k.onsets
                        .iter()
                        .take(k.count.get().into())
                        .any(|&o| o == self.n)
                    {
                        k.env = 1.0;
                    }
                    k.env *= k.burst;
                    let t = if self.n >= k.tail_at {
                        k.tail_level * k.tail.next()
                    } else {
                        0.0
                    };
                    let e = k.env.max(0.6 * t);
                    // ROOM TONE darkens the tail's share alone.
                    k.room_lp += k.room * (*s - k.room_lp);
                    let w = if e > 0.0 { 0.6 * t / e } else { 0.0 };
                    *s += w * (k.room_lp - *s);
                    e
                }
            };
            *s *= e;
            self.n = self.n.saturating_add(1);
        }
        let dry = x;
        self.bp.run(&mut x);
        for ((o, v), d) in out.iter_mut().zip(&x).zip(&dry) {
            self.hard_lp += self.lpk * (v - self.hard_lp);
            *o = self.vel * self.gain * (self.hard_lp + 0.25 * self.wet * d);
        }
        self.quiet = quiet(out);
    }

    pub(super) fn finished(&self) -> bool {
        self.quiet
            && match &self.kind {
                Kind::Clap(k) => self.n > k.tail_at && k.env < FLOOR && k.tail.spent(),
                Kind::Shaker(k) => self.n as f32 >= k.len && k.grain < FLOOR,
            }
    }
}
