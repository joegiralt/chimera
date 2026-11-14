//! `bank2` and `bank2 install` (ADR 0137): bank 2's state, and the staged
//! `/CHIMERA/SYSTEM/BANK2.BK2` written into it. The board runs the install
//! before the answer starts; what it did is a `Bank2Answer`. The boot's
//! own install also takes a `.BK2` dropped in the card root (`find_staged`).

use core::fmt::{self, Write};
use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError, VolumeId};
#[cfg(feature = "bank2-gate")]
use chimera_image::{Bank2Sector, Bank2Word};
use chimera_image::{
    FlashAt, FlashFail, Header, ImageRead, InstallFail, ReadFail, Seal, Slot, Verdict, judge,
    pick_staged, wants_staged,
};

use super::{Arg, Out, Stalled, Words};
use crate::bank2::{Bank2Status, Version, slot_word};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank2Arg {
    Status,
    Install,
    /// S0 erased: bank 2 empty again, for the next power pull.
    #[cfg(feature = "bank2-gate")]
    Erase,
    /// `slot-poke`: one foreign word at `POKE_AT`, S5's middle (gate 8).
    #[cfg(feature = "bank2-gate")]
    SlotPoke,
    /// `erase slots`: S4–S7 erased (gate 8's clear).
    #[cfg(feature = "bank2-gate")]
    EraseSlots,
    /// `ecc-read <hex addr>`: one masked read of that word (gate 1b).
    #[cfg(feature = "bank2-gate")]
    EccRead(Bank2Word),
    /// `pull <point>`: the next install holds there (gate 6).
    #[cfg(feature = "bank2-gate")]
    Pull(PullAt),
}

impl Arg for Bank2Arg {
    const USAGE: &'static str = if cfg!(feature = "bank2-gate") {
        "install, erase, erase slots, slot-poke, ecc-read <hex addr>, pull <point> or nothing"
    } else {
        "install or nothing"
    };
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let got = match words.next() {
            None => Bank2Arg::Status,
            Some(w) if w.eq_ignore_ascii_case(b"install") => Bank2Arg::Install,
            #[cfg(feature = "bank2-gate")]
            Some(w) => gate_arg(w, &mut words)?,
            #[cfg(not(feature = "bank2-gate"))]
            Some(_) => return None,
        };
        words.next().is_none().then_some(got)
    }
}

/// Where the gate's `slot-poke` programs its foreign word: S5's middle.
#[cfg(feature = "bank2-gate")]
pub const POKE_AT: Bank2Word = match Bank2Word::new(0x081A_0000) {
    Some(w) => w,
    None => panic!("a bank-2 word"),
};

/// The foreign word `slot-poke` programs.
#[cfg(feature = "bank2-gate")]
pub const POKE_WORD: [u8; 32] = [0x5A; 32];

/// Where `bank2 pull` stops the next install for a hand on the power
/// (spec § 6.3, gate 6).
#[cfg(feature = "bank2-gate")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PullAt {
    /// S4 re-erased back to back for 3 s, after S5's marker.
    EraseS4,
    /// S6 the same, after S7's marker.
    EraseS6,
    /// 3 s after MIXER's tag word, before its code word.
    TagMixer,
    /// The same for EDIT.
    TagEdit,
}

#[cfg(feature = "bank2-gate")]
impl PullAt {
    pub const ALL: [PullAt; 4] = [
        PullAt::EraseS4,
        PullAt::EraseS6,
        PullAt::TagMixer,
        PullAt::TagEdit,
    ];

    pub const fn word(self) -> &'static str {
        match self {
            PullAt::EraseS4 => "erase-s4",
            PullAt::EraseS6 => "erase-s6",
            PullAt::TagMixer => "tag-mixer",
            PullAt::TagEdit => "tag-edit",
        }
    }

    pub const fn slot(self) -> Slot {
        match self {
            PullAt::EraseS4 | PullAt::TagMixer => Slot::Mixer,
            PullAt::EraseS6 | PullAt::TagEdit => Slot::Edit,
        }
    }

    /// The entry sector's erase, not the tag word.
    pub const fn erase(self) -> bool {
        matches!(self, PullAt::EraseS4 | PullAt::EraseS6)
    }
}

