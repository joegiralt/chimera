//! The shared FX bus (instrument-core spec § Audio path): chorus, delay and
//! reverb run once per block on the sum of every part's sends. Send/return:
//! each effect returns only its wet signal, its MIX acting as the return
//! level; the dry signal reaches the DACs through the parts alone. The
//! chorus and reverb return stereo; the delay is mono, on both sides of
//! DAC pair 1 at unity. The delay's return can feed the reverb's send (REV
//! SEND), in the same block. After the pairs are summed, `master` runs the
//! master section: the compressor linked across all three pairs (the tape
//! on DAC pair 1 before it only with the `master-tape` feature, ADR 0055);
//! `limit` then runs the output stage, the trim and the peak limiter
//! (ADR 0050).

use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::dsp::Stereo;
use crate::dsp::chorus::{ChorusParams, JunoChorus};
use crate::dsp::comp::{CompParams, MasterComp};
use crate::dsp::delay::{DelayParams, TapeDelay};
use crate::dsp::ease::{Ease, ease_coeff};
use crate::dsp::limiter::{DacBlocks, Limiter};
use crate::dsp::reverb::ReverbParams;
use crate::dsp::ring::RingReverb;
#[cfg(feature = "master-tape")]
use crate::dsp::tape::Tape;
use crate::dsp::tape::TapeParams;
use crate::hw::{Cost, DAC_PAIRS, FX_BUS_BUDGET};
use crate::in_place::uninit_at;

/// Sends per part, in this order: chorus, delay, reverb.
pub const FX_SENDS: usize = 3;

// ADR 0014: the FX bus lives in AXI SRAM beside the framebuffer and UI.
const _: () = assert!(core::mem::size_of::<FxBus>() <= FX_BUS_BUDGET);

/// Shared effect settings: one set per Performance, not per Sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FxParams {
    pub chorus: ChorusParams,
    pub delay: DelayParams,
    pub reverb: ReverbParams,
    pub tape: TapeParams,
    pub comp: CompParams,
}

impl Default for FxParams {
    /// Everything off.
    fn default() -> Self {
        Self {
            chorus: ChorusParams::default(),
            delay: DelayParams::default(),
            reverb: ReverbParams::default(),
            tape: TapeParams::default(),
            comp: CompParams::default(),
        }
    }
}

pub struct FxBus {
    chorus: JunoChorus,
    delay: TapeDelay,
    reverb: RingReverb,
    #[cfg(feature = "master-tape")]
    tape: Tape,
    comp: MasterComp,
    limiter: Limiter,
    /// REV SEND, eased.
    rev_send: Ease,
}

#[cfg(feature = "master-tape")]
crate::in_place::field_list!(FxBus => FxBus { chorus, delay, reverb, tape, comp, limiter, rev_send });
#[cfg(not(feature = "master-tape"))]
crate::in_place::field_list!(FxBus => FxBus { chorus, delay, reverb, comp, limiter, rev_send });

impl Default for FxBus {
    fn default() -> Self {
        Self::new()
    }
}

impl FxBus {
    /// The whole bus at its worst settings, with the Instrument's mixing
    /// and the output limiter; reserved from the voice budget whether or
    /// not an effect is on. The bench's BUS row runs `mix_parts`, so it
    /// times the limiter too (ADR 0050); the master tape counts only in
    /// the build that runs it (ADR 0055). Each benched BUS row plus the
    /// settings' eases, run every block (ADR 0061): 6.5 instructions a
    /// sample, × 1.46 × 1.1 = 10.4 by ADR 0056's host method. A setting
    /// moving is the UI's doing, never a route's, so what its ease costs
    /// while it moves (up to 50 instructions a sample, a delay TIME
    /// crossfade under a MIX ramp) is brief and sits in the headroom.
    #[cfg(not(feature = "master-tape"))]
    pub const COST: Cost = Cost(1180); // BUS 1155, measured 2026-09-29, bench, rev V at 480 MHz, b34a66a; + 10.4 (ADR 0061); rounded up
    #[cfg(feature = "master-tape")]
    pub const COST: Cost = Cost(1490); // BUS 1468, measured 2026-09-29, bench, rev V at 480 MHz, 87c2930 (tape in); + 10.4 (ADR 0061); rounded up

