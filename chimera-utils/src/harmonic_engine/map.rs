//! The map: a 5×5 field of spectra, made by formulas only (no third-party
//! data), stored as 8-bit log amplitudes. Rows (Y) are families, ordered so
//! neighbours share a trait: BELL, ORGAN, REED, BRASS/BOWED, VOICE. Columns
//! (X) step brightness, dark to bright. X/Y mix the four nearest nodes
//! bilinearly in the byte (dB) domain; only amplitudes live here, never
//! frequencies.

use super::MAX;

/// The partials a node stores; above them a frame reads the top octave's
/// power law (`voice::fit`).
pub const MAP_N: usize = 64;

/// A stored strength: 0 silent, else (n − 255) × 0.375 dB (−95.6..0 dB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogAmp(pub u8);

impl LogAmp {
    pub const STEP_DB: f32 = 0.375;

    /// `db` ≤ 0 relative to the node's peak; quieter than the floor is silent.
    pub fn from_db(db: f32) -> Self {
        let n = 255.0 + db / Self::STEP_DB;
        Self(if n < 0.5 {
            0
        } else {
            n.round().min(255.0) as u8
        })
    }
}

/// One node: a strength per partial, its peak at 255.
#[derive(Clone, Copy)]
pub struct NodeSpectrum([LogAmp; MAP_N]);

impl NodeSpectrum {
    /// From linear amplitudes (any scale): peak-normalised, then quantised.
    fn from_linear(a: &[f32; MAP_N]) -> Self {
        let pk = a.iter().fold(0.0f32, |m, &v| m.max(v));
        assert!(pk > 0.0, "a node needs one partial");
        let mut n = [LogAmp(0); MAP_N];
        for (n, &v) in n.iter_mut().zip(a) {
            *n = if v > 0.0 {
                LogAmp::from_db(20.0 * (v / pk).log10())
            } else {
                LogAmp(0)
            };
        }
        Self(n)
    }

    pub fn bytes(&self) -> [u8; MAP_N] {
        self.0.map(|l| l.0)
    }
}

pub const ROWS: [&str; 5] = ["BELL", "ORGAN", "REED", "BRASS/BOWED", "VOICE"];

/// Each node's formula, one line each (its provenance): the original map.
pub const FORMULAS: [[&str; 5]; 5] = [
    [
        "sparse set 1 2 3 5 7 10 14 19 26 35 47 63, k^-2.2",
        "the same set, k^-1.6",
        "the same set, k^-1.1",
        "the same set, k^-0.7",
        "the same set, k^-0.35",
    ],
    [
        "drawbars 8' 4' at 8 3 (flute)",
        "drawbars 8' 4' 2-2/3' 2' at 8 6 4 2",
        "drawbars 8 8 6 6 0 0 4 (jazz)",
        "drawbars 8 8 8 8 4 4 6, mixture 1-3/5' at 3",
        "all drawbars out, mixture ranks to the 64th",
    ],
    [
        "clarinet low register: odd k^-1.2, even -30 dB, rolled off",
        "clarinet upper register: odd k^-1.3, even -18 dB",
        "bassoon: k^-1 times a bump at the 4th",
        "oboe: weak fundamental, bump at the 5th",
        "harmonica: k^-0.4, bump at the 10th, odd +3.5 dB",
    ],
    [
        "soft horn: k^-2, bump at the 2nd",
        "cello: k^-1.1, body bumps at the 3rd and 7th",
        "saw: 1/k",
        "trumpet: k^-0.6, bump at the 6th",
        "buzzy brass: k^-0.3, bump at the 14th",
    ],
    [
        "vowel U (Peterson-Barney male means 300/870/2240 Hz) at C3, -6 dB/oct source",
        "vowel O (570/840/2410 Hz) at C3",
        "vowel A (730/1090/2440 Hz) at C3",
        "vowel E (530/1840/2480 Hz) at C3",
        "vowel I (270/2290/3010 Hz) at C3",
    ],
];

