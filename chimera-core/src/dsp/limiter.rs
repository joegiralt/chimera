//! The output stage (ADR 0050): after the master section, the last stage
//! before the DACs. It trims the mix by `OUTPUT_TRIM` and holds each sample
//! at or under `CEILING`, with one gain linked across every pair and side,
//! so `to_dac`'s clamp is never reached. Everything before it is left at
//! main's levels, so the level-dependent stages (the reverb's i16 ring,
//! the delay's saturation, the tape, the compressor) keep their character.
//! Below `THRESHOLD` it is exact: the output is the input × the trim, one
//! block late.
//!
//! The gain moves once per `STEP`-sample chunk and is ramped between, as
//! the compressor's is (`ramp_gains`). Each chunk in asks to keep a share
//! of the trim, `AIM / (peak · trim)` (1 at or under the threshold). The
//! share at a chunk end out is the least any of the next `CHUNKS` chunks
//! asks for (a van Herk minimum over this block and the last), released at
//! 50 ms, then averaged over the last `CHUNKS` chunk ends. Every term of
//! that average covers the chunks the ramp crosses, so the attack is done
//! within the lookahead and no sample passes the ceiling. The average is
//! summed in fixed point: it never drifts, and at rest the gain is exactly
//! the trim.
//!
//! The lookahead is the DAC's own two blocks (`DacBlocks`): the mix lands
//! in one while the other still holds the block before it, which the
//! limiter scales in place and puts out. Nothing is copied.

use chimera_hal::BLOCK_SIZE;
use core::f32::consts::LOG2_E;
use core::mem::MaybeUninit;

use crate::dsp::algo::math::exp2;
use crate::dsp::comp::{STEP, ramp_gains};
use crate::hw::DAC_PAIRS;

/// −1 dBFS.
pub const CEILING: f32 = 0.891_250_9;
/// The output trim (ADR 0050): 1/√8, −9.03 dB, so eight voices summed at
/// random phase land where one voice did. The limiter's rest gain.
pub const OUTPUT_TRIM: f32 = 0.353_553_38;
/// The input level the trim brings to the ceiling: above it, the limiter
/// takes more than the trim.
pub const THRESHOLD: f32 = CEILING / OUTPUT_TRIM;
/// What an over chunk is scaled to: 2^-20 under the ceiling, room for the
/// few roundings between the detector and the output.
const AIM: f32 = CEILING * (1.0 - 1.0 / (1 << 20) as f32);
/// The delay, samples: one block, 1.33 ms at 48 kHz.
pub const LOOKAHEAD: usize = BLOCK_SIZE;
/// Release time constant, s.
pub const RELEASE_S: f32 = 0.05;
/// Chunks per block: the hold and the averaging window, 64 samples.
const CHUNKS: usize = BLOCK_SIZE / STEP;
/// Unity in the averaging sum's fixed point.
const ONE: u32 = 1 << 24;
/// The sum at rest; a power of two, so its reciprocal is exact.
const FULL: u32 = ONE * CHUNKS as u32;
const _: () = assert!(CHUNKS.is_power_of_two() && FULL.is_power_of_two());
/// The release follower snaps to its target this close (−0.0009 dB):
/// near unity a 50 ms one-pole's steps fall under half an f32 ulp at
/// about 1.8e-5 and it would stall short of it.
const SNAP: f32 = 1.0e-4;

type Pairs = [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS];

/// The DAC's blocks, two, for the limiter's one-block lookahead: the one
/// the mix writes and the one it wrote a render ago. Which is which is this
/// type's alone; a shell hands it to `Instrument::render` and reads `out`.
#[derive(Clone)]
pub struct DacBlocks {
    blocks: [Pairs; 2],
    /// The block last put out, and so the next one mixed over; the other
    /// holds the last block in.
    out: usize,
}

impl Default for DacBlocks {
    fn default() -> Self {
        Self::new()
    }
}

impl DacBlocks {
    /// Silence in both: the first block out is silent, as the lookahead
    /// has nothing yet.
    pub const fn new() -> Self {
        Self {
            blocks: [[[0.0; 2 * BLOCK_SIZE]; DAC_PAIRS]; 2],
            out: 0,
        }
    }

    /// The block the next render mixes into, over the last one out.
    pub fn mix(&mut self) -> &mut Pairs {
        &mut self.blocks[self.out]
    }

    /// The last block out, limited: what the DACs play.
    pub fn out(&self) -> &Pairs {
        &self.blocks[self.out]
    }

    /// The last block in, before any gain: the pairs as the mix summed them.
    pub fn input(&self) -> &Pairs {
        &self.blocks[self.out ^ 1]
    }

    /// The block just mixed, to read, and the one before it, to scale.
    fn split(&mut self) -> (&Pairs, &mut Pairs) {
        let [a, b] = &mut self.blocks;
        if self.out == 0 { (a, b) } else { (b, a) }
    }
}

