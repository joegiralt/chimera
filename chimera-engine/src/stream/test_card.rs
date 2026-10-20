//! A card for the host's tests (slice 1 § 10.2): it serves Wanted slots
//! from a source held in memory, a read at a time, after a scripted
//! latency in blocks, through the one `CardSide` as the card interrupt
//! does (`claim`, `fill_buf`, `write_guard`, `fill_done`). So the tests run
//! the real slot protocol, cursor and models.

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::cell::Cell;
use core::sync::atomic::Ordering;

use chimera_hal::card_bus::{CardBus, CardFault, ProtoError};

use super::layout::{CHUNK_BYTES, ChunkIdx, FileChunk, FileKey};
use super::port::StreamPort;
use super::ring::{AudioTurn, CardSide, Claim, Rings};
use super::serve::{ErrorStep, ReadError, Rot, read_failed};
use super::stats::StreamStats;
use super::units::{AudioBlocks, Frames, Packing};
use crate::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, HeadSpec, Shape, streamed_held_len, write_head_table,
};
use crate::project::SourceId;
use crate::voice_alloc::VoiceIdx;

/// Its one file, as a frame error names it: stem 0 at block 0.
const FILE: FileKey = FileKey::new(0, chimera_hal::units::Lba::new(0));

/// Chunk `c` where it lies: a raw chunk's blocks each.
fn key(c: ChunkIdx) -> FileChunk {
    let blocks = super::layout::RAW_CHUNK_BLOCKS;
    FileChunk::new(FILE, chimera_hal::units::Lba::new(c.get() * blocks))
}

pub struct TestCard<'r> {
    card: CardSide<'r>,
    desc: Descriptor,
    /// The source's packed samples.
    samples: Vec<u8>,
    frames: Frames,
    latency: u32,
    fail: u32,
    garble: bool,
    /// The chunks whose records are rotted: they never de-frame.
    rotten: core::ops::Range<u32>,
    rot: Rot,
    /// The read in flight, the block it lands, and whether it is a retry.
    flight: Option<(Claim, AudioBlocks, bool)>,
    claims: u32,
    /// Its lookup: every source is this one.
    lookup: Box<dyn Fn(SourceId) -> Option<Frames>>,
    pub bus: CardBus,
    pub stats: StreamStats,
    /// A cursor still audible under a quiet, set through its ports.
    pub loud: Cell<bool>,
}

