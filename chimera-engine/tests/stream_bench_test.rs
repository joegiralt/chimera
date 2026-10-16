//! The bench's streams: N heads, each over its own bed, every chunk
//! checked, stream 0 played aloud.

use chimera_engine::dsp::tape_engine::read::Shape;
use chimera_engine::name::SoundName;
use chimera_engine::storage::twelve::pack12;
use chimera_engine::storage::{
    Card, SaveError, SourceHead, SourceLayout, SourceRef, WholeShape, source_crc, source_file,
    write_source,
};
use chimera_engine::stream::bench::{
    ArmError, Asked, Bed, BedCheck, BedChecks, BedError, BedTable, BenchBeds, BenchStreams, Listen,
    MAX_BED_CHUNKS,
};
use chimera_engine::stream::edf::{Pick, Want, pick, wants};
use chimera_engine::stream::layout::{CHUNK_BYTES, CHUNK_FRAMES, ChunkIdx, StreamFile, deframe};
use chimera_engine::stream::ring::{AudioSide, CardSide, Rings, ThirdSlots};
use chimera_engine::stream::slot::SlotState;
use chimera_engine::stream::units::{AudioBlocks, Ratio, SlotIdx, SourceRate};
use chimera_engine::voice_alloc::VoiceIdx;
use chimera_hal::store::{FileRuns, ReadSink, Runs, Store};
use chimera_hal::testkit::MemStore;
use chimera_hal::units::{BlockCount, Bytes, Lba, StreamCount};
use core::ops::ControlFlow;

const CF: u32 = CHUNK_FRAMES.get();

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

/// A `.source` file of `samples`, its layout and packed bytes.
fn source(samples: &[i16]) -> (SourceLayout, Vec<u8>, Vec<u8>) {
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
    let mut s = MemStore::new(1);
    let file = source_file(l.head().sref);
    let bytes = Card::new()
        .run(&mut s, |s, r| {
            write_source(s, r, l.head(), &data).unwrap();
            let mut out = Sink(Vec::new());
            s.read(r.volume(), file, &mut out).unwrap();
            Ok::<_, SaveError>(out.0)
        })
        .unwrap()
        .result
        .unwrap();
    (l, data, bytes)
}

/// The file in two runs, cut at block 5 and laid in reverse, so chunks
/// across the cut are two pieces.
fn on_device(file: &[u8]) -> (FileRuns, Vec<u8>) {
    let blocks = file.len().div_ceil(512) as u32;
    let cut = 5.min(blocks - 1);
    let mut runs = Runs::new();
    let mut dev = vec![0u8; blocks as usize * 512];
    let tail = blocks - cut;
    runs.push(Lba::new(tail), BlockCount::new(cut).unwrap());
    runs.push(Lba::new(0), BlockCount::new(tail).unwrap());
    dev[tail as usize * 512..][..cut as usize * 512].copy_from_slice(&file[..cut as usize * 512]);
    dev[..file.len() - cut as usize * 512].copy_from_slice(&file[cut as usize * 512..]);
    let ext = runs.finish().unwrap();
    let len = Bytes::new(file.len() as u32);
    (FileRuns { ext, len }, dev)
}

/// A bed and its device image.
fn bed(samples: &[i16], stem: u32) -> (Bed, Vec<u8>) {
    let (l, data, file) = source(samples);
    let (runs, dev) = on_device(&file);
    let sf = StreamFile::from_layout(&l, runs).unwrap();
    let check = BedCheck::of_packed(stem, &data).unwrap();
    (Bed::new(sf, check).unwrap(), dev)
}

/// 8 beds, each its own content and length: bed k is 10 + k chunks, the
/// last short but for bed 0.
fn beds8() -> (&'static BenchBeds, Vec<Vec<u8>>) {
    let mut beds = Box::new(BenchBeds::new());
    let mut devs = Vec::new();
    for k in 0..8u32 {
        let frames = (10 + k) * CF - 100 * k;
        let s: Vec<i16> = (0..frames)
            .map(|i| ((i.wrapping_mul(2_654_435_761) ^ (k << 9)) % 4096) as i16 - 2048)
            .collect();
        let (b, dev) = bed(&s, 0xBED0 + k);
        beds.beds[k as usize] = Some(b);
        devs.push(dev);
    }
    (Box::leak(beds), devs)
}

