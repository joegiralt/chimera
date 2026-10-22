//! TAPE's sample loop and the TYPE's dispatch, bank 2's (ADR 0212):
//! `TapeDelay::run` calls `run` through the door.

#[cfg(any(feature = "bank2", not(target_os = "none")))]
use super::DelayType;
use super::{
    DelayParams, Eased, FLUTTER_SAMPLES, MAX_DELAY_SAMPLES, TIME_FADE, Tap, TapeDelay, WOW_SAMPLES,
    read, to_line,
};
use crate::dsp::clampf;
use crate::dsp::ease::{at, step_of};
use chimera_hal::BLOCK_SIZE;

/// The door's entry (`DelayDoor::run`): one block of the delay. GRAIN, or
/// TAPE fading back from it, goes on to `grain::run`; a firmware build
/// without bank 2 has no room for GRAIN and plays TAPE (ADR 0202).
/// Whether `buf` is known all +0: the idle skip's (#793).
#[inline(never)]
pub fn run(
    d: &mut TapeDelay,
    buf: &mut [f32; BLOCK_SIZE],
    params: &DelayParams,
    sample_rate: u32,
    insert: bool,
) -> bool {
    #[cfg(any(feature = "bank2", not(target_os = "none")))]
    if params.kind() == DelayType::Grain || d.grain.share != crate::dsp::Unit::ZERO {
        // GRAIN writes the line its own way: TAPE's count starts again.
        d.zeros = 0;
        super::grain::run(d, buf, params, sample_rate, insert);
        return false;
    }
    let tap = Tap::new(params, sample_rate);
    if !d.primed {
        (d.time, d.primed) = (tap.base(), true);
    }
    if d.fade == 0 && tap.base() != d.time {
        (d.next, d.fade) = (tap.base(), TIME_FADE);
    }
    let e = d.eased(params, sample_rate);
    if d.fade == 0 && !e.moving() && idle(d, buf, &tap, &e, insert, sample_rate) {
        // The block `span` would play, every bit of it: the transport moves
        // as each sample moves it; the line, its filter and the return
        // (the send's own +0s) stay as they are.
        for _ in 0..BLOCK_SIZE {
            d.transport.advance(&tap);
        }
        d.write_pos += BLOCK_SIZE;
        if d.write_pos >= MAX_DELAY_SAMPLES {
            d.write_pos -= MAX_DELAY_SAMPLES;
        }
        d.zeros = d.zeros.saturating_add(BLOCK_SIZE as u32);
        crate::dsp::skipped(crate::dsp::Skip::Delay);
        return true;
    }
    match (d.fade > 0, e.moving()) {
        (false, false) => span::<false, false>(d, buf, &tap, e, insert),
        (false, true) => span::<false, true>(d, buf, &tap, e, insert),
        (true, false) => span::<true, false>(d, buf, &tap, e, insert),
        (true, true) => span::<true, true>(d, buf, &tap, e, insert),
    }
    false
}

/// Whether a steady, unfaded block of `span` would leave everything but the
/// transport and the write head as it is (#793): the send all +0, the line
/// all zero (`zeros`), so the head reads +0 at any finite delay; the tone's
/// state a fixed point of its one-pole on that +0, and the write it then
/// makes, `to_line` of the send plus its saturated feedback, 0; and the
/// return +0, which the send's +0s already are. Each test is `span`'s own
/// arithmetic on those values.
#[inline(always)]
fn idle(
    d: &TapeDelay,
    buf: &[f32; BLOCK_SIZE],
    tap: &Tap,
    e: &Eased,
    insert: bool,
    fs: u32,
) -> bool {
    if !crate::dsp::may_skip()
        || d.zeros < MAX_DELAY_SAMPLES as u32
        || !crate::dsp::all_pos_zero(buf)
        || fs == 0
        || !(tap.base.is_finite() && tap.wow.is_finite() && tap.flutter.is_finite())
        || !d.time.is_finite()
        || !d.transport.is_finite()
    {
        return false;
    }
    let lp = d.lp_state;
    let c = 0.2 + e.tone.0 * 0.75;
    if !lp.is_finite() || (lp + c * (0.0 - lp)).to_bits() != lp.to_bits() {
        return false;
    }
    let g = 1.0 + e.saturation.0 * 3.0;
    let saturated = tanhf(lp * g) * (1.0 / g);
    let mix = e.mix.0;
    let dry_gain = if insert { 1.0 - mix } else { 0.0 };
    to_line(0.0 + saturated * e.feedback.0) == 0 && (0.0 * dry_gain + 0.0 * mix).to_bits() == 0
}

