//! TAPE's source files (ADR 0087, ADR 0093): `/CHIMERA/TAPES/<CRC>.SRC`,
//! a framed file of kind `Source` holding a `SourceHead`, a `ZoneSet`'s
//! `SourceZones`, then the packed 12-bit samples in `SourceData` records.
//! The host writes one (`write_source`, `write_zone_set`, feature
//! `source-writer`), reads it back, and seals it read-only; the unit only
//! reads them, and `SourceCheck` judges one as `put` streams it.

use chimera_hal::store::{Dir, FileName, Store, StoreError};

use super::SourceRef;
use crate::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, MAX_ZONES, MIN_SOURCE_FRAMES, Shape, ZONE_LAYOUT_LEN,
};
use crate::name::SoundName;

use super::card::Ready;
use super::crc::Crc32;
use super::file::{LoadError, Scan, scan};
use super::frame::{Event, FileError, FileKind, Framer, HEADER_LEN, RECORD_HEAD_LEN, TRAILER_LEN};
use super::record::{ReadTag, RecordBuf, RecordTag};

/// A `SourceHead` record's payload.
pub const SOURCE_HEAD_LEN: usize = 34;
/// The most packed bytes one `SourceData` record carries: 168 pairs.
pub const SOURCE_DATA_MAX: usize = 504;
/// One zone in a `SourceZones` record: its layout, then `root` 2 and
/// `period` 4.
pub const ZONE_LEN: usize = ZONE_LAYOUT_LEN + 6;
const MAX_ZONE_COUNT: usize = MAX_ZONES as usize;
const _: () = assert!(ZONE_LEN * MAX_ZONE_COUNT <= super::MAX_RECORD_LEN);

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
}

/// One zone of a `ZoneSet` (spec § 8): its frames in the source, the
/// MIDI note it serves, and its labels. 15 B on disk. Its layout
/// (`start`, `frames`, `key`) is in the CRC and in `held`; `root` and
/// `period` are labels, as a head's are, copied into the Sound's
/// `ZoneTune` when SOURCE picks the set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Zone {
    pub start: u32,
    pub frames: u32,
    pub key: u8,
    pub root: u16,
    pub period_q16: u32,
}

impl Zone {
    /// What `held` and the CRC carry of it: `start`, `frames`, `key`.
    fn layout(&self) -> [u8; ZONE_LAYOUT_LEN] {
        let mut b = [0; ZONE_LAYOUT_LEN];
        b[..4].copy_from_slice(&self.start.to_le_bytes());
        b[4..8].copy_from_slice(&self.frames.to_le_bytes());
        b[8] = self.key;
        b
    }

    fn to_bytes(self) -> [u8; ZONE_LEN] {
        let mut b = [0; ZONE_LEN];
        b[..ZONE_LAYOUT_LEN].copy_from_slice(&self.layout());
        b[9..11].copy_from_slice(&self.root.to_le_bytes());
        b[11..].copy_from_slice(&self.period_q16.to_le_bytes());
        b
    }

    fn from_bytes(b: &[u8; ZONE_LEN]) -> Zone {
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Zone {
            start: u32_at(0),
            frames: u32_at(4),
            key: b[8],
            root: u16::from_le_bytes([b[9], b[10]]),
            period_q16: u32_at(11),
        }
    }
}

/// A `ZoneSet`'s zones, as its `SourceZones` record (0x0019) holds them:
/// 1 to `MAX_ZONES`, end to end from frame 0, each at least
/// `MIN_SOURCE_FRAMES` (a source a model could play alone), their keys
/// MIDI notes rising, so a note finds one zone. Only `new` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceZones {
    /// Past `n`, `Zone::default()`.
    zones: [Zone; MAX_ZONE_COUNT],
    n: u8,
}

impl SourceZones {
    pub fn new(zones: &[Zone]) -> Option<SourceZones> {
        let n = u8::try_from(zones.len()).ok()?;
        if !(1..=MAX_ZONES).contains(&n) {
            return None;
        }
        let (mut at, mut key) = (0u32, None::<u8>);
        for z in zones {
            let fits = z.start == at
                && z.frames >= MIN_SOURCE_FRAMES
                && z.key <= 127
                && key.is_none_or(|k| z.key > k);
            if !fits {
                return None;
            }
            at = z.start.checked_add(z.frames)?;
            key = Some(z.key);
        }
        let mut all = [Zone::default(); MAX_ZONE_COUNT];
        all[..zones.len()].copy_from_slice(zones);
        Some(SourceZones { zones: all, n })
    }

