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
pub(super) fn quoted(w: &mut dyn core::fmt::Write, text: &str) -> core::fmt::Result {
    w.write_str(QUOTE.get())?;
    w.write_str(text)?;
    w.write_str(UNQUOTE.get())
}

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
