//! Slice 1 § 10.5's desktop QA, headless (`just stream-qa`): each factory
//! Plain bed (`target/tapes-v1/`, `just factory-tapes`) played by PLAY
//! ONE and LOOP, streamed off the sim's card through the card model
//! (`card_stream`) at `m1` and `slow`, beside the same note held whole.
//! The deck renders as `listen::render` renders it, through its port.
//! Judged by click_free_test's rule: no step over 4 × the whole render's
//! largest; under `slow`, every drop-out a fade. Prints the table; with
//! `STREAM_QA_WAVS=<dir>`, a few renders as WAVs for the owner's ears.

use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use chimera_core::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, Shape, streamed_held_len, write_head_table,
};
use chimera_core::dsp::tape_engine::{PlayMode, TapeEngine, TapeEngineParams, TapeModel};
use chimera_core::project::SourceId;
use chimera_core::source_table::SourceTable;
use chimera_core::storage::{Card, SourceHead, read_packed_into, read_source_head, source_name};
use chimera_core::stream::admit::head_frames;
use chimera_core::stream::port::StreamPort;
use chimera_core::stream::registry::StreamRegistry;
use chimera_core::stream::ring::{Rings, ThirdSlots};
use chimera_core::stream::stage::heads_for;
use chimera_core::stream::stats::StreamStats;
use chimera_core::stream::units::AudioBlocks;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_desktop::card_stream::{CardStream, ClockUs, FaultAfter, Latency, stream_file};
use chimera_desktop::listen::write_wav;
use chimera_desktop::store::DirStore;
use chimera_hal::card_bus::{CardBus, CardFault};
use chimera_hal::store::{Dir, FileName, Store, StoreError};
use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const V: VoiceIdx = VoiceIdx::ALL[0];
/// A block's time on the card's clock: 64 samples at 48 kHz, in µs × 3.
const BLOCK_US_X3: u64 = 4_000;
/// A read fault's drop-out, cleared after this many blocks: ~33 ms, a
/// UI pass or two.
const RECOVER_BLOCKS: usize = 25;
/// The beds the owner hears, LOOP at each latency.
const EARS: [&str; 3] = ["RAIN", "TRON BOWED", "PAD CHORD"];

/// A source's row in a table of one, read through block views.
struct Table {
    table: &'static SourceTable,
    tok: chimera_core::source_table::AudioToken,
}

impl Table {
    fn new(held: &[u8], rate: u16) -> Table {
        let table: &'static SourceTable = Box::leak(Box::new(SourceTable::new()));
        let bytes: &'static mut [u8] = Box::leak(held.to_vec().into_boxed_slice());
        table.attach_for_test(bytes.as_mut_ptr());
        table.publish_for_test(SourceId::ALL[0], 0, held.len() as u32, rate);
        let tok = table.reader().expect("its own table");
        Table { table, tok }
    }
}

/// The streamed row a load stages for `p`: the descriptor, the head table,
/// the heads' samples (`TestCard::held_heads`'s).
fn streamed_row(head: &SourceHead, packed: &[u8], p: &TapeEngineParams) -> Option<Vec<u8>> {
    let heads = heads_for(head.sref, p)?;
    let d = Descriptor {
        shape: Shape::Plain,
        count: 1,
        rate: head.sref.rate,
        head_frames: head_frames(head.sref.rate),
        heads: 0,
    };
    let mut held = vec![0; streamed_held_len(heads.as_slice())];
    let mut at = write_head_table(d, heads.as_slice(), &mut held)?;
    for h in heads.as_slice() {
        let o = 3 * (h.first() / 2) as usize;
        let src = packed.get(o..).unwrap_or(&[]);
        let n = src.len().min(h.bytes());
        held[at..at + n].copy_from_slice(&src[..n]);
        at += h.bytes();
    }
    Some(held)
}

/// One render's verdict.
struct Verdict {
    worst: f32,
    steady: f32,
    identical: bool,
    underruns: u32,
    cold: u32,
    /// The worst step inside a stretch that differs from the whole render.
    fade_worst: f32,
}

impl Verdict {
    fn passes(&self) -> bool {
        self.worst <= 4.0 * self.steady && self.fade_worst <= 4.0 * self.steady
    }
}

fn step(x: &[f32]) -> f32 {
    x.windows(2)
        .map(|d| (d[1] - d[0]).abs())
        .fold(0.0, f32::max)
}

