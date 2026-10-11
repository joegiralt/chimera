pub use chimera_engine::fmt_buf::FmtBuf;

use crate::ui::page::ValFmt;

/// Format a normalized 0..1 value for display.
pub fn fmt_val(buf: &mut FmtBuf, val: f32, fmt: ValFmt) {
    use core::fmt::Write;
    match fmt {
        ValFmt::Uni => {
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
    }
}

/// `x` to `places` decimals (none: a whole number), then `unit` as given,
/// in integers: `{:.1}` would link core's float formatting, several KB.
pub(crate) fn fixed(buf: &mut FmtBuf, x: f32, places: u32, unit: &str) {
    use core::fmt::Write;
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
        Law::BRate | Law::BurstRate => {
            // Each unit is picked from the value as it would print.
            let hz = law.range().map_or(0.0, |r| r.at(v));
            if round(hz * 100.0) < 1000 {
                fixed(buf, hz, 2, " Hz");
            } else if round(hz * 10.0) < 1000 {
                fixed(buf, hz, 1, " Hz");
            } else {
                let _ = write!(buf, "{} Hz", round(hz));
            }
        }
        // Whole Hz under 1 kHz, then kHz to 3 figures.
        Law::Cutoff => {
            let hz = round(CUTOFF_HZ.at(v));
            if hz < 1000 {
                let _ = write!(buf, "{hz} Hz");
            } else if hz < 9995 {
                fixed(buf, hz as f32 / 1000.0, 2, " kHz");
            } else {
                fixed(buf, hz as f32 / 1000.0, 1, " kHz");
            }
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
fn discrete(val: f32, max: u8) -> u8 {
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
