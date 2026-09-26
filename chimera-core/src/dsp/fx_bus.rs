//! The shared FX bus (instrument-core spec § Audio path): chorus, delay and
//! reverb run once per block on the sum of every part's sends. Send/return:
//! each effect returns only its wet signal, its MIX acting as the return
//! level; the dry signal reaches the DACs through the parts alone. The
//! effects are mono today, so the return is mono and lands on both sides of
//! DAC pair 1.

use chimera_hal::BLOCK_SIZE;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::dsp::chorus::{ChorusParams, JunoChorus};
use crate::dsp::delay::{DelayParams, TapeDelay};
use crate::dsp::reverb::{Reverb, ReverbParams};
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
    /// The values `ParamSnapshot` carried before the FX moved out: all off.
    fn default() -> Self {
        Self {
            chorus: ChorusParams::default(),
            delay: DelayParams::default(),
            reverb: ReverbParams {
                reverb_type: 0,
                time: 0.5,
                damping: 0.3,
                size: 0.5,
                mix: 0.0,
            },
        }
    }
}

pub struct FxBus {
    chorus: JunoChorus,
    delay: TapeDelay,
    reverb: Reverb,
}

crate::in_place::field_list!(FxBus => FxBus { chorus, delay, reverb });

impl Default for FxBus {
    fn default() -> Self {
        Self::new()
    }
}

impl FxBus {
    /// Worst reverb (MidiVerb) with the bus and the Instrument's fixed
    /// mixing; reserved from the voice budget whether or not an effect is on.
    pub const COST: Cost = Cost(3200); // measured 2026-09-26, bench, rev V at 480 MHz

    pub fn new() -> Self {
        Self {
            chorus: JunoChorus::new(),
            delay: TapeDelay::new(),
            reverb: Reverb::new(),
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
            Reverb::init_in_place(uninit_at(addr_of_mut!((*p).reverb)));
            slot.assume_init_mut()
        }
    }

    /// Run each effect that is on over its send (replaced in place by its
    /// wet signal × MIX) and write the sum to `ret`. An effect that is off
    /// returns nothing, so a send into it is silent.
    pub fn process(
        &mut self,
        sends: &mut [[f32; BLOCK_SIZE]; FX_SENDS],
        params: &FxParams,
        sample_rate: u32,
        ret: &mut [f32; BLOCK_SIZE],
    ) {
        ret.fill(0.0);
        let [chorus, delay, reverb] = sends;
        if params.chorus.is_on() {
            self.chorus.process_wet(chorus, &params.chorus, sample_rate);
            add(ret, chorus);
        }
        if params.delay.is_on() {
            self.delay.process_wet(delay, &params.delay, sample_rate);
            add(ret, delay);
        }
        if params.reverb.is_on() {
            self.reverb.process_wet(reverb, &params.reverb);
            add(ret, reverb);
        }
    }
}

fn add(acc: &mut [f32; BLOCK_SIZE], x: &[f32; BLOCK_SIZE]) {
    for (a, &v) in acc.iter_mut().zip(x) {
        *a += v;
    }
}