/// `blocks` of `p` on note `note`, whole: the reference.
fn whole(held: &[u8], rate: u16, p: &TapeEngineParams, note: u8, blocks: usize) -> Vec<f32> {
    let mut t = Table::new(held, rate);
    let mut slot = MaybeUninit::uninit();
    let e = TapeEngine::init_in_place(&mut slot);
    e.seed(3);
    e.latch(SourceId::ALL[0]);
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut b = [0.0; BLOCK_SIZE];
    for k in 0..blocks {
        let view = t.tok.begin_block();
        if k == 0 {
            e.start(note, p, &view, SAMPLE_RATE, None);
        }
        e.play(&mut b, p, &view, SAMPLE_RATE, None);
        out.extend_from_slice(&b);
    }
    let _ = t.table;
    out
}

/// The same, streamed: its heads in the table, the rest read off `card`
/// through the card model at `latency`.
#[allow(clippy::too_many_arguments)]
fn streamed(
    card: &Path,
    head: &SourceHead,
    row: &[u8],
    p: &TapeEngineParams,
    note: u8,
    blocks: usize,
    (latency, fault): (Latency, FaultAfter),
) -> (Vec<f32>, StreamStats, u32) {
    let mut t = Table::new(row, head.sref.rate);
    let reg = Box::new(StreamRegistry::new());
    let file = stream_file(&mut DirStore::new(card.to_path_buf()), head.sref).expect("its row");
    reg.insert(SourceId::ALL[0], file).expect("a free row");
    let (rings, third) = (Box::new(Rings::new()), Box::new(ThirdSlots::new()));
    let (mut audio, side) = rings.sides(&third).expect("fresh rings");
    let (bus, stats) = (CardBus::new(), StreamStats::new());
    let mut cs = CardStream::new(
        side,
        &reg,
        DirStore::new(card.to_path_buf()),
        (&bus, &stats),
        (latency, fault),
    );
    let mut slot = MaybeUninit::uninit();
    let e = TapeEngine::init_in_place(&mut slot);
    e.seed(3);
    e.latch(SourceId::ALL[0]);
    let frames = |id| {
        reg.get(id)
            .map(chimera_core::stream::layout::StreamFile::frames)
    };
    let loud = core::cell::Cell::new(false);
    let mut out = Vec::with_capacity(blocks * BLOCK_SIZE);
    let mut b = [0.0; BLOCK_SIZE];
    let mut faulted = None;
    for k in 0..blocks {
        let now = AudioBlocks::new(k as u32);
        cs.serve(ClockUs(k as u64 * BLOCK_US_X3 / 3), now);
        // A fault clears as the UI's next passes would (`recover`).
        match (bus.fault(), faulted) {
            (CardFault::Read, None) => faulted = Some(k),
            (CardFault::Read, Some(at)) if k >= at + RECOVER_BLOCKS => {
                bus.set_fault(CardFault::Clear);
            }
            _ => {}
        }
        let view = t.tok.begin_block();
        let mut turn = audio.turn();
        let mut port = StreamPort {
            turn: &mut turn,
            voice: V,
            frames: &frames,
            now,
            bus: &bus,
            stats: &stats,
            pend: || {},
            ratio_max: None,
            loud: &loud,
        };
        if k == 0 {
            e.start(note, p, &view, SAMPLE_RATE, Some(&mut port));
        }
        e.play(&mut b, p, &view, SAMPLE_RATE, Some(&mut port));
        out.extend_from_slice(&b);
    }
    let reads = cs.reads();
    drop(cs);
    (out, stats, reads)
}

fn judge(w: &[f32], s: &[f32], stats: &StreamStats) -> Verdict {
    let steady = step(w);
    let differs = |i: usize| w[i].to_bits() != s[i].to_bits();
    // Each stretch that differs, a sample either side: a drop-out's fades.
    let mut fade_worst = 0f32;
    let mut i = 0;
    while i < s.len() {
        if !differs(i) {
            i += 1;
            continue;
        }
        let a = i;
        while i < s.len() && differs(i) {
            i += 1;
        }
        fade_worst = fade_worst.max(step(&s[a.saturating_sub(1)..(i + 1).min(s.len())]));
    }
    Verdict {
        worst: step(s),
        steady,
        identical: (0..s.len()).all(|i| !differs(i)),
        underruns: stats.underruns.load(Ordering::Relaxed),
        cold: stats.cold.load(Ordering::Relaxed),
        fade_worst,
    }
}

/// The sim's card for the run: the factory set's `.SRC`s on it.
fn card(out: &Path) -> PathBuf {
    let card = out.join("card");
    let _ = std::fs::remove_dir_all(&card);
    let tapes = card.join("CHIMERA/TAPES");
    std::fs::create_dir_all(&tapes).unwrap();
    let set = PathBuf::from(format!("{ROOT}/target/tapes-v1"));
    for e in std::fs::read_dir(&set).expect("target/tapes-v1: run `just factory-tapes`") {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "SRC") {
            std::fs::copy(&p, tapes.join(p.file_name().unwrap())).unwrap();
        }
    }
    card
}