/// The brighter map: every power law rolls off less (BELL by 0.15..0.7,
/// REED and BRASS/BOWED by 0.2..0.5 in the exponent, VOICE's source from
/// −6 to −3 dB/oct); ORGAN, which has no roll-off law, pulls its upper
/// drawbars out further and adds mixture ranks.
pub const FORMULAS_BRIGHT: [[&str; 5]; 5] = [
    [
        "sparse set 1 2 3 5 7 10 14 19 26 35 47 63, k^-1.5",
        "the same set, k^-1.1",
        "the same set, k^-0.75",
        "the same set, k^-0.45",
        "the same set, k^-0.2",
    ],
    [
        "drawbars 8' 4' 2' at 8 4 2 (flute with a 2')",
        "drawbars 8' 4' 2-2/3' 2' 1' at 8 6 5 4 2",
        "drawbars 8 8 6 6 2 2 5, mixture 1-3/5' at 2",
        "drawbars 8 8 8 8 5 5 7, mixtures to the 24th at 5..3",
        "all drawbars out, mixture ranks to the 64th one drawbar louder, 28th 36th 56th added",
    ],
    [
        "clarinet low register: odd k^-1.0, even -30 dB, rolled off (k/30)",
        "clarinet upper register: odd k^-1.1, even -18 dB",
        "bassoon: k^-0.8 times a bump at the 4th",
        "oboe: weak fundamental, k^-0.3, bump at the 5th",
        "harmonica: k^-0.2, bump at the 10th, odd +3.5 dB",
    ],
    [
        "soft horn: k^-1.5, bump at the 2nd",
        "cello: k^-0.8, body bumps at the 3rd and 7th",
        "bright saw: k^-0.75",
        "trumpet: k^-0.2, bump at the 6th",
        "buzzy brass: k^-0.1, bump at the 14th",
    ],
    [
        "vowel U at C3, -3 dB/oct source",
        "vowel O at C3, -3 dB/oct source",
        "vowel A at C3, -3 dB/oct source",
        "vowel E at C3, -3 dB/oct source",
        "vowel I at C3, -3 dB/oct source",
    ],
];

/// A Gaussian bump over log2 of the partial number, centred on partial `c`.
fn bump(k: f32, c: f32, sigma_oct: f32) -> f32 {
    let d = (k / c).log2() / sigma_oct;
    (-0.5 * d * d).exp()
}

/// A cascade of two-pole resonances: |H(f)| at `f` for formants `(F, bandwidth)`.
fn formants(f: f32, fs: &[(f32, f32)]) -> f32 {
    fs.iter()
        .map(|&(c, bw)| c * c / ((c * c - f * f).powi(2) + (f * bw).powi(2)).sqrt())
        .product()
}

/// The reference note of the VOICE row: its formants sit right at C3.
const VOICE_REF_HZ: f32 = 130.81;