/// The block: `FADE` while a TIME crossfade runs, `RAMP` while any
/// eased setting moves (each ramps from its `.0` to its `.1`; one
/// that isn't moving ramps by 0, so plays its value).
#[inline(always)]
fn span<const FADE: bool, const RAMP: bool>(
    d: &mut TapeDelay,
    buf: &mut [f32; BLOCK_SIZE],
    tap: &Tap,
    e: Eased,
    insert: bool,
) {
    let ramp = |v: (f32, f32)| (v.0, step_of(v, BLOCK_SIZE));
    // Tone: LP coefficient (higher = brighter)
    let lp = |t: f32| 0.2 + t * 0.75;
    let lp_coeff = ramp((lp(e.tone.0), lp(e.tone.1)));
    let gain = |s: f32| 1.0 + s * 3.0;
    let (g0, g1) = (gain(e.saturation.0), gain(e.saturation.1));
    let sat_gain = ramp((g0, g1));
    // Once a block: the loop multiplies, never divides (within 1 ulp).
    // Ramped, the inverse is linear between the ends: still at most 1.
    let sat_inv = ramp((1.0 / g0, 1.0 / g1));
    let (m, f, mech) = (ramp(e.mix), ramp(e.feedback), ramp(e.mechanics));
    // MECHANICS at 1: ramped, the transport's offsets × its depth.
    let unit = Tap {
        wow: WOW_SAMPLES,
        flutter: FLUTTER_SAMPLES,
        ..*tap
    };
    let now = |(v, step): (f32, f32), i: usize| if RAMP { at(v, step, i) } else { v };
    let top = (MAX_DELAY_SAMPLES - 2) as f32;

    // The loop's state in locals: the stores into the line cannot then
    // make the compiler reload and store it every sample.
    let (mut transport, mut write_pos, mut lp_state) = (d.transport, d.write_pos, d.lp_state);
    let (time, next) = (d.time, d.next);
    // From the old head to the new, linearly: the new head's weight.
    let dt = 1.0 / f32::from(TIME_FADE);
    let mut t = 1.0 - f32::from(d.fade) * dt;
    // Every write ORed: 0 if the block wrote only zeros.
    let mut written = 0i16;
    for (i, s) in buf.iter_mut().enumerate() {
        let dry = *s;

        let (wow, flutter) = if RAMP {
            let ((w, f), depth) = (transport.offsets(&unit), now(mech, i));
            (w * depth, f * depth)
        } else {
            transport.offsets(tap)
        };
        let head = |base: f32| clampf(base + wow + flutter, 1.0, top);
        let mut delayed = read(&d.buffer, write_pos, head(time));
        if FADE {
            delayed += (read(&d.buffer, write_pos, head(next)) - delayed) * t;
            t += dt;
        }

        // Tone: one-pole LP in feedback path (tape loses highs each pass)
        lp_state += now(lp_coeff, i) * (delayed - lp_state);
        let filtered = lp_state;

        // Tape saturation in the feedback path, always on (ADR 0038): at
        // SAT 0 the loop is otherwise linear with unity DC gain, so FDBK
        // 1 grows without bound. Bounded by 1 / gain, the write stays
        // within |dry| + FDBK: ±2.0, the line's full scale, at a unity
        // send.
        let saturated = tanhf(filtered * now(sat_gain, i)) * now(sat_inv, i);

        // Write: input + feedback
        let w = to_line(dry + saturated * now(f, i));
        d.buffer[write_pos] = w;
        written |= w;
        write_pos += 1;
        if write_pos == MAX_DELAY_SAMPLES {
            write_pos = 0;
        }

        // Mix
        let mix = now(m, i);
        let dry_gain = if insert { 1.0 - mix } else { 0.0 };
        *s = dry * dry_gain + delayed * mix;
    }
    (d.transport, d.write_pos, d.lp_state) = (transport, write_pos, lp_state);
    d.zeros = if written == 0 {
        d.zeros.saturating_add(BLOCK_SIZE as u32)
    } else {
        0
    };
    if FADE {
        d.fade -= BLOCK_SIZE as u16;
        if d.fade == 0 {
            d.time = next;
        }
    }
}

// `tanhf` and `expm1f` below are libm 0.2.16's (rust-lang/libm, MIT OR
// Apache-2.0), from musl, from FreeBSD's msun `s_tanhf.c` and
// `s_expm1f.c`:
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================
//
// Conversion to float by Ian Lance Taylor, Cygnus Support, ian@cygnus.com.

/// `libm::tanhf`, the same operations in the same order, inlined into the
/// loop: bit for bit libm's at every input (`tanh_is_libms`), without the
/// call, its register spills and its flag-raising stores (#793). Those
/// stores (`force_eval!`) change no value and are left out.
#[inline(always)]
fn tanhf(mut x: f32) -> f32 {
    let mut ix = x.to_bits();
    let sign = (ix >> 31) != 0;
    ix &= 0x7fffffff;
    x = f32::from_bits(ix);
    let w = ix;
    let tt = if w > 0x3f0c9f54 {
        // |x| > log(3)/2 ~= 0.5493 or NaN
        if w > 0x41200000 {
            // |x| > 10
            1. + 0. / x
        } else {
            let t = expm1f(2. * x);
            1. - 2. / (t + 2.)
        }
    } else if w > 0x3e82c578 {
        // |x| > log(5/3)/2 ~= 0.2554
        let t = expm1f(2. * x);
        t / (t + 2.)
    } else if w >= 0x00800000 {
        // |x| >= 0x1p-126
        let t = expm1f(-2. * x);
        -t / (t + 2.)
    } else {
        // |x| is subnormal
        x
    };
    if sign { -tt } else { tt }
}

