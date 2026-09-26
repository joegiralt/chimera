#![allow(clippy::needless_range_loop, clippy::manual_clamp)]

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;

// ── Shared delay line infrastructure ────────────────────────────────

struct DelayLine<const N: usize> {
    buffer: [f32; N],
    write_pos: usize,
}

impl<const N: usize> DelayLine<N> {
    const fn new() -> Self {
        Self {
            buffer: [0.0; N],
            write_pos: 0,
        }
    }

    #[inline]
    fn write(&mut self, sample: f32) {
        self.buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % N;
    }

    #[inline]
    fn read(&self, delay: usize) -> f32 {
        let pos = (self.write_pos + N - delay) % N;
        self.buffer[pos]
    }

    /// Interpolated read for modulated delays.
    #[inline]
    #[allow(dead_code)] // Will be used when LFO modulation is implemented
    fn read_interp(&self, delay: f32) -> f32 {
        let d = delay as usize;
        let frac = delay - d as f32;
        let a = self.read(d);
        let b = self.read(d + 1);
        a + frac * (b - a)
    }

    /// Allpass: read from delay, write input + feedback.
    #[inline]
    fn allpass(&mut self, input: f32, delay: usize, coeff: f32) -> f32 {
        let delayed = self.read(delay);
        let output = delayed - coeff * input;
        self.write(input + coeff * delayed);
        output
    }
}

// ── One-pole lowpass ────────────────────────────────────────────────

struct OnePole {
    state: f32,
}

impl OnePole {
    const fn new() -> Self {
        Self { state: 0.0 }
    }

    #[inline]
    fn process(&mut self, input: f32, coeff: f32) -> f32 {
        self.state += coeff * (input - self.state);
        self.state
    }
}

// ═══════════════════════════════════════════════════════════════════
// REVERB 1: Dattorro Plate (MI Clouds style)
// ═══════════════════════════════════════════════════════════════════

pub struct PlateReverb {
    // Input diffusion: 4 allpass filters
    ap_in: [DelayLine<512>; 4],
    // Tank: 2 branches, each with 2 allpass + 1 delay
    ap_tank: [DelayLine<AP_TANK_LINE>; 4],
    del_tank: [DelayLine<DEL_TANK_LINE>; 2],
    // Damping
    lp: [OnePole; 2],
}

const AP_IN_LENS: [usize; 4] = [113, 162, 241, 399];
const AP_TANK_LENS: [usize; 4] = [1653, 2038, 1913, 1663];
const DEL_TANK_LENS: [usize; 2] = [3411, 4782];
/// Line lengths cover the longest fixed tap (ADR 0014); a `DelayLine<N>`
/// with `N >= delay` reads exactly what a longer one would.
const AP_TANK_LINE: usize = 2048;
const DEL_TANK_LINE: usize = 4800;
const _: () = assert!(AP_TANK_LENS[1] <= AP_TANK_LINE && DEL_TANK_LENS[1] <= DEL_TANK_LINE);

impl Default for PlateReverb {
    fn default() -> Self {
        Self::new()
    }
}

impl PlateReverb {
    pub fn new() -> Self {
        Self {
            ap_in: [
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
            ],
            ap_tank: [
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
            ],
            del_tank: [DelayLine::new(), DelayLine::new()],
            lp: [OnePole::new(), OnePole::new()],
        }
    }

