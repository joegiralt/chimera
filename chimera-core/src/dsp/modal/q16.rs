//! A string's delay line, stored as 16-bit block float (ADR 0052) or as
//! plain f32. All maths stays f32; only the storage differs.
//!
//! A `Q16` line stores `q = sat16(round(x · 2^e))` and reads `q · 2^−e`,
//! one exponent `e` for the whole line. Full scale at `e` is `2^(15−e)`:
//! ±2.0 at 14. Once a period the exponent steps to keep the signal
//! between ¼ and ½ of full scale, so resolution follows a decaying tail
//! down and the tail goes quiet as in f32, neither limit-cycling (plain
//! rounding) nor halving (truncation).

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use super::string::MAX_STRING_DELAY;

/// A line's exponent: always `START..=MAX`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Exp(u8);

impl Exp {
    /// Each note's first: full scale ±2.0 holds the feedback clamp (±1.5)
    /// and the sympathetic injection, so the first period can't clip.
    pub const START: Exp = Exp(14);
    /// The finest. A tail must decay well below the silence threshold
    /// (0.001): Sympathetic's main string drives seven high-Q strings, and
    /// capped at 24 it limit-cycles near 3.5e-5, which keeps them ringing
    /// and the voice from freeing. At 30 a 16-bit sample times 2^−30 is
    /// still an exact, normal f32.
    pub const MAX: Exp = Exp(30);

    pub fn get(self) -> u8 {
        self.0
    }
}

/// `2^e`, exact: `e` is well inside f32's normal exponents.
#[inline]
fn pow2(e: i32) -> f32 {
    f32::from_bits(((127 + e) as u32) << 23)
}

/// `x` at `e`, rounded to nearest, ties away from zero: `as` truncates, so
/// adding ±0.5 first rounds without `roundf`'s bit twiddling. It differs
/// from `roundf` only within an f32 epsilon of a tie. An overshoot clips,
/// never wraps, and NaN stores 0: `as i32` saturates and maps NaN to 0
/// (one `vcvt` on the M7), and the clamp to i16 is one `ssat`.
#[inline]
pub fn store(x: f32, e: Exp) -> i16 {
    let q = (x * pow2(i32::from(e.0)) + 0.5_f32.copysign(x)) as i32;
    q.clamp(i16::MIN.into(), i16::MAX.into()) as i16
}

#[inline]
pub fn load(q: i16, e: Exp) -> f32 {
    f32::from(q) * pow2(-i32::from(e.0))
}

/// The exponent after a period whose largest |q| was `peak`: up below ¼
/// full scale (|q| 8192), down above ½ (16384).
pub fn next_exp(peak: u16, e: Exp) -> Exp {
    if peak < 8192 && e.0 < Exp::MAX.0 {
        Exp(e.0 + 1)
    } else if peak > 16384 && e.0 > Exp::START.0 {
        Exp(e.0 - 1)
    } else {
        e
    }
}

/// A voice's exponent steps for one block: one, so a block pays for at
/// most one rescaled line.
pub struct StepBudget(bool);

impl StepBudget {
    pub const fn one() -> Self {
        Self(true)
    }

    /// Whether a step is left, spending it.
    pub fn take(&mut self) -> bool {
        core::mem::replace(&mut self.0, false)
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for [f32; super::MAX_STRING_DELAY] {}
    impl Sealed for super::Q16 {}
}

/// A string's delay line, indexed `0..MAX_STRING_DELAY`. Sealed:
/// `KsString::init_in_place` trusts `init_in_place` to write every field.
pub trait Store: sealed::Sealed {
    /// Zeros, at `Exp::START`.
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self
    where
        Self: Sized;
    fn load(&self, i: usize) -> f32;
    fn store(&mut self, i: usize, x: f32);
    /// The write position wrapped: a period ended.
    fn wrapped(&mut self, budget: &mut StepBudget);
    /// Zeros, at `Exp::START`: a note starts on a silent line. One memset
    /// of the ring, whatever came before (ADR 0052).
    fn clear(&mut self);
}

/// Today's line: plain f32, nothing to step.
impl Store for [f32; MAX_STRING_DELAY] {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: the slot is valid for one array's writes, and zero bytes
        // are 0.0.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    #[inline]
    fn load(&self, i: usize) -> f32 {
        self[i]
    }