    pub fn zones(&self) -> &[Zone] {
        &self.zones[..self.n as usize]
    }

    /// The frames the zones cover, end to end: their source's.
    pub fn frames(&self) -> u32 {
        let last = self.zones()[self.zones().len() - 1];
        last.start + last.frames
    }

    /// The record's payload: `ZONE_LEN` bytes a zone.
    pub fn to_record(&self) -> RecordBuf {
        let mut r = RecordBuf::new();
        self.zones().iter().for_each(|z| r.bytes(&z.to_bytes()));
        r
    }

    /// `None` unless whole zones that `new` takes.
    pub fn from_record(p: &[u8]) -> Option<SourceZones> {
        let (zones, rest) = p.as_chunks::<ZONE_LEN>();
        if !rest.is_empty() || zones.len() > MAX_ZONE_COUNT {
            return None;
        }
        let mut all = [Zone::default(); MAX_ZONE_COUNT];
        for (z, b) in all.iter_mut().zip(zones) {
            *z = Zone::from_bytes(b);
        }
        SourceZones::new(&all[..zones.len()])
    }
}

/// A head with the layout its shape needs, that TAPE can play: a
/// `ZoneSet` with the zones it counts, covering its frames; a `Plain` or
/// `Transwave` with none. At least `MIN_SOURCE_FRAMES`, a descriptor
/// `SourceView` takes, and a `held` that fits `usize`. Only `whole` and
/// `zoned` make one, so a head never meets a layout it doesn't describe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceLayout {
    head: SourceHead,
    zones: Option<SourceZones>,
}

impl SourceLayout {
    /// A `Plain` or `Transwave` head: it has no layout.
    pub fn whole(head: SourceHead) -> Option<SourceLayout> {
        let l = SourceLayout { head, zones: None };
        (head.shape != Shape::ZoneSet && l.playable()).then_some(l)
    }

    /// A `ZoneSet` head with its zones: `count` of them, ending at its
    /// frames.
    pub fn zoned(head: SourceHead, zones: SourceZones) -> Option<SourceLayout> {
        let l = SourceLayout {
            head,
            zones: Some(zones),
        };
        let fits = head.shape == Shape::ZoneSet
            && head.count == zones.n
            && head.sref.frames == zones.frames();
        (fits && l.playable()).then_some(l)
    }

    fn playable(&self) -> bool {
        self.head.sref.frames >= MIN_SOURCE_FRAMES
            && Descriptor::from_bytes(&self.descriptor().to_bytes()).is_some()
            && held_len(self.head.sref)
                .and_then(|n| n.checked_add(self.layout_len()))
                .is_some()
    }

    pub fn head(&self) -> &SourceHead {
        &self.head
    }

    /// A `ZoneSet`'s zones; none for a whole source.
    pub fn zones(&self) -> &[Zone] {
        self.zones.as_ref().map_or(&[], SourceZones::zones)
    }

    /// The descriptor `held` starts with: the whole source, no head.
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            shape: self.head.shape,
            count: self.head.count,
            rate: self.head.sref.rate,
            head_frames: 0,
        }
    }

    fn layout_len(&self) -> usize {
        ZONE_LAYOUT_LEN * self.zones().len()
    }

    /// Where the samples start in `held`: after the descriptor and layout.
    pub fn data_start(&self) -> usize {
        DESC_LEN + self.layout_len()
    }

    /// The packed samples' bytes: two frames in three.
    pub fn data_len(&self) -> usize {
        3 * self.head.sref.frames.div_ceil(2) as usize
    }

    /// The bytes the arena holds for the source whole (spec § 1, § 8): the
    /// descriptor, a `ZoneSet`'s layout, then the samples.
    pub fn held_len(&self) -> usize {
        self.data_start() + self.data_len()
    }

    /// The CRC its samples `packed` must hash to: its stem.
    pub fn crc(&self, packed: &[u8]) -> u32 {
        let mut c = self.crc_prefix();
        c.update(packed);
        c.finish()
    }

    /// What the CRC covers before the samples: `shape`, `count`, then a
    /// `ZoneSet`'s layout.
    fn crc_prefix(&self) -> Crc32 {
        crc_prefix(self.head.shape, self.head.count, self.zones())
    }
}