/// A gate command's words after `bank2`.
#[cfg(feature = "bank2-gate")]
fn gate_arg(w: &[u8], words: &mut Words<'_>) -> Option<Bank2Arg> {
    let is = |s: &[u8]| w.eq_ignore_ascii_case(s);
    if is(b"erase") {
        return match words.next() {
            None => Some(Bank2Arg::Erase),
            Some(s) if s.eq_ignore_ascii_case(b"slots") => Some(Bank2Arg::EraseSlots),
            Some(_) => None,
        };
    }
    if is(b"slot-poke") {
        return Some(Bank2Arg::SlotPoke);
    }
    if is(b"ecc-read") {
        return Bank2Word::new(hex(words.next()?)?).map(Bank2Arg::EccRead);
    }
    if is(b"pull") {
        let p = words.next()?;
        return PullAt::ALL
            .into_iter()
            .find(|a| p.eq_ignore_ascii_case(a.word().as_bytes()))
            .map(Bank2Arg::Pull);
    }
    None
}

/// 1–8 hex digits, `0x` optional.
#[cfg(feature = "bank2-gate")]
fn hex(w: &[u8]) -> Option<u32> {
    let w = match w {
        [b'0', b'x' | b'X', rest @ ..] => rest,
        _ => w,
    };
    if w.is_empty() || w.len() > 8 {
        return None;
    }
    w.iter()
        .try_fold(0u32, |n, &c| Some(n << 4 | char::from(c).to_digit(16)?))
}

/// The staged image: `/CHIMERA/SYSTEM/BANK2.BK2`. Not a `.bin`, and not in
/// the root: the stock bootloader never takes it.
pub const STAGED: FileName = match FileName::new(Dir::System, b"BANK2", b"BK2") {
    Some(f) => f,
    None => panic!("an 8.3 name"),
};

/// Most root `*.BK2` files a boot judges: one a release dropped and never
/// deleted, past many; another build's costs one header read.
pub const ROOT_BK2_MAX: usize = 32;

/// A staged file as an image, a card read a call. A read past its end is
/// `Err`; the card's own error is kept for `error`.
pub struct CardImage<'a, S: Store + ?Sized> {
    store: &'a mut S,
    vol: VolumeId,
    file: FileName,
    error: Option<StoreError>,
}

impl<'a, S: Store + ?Sized> CardImage<'a, S> {
    /// `STAGED`.
    pub fn new(store: &'a mut S, vol: VolumeId) -> Self {
        Self::at(store, vol, STAGED)
    }

    pub fn at(store: &'a mut S, vol: VolumeId, file: FileName) -> Self {
        CardImage {
            store,
            vol,
            file,
            error: None,
        }
    }

    pub fn file(&self) -> FileName {
        self.file
    }

    /// The last card error a read hit.
    pub fn error(&self) -> Option<StoreError> {
        self.error
    }
}

/// The boot's staged file, unless `flash` already holds this build's
/// (`wants_staged`: then the card isn't touched): `STAGED`, then the first
/// `ROOT_BK2_MAX` root `*.BK2` (a dropped `CHIMERA.BK2`, or a long name a host
/// shortens, `CHIMER~1.BK2`), each judged against `seal`; the first
/// `Good` (`pick_staged`) and its header. The rest are ignored, unsaid.
pub fn find_staged<'a, S: Store + ?Sized>(
    store: &'a mut S,
    flash: &Verdict,
    seal: &Seal,
) -> Option<(CardImage<'a, S>, Header)> {
    if !wants_staged(flash) {
        return None;
    }
    let vol = store.mount().ok()?;
    let mut names = [STAGED; 1 + ROOT_BK2_MAX];
    let mut n = 1;
    // An unlisted root leaves `STAGED` alone to judge.
    let _ = store.list(vol, Dir::Root, &mut |f, _| {
        if f.ext() == b"BK2" && n < names.len() {
            names[n] = f;
            n += 1;
        }
    });
    let judged = names[..n]
        .iter()
        .map(|&f| judge(&mut CardImage::at(&mut *store, vol, f), seal));
    let (i, h) = pick_staged(judged)?;
    Some((CardImage::at(store, vol, names[i]), h))
}

/// Fills `buf` from the read's first byte; short is a miss.
struct Fill<'b> {
    buf: &'b mut [u8],
    at: usize,
}

impl ReadSink for Fill<'_> {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        if (len as usize) < self.buf.len() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let n = bytes.len().min(self.buf.len() - self.at);
        self.buf[self.at..self.at + n].copy_from_slice(&bytes[..n]);
        self.at += n;
        if self.at == self.buf.len() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