    pub fn new() -> Self {
        Self {
            chorus: JunoChorus::new(),
            delay: TapeDelay::new(),
            reverb: RingReverb::new(),
            #[cfg(feature = "master-tape")]
            tape: Tape::new(),
            comp: MasterComp::new(),
            limiter: Limiter::new(),
            rev_send: Ease::default(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from `&mut MaybeUninit<Self>` (valid, aligned,
        // unaliased); each effect's in-place constructor initialises its
        // whole field, and `rev_send` is written, before `assume_init_mut`.
        unsafe {
            JunoChorus::init_in_place(uninit_at(addr_of_mut!((*p).chorus)));
            TapeDelay::init_in_place(uninit_at(addr_of_mut!((*p).delay)));
            RingReverb::init_in_place(uninit_at(addr_of_mut!((*p).reverb)));
            #[cfg(feature = "master-tape")]
            Tape::init_in_place(uninit_at(addr_of_mut!((*p).tape)));
            MasterComp::init_in_place(uninit_at(addr_of_mut!((*p).comp)));
            Limiter::init_in_place(uninit_at(addr_of_mut!((*p).limiter)));
            addr_of_mut!((*p).rev_send).write(Ease::default());
            slot.assume_init_mut()
        }
    }

    /// Run every effect over its send and sum the wet returns into `ret`.
    /// Each runs whatever its MIX, which eases its return: an effect at
    /// MIX 0 returns nothing, and one brought back up plays what its send
    /// is doing now, never a frozen tail (#61).
    pub fn process(
        &mut self,
        sends: &mut [[f32; BLOCK_SIZE]; FX_SENDS],
        params: &FxParams,
        sample_rate: u32,
        ret: &mut Stereo,
    ) {
        let [chorus, delay, reverb] = sends;
        self.chorus
            .process_wet(chorus, &params.chorus, sample_rate, ret);
        self.delay.process_wet(delay, &params.delay, sample_rate);
        add(&mut ret.l, delay);
        add(&mut ret.r, delay);
        // REV SEND: the delay's return into the reverb's send, this block.
        let (from, to) = self
            .rev_send
            .step(unit(params.delay.rev_send), ease_coeff(sample_rate));
        if from != 0.0 || to != 0.0 {
            let n = BLOCK_SIZE as f32;
            for (i, (r, &d)) in reverb.iter_mut().zip(delay.iter()).enumerate() {
                *r += (from + (to - from) * (i + 1) as f32 / n) * d;
            }
        }
        let mut wet = Stereo::SILENT;
        let mix = if params.reverb.is_on() {
            params.reverb.mix
        } else {
            0.0
        };
        self.reverb.process(
            reverb,
            &params.reverb.controls(),
            mix,
            sample_rate,
            &mut wet,
        );
        add(&mut ret.l, &wet.l);
        add(&mut ret.r, &wet.r);
    }

    /// The master section, after every pair is summed: the compressor, one
    /// gain on every pair; with `master-tape`, the tape on pair 1 first.
    pub fn master(
        &mut self,
        out: &mut [[f32; 2 * BLOCK_SIZE]; DAC_PAIRS],
        params: &FxParams,
        sample_rate: u32,
    ) {
        #[cfg(feature = "master-tape")]
        self.tape.process(&mut out[0], &params.tape, sample_rate);
        self.comp.process(out, &params.comp, sample_rate);
    }

    /// The output stage, after the master section: the output trim and the
    /// peak limiter, one block late (ADR 0050). `dac.out()` is then the
    /// block for the DACs.
    pub fn limit(&mut self, dac: &mut DacBlocks, sample_rate: u32) {
        self.limiter.process(dac, sample_rate);
    }

    /// The reverb, for reading its ring.
    pub fn reverb(&self) -> &RingReverb {
        &self.reverb
    }

    /// The compressor's gain reduction, dB, for the GR meter.
    pub fn master_gr_db(&self) -> f32 {
        self.comp.gr_db()
    }
}

fn add(acc: &mut [f32; BLOCK_SIZE], x: &[f32; BLOCK_SIZE]) {
    for (a, &v) in acc.iter_mut().zip(x) {
        *a += v;
    }
}

/// Clamped to 0..1; NaN reads as 0.
fn unit(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}
