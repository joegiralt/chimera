use crate::clock_plan::SiliconRev;
use crate::hw::BlockBudget;
use crate::note_queue::MAX_NOTE_SOURCES;

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
    pub desyncs: u32,
    pub drops: [u32; MAX_NOTE_SOURCES],
    pub sources: u8,
    pub stack_used: u32,
    pub rev: SiliconRev,
    pub cpu_hz: u32,
    window_sum: u32,
    window_len: u32,
}

impl AudioStats {
    pub const fn new(rev: SiliconRev, cpu_hz: u32) -> Self {
        Self {
            load_avg: 0,
            load_peak: 0,
            overruns: 0,
            desyncs: 0,
            drops: [0; MAX_NOTE_SOURCES],
            sources: 0,
            stack_used: 0,
            rev,
            cpu_hz,
            window_sum: 0,
            window_len: 0,
        }
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