impl<S: Store + ?Sized> ImageRead for CardImage<'_, S> {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), ReadFail> {
        if buf.is_empty() {
            return Ok(());
        }
        let mut fill = Fill { buf, at: 0 };
        match self.store.read_at(self.vol, self.file, off, &mut fill) {
            Ok(()) if fill.at == fill.buf.len() => Ok(()),
            Ok(()) => Err(ReadFail),
            Err(e) => {
                self.error = Some(e);
                Err(ReadFail)
            }
        }
    }
}

/// The audio's counters before and after the write: overruns, and blocks
/// rendered (they moved: the audio ran).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bank2Audio {
    pub overruns: [u32; 2],
    pub blocks: [u32; 2],
}

/// Why nothing was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank2Refusal {
    /// This build has no seal: it never opens bank 2.
    Unsealed,
    /// Card work is pending in the UI.
    Busy,
    /// No staged file.
    Nothing,
    Card(StoreError),
    /// Sound, sealed for another build.
    Stale(Version),
    /// The staged file judged neither good nor stale.
    Staged(Bank2Status),
    /// The slot guard found other code in a slot.
    SlotForeign(Slot),
}

/// Why bank 2, written to, isn't open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank2Fail {
    /// The staged file changed between the check and the write.
    Staged(Bank2Status),
    /// A staged block didn't read mid-copy.
    StagedRead,
    Flash(FlashFail, FlashAt),
    /// What flash judged after the write.
    Readback(Bank2Status),
    /// Installed, but the door failed its check.
    Door,
}

/// `bank2 install`'s outcome, reached before the answer starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank2Answer {
    Open {
        version: Version,
        audio: Bank2Audio,
    },
    /// Bank 2 open with the staged seal: nothing written.
    Already,
    Refused(Bank2Refusal),
    Failed {
        why: Bank2Fail,
        audio: Bank2Audio,
    },
    /// `bank2 erase`: S0's erase.
    #[cfg(feature = "bank2-gate")]
    Erased(Result<(), FlashFail>),
    /// `bank2 slot-poke`: the foreign word's program.
    #[cfg(feature = "bank2-gate")]
    SlotPoked(Result<(), FlashFail>),
    /// `bank2 erase slots`: S4–S7's erases, the first failure's sector.
    #[cfg(feature = "bank2-gate")]
    SlotsErased(Result<(), (FlashFail, Bank2Sector)>),
    /// `bank2 ecc-read`: SR2.DBECCERR after the masked read, and the word's
    /// first 8 bytes.
    #[cfg(feature = "bank2-gate")]
    Ecc {
        ecc: bool,
        head: [u8; 8],
    },
    /// `bank2 pull`: the next install holds there.
    #[cfg(feature = "bank2-gate")]
    Armed(PullAt),
}

/// The staged file's verdict, and the card error behind an unreadable one,
/// before anything is written: its header if it is to be installed. `open`:
/// bank 2 is open, so with this build's seal, the staged one's.
pub fn check_staged(
    open: bool,
    v: &Verdict,
    card: Option<StoreError>,
) -> Result<Header, Bank2Answer> {
    let refuse = |r| Err(Bank2Answer::Refused(r));
    match (card, v) {
        (Some(StoreError::NotFound), _) => refuse(Bank2Refusal::Nothing),
        (Some(e), _) => refuse(Bank2Refusal::Card(e)),
        (None, Verdict::Good(_)) if open => Err(Bank2Answer::Already),
        (None, Verdict::Good(h)) => Ok(*h),
        (None, Verdict::Stale { have }) => refuse(Bank2Refusal::Stale(*have)),
        (None, v) => refuse(Bank2Refusal::Staged(Bank2Status::from_verdict(v))),
    }
}

/// The install's result and bank 2 as the door then judged it (`now`).
pub fn installed(
    r: Result<Header, InstallFail>,
    now: &Bank2Status,
    audio: Bank2Audio,
) -> Bank2Answer {
    // The door judges after the install's last judge: open is open.
    if let (Ok(_) | Err(InstallFail::Readback(_)), Bank2Status::Open { version, .. }) = (r, now) {
        return Bank2Answer::Open {
            version: *version,
            audio,
        };
    }
    let why = match r {
        Ok(_) => Bank2Fail::Door,
        Err(InstallFail::Staged(v)) => Bank2Fail::Staged(Bank2Status::from_verdict(&v)),
        Err(InstallFail::Io) => Bank2Fail::StagedRead,
        Err(InstallFail::Flash(f, at)) => Bank2Fail::Flash(f, at),
        Err(InstallFail::Readback(v)) => Bank2Fail::Readback(Bank2Status::from_verdict(&v)),
    };
    Bank2Answer::Failed { why, audio }
}