fn crc_prefix(shape: Shape, count: u8, zones: &[Zone]) -> Crc32 {
    let mut c = Crc32::new();
    c.update(&[shape as u8, count]);
    zones.iter().for_each(|z| c.update(&z.layout()));
    c
}

/// The source's CRC, its file's stem: over `shape`, `count` and the
/// packed bytes, never the labels. A `ZoneSet`'s is `zone_set_crc`, so a
/// `Plain` or `Transwave` CRC never changes.
pub fn source_crc(shape: Shape, count: u8, packed: &[u8]) -> u32 {
    let mut c = crc_prefix(shape, count, &[]);
    c.update(packed);
    c.finish()
}

/// A `ZoneSet`'s CRC: `source_crc`'s, with each zone's `start`, `frames`
/// and `key` folded in after `count`. Never a zone's `root` or `period`:
/// a voice's reads and its zone choice depend on the layout, not on them.
pub fn zone_set_crc(zones: &SourceZones, packed: &[u8]) -> u32 {
    let mut c = crc_prefix(Shape::ZoneSet, zones.n, zones.zones());
    c.update(packed);
    c.finish()
}

/// The bytes the arena holds for a `Plain` or `Transwave` `r` whole: the
/// descriptor, then two frames in three bytes. A `ZoneSet` holds its
/// layout too (`SourceLayout::held_len`), so for one this is the least.
/// `None` past `usize`.
pub fn held_len(r: SourceRef) -> Option<usize> {
    let pairs = usize::try_from(r.frames.div_ceil(2)).ok()?;
    pairs.checked_mul(3)?.checked_add(DESC_LEN)
}

/// `held`'s start for `l`: the descriptor, then a `ZoneSet`'s layout, a
/// zone's `start`, `frames` and `key` each (spec § 8). Returns its length,
/// where the samples start. Panics if `out` is shorter.
pub fn write_descriptor(l: &SourceLayout, out: &mut [u8]) -> usize {
    out[..DESC_LEN].copy_from_slice(&l.descriptor().to_bytes());
    for (i, z) in l.zones().iter().enumerate() {
        out[DESC_LEN + i * ZONE_LAYOUT_LEN..][..ZONE_LAYOUT_LEN].copy_from_slice(&z.layout());
    }
    l.data_start()
}

/// The fewest bytes a whole file of `r` takes, its samples in full
/// `SourceData` records: a listing shorter than this is a torn file.
pub fn source_file_min_len(r: SourceRef) -> Option<u32> {
    let data = held_len(r)? - DESC_LEN;
    let records = (1 + data.div_ceil(SOURCE_DATA_MAX)) * RECORD_HEAD_LEN;
    let n = HEADER_LEN + records + SOURCE_HEAD_LEN + data + TRAILER_LEN;
    u32::try_from(n).ok()
}

/// A `Plain` or `Transwave` source's `held`, from its file's length as
/// `write_source` frames it (`source_file_min_len`): the card rows' cost,
/// with no read. `None` for a length no such file has. A `ZoneSet`'s file
/// carries its zones too, so its length doesn't say.
pub fn held_for_file_len(len: u32) -> Option<u32> {
    let fixed = HEADER_LEN + RECORD_HEAD_LEN + SOURCE_HEAD_LEN + TRAILER_LEN;
    // The samples and their records' heads, one a `SOURCE_DATA_MAX`.
    let body = (len as usize).checked_sub(fixed)?;
    let records = body.div_ceil(SOURCE_DATA_MAX + RECORD_HEAD_LEN);
    let data = body.checked_sub(records * RECORD_HEAD_LEN)?;
    let framed = data > 0 && data % 3 == 0 && data.div_ceil(SOURCE_DATA_MAX) == records;
    framed
        .then(|| u32::try_from(DESC_LEN + data).ok())
        .flatten()
}

