//! VA: two variable-wave oscillators, an operator, a sub and STABILITY
//! (spec § Pages, § Params and Motion, § Combine, § Stability): the params
//! block and `VaEngine`.

pub mod blep;
pub mod combine;
pub mod osc;
pub mod stability;

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::block::{
    Block, Change, DiskCode, Motion, ParamId, ParamSpec, Smoothing, ValFmt, apply_code,
};
use crate::dsp::algo::math::exp2;
use crate::dsp::ease::{Ramp, step_of};
use crate::dsp::glide::Glide;
use crate::dsp::{clampf, sqrt};
use crate::glyph::FocusGlyph;
use crate::hw::Cost;
use crate::project::PartId;
use crate::voice_alloc::VoiceIdx;
use combine::{Crusher, Peaks, ring_gain, xor16_scaled};
use osc::{NO_WRAP, R, Ramps, Sub, VarOsc};
use stability::{JITTER_CENTS, Seed, Stability, depth};
#[cfg(any(test, feature = "test-support"))]
use {osc::Correction, stability::Character};

/// One of the two oscillators: the index into `VaParams`' pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OscId {
    One = 0,
    Two = 1,
}

impl OscId {
    pub const ALL: [OscId; 2] = [OscId::One, OscId::Two];

    pub const fn index(self) -> usize {
        self as usize
    }
}

/// How the two oscillators combine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum VaMode {
    #[default]
    Mix = 0,
    Sync = 1,
    Ring = 2,
    Xor = 3,
    Bits = 4,
}

impl VaMode {
    pub const ALL: [VaMode; 5] = [
        VaMode::Mix,
        VaMode::Sync,
        VaMode::Ring,
        VaMode::Xor,
        VaMode::Bits,
    ];
}

/// MODE's names, in `VaMode`'s order (display only).
pub const MODE_NAMES: [&str; 5] = ["MIX", "SYNC", "RING", "XOR", "BITS"];

impl DiskCode for VaMode {
    fn disk_code(self) -> u8 {
        match self {
            VaMode::Mix => 0,
            VaMode::Sync => 1,
            VaMode::Ring => 2,
            VaMode::Xor => 3,
            VaMode::Bits => 4,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            VaMode::Mix => "MIX",
            VaMode::Sync => "SYNC",
            VaMode::Ring => "RING",
            VaMode::Xor => "XOR",
            VaMode::Bits => "BITS",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(VaMode::Mix),
            1 => Some(VaMode::Sync),
            2 => Some(VaMode::Ring),
            3 => Some(VaMode::Xor),
            4 => Some(VaMode::Bits),
            _ => None,
        }
    }
}

/// The VA Sound's parameters; pairs are indexed by `OscId::index`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VaParams {
    pub wave: [f32; 2],
    pub shape: [f32; 2],
    pub coarse: [i8; 2],
    /// Cents.
    pub fine: [f32; 2],
    pub mode: VaMode,
    pub amount: f32,
    pub sub: f32,
    pub stability: f32,
}

/// The format's neutral (ADR 0087): a saved Sound omits what is neutral.
impl Default for VaParams {
    fn default() -> Self {
        Self {
            wave: [2.0 / 3.0; 2],
            shape: [0.0; 2],
            coarse: [0; 2],
            fine: [0.0; 2],
            mode: VaMode::Mix,
            amount: 0.5,
            sub: 0.0,
            stability: 1.0,
        }
    }
}

impl VaParams {
    pub const WAVE1: ParamId = ParamId(0);
    pub const WAVE2: ParamId = ParamId(1);
    pub const MODE: ParamId = ParamId(2);
    pub const AMOUNT: ParamId = ParamId(3);
    pub const SUB: ParamId = ParamId(4);
    pub const STAB: ParamId = ParamId(5);
    pub const CRSE1: ParamId = ParamId(6);
    pub const FINE1: ParamId = ParamId(7);
    pub const SHAPE1: ParamId = ParamId(8);
    pub const CRSE2: ParamId = ParamId(9);
    pub const FINE2: ParamId = ParamId(10);
    pub const SHAPE2: ParamId = ParamId(11);
}