fn node_linear(row: usize, col: usize, bright: bool) -> [f32; MAP_N] {
    let mut a = [0.0f32; MAP_N];
    let kf = |i: usize| (i + 1) as f32;
    match row {
        0 => {
            // one sparse set for the row, so the log-domain mix between
            // neighbours is a power law too: the centroid rises evenly
            const SET: [usize; 12] = [1, 2, 3, 5, 7, 10, 14, 19, 26, 35, 47, 63];
            let s = if bright {
                [1.5, 1.1, 0.75, 0.45, 0.2]
            } else {
                [2.2, 1.6, 1.1, 0.7, 0.35]
            }[col];
            for k in SET {
                a[k - 1] = (k as f32).powf(-s);
            }
        }
        1 => {
            // footages over an 8' fundamental: 8 4 2-2/3 2 1-3/5 1-1/3 1
            const H: [usize; 7] = [1, 2, 3, 4, 5, 6, 8];
            const REG: [[u8; 7]; 5] = [
                [8, 3, 0, 0, 0, 0, 0],
                [8, 6, 4, 2, 0, 0, 0],
                [8, 8, 6, 6, 0, 0, 4],
                [8, 8, 8, 8, 4, 4, 6],
                [8, 8, 8, 8, 8, 8, 8],
            ];
            // mixture ranks: (harmonic, drawbar)
            const MIX: [&[(usize, u8)]; 5] = [
                &[],
                &[],
                &[],
                &[(10, 3), (12, 3), (16, 2)],
                &[
                    (10, 7),
                    (12, 7),
                    (16, 6),
                    (20, 6),
                    (24, 5),
                    (32, 5),
                    (40, 4),
                    (48, 4),
                    (64, 3),
                ],
            ];
            const REG_B: [[u8; 7]; 5] = [
                [8, 4, 0, 2, 0, 0, 0],
                [8, 6, 5, 4, 0, 0, 2],
                [8, 8, 6, 6, 2, 2, 5],
                [8, 8, 8, 8, 5, 5, 7],
                [8, 8, 8, 8, 8, 8, 8],
            ];
            const MIX_B: [&[(usize, u8)]; 5] = [
                &[],
                &[],
                &[(10, 2)],
                &[(10, 5), (12, 5), (16, 4), (20, 4), (24, 3)],
                &[
                    (10, 8),
                    (12, 8),
                    (16, 7),
                    (20, 7),
                    (24, 6),
                    (28, 5),
                    (32, 6),
                    (36, 4),
                    (40, 5),
                    (48, 5),
                    (56, 3),
                    (64, 4),
                ],
            ];
            let (reg, mix) = if bright {
                (&REG_B, &MIX_B)
            } else {
                (&REG, &MIX)
            };
            // a drawbar step is 3 dB; 0 is off
            let lvl = |bar: u8| 10f32.powf((bar as f32 - 8.0) * 3.0 / 20.0);
            for (h, &bar) in H.iter().zip(&reg[col]) {
                if bar > 0 {
                    a[h - 1] = lvl(bar);
                }
            }
            for &(h, bar) in mix[col] {
                a[h - 1] = lvl(bar);
            }
        }
        2 if bright => {
            for (i, a) in a.iter_mut().enumerate() {
                let k = kf(i);
                let odd = (i + 1) % 2 == 1;
                *a = match col {
                    0 => k.powf(-1.0) * if odd { 1.0 } else { 0.0316 } * (-k / 30.0).exp(),
                    1 => k.powf(-1.1) * if odd { 1.0 } else { 0.126 },
                    2 => k.powf(-0.8) * (0.3 + bump(k, 4.0, 0.5)),
                    3 => k.powf(-0.3) * (0.1 + bump(k, 5.0, 0.6)) * if i == 0 { 0.3 } else { 1.0 },
                    _ => k.powf(-0.2) * (0.15 + bump(k, 10.0, 0.7)) * if odd { 1.5 } else { 1.0 },
                };
            }
        }
        3 if bright => {
            for (i, a) in a.iter_mut().enumerate() {
                let k = kf(i);
                *a = match col {
                    0 => k.powf(-1.5) * (1.0 + bump(k, 2.0, 0.5)),
                    1 => k.powf(-0.8) * (1.0 + 1.5 * bump(k, 3.0, 0.3) + bump(k, 7.0, 0.3)),
                    2 => k.powf(-0.75),
                    3 => k.powf(-0.2) * (0.3 + bump(k, 6.0, 0.6)),
                    _ => k.powf(-0.1) * (0.3 + bump(k, 14.0, 0.7)),
                };
            }
        }
        2 => {
            for (i, a) in a.iter_mut().enumerate() {
                let k = kf(i);
                let odd = (i + 1) % 2 == 1;
                *a = match col {
                    0 => k.powf(-1.2) * if odd { 1.0 } else { 0.0316 } * (-k / 20.0).exp(),
                    1 => k.powf(-1.3) * if odd { 1.0 } else { 0.126 },
                    2 => k.powf(-1.0) * (0.3 + bump(k, 4.0, 0.5)),
                    3 => k.powf(-0.6) * (0.1 + bump(k, 5.0, 0.6)) * if i == 0 { 0.3 } else { 1.0 },
                    _ => k.powf(-0.4) * (0.15 + bump(k, 10.0, 0.7)) * if odd { 1.5 } else { 1.0 },
                };
            }
        }
        3 => {
            for (i, a) in a.iter_mut().enumerate() {
                let k = kf(i);
                *a = match col {
                    0 => k.powf(-2.0) * (1.0 + bump(k, 2.0, 0.5)),
                    1 => k.powf(-1.1) * (1.0 + 1.5 * bump(k, 3.0, 0.3) + bump(k, 7.0, 0.3)),
                    2 => 1.0 / k,
                    3 => k.powf(-0.6) * (0.3 + bump(k, 6.0, 0.6)),
                    _ => k.powf(-0.3) * (0.3 + bump(k, 14.0, 0.7)),
                };
            }
        }
        _ => {
            const VOWELS: [[f32; 3]; 5] = [
                [300.0, 870.0, 2240.0],
                [570.0, 840.0, 2410.0],
                [730.0, 1090.0, 2440.0],
                [530.0, 1840.0, 2480.0],
                [270.0, 2290.0, 3010.0],
            ];
            let v = VOWELS[col];
            let fs = [
                (v[0], 90.0),
                (v[1], 110.0),
                (v[2], 170.0),
                (3300.0, 250.0),
                (3750.0, 300.0),
            ];
            for (i, a) in a.iter_mut().enumerate() {
                let k = kf(i);
                *a = formants(k * VOICE_REF_HZ, &fs) / if bright { k.sqrt() } else { k };
            }
        }
    }
    a
}

