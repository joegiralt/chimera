// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filter's jacks (filter-types spec § 2, § 6): FM3 IN and QM IN pick a
//! matrix source, or on ALGO an audio-rate tap of OP 5 or OP 6.

use core::mem::MaybeUninit;

use crate::block::DiskCode;
use crate::hw::BLOCK_SIZE;
use crate::modulation::{FixedCol, ModSource};
use crate::params::EngineType;

/// An ALGO operator that also feeds the filter at audio rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TapOp {
    Op5 = 0,
    Op6 = 1,
}

impl TapOp {
    pub const ALL: [TapOp; 2] = [TapOp::Op5, TapOp::Op6];

    /// The operator's index in the ALGO voice.
    pub const fn op_index(self) -> usize {
        match self {
            TapOp::Op5 => 4,
            TapOp::Op6 => 5,
        }
    }

    pub const fn tag(self) -> &'static str {
        match self {
            TapOp::Op5 => "OP5",
            TapOp::Op6 => "OP6",
        }
    }

    /// The full name the matrix readout uses, as `ModSource::name`.
    pub const fn name(self) -> &'static str {
        match self {
            TapOp::Op5 => "OP 5",
            TapOp::Op6 => "OP 6",
        }
    }
}

/// What a jack is patched to. `ModSource` stays the 8-row matrix type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jack {
    Mod(ModSource),
    Tap(TapOp),
}

const fn m(s: ModSource) -> Jack {
    Jack::Mod(s)
}

const OP5: Jack = Jack::Tap(TapOp::Op5);
const OP6: Jack = Jack::Tap(TapOp::Op6);

impl Jack {
    /// Every jack, in disk-code order.
    pub const ALL: [Jack; 10] = [
        m(ModSource::Env1),
        m(ModSource::Lfo1),
        m(ModSource::Env2),
        m(ModSource::Env3),
        m(ModSource::Lfo2),
        m(ModSource::Lfo3),
        m(ModSource::Vel),
        m(ModSource::Note),
        OP5,
        OP6,
    ];

    /// FM3 IN's choices: ENV 1 and NOTE are FM1's and FM2's.
    pub const FM3_CHOICES: [Jack; 8] = [
        m(ModSource::Lfo1),
        m(ModSource::Lfo2),
        m(ModSource::Lfo3),
        m(ModSource::Env2),
        m(ModSource::Env3),
        m(ModSource::Vel),
        OP5,
        OP6,
    ];

    /// QM IN's choices: every matrix source, then the taps.
    pub const QM_CHOICES: [Jack; 10] = Jack::ALL;

    /// What the jack acts as on `engine`: a tap off ALGO is `default`, and
    /// the stored tap is kept for a return to ALGO (L-new-2).
    pub fn effective(self, engine: EngineType, default: Jack) -> Jack {
        match self {
            Jack::Tap(_) if engine != EngineType::Algo => default,
            _ => self,
        }
    }

    pub const fn tag(self) -> &'static str {
        match self {
            Jack::Mod(s) => s.tag(),
            Jack::Tap(t) => t.tag(),
        }
    }
}

