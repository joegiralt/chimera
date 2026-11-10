//! SETTINGS › TUNING's TUNING and MAPPING, the active Part's (ADR 0240,
//! 0259): each Part's choice made its table. Pulled, not pushed: after the
//! keys, any Part whose TUNING, MAPPING or Scala file differs from what its
//! table plays (`Tuned::keys`) is applied, so an edit, a project load and a
//! Sound load all land the same way. 12-TET clears the table, a named
//! temperament is parsed from its text, MTS follows the MTS table, and
//! SCALA reads the Part's file, `CHIMERA/TUNINGS/<name>.SCL` (and its
//! `.KBM`, if there is one, else MAPPING maps the keys). A file that won't
//! read or parse plays 12-TET and says why.
//!
//! A TUNING turn past MTS walks the card's files by name: bank 1 keeps the
//! detents (`Tuned::turn`), the card work here calls `tuning_walk`, which
//! lists the folder and moves. Bank 2's (`bank2.x`); bank 1 keeps the
//! turn, the walk, the cell's name and the door calls.

// Without `tuning` no card file is read: the reader is dead code, its
// imports unused.
#![cfg_attr(not(feature = "tuning"), allow(dead_code, unused_imports))]
use core::fmt::Write;
use core::sync::atomic::{AtomicU8, Ordering};

use chimera_hal::store::{FileName, Store, StoreError};
use chimera_theory::tuning::{BUILT_IN, Keymap, Scale, description, offsets_into};

use crate::block::ParamId;
use crate::hw::MAX_PARTS;
use crate::part::{PartParams, TUNING_MTS, TUNING_SCALA};
use crate::preset::Part;
use crate::project::{Line, PartId};
use crate::storage::{Card, Ready};
use crate::tuning::{self, ScalaName};
use crate::ui::UiState;
use crate::ui::busy::Toast;
use crate::ui::file_buf::FileBuf as Buf;

pub use crate::ui::file_buf::FILE_MAX;

pub use crate::ui::tuning_walk::DESC_LEN;
use crate::ui::tuning_walk::SHOWN;

/// What each Part's table plays, and the turn and description the card
/// work owes the page. The UI's (D2): kept small, and without `tuning`
/// (no tables, no files: the bench builds' full D2) only the turn.
pub struct Tuned {
    /// Each Part's TUNING, MAPPING and file as applied (`key_of`);
    /// `u32::MAX` is read again.
    #[cfg(feature = "tuning")]
    pub(crate) keys: [u32; MAX_PARTS],
    /// A TUNING turn the card work makes: the Part and its detents.
    pub(crate) turn: Option<(PartId, i8)>,
    /// The Parts whose file has its `.KBM`, a bit a Part.
    #[cfg(feature = "tuning")]
    kbm: u8,
    /// The last file read (`ScalaName::NONE`: none); what it shows is
    /// `tuning_walk::SHOWN`'s, in AXI.
    #[cfg(feature = "tuning")]
    desc_of: ScalaName,
}

impl Tuned {
    /// Every Part read at the first pass.
    pub const NEW: Tuned = Tuned {
        #[cfg(feature = "tuning")]
        keys: [u32::MAX; MAX_PARTS],
        turn: None,
        #[cfg(feature = "tuning")]
        kbm: 0,
        #[cfg(feature = "tuning")]
        desc_of: ScalaName::NONE,
    };
}

/// The focus band's words, and a toast's: named tables (bank 2's), never
/// literals left in bank 1's rodata.
static WORDS: [u8; 178] = *b"FOLLOWS MTS SYSEX FROM MIDI IN|.SCL|.KBM| NOT FOUND| NOT SCALA| TOO BIG| NOT READ|NO FILE PICKED|FOR SCALA FILES|.KBM MAPS THE KEYS|EACH KEY THE NEXT STEP|EACH C STARTS THE SCALE";

#[derive(Clone, Copy)]
#[repr(u8)]
enum W {
    Mts,
    Scl,
    Kbm,
    NotFound,
    NotScala,
    TooBig,
    NotRead,
    NoFile,
    ForFiles,
    KbmMaps,
    Continuous,
    Keyboard,
}

#[inline(never)]
fn word(w: W) -> &'static str {
    crate::ui::midi::pick(&WORDS, w as u8)
}

