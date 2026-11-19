//! The reverb's duck (ADR 0295): its wet return turned down while its own
//! input is loud, coming back in the gaps, as a 2290's dynamic delay or
//! Pro-R 2's ducking. Keyed by the reverb's send (the Parts' REVERB sends
//! and the delay's REV SEND): each block's peak, in dB, into a follower
//! that rises at once and falls the key's whole range, `KEY_FLOOR` to
//! `KEY_FULL`, over RELEASE, so RELEASE is how long the tail takes to come
//! all the way back. The follower's place in that range sets how far the
//! return goes down, `MAX_DEPTH` at AMOUNT 1. The gain ramps across each
//! block; AMOUNT eases over 20 ms.
//!
//! Bank 2's: `run` is out of line, in an S4 group (`*3rev4duck*`), called
//! only from `Reverb::process` (bank 2's). AMOUNT 0, let go, is no call:
//! the return as before, bit for bit.

use crate::dsp::ease::{ease_coeff, step_of};
use crate::dsp::{Db, Gain, Stereo, Unit, fast_exp2, fast_log2};
use crate::hw::BLOCK_SIZE;

/// How far AMOUNT 1 takes the return down with the key at `KEY_FULL`.
pub const MAX_DEPTH: Db = Db::new(-30.0);
/// The key's level at and below which nothing ducks.
pub const KEY_FLOOR: Db = Db::new(-42.0);
/// The key's level from which all of AMOUNT ducks: the follower's top.
pub const KEY_FULL: Db = Db::new(-6.0);

/// dB a doubling: 20·log10(2).
const DB_PER_OCTAVE: f32 = 6.020_6;

/// RELEASE in seconds, on GLIDE TIME's law (1 ms – 2 s, its readout too):
/// the follower's fall from `KEY_FULL` to `KEY_FLOOR`.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Release(f32);

impl Release {
    pub fn of(knob: Unit) -> Release {
        Release(crate::dsp::modulator::law::GLIDE_TIME.at(knob.get()))
    }

    pub fn secs(self) -> f32 {
        self.0
    }

    /// The follower's fall a block at `sr`, dB.
    fn per_block(self, sr: u32) -> f32 {
        let blocks = self.0 * sr as f32 / BLOCK_SIZE as f32;
        (KEY_FULL.get() - KEY_FLOOR.get()) / blocks.max(1.0)
    }
}

/// The duck's state, the reverb's: AMOUNT as played, the follower, the
/// gain the last block ended on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Duck {
    /// From rest, so a first turn eases in too.
    amount: Unit,
    /// `KEY_FLOOR..=KEY_FULL`.
    env: Db,
    gain: Gain,
}

impl Duck {
    pub const REST: Duck = Duck {
        amount: Unit::ZERO,
        env: KEY_FLOOR,
        gain: Gain::UNITY,
    };

    /// AMOUNT `to` is 0 and the duck has let go: nothing to do, the return
    /// as it is.
    #[inline(always)]
    pub fn idle(&self, to: Unit) -> bool {
        to == Unit::ZERO && self.gain == Gain::UNITY && self.amount == Unit::ZERO
    }
}

/// A block's peak in dB, held to the key's range.
fn key_db(peak: f32) -> Db {
    let db = if peak > 0.0 {
        DB_PER_OCTAVE * fast_log2(peak)
    } else {
        KEY_FLOOR.get()
    };
    Db::new(db.clamp(KEY_FLOOR.get(), KEY_FULL.get()))
}

/// How far the follower at `env` asks to duck: 0 at `KEY_FLOOR`, 1 at
/// `KEY_FULL`.
pub fn key_share(env: Db) -> Unit {
    Unit::new((env.get() - KEY_FLOOR.get()) / (KEY_FULL.get() - KEY_FLOOR.get()))
}

/// The return's gain at AMOUNT `amount` with the key's share `share`.
pub fn gain_of(amount: Unit, share: Unit) -> Gain {
    let db = MAX_DEPTH.get() * amount.get() * share.get();
    Gain::new(fast_exp2(db / DB_PER_OCTAVE))
}