const fn uni(id: u8, label: &'static str, ident: &'static str, default: f32) -> ParamSpec {
    ParamSpec::continuous(
        id,
        label,
        ValFmt::Uni,
        0.0,
        1.0,
        default,
        1.0 / 128.0,
        true,
        Smoothing::KNOB,
    )
    .ident(ident)
}

const fn coarse(id: u8, label: &'static str, short: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        id,
        label,
        ValFmt::Signed(24),
        -24.0,
        24.0,
        0.0,
        true,
        Motion::Stepped(Change::Snap),
    )
    .ident(label)
    .short(short)
    .semitones(24.0)
}

const fn fine(id: u8, label: &'static str, short: &'static str) -> ParamSpec {
    ParamSpec::stepped(
        id,
        label,
        ValFmt::Signed(100),
        -100.0,
        100.0,
        0.0,
        true,
        Motion::Continuous(Smoothing::KNOB),
    )
    .ident(label)
    .short(short)
    .cents(100.0)
}

pub static VA_SPECS: [ParamSpec; 12] = [
    uni(0, "WAVE1", "WAVE1", 2.0 / 3.0).short("WV1"),
    uni(1, "WAVE2", "WAVE2", 2.0 / 3.0).short("WV2"),
    ParamSpec::choice(
        2,
        "MODE",
        ValFmt::Names(&MODE_NAMES),
        4.0,
        0.0,
        Change::Crossfade,
    )
    .ident("MODE")
    .glyph(FocusGlyph::None),
    uni(3, "AMNT", "AMOUNT", 0.5).glyph(FocusGlyph::Crossfader),
    uni(4, "SUB", "SUB", 0.0).glyph(FocusGlyph::LevelBar),
    uni(5, "STAB", "STAB", 1.0),
    coarse(6, "CRSE1", "CR1"),
    fine(7, "FINE1", "FN1"),
    uni(8, "SHAPE1", "SHAPE1", 0.0).short("SH1"),
    coarse(9, "CRSE2", "CR2"),
    fine(10, "FINE2", "FN2"),
    uni(11, "SHAPE2", "SHAPE2", 0.0).short("SH2"),
];

impl Block for VaParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &VA_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::WAVE1 => self.wave[0],
            Self::WAVE2 => self.wave[1],
            Self::MODE => self.mode.disk_code() as f32,
            Self::AMOUNT => self.amount,
            Self::SUB => self.sub,
            Self::STAB => self.stability,
            Self::CRSE1 => self.coarse[0] as f32,
            Self::FINE1 => self.fine[0],
            Self::SHAPE1 => self.shape[0],
            Self::CRSE2 => self.coarse[1] as f32,
            Self::FINE2 => self.fine[1],
            Self::SHAPE2 => self.shape[1],
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        // A modulated CRSE is stepped; NaN rounds to 0 (`as` saturates).
        let semis = || clampf(libm::roundf(v), -24.0, 24.0) as i8;
        match id {
            Self::WAVE1 => self.wave[0] = v,
            Self::WAVE2 => self.wave[1] = v,
            Self::MODE => self.mode = VaMode::from_disk_code(v as u8).unwrap_or_default(),
            Self::AMOUNT => self.amount = v,
            Self::SUB => self.sub = v,
            Self::STAB => self.stability = v,
            Self::CRSE1 => self.coarse[0] = semis(),
            Self::FINE1 => self.fine[0] = v,
            Self::SHAPE1 => self.shape[0] = v,
            Self::CRSE2 => self.coarse[1] = semis(),
            Self::FINE2 => self.fine[1] = v,
            Self::SHAPE2 => self.shape[1] = v,
            _ => {}
        }
    }

    fn enum_code(&self, id: ParamId) -> Option<u8> {
        (id == Self::MODE).then(|| self.mode.disk_code())
    }

    fn enum_ident(&self, id: ParamId) -> Option<&'static str> {
        (id == Self::MODE).then(|| self.mode.disk_ident())
    }

    fn set_enum_code(&mut self, id: ParamId, code: u8) -> bool {
        id == Self::MODE && apply_code(VaMode::from_disk_code(code), |m| self.mode = m)
    }
}

