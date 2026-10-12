//! The `Block` record: block code `u8`, then (`ParamId` `u8`, value 4 B LE)*.
//! A value is an `f32` in spec units, or an enum's frozen code as a `u32`.

use crate::addr::{BlockRef, BlockViewMut, Blocks};
use crate::block::{Block, ParamId};
use crate::dsp::algo::algorithms::AlgoId;
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};

use super::codes::{
    DiskValue, Migration, RETIRED, RETIRED_BLOCKS, RETIRED_CODES, Retired, Translation, ValidAddr,
    read_value,
};
use super::frame::FileError;
use super::record::RecordBuf;

pub(crate) const ENTRY_LEN: usize = 5;

/// The longest spec table a decoded block may have (ENV's is 19).
pub(crate) const MAX_BLOCK_PARAMS: usize = 32;

/// `b`'s code, then every stored param in canonical (spec) order, into an
/// empty `out`. With a `base`, only the params whose disk value differs from
/// its (a real by bits): a sparse block, decoded onto that base. Returns the
/// params written.
///
/// # Panics
/// For a block with no disk code: an encoder bug.
pub fn encode_block(
    b: BlockRef,
    blk: &dyn Block,
    base: Option<&dyn Block>,
    out: &mut RecordBuf,
) -> usize {
    out.u8(b.disk_code().expect("a stored block"));
    let mut n = 0;
    for a in ValidAddr::of_block(b) {
        let v = read_value(blk, a);
        if base.is_some_and(|base| same(read_value(base, a), v)) {
            continue;
        }
        out.u8(a.spec().id.0);
        match v {
            DiskValue::Real(v) => out.f32(v),
            DiskValue::Code(c) => out.u32(c.into()),
        }
        n += 1;
    }
    n
}

/// The same bytes on disk.
fn same(a: DiskValue, b: DiskValue) -> bool {
    match (a, b) {
        (DiskValue::Real(x), DiskValue::Real(y)) => x.to_bits() == y.to_bits(),
        (DiskValue::Code(x), DiskValue::Code(y)) => x == y,
        _ => false,
    }
}

/// Checks a `Block` payload and, with a `target`, applies it: every value is
/// collected first, then written in spec order, so a param decoded through
/// another (MODE through KIND) sees it whatever the file's order.
///
/// `Bounds`: not 1 + 5n bytes. `Corrupt`: an id twice. An unknown block
/// code, or one `target` doesn't hold, is skipped; so is an unknown param id,
/// unless a migration maps it and the file lacks the new id. A finite
/// retired value is kept for the block's translation, run after the writes.
/// A non-finite value or an unknown code keeps the target's value; a real is
/// clamped and quantised by its spec.
///
/// Returns the entries lost, counted only with a `target`: an unknown block
/// code, an unknown id, or an enum code refused, none of them retired or
/// migrated. A block the target doesn't hold counts nothing. An operator
/// kind, or an algorithm linking into one, that the formant budget refuses
/// isn't lost: it keeps the target's, and `refused_kind` says so. Sounds
/// write ALG A and B before their operators (`BlockRef::ALL`), so a FRMT
/// the file's algorithm can't hold degrades to the base's table kind.
pub fn decode_block(
    payload: &[u8],
    migrations: &[Migration],
    translations: &[Translation],
    target: Option<&mut dyn Blocks>,
) -> Result<Applied, FileError> {
    let (&code, body) = payload.split_first().ok_or(FileError::Bounds)?;
    let (entries, []) = body.as_chunks::<ENTRY_LEN>() else {
        return Err(FileError::Bounds);
    };
    let entries = entries
        .iter()
        .map(|&[id, a, b, c, d]| (ParamId(id), [a, b, c, d]));
    let mut seen = ByteSet::new();
    for (id, _) in entries.clone() {
        if !seen.insert(id.0) {
            return Err(FileError::Corrupt);
        }
    }

    let mut out = Applied::default();
    let Some(target) = target else {
        return Ok(out);
    };
    let Some(b) = BlockRef::from_disk_code(code) else {
        out.count = u16::from(!RETIRED_BLOCKS.contains(&code));
        return Ok(out);
    };
    let Some(mut blk) = target.block_mut(b) else {
        return Ok(out);
    };
    let lost = &mut out.count;
    let specs = b.specs();
    debug_assert!(specs.len() <= MAX_BLOCK_PARAMS);
    let slot = |id: ParamId| specs.iter().position(|s| s.id == id);

    let mut vals = [None; MAX_BLOCK_PARAMS];
    let mut retired = Retired::new(seen);
    for (id, raw) in entries {
        let (a, v) = match ValidAddr::find(b, id) {
            Some(a) => (a, value(a, raw)),
            None => {
                let old = f32::from_le_bytes(raw);
                if old.is_finite() && RETIRED.contains(&(code, id.0)) {
                    retired.put(id, old);
                }
                let Some(m) = migrations
                    .iter()
                    .find(|m| m.block == code && m.old == id && !seen.contains(m.new.0))
                else {
                    if !RETIRED.contains(&(code, id.0))
                        && !migrations.iter().any(|m| m.block == code && m.old == id)
                    {
                        *lost += 1;
                    }
                    continue;
                };
                let Some(a) = ValidAddr::find(b, m.new) else {
                    continue;
                };
                let v = (m.map)(old);
                (a, v.is_finite().then_some(DiskValue::Real(v)))
            }
        };
        if let Some(i) = slot(a.spec().id) {
            vals[i] = v;
        }
    }

    for (s, v) in specs.iter().zip(vals) {
        match v {
            Some(DiskValue::Real(x)) => blk.set(s.id, x),
            // false: an unknown code, or one this KIND lacks; keep the base.
            // A retired code goes to the translation, any other is lost.
            Some(DiskValue::Code(c)) if !blk.set_enum_code(s.id, c) => {
                if RETIRED_CODES.contains(&(code, s.id.0, c)) {
                    retired.put_code(s.id, c);
                } else if (matches!(blk, BlockViewMut::Op(_))
                    && s.id == AlgoOpParams::WAVE
                    && OpKind::from_code(c).is_some())
                    || (b == BlockRef::Algo
                        && matches!(s.id, AlgoParams::ALG_A | AlgoParams::ALG_B)
                        && AlgoId::from_index(c).is_some())
                {
                    out.refused_kind = true;
                } else {
                    *lost += 1;
                }
            }
            Some(DiskValue::Code(_)) => {}
            None => {}
        }
    }
    if retired.any()
        && let Some(t) = translations.iter().find(|t| t.block == code)
    {
        (t.apply)(&retired, &mut *blk);
    }
    Ok(out)
}

