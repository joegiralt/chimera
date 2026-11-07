//! Juno-style BBD chorus, stereo (FX diet spec § Chorus): each line has a
//! triangle LFO and two read taps, the normal one on the left and one on
//! the inverted LFO on the right. The mono sum does not cancel.
//!
//! Mode I:  triangle LFO at 0.513 Hz, depth ~1.7ms
//! Mode II: triangle LFO at 0.863 Hz, depth ~2.3ms
//! Mode I+II: both lines; each side averages its two taps
//!
//! FLANGER and PHASER are the slot's other TYPEs (#794), wet only as the
//! chorus is: the mixer's dry beside the return makes the notches.
//! FLANGER: each line a short delay swept by one triangle, the right side
//! on the inverted one, its tap fed back into its own line (the feedback
//! comb, Dattorro, "Effect Design Part 2: Delay-Line Modulation and
//! Chorus", JAES 45(10) 1997, § 3; DAFX 2nd ed., Zölzer, § 3.3.1).
//! PHASER: six first-order all-passes per side (bilinear, `a = (t − 1) /
//! (t + 1)`, `t = tan(π fc / fs)`), their notch swept exponentially by a
//! triangle and fed back from the chain's end (Smith, "An Allpass Approach
//! to Digital Phasing and Flanging", ICMC 1984; DAFX § 2.2.3). Its
//! coefficients are set at the block's ends and ramped across it.

use crate::block::{Block, Change, DiskCode, ParamId, ParamSpec, Smoothing, ValFmt, apply_code};
use crate::dsp::Stereo;
use crate::dsp::ease::{Ease, at, ease_coeff, step_of};
use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;

const MAX_CHORUS_DELAY: usize = 2048; // ~42ms at 48kHz, plenty for chorus

/// MODE's names, in `ChorusMode`'s order (display only).
pub const MODE_NAMES: [&str; 6] = ["OFF", "I", "II", "I+II", "FLANGER", "PHASER"];

/// Chorus mode selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ChorusMode {
    Off = 0,
    JunoI = 1,    // Slow, subtle
    JunoII = 2,   // Faster, wider
    JunoBoth = 3, // Both LFOs (thickest)
    Flanger = 4,
    Phaser = 5,
}

impl ChorusMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 6 {
            0 => ChorusMode::Off,
            1 => ChorusMode::JunoI,
            2 => ChorusMode::JunoII,
            3 => ChorusMode::JunoBoth,
            4 => ChorusMode::Flanger,
            _ => ChorusMode::Phaser,
        }
    }
}

impl DiskCode for ChorusMode {
    fn disk_code(self) -> u8 {
        match self {
            ChorusMode::Off => 0,
            ChorusMode::JunoI => 1,
            ChorusMode::JunoII => 2,
            ChorusMode::JunoBoth => 3,
            ChorusMode::Flanger => 4,
            ChorusMode::Phaser => 5,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            ChorusMode::Off => "OFF",
            ChorusMode::JunoI => "JUNO_I",
            ChorusMode::JunoII => "JUNO_II",
            ChorusMode::JunoBoth => "JUNO_BOTH",
            ChorusMode::Flanger => "FLANGER",
            ChorusMode::Phaser => "PHASER",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(ChorusMode::Off),
            1 => Some(ChorusMode::JunoI),
            2 => Some(ChorusMode::JunoII),
            3 => Some(ChorusMode::JunoBoth),
            4 => Some(ChorusMode::Flanger),
            5 => Some(ChorusMode::Phaser),
            _ => None,
        }
    }
}

/// Chorus parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChorusParams {
    /// Mode: 0=off, 1=Juno I, 2=Juno II, 3=Both, 4=flanger, 5=phaser
    pub mode: u8,
    /// Rate multiplier (0.5..2.0, centered at 1.0)
    pub rate: f32,
    /// Depth multiplier (0.5..2.0, centered at 1.0)
    pub depth: f32,
    /// Dry/wet mix (0..1)
    pub mix: f32,
}

impl ChorusParams {
    /// Juno I at unity return (sends spec § 4): the send sets the amount.
    pub const DEFAULT: Self = Self {
        mode: ChorusMode::JunoI as u8,
        rate: 0.5,
        depth: 0.5,
        mix: 1.0,
    };
}