    #[inline]
    fn store(&mut self, i: usize, x: f32) {
        self[i] = x;
    }

    #[inline]
    fn wrapped(&mut self, _: &mut StepBudget) {}

    fn clear(&mut self) {
        self.fill(0.0);
    }
}

/// The 16-bit line: half the bytes of f32's.
pub struct Q16 {
    q: [i16; MAX_STRING_DELAY],
    e: Exp,
    /// The largest |q| written since the write position last wrapped.
    peak: u16,
}

crate::in_place::field_list!(Q16 => Q16 { q, e, peak });

impl Q16 {
    pub fn exp(&self) -> Exp {
        self.e
    }
}

impl Store for Q16 {
    fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; `q` is zero-filled (zero
        // bytes are 0) and `e` and `peak` are written by value, before
        // `assume_init_mut`.
        unsafe {
            addr_of_mut!((*p).q).write_bytes(0, 1);
            addr_of_mut!((*p).e).write(Exp::START);
            addr_of_mut!((*p).peak).write(0);
            slot.assume_init_mut()
        }
    }

    #[inline]
    fn load(&self, i: usize) -> f32 {
        load(self.q[i], self.e)
    }

    #[inline]
    fn store(&mut self, i: usize, x: f32) {
        let v = store(x, self.e);
        self.q[i] = v;
        self.peak = self.peak.max(v.unsigned_abs());
    }

    /// Steps the whole ring, so samples past the loop keep the line's
    /// exponent should a retune lengthen it. A step the budget denies
    /// waits for the next wrap: saturation covers growth meanwhile.
    fn wrapped(&mut self, budget: &mut StepBudget) {
        let next = next_exp(self.peak, self.e);
        if next != self.e && budget.take() {
            if next.0 > self.e.0 {
                // Exact for this period's samples: their peak was below ¼
                // full scale. A stale one past a loop a retune shortened
                // can be louder, and clips at full scale: never a burst.
                for v in &mut self.q {
                    *v = v.saturating_mul(2);
                }
            } else {
                // Round half up.
                for v in &mut self.q {
                    *v = ((i32::from(*v) + 1) >> 1) as i16;
                }
            }
            self.e = next;
        }
        self.peak = 0;
    }

    fn clear(&mut self) {
        self.q.fill(0);
        self.e = Exp::START;
        self.peak = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn q16_saturates_and_steps() {
        let s = Exp::START;
        let stored = [3.0, f32::INFINITY, -3.0, f32::NEG_INFINITY, f32::NAN].map(|x| store(x, s));
        assert_eq!(stored, [32767, 32767, -32768, -32768, 0]);
        assert_eq!(load(store(0.5, s), s), 0.5);

        assert_eq!(next_exp(8191, Exp(14)), Exp(15));
        assert_eq!(next_exp(8192, Exp(14)), Exp(14));
        assert_eq!(next_exp(16385, Exp(20)), Exp(19));
        assert_eq!(next_exp(16384, Exp(20)), Exp(20));
        for e in Exp::START.0..=Exp::MAX.0 {
            for peak in 0..=32768 {
                let n = next_exp(peak, Exp(e)).get();
                assert!(
                    (Exp::START.0..=Exp::MAX.0).contains(&n),
                    "next_exp({peak}, {e}) = {n}"
                );
            }
        }

        let mut line = Q16 {
            q: [0; MAX_STRING_DELAY],
            e: Exp(14),
            peak: 0,
        };
        line.store(0, 4000.0 / 16384.0);
        assert_eq!(line.q[0], 4000);
        line.wrapped(&mut StepBudget::one());
        assert_eq!(line.exp(), Exp(15));
        assert_eq!(line.load(0), 4000.0 * libm::powf(2.0, -14.0));

        let mut spent = StepBudget::one();
        assert!(spent.take());
        line.store(0, 4000.0 / 16384.0);
        line.wrapped(&mut spent);
        assert_eq!(line.exp(), Exp(15), "a spent budget holds the exponent");
    }
}