/// What `decode_block` did to its target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Entries lost.
    pub count: u16,
    /// An operator kind the formant budget refused (`Kinds`).
    pub refused_kind: bool,
}

/// A set of `u8`s: ids or codes a record may name once.
#[derive(Clone, Copy)]
pub(super) struct ByteSet([u32; 8]);

impl ByteSet {
    pub(super) const fn new() -> Self {
        ByteSet([0; 8])
    }

    pub(super) fn contains(&self, b: u8) -> bool {
        self.0[usize::from(b / 32)] & 1 << (b % 32) != 0
    }

    /// `false`: already in.
    pub(super) fn insert(&mut self, b: u8) -> bool {
        let fresh = !self.contains(b);
        self.0[usize::from(b / 32)] |= 1 << (b % 32);
        fresh
    }
}

/// `raw` as `a`'s value; `None` when it can't be one (NaN, a code past `u8`).
fn value(a: ValidAddr, raw: [u8; 4]) -> Option<DiskValue> {
    if a.coded() {
        u8::try_from(u32::from_le_bytes(raw))
            .ok()
            .map(DiskValue::Code)
    } else {
        let v = f32::from_le_bytes(raw);
        v.is_finite().then_some(DiskValue::Real(v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{ParamKind, ParamSpec};
    use crate::params::{FILTER_SPECS, FilterParams};
    use crate::storage::{MIGRATIONS, TRANSLATIONS};

    /// A Filter that logs the order of its enum writes.
    struct Order {
        log: [ParamId; 4],
        n: usize,
    }

    impl Block for Order {
        fn specs(&self) -> &'static [ParamSpec] {
            &FILTER_SPECS
        }
        fn get(&self, _: ParamId) -> f32 {
            0.0
        }
        fn write(&mut self, _: ParamId, _: f32) {}
        fn set_enum_code(&mut self, id: ParamId, _: u8) -> bool {
            self.log[self.n] = id;
            self.n += 1;
            true
        }
    }

    impl crate::addr::BlockRead for Order {
        fn block(&self, b: BlockRef) -> Option<crate::addr::BlockView<'_>> {
            (b == BlockRef::Filter).then_some(crate::addr::BlockView::Plain(self))
        }
    }

    impl Blocks for Order {
        fn block_mut(&mut self, b: BlockRef) -> Option<BlockViewMut<'_>> {
            (b == BlockRef::Filter).then_some(BlockViewMut::Plain(self))
        }
    }

    #[test]
    fn kind_is_written_before_mode_whatever_the_file_order() {
        let (kind, mode) = (FilterParams::KIND.0, FilterParams::MODE.0);
        let payload = [10, mode, 3, 0, 0, 0, kind, 0, 0, 0, 0];
        let mut o = Order {
            log: [ParamId(0); 4],
            n: 0,
        };
        decode_block(&payload, &[], &[], Some(&mut o)).unwrap();
        assert_eq!(o.log[..o.n], [FilterParams::KIND, FilterParams::MODE]);
    }

    #[test]
    fn every_block_fits_the_value_slots() {
        for b in BlockRef::ALL {
            assert!(b.specs().len() <= MAX_BLOCK_PARAMS, "{b:?}");
        }
    }

    /// A migration maps an old real into a live, stored, non-enum param.
    #[test]
    fn migrations_target_stored_reals() {
        for m in MIGRATIONS {
            let b = BlockRef::from_disk_code(m.block).expect("a live block");
            assert!(ValidAddr::find(b, m.old).is_none(), "old id still live");
            let a = ValidAddr::find(b, m.new).expect("a stored new id");
            assert_ne!(a.spec().kind, ParamKind::Enum);
        }
    }

    /// A translation reads a live block, and a retired id of it.
    #[test]
    fn translations_target_live_blocks() {
        for t in TRANSLATIONS {
            assert!(BlockRef::from_disk_code(t.block).is_some(), "{}", t.block);
            assert!(
                RETIRED.iter().any(|r| r.0 == t.block)
                    || RETIRED_CODES.iter().any(|r| r.0 == t.block),
                "{}",
                t.block
            );
        }
    }
}
