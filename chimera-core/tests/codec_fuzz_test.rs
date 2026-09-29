//! Mutated and random bytes never panic the Sound decoder or leave a value
//! outside its spec. Seeded xorshift: deterministic and fast.

mod common;

use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use common::codec_util::{check_decode, encode, fix_crc, record_offsets};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
}

fn corpus() -> Vec<Vec<u8>> {
    let mut v: Vec<_> = (0..FACTORY_LEN)
        .map(|i| encode(&factory_sound(i).unwrap()))
        .collect();
    v.extend(EngineType::ALL.map(|e| encode(&Sound::init(e))));
    v
}

/// One of: flip a byte, insert one, delete one, poke a record length, poke
/// the Registry/ModDests count byte or any payload byte.
fn mutate(f: &mut Vec<u8>, r: &mut Rng) {
    let body = f.len() - 4;
    match r.below(5) {
        0 => {
            let i = r.below(body);
            f[i] ^= 1 << r.below(8);
        }
        1 => f.insert(28 + r.below(body - 27), r.byte()),
        2 if body > 28 => {
            f.remove(28 + r.below(body - 28));
        }
        // Poke a random record's length.
        3 => {
            let at = record_offsets(f);
            let p = at[r.below(at.len())];
            if p + 4 <= body {
                let len = [0, 1, 2, 511, 512, 513, 0xFFFF, r.next() as u16][r.below(8)];
                f[p + 2..p + 4].copy_from_slice(&len.to_le_bytes());
            }
        }
        // Count poke: a tag's payload first byte (block code, source count).
        _ => {
            let i = 28 + r.below(body - 28);
            f[i] = [0, 1, 0x7F, 0x80, 0xFF, r.byte()][r.below(6)];
        }
    }
}

#[test]
fn mutated_files_never_panic_or_escape_specs() {
    let corpus = corpus();
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..20_000 {
        let mut f = corpus[r.below(corpus.len())].clone();
        for _ in 0..=r.below(8) {
            if f.len() > 33 {
                mutate(&mut f, &mut r);
            }
        }
        fix_crc(&mut f);
        check_decode(&f);
    }
}

#[test]
fn random_bytes_never_panic() {
    let header = corpus()[0][..28].to_vec();
    let mut r = Rng(0xD1B5_4A32_D192_ED03);
    for i in 0..20_000 {
        let n = r.below(2_049);
        let mut f: Vec<u8> = (0..n).map(|_| r.byte()).collect();
        // Half get a valid header and CRC, so the record parser runs.
        if i % 2 == 1 && n >= 32 {
            f[..28].copy_from_slice(&header);
            fix_crc(&mut f);
        }
        check_decode(&f);
    }
}