/// `tuning::files_put` as the last pass saw it.
static SEEN_PUT: AtomicU8 = AtomicU8::new(0);

/// What a Part's table plays, as `Tuned::keys` holds it: a built-in
/// choice is its index, a file a hash of it and MAPPING.
fn key_of(p: &Part) -> u32 {
    let m = &p.mix;
    if m.tuning != TUNING_SCALA {
        return u32::from(m.tuning);
    }
    let name = p.scala().map_or([0; 8], |n| n.padded());
    // FNV-1a.
    [m.tuning, m.mapping]
        .iter()
        .chain(&name)
        .fold(0x811c_9dc5u32, |h, &b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        })
}

/// Every Part whose TUNING differs from what its table plays, applied, and
/// a waiting turn made: the door's entry, every card pass.
pub(crate) fn work_in(ui: &mut UiState, card: &mut Card, s: &mut dyn Store) {
    if let Some((p, d)) = ui.tuned.turn.take() {
        super::tuning_walk::turn(ui, card, s, p, d);
    }
    #[cfg(feature = "tuning")]
    apply(ui, card, s);
}

/// Every Part's table brought to its TUNING, MAPPING and file.
#[cfg(feature = "tuning")]
fn apply(ui: &mut UiState, card: &mut Card, s: &mut dyn Store) {
    // A file `put` since the last pass: the Parts on the card's files read
    // theirs again (the card is inside the case: `put` is the way in).
    let put = tuning::files_put();
    if SEEN_PUT.swap(put, Ordering::Relaxed) != put {
        for (k, p) in ui.tuned.keys.iter_mut().zip(ui.project.perf().parts()) {
            if p.mix.tuning == TUNING_SCALA {
                *k = u32::MAX;
            }
        }
        ui.tuned.desc_of = ScalaName::NONE;
    }
    // The active Part's file described: read again if the last read was
    // another's.
    let a = ui.project.part(ui.active_part);
    if a.mix.tuning == TUNING_SCALA && a.scala().is_some_and(|n| n != ui.tuned.desc_of) {
        ui.tuned.keys[ui.active_part.index()] = u32::MAX;
    }
    for p in PartId::ALL {
        let i = p.index();
        let part = ui.project.part(p);
        let (want, key, name, mapping) = (
            part.mix.tuning,
            key_of(part),
            part.scala(),
            part.mix.mapping,
        );
        if key == ui.tuned.keys[i] {
            continue;
        }
        ui.tuned.keys[i] = key;
        ui.tuned.kbm &= !(1 << i);
        // One table for the Part, written in place: no copies on the stack.
        let mut table = [0.0; 128];
        match (want, name) {
            (TUNING_MTS, _) => tuning::follow_mts(i, true),
            (w, _) if w > 0 && w < TUNING_MTS => {
                if let Ok(sc) = Scale::parse(BUILT_IN[usize::from(w) - 1]) {
                    offsets_into(&sc, None, &mut table);
                }
                tuning::set(i, &table);
            }
            (TUNING_SCALA, Some(n)) => {
                match from_card(&mut ui.tuned, card, s, n, mapping, &mut table) {
                    Ok((silent, kbm)) => {
                        tuning::set(i, &table);
                        tuning::silence(i, silent);
                        ui.tuned.kbm |= u8::from(kbm) << i;
                    }
                    Err(why) => {
                        // 12-TET: whatever the read left, zeroed.
                        table.fill(0.0);
                        tuning::set(i, &table);
                        let mut text = Line::new(n.as_str());
                        let _ = text.write_str(word(why.0));
                        let _ = text.write_str(word(why.1));
                        ui.toast.show(Toast {
                            text,
                            ms: Toast::ERROR_MS,
                        });
                    }
                }
            }
            // 12-TET, or SCALA naming no file: the table as built, all zero.
            _ => tuning::set(i, &table),
        }
    }
}

