//! TAPE's source files (ADR 0087, ADR 0093): `/CHIMERA/TAPES/<CRC>.SRC`,
//! a framed file of kind `Source` holding a `SourceHead`, then the packed
//! 12-bit samples in `SourceData` records. The host writes one
//! (`write_source`, feature `source-writer`), reads it back, and seals it
//! read-only; the unit only reads them.

use chimera_hal::store::{Dir, FileName, Store, StoreError};

use super::SourceRef;
use crate::dsp::tape_engine::read::{DESC_LEN, Descriptor, MIN_SOURCE_FRAMES, Shape};
use crate::name::SoundName;

use super::card::Ready;
use super::crc::Crc32;
use super::file::{LoadError, Scan, scan};
use super::frame::{Event, FileError, FileKind, HEADER_LEN, RECORD_HEAD_LEN, TRAILER_LEN};
use super::record::{ReadTag, RecordTag};

/// A `SourceHead` record's payload.
pub const SOURCE_HEAD_LEN: usize = 34;
/// The most packed bytes one `SourceData` record carries: 168 pairs.
pub const SOURCE_DATA_MAX: usize = 504;

/// A source file's head (ADR 0093): `SourceRef` 10, `shape` 1, `count` 1,
/// `root` 2 (MIDI cents), `period` 4 (Q16.16; 0 is unpitched), name 16.
/// Root, period and name are labels: the CRC never covers them. Root and
/// period are the import's estimate, copied into the Sound's
/// `TapeEngineParams` (ROOT, PERIOD) when SOURCE picks the file; the voice
/// tunes from the Sound, never from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceHead {
    pub sref: SourceRef,
    pub shape: Shape,
    pub count: u8,
    pub root: u16,
    pub period_q16: u32,
    pub name: SoundName,
}

impl SourceHead {
    pub fn to_bytes(&self) -> [u8; SOURCE_HEAD_LEN] {
        let mut b = [0; SOURCE_HEAD_LEN];
        b[..10].copy_from_slice(&self.sref.to_bytes());
        (b[10], b[11]) = (self.shape as u8, self.count);
        b[12..14].copy_from_slice(&self.root.to_le_bytes());
        b[14..18].copy_from_slice(&self.period_q16.to_le_bytes());
        b[18..].copy_from_slice(&self.name.padded());
        b
    }

    /// `None` unless exactly 34 bytes with a known shape and a valid name.
    pub fn from_bytes(b: &[u8]) -> Option<SourceHead> {
        let b: &[u8; SOURCE_HEAD_LEN] = b.try_into().ok()?;
        let (sref, rest) = b.split_at(10);
        let (labels, name) = rest.split_at(8);
        Some(SourceHead {
            sref: SourceRef::from_bytes(sref)?,
            shape: Shape::from_code(labels[0])?,
            count: labels[1],
            root: u16::from_le_bytes([labels[2], labels[3]]),
            period_q16: u32::from_le_bytes([labels[4], labels[5], labels[6], labels[7]]),
            name: SoundName::from_padded(name.try_into().ok()?).ok()?,
        })
    }

    /// The descriptor `held` starts with: the whole source, no head.
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            shape: self.shape,
            count: self.count,
            rate: self.sref.rate,
            head_frames: 0,
        }
    }

    /// A head TAPE can play: a descriptor `SourceView` takes, and at
    /// least `MIN_SOURCE_FRAMES`. Not a `ZoneSet` until TAPE Task 10 gives
    /// it its layout: one written now would be sealed under a CRC and a
    /// length that leave the layout out.
    fn playable(&self) -> bool {
        self.sref.frames >= MIN_SOURCE_FRAMES
            && self.shape != Shape::ZoneSet
            && Descriptor::from_bytes(&self.descriptor().to_bytes()).is_some()
    }
}

/// The source's CRC, its file's stem: over `shape`, `count` and the
/// packed bytes, never the labels. TAPE Task 10 folds a `ZoneSet`'s layout
/// in after `count`, so a `Plain` or `Transwave` CRC never changes.
pub fn source_crc(shape: Shape, count: u8, packed: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(&[shape as u8, count]);
    c.update(packed);
    c.finish()
}

