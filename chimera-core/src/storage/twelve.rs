//! 12-bit samples, two in three bytes (ADR 0087): a little-endian 12-bit
//! stream. `b0 = s0[7:0]`, `b1 = s0[11:8] | s1[3:0] << 4`, `b2 = s1[11:4]`.
//! The S950 stored its samples at 12 bits too.

/// Packs `samples` (each in −2048..=2047; higher bits are dropped) into
/// `out`, three bytes a pair, and returns the bytes written. An odd count's
/// last pair takes a 0 for its second sample. Panics if `out` is short.
pub fn pack12(samples: &[i16], out: &mut [u8]) -> usize {
    let mut n = 0;
    for (pair, o) in samples.chunks(2).zip(out.as_chunks_mut::<3>().0) {
        let a = pair[0] as u16 & 0xFFF;
        let b = pair.get(1).map_or(0, |&s| s as u16 & 0xFFF);
        *o = [a as u8, (a >> 8) as u8 | (b << 4) as u8, (b >> 4) as u8];
        n += 3;
    }
    assert_eq!(n, 3 * samples.len().div_ceil(2), "pack12: out is short");
    n
}

/// The two samples of pair `pair` in `bytes`, sign-extended.
pub fn unpack12(bytes: &[u8], pair: usize) -> (i16, i16) {
    let [b0, b1, b2] = [0, 1, 2].map(|k| bytes[3 * pair + k] as i16);
    let even = b0 | (b1 & 0xF) << 8;
    let odd = b1 >> 4 | b2 << 4;
    ((even << 4) >> 4, (odd << 4) >> 4)
}
