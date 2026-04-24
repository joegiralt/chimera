/// TX81Z 8 waveform functions.
///
/// Ported line-for-line from p81z (TX81Z_extra.cpp / getWaveshape).
/// Phase is 0.0–1.0 (one full cycle). Output is approximately –1.0 to +2.0.

#[inline(always)]
fn sin_of_phase(p: f32) -> f32 {
    libm::sinf(p * core::f32::consts::TAU)
}

#[inline(always)]
fn sin_of_2phase(p: f32) -> f32 {
    libm::sinf(p * 2.0 * core::f32::consts::TAU)
}

/// Compute TX81Z waveform value at given phase (0.0–1.0).
pub fn compute(waveform: u8, phase: f32) -> f32 {
    match waveform {
        0 => sin_of_phase(phase),
        1 => {
            if phase < 0.25 {
                sin_of_phase(phase - 0.25) + 1.0
            } else if phase < 0.5 {
                sin_of_phase(phase + 0.25) + 1.0
            } else if phase < 0.75 {
                sin_of_phase(phase - 0.25) - 1.0
            } else {
                sin_of_phase(phase + 0.25) - 1.0
            }
        }
        2 => {
            if phase < 0.5 {
                sin_of_phase(phase)
            } else {
                0.0
            }
        }
        3 => {
            if phase < 0.25 {
                sin_of_phase(phase - 0.25) + 1.0
            } else if phase < 0.5 {
                sin_of_phase(phase + 0.25) + 1.0
            } else {
                0.0
            }
        }
        4 => {
            if phase < 0.5 {
                sin_of_phase(2.0 * phase)
            } else {
                0.0
            }
        }
        5 => {
            if phase < 0.125 {
                sin_of_2phase(phase - 0.125) + 1.0
            } else if phase < 0.25 {
                sin_of_2phase(phase + 0.125) + 1.0
            } else if phase < 0.375 {
                sin_of_2phase(phase - 0.125) - 1.0
            } else if phase < 0.5 {
                sin_of_2phase(phase + 0.125) - 1.0
            } else {
                0.0
            }
        }
        6 => {
            if phase < 0.25 {
                sin_of_2phase(phase)
            } else if phase < 0.5 {
                -sin_of_2phase(phase)
            } else {
                0.0
            }
        }
        7 => {
            if phase < 0.125 {
                1.0 + sin_of_2phase(phase - 0.125)
            } else if phase < 0.25 {
                1.0 + sin_of_2phase(phase + 0.125)
            } else if phase < 0.375 {
                1.0 - sin_of_2phase(phase - 0.125)
            } else if phase < 0.5 {
                1.0 - sin_of_2phase(phase + 0.125)
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}
