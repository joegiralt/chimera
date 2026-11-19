pub use chimera_engine::fmt_buf::FmtBuf;

use crate::ui::page::ValFmt;

/// Format a normalized 0..1 value for display.
pub fn fmt_val(buf: &mut FmtBuf, val: f32, fmt: ValFmt) {
    use core::fmt::Write;
    match fmt {
        ValFmt::Hold if val >= 0.999 => {
            let _ = buf.write_str("HOLD");
        }
        ValFmt::Inf if val >= 0.999 => {
            let _ = buf.write_str("INF");
        }
        ValFmt::Uni | ValFmt::Hold | ValFmt::Inf => {
            let midi = (val * 127.0 + 0.5) as i32;
            let _ = write!(buf, "{}", midi);
        }
        ValFmt::Bi => {
            let midi = (val * 127.0 + 0.5) as i32;
            let v = midi - 64;
            if v > 0 {
                let _ = write!(buf, "+{}", v);
            } else {
                let _ = write!(buf, "{}", v);
            }
        }
        ValFmt::Pan => {
            let v = (val * 127.0 + 0.5) as i32 - 64;
            let _ = match v {
                0 => buf.write_str("C"),
                v if v < 0 => write!(buf, "L{}", -v),
                v => write!(buf, "R{}", v),
            };
        }
        ValFmt::Keybd if matches!(crate::ui::renderer::amount_of(val), 0 | 64 | 127) => {
            let _ = buf.write_str(match crate::ui::renderer::amount_of(val) {
                0 => "OFF",
                64 => "HALF",
                _ => "FULL",
            });
        }
        ValFmt::Route | ValFmt::Keybd => {
            let a = crate::ui::renderer::amount_of(val) as i32;
            let pct = (a * 100 + a.signum() * 63) / 127;
            let _ = if pct > 0 {
                write!(buf, "+{pct}%")
            } else {
                write!(buf, "{pct}%")
            };
        }
        ValFmt::Int(max) => {
            let _ = write!(buf, "{}", discrete(val, max));
        }
        ValFmt::OneBased(max) => {
            let _ = write!(buf, "{}", discrete(val, max) as u16 + 1);
        }
        ValFmt::Signed(n) => {
            let v = discrete(val, n.saturating_mul(2)) as i16 - n as i16;
            let _ = if v > 0 {
                write!(buf, "+{v}")
            } else {
                write!(buf, "{v}")
            };
        }
        ValFmt::Names(names) => {
            if let Some(name) = names.get(discrete(val, fmt.max_int()) as usize) {
                let _ = buf.write_str(name);
            }
        }
        ValFmt::Law(law) => fmt_law(buf, val, law),
        ValFmt::Bpm => {
            use crate::orbit::clock::Bpm;
            let span = (Bpm::MAX - Bpm::MIN) as f32;
            let _ = write!(buf, "{}", Bpm::MIN + libm::roundf(val * span) as u16);
        }
        // The stored value, not 0..1: the range isn't in the format.
        ValFmt::LogHz { base, per_octave } => {
            hz_text(
                buf,
                libm::roundf(f32::from(base) * libm::exp2f(val / f32::from(per_octave))) as i32,
            );
        }
        ValFmt::Vowel => {
            let _ = buf.write_str(["U", "O", "A", "E", "I"][((val / 25.0 + 0.5) as usize).min(4)]);
        }
        ValFmt::Knob(m, k) => fmt_knob(buf, val, m, k),
    }
}

/// A DRUM knob at `val` (0..1 of its travel), in its machine's unit: the
/// law's value, as `laws::resolve` hears it. Nothing for a knob the
/// machine lacks.
fn fmt_knob(
    buf: &mut FmtBuf,
    val: f32,
    m: crate::dsp::drum::types::Machine,
    k: crate::dsp::drum::types::Knob,
) {
    use crate::dsp::drum::laws::{ValFmt as F, spec};
    use crate::dsp::drum::types::KnobValue;
    use core::fmt::Write;
    let Some(s) = spec(m).knob(k) else {
        return;
    };
    let pos = chimera_midi::value::U7::new(libm::roundf(val * 127.0) as u8)
        .unwrap_or(chimera_midi::value::U7::MAX);
    let x = s.law.at(KnobValue::new(pos));
    let sign = if x > 0.0 { "+" } else { "" };
    match s.fmt {
        F::Hz => hz_text(buf, libm::roundf(x) as i32),
        F::Time if x < 1.0 => fixed(buf, x * 1000.0, 0, " ms"),
        F::Time => fixed(buf, x, 2, " s"),
        F::Semis => {
            let _ = buf.write_str(sign);
            fixed(buf, x, 1, "");
        }
        F::Ratio => fixed(buf, x, 2, "x"),
        F::Amount => fixed(buf, x * 100.0, 0, ""),
        F::Count => fixed(buf, x, 0, ""),
        F::Octaves => fixed(buf, x, 2, " OCT"),
        // Below the middle a low-pass closing, above a high-pass opening.
        F::Tilt => {
            let n = libm::roundf((2.0 * x - 1.0) * 100.0);
            if n == 0.0 {
                let _ = buf.write_str("FLAT");
            } else {
                let _ = buf.write_str(if n < 0.0 { "LP " } else { "HP " });
                fixed(buf, libm::fabsf(n), 0, "");
            }
        }
    }
}