impl Default for ChorusParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl ChorusParams {
    /// Off when the mode is off or the mix is below audibility: the return
    /// fades out, the lines run on (ADR 0061).
    pub fn is_on(&self) -> bool {
        ChorusMode::from_u8(self.mode) != ChorusMode::Off && self.mix >= 0.001
    }

    pub const MODE: ParamId = ParamId(0);
    pub const RATE: ParamId = ParamId(1);
    pub const DEPTH: ParamId = ParamId(2);
    pub const MIX: ParamId = ParamId(3);
}

/// Chorus runs outside `Voice`, on the FX bus: nothing is modulatable.
/// Every chorus param shows the braid (ADR 0067).
const BRAID: crate::glyph::FocusGlyph =
    crate::glyph::FocusGlyph::Composite(crate::glyph::CompositeId::ChorusBraid);

pub static CHORUS_SPECS: [ParamSpec; 4] = [
    ParamSpec::choice(
        0,
        "TYPE",
        ValFmt::Names(&MODE_NAMES),
        5.0,
        1.0,
        Change::Crossfade,
    )
    .ident("MODE")
    .glyph(BRAID),
    ParamSpec::continuous(
        1,
        "RATE",
        ValFmt::Uni,
        0.0,
        1.0,
        0.5,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("RATE")
    .glyph(BRAID),
    ParamSpec::continuous(
        2,
        "DEPTH",
        ValFmt::Uni,
        0.0,
        1.0,
        0.5,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("DEPTH")
    .glyph(BRAID),
    ParamSpec::continuous(
        3,
        "MIX",
        ValFmt::Uni,
        0.0,
        1.0,
        1.0,
        1.0 / 128.0,
        false,
        Smoothing::Own,
    )
    .ident("MIX")
    .glyph(BRAID),
];

impl Block for ChorusParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &CHORUS_SPECS
    }

    #[inline(never)]
    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODE => self.mode as f32,
            Self::RATE => self.rate,
            Self::DEPTH => self.depth,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    #[inline(never)]
    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODE => self.mode = v as u8,
            Self::RATE => self.rate = v,
            Self::DEPTH => self.depth = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }

    /// MODE's code is `ChorusMode`'s: the byte's meaning to the DSP.
    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::MODE).then(|| ChorusMode::from_u8(self.mode).disk_code())
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        (id == Self::MODE).then(|| ChorusMode::from_u8(self.mode).disk_ident())
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        id == Self::MODE && apply_code(ChorusMode::from_disk_code(code), |m| self.mode = m as u8)
    }
}

/// Single BBD delay line with triangle LFO.
struct BbdLine {
    buffer: [f32; MAX_CHORUS_DELAY],
    write_pos: usize,
    /// The LFO's phase, in turns × 2^32: it wraps for free.
    lfo_phase: u32,
}

const MASK: usize = MAX_CHORUS_DELAY - 1;
const _: () = assert!(MAX_CHORUS_DELAY.is_power_of_two());

impl BbdLine {
    fn new() -> Self {
        Self {
            buffer: [0.0; MAX_CHORUS_DELAY],
            write_pos: 0,
            lfo_phase: 0,
        }
    }

    /// Writes `input`; returns the (normal, inverted) taps. `base` and
    /// `depth` are in samples, `inc` is the LFO's phase step in turns ×
    /// 2^32.
    #[inline(always)]
    fn tick(&mut self, input: f32, base: f32, depth: f32, inc: u32) -> (f32, f32) {
        self.write(input, inc);
        self.taps(base, depth)
    }

    /// Writes `input` and moves the LFO on: a line unheard stays current.
    #[inline(always)]
    fn write(&mut self, input: f32, inc: u32) {
        self.buffer[self.write_pos & MASK] = input;
        self.write_pos = (self.write_pos + 1) & MASK;
        self.lfo_phase = self.lfo_phase.wrapping_add(inc);
    }

    /// `n` writes of +0 into a line already all +0: only the heads move,
    /// each as `n` `write`s move it (the phase wraps as they do).
    #[inline(always)]
    fn skip(&mut self, n: usize, inc: u32) {
        self.write_pos = (self.write_pos + n) & MASK;
        self.lfo_phase = self.lfo_phase.wrapping_add(inc.wrapping_mul(n as u32));
    }

