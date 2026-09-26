//! The algorithm diagram's layout, computed from the table: rows by depth
//! (longest path to a carrier, carriers at the bottom), carriers in operator
//! order, each modulator over the mean of its targets, rows packed so no two
//! nodes overlap. A deep chain switches to a compact spacing.

use crate::dsp::algo::algorithms::Algorithm;
use crate::dsp::algo::plan::OPS;
use crate::ui::theme;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AlgLayout {
    pub pos: [(i32, i32); OPS],
    pub r: i32,
}

const STEP_X: f32 = 30.0;
/// Node radius and row step; the compact pair fits six rows in the band.
const NORMAL: (i32, i32) = (7, 17);
const COMPACT: (i32, i32) = (5, 11);
const MAX_NORMAL_ROWS: usize = 4;

pub fn layout(alg: &Algorithm) -> AlgLayout {
    let mut depth = [0usize; OPS];
    for op in 0..OPS {
        if alg.carriers & (1 << op) == 0 {
            depth[op] = (0..op)
                .filter(|&t| alg.mods[op] & (1 << t) != 0)
                .map(|t| depth[t] + 1)
                .max()
                .unwrap_or(0);
        }
    }
    let rows = depth.iter().max().map_or(1, |d| d + 1);
    let (r, step_y) = if rows > MAX_NORMAL_ROWS {
        COMPACT
    } else {
        NORMAL
    };
    let mut x = [0.0f32; OPS];
    for row in 0..rows {
        let mut members = [(0.0f32, 0usize); OPS];
        let mut n = 0;
        for op in (0..OPS).filter(|&op| depth[op] == row) {
            let want = if row == 0 {
                (0..op).filter(|&c| depth[c] == 0).count() as f32
            } else {
                let (sum, count) = (0..op)
                    .filter(|&t| alg.mods[op] & (1 << t) != 0)
                    .fold((0.0, 0.0), |(s, c), t| (s + x[t], c + 1.0));
                sum / count
            };
            members[n] = (want, op);
            n += 1;
        }
        let row_members = &mut members[..n];
        row_members.sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut placed = [0.0f32; OPS];
        for k in 0..n {
            placed[k] = if k == 0 {
                row_members[0].0
            } else {
                row_members[k].0.max(placed[k - 1] + 1.0)
            };
        }
        let want_sum: f32 = row_members.iter().map(|m| m.0).sum();
        let shift = (want_sum - placed[..n].iter().sum::<f32>()) / n as f32;
        for k in 0..n {
            x[row_members[k].1] = placed[k] + shift;
        }
    }
    let (lo, hi) = x
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
    let mid = (lo + hi) / 2.0;
    let bottom = theme::VIZ_BAND_MID + (rows as i32 - 1) * step_y / 2;
    AlgLayout {
        pos: core::array::from_fn(|op| {
            (
                theme::SCREEN_W / 2 + ((x[op] - mid) * STEP_X) as i32,
                bottom - depth[op] as i32 * step_y,
            )
        }),
        r,
    }
}

/// `a` moving to `b` as MORPH goes from 0 to 1.
pub fn blend(a: &AlgLayout, b: &AlgLayout, m: f32) -> AlgLayout {
    let lerp = |p: i32, q: i32| p + ((q - p) as f32 * m) as i32;
    AlgLayout {
        pos: core::array::from_fn(|i| (lerp(a.pos[i].0, b.pos[i].0), lerp(a.pos[i].1, b.pos[i].1))),
        r: a.r.min(b.r),
    }
}
