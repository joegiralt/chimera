//! SETTINGS › TUNING's TUNING, the active Part's (ADR 0240): each Part's choice made its table.
//! Pulled, not pushed: after the keys, any Part whose TUNING differs from
//! what its table plays (`UiState::tuned`) is applied, so an edit, a
//! project load and a Sound load all land the same way. 12-TET clears the
//! table, a named temperament is parsed from its text, MTS follows the
//! MTS table, and `TUNEn` reads `CHIMERA/TUNINGS/TUNEn.SCL` (and
//! `TUNEn.KBM`, if there is one). A file that won't read or parse plays
//! 12-TET and says why. Bank 2's (`bank2.x`); bank 1 keeps `due` and the
//! door call.

// Without `tuning` no card file is read: the reader is dead code.
#![cfg_attr(not(feature = "tuning"), allow(dead_code))]
use core::fmt::Write;
use core::sync::atomic::{AtomicU8, Ordering};

use chimera_hal::store::{Dir, FileName, Store, StoreError};
use chimera_theory::tuning::{BUILT_IN, Keymap, Scale, offsets_into};

use crate::part::{TUNING_MTS, TUNING_NAMES};
use crate::project::{Line, PartId};
use crate::storage::{Card, Ready};
use crate::tuning;
use crate::ui::UiState;
use crate::ui::busy::Toast;
use crate::ui::file_buf::FileBuf as Buf;

pub use crate::ui::file_buf::FILE_MAX;

/// The focus band's words on a TUNEn choice and on MTS, and a toast's:
/// named tables (bank 2's), never literals left in bank 1's rodata.
static WORDS: [u8; 110] = *b"FROM CARD: TUNINGS/TUNEn.SCL|FOLLOWS MTS SYSEX FROM MIDI IN|.SCL|.KBM| NOT FOUND| NOT SCALA| TOO BIG| NOT READ";

#[derive(Clone, Copy)]
#[repr(u8)]
enum W {
    Card,
    Mts,
    Scl,
    Kbm,
    NotFound,
    NotScala,
    TooBig,
    NotRead,
}

#[inline(never)]
fn word(w: W) -> &'static str {
    crate::ui::midi::pick(&WORDS, w as u8)
}

static TWELVE: [f32; 128] = [0.0; 128];

/// `tuning::files_put` as the last pass saw it.
static SEEN_PUT: AtomicU8 = AtomicU8::new(0);

/// `TUNE1` to `TUNE8`, `.SCL` and `.KBM`: names made at compile time.
static FILES: [[FileName; 2]; 8] = {
    const fn name(n: usize, ext: &[u8]) -> FileName {
        let stem = [b'T', b'U', b'N', b'E', b'1' + n as u8];
        match FileName::new(Dir::Tunings, &stem, ext) {
            Some(f) => f,
            None => panic!("an 8.3 name"),
        }
    }
    let mut f = [[name(0, b"SCL"), name(0, b"KBM")]; 8];
    let mut i = 1;
    while i < 8 {
        f[i] = [name(i, b"SCL"), name(i, b"KBM")];
        i += 1;
    }
    f
};

/// Every Part whose TUNING differs from what its table plays, applied:
/// the door's entry, every card pass.
pub(crate) fn work_in(ui: &mut UiState, card: &mut Card, s: &mut dyn Store) {
    // A file `put` since the last pass: the Parts on the card's files read
    // theirs again (the card is inside the case: `put` is the way in).
    let put = tuning::files_put();
    if SEEN_PUT.swap(put, Ordering::Relaxed) != put {
        for t in ui.tuned.iter_mut().filter(|t| **t > TUNING_MTS) {
            *t = u8::MAX;
        }
    }
    for p in PartId::ALL {
        let i = p.index();
        let want = ui.project.part(p).mix.tuning;
        if want == ui.tuned[i] {
            continue;
        }
        ui.tuned[i] = want;
        // One table for the Part, written in place: no copies on the stack.
        let mut table = [0.0; 128];
        match want {
            0 => tuning::set(i, &TWELVE),
            TUNING_MTS => tuning::follow_mts(i, true),
            w if w < TUNING_MTS => {
                if let Ok(sc) = Scale::parse(BUILT_IN[usize::from(w) - 1]) {
                    offsets_into(&sc, None, &mut table);
                }
                tuning::set(i, &table);
            }
            w => {
                let n = usize::from(w - TUNING_MTS - 1) % FILES.len();
                match from_card(card, s, n, &mut table) {
                    Ok(()) => tuning::set(i, &table),
                    Err(why) => {
                        tuning::set(i, &TWELVE);
                        let mut text = Line::new(TUNING_NAMES[usize::from(w) % TUNING_NAMES.len()]);
                        let _ = text.write_str(word(why.0));
                        let _ = text.write_str(word(why.1));
                        ui.toast.show(Toast {
                            text,
                            ms: Toast::ERROR_MS,
                        });
                    }
                }
            }
        }
    }
}