/// Every Plain source on `card`: its head, its packed samples, its name.
fn beds(card: &Path) -> Vec<(SourceHead, Vec<u8>, String)> {
    let mut store = DirStore::new(card.to_path_buf());
    let mut files = Vec::new();
    let _ = Card::new()
        .run(&mut store, |s, r| {
            s.list(r.volume(), Dir::Tapes, &mut |f: FileName, n| {
                files.push((f, n))
            })?;
            Ok::<_, StoreError>(())
        })
        .unwrap();
    let mut out = Vec::new();
    for (f, len) in files {
        let stem = u32::from_str_radix(std::str::from_utf8(f.stem()).unwrap(), 16).unwrap();
        let got = Card::new()
            .run(&mut store, |s, r| {
                let head = read_source_head(s, r, stem, len).map_err(|_| StoreError::Io)?;
                let mut packed = vec![0; 3 * head.sref.frames.div_ceil(2) as usize];
                read_packed_into(s, r, head.sref, 0, &mut packed).map_err(|_| StoreError::Io)?;
                Ok::<_, StoreError>((head, packed))
            })
            .unwrap()
            .result
            .unwrap();
        assert_eq!(source_name(got.0.sref.crc), f);
        if got.0.shape == Shape::Plain {
            let name = got.0.name.as_str().trim_end().to_string();
            out.push((got.0, got.1, name));
        }
    }
    out.sort_by(|a, b| a.2.cmp(&b.2));
    out
}

#[test]
#[ignore = "QA: `just stream-qa`, after `just factory-tapes`"]
fn stream_qa() {
    let out = PathBuf::from(format!("{ROOT}/target/stream-qa"));
    let card = card(&out);
    let wavs = std::env::var_os("STREAM_QA_WAVS")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);
    if let Some(w) = &wavs {
        std::fs::create_dir_all(w).unwrap();
    }
    let mut fails = 0;
    println!(
        "bed | mode | card | reads | identical | worst/steady | fade worst/steady | underruns | cold | verdict"
    );
    for (head, packed, name) in beds(&card) {
        let rate = head.sref.rate;
        let mut held = Descriptor {
            shape: Shape::Plain,
            count: 1,
            rate,
            head_frames: 0,
            heads: 0,
        }
        .to_bytes()
        .to_vec();
        held.extend_from_slice(&packed);
        assert_eq!(held.len(), DESC_LEN + packed.len());
        let note = ((u32::from(head.root) + 50) / 100).min(127) as u8;
        let secs = head.sref.frames as f32 / f32::from(rate);
        for mode in [PlayMode::One, PlayMode::Loop] {
            let p = TapeEngineParams {
                model: TapeModel::Play,
                mode,
                root: f32::from(head.root),
                ..TapeEngineParams::default()
            };
            let len = if mode == PlayMode::One {
                secs + 0.5
            } else {
                secs.max(1.0) * 3.0
            };
            let blocks = (len * SAMPLE_RATE as f32) as usize / BLOCK_SIZE;
            let Some(row) = streamed_row(&head, &packed, &p) else {
                println!("{name} | {mode:?} | - | needs RAM: not streamed");
                continue;
            };
            let w = whole(&held, rate, &p, note, blocks);
            let cards = [
                (Latency::M1, FaultAfter::NEVER, "m1"),
                (Latency::Slow, FaultAfter::NEVER, "slow"),
                (
                    Latency::M1,
                    FaultAfter::parse("after:2").unwrap(),
                    "m1 fault",
                ),
            ];
            for (lat, fault, tag) in cards {
                let (s, stats, reads) =
                    streamed(&card, &head, &row, &p, note, blocks, (lat, fault));
                let v = judge(&w, &s, &stats);
                if !v.passes() {
                    fails += 1;
                }
                println!(
                    "{name} | {mode:?} | {tag} | {reads} | {} | {:.2} | {:.2} | {} | {} | {}",
                    if v.identical { "yes" } else { "no" },
                    v.worst / v.steady.max(f32::MIN_POSITIVE),
                    v.fade_worst / v.steady.max(f32::MIN_POSITIVE),
                    v.underruns,
                    v.cold,
                    if v.passes() { "pass" } else { "FAIL" }
                );
                let slug = name.to_lowercase().replace(' ', "-");
                let tag = tag.replace(' ', "-");
                if let Some(dir) = &wavs
                    && mode == PlayMode::Loop
                    && EARS.contains(&name.as_str())
                {
                    write_wav(&dir.join(format!("{slug}-loop-{tag}.wav")), &s).unwrap();
                    if tag == "m1" {
                        write_wav(&dir.join(format!("{slug}-loop-whole.wav")), &w).unwrap();
                    }
                }
            }
        }
    }
    assert_eq!(fails, 0, "{fails} renders over the click rule");
}