pub struct Map {
    /// [row (Y)][col (X)]
    nodes: [[NodeSpectrum; 5]; 5],
}

impl Map {
    fn build(bright: bool) -> Self {
        Self {
            nodes: core::array::from_fn(|r| {
                core::array::from_fn(|c| NodeSpectrum::from_linear(&node_linear(r, c, bright)))
            }),
        }
    }

    /// The original map, or the brighter one.
    pub fn get(bright: bool) -> &'static Map {
        static M: [std::sync::OnceLock<Map>; 2] =
            [std::sync::OnceLock::new(), std::sync::OnceLock::new()];
        M[bright as usize].get_or_init(|| Map::build(bright))
    }

    pub fn node(&self, row: usize, col: usize) -> &NodeSpectrum {
        &self.nodes[row][col]
    }

    /// The first `n` (≤ `MAP_N`) partials' levels in dB at (x, y) ∈ [0, 1]²: the four
    /// nearest nodes mixed bilinearly in the byte domain. Silent partials
    /// read `f32::NEG_INFINITY`.
    pub fn mix(&self, x: f32, y: f32, n: usize, db: &mut [f32; MAX]) {
        assert!(n <= MAP_N);
        let split = |v: f32| {
            let p = v.clamp(0.0, 1.0) * 4.0;
            let i = (p as usize).min(3);
            (i, p - i as f32)
        };
        let ((cx, fx), (cy, fy)) = (split(x), split(y));
        let w = [
            (1.0 - fx) * (1.0 - fy),
            fx * (1.0 - fy),
            (1.0 - fx) * fy,
            fx * fy,
        ];
        let nd = [
            &self.nodes[cy][cx].0,
            &self.nodes[cy][cx + 1].0,
            &self.nodes[cy + 1][cx].0,
            &self.nodes[cy + 1][cx + 1].0,
        ];
        for (k, d) in db.iter_mut().enumerate().take(n) {
            let m: f32 = (0..4).map(|j| w[j] * nd[j][k].0 as f32).sum();
            *d = if m < 0.5 {
                f32::NEG_INFINITY
            } else {
                (m - 255.0) * LogAmp::STEP_DB
            };
        }
    }
}

/// The amplitude-weighted mean partial number of `n` partials at (x, y).
pub fn centroid(bright: bool, x: f32, y: f32, n: usize) -> f32 {
    let mut db = [0.0; MAX];
    Map::get(bright).mix(x, y, n, &mut db);
    let (mut s, mut sk) = (0.0f32, 0.0f32);
    for (i, d) in db.iter().enumerate().take(n) {
        let a = 10f32.powf(d / 20.0);
        s += a;
        sk += a * (i + 1) as f32;
    }
    sk / s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_node_peaks_at_255_and_is_not_empty() {
        for m in [Map::get(false), Map::get(true)] {
            for r in 0..5 {
                for c in 0..5 {
                    let b = m.node(r, c).bytes();
                    assert_eq!(b.iter().max(), Some(&255), "{r},{c}");
                }
            }
        }
    }

    /// VOICE is out: its X runs U O A E I by F2, as the design lays it
    /// out, but /i/'s low F1 puts its centroid under /e/'s (for the ear).
    #[test]
    fn centroid_rises_along_x_in_every_row_but_voice() {
        for bright in [false, true] {
            for (r, row) in ROWS.iter().enumerate().take(4) {
                let y = r as f32 / 4.0;
                let c: Vec<f32> = (0..=16)
                    .map(|i| centroid(bright, i as f32 / 16.0, y, MAP_N))
                    .collect();
                for w in c.windows(2) {
                    assert!(w[1] >= w[0] - 1e-3, "{row} bright {bright}: {c:?}");
                }
            }
        }
    }
}