    /// Process a block in-place.
    /// time: 0..1 (reverb time), diffusion: 0..1, damping: 0..1, dry_gain: dry level
    /// (1 − mix inserted, 0 on a send), mix: wet level 0..1
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        time: f32,
        diffusion: f32,
        damping: f32,
        dry_gain: f32,
        mix: f32,
    ) {
        let krt = time; // feedback coefficient
        let kap = 0.3 + diffusion * 0.4; // allpass coefficient 0.3..0.7
        let klp = 0.3 + damping * 0.6; // lowpass coefficient

        for s in buf.iter_mut() {
            let dry = *s;
            let mut input = dry;

            // Input diffusion: 4 series allpass
            for i in 0..4 {
                input = self.ap_in[i].allpass(input, AP_IN_LENS[i], kap);
            }

            // Read from tank delays (cross-coupled)
            let tank_a = self.del_tank[0].read(DEL_TANK_LENS[0]);
            let tank_b = self.del_tank[1].read(DEL_TANK_LENS[1]);

            // Branch A: input + cross-feedback from B
            let mut a = input + tank_b * krt;
            a = self.ap_tank[0].allpass(a, AP_TANK_LENS[0], -kap);
            a = self.ap_tank[1].allpass(a, AP_TANK_LENS[1], kap);
            a = self.lp[0].process(a, klp);
            self.del_tank[0].write(a);

            // Branch B: input + cross-feedback from A
            let mut b = input + tank_a * krt;
            b = self.ap_tank[2].allpass(b, AP_TANK_LENS[2], -kap);
            b = self.ap_tank[3].allpass(b, AP_TANK_LENS[3], kap);
            b = self.lp[1].process(b, klp);

            self.del_tank[1].write(b);

            // Output: multi-tap from both branches
            let wet = (tank_a + tank_b) * 0.5;

            // Mix
            *s = dry * dry_gain + wet * mix;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// REVERB 2: Feedback Delay Network (Romb / metallic style)
// ═══════════════════════════════════════════════════════════════════

pub struct FdnReverb {
    // 4 delay lines with mutually prime lengths
    lines: [DelayLine<FDN_LINE>; 4],
    // Per-line damping
    lp: [OnePole; 4],
}

// Mutually prime delay lengths for dense, non-repeating reflections
const FDN_LENS: [usize; 4] = [601, 773, 947, 1123];
/// Covers the longest line at size 1.0 (`size_scale` = 1.0): 1,123 samples.
const FDN_LINE: usize = 1152;

impl Default for FdnReverb {
    fn default() -> Self {
        Self::new()
    }
}

impl FdnReverb {
    pub fn new() -> Self {
        Self {
            lines: [
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
            ],
            lp: [
                OnePole::new(),
                OnePole::new(),
                OnePole::new(),
                OnePole::new(),
            ],
        }
    }

    /// Process a block in-place.
    /// time: 0..1, diffusion: 0..1, damping: 0..1, dry_gain: dry level
    /// (1 − mix inserted, 0 on a send), mix: wet level 0..1
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        time: f32,
        damping: f32,
        size: f32,
        dry_gain: f32,
        mix: f32,
    ) {
        let fb = 0.3 + time * 0.65; // feedback 0.3..0.95
        let klp = 0.2 + damping * 0.7;
        // Size scales delay lengths
        let size_scale = 0.3 + size * 0.7;

        for s in buf.iter_mut() {
            let dry = *s;
            let input = dry * 0.25; // distribute to 4 lines

            // Read from all 4 lines
            let mut taps = [0.0f32; 4];
            for i in 0..4 {
                let len = (FDN_LENS[i] as f32 * size_scale) as usize;
                let len = len.max(2).min(FDN_LINE - 1);
                taps[i] = self.lines[i].read(len);
            }

            // Hadamard-like mixing matrix (unitary, preserves energy)
            // [+1 +1 +1 +1]     [a]
            // [+1 -1 +1 -1]  x  [b]
            // [+1 +1 -1 -1]     [c]
            // [+1 -1 -1 +1]     [d]  * 0.5
            let mixed = [
                (taps[0] + taps[1] + taps[2] + taps[3]) * 0.5,
                (taps[0] - taps[1] + taps[2] - taps[3]) * 0.5,
                (taps[0] + taps[1] - taps[2] - taps[3]) * 0.5,
                (taps[0] - taps[1] - taps[2] + taps[3]) * 0.5,
            ];

            // Feed back with damping
            for i in 0..4 {
                let damped = self.lp[i].process(mixed[i], klp);
                self.lines[i].write(input + damped * fb);
            }

            // Output: sum of all taps
            let wet = (taps[0] + taps[1] + taps[2] + taps[3]) * 0.25;
            *s = dry * dry_gain + wet * mix;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// REVERB 3: MidiVerb II (lo-fi allpass chain)
// ═══════════════════════════════════════════════════════════════════

pub struct MidiVerbReverb {
    // Input diffusion: 5 allpass filters (max delay 272)
    ap_diff: [DelayLine<512>; 5],
    // Network A: 4 allpass filters (max delay 830)
    ap_net_a: [DelayLine<1024>; 4],
    // Network B: 4 allpass filters (max delay 910)
    ap_net_b: [DelayLine<1024>; 4],
    // Recirculation state
    recirc_a: f32,
    recirc_b: f32,
}

// Input diffusion lengths (scaled from original 23kHz to 48kHz)
const MV_DIFF_LENS: [usize; 5] = [38, 90, 168, 230, 272];
// Network A allpass lengths
const MV_NET_A_LENS: [usize; 4] = [412, 558, 674, 830];
// Network B allpass lengths
const MV_NET_B_LENS: [usize; 4] = [450, 620, 742, 910];

impl Default for MidiVerbReverb {
    fn default() -> Self {
        Self::new()
    }
}

impl MidiVerbReverb {
    pub fn new() -> Self {
        Self {
            ap_diff: [
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
            ],
            ap_net_a: [
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
            ],
            ap_net_b: [
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
                DelayLine::new(),
            ],
            recirc_a: 0.0,
            recirc_b: 0.0,
        }
    }

    /// Process a block in-place.
    /// time: 0..1, tone: 0..1 (dark/bright), dry_gain: dry
    /// level (1 − mix inserted, 0 on a send), mix: wet level 0..1
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        time: f32,
        tone: f32,
        _size: f32,
        dry_gain: f32,
        mix: f32,
    ) {
        // MidiVerb uses power-of-2 coefficients: 1/2, 3/4, 3/8
        let ap_coeff = 0.75; // 3/4 (MidiVerb's primary allpass coefficient)
        let fb = 0.3 + time * 0.55; // recirculation feedback
        let _ = tone; // TODO: could control a simple tilt EQ

        for s in buf.iter_mut() {
            let dry = *s;
            let mut input = dry;

            // Input diffusion: 5 cascaded allpass (coeff 3/4)
            for i in 0..5 {
                input = self.ap_diff[i].allpass(input, MV_DIFF_LENS[i], ap_coeff);
            }

            // Network A: 4 cascaded allpass with recirculation
            let mut a = input + self.recirc_a * fb;
            for i in 0..4 {
                a = self.ap_net_a[i].allpass(a, MV_NET_A_LENS[i], 0.5); // coeff 1/2
            }
            self.recirc_a = a * 0.75; // 3/4 feedback scaling

            // Network B: 4 cascaded allpass with recirculation
            let mut b = input + self.recirc_b * fb;
            for i in 0..4 {
                b = self.ap_net_b[i].allpass(b, MV_NET_B_LENS[i], 0.5);
            }
            self.recirc_b = b * 0.75;

            // Output: sum of both networks (MidiVerb uses multiple taps but
            // we simplify to the network outputs for now)
            let wet = (a + b) * 0.5;
            *s = dry * dry_gain + wet * mix;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Reverb selector
// ═══════════════════════════════════════════════════════════════════

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReverbType {
    Plate = 0,    // MI Clouds / Dattorro
    Fdn = 1,      // Metallic / Romb-style
    MidiVerb = 2, // Alesis MidiVerb II lo-fi
}

impl ReverbType {
    pub fn from_u8(v: u8) -> Self {
        match v % 3 {
            0 => ReverbType::Plate,
            1 => ReverbType::Fdn,
            _ => ReverbType::MidiVerb,
        }
    }
}

/// Parameters for the reverb effect.
#[derive(Clone, Copy, Debug)]
pub struct ReverbParams {
    pub reverb_type: u8, // 0-2
    pub time: f32,       // 0..1 reverb time
    pub damping: f32,    // 0..1 high-freq damping
    pub size: f32,       // 0..1 room size / diffusion
    pub mix: f32,        // 0..1 dry/wet
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            reverb_type: 0,
            time: 0.5,
            damping: 0.3,
            size: 0.5,
            mix: 0.3,
        }
    }
}

impl ReverbParams {
    /// Off when the mix is below audibility; `process` passes the input
    /// through unchanged then.
    pub fn is_on(&self) -> bool {
        self.mix >= 0.001
    }

    pub const REVERB_TYPE: ParamId = ParamId(0);
    pub const TIME: ParamId = ParamId(1);
    pub const DAMPING: ParamId = ParamId(2);
    pub const SIZE: ParamId = ParamId(3);
    pub const MIX: ParamId = ParamId(4);
}

/// Reverb runs outside `Voice` (desktop only): nothing is modulatable.
pub static REVERB_SPECS: [ParamSpec; 5] = [
    ParamSpec::choice(0, "TYPE", ValFmt::Int(2), 2.0, 0.0),
    ParamSpec::continuous(1, "TIME", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "DAMP", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "SIZE", ValFmt::Uni, 0.0, 1.0, 0.5, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "MIX", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
];

impl Block for ReverbParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &REVERB_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::REVERB_TYPE => self.reverb_type as f32,
            Self::TIME => self.time,
            Self::DAMPING => self.damping,
            Self::SIZE => self.size,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::REVERB_TYPE => self.reverb_type = v as u8,
            Self::TIME => self.time = v,
            Self::DAMPING => self.damping = v,
            Self::SIZE => self.size = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}

/// Multi-algorithm reverb processor.
pub struct Reverb {
    plate: PlateReverb,
    fdn: FdnReverb,
    midiverb: MidiVerbReverb,
}

crate::in_place::field_list!(Reverb => Reverb { plate, fdn, midiverb });
crate::in_place::field_list!(PlateReverb => PlateReverb { ap_in, ap_tank, del_tank, lp });
crate::in_place::field_list!(FdnReverb => FdnReverb { lines, lp });
crate::in_place::field_list!(MidiVerbReverb => MidiVerbReverb { ap_diff, ap_net_a, ap_net_b, recirc_a, recirc_b });
crate::in_place::field_list!(DelayLine<1> => DelayLine { buffer, write_pos });
crate::in_place::field_list!(OnePole => OnePole { state });

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}

impl Reverb {
    pub fn new() -> Self {
        Self {
            plate: PlateReverb::new(),
            fdn: FdnReverb::new(),
            midiverb: MidiVerbReverb::new(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: the plate, FDN and MidiVerb reverbs hold only `DelayLine`s
        // (`[f32; N]` + `usize`), `OnePole`s (`f32`) and two `f32`s, all valid
        // as zero bytes; zero is exactly `new()`'s state.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    /// Insert use: dry/wet mix in place.
    pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &ReverbParams) {
        self.run(buf, params, 1.0 - params.mix);
    }

    /// Send/return use (the FX bus): writes only the wet signal × MIX, the
    /// return level, in place of the send.
    pub fn process_wet(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &ReverbParams) {
        self.run(buf, params, 0.0);
    }

    fn run(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &ReverbParams, dry_gain: f32) {
        if !params.is_on() {
            return; // bypass
        }
        match ReverbType::from_u8(params.reverb_type) {
            ReverbType::Plate => {
                self.plate.process(
                    buf,
                    params.time,
                    params.size,
                    params.damping,
                    dry_gain,
                    params.mix,
                );
            }
            ReverbType::Fdn => {
                self.fdn.process(
                    buf,
                    params.time,
                    params.damping,
                    params.size,
                    dry_gain,
                    params.mix,
                );
            }
            ReverbType::MidiVerb => {
                self.midiverb.process(
                    buf,
                    params.time,
                    params.damping,
                    params.size,
                    dry_gain,
                    params.mix,
                );
            }
        }
    }
}