/// `/CHIMERA/TAPES/<CRC>.SRC`, the CRC in upper-case hex.
pub fn source_file(r: SourceRef) -> FileName {
    let hex = |n: u32| b"0123456789ABCDEF"[n as usize & 0xF];
    let stem: [u8; 8] = core::array::from_fn(|i| hex(r.crc >> (28 - 4 * i)));
    FileName::new(Dir::Tapes, &stem, b"SRC").expect("8 hex digits is a stem")
}

/// What a source file's head must name: the ref a load asked for, or the
/// stem `put` gives it.
#[derive(Clone, Copy)]
enum Expect {
    Ref(SourceRef),
    Stem(u32),
}

/// Why a source file's records are refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bad {
    /// A head or zones that don't parse, or that TAPE can't play.
    Head,
    /// A head that names another source.
    Other,
    /// No room in `out` for the source.
    Room,
    /// A record out of order.
    Order,
    /// A `SourceData` record empty or past `SOURCE_DATA_MAX`.
    Record,
    /// More samples than the head's frames.
    Long,
    /// Fewer.
    Short,
    /// Samples that don't hash to the head's CRC.
    Crc,
}

impl Bad {
    /// A load's reading: the pool's (ADR 0087).
    fn file(self) -> FileError {
        match self {
            Bad::Head | Bad::Room | Bad::Record | Bad::Long => FileError::Bounds,
            Bad::Other | Bad::Order | Bad::Short | Bad::Crc => FileError::Corrupt,
        }
    }

    /// `put`'s reading.
    fn put(self) -> PutRefusal {
        match self {
            Bad::Long | Bad::Short => PutRefusal::Frames,
            Bad::Other | Bad::Crc => PutRefusal::Crc,
            Bad::Head | Bad::Room | Bad::Order | Bad::Record => PutRefusal::File(self.file()),
        }
    }
}

/// Where a source file's records are: the head, a `ZoneSet`'s zones, then
/// the samples.
#[expect(clippy::large_enum_variant, reason = "one, in one read's frame")]
enum Step {
    Head,
    Zones(SourceHead),
    Data(SourceLayout),
}

/// A source file's records as they stream: each checked as it comes, the
/// samples hashed and, with `out`, written where `held` keeps them.
struct Records<'o> {
    expect: Expect,
    step: Step,
    crc: Crc32,
    /// Sample bytes so far.
    len: usize,
    out: Option<&'o mut [u8]>,
}

impl<'o> Records<'o> {
    fn new(expect: Expect, out: Option<&'o mut [u8]>) -> Self {
        Records {
            expect,
            step: Step::Head,
            crc: Crc32::new(),
            len: 0,
            out,
        }
    }

    fn record(&mut self, tag: RecordTag, p: &[u8]) -> Result<(), Bad> {
        match (tag, &self.step) {
            (RecordTag::SourceHead, Step::Head) => {
                let h = SourceHead::from_bytes(p).ok_or(Bad::Head)?;
                let named = match self.expect {
                    Expect::Ref(r) => h.sref == r,
                    Expect::Stem(crc) => h.sref.crc == crc,
                };
                if !named {
                    return Err(Bad::Other);
                }
                if h.shape == Shape::ZoneSet {
                    self.step = Step::Zones(h);
                    return Ok(());
                }
                self.begin(SourceLayout::whole(h).ok_or(Bad::Head)?)
            }
            (RecordTag::SourceZones, Step::Zones(h)) => {
                let h = *h;
                let z = SourceZones::from_record(p).ok_or(Bad::Head)?;
                self.begin(SourceLayout::zoned(h, z).ok_or(Bad::Head)?)
            }
            (RecordTag::SourceData, Step::Data(l)) => {
                let (start, total, crc) = (l.data_start(), l.data_len(), l.head.sref.crc);
                self.data(p, start, total, crc)
            }
            _ => Err(Bad::Order),
        }
    }