/// One bed of `samples` on stream 0.
fn beds1(samples: &[i16]) -> (&'static BenchBeds, Vec<Vec<u8>>) {
    let mut beds = Box::new(BenchBeds::new());
    let (b, dev) = bed(samples, 7);
    beds.beds[0] = Some(b);
    (Box::leak(beds), vec![dev])
}

/// Serves Wanted slots as the card IRQ does: claim, the chunk's pieces off
/// the device, `deframe`, the bed's check, the guard, Ready.
struct FakeCard {
    rings: &'static Rings,
    side: CardSide<'static>,
    devs: Vec<Vec<u8>>,
    /// Fills land this many blocks past their deadline; 0 is at once.
    late: i32,
    /// Serve (stream, chunk) once with the next chunk's bytes.
    wrong: Option<(usize, u32)>,
    /// Leave (stream, chunk) unfilled.
    skip: Option<(usize, u32)>,
    /// Serve nothing while `now` is in it: a card the audio holds off.
    stall: core::ops::Range<u32>,
    /// A UI waits on the bus all the while: wants are served as `pick`
    /// hands them out, and the UI has the bus whenever it doesn't.
    ui_waiting: bool,
    claimed: Vec<(usize, u32)>,
}

impl FakeCard {
    fn serve(&mut self, beds: &BenchBeds, bench: &BenchStreams, now: AudioBlocks) {
        if self.stall.contains(&now.get()) {
            return;
        }
        if self.ui_waiting {
            while let Pick::Fill(w) = pick(wants(self.rings), now) {
                self.fill(w, beds, bench, now);
            }
            return;
        }
        let ws: Vec<_> = wants(self.rings).collect();
        for w in ws {
            self.fill(w, beds, bench, now);
        }
    }

    fn fill(&mut self, w: Want, beds: &BenchBeds, bench: &BenchStreams, now: AudioBlocks) {
        {
            if self.late > 0 && now.until(w.deadline) > -self.late {
                return;
            }
            let Some(claim) = self.side.claim(w.voice, w.slot) else {
                return;
            };
            let (v, c) = (w.voice.index(), claim.chunk());
            if self.skip == Some((v, c.get())) {
                self.side.requeue(claim);
                return;
            }
            self.claimed.push((v, c.get()));
            let bed = beds.beds[v].as_ref().unwrap();
            let mut from = c;
            if self.wrong == Some((v, c.get())) {
                self.wrong = None;
                from = c.next().unwrap();
            }
            let r = bed.file().chunk_read(from).unwrap();
            let mut raw = Vec::new();
            let mut p = r.first();
            while let Some(pc) = p {
                let at = pc.lba.get() as usize * 512;
                raw.extend_from_slice(&self.devs[v][at..][..pc.blocks.get() as usize * 512]);
                p = r.after(&pc);
            }
            let len = bed.file().chunk_read(c).unwrap().packed().get() as usize;
            let out = &mut self.side.fill_buf(&claim).unwrap()[..len];
            deframe(&raw, r.skip(), out).unwrap();
            if !bed.check().verify(c, out) {
                bench.check_failed(w.voice);
            }
            self.side.write_guard(&claim);
            assert!(self.side.fill_done(claim));
        }
    }
}

struct Rig {
    bench: &'static BenchStreams,
    beds: &'static BenchBeds,
    audio: AudioSide<'static>,
    card: FakeCard,
    now: AudioBlocks,
}

