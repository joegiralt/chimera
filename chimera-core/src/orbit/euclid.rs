//! Euclidean fills: k hits spread as evenly as possible over n steps
//! (Bjorklund), rotated.

/// Steps per bar, 1..=32.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Steps(u8);

impl Steps {
    pub const MAX: Steps = Steps(super::MAX_STEPS);

    pub const fn new(n: u8) -> Option<Steps> {
        if n >= 1 && n <= super::MAX_STEPS {
            Some(Steps(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// STEPS, HITS and ROTATE; hits never exceed steps, rotate is mod steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fill {
    steps: Steps,
    hits: u8,
    rotate: u8,
}

impl Fill {
    pub const fn new(steps: Steps, hits: u8, rotate: u8) -> Option<Fill> {
        if hits > steps.0 {
            return None;
        }
        Some(Fill {
            steps,
            hits,
            rotate: rotate % steps.0,
        })
    }

    /// New step count; hits clamp, rotate wraps.
    pub const fn with_steps(self, steps: Steps) -> Fill {
        let hits = if self.hits < steps.0 {
            self.hits
        } else {
            steps.0
        };
        Fill {
            steps,
            hits,
            rotate: self.rotate % steps.0,
        }
    }

    pub const fn steps(self) -> Steps {
        self.steps
    }

    pub const fn hits(self) -> u8 {
        self.hits
    }

    pub const fn rotate(self) -> u8 {
        self.rotate
    }

    pub const fn mask(self) -> HitMask {
        euclid(self.steps, self.hits, self.rotate)
    }
}

/// One bar of hits, bit i = step i.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HitMask {
    bits: u32,
    steps: Steps,
}

impl HitMask {
    pub const fn empty(steps: Steps) -> HitMask {
        HitMask { bits: 0, steps }
    }

    /// `bits` as `steps`' hits; None with a hit past STEPS.
    pub const fn new(steps: Steps, bits: u32) -> Option<HitMask> {
        if (bits as u64) >> steps.0 == 0 {
            Some(HitMask { bits, steps })
        } else {
            None
        }
    }

    pub const fn steps(self) -> Steps {
        self.steps
    }

    pub const fn bits(self) -> u32 {
        self.bits
    }

    pub const fn hits(self) -> u8 {
        self.bits.count_ones() as u8
    }

    /// Step `i` of the loop; wraps every bar.
    pub const fn is_hit(self, i: usize) -> bool {
        self.bits & (1 << (i % self.steps.0 as usize)) != 0
    }

    /// Which hit step `i` is, counting from the bar's first.
    pub const fn hit_index(self, i: usize) -> Option<usize> {
        if !self.is_hit(i) {
            return None;
        }
        let s = i % self.steps.0 as usize;
        Some((self.bits & ((1u32 << s) - 1)).count_ones() as usize)
    }

    /// The hit on step `from` moved to step `to` (both wrap). None unless
    /// `from` is a hit and `to` is free.
    pub const fn moved(self, from: usize, to: usize) -> Option<HitMask> {
        if !self.is_hit(from) || self.is_hit(to) {
            return None;
        }
        let n = self.steps.0 as usize;
        Some(HitMask {
            bits: (self.bits & !(1 << (from % n))) | (1 << (to % n)),
            steps: self.steps,
        })
    }
}

/// A run of steps as bits, bit 0 first.
#[derive(Clone, Copy)]
struct Group {
    bits: u32,
    len: u8,
}

impl Group {
    const fn then(self, g: Group) -> Group {
        Group {
            bits: self.bits | (g.bits << self.len),
            len: self.len + g.len,
        }
    }
}

const fn bjorklund(n: u8, k: u8) -> u32 {
    if k == 0 {
        return 0;
    }
    let hit = Group { bits: 1, len: 1 };
    let rest = Group { bits: 0, len: 1 };
    let mut a = [hit; 32];
    let mut b = [rest; 32];
    let (mut na, mut nb) = (k as usize, (n - k) as usize);
    while nb > 1 {
        let m = if na < nb { na } else { nb };
        let mut i = 0;
        while i < m {
            a[i] = a[i].then(b[i]);
            i += 1;
        }
        // The unpaired groups become the new remainder.
        let mut r = [rest; 32];
        let mut nr = 0;
        if na > m {
            while m + nr < na {
                r[nr] = a[m + nr];
                nr += 1;
            }
            na = m;
        } else {
            while m + nr < nb {
                r[nr] = b[m + nr];
                nr += 1;
            }
        }
        b = r;
        nb = nr;
    }
    let mut out = Group { bits: 0, len: 0 };
    let mut i = 0;
    while i < na {
        out = out.then(a[i]);
        i += 1;
    }
    let mut i = 0;
    while i < nb {
        out = out.then(b[i]);
        i += 1;
    }
    out.bits
}

/// ROTATE r: step i plays the unrotated step i + r, as the spec's
/// `((i + rotate) · hits) mod steps < hits`.
const fn rotate(bits: u32, n: u8, r: u8) -> u32 {
    if r == 0 {
        return bits;
    }
    let mask = if n == 32 { u32::MAX } else { (1 << n) - 1 };
    ((bits >> r) | (bits << (n - r))) & mask
}

/// Total: HITS clamps to STEPS, ROTATE wraps.
pub const fn euclid(steps: Steps, hits: u8, rotate_by: u8) -> HitMask {
    let n = steps.0;
    let k = if hits < n { hits } else { n };
    HitMask {
        bits: rotate(bjorklund(n, k), n, rotate_by % n),
        steps,
    }
}

/// The ACCENT fill: `euclid(k, accent, 0)` laid over the `k` hits of
/// `mask` in order.
pub const fn accents(mask: HitMask, accent: u8) -> HitMask {
    let Some(k) = Steps::new(mask.hits()) else {
        return HitMask::empty(mask.steps);
    };
    let a = euclid(k, accent, 0);
    let mut bits = 0;
    let (mut s, mut j) = (0, 0);
    while s < mask.steps.0 as usize {
        if mask.is_hit(s) {
            if a.is_hit(j) {
                bits |= 1 << s;
            }
            j += 1;
        }
        s += 1;
    }
    HitMask {
        bits,
        steps: mask.steps,
    }
}
