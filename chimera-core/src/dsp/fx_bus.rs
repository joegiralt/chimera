//! The shared FX bus (instrument-core spec § Audio path): chorus, delay and
//! reverb run once per block on the sum of every part's sends. Send/return:
//! each effect returns only its wet signal, its MIX acting as the return
//! level; the dry signal reaches the DACs through the parts alone. The
//! chorus and reverb return stereo; the delay is mono, on both sides of
//! DAC pair 1 at unity.

use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::dsp::Stereo;
use crate::dsp::chorus::{ChorusParams, JunoChorus};
use crate::dsp::delay::{DelayParams, TapeDelay};
use crate::dsp::reverb::ReverbParams;
use crate::dsp::ring::RingReverb;
use crate::hw::{Cost, FX_BUS_BUDGET};
use crate::in_place::uninit_at;

/// Sends per part, in this order: chorus, delay, reverb.
pub const FX_SENDS: usize = 3;

// ADR 0014: the FX bus lives in AXI SRAM beside the framebuffer and UI.
const _: () = assert!(core::mem::size_of::<FxBus>() <= FX_BUS_BUDGET);

/// Shared effect settings: one set per Performance, not per Sound.
#[derive(Clone, Copy, Debug)]
pub struct FxParams {
    pub chorus: ChorusParams,
    pub delay: DelayParams,
    pub reverb: ReverbParams,
}

impl Default for FxParams {
    /// Everything off.
    fn default() -> Self {
        Self {
            chorus: ChorusParams::default(),
            delay: DelayParams::default(),
            reverb: ReverbParams::default(),
        }
    }
}

pub struct FxBus {
    chorus: JunoChorus,
    delay: TapeDelay,
    reverb: RingReverb,
}

crate::in_place::field_list!(FxBus => FxBus { chorus, delay, reverb });

impl Default for FxBus {
    fn default() -> Self {
        Self::new()
    }
}

impl FxBus {
    /// The whole bus at its worst settings, with the Instrument's mixing;
    /// reserved from the voice budget whether or not an effect is on.
    pub const COST: Cost = Cost(3310); // MV 3300, measured 2026-09-27, bench, rev V at 480 MHz; rounded up

    pub fn new() -> Self {
        Self {
            chorus: JunoChorus::new(),
            delay: TapeDelay::new(),
            reverb: RingReverb::new(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from `&mut MaybeUninit<Self>` (valid, aligned,
        // unaliased); each effect's in-place constructor initialises its
        // whole field before `assume_init_mut`.
        unsafe {
            JunoChorus::init_in_place(uninit_at(addr_of_mut!((*p).chorus)));
            TapeDelay::init_in_place(uninit_at(addr_of_mut!((*p).delay)));
            RingReverb::init_in_place(uninit_at(addr_of_mut!((*p).reverb)));
            slot.assume_init_mut()
        }
    }

    /// Run each effect that is on over its send and sum the wet returns
    /// into `ret`. An effect that is off returns nothing, so a send into
    /// it is silent.
    pub fn process(
        &mut self,
        sends: &mut [[f32; BLOCK_SIZE]; FX_SENDS],
        params: &FxParams,
        sample_rate: u32,
        ret: &mut Stereo,
    ) {
        *ret = Stereo::SILENT;
        let [chorus, delay, reverb] = sends;
        if params.chorus.is_on() {
            let mut wet = Stereo::SILENT;
            self.chorus
                .process_wet(chorus, &params.chorus, sample_rate, &mut wet);
            add(&mut ret.l, &wet.l);
            add(&mut ret.r, &wet.r);
        }
        if params.delay.is_on() {
            self.delay.process_wet(delay, &params.delay, sample_rate);
            add(&mut ret.l, delay);
            add(&mut ret.r, delay);
        }
        if params.reverb.is_on() {
            let mut wet = Stereo::SILENT;
            self.reverb.process(
                reverb,
                &params.reverb.controls(),
                params.reverb.mix,
                sample_rate,
                &mut wet,
            );
            add(&mut ret.l, &wet.l);
            add(&mut ret.r, &wet.r);
        }
    }
}

fn add(acc: &mut [f32; BLOCK_SIZE], x: &[f32; BLOCK_SIZE]) {
    for (a, &v) in acc.iter_mut().zip(x) {
        *a += v;
    }
}
