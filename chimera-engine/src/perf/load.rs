
use crate::board::BoardId;
use crate::clock_plan::SiliconRev;
use crate::hw::{BlockBudget, Cycles};
use crate::instrument::Instrument;
use crate::note_queue::MAX_NOTE_SOURCES;
use crate::reset::ResetCause;

pub const AVG_BLOCKS: u32 = 64;

pub fn load_percent(cycles: u32, budget: BlockBudget) -> u16 {
    let block = budget.block_cycles() as u64;
    ((cycles as u64 * 100 + block / 2) / block).min(u16::MAX as u64) as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioStats {
    pub load_avg: u16,
    pub load_peak: u16,
    pub overruns: u32,
    /// Voices the load net shed (ADR 0273).
    pub load_sheds: u32,
    pub desyncs: u32,
    /// DMA transfer errors on the trailing streams, each re-armed.
    pub dma_errors: u32,
    pub fifo_errors: u32,
    pub drops: [u32; MAX_NOTE_SOURCES],
    pub sources: u8,
    pub stack_used: u32,
    pub rev: SiliconRev,
    /// The main board, from its strap (ADR 0140).
    pub board: BoardId,
    pub cpu_hz: u32,
    /// Why the chip last reset: a watchdog reset is otherwise a silent
    /// reboot.
    pub reset: ResetCause,
    /// Voices sounding after the last block.
    pub voices: u8,
    /// The most `voices` since the console last read them.
    pub voices_peak: u8,
    /// The voice budget booked after the last block, FX bus included: the
    /// share the allocator steals and sheds against (ADR 0027).
    pub cost_pct: u8,
    window_sum: u32,
    window_len: u32,
}

impl AudioStats {
    pub const fn new(rev: SiliconRev, board: BoardId, cpu_hz: u32, reset: ResetCause) -> Self {
        Self {
            load_avg: 0,
            load_peak: 0,
            overruns: 0,
            load_sheds: 0,
            desyncs: 0,
            dma_errors: 0,
            fifo_errors: 0,
            drops: [0; MAX_NOTE_SOURCES],
            sources: 0,
            stack_used: 0,
            rev,
            board,
            cpu_hz,
            reset,
            voices: 0,
            voices_peak: 0,
            cost_pct: 0,
            window_sum: 0,
            window_len: 0,
        }
    }

    /// One drop count per note source: `drops[..sources]`, `sources` clamped to the array.
    pub fn active_drops(&self) -> &[u32] {
        &self.drops[..usize::from(self.sources).min(MAX_NOTE_SOURCES)]
    }

    /// One block's voices: `now` sounding, `cost_pct` of the budget
    /// booked. `restart`: the console read the peak, so it starts over.
    pub fn record_voices(&mut self, now: u8, cost_pct: u8, restart: bool) {
        self.voices = now;
        self.voices_peak = if restart {
            now
        } else {
            self.voices_peak.max(now)
        };
        self.cost_pct = cost_pct;
    }

    pub fn record(&mut self, cycles: u32, budget: BlockBudget) {
        let pct = load_percent(cycles, budget);
        self.load_peak = self.load_peak.max(pct);
        self.window_sum += pct as u32;
        self.window_len += 1;
        if self.window_len == AVG_BLOCKS {
            self.load_avg = (self.window_sum / AVG_BLOCKS) as u16;
            self.window_sum = 0;
            self.window_len = 0;
        }
    }
}

/// Proof the cycle counter counts (`CycleCounter::new`): the load net is
/// built only on one (ADR 0273).
#[derive(Clone, Copy, Debug)]
pub struct CycleCounter(());

impl CycleCounter {
    /// The proof, if the cycle counter `counting` moves.
    pub const fn new(counting: bool) -> Option<Self> {
        if counting { Some(Self(())) } else { None }
    }
}

/// The load net (ADR 0273): a block rendered past its `shed_at` sheds one
/// voice before the next. Built only with a `CycleCounter`, proof the DWT
/// counts; the audio interrupt calls `after` once a half.
#[derive(Clone, Copy, Debug)]
pub struct LoadNet {
    shed_at: Cycles,
}

impl LoadNet {
    pub const fn new(_counter: CycleCounter, block: BlockBudget) -> Self {
        Self {
            shed_at: block.shed_at(),
        }
    }

    /// After a block that took `took`: past `shed_at`, one sounding voice
    /// of `inst` fades out (`Instrument::shed_for_load`). True when one
    /// did: the caller counts it.
    pub fn after(self, took: Cycles, inst: &mut Instrument) -> bool {
        took > self.shed_at && inst.shed_for_load()
    }
}
