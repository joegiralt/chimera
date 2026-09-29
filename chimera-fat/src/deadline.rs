//! One card operation's time budget, ticked once per bus transaction: an
//! idle limit that restarts whenever a block moves, and a cap that never
//! does. Pure: the shell feeds it the cycle counter, or counts
//! transactions when the counter won't run.

/// A deadline passed. Sticky until the next `arm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Over;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clock {
    /// A wrapping u32 counter, last read at `last`.
    Cycles { last: u32 },
    /// Each tick is one unit.
    Transfers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline {
    clock: Clock,
    idle: u64,
    op: u64,
    idle_max: u64,
    cap: u64,
    max_gap: u64,
    over: bool,
}

impl Deadline {
    /// Counts cycles from `now`; limits in cycles.
    pub const fn arm(now: u32, idle_cycles: u64, cap_cycles: u64) -> Self {
        Self::new(Clock::Cycles { last: now }, idle_cycles, cap_cycles)
    }

    /// Counts ticks; limits in ticks.
    pub const fn arm_transfers(idle: u64, cap: u64) -> Self {
        Self::new(Clock::Transfers, idle, cap)
    }

    /// Already over: an operation with no time, such as one on an empty
    /// slot. Its first tick fails.
    pub const fn passed() -> Self {
        let mut d = Self::new(Clock::Transfers, 0, 0);
        d.over = true;
        d
    }

    const fn new(clock: Clock, idle_max: u64, cap: u64) -> Self {
        Self {
            clock,
            idle: 0,
            op: 0,
            idle_max,
            cap,
            max_gap: 0,
            over: false,
        }
    }

    /// Advances to `now` (ignored when counting transfers). `moved_block`
    /// restarts the idle limit, not the cap.
    pub fn tick(&mut self, now: u32, moved_block: bool) -> Result<(), Over> {
        if self.over {
            return Err(Over);
        }
        let d = match &mut self.clock {
            Clock::Cycles { last } => {
                let d = now.wrapping_sub(*last);
                *last = now;
                u64::from(d)
            }
            Clock::Transfers => 1,
        };
        self.idle += d;
        self.op += d;
        if moved_block {
            if let Clock::Cycles { .. } = self.clock {
                self.max_gap = self.max_gap.max(self.idle);
            }
            self.idle = 0;
        }
        self.over = self.idle >= self.idle_max || self.op >= self.cap;
        if self.over { Err(Over) } else { Ok(()) }
    }

    pub const fn is_over(&self) -> bool {
        self.over
    }

    /// The longest idle stretch a block ended, in cycles (0 when counting
    /// transfers).
    pub const fn max_gap(&self) -> u64 {
        self.max_gap
    }
}
