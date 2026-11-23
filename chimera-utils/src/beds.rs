// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bench beds (slice 0's gate): one aperiodic chirp WAV a stream, and the
//! per-chunk CRC table the unit checks every chunk it consumes against.
//!
//! A chirp has no period, so no period divides a chunk, and a repeated,
//! skipped or misplaced chunk never lines up with the one it replaces.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chimera_engine::dsp::tape_engine::read::Shape;
use chimera_engine::storage::{Crc32, PutRefusal, SourceCheck, SourceLayout, Stem};
use chimera_engine::stream::bench::{BedCheck, BedChecks, MAX_BED_CHUNKS};
use chimera_engine::stream::layout::{ChunkIdx, StreamFile, deframe};
use chimera_hal::store::{FileRuns, Runs, Seal};
use chimera_hal::units::{BlockCount, Bytes, Lba};

/// A bed's first frequency, and the ratio between beds.
const F0_HZ: f64 = 100.0;
const STEP: f64 = 1.3;
/// −6 dBFS.
const AMP: f64 = 0.5;

/// `--name V` taken out of `args`, else `default`.
pub fn opt<T: FromStr>(args: &mut Vec<String>, name: &str, default: T) -> Result<T, String> {
    let Some(i) = args.iter().position(|a| a == name) else {
        return Ok(default);
    };
    if i + 1 >= args.len() {
        return Err(format!("{name} needs a value"));
    }
    let v = args.remove(i + 1);
    args.remove(i);
    v.parse()
        .map_err(|_| format!("{name}: `{v}` is not a number"))
}

/// Bed `k`: a linear chirp from `100 x 1.3^k` Hz to 4 times that, over
/// `secs`, at −6 dBFS, as 16-bit samples at `rate`.
pub fn chirp(k: u32, secs: f64, rate: u32) -> Vec<i16> {
    let f0 = F0_HZ * STEP.powi(k as i32);
    let slope = 3.0 * f0 / secs;
    let n = (secs * f64::from(rate)).round() as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / f64::from(rate);
            let phase = TAU * (f0 * t + 0.5 * slope * t * t);
            (AMP * 32767.0 * phase.sin()).round() as i16
        })
        .collect()
}

/// `beds write DIR [--n 8] [--secs 25] [--rate 24000]`.
pub fn write(args: &[String]) -> Result<(), String> {
    let mut args = args.to_vec();
    let n: u32 = opt(&mut args, "--n", 8)?;
    let secs: f64 = opt(&mut args, "--secs", 25.0)?;
    let rate: u32 = opt(&mut args, "--rate", 24_000)?;
    let [dir] = args.as_slice() else {
        return Err("beds write DIR [--n 8] [--secs 25] [--rate 24000]".into());
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    for k in 0..n {
        let path = Path::new(dir).join(format!("bed{k}.wav"));
        let mut w = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
        for s in chirp(k, secs, rate) {
            w.write_sample(s).map_err(|e| e.to_string())?;
        }
        w.finalize().map_err(|e| e.to_string())?;
        println!("{}", path.display());
    }
    Ok(())
}

/// A sealed source's table, read as the unit will: its head, one run, its
/// `StreamFile`, every chunk de-framed and checked.
fn table(path: &Path) -> Result<BedCheck, String> {
    let name = path.display();
    let file = std::fs::read(path).map_err(|e| format!("{name}: {e}"))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| u32::from_str_radix(s, 16).ok())
        .ok_or_else(|| format!("{name}: the name is not the source's CRC in hex"))?;
    let len = u32::try_from(file.len()).map_err(|_| format!("{name}: too big"))?;
    let refuse = |e: PutRefusal| format!("{name}: {}", e.message());
    let mut check = SourceCheck::new(Stem::new(stem), len).map_err(refuse)?;
    check.feed(&file).map_err(refuse)?;
    let head = check.finish().map_err(refuse)?;
    if head.shape != Shape::Plain {
        return Err(format!("{name}: not a Plain source"));
    }
    let layout = SourceLayout::whole(head).ok_or_else(|| format!("{name}: no layout"))?;
    let blocks = BlockCount::new(len.div_ceil(512)).ok_or_else(|| format!("{name}: empty"))?;
    let mut runs = Runs::new();
    runs.push(Lba::new(0), blocks);
    let ext = runs.finish().map_err(|_| format!("{name}: fragmented"))?;
    let sf = StreamFile::from_layout(
        &layout,
        FileRuns {
            ext,
            len: Bytes::new(len),
            seal: Seal::Sealed,
        },
    )
    .ok_or_else(|| format!("{name}: the file does not hold its samples"))?;
    let mut crcs = Vec::new();
    for c in 0..=sf.last_chunk().get() {
        let c = ChunkIdx::new(c).ok_or_else(|| format!("{name}: too many chunks"))?;
        let read = sf
            .chunk_read(c)
            .ok_or_else(|| format!("{name}: chunk {c:?} unreadable"))?;
        let blocks = sf
            .chunk_blocks(c)
            .ok_or_else(|| format!("{name}: chunk {c:?}"))?;
        let mut raw = Vec::new();
        for (lba, n) in sf.reads(&blocks) {
            let at = lba.get() as usize * 512;
            let end = (at + n.get() as usize * 512).min(file.len());
            raw.extend_from_slice(
                file.get(at..end)
                    .ok_or_else(|| format!("{name}: past its end"))?,
            );
        }
        let mut out = vec![0; read.packed().get() as usize];
        deframe(&raw, read.skip(), &mut out)
            .map_err(|e| format!("{name}: chunk {}: {e:?}", c.get()))?;
        let mut crc = Crc32::new();
        crc.update(&out);
        crcs.push(crc.finish());
    }
    if crcs.len() > MAX_BED_CHUNKS {
        return Err(format!(
            "{name}: {} chunks, at most {MAX_BED_CHUNKS}",
            crcs.len()
        ));
    }
    BedCheck::new(stem, &crcs).ok_or_else(|| format!("{name}: no chunks"))
}

