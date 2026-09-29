//! Sound-file helpers shared by the codec compat and fuzz tests and the
//! `cargo fuzz` target (which includes this file by path): two-pass decode,
//! CRC repair and the bounds every decoded Sound must keep.
#![allow(dead_code)]

use chimera_core::addr::{BlockRef, Blocks};
use chimera_core::block::ParamKind;
use chimera_core::mod_path::MAX_REGISTRY_DESTS;
use chimera_core::modulation::{MAX_MOD_DESTS, MAX_MOD_SOURCES, amount_scale};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    Crc32, Decode, FileError, FileKind, Framer, Generation, Header, SoundDecoder, encode_sound,
    write_file,
};
use chimera_hal::store::{ByteSink, StoreError};

pub struct VecSink(pub Vec<u8>);

impl ByteSink for VecSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }
}

/// `s` as a Sound file at `Generation::FIRST`.
pub fn encode(s: &Sound) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| encode_sound(s, w)).unwrap();
    sink.0
}

/// One pass onto `target`; `apply` false only checks.
pub fn pass(target: &mut Sound, bytes: &[u8], apply: bool) -> Result<(), FileError> {
    let mut d = SoundDecoder::new(target);
    let mut f = Framer::new(bytes.len() as u32)?;
    f.push(bytes, &mut |e| d.event(e, apply))?;
    f.finish()?;
    d.end(apply)
}

/// Two passes onto `target`: check, then apply once the first is Ok.
pub fn decode_into(target: &mut Sound, bytes: &[u8]) -> Result<(), FileError> {
    pass(target, bytes, false)?;
    pass(target, bytes, true)
}

/// A fresh Algo-based decode, as the fixtures and the fuzzers use.
pub fn decode(bytes: &[u8]) -> Result<Sound, FileError> {
    let mut s = Sound::neutral(EngineType::Algo);
    decode_into(&mut s, bytes)?;
    Ok(s)
}

/// Rewrites the CRC trailer to match the body.
pub fn fix_crc(f: &mut [u8]) {
    if let Some(n) = f.len().checked_sub(4) {
        let mut c = Crc32::new();
        c.update(&f[..n]);
        f[n..].copy_from_slice(&c.finish().to_le_bytes());
    }
}

/// Start of each record, then the end of the last (where the trailer begins).
pub fn record_offsets(f: &[u8]) -> Vec<usize> {
    let end = f.len() - 4;
    let mut v = vec![];
    let mut p = 28;
    while p + 4 <= end {
        v.push(p);
        p += 4 + usize::from(u16::from_le_bytes([f[p + 2], f[p + 3]]));
    }
    v.push(p.min(end));
    v
}

/// Decodes `bytes` into `neutral(Algo)`: never a panic; a pass-1 Ok means
/// pass 2 is Ok; any Err leaves the target `neutral`; and on Ok every
/// voice-block value is finite and in its spec's range, every stored enum has
/// a frozen code, the name is a valid `Name`, and the matrix and registry are
/// within their capacities and made of live addresses.
pub fn check_decode(bytes: &[u8]) {
    let neutral = Sound::neutral(EngineType::Algo);
    let mut s = Sound::neutral(EngineType::Algo);
    let first = pass(&mut s, bytes, false);
    let second = first.and_then(|()| pass(&mut s, bytes, true));
    if first.is_ok() {
        assert!(second.is_ok(), "pass 1 Ok, pass 2 {second:?}");
    }
    if second.is_err() {
        assert!(s.bits_eq(&neutral), "an Err touched the target");
        return;
    }
    for b in BlockRef::ALL {
        let Some(blk) = s.params.block(b) else {
            continue;
        };
        for sp in blk.specs() {
            let v = blk.get(sp.id);
            assert!(
                v.is_finite() && (sp.min..=sp.max).contains(&v),
                "{b:?} {} = {v} outside {}..={}",
                sp.ident,
                sp.min,
                sp.max
            );
            // A live view (ENV's FORM) has no code of its own: nothing stores it.
            if sp.kind == ParamKind::Enum && sp.stored {
                assert!(blk.enum_code(sp.id).is_some(), "{b:?} {}", sp.ident);
            }
        }
    }
    assert!(SoundName::from_padded(&s.name.padded()).is_ok());
    let m = &s.mod_state;
    assert!(m.num_dests() <= MAX_MOD_DESTS);
    assert!(m.num_sources() <= MAX_MOD_SOURCES);
    for d in 0..m.num_dests() {
        assert!(m.dest(d).modulatable(), "dest {d}");
        assert_eq!(u32::from(m.present(d)) >> m.num_sources(), 0, "dest {d}");
        for src in 0..m.num_sources() {
            let scale = amount_scale(m.amount(src, d));
            assert!((-1.0..=1.0).contains(&scale), "amount {src}->{d}");
        }
    }
    assert!(s.dest_registry.len() <= MAX_REGISTRY_DESTS);
    for i in 0..s.dest_registry.len() {
        let e = s.dest_registry.get(i).expect("a counted entry");
        assert!(e.addr.modulatable(), "registry {i}");
    }
}