/// What the focus band says on SETTINGS › TUNING's TUNING or MAPPING for
/// Part `p`, if anything.
pub(crate) fn note_in(t: &Tuned, id: ParamId, p: PartId, part: &Part) -> Option<Line> {
    #[cfg(not(feature = "tuning"))]
    let _ = (t, p);
    let m = &part.mix;
    let name = part.scala().filter(|_| m.tuning == TUNING_SCALA);
    let w = match id {
        PartParams::TUNING if m.tuning == TUNING_MTS => W::Mts,
        PartParams::TUNING if m.tuning != TUNING_SCALA => return None,
        PartParams::TUNING => match name {
            None => W::NoFile,
            #[cfg(feature = "tuning")]
            Some(n)
                if t.desc_of == n
                    && let Some(l) = SHOWN.desc(n) =>
            {
                return Some(l);
            }
            Some(n) => return Some(named(n, W::Scl)),
        },
        PartParams::MAPPING => match name {
            None => W::ForFiles,
            #[cfg(feature = "tuning")]
            Some(n) if t.kbm >> p.index() & 1 != 0 => return Some(named(n, W::KbmMaps)),
            Some(_) if m.mapping == 0 => W::Continuous,
            Some(_) => W::Keyboard,
        },
        _ => return None,
    };
    Some(Line::new(word(w)))
}

/// `n` then `w`: `PELOG.SCL`.
fn named(n: ScalaName, w: W) -> Line {
    let mut l = Line::new(n.as_str());
    let _ = l.write_str(word(w));
    l
}

/// Which file, and what went wrong with it: `PELOG` then `.SCL NOT FOUND`.
type Why = (W, W);

/// Scala file `n`, and its map if the card has one, else `mapping`'s, as
/// a table: its silent keys and whether a `.KBM` mapped it. Its
/// description kept, even on a failure (read once). A card that won't
/// read is the scale's `NOT READ`. Out of line, as `read_both`: no frame
/// of 8 KB (`stack-check`).
#[cfg(feature = "tuning")]
#[inline(never)]
fn from_card(
    t: &mut Tuned,
    card: &mut Card,
    s: &mut dyn Store,
    n: ScalaName,
    mapping: u8,
    out: &mut [f32; 128],
) -> Result<(u128, bool), Why> {
    t.desc_of = n;
    SHOWN.set_file(n, n.as_bytes());
    let p = BUF.0.get().cast::<Buf>();
    // SAFETY: `BUF` is touched here alone, on the UI thread's card work,
    // which never re-enters itself: this is its only reference. D2 is not zeroed at
    // boot: the memset makes it a valid `Buf` (all zero is one) before
    // the reference is made.
    let buf = unsafe {
        core::ptr::write_bytes(p, 0, 1);
        &mut *p
    };
    let ran = card.run(s, |s, r| {
        Ok::<_, StoreError>(read_both(s, r, n, mapping, buf, out))
    });
    match ran {
        Ok(o) => o.result.unwrap_or(Err((W::Scl, W::NotRead))),
        Err(_) => Err((W::Scl, W::NotRead)),
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

#[cfg(feature = "tuning")]
#[inline(never)]
fn read_both(
    s: &mut dyn Store,
    r: &Ready,
    n: ScalaName,
    mapping: u8,
    buf: &mut Buf,
    out: &mut [f32; 128],
) -> Result<(u128, bool), Why> {
    // By the name it shows (ADR 0259): the `.SCL` and its `.KBM`.
    let mut name = [0; crate::scala_name::LONG_MAX];
    let (scl, kbm, len) = super::tuning_walk::find(s, r, n, &mut name);
    let Some(scl) = scl else {
        return Err((W::Scl, W::NotFound));
    };
    SHOWN.set_file(n, &name[..len]);
    if !read(s, r, scl, buf).map_err(|w| (W::Scl, w))? {
        return Err((W::Scl, W::NotFound));
    }
    let text = &buf.b[..buf.n];
    SHOWN.set_desc(description(text));
    let scale = Scale::parse(text).map_err(|_| (W::Scl, W::NotScala))?;
    let map = match kbm {
        Some(kbm) if read(s, r, kbm, buf).map_err(|w| (W::Kbm, w))? => {
            Some(Keymap::parse(&buf.b[..buf.n]).map_err(|_| (W::Kbm, W::NotScala))?)
        }
        _ => None,
    };
    let kbm = map.is_some();
    let map = map.or((mapping == 1).then(|| Keymap::keyboard(scale.len())));
    let unmapped = offsets_into(&scale, map.as_ref(), out);
    // Only KEYBOARD's spare keys are silent: a `.KBM`'s `x` plays 12-TET.
    Ok((if kbm { 0 } else { unmapped }, kbm))
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