/// `beds check OUT.CHK BED.SRC …`.
pub fn check(args: &[String]) -> Result<(), String> {
    let [out, srcs @ ..] = args else {
        return Err("beds check OUT.CHK BED.SRC …".into());
    };
    let tables: Vec<BedCheck> = srcs
        .iter()
        .map(|s| table(&PathBuf::from(s)))
        .collect::<Result<_, _>>()?;
    let all = BedChecks::new(&tables).ok_or("at most 8 beds")?;
    let mut bytes = vec![0; 5 + tables.len() * (6 + 4 * MAX_BED_CHUNKS)];
    let n = all.encode(&mut bytes);
    std::fs::write(out, &bytes[..n]).map_err(|e| format!("{out}: {e}"))?;
    println!("{out} {n}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_engine::name::SoundName;
    use chimera_engine::storage::twelve::pack12;
    use chimera_engine::storage::{
        Card, SaveError, SourceHead, SourceRef, WholeShape, source_crc, source_file, write_source,
    };
    use chimera_engine::stream::bench::{BedCheck, BedTable};
    use chimera_engine::stream::layout::{CHUNK_BYTES, CHUNK_FRAMES};
    use chimera_hal::store::{ReadSink, Store};
    use chimera_hal::testkit::MemStore;
    use core::ops::ControlFlow;

    struct Sink(Vec<u8>);

    impl ReadSink for Sink {
        fn begin(&mut self, _: u32) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }
        fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
            self.0.extend_from_slice(b);
            ControlFlow::Continue(())
        }
    }

    #[test]
    fn no_two_chunks_of_a_bed_match() {
        let cf = CHUNK_FRAMES.get() as usize;
        for k in 0..8 {
            let bed = chirp(k, 25.0, 24_000);
            let chunks: Vec<&[i16]> = bed.chunks_exact(cf).collect();
            for (i, a) in chunks.iter().enumerate() {
                for (j, b) in chunks.iter().enumerate().skip(i + 1) {
                    // Early out: windows that differ do so within a few ms.
                    let worst = a
                        .iter()
                        .zip(*b)
                        .map(|(&x, &y)| (i32::from(x) - i32::from(y)).abs())
                        .find(|&d| d > 3_277);
                    assert!(
                        worst.is_some(),
                        "bed {k}: chunks {i} and {j} differ by at most 0.1"
                    );
                }
            }
        }
    }

    /// A sealed `.SRC` of `samples` (12-bit): its name's CRC, its bytes and
    /// the packed samples.
    fn sealed(samples: &[i16]) -> (u32, Vec<u8>, Vec<u8>) {
        let mut data = vec![0; samples.len().div_ceil(2) * 3];
        pack12(samples, &mut data);
        let head = SourceHead {
            sref: SourceRef {
                crc: source_crc(WholeShape::Plain, 1, &data),
                frames: samples.len() as u32,
                rate: 24_000,
            },
            shape: Shape::Plain,
            count: 1,
            root: 6_000,
            period_q16: 0,
            name: SoundName::new("BED").unwrap(),
        };
        let l = SourceLayout::whole(head).unwrap();
        let file = source_file(l.head().sref);
        let bytes = Card::new()
            .run(&mut MemStore::new(1), |s, r| {
                write_source(s, r, l.head(), &data).unwrap();
                let mut out = Sink(Vec::new());
                s.read(r.volume(), file, &mut out).unwrap();
                Ok::<_, SaveError>(out.0)
            })
            .unwrap()
            .result
            .unwrap();
        (head.sref.crc, bytes, data)
    }

    #[test]
    fn check_table_matches_deframe() {
        // 12-bit samples: a bed through the sealed source, as the unit gets it.
        let bed: Vec<i16> = chirp(0, 3.0, 24_000).iter().map(|&s| s >> 4).collect();
        let (crc, bytes, packed) = sealed(&bed);
        let dir = std::env::temp_dir().join(format!("beds-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join(format!("{crc:08X}.SRC"));
        let out = dir.join("BEDS.CHK");
        std::fs::write(&src, &bytes).unwrap();
        check(&[out.display().to_string(), src.display().to_string()]).unwrap();
        let table = std::fs::read(&out).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let table = BedTable::parse(&table).unwrap();
        assert_eq!(table.count(), 1);
        assert_eq!(table.stem(0), Some(crc));
        let mut got = BedCheck::new(0, &[0]).unwrap();
        assert!(table.check_into(0, &mut got));
        let chunks: Vec<&[u8]> = packed.chunks(CHUNK_BYTES.get() as usize).collect();
        assert_eq!(got.chunks().get() as usize + 1, chunks.len());
        for (c, bytes) in chunks.iter().enumerate() {
            let mut crc = Crc32::new();
            crc.update(bytes);
            assert_eq!(
                got.crc(ChunkIdx::new(c as u32).unwrap()),
                Some(crc.finish()),
                "chunk {c}"
            );
        }
    }
}