impl Rig {
    fn new((beds, devs): (&'static BenchBeds, Vec<Vec<u8>>)) -> Rig {
        let rings: &'static Rings = Box::leak(Box::new(Rings::new()));
        let third: &'static ThirdSlots = Box::leak(Box::new(ThirdSlots::new()));
        let (audio, side) = rings.sides(third).unwrap();
        Rig {
            bench: Box::leak(Box::new(BenchStreams::new())),
            beds,
            audio,
            card: FakeCard {
                rings,
                side,
                devs,
                late: 0,
                wrong: None,
                skip: None,
                stall: 0..0,
                ui_waiting: false,
                claimed: Vec::new(),
            },
            now: AudioBlocks::new(1_000),
        }
    }

    fn arm(&self, hz: u16, milli: u32, n: u8, listen: Listen) {
        let rate = SourceRate::new(hz).unwrap();
        let speed = Ratio::from_milli(milli).unwrap();
        let n = StreamCount::new(n).unwrap();
        self.bench.arm(self.beds, rate, n, speed, listen).unwrap();
    }

    /// One audio block, then the card's turn; the block's answer.
    fn block(&mut self) -> Asked {
        let asked = self
            .bench
            .block(&mut self.audio.turn(), self.beds, self.now);
        self.card.serve(self.beds, self.bench, self.now);
        self.now = self.now.after(1);
        asked
    }

    /// A block with stream 0's output.
    fn listen(&mut self) -> [i32; 128] {
        let mut out = [7; 128];
        let mut turn = self.audio.turn();
        self.bench.block(&mut turn, self.beds, self.now);
        self.bench.fill_listen(&turn, &mut out);
        self.card.serve(self.beds, self.bench, self.now);
        self.now = self.now.after(1);
        out
    }

    fn run(&mut self, blocks: u32) {
        for _ in 0..blocks {
            self.block();
        }
    }

    fn wanted(&mut self) -> usize {
        let turn = self.audio.turn();
        VoiceIdx::ALL
            .iter()
            .flat_map(|&v| SlotIdx::ALL.map(|s| turn.seen(v, s)))
            .filter(|s| matches!(s, SlotState::Wanted { .. }))
            .count()
    }
}

/// Blocks a chunk lasts at `fpb` thousandths of a frame a block, rounded up.
fn blocks_per_chunk(fpb: u32) -> u32 {
    (CF * 1_000).div_ceil(fpb)
}

#[test]
fn on_time_card_has_no_empties() {
    let mut r = Rig::new(beds8());
    r.arm(24_000, 1_200, 8, Listen::Off);
    r.run(10_000);
    let rep = r.bench.report();
    assert_eq!(rep.empties, 0);
    assert_eq!(rep.order_errors, 0);
    assert_eq!(rep.blocks, 10_000);
    assert_eq!(rep.check_errors, [0; 8]);
}

#[test]
fn late_card_counts_empties() {
    // 8 kHz: a chunk is 252 blocks, so a fill 100 late lands inside it.
    let mut r = Rig::new(beds8());
    r.card.late = 100;
    r.arm(8_000, 1_000, 2, Listen::Off);
    r.run(2_000);
    let rep = r.bench.report();
    assert!(rep.empties > 0);
    assert!(rep.late_max_blocks >= 100, "{}", rep.late_max_blocks);
}

#[test]
fn wants_carry_deadlines_in_blocks() {
    let mut r = Rig::new(beds8());
    r.arm(24_000, 1_200, 1, Listen::Off);
    let now = r.now;
    r.bench.block(&mut r.audio.turn(), r.beds, now);
    let turn = r.audio.turn();
    let v = VoiceIdx::ALL[0];
    assert!(matches!(
        turn.seen(v, SlotIdx::A),
        SlotState::Wanted { chunk, deadline, .. } if chunk.get() == 0 && deadline == now
    ));
    assert!(matches!(
        turn.seen(v, SlotIdx::B),
        SlotState::Wanted { chunk, deadline, .. }
            if chunk.get() == 1 && deadline == now.after(2_688 * 1_000 / 38_400)
    ));
    assert!(matches!(
        turn.seen(v, SlotIdx::C),
        SlotState::Wanted { chunk, deadline, .. }
            if chunk.get() == 2 && deadline == now.after(2 * (2_688 * 1_000 / 38_400))
    ));
    assert_eq!(2_688 * 1_000 / 38_400, 70);
}

#[test]
fn a_waiting_ui_never_holds_back_the_spare_slot() {
    // Owner: streams always beat the UI. A UI on the bus the whole run
    // still leaves every want served, so the stall is covered.
    let per = blocks_per_chunk(38_400);
    for from in (0..per).step_by(5) {
        let mut r = Rig::new(beds8());
        r.card.ui_waiting = true;
        r.arm(24_000, 1_200, 8, Listen::Off);
        r.run(3 * per);
        let at = r.now.get() + from;
        r.card.stall = at..at + per * 3 / 2;
        r.run(4 * per);
        let rep = r.bench.report();
        assert_eq!((rep.empties, rep.order_errors), (0, 0), "stall from {from}");
    }
}

#[test]
fn the_spare_slot_covers_a_card_stalled_a_chunk_and_a_half() {
    let per = blocks_per_chunk(38_400);
    // From every point of a chunk: two slots run dry from its start.
    for from in (0..per).step_by(5) {
        let mut r = Rig::new(beds8());
        r.arm(24_000, 1_200, 8, Listen::Off);
        r.run(3 * per);
        let at = r.now.get() + from;
        r.card.stall = at..at + per * 3 / 2;
        r.run(4 * per);
        let rep = r.bench.report();
        assert_eq!((rep.empties, rep.order_errors), (0, 0), "stall from {from}");
        assert_eq!(rep.check_errors, [0; 8]);
    }
}

#[test]
fn burst_queues_every_stream_in_one_block() {
    let mut r = Rig::new(beds8());
    r.arm(24_000, 1_200, 8, Listen::Off);
    r.run(30);
    assert_eq!(r.wanted(), 0);
    r.bench.queue_burst();
    let now = r.now;
    assert_eq!(r.bench.block(&mut r.audio.turn(), r.beds, now), Asked::New);
    assert_eq!(r.wanted(), 8);
}

#[test]
fn block_asks_only_when_it_wants() {
    let mut r = Rig::new(beds8());
    r.arm(24_000, 1_200, 8, Listen::Off);
    assert_eq!(r.block(), Asked::New);
    assert_eq!(r.block(), Asked::Nothing);
    assert_eq!(r.block(), Asked::Nothing);
}

#[test]
fn every_stream_reads_its_own_bed_in_order() {
    let mut r = Rig::new(beds8());
    r.arm(24_000, 1_200, 8, Listen::Off);
    // The longest bed, 17 chunks, three times over.
    r.run(3 * 17 * blocks_per_chunk(38_400));
    let rep = r.bench.report();
    assert_eq!(rep.order_errors, 0);
    assert_eq!(rep.empties, 0);
    assert_eq!(rep.check_errors, [0; 8]);
    for k in 0..8 {
        let mine: Vec<u32> = r
            .card
            .claimed
            .iter()
            .filter(|c| c.0 == k)
            .map(|c| c.1)
            .collect();
        let last = 9 + k as u32;
        assert!(mine.len() as u32 > 3 * (last + 1), "stream {k}");
        let want: Vec<u32> = (0..mine.len() as u32).map(|i| i % (last + 1)).collect();
        assert_eq!(mine, want, "stream {k}");
    }
}

#[test]
fn a_wrong_chunk_is_caught_on_any_stream() {
    let mut r = Rig::new(beds8());
    r.card.wrong = Some((5, 7));
    r.arm(24_000, 1_200, 8, Listen::Off);
    r.run(2 * 10 * blocks_per_chunk(38_400));
    let mut want = [0; 8];
    want[5] = 1;
    assert_eq!(r.bench.report().check_errors, want);
    assert!(r.card.wrong.is_none(), "served");
}

/// Stream `k`'s slot for chunk 4 swapped for a fill of chunk 9: the order
/// errors and stream 0's output while its head is in chunk 4.
fn misordered(k: usize) -> (u32, Vec<i32>) {
    let mut r = Rig::new(beds8());
    r.arm(24_000, 1_000, 8, Listen::Stream0);
    let v = VoiceIdx::ALL[k];
    let four = ChunkIdx::new(4).unwrap();
    // The want of chunk 4 goes out as the head enters chunk 3.
    let slot = (0..10 * blocks_per_chunk(32_000)).find_map(|_| {
        r.block();
        let turn = r.audio.turn();
        SlotIdx::ALL
            .into_iter()
            .find(|&s| matches!(turn.seen(v, s), SlotState::Ready { chunk, .. } if chunk == four))
    });
    let slot = slot.expect("chunk 4 Ready within 10 chunks");
    let mut turn = r.audio.turn();
    turn.cancel(v, slot);
    turn.want(v, slot, ChunkIdx::new(9).unwrap(), r.now)
        .unwrap();
    let mut out = Vec::new();
    // 32 frames a block: chunk 4 is frames 10,752..13,440.
    let (start, end) = (4 * CF * 1_000, 5 * CF * 1_000);
    for b in r.now.get()..r.now.get() + 6 * CF / 32 {
        let at = (b - 1_000) * 32_000;
        let o = r.listen();
        for i in 0..64 {
            let p = at + i * 500;
            if p >= start && p < end - 1_000 {
                out.push(o[2 * i as usize]);
            }
        }
    }
    (r.bench.report().order_errors, out)
}

#[test]
fn a_ready_slot_with_another_chunk_is_an_order_error() {
    assert_eq!(misordered(3).0, 1);
    let (errors, out) = misordered(0);
    assert_eq!(errors, 1);
    assert!(
        !out.is_empty() && out.iter().all(|&s| s == 0),
        "chunk 4 plays silent"
    );
}

#[test]
fn beds_loop_at_their_end() {
    let s: Vec<i16> = (0..3 * CF).map(|i| (i % 4096) as i16 - 2048).collect();
    let mut r = Rig::new(beds1(&s));
    r.arm(24_000, 1_200, 1, Listen::Off);
    r.run(10 * blocks_per_chunk(38_400));
    let got: Vec<u32> = r.card.claimed.iter().map(|c| c.1).collect();
    assert!(got.len() >= 10);
    let want: Vec<u32> = (0..got.len() as u32).map(|i| i % 3).collect();
    assert_eq!(got, want);
    let rep = r.bench.report();
    assert_eq!((rep.empties, rep.check_errors[0]), (0, 0));
}

fn check(stem: u32, chunks: u32) -> BedCheck {
    let data: Vec<u8> = (0..chunks * CHUNK_BYTES.get() - 5)
        .map(|i| (i ^ stem) as u8)
        .collect();
    BedCheck::of_packed(stem, &data).unwrap()
}

#[test]
fn bed_checks_round_trip() {
    let all = [check(1, 1), check(0xDEAD_BEEF, 9), check(3, 264)];
    let checks = BedChecks::new(&all).unwrap();
    let mut out = vec![0; 8_192];
    let n = checks.encode(&mut out);
    assert_eq!(n, 5 + 3 * 6 + (1 + 9 + 264) * 4);
    assert_eq!(&out[..4], b"BEDS");
    let back = BedTable::parse(&out[..n]).unwrap();
    assert_eq!(back.count(), 3);
    let mut a = BedCheck::new(0, &[0]).unwrap();
    for (k, b) in all.iter().enumerate() {
        assert!(back.check_into(k, &mut a));
        assert_eq!(back.stem(k), Some(b.stem()));
        assert_eq!(a.stem(), b.stem());
        assert_eq!(a.chunks(), b.chunks());
        let mut c = ChunkIdx::new(0).unwrap();
        loop {
            assert_eq!(a.crc(c), b.crc(c));
            if c == a.chunks() {
                break;
            }
            c = c.next().unwrap();
        }
    }
    assert_eq!(all[1].chunks().get(), 8);
    assert!(!back.check_into(3, &mut a));
    assert_eq!(back.stem(3), None);
}

#[test]
fn bed_checks_refuse_a_bad_table() {
    let mut out = vec![0; 4_096];
    let n = BedChecks::new(&[check(1, 2)]).unwrap().encode(&mut out);
    let good = out[..n].to_vec();
    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(BedTable::parse(&bad).err(), Some(BedError::Magic));
    let mut bad = good.clone();
    bad[4] = 9;
    assert_eq!(BedTable::parse(&bad).err(), Some(BedError::Count));
    let mut bad = good.clone();
    bad[9..11].copy_from_slice(&(MAX_BED_CHUNKS as u16 + 1).to_le_bytes());
    assert_eq!(BedTable::parse(&bad).err(), Some(BedError::Chunks));
    assert_eq!(BedTable::parse(&good[..n - 1]).err(), Some(BedError::Short));
    assert_eq!(BedTable::parse(b"BED").err(), Some(BedError::Short));
}

#[test]
fn a_bed_needs_its_table_to_match_its_chunks() {
    let s: Vec<i16> = vec![0; 3 * CF as usize];
    let (l, data, file) = source(&s);
    let sf = StreamFile::from_layout(&l, on_device(&file).0).unwrap();
    assert!(Bed::new(sf, check(1, 2)).is_none());
    assert!(Bed::new(sf, BedCheck::of_packed(1, &data).unwrap()).is_some());
    // From the table, in place.
    let all = [check(1, 2), BedCheck::of_packed(9, &data).unwrap()];
    let mut out = vec![0; 1_024];
    let n = BedChecks::new(&all).unwrap().encode(&mut out);
    let table = BedTable::parse(&out[..n]).unwrap();
    let mut slot = None;
    assert!(!table.place(0, sf, &mut slot) && slot.is_none());
    assert!(!table.place(2, sf, &mut slot) && slot.is_none());
    assert!(table.place(1, sf, &mut slot));
    let bed = slot.unwrap();
    assert_eq!(bed.check().stem(), 9);
    let c = ChunkIdx::new(2).unwrap();
    assert_eq!(bed.check().crc(c), all[1].crc(c));
}

#[test]
fn arming_needs_a_bed_a_stream() {
    let s: Vec<i16> = vec![0; CF as usize];
    let r = Rig::new(beds1(&s));
    let rate = SourceRate::new(24_000).unwrap();
    let two = StreamCount::new(2).unwrap();
    assert_eq!(
        r.bench.arm(r.beds, rate, two, Ratio::ONE, Listen::Off),
        Err(ArmError::Beds)
    );
}

/// Frame `i` of the listen beds: a ramp, wrapping in 12 bits.
fn ramp(i: u32) -> i32 {
    (i % 4096) as i32 - 2048
}

/// The output word of stream 0 at `p` thousandths of a frame, at 24 kHz
/// played at 48 kHz: on a frame or halfway, −6 dBFS.
fn expected(p: u32) -> i32 {
    let f = p / 1_000;
    if p.is_multiple_of(1_000) {
        ramp(f) << 19
    } else {
        (ramp(f) + ramp(f + 1)) << 18
    }
}

#[test]
fn listen_plays_ring_samples_in_order() {
    let frames = 3 * CF;
    let s: Vec<i16> = (0..frames).map(|i| ramp(i) as i16).collect();
    let mut r = Rig::new(beds1(&s));
    r.arm(24_000, 1_000, 1, Listen::Stream0);
    assert_eq!(r.listen(), [0; 128], "nothing Ready at the first block");
    // Two passes: every chunk boundary and the loop's.
    for b in 1..2 * frames / 32 {
        let o = r.listen();
        for i in 0..64 {
            let p = (b * 32_000 + i * 500) % (frames * 1_000);
            let w = if p / 1_000 == frames - 1 && !p.is_multiple_of(1_000) {
                (ramp(frames - 1) + ramp(0)) << 18
            } else {
                expected(p)
            };
            let i = i as usize;
            assert_eq!((o[2 * i], o[2 * i + 1]), (w, w), "block {b} sample {i}");
        }
    }
    assert_eq!(r.bench.report().empties, 0);
}

#[test]
fn listen_writes_silence_on_an_empty() {
    let frames = 4 * CF;
    let s: Vec<i16> = (0..frames).map(|i| 100 + (i % 1_000) as i16).collect();
    let mut r = Rig::new(beds1(&s));
    r.card.skip = Some((0, 2));
    r.arm(24_000, 1_000, 1, Listen::Stream0);
    let (start, end) = (2 * CF * 1_000, 3 * CF * 1_000);
    let mut seen = 0;
    for b in 0..frames / 32 {
        let o = r.listen();
        for i in 0..64 {
            let p = b * 32_000 + i * 500;
            if p >= start && p < end - 1_000 {
                assert_eq!(o[2 * i as usize], 0, "block {b} sample {i}");
                seen += 1;
            } else if b > 0 && p + 1_000 < start {
                assert_ne!(o[2 * i as usize], 0, "block {b} sample {i}");
            }
        }
    }
    assert!(seen > 0);
    assert_eq!(r.bench.report().empties, 1);
}

#[test]
fn listen_off_leaves_the_synth() {
    let s: Vec<i16> = vec![5; CF as usize];
    let mut r = Rig::new(beds1(&s));
    r.arm(24_000, 1_000, 1, Listen::Off);
    assert_eq!(r.listen(), [7; 128]);
    assert!(!r.bench.listening());
}