    /// The (normal, inverted) taps at the LFO's phase.
    #[inline(always)]
    fn taps(&self, base: f32, depth: f32) -> (f32, f32) {
        let tri = tri(self.lfo_phase);
        (self.read(base + tri * depth), self.read(base - tri * depth))
    }

    #[inline(always)]
    fn read(&self, delay: f32) -> f32 {
        // max then min: VMAXNM and VMINNM, no compare.
        let delay = delay.max(1.0).min((MAX_CHORUS_DELAY - 2) as f32);
        let d = delay as usize;
        let frac = delay - d as f32;
        let a = self.write_pos.wrapping_sub(d) & MASK;
        let b = a.wrapping_sub(1) & MASK;
        self.buffer[a] * (1.0 - frac) + self.buffer[b] * frac
    }
}

/// Triangle, 0→1→0→-1→0 over a turn of `phase` (turns × 2^32): 1 − |4v
/// − 2| a quarter turn on, v ∈ [0, 1), from the phase's distance to the
/// half turn.
#[inline(always)]
fn tri(phase: u32) -> f32 {
    let x = (phase.wrapping_add(1 << 30) ^ (1 << 31)) as i32;
    1.0 - (x as f32).abs() * (1.0 / (1u32 << 30) as f32)
}

/// FLANGER's sweep: its shortest delay, and its span at DEPTH ×1 (×0.5 to
/// ×2), ms. Its feedback.
const FL_MIN_MS: f32 = 0.25;
const FL_SPAN_MS: f32 = 2.5;
const FL_FB: f32 = 0.6;

/// PHASER's all-passes a side (three notches), its feedback, the sweep's
/// geometric centre (Hz) and its width at DEPTH ×1, octaves.
const PH_STAGES: usize = 6;
const PH_FB: f32 = 0.5;
const PH_CENTRE_HZ: f32 = 600.0;
const PH_OCTAVES: f32 = 2.5;

/// Samples of a silent send after which a fed-back tail is below −400 dB
/// (FLANGER's 5.25 ms at 0.6 a trip; PHASER's slowest pole 0.99) and the
/// lines and the all-passes are cleared: f32 never decays to +0 alone.
const TAIL: u32 = 24_000;

/// PHASER's two chains: the all-passes' states, the last outputs (the
/// feedback) and the coefficients at the last block's end.
struct Phaser {
    s: [[f32; PH_STAGES]; 2],
    y: [f32; 2],
    a: [f32; 2],
}

crate::in_place::field_list!(JunoChorus => JunoChorus { line_i, line_ii, weight, depth, zeros, phase, phaser, fed });
crate::in_place::field_list!(BbdLine => BbdLine { buffer, write_pos, lfo_phase });
crate::in_place::field_list!(Phaser => Phaser { s, y, a });

/// Juno-style chorus: mono send in, stereo wet out. Both lines are
/// written whatever the mode, so a line faded in is current; each TYPE's
/// weight (its share of the mode × MIX: I, II, FLANGER, PHASER) and
/// DEPTH ease (never snap).
pub struct JunoChorus {
    line_i: BbdLine,
    line_ii: BbdLine,
    weight: [Ease; 4],
    depth: Ease,
    /// Samples since the send was last anything but +0, counted a block at
    /// a time (saturating): at `MAX_CHORUS_DELAY`, unless `fed`, both
    /// lines are all +0 (`kern::process`'s idle skip).
    zeros: u32,
    /// FLANGER's and PHASER's LFO, turns × 2^32.
    phase: u32,
    phaser: Phaser,
    /// FLANGER or PHASER heard a sound since the last clear: the lines or
    /// the all-passes may hold a fed-back tail until `TAIL`.
    fed: bool,
}

impl Default for JunoChorus {
    fn default() -> Self {
        Self::new()
    }
}

