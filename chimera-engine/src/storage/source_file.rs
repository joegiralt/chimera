//! TAPE's source files (ADR 0087, ADR 0093): `/CHIMERA/TAPES/<CRC>.SRC`,
//! a framed file of kind `Source` holding a `SourceHead`, a `ZoneSet`'s
//! `SourceZones`, then the packed 12-bit samples in `SourceData` records.
//! The host writes one (`write_source`, `write_zone_set`, feature
//! `source-writer`), reads it back, and seals it read-only. The unit takes
//! one only through `put` (ADR 0106): `SourceCheck` judges it as it
//! streams, `source_target` and `verify_and_seal` around the write.

use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError};
use chimera_hal::units::Bytes;

use super::{SourceRef, Stem};
use crate::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, MAX_ZONES, MIN_SOURCE_FRAMES, Shape, ZONE_LAYOUT_LEN,
};
use crate::name::SoundName;

use super::Crc32;
use super::card::Ready;
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
            && self.descriptor().is_some()
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
    fn descriptor(&self) -> Option<Descriptor> {
        Descriptor::whole(self.head.shape, self.head.count, self.head.sref.rate)
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

/// The first `SourceData` record's offset in the file: after the header,
/// the head record and a zoned source's `SourceZones` record.
pub fn data_offset(l: &SourceLayout) -> Bytes {
    let zones = match l.zones().len() {
        0 => 0,
        n => RECORD_HEAD_LEN + ZONE_LEN * n,
    };
    Bytes::new((HEADER_LEN + RECORD_HEAD_LEN + SOURCE_HEAD_LEN + zones) as u32)
}

fn crc_prefix(shape: Shape, count: u8, zones: &[Zone]) -> Crc32 {
    let mut c = Crc32::new();
    c.update(&[shape as u8, count]);
    zones.iter().for_each(|z| c.update(&z.layout()));
    c
}

/// A shape held whole, with no zone layout: the shapes `source_crc`
/// hashes. A `ZoneSet` hashes its layout too, through `zone_set_crc`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WholeShape {
    Plain,
    Transwave,
}

impl From<WholeShape> for Shape {
    fn from(w: WholeShape) -> Shape {
        match w {
            WholeShape::Plain => Shape::Plain,
            WholeShape::Transwave => Shape::Transwave,
        }
    }
}

