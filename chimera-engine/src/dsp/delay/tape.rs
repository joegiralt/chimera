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
#[inline(never)]
pub fn run(
    d: &mut TapeDelay,
    buf: &mut [f32; BLOCK_SIZE],
    params: &DelayParams,
    sample_rate: u32,
    insert: bool,
) {
    #[cfg(any(feature = "bank2", not(target_os = "none")))]
    if params.kind() == DelayType::Grain || d.grain.share != crate::dsp::Unit::ZERO {
        // GRAIN writes the line its own way: TAPE's count starts again.
        d.zeros = 0;
        super::grain::run(d, buf, params, sample_rate, insert);
        return;
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
        return;
    }
    match (d.fade > 0, e.moving()) {
        (false, false) => span::<false, false>(d, buf, &tap, e, insert),
        (false, true) => span::<false, true>(d, buf, &tap, e, insert),
        (true, false) => span::<true, false>(d, buf, &tap, e, insert),
        (true, true) => span::<true, true>(d, buf, &tap, e, insert),
    }
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
    let saturated = libm::tanhf(lp * g) * (1.0 / g);
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
        let saturated = libm::tanhf(filtered * now(sat_gain, i)) * now(sat_inv, i);

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