    /// Samples `p`, at `start` in `held`, of `total` bytes hashing to `crc`.
    fn data(&mut self, p: &[u8], start: usize, total: usize, crc: u32) -> Result<(), Bad> {
        if p.is_empty() || p.len() > SOURCE_DATA_MAX {
            return Err(Bad::Record);
        }
        if self.len + p.len() > total {
            return Err(Bad::Long);
        }
        if let Some(o) = self.out.as_deref_mut() {
            o[start + self.len..][..p.len()].copy_from_slice(p);
        }
        self.crc.update(p);
        self.len += p.len();
        // The last samples' record is the one that shows the CRC.
        if self.len == total && self.crc.finish() != crc {
            return Err(Bad::Crc);
        }
        Ok(())
    }

    fn begin(&mut self, l: SourceLayout) -> Result<(), Bad> {
        if self.out.as_ref().is_some_and(|o| o.len() < l.held_len()) {
            return Err(Bad::Room);
        }
        self.crc = l.crc_prefix();
        self.step = Step::Data(l);
        Ok(())
    }

    /// After the last record: the layout, its samples whole. With `out`,
    /// writes the descriptor before them.
    fn end(&mut self) -> Result<SourceLayout, Bad> {
        let Step::Data(l) = &self.step else {
            return Err(Bad::Order);
        };
        let l = *l;
        if self.len != l.data_len() {
            return Err(Bad::Short);
        }
        if let Some(o) = self.out.as_deref_mut() {
            write_descriptor(&l, o);
        }
        Ok(l)
    }
}

/// One read of `sref`'s file: the head must name `sref`, and with any
/// zones be a layout TAPE can play; the samples must be exactly its frames
/// and hash to its CRC. With `out`, `held` lands in it as it streams; the
/// caller trusts `out` only on `Ok`. Returns the layout as stored.
fn read_source<S: Store>(
    s: &mut S,
    r: &Ready,
    sref: SourceRef,
    out: Option<&mut [u8]>,
) -> Result<SourceLayout, LoadError> {
    let mut rec = Records::new(Expect::Ref(sref), out);
    let mut on = |e: Event<'_>| match e {
        Event::Header(_) | Event::Record(ReadTag::Unknown(_), _) => Ok(()),
        Event::Record(ReadTag::Known(t), p) => rec.record(t, p).map_err(Bad::file),
    };
    match scan(s, r, source_file(sref), Some(FileKind::Source), &mut on) {
        Ok(Scan::Passed { .. }) => {}
        Ok(Scan::Failed { err, .. }) => return Err(LoadError::File(err)),
        Err(StoreError::NotFound) => return Err(LoadError::Missing),
        // A broken or short chain: what a cut can leave.
        Err(StoreError::Corrupt) => return Err(LoadError::File(FileError::Truncated)),
        Err(e) => return Err(LoadError::Store(e)),
    }
    rec.end().map_err(|b| LoadError::File(b.file()))
}

/// Source `sref` into `out` as the arena holds it: the descriptor, a
/// `ZoneSet`'s layout, then the packed samples. Returns the bytes written,
/// the layout's `held_len`; `File(Bounds)` if `out` is shorter. A head
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
    read_source(s, r, sref, Some(out)).map(|l| l.held_len())
}

/// Why `put` refuses a source file (TAPE Task 18), at the record that
/// shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutRefusal {
    /// Not a `Source` file: its header says.
    Kind,
    /// Samples past the head's frames (at the record that overruns), or
    /// short of them (at the file's end).
    Frames,
    /// A head naming another stem, or samples that don't hash to it (at
    /// the last sample's record).
    Crc,
    /// Bad framing, records out of order, or a head or zones TAPE can't
    /// play.
    File(FileError),
}

/// Judges a source file as `put` streams it, before anything is kept: it
/// must be a `Source`, named by the stem it goes under, its samples its
/// head's frames and hashing to that stem. Each refusal comes from the
/// `feed` whose bytes show it, and sticks.
pub struct SourceCheck {
    framer: Framer,
    rec: Records<'static>,
    /// Bytes fed, of `len`.
    fed: u64,
    len: u32,
    refused: Option<PutRefusal>,
}

