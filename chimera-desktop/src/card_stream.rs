//! The sim's card for streams (slice 1 § 10.5): Wanted slots served from
//! `DirStore` files through the one `CardSide`, as the card interrupt
//! serves them (`serve_step`, `deframe`, `write_guard`, `fill_done`), one
//! read in flight, each landing after a latency drawn from M1's reads.
//! A file's runs are `synthetic_runs`, so a piece's block maps back to the
//! file by `synthetic_lba`.
//!
//! The clock is the caller's: wall time on the live thread (`spawn`),
//! audio blocks' time in an offline render, so both run this one path.

use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use chimera_core::storage::{Card, SourceLayout, SourceRef, read_source_head, source_file};
use chimera_core::stream::layout::{RAW_CHUNK_BYTES, StreamFile, deframe};
use chimera_core::stream::registry::StreamRegistry;
use chimera_core::stream::ring::{CardSide, Claim};
use chimera_core::stream::serve::{
    Attempt, ErrorStep, HeldRead, Parked, Rot, ServeStep, read_failed, serve_step,
};
use chimera_core::stream::stats::StreamStats;
use chimera_core::stream::units::AudioBlocks;
use chimera_hal::card_bus::{CardBus, CardFault, ProtoError, Quiet};
use chimera_hal::store::{CHUNK, Store, StoreError, synthetic_lba};

use crate::store::DirStore;

/// A time on the card's clock, in µs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ClockUs(pub u64);

impl ClockUs {
    fn after(self, d: Duration) -> ClockUs {
        ClockUs(self.0.saturating_add(d.as_micros() as u64))
    }
}

/// `CHIMERA_CARD_LATENCY`: how long a chunk's read takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Latency {
    Zero,
    /// M1's 4 KB random reads, replayed.
    M1,
    /// Three times M1's.
    Slow,
}

/// M1's 4 KB random reads on the unit's card, seed 1 (`m1/seed1.txt`):
/// (quantile, µs) at min, median, p95, p99 and max.
const M1_US: [(f32, f32); 5] = [
    (0.0, 4_312.0),
    (0.5, 5_310.0),
    (0.95, 6_246.0),
    (0.99, 6_301.0),
    (1.0, 6_517.0),
];

impl Latency {
    pub fn parse(s: &str) -> Option<Latency> {
        match s {
            "zero" => Some(Latency::Zero),
            "m1" => Some(Latency::M1),
            "slow" => Some(Latency::Slow),
            _ => None,
        }
    }

    /// `CHIMERA_CARD_LATENCY`, `m1` unset; `None` for one it doesn't know.
    pub fn from_env() -> Option<Latency> {
        std::env::var("CHIMERA_CARD_LATENCY").map_or(Some(Latency::M1), |v| Latency::parse(&v))
    }

    /// The read at quantile `q` (0 to 1): M1's points joined by lines.
    pub fn at(self, q: f32) -> Duration {
        let m1 = || {
            let q = chimera_core::dsp::clampf(q, 0.0, 1.0);
            let us = M1_US
                .windows(2)
                .find(|w| q <= w[1].0)
                .map_or(M1_US[4].1, |w| {
                    let t = (q - w[0].0) / (w[1].0 - w[0].0);
                    w[0].1 + t * (w[1].1 - w[0].1)
                });
            Duration::from_secs_f32(us * 1e-6)
        };
        match self {
            Latency::Zero => Duration::ZERO,
            Latency::M1 => m1(),
            Latency::Slow => 3 * m1(),
        }
    }
}

/// `CHIMERA_CARD_FAULT=after:N`: N reads land, then one fails its read
/// and its retry, which faults the card (spec § 9). Once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultAfter(Option<u32>);

impl FaultAfter {
    pub const NEVER: FaultAfter = FaultAfter(None);

    pub fn parse(s: &str) -> Option<FaultAfter> {
        let n = s.strip_prefix("after:")?.parse().ok()?;
        Some(FaultAfter(Some(n)))
    }

    /// `CHIMERA_CARD_FAULT`, never unset; `None` for one it can't read.
    pub fn from_env() -> Option<FaultAfter> {
        std::env::var("CHIMERA_CARD_FAULT")
            .map_or(Some(FaultAfter::NEVER), |v| FaultAfter::parse(&v))
    }
}

/// `sref`'s file as the registry streams it, from its head and runs.
pub fn stream_file(store: &mut DirStore, sref: SourceRef) -> Option<StreamFile> {
    Card::new()
        .run(store, |s, r| {
            let runs = s
                .runs(r.volume(), source_file(sref))
                .map_err(|_| StoreError::Io)?;
            let head = read_source_head(
                s,
                r,
                sref.stem(),
                runs.len.get(),
                chimera_core::storage::Reach::Head,
            )
            .map_err(|_| StoreError::Io)?;
            Ok::<_, StoreError>(
                SourceLayout::whole(head).and_then(|l| StreamFile::from_layout(&l, runs)),
            )
        })
        .ok()
        .and_then(|o| o.result.ok())
        .flatten()
}

/// The read in flight: its claim, its row held, the raw blocks read, and
/// when it lands.
struct Flight<'r> {
    claim: Claim,
    row: HeldRead<'r>,
    raw: Vec<u8>,
    due: ClockUs,
    attempt: Attempt,
    fails: bool,
}

pub struct CardStream<'r> {
    side: CardSide<'r>,
    reg: &'r StreamRegistry,
    store: DirStore,
    bus: &'r CardBus,
    stats: &'r StreamStats,
    latency: Latency,
    fault: FaultAfter,
    rng: u32,
    reads: u32,
    flight: Option<Flight<'r>>,
    ceiling: bool,
    rot: Rot,
    parked: Parked,
}

