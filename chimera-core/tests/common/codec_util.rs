//! Sound-file helpers shared by the codec compat and fuzz tests and the
//! `cargo fuzz` target (which includes this file by path): two-pass decode,
//! CRC repair and the bounds every decoded Sound must keep.
#![allow(dead_code)]

use chimera_core::addr::{BlockRef, Blocks};
use chimera_core::block::ParamKind;
use chimera_core::mod_path::MAX_REGISTRY_DESTS;
use chimera_core::modulation::MAX_MOD_DESTS;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::storage::{
    Crc32, FileError, FileKind, Framer, Generation, Header, SoundDecoder, encode_sound, write_file,
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

/// Two passes onto `target`: check, then apply once the first `finish` is Ok.
pub fn decode_into(target: &mut Sound, bytes: &[u8]) -> Result<(), FileError> {
    for apply in [false, true] {
        let mut d = SoundDecoder::new(target);
        let mut f = Framer::new(bytes.len() as u32)?;
        f.push(bytes, &mut |e| d.event(e, apply))?;
        f.finish()?;
        d.end(apply)?;
    }
    Ok(())
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

/// Decodes `bytes` into `neutral(Algo)`: never a panic, and on Ok every
/// voice-block value is finite and in its spec's range, every enum has a
/// frozen code, and the matrix and registry are within their capacities.
pub fn check_decode(bytes: &[u8]) {
    let Ok(s) = decode(bytes) else { return };
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
    assert!(s.mod_state.num_dests() <= MAX_MOD_DESTS);
    assert!(s.dest_registry.len() <= MAX_REGISTRY_DESTS);
}