/// One block: `send` (the reverb's input) keys the follower; `out`, the
/// wet return, is ramped from the last block's gain to this one's.
#[inline(never)]
#[optimize(size)]
pub fn run(
    d: &mut Duck,
    send: &[f32; BLOCK_SIZE],
    amount: Unit,
    release: Unit,
    sr: u32,
    out: &mut Stereo,
) {
    // AMOUNT's 20 ms ease, landing within −100 dB of its target.
    let (t, k) = (amount.get(), ease_coeff(sr));
    let a = t + k * (d.amount.get() - t);
    let a = if libm::fabsf(a - t) < 1e-5 { t } else { a };
    d.amount = Unit::new(a);
    let peak = key_db(send.iter().fold(0.0f32, |m, x| m.max(libm::fabsf(*x))));
    let fall = Release::of(release).per_block(sr);
    let fallen = Db::new((d.env.get() - fall).max(KEY_FLOOR.get()));
    d.env = if peak > fallen { peak } else { fallen };
    let to = if a == 0.0 || d.env == KEY_FLOOR {
        // Let go exactly: AMOUNT at 0, or the key at its floor.
        Gain::UNITY
    } else {
        gain_of(Unit::new(a), key_share(d.env))
    };
    let from = core::mem::replace(&mut d.gain, to);
    if from == Gain::UNITY && to == Gain::UNITY {
        return;
    }
    let step = step_of((from.get(), to.get()), BLOCK_SIZE);
    let mut g = from.get();
    for (l, r) in out.l.iter_mut().zip(out.r.iter_mut()) {
        g += step;
        *l *= g;
        *r *= g;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_ducks_from_its_floor_to_full() {
        assert_eq!(key_share(key_db(0.0)), Unit::ZERO);
        assert_eq!(key_share(key_db(0.001)), Unit::ZERO, "−60 dB");
        assert_eq!(key_share(key_db(1.0)), Unit::ONE, "0 dB");
        let mid = key_share(key_db(0.1)).get();
        assert!((mid - 22.0 / 36.0).abs() < 0.01, "−20 dB: {mid}");
    }

    #[test]
    fn amount_one_at_full_key_is_the_max_depth() {
        let g = gain_of(Unit::ONE, Unit::ONE).get();
        assert!((20.0 * g.log10() - MAX_DEPTH.get()).abs() < 0.01, "{g}");
        assert_eq!(gain_of(Unit::ZERO, Unit::ONE), Gain::UNITY);
    }

    #[test]
    fn release_spans_1_ms_to_2_s() {
        assert!((Release::of(Unit::ZERO).secs() - 0.001).abs() < 1e-5);
        assert!((Release::of(Unit::ONE).secs() - 2.0).abs() < 2e-3);
        let d = Release::of(Unit::new(crate::dsp::reverb::DUCK_RELEASE_DEFAULT)).secs();
        assert!((d - 0.3).abs() < 0.01, "{d}");
    }

    fn ones() -> Stereo {
        Stereo {
            l: [1.0; BLOCK_SIZE],
            r: [1.0; BLOCK_SIZE],
        }
    }

    /// AMOUNT 1 under a loud key ducks, eased in and ramped; key gone, it
    /// lets go within RELEASE; AMOUNT back to 0, it goes idle.
    #[test]
    fn a_duck_eases_in_ramps_and_lets_go_within_release() {
        let mut d = Duck::REST;
        assert!(d.idle(Unit::ZERO));
        let loud = [0.5f32; BLOCK_SIZE];
        let mut out = ones();
        run(&mut d, &loud, Unit::ONE, Unit::ZERO, 48_000, &mut out);
        assert!(out.l[0] > out.l[BLOCK_SIZE - 1], "ramped down, not stepped");
        for _ in 0..200 {
            out = ones();
            run(&mut d, &loud, Unit::ONE, Unit::ZERO, 48_000, &mut out);
        }
        let ducked = out.l[BLOCK_SIZE - 1];
        assert!(
            (20.0 * ducked.log10() - MAX_DEPTH.get()).abs() < 0.1,
            "{ducked}"
        );
        // 300 ms is 225 blocks: back to unity by then.
        let release = Unit::new(crate::dsp::reverb::DUCK_RELEASE_DEFAULT);
        let quiet = [0.0f32; BLOCK_SIZE];
        let mut back_at = None;
        for b in 0..300 {
            out = ones();
            run(&mut d, &quiet, Unit::ONE, release, 48_000, &mut out);
            if back_at.is_none() && out.l[BLOCK_SIZE - 1] == 1.0 {
                back_at = Some(b);
            }
        }
        let b = back_at.expect("let go");
        assert!((200..=226).contains(&b), "back at block {b}");
        for _ in 0..400 {
            out = ones();
            run(&mut d, &quiet, Unit::ZERO, release, 48_000, &mut out);
        }
        assert!(d.idle(Unit::ZERO), "{d:?}");
    }
}