/// `x` to `places` decimals (none: a whole number), then `unit` as given,
/// in integers: `{:.1}` would link core's float formatting, several KB.
pub(crate) fn fixed(buf: &mut FmtBuf, x: f32, places: u32, unit: &str) {
    let k = 10i32.pow(places);
    let n = libm::roundf(x * k as f32) as i32;
    let sign = if n < 0 { "-" } else { "" };
    // `as` saturates (-inf is `i32::MIN`), whose `abs` would overflow.
    let n = n.unsigned_abs();
    let k = k as u32;
    let _ = if places == 0 {
        write!(buf, "{sign}{n}{unit}")
    } else {
        write!(buf, "{sign}{}.{:02$}{unit}", n / k, n % k, places as usize)
    };
}

/// Whole Hz under 1 kHz, then kHz to 3 figures.
fn hz_text(buf: &mut FmtBuf, hz: i32) {
    if hz < 1000 {
        let _ = write!(buf, "{hz} Hz");
    } else if hz < 9995 {
        fixed(buf, hz as f32 / 1000.0, 2, " kHz");
    } else {
        fixed(buf, hz as f32 / 1000.0, 1, " kHz");
    }
}

fn fmt_law(buf: &mut FmtBuf, v: f32, law: crate::dsp::modulator::law::Law) {
    use crate::dsp::modulator::law::{CUTOFF_HZ, Law};
    use core::fmt::Write;
    let round = |x: f32| libm::roundf(x) as i32;
    let bend = |buf: &mut FmtBuf, lo: &str, mid: &str, hi: &str| {
        let n = round((2.0 * v - 1.0) * 100.0);
        let _ = match n {
            0 => buf.write_str(mid),
            n if n < 0 => write!(buf, "{lo} {}", -n),
            n => write!(buf, "{hi} {n}"),
        };
    };
    match law {
        Law::Pct => {
            let _ = write!(buf, "{}%", round(v * 100.0));
        }
        // Degrees; the u8g2 face has no "°".
        Law::Phase => {
            let _ = write!(buf, "{}", round(v * 360.0));
        }
        Law::Curve => bend(buf, "LOG", "LIN", "EXP"),
        Law::Tilt => bend(buf, "SAW", "TRI", "RAMP"),
        Law::BRate | Law::BurstRate | Law::LfoRate | Law::LfoHz => {
            // Each unit is picked from the value as it would print.
            let hz = match law {
                Law::LfoHz => {
                    let s = &crate::dsp::lfo::LFO_SPECS[0];
                    s.min + v * (s.max - s.min)
                }
                _ => law.range().map_or(0.0, |r| r.at(v)),
            };
            // CLASSIC RATE turns a semitone a detent (ADR 0279): under 1 Hz
            // places enough that each one reads new.
            let places = match law {
                Law::LfoHz if round(hz * 1000.0) < 100 => 4,
                Law::LfoHz if round(hz * 1000.0) < 1000 => 3,
                _ => 2,
            };
            if round(hz * 100.0) < 1000 {
                fixed(buf, hz, places, " Hz");
            } else if round(hz * 10.0) < 1000 {
                fixed(buf, hz, 1, " Hz");
            } else {
                let _ = write!(buf, "{} Hz", round(hz));
            }
        }
        // Whole Hz under 1 kHz, then kHz to 3 figures.
        Law::Cutoff => hz_text(buf, round(CUTOFF_HZ.at(v))),
        // The engine's laws, bank 2's: the door hands these formats out
        // only open (`view::fmt_of`).
        Law::StretchSpeed | Law::ScrubSpeed | Law::FadeHold | Law::ScrubLength | Law::DragTime => {
            crate::bank2::ui_door(|u| u.law_text(buf, law, v));
        }
        _ => {
            let s = law.range().map_or(0.0, |r| r.at(v));
            let ms = s * 1000.0;
            if round(ms * 10.0) < 100 {
                fixed(buf, ms, 1, " ms");
            } else if round(ms) < 1000 {
                let _ = write!(buf, "{} ms", round(ms));
            } else {
                fixed(buf, s, 1, " s");
            }
        }
    }
}

/// Normalized `val` rounded to the nearest of 0..=max.
pub(crate) fn discrete(val: f32, max: u8) -> u8 {
    let v = (val * max as f32 + 0.5) as u8;
    if v > max { max } else { v }
}

#[cfg(test)]
mod tests {
    use super::{FmtBuf, fixed};

    /// Out-of-range values saturate, never overflow: +-inf print the
    /// `i32` ends, NaN prints 0.
    #[test]
    fn fixed_saturates() {
        let at = |x: f32, places| {
            let mut b = FmtBuf::new();
            fixed(&mut b, x, places, "ms");
            b
        };
        assert_eq!(at(f32::NEG_INFINITY, 0).as_str(), "-2147483648ms");
        assert_eq!(at(f32::NEG_INFINITY, 2).as_str(), "-21474836.48ms");
        assert_eq!(at(f32::INFINITY, 1).as_str(), "214748364.7ms");
        assert_eq!(at(f32::NAN, 2).as_str(), "0.00ms");
        assert_eq!(at(-3.6, 1).as_str(), "-3.6ms");
        assert_eq!(at(0.37, 2).as_str(), "0.37ms");
    }
}
