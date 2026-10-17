//! A card for the host's tests (slice 1 § 10.2): it serves Wanted slots
//! from a source held in memory, a read at a time, after a scripted
//! latency in blocks, through the one `CardSide` as the card interrupt
//! does (`claim`, `fill_buf`, `write_guard`, `fill_done`). So the tests run
//! the real slot protocol, cursor and models.

use alloc::vec::Vec;

use chimera_hal::card_bus::CardBus;

use super::layout::{CHUNK_BYTES, ChunkIdx};
use super::port::StreamPort;
use super::ring::{AudioTurn, CardSide, Claim, Rings};
use super::stats::StreamStats;
use super::units::{AudioBlocks, Frames};
use crate::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, HeadSpec, Shape, streamed_held_len, write_head_table,
};
use crate::voice_alloc::VoiceIdx;

pub struct TestCard<'r> {
    card: CardSide<'r>,
    desc: Descriptor,
    /// The source's packed samples.
    samples: Vec<u8>,
    frames: Frames,
    latency: u32,
    fail: u32,
    /// The read in flight, and the block it lands.
    flight: Option<(Claim, AudioBlocks)>,
    claims: u32,
    pub bus: CardBus,
    pub stats: StreamStats,
}

impl<'r> TestCard<'r> {
    /// A card serving `src` (a `Plain` row held whole: its descriptor,
    /// then its samples) through `card`, each read landing `latency_blocks`
    /// after it starts.
    pub fn new(card: CardSide<'r>, src: &[u8], latency_blocks: u32) -> TestCard<'r> {
        let desc = Descriptor::from_bytes(src).expect("a Plain row");
        assert!(desc.shape == Shape::Plain && desc.heads == 0, "held whole");
        let samples = src[DESC_LEN..].to_vec();
        let frames = Frames::new(2 * (samples.len() / 3) as u32);
        TestCard {
            card,
            desc,
            samples,
            frames,
            latency: latency_blocks,
            fail: 0,
            flight: None,
            claims: 0,
            bus: CardBus::new(),
            stats: StreamStats::new(),
        }
    }

    pub fn frames(&self) -> Frames {
        self.frames
    }

    /// Reads from now on take `blocks`.
    pub fn set_latency(&mut self, blocks: u32) {
        self.latency = blocks;
    }

    /// The next `n` reads fail: requeued, as a read error's retry.
    pub fn fail_next(&mut self, n: u32) {
        self.fail = n;
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
        let d = Descriptor {
            head_frames: crate::stream::admit::head_frames(self.desc.rate),
            ..self.desc
        };
        let mut held = alloc::vec![0; streamed_held_len(heads)];
        let mut at = write_head_table(d, heads, &mut held).expect("its length");
        for h in heads {
            let o = 3 * (h.first() / 2) as usize;
            let src = self.samples.get(o..).unwrap_or(&[]);
            let n = src.len().min(h.bytes());
            held[at..at + n].copy_from_slice(&src[..n]);
            at += h.bytes();
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
            file_frames: self.frames,
            now,
            bus: &self.bus,
            stats: &self.stats,
            pend: || {},
            ratio_max: None,
        }
    }

    /// Between blocks, at `now`: lands the read in flight if it is due,
    /// then starts the earliest-deadline Wanted slot's, one at a time; at
    /// latency 0 every Wanted slot lands at once.
    pub fn serve(&mut self, rings: &Rings, now: AudioBlocks) {
        loop {
            if let Some((_, due)) = &self.flight {
                if now.until(*due) > 0 {
                    return;
                }
                if let Some((c, _)) = self.flight.take() {
                    self.land(c);
                }
            }
            let Some((v, s, _)) = rings.wants().min_by_key(|&(_, _, d)| now.until(d)) else {
                return;
            };
            let Some(c) = self.card.claim(v, s) else {
                return;
            };
            self.claims += 1;
            self.flight = Some((c, now.after(self.latency)));
        }
    }

    /// The claimed chunk's samples into its slot, its guard ahead, Ready;
    /// a failing read requeues it.
    fn land(&mut self, c: Claim) {
        if self.fail > 0 {
            self.fail -= 1;
            self.card.requeue(c);
            return;
        }
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