/// The morph's working level `R` to the chain's: the plain saw at ±1, the
/// "maximum saw" the 2040 is tuned on (ADR 0096).
pub const VA_CHAIN_GAIN: f32 = 1.0 / (R * 1.732_050_8);

/// VA's output gain at the VCA (ADR 0058); Task 10 sets it by INIT's
/// loudness.
pub const VA_OUT_GAIN: f32 = 1.0;

/// Cycles a sample per MODE by ADR 0056's count on the release ELF (instr ×
/// 1.46 × 1.1, + 12 a `vdiv`/`vsqrt`; Task 9b's scripts). Each oscillator
/// bills its dearest wave pair, constants lerped and SHAPE and WAVE moving:
/// its loop's round with no event, + 0.45 of its wrap's side, + each edge's
/// crossing side at 0.45 + 0.225/64 a sample (backward 0.225/64), all
/// stretched by the jitter's half cent; the SYNC slave + 0.45 of a reset.
/// Then the operator loop (sub flips at 0.45), the steady block's work ÷ 64
/// (the duck's loop at its 4 trips) and two WAVE segment crossings an
/// oscillator every block (an audio-rate LFO on WAVE), each an extra run,
/// and LP24's 95. Each line: OSC 1 (TRI/SAW) + OSC 2 + operator + block +
/// crossings + 95.
///
/// MIX: 155.1 + 148.0 (TRI/SAW) + 41.6 + 76.5 + 64.2 + 95.
pub const COST_MIX: Cost = Cost(590);
/// SYNC: 155.1 + 255.9 (SINE/TRI slave: a reset's 204.0 × 0.45) + 41.6 +
/// 80.4 + 65.6 + 95.
pub const COST_SYNC: Cost = Cost(700);
/// RING: 155.1 + 148.0 + 51.7 + 76.6 + 64.2 + 95.
pub const COST_RING: Cost = Cost(600);
/// XOR: 155.1 + 148.0 + 66.1 + 77.1 + 64.2 + 95.
pub const COST_XOR: Cost = Cost(610);
/// BITS: 155.1 + 148.0 + 55.7 + 76.8 + 64.2 + 95.
pub const COST_BITS: Cost = Cost(600);

/// The MODE duck's length each way, two blocks.
const DUCK: usize = 2 * BLOCK_SIZE;

/// A jitter's cents as a share of the increment, `2^(c/1200) − 1` to first
/// order.
const CENT: f32 = core::f32::consts::LN_2 / 1200.0;

/// A MODE change: down over two blocks, the mode swaps, up over two. A
/// change while it goes down retargets it; one while it comes up waits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Duck {
    Steady,
    /// Which of the two blocks down.
    Down(u8),
    Up(u8),
}

/// The duck's gain at sample `j` of its way down, `½(1 + cos(π(j + 1)/128))`:
/// a raised cosine landing on 0 (a linear duck clicks at its turnaround).
/// The cosine is its Taylor series in f64 at compile time.
static DUCK_DOWN: [[f32; BLOCK_SIZE]; 2] = {
    let mut g = [[0.0f32; BLOCK_SIZE]; 2];
    let mut j = 0;
    while j < DUCK {
        let x = core::f64::consts::PI * (j + 1) as f64 / DUCK as f64;
        // x ≤ π: 40 terms are past f64's precision
        let (mut term, mut sum, mut k) = (1.0f64, 1.0f64, 1);
        while k < 40 {
            term *= -x * x / ((2 * k - 1) * (2 * k)) as f64;
            sum += term;
            k += 1;
        }
        g[j / BLOCK_SIZE][j % BLOCK_SIZE] = (0.5 + 0.5 * sum) as f32;
        j += 1;
    }
    g
};