impl<'r> TestCard<'r> {
    /// A card serving `src` (a `Plain` row held whole: its descriptor,
    /// then its samples) through `card`, each read landing `latency_blocks`
    /// after it starts.
    pub fn new(card: CardSide<'r>, src: &[u8], latency_blocks: u32) -> TestCard<'r> {
        let desc = Descriptor::from_bytes(src).expect("a Plain row");
        assert!(
            desc.shape() == Shape::Plain && !desc.is_streamed(),
            "held whole"
        );
        let samples = src[DESC_LEN..].to_vec();
        let frames = Frames::new(2 * (samples.len() / 3) as u32);
        TestCard {
            card,
            desc,
            samples,
            frames,
            latency: latency_blocks,
            fail: 0,
            garble: false,
            rotten: 0..0,
            rot: Rot::new(),
            flight: None,
            claims: 0,
            lookup: Box::new(move |_| Some(frames)),
            bus: CardBus::new(),
            stats: StreamStats::new(),
            loud: Cell::new(false),
        }
    }

    pub fn frames(&self) -> Frames {
        self.frames
    }

    /// Reads from now on take `blocks`.
    pub fn set_latency(&mut self, blocks: u32) {
        self.latency = blocks;
    }

    /// The next `n` reads fail a CRC, as the card IRQ takes it
    /// (`serve::read_failed`): read again once, then the fault.
    pub fn fail_next(&mut self, n: u32) {
        self.fail = n;
    }

    /// The next read lands bytes that won't de-frame.
    pub fn garble_next(&mut self) {
        self.garble = true;
    }

    /// Chunks `cs` never de-frame from now on: rotted records in the file.
    pub fn rot_chunks(&mut self, cs: core::ops::Range<u32>) {
        self.rotten = cs;
    }

    /// Reads claimed so far.
    pub fn claims(&self) -> u32 {
        self.claims
    }

    /// The streamed row a load would stage: the descriptor, the head table
    /// and `head_frames` of samples at each of `at`. A note reads from `at`,
    /// and a read at `S` takes the frame before (`stream::cursor`), so each
    /// head is staged from `S − 1`, down to its pair.
    pub fn held(&self, at: &[u32], head_frames: u32) -> Vec<u8> {
        let heads: Vec<HeadSpec> = at
            .iter()
            .filter_map(|&s| {
                let f = s.saturating_sub(1) & !1;
                HeadSpec::new(f, f + head_frames.next_multiple_of(2))
            })
            .collect();
        self.held_heads(&heads)
    }

    /// The streamed row holding `heads`, past the source's end as silence.
    pub fn held_heads(&self, heads: &[HeadSpec]) -> Vec<u8> {
        let rate = self.desc.rate();
        let reach = Frames::new(crate::stream::admit::head_frames(rate));
        let mut held = alloc::vec![0; streamed_held_len(heads)];
        let mut at = write_head_table(rate, reach, heads, &mut held).expect("its length");
        for h in heads {
            let o = Frames::new(h.first()).to_bytes(Packing::Twelve).get() as usize;
            let src = self.samples.get(o..).unwrap_or(&[]);
            let n = src.len().min(h.byte_len());
            held[at..at + n].copy_from_slice(&src[..n]);
            at += h.byte_len();
        }
        held
    }

    /// Voice `voice`'s port for one block at `now`, on `turn`.
    pub fn port<'a, 's>(
        &'a self,
        turn: &'a mut AudioTurn<'s>,
        voice: VoiceIdx,
        now: AudioBlocks,
    ) -> StreamPort<'a, 's> {
        StreamPort {
            turn,
            voice,
            frames: &*self.lookup,
            now,
            bus: &self.bus,
            stats: &self.stats,
            pend: || {},
            ratio_max: None,
            loud: &self.loud,
        }
    }

    /// Between blocks, at `now`: lands the read in flight if it is due,
    /// then starts the earliest-deadline Wanted slot's, one at a time; at
    /// latency 0 every Wanted slot lands at once. Faulted, nothing starts.
    /// A rotted chunk's want is passed over, requeued and counted, as
    /// `serve_step` does.
    pub fn serve(&mut self, rings: &Rings, now: AudioBlocks) {
        let mut passed = Vec::new();
        loop {
            if let Some((_, due, _)) = &self.flight {
                if now.until(*due) > 0 {
                    return;
                }
                if let Some((c, _, retried)) = self.flight.take() {
                    self.land(c, retried, now);
                    continue;
                }
            }
            if self.bus.fault() == CardFault::Read {
                return;
            }
            let open = rings.wants().filter(|&(v, s, _)| !passed.contains(&(v, s)));
            let Some((v, s, _)) = open.min_by_key(|&(_, _, d)| now.until(d)) else {
                return;
            };
            let Some(c) = self.card.claim(v, s) else {
                return;
            };
            if self.rot.passes_over(key(c.chunk())) {
                self.card.requeue(c);
                self.stats.unreadable.fetch_add(1, Ordering::Relaxed);
                passed.push((v, s));
                continue;
            }
            self.claims += 1;
            self.flight = Some((c, now.after(self.latency), false));
        }
    }

    /// The claimed chunk's samples into its slot, its guard ahead, Ready;
    /// a failing read is read again once, else requeued, the card faulted.
    fn land(&mut self, c: Claim, retried: bool, now: AudioBlocks) {
        let key = key(c.chunk());
        let garbled = core::mem::take(&mut self.garble) || self.rotten.contains(&c.chunk().get());
        let err = if self.fail > 0 {
            self.fail -= 1;
            Some(ReadError::Proto(ProtoError::Crc))
        } else {
            garbled.then(|| ReadError::Frame(self.rot.blame(key)))
        };
        if let Some(err) = err {
            match read_failed(err, retried, &self.bus, &self.stats) {
                ErrorStep::Retry => {
                    self.claims += 1;
                    self.flight = Some((c, now.after(self.latency), true));
                }
                ErrorStep::Fault | ErrorStep::FrameError | ErrorStep::Rotted => {
                    self.card.requeue(c)
                }
            }
            return;
        }
        self.rot.cleared(key);
        let chunk = |c: ChunkIdx| {
            let o = c.get() as usize * CHUNK_BYTES.get() as usize;
            let end = (o + CHUNK_BYTES.get() as usize).min(self.samples.len());
            self.samples.get(o..end).unwrap_or(&[])
        };
        let bytes = chunk(c.chunk());
        if let Some(buf) = self.card.fill_buf(&c) {
            buf[..bytes.len()].copy_from_slice(bytes);
        }
        self.card.write_guard(&c);
        self.card.fill_done(c);
    }
}
