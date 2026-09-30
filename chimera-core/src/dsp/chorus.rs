//! Juno-style BBD chorus, stereo (FX diet spec § Chorus): each line has a
//! triangle LFO and two read taps, the normal one on the left and one on
//! the inverted LFO on the right. The mono sum does not cancel.
//!
//! Mode I:  triangle LFO at 0.513 Hz, depth ~1.7ms
//! Mode II: triangle LFO at 0.863 Hz, depth ~2.3ms
//! Mode I+II: both lines; each side averages its two taps

use crate::block::{Block, DiskCode, ParamId, ParamSpec, ValFmt, apply_code};
use crate::dsp::Stereo;
use crate::dsp::ease::{Ease, at, ease_coeff, step_of};
use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;

const MAX_CHORUS_DELAY: usize = 2048; // ~42ms at 48kHz, plenty for chorus

/// Chorus mode selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ChorusMode {
    Off = 0,
    JunoI = 1,    // Slow, subtle
    JunoII = 2,   // Faster, wider
    JunoBoth = 3, // Both LFOs (thickest)
}

impl ChorusMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 4 {
            0 => ChorusMode::Off,
            1 => ChorusMode::JunoI,
            2 => ChorusMode::JunoII,
            _ => ChorusMode::JunoBoth,
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
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            ChorusMode::Off => "OFF",
            ChorusMode::JunoI => "JUNO_I",
            ChorusMode::JunoII => "JUNO_II",
            ChorusMode::JunoBoth => "JUNO_BOTH",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(ChorusMode::Off),
            1 => Some(ChorusMode::JunoI),
            2 => Some(ChorusMode::JunoII),
            3 => Some(ChorusMode::JunoBoth),
            _ => None,
        }
    }
}

/// Chorus parameters.
#[derive(Clone, Copy, Debug)]
pub struct ChorusParams {
    /// Mode: 0=off, 1=Juno I, 2=Juno II, 3=Both
    pub mode: u8,
    /// Rate multiplier (0.5..2.0, centered at 1.0)
    pub rate: f32,
    /// Depth multiplier (0.5..2.0, centered at 1.0)
    pub depth: f32,
    /// Dry/wet mix (0..1)
    pub mix: f32,
}

impl Default for ChorusParams {
    fn default() -> Self {
        Self {
            mode: 0, // off by default
            rate: 0.5,
            depth: 0.5,
            mix: 0.0,
        }
    }
}

impl ChorusParams {
    /// Off when the mode is off or the mix is below audibility; `process`
    /// passes the input through unchanged then.
    pub fn is_on(&self) -> bool {
        ChorusMode::from_u8(self.mode) != ChorusMode::Off && self.mix >= 0.001
    }

    pub const MODE: ParamId = ParamId(0);
    pub const RATE: ParamId = ParamId(1);
    pub const DEPTH: ParamId = ParamId(2);
    pub const MIX: ParamId = ParamId(3);
}

/// Chorus runs outside `Voice`, on the FX bus: nothing is modulatable.
pub static CHORUS_SPECS: [ParamSpec; 4] = [
    ParamSpec::choice(0, "MODE", ValFmt::Int(3), 3.0, 0.0).ident("MODE"),
    ParamSpec::continuous(1, "RATE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false).ident("RATE"),
    ParamSpec::continuous(2, "DEPTH", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false)
        .ident("DEPTH"),
    ParamSpec::continuous(3, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false).ident("MIX"),
];

impl Block for ChorusParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &CHORUS_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODE => self.mode as f32,
            Self::RATE => self.rate,
            Self::DEPTH => self.depth,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

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

    /// The (normal, inverted) taps at the LFO's phase.
    #[inline(always)]
    fn taps(&self, base: f32, depth: f32) -> (f32, f32) {
        // Triangle, 0→1→0→-1→0: 1 − |4v − 2| a quarter turn on, v ∈ [0, 1),
        // from the phase's distance to the half turn.
        let x = (self.lfo_phase.wrapping_add(1 << 30) ^ (1 << 31)) as i32;
        let tri = 1.0 - (x as f32).abs() * (1.0 / (1u32 << 30) as f32);
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

crate::in_place::field_list!(JunoChorus => JunoChorus { line_i, line_ii, weight, depth });
crate::in_place::field_list!(BbdLine => BbdLine { buffer, write_pos, lfo_phase });

/// Juno-style chorus: mono send in, stereo wet out. Both lines are
/// written whatever the mode, so a line faded in is current; each line's
/// weight (its share of the mode × MIX) and DEPTH ease (never snap).
pub struct JunoChorus {
    line_i: BbdLine,
    line_ii: BbdLine,
    weight: [Ease; 2],
    depth: Ease,
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
            weight: [Ease::default(); 2],
            depth: Ease::default(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: every field (two `BbdLine { buffer: [f32; N], write_pos:
        // usize, lfo_phase: u32 }` and three `Ease { f32, bool }`) is valid
        // as zero bytes, and zero is exactly `new()`'s state; `write_bytes`
        // covers the whole slot.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    /// Send/return use (the FX bus): the wet signal × MIX, the return
    /// level, per side. At MIX 0 or mode OFF the lines are still written
    /// and the return fades out.
    pub fn process_wet(
        &mut self,
        send: &[f32; BLOCK_SIZE],
        params: &ChorusParams,
        sample_rate: u32,
        out: &mut Stereo,
    ) {
        let mode = ChorusMode::from_u8(params.mode);
        // Each line's share of the return: I+II averages the two.
        let share = match mode {
            ChorusMode::Off => (0.0, 0.0),
            ChorusMode::JunoI => (1.0, 0.0),
            ChorusMode::JunoII => (0.0, 1.0),
            ChorusMode::JunoBoth => (0.5, 0.5),
        };
        let mix = if params.mix >= 0.001 { params.mix } else { 0.0 };
        let k = ease_coeff(sample_rate);
        let w = [
            self.weight[0].step(share.0 * mix, k),
            self.weight[1].step(share.1 * mix, k),
        ];
        let dp = self.depth.step(params.depth, k);
        // Juno I: 0.513 Hz, 1.7 ms; Juno II: 0.863 Hz, 2.3 ms; both 3.6 ms
        // from centre. RATE and DEPTH scale 0.5x to 2.0x.
        let rate = 0.5 + params.rate * 1.5;
        let ms = sample_rate as f32 / 1000.0;
        let base = 3.6 * ms;
        let turns = |hz: f32| (hz / sample_rate as f32 * 4_294_967_296.0) as u32;
        let (inc_i, inc_ii) = (turns(0.513 * rate), turns(0.863 * rate));
        let (a, b) = (&mut self.line_i, &mut self.line_ii);
        let steady = w[0].0 == w[0].1 && w[1].0 == w[1].1 && dp.0 == dp.1;
        if steady {
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
        // A mode, MIX or DEPTH move: both lines' taps, each by its weight.
        let (sw, sd) = (
            [step_of(w[0], BLOCK_SIZE), step_of(w[1], BLOCK_SIZE)],
            step_of(dp, BLOCK_SIZE),
        );
        for (i, &x) in send.iter().enumerate() {
            let depth = 0.5 + at(dp.0, sd, i) * 1.5;
            let (l1, r1) = a.tick(x, base, 1.7 * depth * ms, inc_i);
            let (l2, r2) = b.tick(x, base, 2.3 * depth * ms, inc_ii);
            let (g1, g2) = (at(w[0].0, sw[0], i), at(w[1].0, sw[1], i));
            out.l[i] = l1 * g1 + l2 * g2;
            out.r[i] = r1 * g1 + r2 * g2;
        }
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