/// What the focus band says on TUNING at `v`, if anything.
pub(crate) fn note_in(v: u8) -> Option<&'static str> {
    match v {
        TUNING_MTS => Some(word(W::Mts)),
        v if v > TUNING_MTS => Some(word(W::Card)),
        _ => None,
    }
}

/// Which file, and what went wrong with it: `TUNE3` then `.SCL NOT FOUND`.
type Why = (W, W);

/// `TUNEn`'s scale (`n` from 0), and its map if the card has one, as a
/// table. A card that won't read is the scale's `NOT FOUND`. Out of line,
/// as `read_both`: no frame of 8 KB (`stack-check`).
#[inline(never)]
fn from_card(
    card: &mut Card,
    s: &mut dyn Store,
    n: usize,
    out: &mut [f32; 128],
) -> Result<(), Why> {
    #[cfg(feature = "tuning")]
    {
        let p = BUF.0.get().cast::<Buf>();
        // SAFETY: `BUF` is touched here alone, on the UI thread's card work,
        // which never re-enters itself: this is its only reference. D2 is
        // not zeroed at boot: the memset makes it a valid `Buf` (all zero
        // is one) before the reference is made.
        let buf = unsafe {
            core::ptr::write_bytes(p, 0, 1);
            &mut *p
        };
        let ran = card.run(s, |s, r| Ok::<_, StoreError>(read_both(s, r, n, buf, out)));
        match ran {
            Ok(o) => o.result.unwrap_or(Err((W::Scl, W::NotRead))),
            Err(_) => Err((W::Scl, W::NotRead)),
        }
    }
    #[cfg(not(feature = "tuning"))]
    {
        let _ = (card, s, n, out);
        Err((W::Scl, W::NotRead))
    }
}

/// The card read's buffer, in D2 (NOLOAD, where AXI has no room), not on
/// the stack: a door entry's depth is charged to every call through a
/// pointer (`stack_depth.py`).
struct ReadBuf(core::cell::UnsafeCell<core::mem::MaybeUninit<Buf>>);

// SAFETY: only `from_card` reaches it, on the UI thread (see there).
unsafe impl Sync for ReadBuf {}

#[cfg(feature = "tuning")]
#[cfg_attr(target_os = "none", unsafe(link_section = ".ram_d2"))]
static BUF: ReadBuf = ReadBuf(core::cell::UnsafeCell::new(core::mem::MaybeUninit::uninit()));

#[inline(never)]
fn read_both(
    s: &mut dyn Store,
    r: &Ready,
    n: usize,
    buf: &mut Buf,
    out: &mut [f32; 128],
) -> Result<(), Why> {
    let [scl, kbm] = FILES[n % FILES.len()];
    if !read(s, r, scl, buf).map_err(|w| (W::Scl, w))? {
        return Err((W::Scl, W::NotFound));
    }
    let scale = Scale::parse(&buf.b[..buf.n]).map_err(|_| (W::Scl, W::NotScala))?;
    let map = if read(s, r, kbm, buf).map_err(|w| (W::Kbm, w))? {
        Some(Keymap::parse(&buf.b[..buf.n]).map_err(|_| (W::Kbm, W::NotScala))?)
    } else {
        None
    };
    offsets_into(&scale, map.as_ref(), out);
    Ok(())
}

/// `f` into `buf`: `false` if the card hasn't it. Inlined: the machine
/// outliner would share its calls from bank 1 (ADR 0175).
#[inline(always)]
fn read(s: &mut dyn Store, r: &Ready, f: FileName, buf: &mut Buf) -> Result<bool, W> {
    match s.read(r.volume(), f, buf) {
        Ok(()) if buf.over => Err(W::TooBig),
        Ok(()) => Ok(true),
        Err(StoreError::NotFound) => Ok(false),
        Err(_) => Err(W::NotRead),
    }
}