impl JunoChorus {
    pub fn new() -> Self {
        Self {
            line_i: BbdLine::new(),
            line_ii: BbdLine::new(),
            weight: [Ease::default(); 4],
            depth: Ease::default(),
            zeros: 0,
            phase: 0,
            phaser: Phaser {
                s: [[0.0; PH_STAGES]; 2],
                y: [0.0; 2],
                a: [0.0; 2],
            },
            fed: false,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: every field (two `BbdLine { buffer: [f32; N], write_pos:
        // usize, lfo_phase: u32 }`, five `Ease { f32, bool }`, two `u32`, a
        // `Phaser` of `f32` arrays and a `bool`) is valid as zero bytes, and zero is exactly `new()`'s state; `write_bytes`
        // covers the whole slot.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    /// Send/return use (the FX bus): the wet signal × MIX, the return
    /// level, per side. At MIX 0 or mode OFF the lines are still written
    /// and the return fades out. Through the door with `bank2`; closed,
    /// silent (ADR 0213).
    pub fn process_wet(
        &mut self,
        send: &[f32; BLOCK_SIZE],
        params: &ChorusParams,
        sample_rate: u32,
        out: &mut Stereo,
    ) {
        #[cfg(feature = "bank2")]
        if crate::bank2::DOOR_CELL
            .with(|d| (d.chorus.process)(self, send, params, sample_rate, out))
            .is_none()
        {
            *out = Stereo::SILENT;
        }
        #[cfg(not(feature = "bank2"))]
        kern::process(self, send, params, sample_rate, out);
    }
}

/// The chorus's per-sample work, bank 2's (ADR 0213): bank 1 calls
/// `process` once a block.
#[repr(C)]
pub struct ChorusDoor {
    pub process: fn(&mut JunoChorus, &[f32; BLOCK_SIZE], &ChorusParams, u32, &mut Stereo),
}

impl ChorusDoor {
    pub const DOOR: ChorusDoor = ChorusDoor {
        process: kern::process,
    };
}

/// The lines' loops, linked into bank 2 (`bank2.x`, `*6chorus4kern*`).
mod kern {
    use super::*;

    // PHASER's work, here so it links into bank 2 with the loops.
    impl Phaser {
        /// One sample of `side`'s chain at coefficient `a`: each stage `y = a·v
        /// + s; s = v − a·y` (transposed direct form II).
        #[inline(always)]
        fn tick(&mut self, side: usize, x: f32, a: f32) -> f32 {
            let mut v = x + PH_FB * self.y[side];
            for s in &mut self.s[side] {
                let y = a * v + *s;
                *s = v - a * y;
                v = y;
            }
            self.y[side] = v;
            v
        }

        /// The (left, right) coefficients across a block ending at LFO `phase`
        /// with DEPTH ×`dm`: (start, per-sample step), and moves `a` to the end.
        fn ramp(&mut self, phase: u32, dm: f32, sample_rate: u32) -> ([f32; 2], [f32; 2]) {
            let swing = 0.5 * PH_OCTAVES * dm * tri(phase);
            let coef = |oct: f32| {
                let fc = PH_CENTRE_HZ * crate::dsp::filter::coef::exp2_poly(oct);
                let t = crate::dsp::filter::coef::tan_accurate(fc / sample_rate as f32);
                (t - 1.0) / (t + 1.0)
            };
            let from = self.a;
            self.a = [coef(swing), coef(-swing)];
            let step = [
                step_of((from[0], self.a[0]), BLOCK_SIZE),
                step_of((from[1], self.a[1]), BLOCK_SIZE),
            ];
            (from, step)
        }
    }

    /// The door's entry: one block, the wet × MIX into `out`.
    #[inline(never)]
    pub fn process(
        c: &mut JunoChorus,
        send: &[f32; BLOCK_SIZE],
        params: &ChorusParams,
        sample_rate: u32,
        out: &mut Stereo,
    ) {
        let mode = ChorusMode::from_u8(params.mode);
        // Each TYPE's share of the return (I, II, FLANGER, PHASER): I+II
        // averages the two lines.
        let share = match mode {
            ChorusMode::Off => [0.0, 0.0, 0.0, 0.0],
            ChorusMode::JunoI => [1.0, 0.0, 0.0, 0.0],
            ChorusMode::JunoII => [0.0, 1.0, 0.0, 0.0],
            ChorusMode::JunoBoth => [0.5, 0.5, 0.0, 0.0],
            ChorusMode::Flanger => [0.0, 0.0, 1.0, 0.0],
            ChorusMode::Phaser => [0.0, 0.0, 0.0, 1.0],
        };
        let mix = if params.mix >= 0.001 { params.mix } else { 0.0 };
        let k = ease_coeff(sample_rate);
        let w: [(f32, f32); 4] = core::array::from_fn(|j| c.weight[j].step(share[j] * mix, k));
        let dp = c.depth.step(params.depth, k);
        // Juno I: 0.513 Hz, 1.7 ms; Juno II: 0.863 Hz, 2.3 ms; both 3.6 ms
        // from centre. RATE and DEPTH scale 0.5x to 2.0x.
        let rate = 0.5 + params.rate * 1.5;
        let ms = sample_rate as f32 / 1000.0;
        let base = 3.6 * ms;
        let turns = |hz: f32| (hz / sample_rate as f32 * 4_294_967_296.0) as u32;
        let (inc_i, inc_ii) = (turns(0.513 * rate), turns(0.863 * rate));
        // FLANGER's LFO 1/16 to 2 Hz, PHASER's 1/8 to 4 Hz, five octaves
        // over RATE (classic pedals' ranges).
        let lowest = if w[3].1 > 0.0 { 0.125 } else { 0.0625 };
        let inc_n = turns(lowest * crate::dsp::filter::coef::exp2_poly(params.rate * 5.0));
        let on = |j: usize| w[j].0 > 0.0 || w[j].1 > 0.0;
        let steady = w.iter().all(|w| w.0 == w.1) && dp.0 == dp.1;
        // Idle (#793): both lines all +0 and the send +0, so every tap
        // reads +0 (each a sum of +0 × a weight in [0, 1]), and a steady
        // block returns it × a finite MIX, +0: `SILENT`, bit for bit. Only
        // the heads move.
        let silent = crate::dsp::all_pos_zero(send);
        if c.fed && silent && c.zeros >= TAIL {
            // The fed-back tail, rung out: cleared to +0 (ADR 0214's skip).
            c.line_i.buffer.fill(0.0);
            c.line_ii.buffer.fill(0.0);
            c.phaser.s = [[0.0; PH_STAGES]; 2];
            c.phaser.y = [0.0; 2];
            c.fed = false;
        }
        if silent
            && steady
            && !c.fed
            && mix.is_finite()
            && c.zeros >= MAX_CHORUS_DELAY as u32
            && crate::dsp::may_skip()
        {
            c.line_i.skip(BLOCK_SIZE, inc_i);
            c.line_ii.skip(BLOCK_SIZE, inc_ii);
            c.phase = c.phase.wrapping_add(inc_n.wrapping_mul(BLOCK_SIZE as u32));
            c.zeros = c.zeros.saturating_add(BLOCK_SIZE as u32);
            *out = Stereo::SILENT;
            crate::dsp::skipped(crate::dsp::Skip::Chorus);
            return;
        }
        c.zeros = if silent {
            c.zeros.saturating_add(BLOCK_SIZE as u32)
        } else {
            0
        };
        c.fed |= !silent && (on(2) || on(3));
        let (a, b) = (&mut c.line_i, &mut c.line_ii);
        // FLANGER's delay, samples: `lo` at the comb's top, `half` its
        // half-span, so the taps sweep `lo + half · (1 ± tri)`.
        let lo = FL_MIN_MS * ms;
        let half = |dm: f32| 0.5 * FL_SPAN_MS * dm * ms;
        if steady && !on(2) && !on(3) {
            let depth = 0.5 + dp.1 * 1.5;
            let (depth_i, depth_ii) = (1.7 * depth * ms, 2.3 * depth * ms);
            // Today's loops, bit for bit: × MIX, I+II's taps averaged first.
            match (w[0].1 > 0.0, w[1].1 > 0.0) {
                (false, false) => {
                    for &x in send {
                        a.write(x, inc_i);
                        b.write(x, inc_ii);
                    }
                    *out = Stereo::SILENT;
                }
                (true, false) => each(send, mix, out, |x| {
                    b.write(x, inc_ii);
                    a.tick(x, base, depth_i, inc_i)
                }),
                (false, true) => each(send, mix, out, |x| {
                    a.write(x, inc_i);
                    b.tick(x, base, depth_ii, inc_ii)
                }),
                (true, true) => each(send, mix, out, |x| {
                    let (l1, r1) = a.tick(x, base, depth_i, inc_i);
                    let (l2, r2) = b.tick(x, base, depth_ii, inc_ii);
                    ((l1 + l2) * 0.5, (r1 + r2) * 0.5)
                }),
            }
            return;
        }
        let dm = (0.5 + dp.0 * 1.5, 0.5 + dp.1 * 1.5);
        if steady && on(2) {
            // FLANGER: each line read before it is written, its tap fed back.
            let (phase, h) = (&mut c.phase, half(dm.1));
            each(send, mix, out, |x| {
                let t = tri(*phase);
                *phase = phase.wrapping_add(inc_n);
                let (l, r) = (a.read(lo + h * (1.0 + t)), b.read(lo + h * (1.0 - t)));
                a.write(x + FL_FB * l, inc_i);
                b.write(x + FL_FB * r, inc_ii);
                (l, r)
            });
            return;
        }
        let ph = &mut c.phaser;
        if steady && on(3) {
            // PHASER: the lines kept current, the chains at a ramped `a`.
            c.phase = c.phase.wrapping_add(inc_n.wrapping_mul(BLOCK_SIZE as u32));
            let (a0, sa) = ph.ramp(c.phase, dm.1, sample_rate);
            let mut i = 0;
            each(send, mix, out, |x| {
                a.write(x, inc_i);
                b.write(x, inc_ii);
                let (al, ar) = (at(a0[0], sa[0], i), at(a0[1], sa[1], i));
                i += 1;
                (ph.tick(0, x, al), ph.tick(1, x, ar))
            });
            return;
        }
        let sw: [f32; 4] = core::array::from_fn(|j| step_of(w[j], BLOCK_SIZE));
        let sd = step_of(dp, BLOCK_SIZE);
        if !on(2) && !on(3) {
            // A chorus mode, MIX or DEPTH move: both lines' taps, each by
            // its weight (today's loop, bit for bit).
            for (i, &x) in send.iter().enumerate() {
                let depth = 0.5 + at(dp.0, sd, i) * 1.5;
                let (l1, r1) = a.tick(x, base, 1.7 * depth * ms, inc_i);
                let (l2, r2) = b.tick(x, base, 2.3 * depth * ms, inc_ii);
                let (g1, g2) = (at(w[0].0, sw[0], i), at(w[1].0, sw[1], i));
                out.l[i] = l1 * g1 + l2 * g2;
                out.r[i] = r1 * g1 + r2 * g2;
            }
            return;
        }
        // Into, out of or within FLANGER or PHASER: every TYPE, each by its
        // weight. The lines take FLANGER's feedback by its share of the
        // return, so a line still heard as I or II never steps.
        let phasing = on(3);
        let (a0, sa) = if phasing {
            let end = c.phase.wrapping_add(inc_n.wrapping_mul(BLOCK_SIZE as u32));
            ph.ramp(end, dm.1, sample_rate)
        } else {
            ([0.0; 2], [0.0; 2])
        };
        for (i, &x) in send.iter().enumerate() {
            let depth = 0.5 + at(dp.0, sd, i) * 1.5;
            let g: [f32; 4] = core::array::from_fn(|j| at(w[j].0, sw[j], i));
            let sum = g[0] + g[1] + g[2] + g[3];
            let fb = if sum > 0.0 { FL_FB * g[2] / sum } else { 0.0 };
            let t = tri(c.phase);
            c.phase = c.phase.wrapping_add(inc_n);
            let h = half(depth);
            let (fl, fr) = (a.read(lo + h * (1.0 + t)), b.read(lo + h * (1.0 - t)));
            let (l1, r1) = a.tick(x + fb * fl, base, 1.7 * depth * ms, inc_i);
            let (l2, r2) = b.tick(x + fb * fr, base, 2.3 * depth * ms, inc_ii);
            let (pl, pr) = if phasing {
                let (al, ar) = (at(a0[0], sa[0], i), at(a0[1], sa[1], i));
                (ph.tick(0, x, al), ph.tick(1, x, ar))
            } else {
                (0.0, 0.0)
            };
            out.l[i] = l1 * g[0] + l2 * g[1] + fl * g[2] + pl * g[3];
            out.r[i] = r1 * g[0] + r2 * g[1] + fr * g[2] + pr * g[3];
        }
    }

    /// One loop per mode, so no per-sample dispatch: `tick` from the send to
    /// (L, R), × `mix`.
    #[inline(always)]
    fn each(
        send: &[f32; BLOCK_SIZE],
        mix: f32,
        out: &mut Stereo,
        mut tick: impl FnMut(f32) -> (f32, f32),
    ) {
        for (i, &x) in send.iter().enumerate() {
            let (l, r) = tick(x);
            out.l[i] = l * mix;
            out.r[i] = r * mix;
        }
    }
}