/// The source's CRC, its file's stem: over `shape`, `count` and the
/// packed bytes, never the labels. A `ZoneSet`'s is `zone_set_crc`, so a
/// `Plain` or `Transwave` CRC never changes.
pub fn source_crc(shape: WholeShape, count: u8, packed: &[u8]) -> u32 {
    let mut c = crc_prefix(shape.into(), count, &[]);
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
/// layout too (`held_len_zoned`). `None` past `usize`.
pub fn held_len(r: SourceRef) -> Option<usize> {
    let pairs = usize::try_from(r.frames.div_ceil(2)).ok()?;
    pairs.checked_mul(3)?.checked_add(DESC_LEN)
}

/// `held`'s start for `l`: the descriptor, then a `ZoneSet`'s layout, a
/// zone's `start`, `frames` and `key` each (spec § 8). Returns its length,
/// where the samples start. Panics if `out` is shorter.
pub fn write_descriptor(l: &SourceLayout, out: &mut [u8]) -> usize {
    // `playable` made `Descriptor::whole` of these three accept them: held
    // whole, no heads, no reach, so the rest is zero.
    let h = l.head();
    debug_assert!(l.descriptor().is_some());
    let d = &mut out[..DESC_LEN];
    d.fill(0);
    (d[0], d[1]) = (h.shape as u8, h.count);
    d[2..4].copy_from_slice(&h.sref.rate.to_le_bytes());
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

/// A source's `held`, from its file's length as `write_source` frames it
/// (`source_file_min_len`), with no read: exact for a `Plain` or
/// `Transwave` file, an upper bound for any file (a `ZoneSet`'s length can
/// parse as a whole file's, and then overcounts by up to 6 B a zone and 4).
/// `None` for a length no whole file has. A card row takes the shape from
/// the head, not from `Some` (Task 17).
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

/// The zones a listed file of `r` holds, from its size: 0 for a whole
/// source as written, `n` for a `ZoneSet`, whose `SourceZones` record adds
/// `RECORD_HEAD_LEN + ZONE_LEN·n` B; at most `MAX_ZONES`, so a padded file
/// counts high, never low. `None` for a file shorter than a whole one: torn.
pub fn listed_zones(r: SourceRef, size: u32) -> Option<u8> {
    let extra = size.checked_sub(source_file_min_len(r)?)? as usize;
    // The record's head is less than a zone: it drops out of the division.
    const _: () = assert!(RECORD_HEAD_LEN < ZONE_LEN);
    Some((extra / ZONE_LEN).min(MAX_ZONE_COUNT) as u8)
}

/// What `held` takes for `r` with `zones` zones: `held_len`, then 9 B a
/// zone's layout. `None` past `usize`.
pub fn held_len_zoned(r: SourceRef, zones: u8) -> Option<usize> {
    held_len(r)?.checked_add(ZONE_LAYOUT_LEN * zones as usize)
}

/// `/CHIMERA/TAPES/<CRC>.SRC`, the CRC in upper-case hex.
pub fn source_file(r: SourceRef) -> FileName {
    source_name(r.stem())
}

/// `source_file` by the stem alone, as `put` names it.
pub fn source_name(stem: Stem) -> FileName {
    let crc = stem.get();
    let hex = |n: u32| b"0123456789ABCDEF"[n as usize & 0xF];
    let stem: [u8; 8] = core::array::from_fn(|i| hex(crc >> (28 - 4 * i)));
    FileName::new(Dir::Tapes, &stem, b"SRC").expect("8 hex digits is a stem")
}

/// Card rows listed; past this many files the rest don't show.
pub const MAX_TAPES: usize = 64;

/// `/CHIMERA/TAPES` as listed: each `.SRC` stem and size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TapeList {
    files: [(Stem, u32); MAX_TAPES],
    n: u8,
}

impl TapeList {
    pub const EMPTY: TapeList = TapeList {
        files: [(Stem::new(0), 0); MAX_TAPES],
        n: 0,
    };

    pub fn files(&self) -> &[(Stem, u32)] {
        &self.files[..self.n as usize]
    }

    /// One directory read; no file is opened. No `TAPES` is an empty list.
    pub fn list(s: &mut dyn Store, r: &Ready) -> Result<TapeList, StoreError> {
        let mut l = TapeList::EMPTY;
        let listed = s.list(r.volume(), Dir::Tapes, &mut |f, size| {
            let stem = core::str::from_utf8(f.stem()).ok();
            let stem = stem.filter(|t| t.len() == 8 && f.ext() == b"SRC");
            if let Some(crc) = stem.and_then(|t| u32::from_str_radix(t, 16).ok())
                && (l.n as usize) < MAX_TAPES
            {
                l.files[l.n as usize] = (Stem::new(crc), size);
                l.n += 1;
            }
        });
        match listed {
            Ok(()) | Err(StoreError::NotFound) => Ok(l),
            Err(e) => Err(e),
        }
    }
}

/// What a source file's head must name: the ref a load asked for, or the
/// stem `put` gives it.
#[derive(Clone, Copy)]
enum Expect {
    Ref(SourceRef),
    Stem(Stem),
}

impl Expect {
    fn file(self) -> FileName {
        match self {
            Expect::Ref(r) => source_file(r),
            Expect::Stem(stem) => source_name(stem),
        }
    }
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
/// samples hashed and, with `out` (`held`, lent each call), written where
/// `held` keeps them.
struct Records {
    expect: Expect,
    step: Step,
    crc: Crc32,
    /// Sample bytes so far.
    len: usize,
}

impl Records {
    fn new(expect: Expect) -> Self {
        Records {
            expect,
            step: Step::Head,
            crc: Crc32::new(),
            len: 0,
        }
    }

    /// The head, once its record came.
    fn head(&self) -> Option<SourceHead> {
        match &self.step {
            Step::Head => None,
            Step::Zones(h) => Some(*h),
            Step::Data(l) => Some(l.head),
        }
    }

    fn record(&mut self, tag: RecordTag, p: &[u8], out: Option<&mut [u8]>) -> Result<(), Bad> {
        match (tag, &self.step) {
            (RecordTag::SourceHead, Step::Head) => {
                let h = SourceHead::from_bytes(p).ok_or(Bad::Head)?;
                let named = match self.expect {
                    Expect::Ref(r) => h.sref == r,
                    Expect::Stem(stem) => h.sref.stem() == stem,
                };
                if !named {
                    return Err(Bad::Other);
                }
                if h.shape == Shape::ZoneSet {
                    self.step = Step::Zones(h);
                    return Ok(());
                }
                self.begin(SourceLayout::whole(h).ok_or(Bad::Head)?, out)
            }
            (RecordTag::SourceZones, Step::Zones(h)) => {
                let h = *h;
                let z = SourceZones::from_record(p).ok_or(Bad::Head)?;
                self.begin(SourceLayout::zoned(h, z).ok_or(Bad::Head)?, out)
            }
            (RecordTag::SourceData, Step::Data(l)) => {
                let (start, total, crc) = (l.data_start(), l.data_len(), l.head.sref.crc);
                self.data(p, start, total, crc, out)
            }
            _ => Err(Bad::Order),
        }
    }

    /// Samples `p`, at `start` in `held`, of `total` bytes hashing to `crc`.
    fn data(
        &mut self,
        p: &[u8],
        start: usize,
        total: usize,
        crc: u32,
        out: Option<&mut [u8]>,
    ) -> Result<(), Bad> {
        if p.is_empty() || p.len() > SOURCE_DATA_MAX {
            return Err(Bad::Record);
        }
        if self.len + p.len() > total {
            return Err(Bad::Long);
        }
        if let Some(o) = out {
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

    fn begin(&mut self, l: SourceLayout, out: Option<&mut [u8]>) -> Result<(), Bad> {
        if out.is_some_and(|o| o.len() < l.held_len()) {
            return Err(Bad::Room);
        }
        self.crc = l.crc_prefix();
        self.step = Step::Data(l);
        Ok(())
    }

    /// After the last record: the layout, its samples whole. With `out`,
    /// writes the descriptor before them.
    fn end(&mut self, out: Option<&mut [u8]>) -> Result<SourceLayout, Bad> {
        let Step::Data(l) = &self.step else {
            return Err(Bad::Order);
        };
        let l = *l;
        if self.len != l.data_len() {
            return Err(Bad::Short);
        }
        if let Some(o) = out {
            write_descriptor(&l, o);
        }
        Ok(l)
    }
}

/// One read of the file `expect` names: its head must name it, and with
/// any zones be a layout TAPE can play; the samples must be exactly its
/// frames and hash to its CRC. With `out`, `held` lands in it as it
/// streams; the caller trusts `out` only on `Ok`. Returns the layout as
/// stored.
fn read_source<S: Store>(
    s: &mut S,
    r: &Ready,
    expect: Expect,
    mut out: Option<&mut [u8]>,
) -> Result<SourceLayout, SourceReadError> {
    let mut rec = Records::new(expect);
    let mut room = false;
    let mut on = |e: Event<'_>| match e {
        Event::Header(_) | Event::Record(ReadTag::Unknown(_), _) => Ok(()),
        Event::Record(ReadTag::Known(t), p) => rec.record(t, p, out.as_deref_mut()).map_err(|b| {
            room |= b == Bad::Room;
            b.file()
        }),
    };
    let load = |e| Err(SourceReadError::Load(e));
    match scan(s, r, expect.file(), Some(FileKind::Source), &mut on) {
        // Out of room only if the file is whole: a torn one is torn.
        Ok(Scan::Failed { err, .. }) if room && !err.is_torn() => {
            return Err(SourceReadError::NoRoom);
        }
        Ok(Scan::Passed { .. }) => {}
        Ok(Scan::Failed { err, .. }) => return load(LoadError::File(err)),
        Err(StoreError::NotFound) => return load(LoadError::Missing),
        // A broken or short chain: what a cut can leave.
        Err(StoreError::Corrupt) => return load(LoadError::File(FileError::Truncated)),
        Err(e) => return load(LoadError::Store(e)),
    }
    rec.end(out)
        .map_err(|b| SourceReadError::Load(LoadError::File(b.file())))
}

/// A read back with no `out`, which has no room to run out of.
#[cfg(feature = "source-writer")]
fn read_back<S: Store>(s: &mut S, r: &Ready, e: Expect) -> Result<SourceLayout, LoadError> {
    read_source(s, r, e, None).map_err(|e| match e {
        SourceReadError::Load(e) => e,
        SourceReadError::NoRoom => LoadError::File(FileError::Bounds),
    })
}

/// Why `read_source_into` read no source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceReadError {
    /// A whole file whose `held` is more than `out`: it didn't fit.
    NoRoom,
    /// Missing, damaged, another ref's, or a card fault.
    Load(LoadError),
}

impl From<LoadError> for SourceReadError {
    fn from(e: LoadError) -> Self {
        SourceReadError::Load(e)
    }
}

impl From<StoreError> for SourceReadError {
    fn from(e: StoreError) -> Self {
        SourceReadError::Load(LoadError::Store(e))
    }
}

/// A card fault ends a load's reads; a source that didn't fit is no fault.
impl super::card::CardFault for SourceReadError {
    fn store_error(&self) -> Option<StoreError> {
        match self {
            SourceReadError::Load(e) => e.store_error(),
            SourceReadError::NoRoom => None,
        }
    }
}

/// Source `sref` into `out` as the arena holds it: the descriptor, a
/// `ZoneSet`'s layout, then the packed samples. Returns the bytes written,
/// the layout's `held_len`; `NoRoom` if `out` is shorter. A head
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
) -> Result<usize, SourceReadError> {
    read_source(s, r, Expect::Ref(sref), Some(out)).map(|l| l.held_len())
}

/// A whole `Plain` file of `r`'s packed bytes: where its first
/// `SourceData` record starts, and how many sample bytes there are.
fn plain_data(r: SourceRef) -> Option<(u32, u32)> {
    let off = HEADER_LEN + RECORD_HEAD_LEN + SOURCE_HEAD_LEN;
    let len = u32::try_from(held_len(r)? - DESC_LEN).ok()?;
    Some((off as u32, len))
}

/// Takes the samples of `SourceData` records from a record's start, the
/// first `skip` bytes of the first one dropped, into `out`: each record's
/// tag and length checked (a full record but the last), never its
/// samples. A streamed load trusts the seal (Review Focus 2).
struct HeadSink<'a> {
    out: &'a mut [u8],
    got: usize,
    skip: usize,
    /// Sample bytes left in the file from the record being read.
    left: u32,
    head: [u8; RECORD_HEAD_LEN],
    /// Head bytes seen of the record being read, then its samples left.
    at_head: usize,
    body: usize,
    bad: bool,
}