/// `Lanes::norm` for SUB's block `(from, to)`.
fn norm((s0, s1): (f32, f32)) -> (f32, f32, f32) {
    let n = |s: f32| VA_CHAIN_GAIN / sqrt(1.0 + s * s);
    let h = (BLOCK_SIZE / 2) as f32;
    let (n0, n1, n2) = (n(s0), n(0.5 * (s0 + s1)), n(s1));
    // n(j) = n0 + a·j + b·j², j = 0 the sample before, through j = h and 2h
    let b = (n2 - 2.0 * n1 + n0) / (2.0 * h * h);
    let a = (n1 - n0) / h - b * h;
    (n0 + a + b, a + 3.0 * b, 2.0 * b)
}

/// What a voice's VA keeps across the idle slot's rebuild (spec
/// § Stability, free-run): both phases, the sub's level, the walks and the
/// seeded character, and whose seed it is.
#[derive(Clone, Copy)]
pub(crate) struct VaCarry {
    phase: [f32; 2],
    high: bool,
    stab: Stability,
    seeded: Option<PartId>,
}

/// One block's lanes: each a `(from, step)` ramp a sample (`ease::at`).
#[derive(Clone, Copy)]
struct Lanes {
    osc: [Ramps; 2],
    amount: (f32, f32),
    sub: (f32, f32),
    k: (f32, f32),
    /// `VA_CHAIN_GAIN / √(1 + SUB²)` along SUB's ramp: a quadratic through
    /// the block's ends and middle, its value at sample 0, its step and its
    /// step's step (within 10⁻⁷ of exact on a knob's sweep).
    norm: (f32, f32, f32),
    pk: Peaks,
    d: f32,
}

/// Two variable-wave oscillators, combined by MODE, with a sub under OSC 1
/// and STABILITY's drift (spec § Combine, § Stability).
pub struct VaEngine {
    osc: [VarOsc; 2],
    sub: Sub,
    stab: Stability,
    crusher: Crusher,
    /// Each oscillator's cycle factor from its last wrap's jitter.
    jit: [f32; 2],
    /// Scratch, written before it is read every block: OSC 2's block and
    /// OSC 1's wraps (kept here so no block zeroes them).
    osc2: [f32; BLOCK_SIZE],
    wraps: [f32; BLOCK_SIZE],
    inc: [Ramp; 2],
    wave: [Ramp; 2],
    shape: [Ramp; 2],
    amount: Ramp,
    sub_level: Ramp,
    ring_k: Ramp,
    note: u8,
    pitch: f32,
    slide: Glide,
    sample_rate: u32,
    voice: VoiceIdx,
    /// The Part whose seed `stab` holds.
    seeded: Option<PartId>,
    active: bool,
    playing: VaMode,
    duck: Duck,
    #[cfg(any(test, feature = "test-support"))]
    last_k: [f32; 2],
}

#[cfg(not(any(test, feature = "test-support")))]
const _: () = assert!(size_of::<VaEngine>() <= size_of::<crate::dsp::modal::ModalEngine>());