/// `libm::expm1f` as `tanhf` needs it, the same operations in the same order.
/// Its constants as libm writes them, each the same `f32`.
#[inline(always)]
#[allow(clippy::excessive_precision)]
fn expm1f(mut x: f32) -> f32 {
    const LN2_HI: f32 = 6.9313812256e-01; // 0x3f317180
    const LN2_LO: f32 = 9.0580006145e-06; // 0x3717f7d1
    const INV_LN2: f32 = 1.4426950216e+00; // 0x3fb8aa3b
    const Q1: f32 = -3.3333212137e-2; // -0x888868.0p-28
    const Q2: f32 = 1.5807170421e-3; //  0xcf3010.0p-33
    let x1p127 = f32::from_bits(0x7f000000);
    let mut hx = x.to_bits();
    let sign = (hx >> 31) != 0;
    hx &= 0x7fffffff;
    // Huge and non-finite arguments.
    if hx >= 0x4195b844 {
        // |x| >= 27·ln2
        if hx > 0x7f800000 {
            return x;
        }
        if sign {
            return -1.;
        }
        if hx > 0x42b17217 {
            // x > log(FLT_MAX)
            x *= x1p127;
            return x;
        }
    }
    let k: i32;
    let hi: f32;
    let lo: f32;
    let mut c = 0f32;
    // Argument reduction.
    if hx > 0x3eb17218 {
        // |x| > 0.5·ln2
        if hx < 0x3F851592 {
            // and |x| < 1.5·ln2
            if !sign {
                hi = x - LN2_HI;
                lo = LN2_LO;
                k = 1;
            } else {
                hi = x + LN2_HI;
                lo = -LN2_LO;
                k = -1;
            }
        } else {
            k = (INV_LN2 * x + (if sign { -0.5 } else { 0.5 })) as i32;
            let t = k as f32;
            hi = x - t * LN2_HI; // t·ln2_hi is exact here
            lo = t * LN2_LO;
        }
        x = hi - lo;
        c = (hi - x) - lo;
    } else if hx < 0x33000000 {
        // |x| < 2^-25
        return x;
    } else {
        k = 0;
    }
    // x is now in the primary range.
    let hfx = 0.5 * x;
    let hxs = x * hfx;
    let r1 = 1. + hxs * (Q1 + hxs * Q2);
    let t = 3. - r1 * hfx;
    let mut e = hxs * ((r1 - t) / (6. - x * t));
    if k == 0 {
        // c is 0
        return x - (x * e - hxs);
    }
    e = x * (e - c) - c;
    e -= hxs;
    // exp(x) ~ 2^k (x_reduced - e + 1)
    if k == -1 {
        return 0.5 * (x - e) - 0.5;
    }
    if k == 1 {
        if x < -0.25 {
            return -2. * (e - (x + 0.5));
        }
        return 1. + 2. * (x - e);
    }
    let twopk = f32::from_bits(((0x7f + k) << 23) as u32); // 2^k
    if !(0..=56).contains(&k) {
        // suffice to return exp(x)-1
        let mut y = x - e + 1.;
        if k == 128 {
            y = y * 2. * x1p127;
        } else {
            y *= twopk;
        }
        return y - 1.;
    }
    let uf = f32::from_bits(((0x7f - k) << 23) as u32); // 2^-k
    if k < 23 {
        (x - e + (1. - uf)) * twopk
    } else {
        (x - (e + uf) + 1.) * twopk
    }
}

#[cfg(test)]
mod tests {
    use super::tanhf;

    fn check(bits: u32) {
        let x = f32::from_bits(bits);
        assert_eq!(tanhf(x).to_bits(), libm::tanhf(x).to_bits(), "{bits:#x}");
    }

    /// Every 61st f32, and every one in the range a tape loop meets most.
    #[test]
    fn tanh_is_libms() {
        for bits in (0..=u32::MAX).step_by(61) {
            check(bits);
        }
        for bits in 0x3e00_0000..0x4120_0000u32 {
            check(bits);
            check(bits | 0x8000_0000);
        }
    }

    /// All 2³² f32: `cargo test --release -p chimera-engine -- --ignored tanh_is_libms_everywhere`.
    #[test]
    #[ignore]
    fn tanh_is_libms_everywhere() {
        for bits in 0..=u32::MAX {
            check(bits);
        }
    }
}