impl SourceCheck {
    /// A file of `len` bytes, put as `<stem_crc>.SRC`. `len` is the one
    /// `put` announces: the framer needs it to find the trailer.
    pub fn new(stem_crc: u32, len: u32) -> Result<SourceCheck, PutRefusal> {
        Ok(SourceCheck {
            framer: Framer::new(len).map_err(PutRefusal::File)?,
            rec: Records::new(Expect::Stem(stem_crc), None),
            fed: 0,
            len,
            refused: None,
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), PutRefusal> {
        if let Some(r) = self.refused {
            return Err(r);
        }
        let mut said = None;
        let rec = &mut self.rec;
        let pushed = self.framer.push(bytes, &mut |e| {
            let r = match e {
                Event::Header(h) if h.kind != FileKind::Source => Err(PutRefusal::Kind),
                Event::Header(_) | Event::Record(ReadTag::Unknown(_), _) => Ok(()),
                Event::Record(ReadTag::Known(t), p) => rec.record(t, p).map_err(Bad::put),
            };
            // The framer stops the events at an error; `said` keeps ours.
            r.map_err(|e| *said.insert(e))
                .map_err(|_| FileError::Corrupt)
        });
        self.fed += bytes.len() as u64;
        let r = match (said, pushed) {
            (Some(r), _) => Err(r),
            (None, Err(e)) => Err(PutRefusal::File(e)),
            (None, Ok(())) if self.fed == self.len as u64 => self.verdict().map(|_| ()),
            (None, Ok(())) => Ok(()),
        };
        self.refused = r.err();
        r
    }

    /// The head, once every byte came and passed.
    pub fn finish(mut self) -> Result<SourceHead, PutRefusal> {
        if let Some(r) = self.refused {
            return Err(r);
        }
        self.verdict()
    }

    fn verdict(&mut self) -> Result<SourceHead, PutRefusal> {
        self.framer.finish().map_err(PutRefusal::File)?;
        self.rec.end().map(|l| l.head).map_err(Bad::put)
    }
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
/// or that TAPE couldn't play, a `ZoneSet` included (its zones go through
/// `write_zone_set`); `Damaged`, nothing written, for a sealed file that
/// fails its check, which only the user may remove. A file is checked
/// before anything else: only one that fails is deleted.
#[cfg(feature = "source-writer")]
pub fn write_source<S: Store>(
    s: &mut S,
    r: &Ready,
    head: &SourceHead,
    data: &[u8],
) -> Result<Wrote, super::SaveError> {
    let l = SourceLayout::whole(*head).ok_or(super::SaveError::File(FileError::Bounds))?;
    write_layout(s, r, &l, data)
}

/// `write_source` for a `ZoneSet`: its head, then its `zones` in a
/// `SourceZones` record, then the samples.
#[cfg(feature = "source-writer")]
pub fn write_zone_set<S: Store>(
    s: &mut S,
    r: &Ready,
    head: &SourceHead,
    zones: &SourceZones,
    data: &[u8],
) -> Result<Wrote, super::SaveError> {
    let l = SourceLayout::zoned(*head, *zones).ok_or(super::SaveError::File(FileError::Bounds))?;
    write_layout(s, r, &l, data)
}

#[cfg(feature = "source-writer")]
fn write_layout<S: Store>(
    s: &mut S,
    r: &Ready,
    l: &SourceLayout,
    data: &[u8],
) -> Result<Wrote, super::SaveError> {
    use super::SaveError;
    use super::frame::{Generation, Header};
    use super::record::write_file;

    let sref = l.head.sref;
    if data.len() != l.data_len() || l.crc(data) != sref.crc {
        return Err(SaveError::File(FileError::Bounds));
    }
    let (vol, file) = (r.volume(), source_file(sref));
    s.make_dir(vol, Dir::Chimera)?;
    s.make_dir(vol, Dir::Tapes)?;
    let wrote = match read_source(s, r, sref, None) {
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
            w.put(RecordTag::SourceHead, &l.head.to_bytes())?;
            if let Some(z) = &l.zones {
                w.put(RecordTag::SourceZones, z.to_record().as_slice())?;
            }
            data.chunks(SOURCE_DATA_MAX)
                .try_for_each(|c| w.put(RecordTag::SourceData, c))
        })
    })?;
    // Unsealed until it reads back whole: a failure here is replaced by
    // the next write.
    match read_source(s, r, sref, None) {
        Ok(back) if back == *l => {}
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