/// The bytes the arena holds for `r` whole: the descriptor, then two
/// frames in three bytes. `None` past `usize`.
pub fn held_len(r: SourceRef) -> Option<usize> {
    let pairs = usize::try_from(r.frames.div_ceil(2)).ok()?;
    pairs.checked_mul(3)?.checked_add(DESC_LEN)
}

/// The fewest bytes a whole file of `r` takes, its samples in full
/// `SourceData` records: a listing shorter than this is a torn file.
pub fn source_file_min_len(r: SourceRef) -> Option<u32> {
    let data = held_len(r)? - DESC_LEN;
    let records = (1 + data.div_ceil(SOURCE_DATA_MAX)) * RECORD_HEAD_LEN;
    let n = HEADER_LEN + records + SOURCE_HEAD_LEN + data + TRAILER_LEN;
    u32::try_from(n).ok()
}

/// `/CHIMERA/TAPES/<CRC>.SRC`, the CRC in upper-case hex.
pub fn source_file(r: SourceRef) -> FileName {
    let hex = |n: u32| b"0123456789ABCDEF"[n as usize & 0xF];
    let stem: [u8; 8] = core::array::from_fn(|i| hex(r.crc >> (28 - 4 * i)));
    FileName::new(Dir::Tapes, &stem, b"SRC").expect("8 hex digits is a stem")
}

/// One read of `sref`'s file: the head must name `sref` and be playable,
/// the samples must be exactly its frames and hash to its CRC. The
/// samples land in `out` after `DESC_LEN`, as they stream; the caller
/// trusts `out` only on `Ok`. Returns the head as stored.
fn read_source<S: Store>(
    s: &mut S,
    r: &Ready,
    sref: SourceRef,
    mut out: Option<&mut [u8]>,
) -> Result<SourceHead, LoadError> {
    let bounds = LoadError::File(FileError::Bounds);
    let total = held_len(sref).ok_or(bounds)? - DESC_LEN;
    if out.as_ref().is_some_and(|o| o.len() < DESC_LEN + total) {
        return Err(bounds);
    }
    let mut head: Option<SourceHead> = None;
    let (mut crc, mut len) = (Crc32::new(), 0);
    let mut on = |e: Event<'_>| {
        let (tag, p) = match e {
            Event::Header(_) => return Ok(()),
            Event::Record(ReadTag::Unknown(_), _) => return Ok(()),
            Event::Record(ReadTag::Known(t), p) => (t, p),
        };
        match (tag, head) {
            (RecordTag::SourceHead, None) => {
                let h = SourceHead::from_bytes(p).ok_or(FileError::Bounds)?;
                if h.sref != sref {
                    return Err(FileError::Corrupt);
                }
                if !h.playable() {
                    return Err(FileError::Bounds);
                }
                crc.update(&[h.shape as u8, h.count]);
                head = Some(h);
            }
            (RecordTag::SourceData, Some(_)) => {
                if p.is_empty() || p.len() > SOURCE_DATA_MAX || len + p.len() > total {
                    return Err(FileError::Bounds);
                }
                if let Some(o) = out.as_deref_mut() {
                    o[DESC_LEN + len..][..p.len()].copy_from_slice(p);
                }
                crc.update(p);
                len += p.len();
            }
            _ => return Err(FileError::Corrupt),
        }
        Ok(())
    };
    let h = match scan(s, r, source_file(sref), Some(FileKind::Source), &mut on) {
        Ok(Scan::Passed { .. }) => head.ok_or(LoadError::File(FileError::Corrupt))?,
        Ok(Scan::Failed { err, .. }) => return Err(LoadError::File(err)),
        Err(StoreError::NotFound) => return Err(LoadError::Missing),
        // A broken or short chain: what a cut can leave.
        Err(StoreError::Corrupt) => return Err(LoadError::File(FileError::Truncated)),
        Err(e) => return Err(LoadError::Store(e)),
    };
    if len != total || crc.finish() != sref.crc {
        return Err(LoadError::File(FileError::Corrupt));
    }
    if let Some(o) = out {
        o[..DESC_LEN].copy_from_slice(&h.descriptor().to_bytes());
    }
    Ok(h)
}