pub struct Limiter {
    /// Suffix minima of the last block's per-chunk gains: `tail[c]` is the
    /// least any of its chunks `c..` asks for, a chunk bounded by its
    /// neighbour before it.
    tail: [f32; CHUNKS],
    /// What the last block's final chunk asked for.
    last: f32,
    /// The release follower.
    rel: f32,
    /// The released gains at the last `CHUNKS` chunk ends, fixed point,
    /// by chunk; `sum` is their total.
    held: [u32; CHUNKS],
    sum: u32,
    /// The gain applied at the end of the last chunk out.
    gain: f32,
    /// The release coefficient per chunk, and the rate it was made for.
    k: f32,
    rate: u32,
}

crate::in_place::field_list!(Limiter => Limiter { tail, last, rel, held, sum, gain, k, rate });

impl Default for Limiter {
    fn default() -> Self {
        Self::new()
    }
}

impl Limiter {
    pub const fn new() -> Self {
        Self {
            tail: [1.0; CHUNKS],
            last: 1.0,
            rel: 1.0,
            held: [ONE; CHUNKS],
            sum: FULL,
            gain: OUTPUT_TRIM,
            k: 0.0,
            rate: 0,
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(Self::new())
    }

    /// Put out the last block, limited, and keep the one just mixed.
    pub fn process(&mut self, dac: &mut DacBlocks, sample_rate: u32) {
        if sample_rate != self.rate {
            let per_fs = 1.0 / sample_rate.max(1) as f32;
            self.k = exp2(-LOG2_E * STEP as f32 / RELEASE_S * per_fs);
            self.rate = sample_rate;
        }
        let (fresh, held) = dac.split();
        let mut m = [0.0f32; CHUNKS];
        let (mut prev, mut head, mut e0) = (self.last, 1.0f32, self.gain);
        for (c, mc) in m.iter_mut().enumerate() {
            let i0 = c * STEP;
            let peak = chunk_peak(fresh, i0);
            // The share of the trim this chunk keeps: `AIM / (peak · trim)`.
            let r = if peak > THRESHOLD {
                (AIM / OUTPUT_TRIM) / peak
            } else {
                1.0
            };
            *mc = least(r, prev);
            prev = r;
            head = least(head, *mc);
            // The least any chunk from `c + 1` to `c + CHUNKS` asks for.
            let h = match self.tail.get(c + 1) {
                Some(&t) => least(t, head),
                None => head,
            };
            self.rel = if h <= self.rel || h - self.rel < SNAP {
                h
            } else {
                h + self.k * (self.rel - h)
            };
            let slot = (c + 1) % CHUNKS;
            let q = (self.rel * ONE as f32) as u32; // floors: never above
            self.sum = self.sum - self.held[slot] + q;
            self.held[slot] = q;
            // The trim times the average: exactly the trim at rest.
            let e1 = self.sum as f32 * (OUTPUT_TRIM / FULL as f32);
            // The chunk in stays where it is; the one in a block ago is
            // scaled in place and goes out.
            for (j, g) in ramp_gains(e0, e1).into_iter().enumerate() {
                let k = 2 * (i0 + j);
                for pair in held.iter_mut() {
                    pair[k] *= g;
                    pair[k + 1] *= g;
                }
            }
            e0 = e1;
        }
        dac.out ^= 1;
        self.gain = e0;
        self.last = prev;
        let mut t = 1.0f32;
        for (tail, &m) in self.tail.iter_mut().zip(&m).rev() {
            t = least(t, m);
            *tail = t;
        }
    }
}

/// The smaller of two gains, never NaN here: a plain compare, without
/// `f32::min`'s NaN handling.
#[inline(always)]
fn least(a: f32, b: f32) -> f32 {
    if a < b { a } else { b }
}

/// The largest magnitude in chunk `i0..i0 + STEP` of any pair or side
/// (the compressor's detector sums the pairs; this one bounds each). On
/// the bits: a float's magnitude orders as its bits do, and integer
/// compares skip the FPU's flag round trip.
///
/// A non-finite sample reads as the level the trim brings to full scale.
/// It has no level to measure,
/// and the DAC plays it at full scale at most (`to_dac`: ±inf clamps, NaN
/// is silence), so it asks for the ceiling's 1 dB and no more: a fault
/// upstream never mutes the music around it, and a finite over beside it
/// is still measured. Rare, so it costs a second look only then.
#[inline(always)]
fn chunk_peak(x: &Pairs, i0: usize) -> f32 {
    let peak = chunk_max(x, i0, |b| b);
    if peak < INF {
        return f32::from_bits(peak);
    }
    f32::from_bits(chunk_max(x, i0, |b| if b < INF { b } else { FULL_SCALE }))
}

const INF: u32 = 0x7f80_0000;
/// The input the trim brings to the DAC's full scale.
const FULL_SCALE: u32 = (1.0 / OUTPUT_TRIM).to_bits();

/// The largest `read(|s| bits)` over the chunk.
#[inline(always)]
fn chunk_max(x: &Pairs, i0: usize, read: impl Fn(u32) -> u32) -> u32 {
    let bits = |s: f32| read(s.to_bits() & 0x7fff_ffff);
    let mut peak = 0u32;
    for i in i0..i0 + STEP {
        for pair in x {
            peak = peak.max(bits(pair[2 * i])).max(bits(pair[2 * i + 1]));
        }
    }
    peak
}