impl VaEngine {
    fn idle() -> Self {
        Self {
            osc: [VarOsc::NEW; 2],
            sub: Sub::NEW,
            stab: Stability::NEW,
            crusher: Crusher::NEW,
            jit: [1.0; 2],
            osc2: [0.0; BLOCK_SIZE],
            wraps: [NO_WRAP; BLOCK_SIZE],
            inc: [Ramp::default(); 2],
            wave: [Ramp::default(); 2],
            shape: [Ramp::default(); 2],
            amount: Ramp::default(),
            sub_level: Ramp::default(),
            ring_k: Ramp::default(),
            note: 60,
            pitch: 0.0,
            slide: Glide::new(1.0, 1.0),
            sample_rate: 0,
            voice: VoiceIdx::ALL[0],
            seeded: None,
            active: false,
            playing: VaMode::Mix,
            duck: Duck::Steady,
            #[cfg(any(test, feature = "test-support"))]
            last_k: [0.0; 2],
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(Self::idle())
    }

    /// The voice slot whose seed this engine draws.
    pub fn place(&mut self, voice: VoiceIdx) {
        self.voice = voice;
    }

    pub(crate) fn carry(&self) -> VaCarry {
        VaCarry {
            phase: [self.osc[0].phase(), self.osc[1].phase()],
            high: self.sub.high(),
            stab: self.stab,
            seeded: self.seeded,
        }
    }

    /// A fresh engine where the voice's last one left off: the phases run
    /// on from there at the next note-on, below STABILITY 1.
    pub(crate) fn restore(&mut self, c: VaCarry) {
        for (o, p) in self.osc.iter_mut().zip(c.phase) {
            o.set_phase(p);
        }
        self.sub.set_high(c.high);
        (self.stab, self.seeded) = (c.stab, c.seeded);
    }

    /// The oscillators' and the walks' rates.
    fn prepare(&mut self, sample_rate: u32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            for o in &mut self.osc {
                o.prepare(sample_rate as f32);
            }
            self.stab.prepare(sample_rate as f32 / BLOCK_SIZE as f32);
        }
    }

    /// The voice's PITCH and FINE with their routes, in cents.
    pub fn set_pitch(&mut self, cents: f32) {
        self.pitch = cents;
    }

    /// At STABILITY 1 an idle voice starts both oscillators and the sub at
    /// phase 0; otherwise, and on a sounding voice, the phases run on.
    pub fn note_on(&mut self, note: u8, p: &VaParams, part: PartId, sample_rate: u32) {
        self.prepare(sample_rate);
        if !self.active {
            if p.stability >= 1.0 {
                for o in &mut self.osc {
                    o.reset();
                }
                self.sub.reset();
                self.jit = [1.0; 2];
            }
            (self.playing, self.duck) = (p.mode, Duck::Steady);
        }
        if self.seeded != Some(part) {
            debug_assert!(self.voice.index() < 256);
            self.stab
                .seed(Seed::of(part.index() as u32, self.voice.index() as u32));
            self.seeded = Some(part);
        }
        self.note = note;
        self.slide = Glide::new(1.0, 1.0);
        self.active = true;
    }

    /// A steal's glide (ADR 0065): sounding, the phases play on and the
    /// pitch glides to `note`'s; idle, a plain `note_on`.
    pub fn glide_on(
        &mut self,
        note: u8,
        p: &VaParams,
        part: PartId,
        (sample_rate, tau): (u32, f32),
    ) {
        if self.active {
            self.glide_to(note, (sample_rate, tau));
        } else {
            self.note_on(note, p, part, sample_rate);
        }
    }

    /// The pitch glides to `note`'s from where it sounds, a one-pole of
    /// `tau` seconds in log pitch. Idle, or on `note`, nothing moves.
    pub fn glide_to(&mut self, note: u8, (_sample_rate, tau): (u32, f32)) {
        if !self.active || note == self.note {
            return;
        }
        let from = exp2((self.note as f32 - note as f32) / 12.0) * self.slide.value();
        self.slide = Glide::new(from, tau);
        self.slide.toward(1.0);
        self.note = note;
    }

    /// Nothing: the VCA ends the note, and the oscillators run on.
    pub fn note_off(&mut self) {}

    /// A fresh note's ramps start on their values.
    pub fn hold(&mut self) {
        for r in self
            .inc
            .iter_mut()
            .chain(&mut self.wave)
            .chain(&mut self.shape)
        {
            r.hold();
        }
        self.amount.hold();
        self.sub_level.hold();
        self.ring_k.hold();
        for o in &mut self.osc {
            o.hold();
        }
    }

    /// From the first note-on on: VA never decays.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The MODE playing: a new one takes over halfway through its duck.
    pub fn mode(&self) -> VaMode {
        self.playing
    }

