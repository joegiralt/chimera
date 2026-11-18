//! The proof that a plain block plays the saw alone (ADR 0283), sealed:
//! only `VarOsc::on_anchor` builds an `OnAnchor`, and it holds the
//! oscillator it was built on, so it plays that one and no other, and
//! nothing can change the oscillator's held pair between the check and the
//! run.

use chimera_hal::BLOCK_SIZE;

use super::{HeldPair, Pair, PulseQuiet, Ramps, Saw, VarOsc, Wave, ZEROS};
use crate::dsp::va::stability::WrapJitter;

/// A plain block on the saw's anchor, and the oscillator it plays on:
/// WAVE on the saw's anchor at both ends of the block (so through it: the
/// ramp is linear), and the pulse's held constants, which the run lerps
/// from, its weight-0 ones.
pub struct OnAnchor<'a> {
    o: &'a mut VarOsc,
    r: &'a Ramps,
}

impl VarOsc {
    /// The block on `r` as the saw alone (`OnAnchor`), or the oscillator
    /// back for its pair's runs. A block whose held constants are the
    /// pulse's at another WAVE (a LOOPFM block between, which holds none)
    /// plays the pair, which lerps them home.
    #[inline(always)]
    pub fn on_anchor<'a>(&'a mut self, r: &'a Ramps) -> Result<OnAnchor<'a>, &'a mut VarOsc> {
        let saw = Wave::Saw.anchor();
        // the run reads WAVE from sample 0 (a new segment's start) to the
        // block's end; between them it moves one way
        let on = r.wave_at(0) == saw && r.wave_at(BLOCK_SIZE) == saw;
        let held_quiet = match self.held_pair {
            // a new note starts on this block's values
            None => true,
            // the run's start: quiet only if taken on the anchor
            Some(HeldPair {
                pair: Pair::SawPulse,
                wave,
            }) => wave == saw,
            // another pair's start is taken at this block's WAVE
            Some(HeldPair {
                pair: Pair::SineTri | Pair::TriSaw,
                ..
            }) => true,
        };
        if on && held_quiet {
            Ok(OnAnchor { o: self, r })
        } else {
            Err(self)
        }
    }
}

impl<'a> OnAnchor<'a> {
    /// The oscillator, unplayed: its pair's runs play the block.
    #[inline(always)]
    pub fn release(self) -> &'a mut VarOsc {
        self.o
    }

    /// `render`'s plain block on the saw's anchor: one run of the saw, the
    /// pulse quiet (`quiet_kind!`). Every sample `render`'s, bit for bit,
    /// and the pulse's edges left as its run leaves them. Inlined into
    /// `solo::play`, so with `bank2` the run links with it in bank 2.
    #[inline(always)]
    pub(super) fn render<const REC: bool, const FLIP: bool>(
        self,
        out: &mut [f32; BLOCK_SIZE],
        mk: &mut [[f32; 2]; BLOCK_SIZE],
        jit: &mut WrapJitter,
        draw: &mut impl FnMut() -> f32,
    ) {
        let OnAnchor { o, r } = self;
        let blk = o.block::<false, false, false>(r, BLOCK_SIZE, jit.factor());
        #[cfg(any(test, feature = "test-support"))]
        {
            o.runs = 1;
        }
        o.run_in::<Saw, PulseQuiet, BLOCK_SIZE, REC, false, FLIP, false, false, false>(
            &blk,
            (0, BLOCK_SIZE),
            r,
            out,
            mk,
            jit.factor_mut(),
            draw,
            &ZEROS,
            (0.0, 0.0),
        );
    }
}
