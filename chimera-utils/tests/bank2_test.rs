//! bank2-seal and door-check on fake images and object-file fixtures.

#[allow(dead_code)]
#[path = "../src/bank2.rs"]
mod bank2;

use chimera_image::{Header, Seal, UNSEALED_HEADER, Verdict, judge};
use object::write::{Object, Relocation, Symbol, SymbolSection};
use object::{
    Architecture, BinaryFormat, Endianness, RelocationFlags, SectionKind, SymbolFlags, SymbolKind,
    SymbolScope, elf,
};
use sha2::{Digest, Sha256};

struct Img(Vec<u8>);
impl chimera_image::ImageRead for Img {
    fn read(&mut self, off: u32, buf: &mut [u8]) -> Result<(), chimera_image::ReadFail> {
        let o = off as usize;
        buf.copy_from_slice(&self.0[o..o + buf.len()]);
        Ok(())
    }
}

fn fake() -> (Vec<u8>, Vec<u8>) {
    let mut bin = vec![0xAAu8; 256];
    bin[100..108].copy_from_slice(b"UNSEALED");
    let mut bk2 = UNSEALED_HEADER.to_vec();
    bk2.extend((0..1024).map(|i| (i * 7) as u8));
    (bin, bk2)
}

#[test]
fn seal_round_trip() {
    let (mut bin, mut bk2) = fake();
    let seal = bank2::seal_images(&mut bin, 100, &mut bk2, 256, "0.1.0 75222a81").unwrap();
    let hb: [u8; 256] = bk2[..256].try_into().unwrap();
    assert!(Header::parse(&hb).is_ok());
    let want = Seal::new(bin[100..108].try_into().unwrap()).unwrap();
    assert_eq!(want, seal);
    assert!(matches!(
        judge(&mut Img(bk2.clone()), &want),
        Verdict::Good(_)
    ));
    assert_eq!(bin[100..108], Sha256::digest(&bk2[256..])[..8]);
}

#[test]
fn seal_refuses_missing_placeholder() {
    let (mut bin, mut bk2) = fake();
    assert!(bank2::seal_images(&mut bin, 0, &mut bk2, 256, "v").is_err());
    let before = bk2.clone();
    bank2::seal_images(&mut bin, 100, &mut bk2, 256, "v").unwrap();
    // Sealed twice: the placeholder is gone.
    assert!(bank2::seal_images(&mut bin, 100, &mut bk2, 256, "v").is_err());
    assert_ne!(before, bk2);
}

#[test]
fn seal_refuses_bad_inputs() {
    let (mut bin, mut bk2) = fake();
    assert!(bank2::seal_images(&mut bin, 100, &mut bk2, 256, &"x".repeat(33)).is_err());
    assert!(bank2::seal_images(&mut bin, 100, &mut bk2, 4, "v").is_err());
    bk2[0] = 0;
    assert!(bank2::seal_images(&mut bin, 100, &mut bk2, 256, "v").is_err());
    assert_eq!(&bin[100..108], b"UNSEALED");
}

/// A `.text` with one ABS32 relocation to a symbol in `into`.
fn elf_with(into: &[u8], relocs: bool) -> Vec<u8> {
    let mut o = Object::new(BinaryFormat::Elf, Architecture::Arm, Endianness::Little);
    let text = o.add_section(vec![], b".text".to_vec(), SectionKind::Text);
    o.append_section_data(text, &[0; 8], 4);
    let b2 = o.add_section(vec![], b".bank2".to_vec(), SectionKind::ReadOnlyData);
    o.append_section_data(b2, &[0; 8], 4);
    let host = if into == b".bank2" { b2 } else { text };
    let sym = o.add_symbol(Symbol {
        name: b"door".to_vec(),
        value: 0,
        size: 0,
        kind: SymbolKind::Data,
        scope: SymbolScope::Linkage,
        weak: false,
        section: SymbolSection::Section(host),
        flags: SymbolFlags::None,
    });
    if relocs {
        o.add_relocation(
            text,
            Relocation {
                offset: 0,
                symbol: sym,
                addend: 0,
                flags: RelocationFlags::Elf {
                    r_type: elf::R_ARM_ABS32,
                },
            },
        )
        .unwrap();
    }
    o.write().unwrap()
}

#[test]
fn door_check_flags_forbidden_relocation() {
    let hits = bank2::door_hits(&elf_with(b".bank2", true)).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(
        bank2::door_hits(&elf_with(b".text", true))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn door_check_refuses_no_relocs() {
    assert!(bank2::door_hits(&elf_with(b".bank2", false)).is_err());
}
