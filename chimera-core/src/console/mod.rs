//! The USB console's command table and line parser: bytes in, requests out.

use core::fmt;

pub const MAX_LINE: usize = 64;
pub const PROTOCOL: u8 = 2;
/// An answer gives up after this long with no byte taken…
pub const STALL_MS: u32 = 250;
/// …or after this long in all, however the bytes dribble out.
pub const ANSWER_MS: u32 = 1000;
pub const WORD_MAX: usize = 16;
const _: () = assert!(MAX_LINE <= u8::MAX as usize);
const _: () = assert!(WORD_MAX <= u8::MAX as usize);

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

/// `ed`'s words as they came: bank 2 reads them (`EdDoor`, ADR 0251), so
/// bank 1 links none of their parse, and refuses none of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdLine {
    bytes: [u8; MAX_LINE],
    len: u8,
}

impl EdLine {
    pub fn words(&self) -> Words<'_> {
        Words {
            rest: self.bytes.get(..usize::from(self.len)).unwrap_or(&[]),
        }
    }
}

impl Arg for EdLine {
    const USAGE: &'static str = EdArg::USAGE;
    fn parse(words: Words<'_>) -> Option<Self> {
        let mut bytes = [0; MAX_LINE];
        bytes.iter_mut().zip(words.rest).for_each(|(d, &s)| *d = s);
        let len = words.rest.len().min(MAX_LINE) as u8;
        Some(EdLine { bytes, len })
    }
}

/// One entry per command: the variant, its argument type, its name and its help line.
macro_rules! commands {
    ($($(#[$m:meta])* $v:ident ($arg:ty) => $name:literal, $about:literal),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Command { $($(#[$m])* $v),+ }

        impl Command {
            pub const ALL: [Command; [$($(#[$m])* Command::$v),+].len()] =
                [$($(#[$m])* Command::$v),+];

            pub const fn name(self) -> &'static str {
                match self { $($(#[$m])* Command::$v => $name),+ }
            }

            pub const fn about(self) -> &'static str {
                match self { $($(#[$m])* Command::$v => $about),+ }
            }

            pub const fn usage(self) -> &'static str {
                match self { $($(#[$m])* Command::$v => <$arg as Arg>::USAGE),+ }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Request { $($(#[$m])* $v($arg)),+ }

        impl Request {
            pub const fn command(self) -> Command {
                match self { $($(#[$m])* Request::$v(_) => Command::$v),+ }
            }
        }

        fn parse(word: &[u8], rest: Words<'_>) -> Result<Request, Refusal> {
            $(
                $(#[$m])*
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

/// The table: one row per command. `ls`, `put`, `rm` and `get` only with
/// `card-put`.
macro_rules! table {
    ($($card:tt)*) => {
        commands! {
            Help   (NoArg)   => "help",   "this list",
            Status (NoArg)   => "status", "firmware, project, Part and where the UI is",
            Stats  (NoArg)   => "stats",  "AUDIO LOAD and the UI loop's time",
            Bench  (NoArg)   => "bench",  "the bench's numbers (bench builds)",
            Card   (CardArg) => "card",   "SD speed and streams, a chord playing; clean (bench builds)",
            Shot   (Colours) => "shot",   "the screen in THEME's colours; shot raw: canonical",
            Dfu    (NoArg)   => "dfu",    "restart into the ROM loader for just flash",
            Sweep  (NoArg)   => "sweep",  "the lost-cluster sweep at the next boot: restarts into it",
            Flash  (NoArg)   => "flash",  "flash controller and bank 2, read only",
            Bank2  (Bank2Arg) => "bank2", "bank 2: its state; bank2 install: the staged file into it",
            Key    (KeyArg)  => "key",    "B1-B6 MENU MINUS PLUS MIX EDIT SEQ PUSH1-6, A+B [hold_ms]",
            Turn   (TurnArg) => "turn",   "<encoder 1-6> <delta -64..64>: detents at a hand's pace",
            Ed     (EdLine)  => "ed",     "editor protocol",
            $($card)*
        }
    };
}

#[cfg(all(feature = "card-put", feature = "fault-inject"))]
table! {
    Ls     (LsArg)   => "ls",     "files and sizes in PROJECTS; ls tapes|sounds|maps: theirs",
    Put    (PutArg)  => "put",    "a project, Sound or source: <path> <size> <crc32> [--over]",
    Rm     (RmArg)   => "rm",    "SOUNDS/S#######, MAPS/<stem> or TUNINGS/<stem>.SCL|KBM",
    Get    (PairArg) => "get",   "base64 of a pair: /CHIMERA/SOUNDS/S####### or MAPS/<stem>",
    Fault  (FaultArg) => "fault",  "fault test: fault dma <1|2>, fault ndtr, fault card (fault-inject builds)",
}
#[cfg(all(feature = "card-put", not(feature = "fault-inject")))]
table! {
    Ls     (LsArg)   => "ls",     "files and sizes in PROJECTS; ls tapes|sounds|maps: theirs",
    Put    (PutArg)  => "put",    "a project, Sound or source: <path> <size> <crc32> [--over]",
    Rm     (RmArg)   => "rm",    "SOUNDS/S#######, MAPS/<stem> or TUNINGS/<stem>.SCL|KBM",
    Get    (PairArg) => "get",   "base64 of a pair: /CHIMERA/SOUNDS/S####### or MAPS/<stem>",
}
#[cfg(all(not(feature = "card-put"), feature = "fault-inject"))]
table! {
    Fault  (FaultArg) => "fault",  "fault test: fault dma <1|2>, fault card (fault-inject builds)",
}
#[cfg(all(not(feature = "card-put"), not(feature = "fault-inject")))]
table! {}

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

mod ed;
pub(crate) use ed::answer_in as ed_answer_in;
pub use ed::{EdAddr, EdArg, EdValue};
mod status;
pub use status::*;
mod out;
pub use out::*;
mod shot;
pub use shot::*;
mod answer;
pub use answer::*;
mod shell;
pub use shell::*;
mod card;
pub use card::*;
#[cfg(feature = "card-put")]
mod put;
#[cfg(feature = "card-put")]
pub use put::*;
#[cfg(feature = "card-put")]
mod sound_file;
#[cfg(feature = "card-put")]
pub use sound_file::*;
#[cfg(feature = "fault-inject")]
mod fault;
#[cfg(feature = "fault-inject")]
pub use fault::*;
mod flash;
pub use flash::*;
mod bank2;
pub use bank2::*;
mod hands;
pub use hands::*;
mod nocard;
pub use nocard::NoCard;