impl ReadSink for HeadSink<'_> {
    fn begin(&mut self, _len: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, mut bytes: &[u8]) -> ControlFlow<()> {
        while !bytes.is_empty() && self.got < self.out.len() {
            if self.body == 0 {
                let n = (RECORD_HEAD_LEN - self.at_head).min(bytes.len());
                self.head[self.at_head..][..n].copy_from_slice(&bytes[..n]);
                (self.at_head, bytes) = (self.at_head + n, &bytes[n..]);
                if self.at_head < RECORD_HEAD_LEN {
                    break;
                }
                let tag = u16::from_le_bytes([self.head[0], self.head[1]]);
                let len = u16::from_le_bytes([self.head[2], self.head[3]]) as usize;
                let want = (self.left as usize).min(SOURCE_DATA_MAX);
                if tag != RecordTag::SourceData.code() || len != want || len == 0 {
                    self.bad = true;
                    return ControlFlow::Break(());
                }
                (self.at_head, self.body) = (0, len);
                self.left -= len as u32;
                continue;
            }
            let n = self.body.min(bytes.len());
            let drop = self.skip.min(n);
            let take = (n - drop).min(self.out.len() - self.got);
            self.out[self.got..][..take].copy_from_slice(&bytes[drop..drop + take]);
            self.got += take;
            (self.skip, self.body, bytes) = (self.skip - drop, self.body - n, &bytes[n..]);
        }
        if self.got == self.out.len() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// `out.len()` packed bytes of whole `Plain` source `r` from packed byte
/// `from`, read from the `SourceData` record holding it: a head, or a
/// UI pass's share of one. Framing a record's tag and length don't match
/// fails it (`Corrupt`), as does a file that ends first (`Truncated`).
pub fn read_packed_into<S: Store + ?Sized>(
    s: &mut S,
    r: &Ready,
    sref: SourceRef,
    from: Bytes,
    out: &mut [u8],
) -> Result<(), SourceReadError> {
    let from = from.get();
    let bad = |e| SourceReadError::Load(LoadError::File(e));
    let (off, len) = plain_data(sref).ok_or(bad(FileError::Bounds))?;
    let end = (from as u64) + out.len() as u64;
    if end > u64::from(len) {
        return Err(bad(FileError::Bounds));
    }
    let k = from / SOURCE_DATA_MAX as u32;
    let mut sink = HeadSink {
        out,
        got: 0,
        skip: (from % SOURCE_DATA_MAX as u32) as usize,
        left: len - k * SOURCE_DATA_MAX as u32,
        head: [0; RECORD_HEAD_LEN],
        at_head: 0,
        body: 0,
        bad: false,
    };
    let at = off + k * (RECORD_HEAD_LEN + SOURCE_DATA_MAX) as u32;
    s.read_at(r.volume(), source_file(sref), at, &mut sink)
        .map_err(|e| match e {
            StoreError::NotFound => SourceReadError::Load(LoadError::Missing),
            e => e.into(),
        })?;
    match (sink.bad, sink.got == sink.out.len()) {
        (true, _) => Err(bad(FileError::Corrupt)),
        (false, false) => Err(bad(FileError::Truncated)),
        (false, true) => Ok(()),
    }
}

/// How much of a card source `read_source_head` checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Up to its head, in its first 4 KB.
    Head,
    /// Every byte, its samples against its CRC too: an unsealed source's
    /// check before its seal.
    Whole,
}