/// Source `sref` into `out` as the arena holds it: the descriptor, then
/// the packed samples. Returns the bytes written, `held_len(sref)`. A head
/// that names another ref, a short or damaged file, or a missing one is an
/// error, and `out` holds nothing to trust: the source reads as missing,
/// never as other audio.
///
/// The FAT driver copies each block out of its own buffer, so `out` (the
/// arena, in cached AXI) takes CPU writes only: no D-cache upkeep.
pub fn read_source_into<S: Store>(
    s: &mut S,
    r: &Ready,
    sref: SourceRef,
    out: &mut [u8],
) -> Result<usize, LoadError> {
    read_source(s, r, sref, Some(&mut *out))?;
    held_len(sref).ok_or(LoadError::File(FileError::Bounds))
}

/// What `write_source` found and did.
#[cfg(feature = "source-writer")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrote {
    /// No file: written, verified, sealed.
    New,
    /// A file that verifies: nothing written, sealed if it wasn't (a cut
    /// at the seal, or a host copy that dropped the attribute).
    Present,
    /// An unsealed file that fails its check, the torn remains of a cut
    /// write: deleted, then written, verified and sealed.
    Replaced,
}

/// Writes source `head` with its packed samples `data`, reads it back,
/// checks the head and the CRC, then seals it (ADR 0087). Host only: the
/// firmware never enables `source-writer`.
///
/// `File(Bounds)` with nothing written for a head and data that disagree
/// or that TAPE couldn't play; `Damaged`, nothing written, for a sealed
/// file that fails its check, which only the user may remove. A file is
/// checked before anything else: only one that fails is deleted.
#[cfg(feature = "source-writer")]
pub fn write_source<S: Store>(
    s: &mut S,
    r: &Ready,
    head: &SourceHead,
    data: &[u8],
) -> Result<Wrote, super::SaveError> {
    use super::SaveError;
    use super::frame::{Generation, Header};
    use super::record::write_file;

    let ok = held_len(head.sref) == Some(DESC_LEN + data.len())
        && head.playable()
        && source_crc(head.shape, head.count, data) == head.sref.crc;
    if !ok {
        return Err(SaveError::File(FileError::Bounds));
    }
    let (vol, file) = (r.volume(), source_file(head.sref));
    s.make_dir(vol, Dir::Chimera)?;
    s.make_dir(vol, Dir::Tapes)?;
    let wrote = match read_source(s, r, head.sref, None) {
        Ok(_) => {
            s.seal(vol, file)?;
            return Ok(Wrote::Present);
        }
        Err(LoadError::Missing) => Wrote::New,
        Err(LoadError::Store(e)) => return Err(e.into()),
        // A load's refusal: `read_source` never says it.
        Err(LoadError::Busy) => return Err(StoreError::Io.into()),
        // Only a sealed file refuses the delete: that one is never
        // rewritten.
        Err(LoadError::File(_)) => match s.delete(vol, file) {
            Ok(()) => Wrote::Replaced,
            Err(StoreError::ReadOnly) => return Err(SaveError::Damaged),
            Err(e) => return Err(e.into()),
        },
    };
    let h = Header {
        kind: FileKind::Source,
        generation: Generation::FIRST,
        name: None,
    };
    s.write(vol, file, &mut |sink| {
        write_file(sink, &h, &mut |w| {
            w.put(RecordTag::SourceHead, &head.to_bytes())?;
            data.chunks(SOURCE_DATA_MAX)
                .try_for_each(|c| w.put(RecordTag::SourceData, c))
        })
    })?;
    // Unsealed until it reads back whole: a failure here is replaced by
    // the next write.
    match read_source(s, r, head.sref, None) {
        Ok(back) if back == *head => {}
        Ok(_) => return Err(SaveError::File(FileError::Corrupt)),
        Err(LoadError::Store(e)) => return Err(e.into()),
        Err(LoadError::File(e)) => return Err(SaveError::File(e)),
        Err(LoadError::Missing) => return Err(StoreError::NotFound.into()),
        // A load's refusal: `read_source` never says it.
        Err(LoadError::Busy) => return Err(StoreError::Io.into()),
    }
    s.seal(vol, file)?;
    Ok(wrote)
}