impl DiskCode for Jack {
    fn disk_code(self) -> u8 {
        match self {
            Jack::Mod(s) => s.disk_code(),
            Jack::Tap(t) => 8 + t as u8,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            Jack::Mod(s) => s.disk_ident(),
            Jack::Tap(t) => t.tag(),
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        Jack::ALL.into_iter().find(|j| j.disk_code() == c)
    }
}

const fn tags<const N: usize>(j: [Jack; N]) -> [&'static str; N] {
    let mut t = [""; N];
    let mut i = 0;
    while i < N {
        t[i] = j[i].tag();
        i += 1;
    }
    t
}

/// FM3 IN's and QM IN's values on screen.
pub static FM3_IN_NAMES: [&str; 8] = tags(Jack::FM3_CHOICES);
pub static QM_IN_NAMES: [&str; 10] = tags(Jack::QM_CHOICES);

/// `j`'s index in `choices`, if it is one.
fn index_of(choices: &[Jack], j: Jack) -> Option<u8> {
    choices.iter().position(|&c| c == j).map(|i| i as u8)
}

/// FM3 IN and QM IN in one byte: their indexes into `FM3_CHOICES` (high
/// nibble) and `QM_CHOICES` (low). Only a choice is representable, and the
/// voice chain keeps its 2,112 B (ADR 0101): two `Jack` fields would pad
/// `FilterParams` by 4 B in every voice's snapshot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct JackPair(u8);

const _: () = assert!(Jack::FM3_CHOICES.len() <= 16 && Jack::QM_CHOICES.len() <= 16);

/// `j`'s index in `choices`, at compile time: a default that isn't a
/// choice fails the build.
const fn choice(choices: &[Jack], j: Jack) -> u8 {
    let mut i = 0;
    while i < choices.len() {
        let same = match (choices[i], j) {
            (Jack::Mod(a), Jack::Mod(b)) => a as u8 == b as u8,
            (Jack::Tap(a), Jack::Tap(b)) => a as u8 == b as u8,
            _ => false,
        };
        if same {
            return i as u8;
        }
        i += 1;
    }
    panic!("not a choice")
}

impl JackPair {
    /// FM3 IN on LFO 1, QM IN on ENV 3 (design answer 6).
    pub const DEFAULT: JackPair = JackPair(
        choice(&Jack::FM3_CHOICES, Jack::Mod(ModSource::Lfo1)) << 4
            | choice(&Jack::QM_CHOICES, Jack::Mod(ModSource::Env3)),
    );

    pub const fn fm3(self) -> Jack {
        Jack::FM3_CHOICES[(self.0 >> 4) as usize]
    }

    pub const fn qm(self) -> Jack {
        Jack::QM_CHOICES[(self.0 & 0xF) as usize]
    }

    /// FM3 IN's index into `FM3_CHOICES`, and QM IN's into `QM_CHOICES`.
    pub const fn fm3_index(self) -> u8 {
        self.0 >> 4
    }

    pub const fn qm_index(self) -> u8 {
        self.0 & 0xF
    }

    /// With FM3 IN on `j`; `None` if `j` isn't one of its choices.
    pub fn with_fm3(self, j: Jack) -> Option<Self> {
        index_of(&Jack::FM3_CHOICES, j).map(|i| JackPair(i << 4 | self.0 & 0xF))
    }

    /// With QM IN on `j`; `None` if `j` isn't one of its choices.
    pub fn with_qm(self, j: Jack) -> Option<Self> {
        index_of(&Jack::QM_CHOICES, j).map(|i| JackPair(self.0 & 0xF0 | i))
    }
}

impl core::fmt::Debug for JackPair {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JackPair")
            .field("fm3", &self.fm3())
            .field("qm", &self.qm())
            .finish()
    }
}

/// The taps' routes (spec § 6): an amount per (tap, fixed column), or
/// none. The columns are `FixedCol`, so a tap into an operator's pitch or
/// level can't be written. An absent route holds `ABSENT`, which no amount
/// is (±127): 8 B, so the voice chain keeps its 2,112 B (ADR 0101), where a
/// presence byte would cost 8 more in every voice's snapshot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TapRoutes {
    amount: [[i8; 4]; 2],
}

const ABSENT: i8 = i8::MIN;

impl Default for TapRoutes {
    fn default() -> Self {
        TapRoutes {
            amount: [[ABSENT; 4]; 2],
        }
    }
}

impl TapRoutes {
    pub fn get(&self, op: TapOp, c: FixedCol) -> Option<i8> {
        let a = self.amount[op as usize][c.index()];
        (a != ABSENT).then_some(a)
    }

    /// Sets (and so creates) the route; `a` is held to ±127.
    pub fn set(&mut self, op: TapOp, c: FixedCol, a: i8) {
        self.amount[op as usize][c.index()] = a.max(-127);
    }

    pub fn clear(&mut self, op: TapOp, c: FixedCol) {
        self.amount[op as usize][c.index()] = ABSENT;
    }

