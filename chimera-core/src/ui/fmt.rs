/// Fixed-size buffer for no_std text formatting via core::fmt::Write.
pub struct FmtBuf {
    buf: [u8; 32],
    pos: usize,
}

impl Default for FmtBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl FmtBuf {
    pub fn new() -> Self {
        Self {
            buf: [0; 32],
            pos: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: `pos` only ever advances in `write_str`, which copies a
        // prefix of a `&str` (valid UTF-8) cut at a char boundary, so
        // `buf[..pos]` is always a concatenation of whole UTF-8 chars.
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.pos]) }
    }

    pub fn clear(&mut self) {
        self.pos = 0;
    }
}

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
        ValFmt::Route => {
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
    }
}

/// `x ≥ 0` to `places` decimals in integers: `{:.1}` would link core's
/// float formatting, several KB of flash.
fn fixed(buf: &mut FmtBuf, x: f32, places: u32, unit: &str) {
    use core::fmt::Write;
    let k = 10i32.pow(places);
    let n = libm::roundf(x * k as f32) as i32;
    let _ = write!(buf, "{}.{:02$} {unit}", n / k, n % k, places as usize);
}

fn fmt_law(buf: &mut FmtBuf, v: f32, law: crate::dsp::modulator::law::Law) {
    use crate::dsp::modulator::law::Law;
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
            let hz = law.range().map_or(0.0, |r| r.at(v));
            if hz < 10.0 {
                fixed(buf, hz, 2, "Hz");
            } else if hz < 100.0 {
                fixed(buf, hz, 1, "Hz");
            } else {
                let _ = write!(buf, "{} Hz", round(hz));
            }
        }
        _ => {
            let s = law.range().map_or(0.0, |r| r.at(v));
            if s >= 1.0 {
                fixed(buf, s, 1, "s");
            } else if s >= 0.01 {
                let _ = write!(buf, "{} ms", round(s * 1000.0));
            } else {
                fixed(buf, s * 1000.0, 1, "ms");
            }
        }
    }
}

/// Normalized `val` rounded to the nearest of 0..=max.
fn discrete(val: f32, max: u8) -> u8 {
    let v = (val * max as f32 + 0.5) as u8;
    if v > max { max } else { v }
}

impl core::fmt::Write for FmtBuf {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let bytes = s.as_bytes();
        let mut len = bytes.len().min(self.buf.len() - self.pos);
        // Truncate whole chars only: `as_str` relies on `buf[..pos]` being
        // valid UTF-8.
        while !s.is_char_boundary(len) {
            len -= 1;
        }
        self.buf[self.pos..self.pos + len].copy_from_slice(&bytes[..len]);
        self.pos += len;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::FmtBuf;
    use core::fmt::Write;

    /// A multi-byte char straddling the capacity is dropped whole, never
    /// split, so `as_str`'s unchecked conversion stays sound.
    #[test]
    fn truncation_never_splits_a_char() {
        let mut buf = FmtBuf::new();
        let _ = buf.write_str("0123456789012345678901234567890"); // 31 bytes
        let _ = buf.write_str("é"); // 2 bytes: would straddle byte 32
        assert!(core::str::from_utf8(&buf.buf[..buf.pos]).is_ok());
        assert_eq!(buf.as_str(), "0123456789012345678901234567890");
        let _ = buf.write_str("x"); // the one byte left still fits
        assert_eq!(buf.as_str().len(), 32);
    }
}
