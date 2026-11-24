// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The words protocol 4's answers write, kept in bank 2 with the code
//! that writes them (ADR 0289). A `str` literal, and a `write!`'s pieces,
//! are anonymous rodata, which no glob can place: bank 1 would hold them.
//! A `Lit` is a named static in `.bank2_ed.lit`.

/// A literal's bytes, copied whole from a `str` by `new`.
pub(super) struct Lit<const N: usize>([u8; N]);

impl<const N: usize> Lit<N> {
    /// `s` as a `Lit`: its length is the type's, checked at compile time.
    pub(super) const fn new(s: &str) -> Lit<N> {
        let b = s.as_bytes();
        assert!(b.len() == N);
        let mut out = [0; N];
        let mut i = 0;
        while i < N {
            out[i] = b[i];
            i += 1;
        }
        Lit(out)
    }

    /// The literal.
    #[inline(always)]
    pub(super) fn get(&'static self) -> &'static str {
        // SAFETY: the bytes are a `str`'s, copied whole by `new`.
        unsafe { core::str::from_utf8_unchecked(&self.0) }
    }

    /// Its bytes.
    #[inline(always)]
    pub(super) fn bytes(&'static self) -> &'static [u8] {
        &self.0
    }
}

/// `lit!(NAME = "text")`: a `Lit` static in bank 2.
macro_rules! lit {
    ($(#[$m:meta])* $vis:vis $name:ident = $s:literal) => {
        $(#[$m])*
        #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.lit"))]
        $vis static $name: $crate::console::ed::lit::Lit<{ $s.len() }> =
            $crate::console::ed::lit::Lit::new($s);
    };
}
pub(super) use lit;

lit!(
    /// Opens a quoted text after a word: ` "`.
    pub(super) QUOTE = " \""
);
lit!(
    /// Closes a quoted text and its line.
    pub(super) UNQUOTE = "\"\n"
);
lit!(
    /// Closes a quoted text before the line goes on: `" `.
    pub(super) QUOTE_SHUT = "\" "
);
lit!(
    /// A panel's line's head.
    pub(super) PANEL = "panel "
);
lit!(
    /// TAPE's SOURCE's line's head, its text quoted.
    pub(super) SOURCE = "source \""
);
lit!(
    /// The tail of a refusal naming what is not here.
    pub(super) HERE = " here\n"
);

/// ` "<text>"` and the line's end.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.lit.quoted"))]
pub(super) fn quoted(w: &mut dyn core::fmt::Write, t: &str) -> core::fmt::Result {
    w.write_str(QUOTE.get())?;
    text(w, t)?;
    w.write_str(UNQUOTE.get())
}

/// `s` inside a quoted field, every answer's (ADR 0336): `\` as `\\`, `"`
/// as `\"`, a control byte (below 0x20, and 0x7F) as `\xHH`, a C1 control
/// (U+0080–U+009F) as its two bytes `\xC2\xHH`, the rest verbatim, so a
/// field never closes early and its line never breaks. The box's names
/// hold none of them; this holds the line whatever a text carries.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.lit.text"))]
pub(super) fn text(w: &mut dyn core::fmt::Write, s: &str) -> core::fmt::Result {
    let mut rest = s;
    loop {
        let b = rest.as_bytes();
        let (mut i, mut n) = (0, 0);
        while i < b.len() {
            n = match b[i] {
                b'"' | b'\\' | 0..0x20 | 0x7f => 1,
                0xc2 if matches!(b.get(i + 1), Some(0x80..0xa0)) => 2,
                _ => 0,
            };
            if n > 0 {
                break;
            }
            i += 1;
        }
        if n == 0 {
            return w.write_str(rest);
        }
        // SAFETY: `i` is a char's start (an ASCII byte, or a C1 control's
        // lead) and `i + n` the next's, both within `b`: each cut is a
        // `str`. Cut as bytes: `str`'s range index is a bank-1 function.
        let (head, tail) = unsafe {
            (
                core::str::from_utf8_unchecked(b.get_unchecked(..i)),
                core::str::from_utf8_unchecked(b.get_unchecked(i + n..)),
            )
        };
        w.write_str(head)?;
        escape(w, b[i])?;
        if n == 2 {
            escape(w, b[i + 1])?;
        }
        rest = tail;
    }
}

/// One byte escaped: `\"`, `\\`, else `\xHH`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.lit.escape"))]
fn escape(w: &mut dyn core::fmt::Write, b: u8) -> core::fmt::Result {
    w.write_char('\\')?;
    if b == b'"' || b == b'\\' {
        return w.write_char(char::from(b));
    }
    w.write_char('x')?;
    w.write_char(char::from(HEX[usize::from(b >> 4)]))?;
    w.write_char(char::from(HEX[usize::from(b & 0xf)]))
}

/// `\xHH`'s digits.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.lit"))]
static HEX: [u8; 16] = *b"0123456789ABCDEF";

/// `n` in decimal, with no `write!`: a format's pieces are bank 1's.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.lit.uint"))]
pub(super) fn uint(w: &mut dyn core::fmt::Write, n: u32) -> core::fmt::Result {
    let mut digits = [0u8; 10];
    let (mut n, mut i) = (n, digits.len());
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for &d in &digits[i..] {
        w.write_char(char::from(d))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate std;
    use std::string::String;

    /// A quoted field never closes early nor breaks its line (ADR 0336):
    /// `\\` and `"` escaped by a backslash, a control byte as `\xHH`, the
    /// rest verbatim.
    #[test]
    fn a_quoted_field_keeps_its_line_whole() {
        let mut s = String::new();
        quoted(&mut s, "A\"B\\C\nD\rE\tF\x7fG\x01 H-1 é").unwrap();
        assert_eq!(s, " \"A\\\"B\\\\C\\x0AD\\x0DE\\x09F\\x7FG\\x01 H-1 é\"\n");
    }

    /// A C1 control (U+0080–U+009F) is a control too: its two UTF-8 bytes
    /// as `\xC2\xHH`. U+00A0 on is verbatim.
    #[test]
    fn a_c1_control_is_escaped_byte_by_byte() {
        let mut s = String::new();
        text(&mut s, "a\u{80}b\u{9f}\u{a0}\u{7f}\u{100}").unwrap();
        assert_eq!(s, "a\\xC2\\x80b\\xC2\\x9F\u{a0}\\x7F\u{100}");
    }
}