    pub const fn mode_cost(m: VaMode) -> Cost {
        match m {
            VaMode::Mix => COST_MIX,
            VaMode::Sync => COST_SYNC,
            VaMode::Ring => COST_RING,
            VaMode::Xor => COST_XOR,
            VaMode::Bits => COST_BITS,
        }
    }

    pub fn cost(p: &VaParams) -> Cost {
        Self::mode_cost(p.mode)
    }

    /// The playing MODE's bill, while a note sounds.
    pub fn playing_cost(&self) -> Option<Cost> {
        self.active.then(|| Self::mode_cost(self.playing))
    }

    /// One block. Never inlined: its block work is counted on its own.
    #[inline(never)]
    pub fn render(&mut self, out: &mut [f32; BLOCK_SIZE], p: &VaParams, sample_rate: u32) {
        if !self.active {
            out.fill(0.0);
            return;
        }
        self.prepare(sample_rate);
        let unit = |v: f32| clampf(v, 0.0, 1.0);
        let d = depth(p.stability);
        self.slide.tick();
        let drift = self.stab.step(d);
        let base = self.note as f32 * 100.0 + self.pitch;
        let mut cents = [0.0f32; 2];
        for (k, c) in cents.iter_mut().enumerate() {
            *c = base
                + f32::from(p.coarse[k].clamp(-24, 24)) * 100.0
                + clampf(p.fine[k], -100.0, 100.0)
                + drift[k];
        }
        let scale = self.stab.scale_cents(cents, d);
        let slide = self.slide.value();
        let hz = [0, 1].map(|k| 440.0 * exp2((cents[k] + scale[k] - 6900.0) / 1200.0) * slide);
        let hz = self.stab.tracked(hz, d);

        let lane = |r: &mut Ramp, v: f32| {
            let ft = r.step(v);
            (ft, (ft.0, step_of(ft, BLOCK_SIZE)))
        };
        let (sub_ft, sub) = lane(&mut self.sub_level, unit(p.sub));
        let mut l = Lanes {
            osc: [Ramps::default(); 2],
            amount: lane(&mut self.amount, unit(p.amount)).1,
            sub,
            k: (0.0, 0.0),
            norm: norm(sub_ft),
            pk: Peaks { a: 0.0, b: 0.0 },
            d,
        };
        let mut pk = [0.0f32; 2];
        for k in 0..2 {
            let (_, li) = lane(&mut self.inc[k], self.osc[k].inc_for(hz[k]));
            let (_, lw) = lane(&mut self.wave[k], unit(p.wave[k]));
            let (_, ls) = lane(&mut self.shape[k], unit(p.shape[k]));
            l.osc[k] = Ramps {
                inc: li,
                wave: lw,
                shape: ls,
            };
            pk[k] = self.osc[k].begin(&l.osc[k], BLOCK_SIZE);
        }
        l.pk = Peaks { a: pk[0], b: pk[1] };
        let k = self.ring_k.step(ring_gain(l.pk));
        l.k = (k.0, step_of(k, BLOCK_SIZE));
        #[cfg(any(test, feature = "test-support"))]
        {
            self.last_k = [k.0, k.1];
        }

        let duck = match self.duck {
            Duck::Steady if p.mode != self.playing => Duck::Down(0),
            d => d,
        };
        match self.playing {
            VaMode::Mix => self.render_mix(out, &l),
            VaMode::Sync => self.render_sync(out, &l),
            VaMode::Ring => self.render_ring(out, &l),
            VaMode::Xor => self.render_xor(out, &l),
            VaMode::Bits => {
                self.crusher.set(unit(p.amount));
                self.render_bits(out, &l)
            }
        }
        self.duck = match duck {
            Duck::Steady => Duck::Steady,
            Duck::Down(b) => {
                let g = &DUCK_DOWN[usize::from(b & 1)];
                for (y, g) in out.iter_mut().zip(g) {
                    *y *= g;
                }
                if b == 0 {
                    Duck::Down(1)
                } else {
                    self.playing = p.mode;
                    Duck::Up(0)
                }
            }
            Duck::Up(b) => {
                let g = &DUCK_DOWN[usize::from(b & 1)];
                for (y, g) in out.iter_mut().zip(g) {
                    *y *= 1.0 - g;
                }
                if b == 0 { Duck::Up(1) } else { Duck::Steady }
            }
        };
    }

