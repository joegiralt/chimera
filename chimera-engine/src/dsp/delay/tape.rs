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
    match (d.fade > 0, e.moving()) {
        (false, false) => span::<false, false>(d, buf, &tap, e, insert),
        (false, true) => span::<false, true>(d, buf, &tap, e, insert),
        (true, false) => span::<true, false>(d, buf, &tap, e, insert),
        (true, true) => span::<true, true>(d, buf, &tap, e, insert),
    }
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
        d.buffer[write_pos] = to_line(dry + saturated * now(f, i));
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
    if FADE {
        d.fade -= BLOCK_SIZE as u16;
        if d.fade == 0 {
            d.time = next;
        }
    }
}
