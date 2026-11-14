//! Bank 2's seal and door check (ADR 0137): the build's two halves are tied
//! by one 8-byte seal, and nothing in bank 1 may reach bank 2 but the door.

use chimera_image::crc::Crc32;
use chimera_image::slot::{RESERVE, code_word, end_marker, tag_word};
use chimera_image::{BANK2_BASE, BANK2_LEN, HEADER_LEN, Header, MAGIC, Seal, Slot, Version};
use object::elf::{SHF_ALLOC, SHT_REL, SHT_RELA};
use object::read::elf::{ElfFile32, SectionHeader};
use object::{Endianness, Object, ObjectSection, ObjectSymbol, RelocationTarget, SymbolSection};
use sha2::{Digest, Sha256};

/// Fill `bk2`'s header from its body and patch the same seal into `bin`.
/// Nothing is written unless every check passes.
pub fn seal_images(
    bin: &mut [u8],
    seal_off: usize,
    bk2: &mut [u8],
    door_off: u32,
    version: &str,
) -> Result<Seal, String> {
    if version.is_empty()
        || version != version.trim()
        || version.len() > 32
        || !version.bytes().all(|c| (0x20..0x7F).contains(&c))
    {
        return Err(format!(
            "version `{version}` is not 32 printable ASCII bytes"
        ));
    }
    let slot = bin
        .get(seal_off..seal_off + 8)
        .ok_or("BANK2_SEAL is outside the .bin")?;
    if slot != Seal::UNSET {
        return Err("BANK2_SEAL does not read UNSEALED (sealed twice, or a wrong offset)".into());
    }
    if bk2.len() < HEADER_LEN as usize || bk2[..8] != MAGIC {
        return Err("the .bk2 does not start with the CHIMERA2 header".into());
    }
    let body = &bk2[HEADER_LEN as usize..];
    let body_len = u32::try_from(body.len()).map_err(|_| "body too long")?;
    let mut crc = Crc32::new();
    crc.update(body);
    let digest = Sha256::digest(body);
    let mut b = [0u8; 8];
    b.copy_from_slice(&digest[..8]);
    let seal = Seal::new(b).ok_or("the body's seal is a reserved value")?;
    let h = Header {
        body_len,
        body_crc: crc.finish(),
        door_off,
        seal: b,
        version: Version::new(version),
    };
    let hb = h.to_bytes(true);
    Header::parse(&hb).map_err(|v| format!("header would judge {v:?}"))?;
    bk2[..HEADER_LEN as usize].copy_from_slice(&hb);
    bin[seal_off..seal_off + 8].copy_from_slice(&b);
    Ok(seal)
}

/// The `.bk2`'s slot words: its exact size, each slot's stub, tag and end
/// marker at their flash words, and 0xFF in the reserve after each tag.
pub fn check_slots(bk2: &[u8]) -> Result<(), String> {
    if bk2.len() != BANK2_LEN as usize {
        return Err(format!("the .bk2 is {} B, not {BANK2_LEN}", bk2.len()));
    }
    let at = |addr: u32| (addr - BANK2_BASE) as usize;
    for s in Slot::ALL {
        let words = [
            (s.base(), code_word(s), "stub"),
            (s.tag_addr(), tag_word(s), "tag"),
            (s.marker_addr(), end_marker(s), "end marker"),
        ];
        for (addr, want, what) in words {
            if bk2[at(addr)..at(addr) + 32] != want {
                return Err(format!("slot {s:?} {what} at {addr:#010x} is wrong"));
            }
        }
        let from = s.tag_addr() + 32;
        let r = at(from)..at(from) + RESERVE as usize - 32;
        if let Some(i) = bk2[r.clone()].iter().position(|&b| b != 0xFF) {
            return Err(format!(
                "slot {s:?} reserve at {:#010x} is not 0xFF",
                from + i as u32
            ));
        }
    }
    Ok(())
}

/// `chimera-utils check-slots BK2`.
pub fn check_slots_file(bk2: &str) -> Result<(), String> {
    let b = std::fs::read(bk2).map_err(|e| format!("{bk2}: {e}"))?;
    check_slots(&b).map_err(|e| format!("{bk2}: {e}"))?;
    println!("{bk2}: slots ok");
    Ok(())
}

/// The `.bk2`'s header seal, found once in the `.bin`: the two are one build.
pub fn check_pair(bin: &[u8], bk2: &[u8]) -> Result<Seal, String> {
    let hb: &[u8; HEADER_LEN as usize] = bk2
        .get(..HEADER_LEN as usize)
        .and_then(|h| h.try_into().ok())
        .ok_or("the .bk2 is shorter than its header")?;
    let h = Header::parse(hb).map_err(|v| format!("the .bk2 header judges {v:?}"))?;
    let seal = Seal::new(h.seal).ok_or("the .bk2 seal is a reserved value")?;
    check_slots(bk2)?;
    let body = &bk2[HEADER_LEN as usize..];
    if body.len() != h.body_len as usize {
        return Err(format!(
            "the .bk2 body is {} B, its header says {}",
            body.len(),
            h.body_len
        ));
    }
    let mut crc = Crc32::new();
    crc.update(body);
    if crc.finish() != h.body_crc {
        return Err("the .bk2 body fails its CRC".into());
    }
    if Sha256::digest(body)[..8] != h.seal {
        return Err("the .bk2 seal is not its body's SHA-256 prefix".into());
    }
    match bin.windows(8).filter(|w| *w == h.seal).count() {
        1 => Ok(seal),
        n => Err(format!(
            "the .bk2's seal is in the .bin {n} times, not once"
        )),
    }
}