/// Feeds a `SourceCheck` until its head came (on `Reach::Head`), or
/// `left` bytes went.
struct HeadOnly<'a> {
    check: &'a mut SourceCheck,
    left: u32,
    reach: Reach,
    refused: bool,
}

impl ReadSink for HeadOnly<'_> {
    fn begin(&mut self, _len: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let n = bytes.len().min(self.left as usize);
        self.refused = self.check.feed(&bytes[..n]).is_err();
        self.left -= n as u32;
        if self.refused
            || self.left == 0
            || (self.reach == Reach::Head && self.check.head().is_some())
        {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// The head of card file `stem`, `len` bytes long, from its first records:
/// a streamed load's descriptor, read without the samples, or on
/// `Reach::Whole` with every sample checked against the stem.
pub fn read_source_head<S: Store + ?Sized>(
    s: &mut S,
    r: &Ready,
    stem: Stem,
    len: u32,
    reach: Reach,
) -> Result<SourceHead, SourceReadError> {
    let bad = SourceReadError::Load(LoadError::File(FileError::Corrupt));
    let mut check = SourceCheck::new(stem, len).map_err(|_| bad)?;
    let mut sink = HeadOnly {
        check: &mut check,
        left: if reach == Reach::Head { 4_096 } else { len },
        reach,
        refused: false,
    };
    s.read_at(r.volume(), source_name(stem), 0, &mut sink)
        .map_err(|e| match e {
            StoreError::NotFound => SourceReadError::Load(LoadError::Missing),
            e => e.into(),
        })?;
    match reach {
        Reach::Head => check.head().ok_or(bad),
        Reach::Whole => check.finish().map_err(|_| bad),
    }
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

impl PutRefusal {
    /// `ERR put: <message>`.
    pub fn message(self) -> &'static str {
        match self {
            PutRefusal::Kind => FileError::WrongKind.message(),
            PutRefusal::Frames => "frames aren't its head's",
            PutRefusal::Crc => "CRC isn't its name",
            PutRefusal::File(e) => e.message(),
        }
    }
}

/// Judges a source file as `put` streams it, before anything is kept: it
/// must be a `Source`, named by the stem it goes under, its samples its
/// head's frames and hashing to that stem. Each refusal comes from the
/// `feed` whose bytes show it, and sticks.
pub struct SourceCheck {
    framer: Framer,
    rec: Records,
    /// Bytes fed, of `len`.
    fed: u64,
    len: u32,
    refused: Option<PutRefusal>,
}

impl SourceCheck {
    /// A file of `len` bytes, put as `<stem>.SRC`. `len` is the one
    /// `put` announces: the framer needs it to find the trailer.
    pub fn new(stem: Stem, len: u32) -> Result<SourceCheck, PutRefusal> {
        Ok(SourceCheck {
            framer: Framer::new(len).map_err(PutRefusal::File)?,
            rec: Records::new(Expect::Stem(stem)),
            fed: 0,
            len,
            refused: None,
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), PutRefusal> {
        self.feed_into(bytes, None)
    }

    /// `feed`, the samples written into `held` as `read_source_into` lays
    /// them out; the descriptor too, with the last byte. `held` must be the
    /// same each call, as long as the layout's `held_len`. `held` is
    /// trusted only once every byte came and passed.
    pub fn feed_into(
        &mut self,
        bytes: &[u8],
        mut held: Option<&mut [u8]>,
    ) -> Result<(), PutRefusal> {
        if let Some(r) = self.refused {
            return Err(r);
        }
        let mut said = None;
        let rec = &mut self.rec;
        let pushed = self.framer.push(bytes, &mut |e| {
            let r = match e {
                Event::Header(h) if h.kind != FileKind::Source => Err(PutRefusal::Kind),
                Event::Header(_) | Event::Record(ReadTag::Unknown(_), _) => Ok(()),
                Event::Record(ReadTag::Known(t), p) => {
                    rec.record(t, p, held.as_deref_mut()).map_err(Bad::put)
                }
            };
            // The framer stops the events at an error; `said` keeps ours.
            r.map_err(|e| *said.insert(e))
                .map_err(|_| FileError::Corrupt)
        });
        self.fed += bytes.len() as u64;
        let r = match (said, pushed) {
            (Some(r), _) => Err(r),
            (None, Err(e)) => Err(PutRefusal::File(e)),
            (None, Ok(())) if self.fed == self.len as u64 => self.verdict(held).map(|_| ()),
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
        self.verdict(None)
    }

    /// The head, once its record came: a card row's name, read from the
    /// file's first 4 KB.
    pub fn head(&self) -> Option<SourceHead> {
        self.rec.head()
    }

    /// The bytes `held` takes, once the head (and a `ZoneSet`'s zones)
    /// came.
    pub fn held_len(&self) -> Option<usize> {
        match &self.rec.step {
            Step::Data(l) => Some(l.held_len()),
            _ => None,
        }
    }

    fn verdict(&mut self, held: Option<&mut [u8]>) -> Result<SourceHead, PutRefusal> {
        self.framer.finish().map_err(PutRefusal::File)?;
        self.rec.end(held).map(|l| l.head).map_err(Bad::put)
    }
}

/// What a source write found and did: `source_target` before it,
/// `write_source` after.
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
/// checks the head and the CRC, then seals it (ADR 0087). Host and `put`
/// only: `source-writer`.
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

/// Before a source is written as `<stem>.SRC`, the file already
/// there: `Present` if it verifies (sealed now if it wasn't: nothing to
/// write); `Replaced` for an unsealed one that fails, the torn remains of
/// a cut write, deleted; `New` for none. `Damaged` for a sealed file that
/// fails: never rewritten. `TAPES` must exist.
#[cfg(feature = "source-writer")]
pub fn source_target<S: Store>(
    s: &mut S,
    r: &Ready,
    stem: Stem,
) -> Result<Wrote, super::SaveError> {
    target(s, r, Expect::Stem(stem))
}

/// `source_target` for the file `e` names; `write_source` asks for its
/// whole ref, so another rate under the same stem is no match.
#[cfg(feature = "source-writer")]
fn target<S: Store>(s: &mut S, r: &Ready, e: Expect) -> Result<Wrote, super::SaveError> {
    use super::SaveError;
    let (vol, file) = (r.volume(), e.file());
    match read_back(s, r, e) {
        Ok(_) => {
            s.seal(vol, file)?;
            Ok(Wrote::Present)
        }
        Err(LoadError::Missing) => Ok(Wrote::New),
        Err(LoadError::Store(e)) => Err(e.into()),
        // A load's refusal: `read_source` never says it.
        Err(LoadError::Busy) => Err(StoreError::Io.into()),
        // Only a sealed file refuses the delete: that one is never
        // rewritten.
        Err(LoadError::File(_)) => match s.delete(vol, file) {
            Ok(()) => Ok(Wrote::Replaced),
            Err(StoreError::ReadOnly) => Err(SaveError::Damaged),
            Err(e) => Err(e.into()),
        },
    }
}

/// After a write of `<stem>.SRC`: read back whole, its head naming
/// the stem, its samples hashing to it, then sealed. Unsealed on any
/// `Err`, so the next write replaces it.
#[cfg(feature = "source-writer")]
pub fn verify_and_seal<S: Store>(s: &mut S, r: &Ready, stem: Stem) -> Result<(), super::SaveError> {
    verify(s, r, Expect::Stem(stem))?;
    s.seal(r.volume(), source_name(stem))?;
    Ok(())
}

#[cfg(feature = "source-writer")]
fn verify<S: Store>(s: &mut S, r: &Ready, e: Expect) -> Result<SourceLayout, super::SaveError> {
    use super::SaveError;
    read_back(s, r, e).map_err(|e| match e {
        LoadError::Store(e) => e.into(),
        LoadError::File(e) => SaveError::File(e),
        LoadError::Missing => StoreError::NotFound.into(),
        // A load's refusal: `read_source` never says it.
        LoadError::Busy => StoreError::Io.into(),
    })
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
    let wrote = target(s, r, Expect::Ref(sref))?;
    if wrote == Wrote::Present {
        return Ok(wrote);
    }
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
    // Unsealed until it reads back whole and as written: a failure here is
    // replaced by the next write.
    if verify(s, r, Expect::Ref(sref))? != *l {
        return Err(SaveError::File(FileError::Corrupt));
    }
    s.seal(vol, file)?;
    Ok(wrote)
}
