//! The USB console's command table and line parser: bytes in, requests out.

use core::fmt;

pub const MAX_LINE: usize = 64;
pub const PROTOCOL: u8 = 1;
pub const STALL_MS: u32 = 250;
pub const WORD_MAX: usize = 16;

/// The words after the command, split on one or more spaces.
#[derive(Clone, Copy, Debug)]
pub struct Words<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let start = self.rest.iter().position(|&b| b != b' ')?;
        let rest = &self.rest[start..];
        let end = rest.iter().position(|&b| b == b' ').unwrap_or(rest.len());
        let (word, tail) = rest.split_at(end);
        self.rest = tail;
        Some(word)
    }
}

/// A command's argument shape.
pub trait Arg: Sized + Copy {
    /// The tail of `ERR <name> takes <USAGE>`.
    const USAGE: &'static str;
    fn parse(words: Words<'_>) -> Option<Self>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoArg;

impl Arg for NoArg {
    const USAGE: &'static str = "no arguments";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        words.next().is_none().then_some(NoArg)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Colours {
    Theme,
    Raw,
}

impl Arg for Colours {
    const USAGE: &'static str = "raw or nothing";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let got = match words.next() {
            None => Colours::Theme,
            Some(w) if w.eq_ignore_ascii_case(b"raw") => Colours::Raw,
            Some(_) => return None,
        };
        words.next().is_none().then_some(got)
    }
}

/// One entry per command: the variant, its argument type, its name and its help line.
macro_rules! commands {
    ($($v:ident ($arg:ty) => $name:literal, $about:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Command { $($v),+ }

        impl Command {
            pub const ALL: [Command; [$(Command::$v),+].len()] = [$(Command::$v),+];

            pub const fn name(self) -> &'static str {
                match self { $(Command::$v => $name),+ }
            }

            pub const fn about(self) -> &'static str {
                match self { $(Command::$v => $about),+ }
            }

            pub const fn usage(self) -> &'static str {
                match self { $(Command::$v => <$arg as Arg>::USAGE),+ }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Request { $($v($arg)),+ }

        impl Request {
            pub const fn command(self) -> Command {
                match self { $(Request::$v(_) => Command::$v),+ }
            }
        }

        fn parse(word: &[u8], rest: Words<'_>) -> Result<Request, Refusal> {
            $(
                if word.eq_ignore_ascii_case($name.as_bytes()) {
                    return <$arg as Arg>::parse(rest)
                        .map(Request::$v)
                        .ok_or(Refusal::Arguments(Command::$v));
                }
            )+
            Err(Refusal::Unknown(Word::new(word)))
        }
    };
}

commands! {
    Help   (NoArg)   => "help",   "this list",
    Status (NoArg)   => "status", "firmware, project, Part and where the UI is",
    Stats  (NoArg)   => "stats",  "AUDIO LOAD and the UI loop's time",
    Bench  (NoArg)   => "bench",  "the bench's numbers (bench builds)",
    Shot   (Colours) => "shot",   "the screen in THEME's colours; shot raw: canonical",
}

/// Printable ASCII only (anything else is stored as `?`), at most `WORD_MAX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    bytes: [u8; WORD_MAX],
    len: u8,
}

impl Word {
    pub fn new(raw: &[u8]) -> Word {
        let mut bytes = [0; WORD_MAX];
        let mut len = 0;
        for (slot, &b) in bytes.iter_mut().zip(raw) {
            *slot = if (0x20..=0x7e).contains(&b) { b } else { b'?' };
            len += 1;
        }
        Word { bytes, len }
    }

    pub fn as_str(&self) -> &str {
        let used = self.bytes.get(..usize::from(self.len)).unwrap_or(&[]);
        core::str::from_utf8(used).unwrap_or("?")
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Unknown(Word),
    Arguments(Command),
    TooLong,
}

pub struct Console {
    line: [u8; MAX_LINE],
    len: u8,
    overflowed: bool,
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Console {
    pub const fn new() -> Self {
        Console {
            line: [0; MAX_LINE],
            len: 0,
            overflowed: false,
        }
    }

    /// One received byte. `Some` when it ends a non-empty line.
    pub fn push(&mut self, byte: u8) -> Option<Result<Request, Refusal>> {
        if byte != b'\n' && byte != b'\r' {
            match self.line.get_mut(usize::from(self.len)) {
                Some(slot) => {
                    *slot = byte;
                    self.len += 1;
                }
                None => self.overflowed = true,
            }
            return None;
        }
        let used = self.line.get(..usize::from(self.len)).unwrap_or(&[]);
        let out = if self.overflowed {
            Some(Err(Refusal::TooLong))
        } else {
            let mut words = Words { rest: used };
            words.next().map(|word| parse(word, words))
        };
        self.len = 0;
        self.overflowed = false;
        out
    }
}