/// `chimera-utils bank2-seal --check BIN BK2`.
pub fn check_files(bin: &str, bk2: &str) -> Result<(), String> {
    let b1 = std::fs::read(bin).map_err(|e| format!("{bin}: {e}"))?;
    let b2 = std::fs::read(bk2).map_err(|e| format!("{bk2}: {e}"))?;
    let seal = check_pair(&b1, &b2).map_err(|e| format!("{bin} + {bk2}: {e}"))?;
    let hex: String = seal.bytes().iter().map(|x| format!("{x:02x}")).collect();
    println!("{bk2}: seal {hex} is {bin}'s");
    Ok(())
}

fn sym_addr(f: &ElfFile32<Endianness>, name: &str) -> Result<u64, String> {
    f.symbol_by_name(name)
        .map(|s| s.address())
        .ok_or_else(|| format!("symbol {name} missing from the ELF"))
}

/// `chimera-utils bank2-seal ELF BIN BK2 --version TEXT`.
pub fn seal_files(elf: &str, bin: &str, bk2: &str, version: &str) -> Result<(), String> {
    let e = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let f = ElfFile32::<Endianness>::parse(&*e).map_err(|e| format!("{elf}: {e}"))?;
    let base = f
        .section_by_name(".vector_table")
        .ok_or(".vector_table missing from the ELF")?
        .address();
    let seal_off = sym_addr(&f, "BANK2_SEAL")?
        .checked_sub(base)
        .ok_or("BANK2_SEAL is below the vector table")? as usize;
    let door = sym_addr(&f, "__bank2_door")?
        .checked_sub(BANK2_BASE as u64)
        .ok_or("__bank2_door is below bank 2")?;
    let door_off = u32::try_from(door).map_err(|_| "__bank2_door is out of range")?;
    let mut b1 = std::fs::read(bin).map_err(|e| format!("{bin}: {e}"))?;
    let mut b2 = std::fs::read(bk2).map_err(|e| format!("{bk2}: {e}"))?;
    let seal = seal_images(&mut b1, seal_off, &mut b2, door_off, version)?;
    std::fs::write(bin, &b1).map_err(|e| format!("{bin}: {e}"))?;
    std::fs::write(bk2, &b2).map_err(|e| format!("{bk2}: {e}"))?;
    let hex: String = seal.bytes().iter().map(|x| format!("{x:02x}")).collect();
    println!("sealed {hex} body {} B", b2.len() - HEADER_LEN as usize);
    Ok(())
}

/// Every relocation in a loaded section other than `.bank2` whose symbol
/// lives in `.bank2`, one line each. Err if the ELF kept no relocations.
pub fn door_hits(elf: &[u8]) -> Result<Vec<String>, String> {
    let f = ElfFile32::<Endianness>::parse(elf).map_err(|e| e.to_string())?;
    let en = f.endian();
    let has_relocs = f.elf_section_table().iter().any(|h| {
        let t = h.sh_type(en);
        t == SHT_REL || t == SHT_RELA
    });
    if !has_relocs {
        return Err("no relocation sections: built without --emit-relocs".into());
    }
    let b2 = f.section_by_name(".bank2").map(|s| s.index());
    let range = BANK2_BASE as u64..(BANK2_BASE + BANK2_LEN) as u64;
    let mut hits = Vec::new();
    for s in f.sections() {
        let alloc = s.elf_section_header().sh_flags(en) & SHF_ALLOC != 0;
        if !alloc || Some(s.index()) == b2 {
            continue;
        }
        for (off, r) in s.relocations() {
            let (name, addr, defined_in) = match r.target() {
                RelocationTarget::Symbol(i) => {
                    let sym = f.symbol_by_index(i).map_err(|e| {
                        format!("{}: unreadable symbol: {e}", s.name().unwrap_or("?"))
                    })?;
                    let name = sym
                        .name()
                        .map_err(|e| format!("unreadable symbol name: {e}"))?;
                    let sec = match sym.section() {
                        SymbolSection::Section(x) => Some(x),
                        _ => None,
                    };
                    (name.to_string(), sym.address(), sec)
                }
                RelocationTarget::Section(x) => {
                    let a = f.section_by_index(x).map_or(0, |t| t.address());
                    let n = f.section_by_index(x).ok().and_then(|t| t.name().ok());
                    (n.unwrap_or("?").to_string(), a, Some(x))
                }
                _ => {
                    return Err(format!(
                        "{}+{off:#x}: unexpected relocation target",
                        s.name().unwrap_or("?")
                    ));
                }
            };
            // A linked ELF has addresses; an object file only has sections.
            if defined_in == b2 && b2.is_some() || range.contains(&addr) {
                let site = s.address() + off;
                hits.push(format!(
                    "{} @{site:#010x} -> {name} @{addr:#010x}",
                    s.name().unwrap_or("?")
                ));
            }
        }
    }
    Ok(hits)
}

/// `chimera-utils door-check ELF`: false (exit 1) on any hit.
pub fn door_check(elf: &str) -> Result<bool, String> {
    let e = std::fs::read(elf).map_err(|e| format!("{elf}: {e}"))?;
    let hits = door_hits(&e)?;
    for h in &hits {
        eprintln!("door-check: {h}");
    }
    if hits.is_empty() {
        let f = ElfFile32::<Endianness>::parse(&*e).map_err(|e| e.to_string())?;
        let n: usize = f.sections().map(|s| s.relocations().count()).sum();
        println!("door-check: {n} relocations, none into bank 2");
    }
    Ok(hits.is_empty())
}