    #[inline(never)]
    fn render_mix(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes) {
        self.run(out, l, VaMode::Mix);
    }

    #[inline(never)]
    fn render_sync(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes) {
        self.run(out, l, VaMode::Sync);
    }

    #[inline(never)]
    fn render_ring(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes) {
        self.run(out, l, VaMode::Ring);
    }

    #[inline(never)]
    fn render_xor(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes) {
        self.run(out, l, VaMode::Xor);
    }

    #[inline(never)]
    fn render_bits(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes) {
        self.run(out, l, VaMode::Bits);
    }

    /// One block in `mode`, a constant in each caller, so each mode's loop
    /// is its own (the count sees each). OSC 1 renders into `out` with its
    /// wraps, OSC 2 beside it (synced to them in SYNC), then the operator,
    /// the sub and the chain gain run over both.
    #[inline(always)]
    fn run(&mut self, out: &mut [f32; BLOCK_SIZE], l: &Lanes, mode: VaMode) {
        let Self {
            osc: [o1, o2],
            stab,
            jit,
            sub,
            crusher,
            osc2: b,
            wraps: wr,
            ..
        } = self;
        // a wrap's factor, `1 + jitter(d)·CENT`, from the raw draw: exactly
        // 1 at d = 0 (STAB 1)
        let jd = JITTER_CENTS * l.d * CENT * (1.0 / 2_147_483_648.0);
        let mut draw = || 1.0 + jd * stab.jitter_word() as f32;
        o1.render::<BLOCK_SIZE, true, false>(out, wr, &l.osc[0], &mut jit[0], &mut draw);
        if mode == VaMode::Sync {
            o2.render::<BLOCK_SIZE, false, true>(b, wr, &l.osc[1], &mut jit[1], &mut draw);
        } else {
            o2.render::<BLOCK_SIZE, false, false>(b, wr, &l.osc[1], &mut jit[1], &mut draw);
        }
        let (mut x, dx) = l.amount;
        let (mut k, dk) = l.k;
        let (s0, ds) = l.sub;
        let (mut n, mut dn, ddn) = l.norm;
        // XOR's and BITS' block constants
        let p = l.pk.a.max(l.pk.b);
        let (kx, gx) = (32767.0 / p, p / 32768.0);
        let q = 0.5 * (l.pk.a + l.pk.b);
        let liq = crusher.levels() / q;
        let (mut s, mut j) = (*sub, 0.0f32);
        for (i, (y, b)) in out.iter_mut().zip(b.iter()).enumerate() {
            x += dx;
            k += dk;
            j += 1.0;
            let (a, b) = (*y, *b);
            let half = 0.5 * (a + b);
            let c = match mode {
                VaMode::Mix | VaMode::Sync => a + (b - a) * x,
                VaMode::Ring => half + (k * a * b - half) * x,
                VaMode::Xor => half + (xor16_scaled(a, b, kx, gx) - half) * x,
                VaMode::Bits => q * crusher.process_scaled(half, liq),
            };
            let sv = s.step(wr[i]);
            *y = (c + (s0 + ds * j) * sv) * n;
            n += dn;
            dn += ddn;
        }
        *sub = s;
    }