fn write_audio(a: &Bank2Audio, t: &mut impl Write) -> fmt::Result {
    let [o0, o1] = a.overruns;
    let blocks = a.blocks[1].wrapping_sub(a.blocks[0]);
    writeln!(t, "audio overruns {o0} -> {o1} blocks +{blocks}")
}

fn write_flash(f: FlashFail, at: FlashAt, t: &mut impl Write) -> fmt::Result {
    crate::bank2::write_flash_fail(f, t)?;
    match at {
        FlashAt::Sector(s) => write!(t, " at S{}", s.index()),
        FlashAt::Word(w) => write!(t, " at {:08x}", w.get()),
    }
}

/// `bank2 install`'s whole answer: an `audio` line if flash was written,
/// then `OK bank2 open <version>`, `OK already installed`,
/// `ERR bank2 slot foreign <slot>` or `ERR bank2: <why>`; `bank2 erase`'s
/// `OK bank2 erased` or `ERR bank2: flash <fail>`.
pub fn write_bank2<O: Out>(a: &Bank2Answer, out: &mut O) -> Result<(), Stalled> {
    lines(a, &mut super::Text(out)).map_err(|fmt::Error| Stalled)
}

fn lines(a: &Bank2Answer, t: &mut impl Write) -> fmt::Result {
    match a {
        Bank2Answer::Open { version, audio } => {
            write_audio(audio, t).and_then(|()| writeln!(t, "OK bank2 open {}", version.as_str()))
        }
        Bank2Answer::Already => t.write_str("OK already installed\n"),
        Bank2Answer::Refused(r) => {
            t.write_str("ERR bank2")?;
            match r {
                Bank2Refusal::Unsealed => t.write_str(": firmware unsealed"),
                Bank2Refusal::Busy => t.write_str(": busy"),
                Bank2Refusal::Nothing => t.write_str(": nothing staged"),
                Bank2Refusal::Card(e) => write!(t, ": {}", e.message()),
                Bank2Refusal::Stale(v) => write!(t, ": staged: needs firmware {}", v.as_str()),
                Bank2Refusal::Staged(s) => t.write_str(": staged: ").and_then(|()| s.state(t)),
                // `status`'s own words, no colon: the host tool matches them.
                Bank2Refusal::SlotForeign(s) => write!(t, " slot foreign {}", slot_word(*s)),
            }
            .and_then(|()| t.write_char('\n'))
        }
        Bank2Answer::Failed { why, audio } => {
            write_audio(audio, t)?;
            t.write_str("ERR bank2: ")?;
            match *why {
                Bank2Fail::Staged(s) => t.write_str("staged: ").and_then(|()| s.state(t)),
                Bank2Fail::StagedRead => t.write_str("staged file unreadable"),
                Bank2Fail::Flash(f, at) => {
                    t.write_str("flash ").and_then(|()| write_flash(f, at, t))
                }
                Bank2Fail::Readback(s) => t.write_str("readback ").and_then(|()| s.state(t)),
                Bank2Fail::Door => t.write_str("door check failed"),
            }
            .and_then(|()| t.write_char('\n'))
        }
        #[cfg(feature = "bank2-gate")]
        Bank2Answer::Erased(Ok(())) => t.write_str("OK bank2 erased\n"),
        #[cfg(feature = "bank2-gate")]
        Bank2Answer::Erased(Err(f)) | Bank2Answer::SlotPoked(Err(f)) => {
            t.write_str("ERR bank2: flash ")?;
            crate::bank2::write_flash_fail(*f, t)?;
            t.write_char('\n')
        }
        #[cfg(feature = "bank2-gate")]
        Bank2Answer::SlotPoked(Ok(())) => t.write_str("OK slot poked\n"),
        #[cfg(feature = "bank2-gate")]
        Bank2Answer::SlotsErased(Ok(())) => t.write_str("OK slots erased\n"),
        #[cfg(feature = "bank2-gate")]
        Bank2Answer::SlotsErased(Err((f, s))) => {
            t.write_str("ERR bank2: flash ")?;
            write_flash(*f, FlashAt::Sector(*s), t)?;
            t.write_char('\n')
        }
        #[cfg(feature = "bank2-gate")]
        Bank2Answer::Ecc { ecc, head } => {
            write!(t, "ecc={} ", u8::from(*ecc))?;
            head.iter().try_for_each(|b| write!(t, "{b:02x}"))?;
            t.write_char('\n')
        }
        #[cfg(feature = "bank2-gate")]
        Bank2Answer::Armed(p) => writeln!(t, "OK pull {} armed", p.word()),
    }
}