    /// A tap moves `c`: some route into it with a nonzero amount.
    pub fn moving(&self, c: FixedCol) -> bool {
        TapOp::ALL
            .into_iter()
            .any(|op| self.get(op, c).is_some_and(|a| a != 0))
    }

    /// The ops some moving route reads: the ones the engine must capture.
    pub fn wanted(&self) -> [bool; 2] {
        TapOp::ALL.map(|op| {
            FixedCol::ALL
                .into_iter()
                .any(|c| self.get(op, c).is_some_and(|a| a != 0))
        })
    }

    /// Every cell's amount, op-major in `FixedCol` order; 0 when absent.
    pub fn amounts(&self) -> [[i8; 4]; 2] {
        self.amount
            .map(|o| o.map(|a| if a == ABSENT { 0 } else { a }))
    }

    pub fn is_empty(&self) -> bool {
        self.entries().next().is_none()
    }

    /// Every present route, op-major.
    pub fn entries(&self) -> impl Iterator<Item = (TapOp, FixedCol, i8)> + '_ {
        TapOp::ALL.into_iter().flat_map(move |op| {
            FixedCol::ALL
                .into_iter()
                .filter_map(move |c| self.get(op, c).map(|a| (op, c, a)))
        })
    }
}

/// A CUTOFF route's amount as a FREQ tap's, keeping its depth in octaves
/// (M-new-2): 100 % is 10 octaves on a route, 4 on a tap.
pub fn to_tap_amount(a: i8) -> i8 {
    div_round(a as i16 * 10, 4).clamp(-127, 127) as i8
}

/// A FREQ tap's amount as a CUTOFF route's.
pub fn from_tap_amount(a: i8) -> i8 {
    div_round(a as i16 * 4, 10) as i8
}

/// `n / d` rounded half away from zero (`d > 0`).
const fn div_round(n: i16, d: i16) -> i16 {
    (n + n.signum() * (d / 2)) / d
}

impl core::fmt::Debug for TapRoutes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.entries().map(|(op, c, a)| (op.tag(), c.tag(), a)))
            .finish()
    }
}

/// What the taps add to a voice on ALGO, cycles a sample, each counted on
/// the bench ELF at 1879169d (2026-10-07, `tools/loop_count.py`, cycles =
/// ceil5(instructions × 1.606 + 12 × `vdiv`)). A FREQ or a Q tap also makes
/// the block a moving one (`FilterKind::move_cost`, billed once).
///
/// `TapShape::apply`, both lanes and both ops in one loop, whenever a FREQ
/// or Q tap is present: 96 instructions for 2 samples, 0 `vdiv`, + 0.45 of
/// block: ceil5(48.45 × 1.606) = 80. Once, however many taps. The unit's
/// worst block reads TAP FREQ − 1 OP 201 over a billed 170 (883b, ADR
/// 0262): 115.
pub const TAP_SHAPE_COST: crate::hw::Cost = crate::hw::Cost(115);
/// The tap-VCA, both ops in one loop (`tap_vca_probe`): 61 instructions
/// for 4 samples, 0 `vdiv`, + 0.70 of block: ceil5(15.95 × 1.606) = 30.
/// Once, however many VCA taps.
pub const TAP_VCA_COST: crate::hw::Cost = crate::hw::Cost(30);
/// A tapped FOLD's per-sample loop over its moving one (`fold::run`). The
/// unit reads 146 against a billed 140 (2026-10-19, rev V, ROUTING bench,
/// run 2, `/VOICE` slope): 51. The host count was 45 (#642). On top of
/// FOLD's `cost` and `ramp_cost`, which an FX tap under FOLD always bills;
/// under any other type the tap is inert and unbilled.
pub const TAP_FOLD_COST: crate::hw::Cost = crate::hw::Cost(51);
/// Each wanted op's capture in the kernel, a compare-and-select clamp and
/// a store a sample: 70 instructions for 8 samples in `render_ops`, 0
/// `vdiv`: ceil5(8.75 × 1.606) = 15. Per op with any nonzero tap.
pub const TAP_CAPTURE_COST: crate::hw::Cost = crate::hw::Cost(15);