    /// A glide steal's ratio now, 1 at rest.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        self.slide.value()
    }

    /// Both oscillators uncorrected: the alias gate's baseline.
    #[cfg(any(test, feature = "test-support"))]
    pub fn uncorrected(&mut self) {
        self.osc = self.osc.map(|o| o.with_correction(Correction::Off));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn osc_phase(&self) -> [f32; 2] {
        [self.osc[0].phase(), self.osc[1].phase()]
    }

    /// The seeded character, once a note-on (or a carry) seeded it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn character(&self) -> Option<Character> {
        self.seeded.map(|_| self.stab.character())
    }

    /// The last block's RING gain ramp, from and to.
    #[cfg(any(test, feature = "test-support"))]
    pub fn ring_k(&self) -> [f32; 2] {
        self.last_k
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::boxed::Box;
    use std::vec::Vec;

    const SR: u32 = 48_000;

    fn with_engine<R>(f: impl FnOnce(&mut VaEngine) -> R) -> R {
        let mut slot = Box::new(MaybeUninit::<VaEngine>::uninit());
        f(VaEngine::init_in_place(&mut slot))
    }

    fn run(e: &mut VaEngine, p: &VaParams, blocks: usize) -> Vec<u32> {
        let mut out = [0.0f32; BLOCK_SIZE];
        (0..blocks)
            .flat_map(|_| {
                e.render(&mut out, p, SR);
                out.map(f32::to_bits)
            })
            .collect()
    }

    /// The carry is `carry_restores_the_state` (spec § Stability, free-run):
    /// `carry` is the crate's, so its test lives here.
    #[test]
    fn carry_restores_the_state() {
        let p = VaParams {
            stability: 0.5,
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let part = PartId::new(2).unwrap();
        let c = with_engine(|e| {
            e.place(VoiceIdx::ALL[3]);
            e.note_on(69, &p, part, SR);
            run(e, &p, 225);
            let c = e.carry();
            with_engine(|f| {
                f.restore(c);
                assert_eq!(f.osc_phase(), e.osc_phase());
                assert_eq!(f.character(), e.character());
                assert!(f.character().is_some());
                assert!(!f.is_active());
            });
            c
        });
        let next = || {
            with_engine(|e| {
                e.place(VoiceIdx::ALL[3]);
                e.restore(c);
                e.hold();
                e.note_on(64, &p, part, SR);
                assert_ne!(e.osc_phase(), [0.0; 2], "below STAB 1 the phases run on");
                run(e, &p, 150)
            })
        };
        assert_eq!(next(), next());
    }

    /// The carry holds the walks and the streams themselves: two engines
    /// restoring one carry would agree whatever it held.
    #[test]
    fn the_carry_holds_the_walks_and_streams() {
        let p = VaParams {
            stability: 0.25,
            ..VaParams::default()
        };
        with_engine(|e| {
            e.place(VoiceIdx::ALL[1]);
            e.note_on(69, &p, PartId::new(1).unwrap(), SR);
            run(e, &p, 300);
            with_engine(|f| {
                f.restore(e.carry());
                let (mut a, mut b) = (e.stab, f.stab);
                for _ in 0..64 {
                    assert_eq!(a.step(0.25), b.step(0.25));
                    assert_eq!(a.jitter(0.25), b.jitter(0.25));
                }
            });
        });
    }

    /// STAB 1 on an idle engine that ran before starts at phase 0: it
    /// renders as an engine that never played.
    #[test]
    fn stab_one_resets_a_restored_engine() {
        let part = PartId::new(0).unwrap();
        let alive = VaParams {
            stability: 0.5,
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let c = with_engine(|e| {
            e.note_on(69, &alive, part, SR);
            run(e, &alive, 77);
            e.carry()
        });
        let steady = VaParams {
            fine: [0.0, 6.0],
            ..VaParams::default()
        };
        let played = with_engine(|e| {
            e.restore(c);
            assert_ne!(e.osc_phase(), [0.0; 2]);
            e.hold();
            e.note_on(69, &steady, part, SR);
            assert_eq!(e.osc_phase(), [0.0; 2]);
            run(e, &steady, 150)
        });
        let fresh = with_engine(|e| {
            e.hold();
            e.note_on(69, &steady, part, SR);
            run(e, &steady, 150)
        });
        assert_eq!(played, fresh);
    }
}
