#![allow(clippy::needless_range_loop, clippy::manual_clamp)]

use chimera_hal::BLOCK_SIZE;

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
    ap_tank: [DelayLine<4096>; 4],
    del_tank: [DelayLine<8192>; 2],
    // Damping
    lp: [OnePole; 2],
}

const AP_IN_LENS: [usize; 4] = [113, 162, 241, 399];
const AP_TANK_LENS: [usize; 4] = [1653, 2038, 1913, 1663];
const DEL_TANK_LENS: [usize; 2] = [3411, 4782];

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
    /// time: 0..1 (reverb time), diffusion: 0..1, damping: 0..1, mix: 0..1
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        time: f32,
        diffusion: f32,
        damping: f32,
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
            *s = dry * (1.0 - mix) + wet * mix;
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// REVERB 2: Feedback Delay Network (Romb / metallic style)
// ═══════════════════════════════════════════════════════════════════

pub struct FdnReverb {
    // 4 delay lines with mutually prime lengths
    lines: [DelayLine<2048>; 4],
    // Per-line damping
    lp: [OnePole; 4],
}

// Mutually prime delay lengths for dense, non-repeating reflections
const FDN_LENS: [usize; 4] = [601, 773, 947, 1123];

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
    /// time: 0..1, diffusion: 0..1, damping: 0..1, mix: 0..1
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        time: f32,
        damping: f32,
        size: f32,
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
                let len = len.max(2).min(2047);
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
            *s = dry * (1.0 - mix) + wet * mix;
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
    /// time: 0..1, tone: 0..1 (dark/bright), mix: 0..1
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        time: f32,
        tone: f32,
        _size: f32,
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
            *s = dry * (1.0 - mix) + wet * mix;
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

/// Multi-algorithm reverb processor.
pub struct Reverb {
    plate: PlateReverb,
    fdn: FdnReverb,
    midiverb: MidiVerbReverb,
}

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

    pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &ReverbParams) {
        if params.mix < 0.001 {
            return; // bypass
        }
        match ReverbType::from_u8(params.reverb_type) {
            ReverbType::Plate => {
                self.plate
                    .process(buf, params.time, params.size, params.damping, params.mix);
            }
            ReverbType::Fdn => {
                self.fdn
                    .process(buf, params.time, params.damping, params.size, params.mix);
            }
            ReverbType::MidiVerb => {
                self.midiverb
                    .process(buf, params.time, params.damping, params.size, params.mix);
            }
        }
    }
}