/// Each tapped op's output this block, clamped to ±1 (spec § 6): the
/// engine fills an op's row only when it is wanted, and a silent op's row
/// is zeros. 512 B, on the audio stack, zeroed only while an op is
/// wanted: an untapped block (every block off ALGO) skips the fill.
pub struct TapRows {
    want: [bool; 2],
    rows: MaybeUninit<[[f32; BLOCK_SIZE]; 2]>,
}

impl TapRows {
    #[inline(always)]
    pub fn new(want: [bool; 2]) -> Self {
        let mut t = Self {
            want,
            rows: MaybeUninit::uninit(),
        };
        if want != [false; 2] {
            t.rows.write([[0.0; BLOCK_SIZE]; 2]);
        }
        t
    }

    pub fn want(&self) -> [bool; 2] {
        self.want
    }

    /// Both rows, while an op is wanted; `None` when none is (never written).
    #[inline(always)]
    pub fn rows(&self) -> Option<&[[f32; BLOCK_SIZE]; 2]> {
        // SAFETY: `new` zeroed the rows whenever an op is wanted, and
        // `want` is private and never changed after.
        (self.want != [false; 2]).then(|| unsafe { self.rows.assume_init_ref() })
    }

    /// Each wanted op's row to write.
    #[inline(always)]
    pub fn rows_mut(&mut self) -> [Option<&mut [f32; BLOCK_SIZE]>; 2] {
        let want = self.want;
        if want == [false; 2] {
            return [None, None];
        }
        // SAFETY: as `rows`: zeroed by `new` while an op is wanted.
        let [r0, r1] = unsafe { self.rows.assume_init_mut() };
        [want[0].then_some(r0), want[1].then_some(r1)]
    }
}

/// One destination's taps for a block: each op's `amount/127` (0 when
/// absent), ramped `from → to`, the block's last sample landing on `to`.
#[derive(Clone, Copy, Debug)]
pub struct TapLane<'a> {
    pub rows: &'a [[f32; BLOCK_SIZE]; 2],
    pub from: [f32; 2],
    pub to: [f32; 2],
}

impl TapLane<'_> {
    /// Each op's amount a sample is stepped by, adding from `from`: the
    /// block's last sample lands on `to`.
    #[inline(always)]
    pub fn step(&self) -> [f32; 2] {
        [0, 1].map(|i| (self.to[i] - self.from[i]) * (1.0 / BLOCK_SIZE as f32))
    }

    /// Σ over ops of the ramped amount × the op's row, at sample `n`.
    #[inline(always)]
    pub fn at(&self, n: usize) -> f32 {
        let t = (n + 1) as f32;
        let d = self.step();
        let a = |i: usize| self.from[i] + d[i] * t;
        a(0) * self.rows[0][n] + a(1) * self.rows[1][n]
    }

    /// The largest `|amount|` either op reaches this block: the most `at`
    /// can move, rows being ±1.
    pub fn reach(&self) -> f32 {
        (0..2).fold(0.0, |m, i| m + self.from[i].abs().max(self.to[i].abs()))
    }
}

/// The tap-VCA (spec § 6), AM to ring: `x·Π(1 − |a| + a·row)` over the ops,
/// an absent op's factor 1. Before the DC blocker, which takes the DC a
/// self-correlated tap leaves.
pub fn tap_vca(buf: &mut [f32; BLOCK_SIZE], lane: &TapLane) {
    let (d, [r0, r1]) = (lane.step(), lane.rows);
    let mut a = lane.from;
    for (n, x) in buf.iter_mut().enumerate() {
        a = [a[0] + d[0], a[1] + d[1]];
        let f0 = 1.0 - a[0].abs() + a[0] * r0[n];
        let f1 = 1.0 - a[1].abs() + a[1] * r1[n];
        *x *= f0 * f1;
    }
}
