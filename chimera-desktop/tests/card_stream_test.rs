// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The desktop's card model (slice 1 § 10.5): Wanted slots served from
//! `DirStore` files through the one `CardSide`, after a latency drawn
//! from M1's reads.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use chimera_core::dsp::tape_engine::read::Shape;
use chimera_core::name::SoundName;
use chimera_core::project::SourceId;
use chimera_core::storage::{Card, SourceHead, SourceRef, WholeShape, source_crc, write_source};
use chimera_core::stream::layout::{CHUNK_BYTES, ChunkIdx};
use chimera_core::stream::registry::StreamRegistry;
use chimera_core::stream::ring::{Rings, ThirdSlots};
use chimera_core::stream::stats::StreamStats;
use chimera_core::stream::units::{AudioBlocks, SlotIdx};
use chimera_core::voice_alloc::VoiceIdx;
use chimera_desktop::card_stream::{CardStream, ClockUs, FaultAfter, Latency, stream_file};
use chimera_desktop::store::DirStore;
use chimera_hal::card_bus::{CardBus, CardFault};

/// A fresh card under `target/`, never the scratch `/tmp`.
fn card_dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("card-stream-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A 24 kHz Plain `.SRC` of `frames` seeded bytes on `root`: its row's
/// file and its packed samples.
fn bed(root: &Path, frames: u32) -> (SourceRef, Vec<u8>) {
    let packed: Vec<u8> = (0..3 * frames.div_ceil(2))
        .scan(0x5EED_0025u32, |x, _| {
            *x ^= *x << 13;
            *x ^= *x >> 17;
            *x ^= *x << 5;
            Some(*x as u8)
        })
        .collect();
    let sref = SourceRef {
        crc: source_crc(WholeShape::Plain, 1, &packed),
        frames,
        rate: 24_000,
    };
    let head = SourceHead {
        sref,
        shape: Shape::Plain,
        count: 1,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("BED").unwrap(),
    };
    let mut store = DirStore::new(root.to_path_buf());
    Card::new()
        .run(&mut store, |st, rd| write_source(st, rd, &head, &packed))
        .unwrap()
        .result
        .unwrap();
    (sref, packed)
}

#[test]
fn card_stream_serves_wanted_slots() {
    let root = card_dir("serves");
    let (sref, packed) = bed(&root, 30_000);
    let reg = Box::new(StreamRegistry::new());
    let id = SourceId::ALL[3];
    let file = stream_file(&mut DirStore::new(root.clone()), sref).expect("its row");
    reg.insert(id, file).unwrap();
    let (rings, third) = (Box::new(Rings::new()), Box::new(ThirdSlots::new()));
    let (mut audio, card) = rings.sides(&third).unwrap();
    let (bus, stats) = (CardBus::new(), StreamStats::new());
    let mut cs = CardStream::new(
        card,
        &reg,
        DirStore::new(root),
        (&bus, &stats),
        (Latency::M1, FaultAfter::NEVER),
    );
    let v = VoiceIdx::ALL[5];
    let now = AudioBlocks::new(0);
    {
        let turn = audio.turn();
        turn.stream(v, id);
        for (s, c) in [(SlotIdx::A, 0), (SlotIdx::B, 1), (SlotIdx::C, 7)] {
            turn.want(v, s, ChunkIdx::new(c).unwrap(), now.after(10))
                .unwrap();
        }
    }
    // One read in flight at a time, each landing after M1's latency: none
    // before 4.3 ms, all three by 3 × 6.52 ms served every 0.1 ms.
    cs.serve(ClockUs(0), now);
    cs.serve(ClockUs(4_000), now);
    assert_eq!(cs.reads(), 0, "M1's quickest read is 4.3 ms");
    for us in (4_000..=20_000).step_by(100) {
        cs.serve(ClockUs(us), now);
    }
    assert_eq!(cs.reads(), 3);
    let turn = audio.turn();
    for (s, c) in [(SlotIdx::A, 0usize), (SlotIdx::B, 1), (SlotIdx::C, 7)] {
        let chimera_core::stream::slot::SlotState::Ready { generation, .. } = turn.seen(v, s)
        else {
            panic!("slot {s:?} not Ready");
        };
        let r = turn.ready(v, s, generation).unwrap();
        let n = CHUNK_BYTES.get() as usize;
        // A guarded slot's bytes start at the guard's frames, 3 B a pair.
        let (first, bytes) = r.packed();
        let at = first.unsigned_abs() as usize / 2 * 3;
        assert_eq!(bytes[at..at + n], packed[c * n..(c + 1) * n], "chunk {c}");
    }
    assert_eq!(stats.unreadable.load(Ordering::Relaxed), 0);
}

/// `CHIMERA_CARD_FAULT=after:N`: N reads land, the next fails its read
/// and its retry, and the card faults; nothing reads till it clears.
#[test]
fn card_fault_after_n_reads() {
    let root = card_dir("fault");
    let (sref, _) = bed(&root, 30_000);
    let reg = Box::new(StreamRegistry::new());
    let id = SourceId::ALL[0];
    reg.insert(
        id,
        stream_file(&mut DirStore::new(root.clone()), sref).unwrap(),
    )
    .unwrap();
    let (rings, third) = (Box::new(Rings::new()), Box::new(ThirdSlots::new()));
    let (mut audio, card) = rings.sides(&third).unwrap();
    let (bus, stats) = (CardBus::new(), StreamStats::new());
    let fault = FaultAfter::parse("after:2").unwrap();
    let mut cs = CardStream::new(
        card,
        &reg,
        DirStore::new(root),
        (&bus, &stats),
        (Latency::Zero, fault),
    );
    let v = VoiceIdx::ALL[0];
    let now = AudioBlocks::new(0);
    {
        let turn = audio.turn();
        turn.stream(v, id);
        for (s, c) in [(SlotIdx::A, 0), (SlotIdx::B, 1), (SlotIdx::C, 2)] {
            turn.want(v, s, ChunkIdx::new(c).unwrap(), now.after(10))
                .unwrap();
        }
    }
    cs.serve(ClockUs(0), now);
    assert_eq!(cs.reads(), 2);
    assert_eq!(bus.fault(), CardFault::Read);
    assert!(matches!(
        audio.turn().seen(v, SlotIdx::C),
        chimera_core::stream::slot::SlotState::Wanted { .. }
    ));
    bus.set_fault(CardFault::Clear);
    cs.serve(ClockUs(1), now);
    assert_eq!(cs.reads(), 3, "cleared, it reads again");
    assert!(FaultAfter::parse("after:x").is_none());
    assert!(FaultAfter::parse("2").is_none());
}

/// `CHIMERA_CARD_LATENCY`: `m1` replays M1's 4 KB random reads (seed 1:
/// min 4.31, median 5.31, p95 6.25, p99 6.30, max 6.52 ms), `slow` three
/// times them, `zero` none.
#[test]
fn latency_model_m1_median_5_3ms() {
    let mut draws: Vec<Duration> = (0..10_001)
        .map(|i| Latency::M1.at(i as f32 / 10_000.0))
        .collect();
    draws.sort();
    let ms = |d: Duration| d.as_secs_f32() * 1e3;
    assert!((ms(draws[5_000]) - 5.31).abs() < 0.01, "{:?}", draws[5_000]);
    assert!((ms(draws[0]) - 4.31).abs() < 0.01);
    assert!((ms(draws[10_000]) - 6.52).abs() < 0.01);
    assert!((ms(draws[9_500]) - 6.25).abs() < 0.01);
    assert!((ms(Latency::Slow.at(0.5)) - 3.0 * 5.31).abs() < 0.03);
    assert_eq!(Latency::Zero.at(0.7), Duration::ZERO);
    for (v, l) in [
        ("m1", Latency::M1),
        ("slow", Latency::Slow),
        ("zero", Latency::Zero),
    ] {
        assert_eq!(Latency::parse(v), Some(l));
    }
    assert_eq!(Latency::parse("fast"), None);
}