impl<'r> CardStream<'r> {
    pub fn new(
        side: CardSide<'r>,
        reg: &'r StreamRegistry,
        store: DirStore,
        (bus, stats): (&'r CardBus, &'r StreamStats),
        (latency, fault): (Latency, FaultAfter),
    ) -> CardStream<'r> {
        CardStream {
            side,
            reg,
            store,
            bus,
            stats,
            latency,
            fault,
            rng: 0x5EED_0C4D,
            reads: 0,
            flight: None,
            ceiling: false,
            rot: Rot::new(),
            parked: Parked::new(),
        }
    }

    /// Every read at the model's ceiling (quantile 1): its worst card.
    pub fn at_ceiling(self) -> CardStream<'r> {
        CardStream {
            ceiling: true,
            ..self
        }
    }

    /// Reads that went Ready.
    pub fn reads(&self) -> u32 {
        self.reads
    }

    /// At `at`, the audio at block `now`: lands the read in flight once
    /// due, then starts the earliest want's, one at a time. Faulted or
    /// quiet, nothing starts.
    pub fn serve(&mut self, at: ClockUs, now: AudioBlocks) {
        loop {
            match self.flight.take() {
                Some(f) if at < f.due => {
                    self.flight = Some(f);
                    return;
                }
                Some(f) => {
                    self.land(f, at);
                    continue;
                }
                None => {}
            }
            if self.bus.fault() == CardFault::Read || self.bus.quiet() != Quiet::Off {
                return;
            }
            let ServeStep::Read { claimed, row } = serve_step(
                &mut self.side,
                self.reg,
                now,
                &mut self.rot,
                &self.parked,
                self.stats,
            ) else {
                return;
            };
            let fails = self.fault.0 == Some(self.reads);
            match self.read(&row) {
                Some(raw) => {
                    self.flight = Some(Flight {
                        claim: claimed,
                        row,
                        raw,
                        due: at.after(self.draw()),
                        attempt: Attempt::First,
                        fails,
                    });
                }
                // The file's bytes are gone under it: as `serve_step`'s
                // unreadable want.
                None => {
                    self.side.requeue(claimed);
                    self.stats.unreadable.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
        }
    }

    /// A latency from the model, at an xorshift quantile.
    fn draw(&mut self) -> Duration {
        let x = &mut self.rng;
        *x ^= *x << 13;
        *x ^= *x >> 17;
        *x ^= *x << 5;
        let q = if self.ceiling {
            1.0
        } else {
            *x as f32 / u32::MAX as f32
        };
        self.latency.at(q)
    }

    /// The claimed chunk's raw blocks: each piece's from its file, at its
    /// block less the file's `synthetic_lba`.
    fn read(&self, row: &HeldRead<'_>) -> Option<Vec<u8>> {
        let file = row.file();
        let r = row.read();
        let base = synthetic_lba(file.file()).get();
        let mut raw = vec![0; RAW_CHUNK_BYTES];
        let mut piece = r.first();
        while let Some(p) = piece {
            let at = p.at as usize * CHUNK;
            let out = raw.get_mut(at..at + p.blocks.get() as usize * CHUNK)?;
            let from = u64::from(p.lba.get().checked_sub(base)?) * CHUNK as u64;
            self.store.read_bytes(file.file(), from, out).ok()?;
            piece = r.after(&p);
        }
        Some(raw)
    }

    /// The read's chunk de-framed into its slot, the guard, Ready; a
    /// failing read is read again once, else requeued, the card faulted.
    fn land(&mut self, f: Flight<'r>, at: ClockUs) {
        let Flight {
            claim,
            row,
            raw,
            attempt,
            fails,
            ..
        } = f;
        if fails {
            let err = row.proto_error(ProtoError::Crc, attempt, &mut self.rot);
            match read_failed(err, self.bus, self.stats) {
                ErrorStep::Retry => {
                    self.flight = Some(Flight {
                        claim,
                        row,
                        raw,
                        due: at.after(self.draw()),
                        attempt: Attempt::Retry,
                        fails: true,
                    });
                }
                ErrorStep::Fault | ErrorStep::FrameError | ErrorStep::Rotted => {
                    self.fault = FaultAfter::NEVER;
                    self.side.requeue(claim);
                }
            }
            return;
        }
        let r = row.read();
        let Some(out) = self.side.fill_buf(&claim) else {
            // Stale: the audio cancelled it.
            let _ = self.side.fill_done(claim);
            return;
        };
        let len = r.packed().get() as usize;
        match out.get_mut(..len).map(|o| deframe(&raw, r.skip(), o)) {
            Some(Ok(_)) => {
                self.rot.cleared(row.key());
                self.side.write_guard(&claim);
                if self.side.fill_done(claim) {
                    self.reads += 1;
                }
            }
            _ => {
                let err = row.frame_error(&mut self.rot);
                read_failed(err, self.bus, self.stats);
                self.side.requeue(claim);
            }
        }
    }
}

/// The card's thread: `cs` served on wall time every half millisecond,
/// at the audio's block count `blocks`.
pub fn spawn(mut cs: CardStream<'static>, blocks: &'static AtomicU32) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("card".into())
        .spawn(move || {
            let t0 = Instant::now();
            loop {
                let at = ClockUs(t0.elapsed().as_micros() as u64);
                cs.serve(at, AudioBlocks::new(blocks.load(Ordering::Relaxed)));
                std::thread::sleep(Duration::from_micros(500));
            }
        })
        .expect("the card thread")
}
