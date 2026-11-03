//! The shells' pure helpers: loop timing, an answer's deadlines, the bench
//! report, the serial number.

use super::{ANSWER_MS, STALL_MS};
use chimera_hal::Ms;
use chimera_hal::units::Cycles;

/// Whether a loop iteration answered a console request.
#[must_use]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Served {
    Idle,
    Answered,
}

/// One answer's deadlines on the shell's own clock: STALL_MS without
/// progress, or ANSWER_MS in all. An `Out` takes one per answer.
#[derive(Clone, Copy, Debug)]
pub struct AnswerClock {
    start: Ms,
    last_progress: Ms,
}

impl AnswerClock {
    pub const fn start(now: Ms) -> Self {
        AnswerClock {
            start: now,
            last_progress: now,
        }
    }

    /// The host took a byte.
    pub fn progress(&mut self, now: Ms) {
        self.last_progress = now;
    }

    pub fn expired(&self, now: Ms) -> bool {
        now.since(self.last_progress) >= STALL_MS || now.since(self.start) >= ANSWER_MS
    }
}

/// Wall time in DWT cycles since boot, folded each UI lap: the shares'
/// denominator. Not SysTick's `Ms`, which a storm stalls (up to the guard's
/// `TICK_STALL_MS` a trip) while the cycles it is shared against run on.
/// SysTick only says how many of the counter's 8.9 s wraps a long lap
/// spanned.
#[derive(Clone, Copy, Debug)]
pub struct WallClock {
    dwt: Cycles,
    ms: Ms,
    total: u64,
}

impl WallClock {
    pub const fn new(dwt: Cycles, ms: Ms) -> Self {
        WallClock { dwt, ms, total: 0 }
    }

    /// The counter at `dwt` and SysTick at `ms` now (`per_ms` cycles a
    /// millisecond): the cycles since, added, the wraps SysTick's ms say
    /// passed (rounded to the nearest) with them. The total.
    pub fn fold(&mut self, dwt: Cycles, ms: Ms, per_ms: Cycles) -> u64 {
        let d = u64::from(dwt.since(self.dwt).get());
        let by_ms = u64::from(ms.since(self.ms)) * u64::from(per_ms.get());
        let wraps = (by_ms.saturating_sub(d) + (1 << 31)) >> 32;
        self.total = self.total.wrapping_add(d + (wraps << 32));
        (self.dwt, self.ms) = (dwt, ms);
        self.total
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

/// The UI loop's time between loop tops, since the last `take`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoopTimer {
    sum_us: u64,
    laps: u32,
    peak_us: u32,
}

impl LoopTimer {
    pub const fn new() -> Self {
        LoopTimer {
            sum_us: 0,
            laps: 0,
            peak_us: 0,
        }
    }

    /// One loop top `us` after the last. An iteration that answered a request is not timed.
    pub fn lap(&mut self, us: u32, served: Served) {
        if served == Served::Answered {
            return;
        }
        self.sum_us = self.sum_us.saturating_add(u64::from(us));
        self.laps = self.laps.saturating_add(1);
        self.peak_us = self.peak_us.max(us);
    }

    /// (avg, peak) in µs, then reset. (0, 0) with no laps.
    pub fn take(&mut self) -> (u32, u32) {
        let out = match self.sum_us.checked_div(u64::from(self.laps)) {
            Some(avg) => (u32::try_from(avg).unwrap_or(u32::MAX), self.peak_us),
            None => (0, 0),
        };
        *self = Self::new();
        out
    }
}

/// The USB interrupt's entries in one millisecond at most; over it, the
/// shell masks the interrupt until the UI's next lap. Provisional: set from
/// the unit's measured worst (spec § 13 ruling 5); § 13.5's 4× a dense
/// stream's peak waits on the unit.
pub const STORM_LIMIT: u32 = 200;

/// The USB interrupt's entries in the millisecond of DWT cycles it opened.
#[derive(Clone, Copy, Debug)]
pub struct StormWindow {
    start: Cycles,
    entries: u32,
}

impl StormWindow {
    pub const fn new() -> Self {
        StormWindow {
            start: Cycles::new(0),
            entries: 0,
        }
    }
}

impl Default for StormWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// One entry at `now`: true once this millisecond (`per_ms`) holds more
/// than `STORM_LIMIT` entries. Cycles, not the shell's `Ms`, which steps
/// every 2 ms and stalls while the storm holds the CPU.
pub fn storm(window: &mut StormWindow, now: Cycles, per_ms: Cycles) -> bool {
    if now.since(window.start) >= per_ms {
        *window = StormWindow {
            start: now,
            entries: 0,
        };
    }
    window.entries = window.entries.saturating_add(1);
    window.entries > STORM_LIMIT
}

/// SysTick's tick, which also kicks the 100 ms watchdog, stalled this long
/// under the USB interrupt: a storm. It steps every 2 ms: three missed.
pub const TICK_STALL_MS: u32 = 6;
/// The UI's last sign of life this long ago under the USB interrupt: a
/// storm. A lap peaks at 40 ms, and a long answer holds the cell at every
/// pump.
pub const SERVICE_STALL_MS: u32 = 250;

/// Which of the guard's tests tripped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storm {
    /// Over `STORM_LIMIT` entries in a millisecond.
    Entries,
    /// SysTick stalled over `TICK_STALL_MS`.
    Tick,
    /// The UI unseen over `SERVICE_STALL_MS`.
    Ui,
}

/// One entry at `now`: which test, if any, starves. The tick, last seen
/// moving at `tick_at`, stalled over `TICK_STALL_MS`, or the UI's last sign
/// of life, `alive`, over `SERVICE_STALL_MS` old. `None` for `alive`: none
/// yet (boot) or the UI on an errand, which never starves. Starvation
/// itself, not a CPU share: three share tests failed against audio's
/// whole-half renders. `alive` isn't kept fresh as `tick_at` is: past the
/// DWT's 8.9 s wrap with no entry it can read young, so a storm after a
/// wedged UI waits at most `SERVICE_STALL_MS` more; the tick test holds.
pub fn starved(
    now: Cycles,
    tick_at: Cycles,
    alive: Option<Cycles>,
    per_ms: Cycles,
) -> Option<Storm> {
    let over = |at: Cycles, ms: u32| now.since(at).get() > ms * per_ms.get();
    if over(tick_at, TICK_STALL_MS) {
        Some(Storm::Tick)
    } else if alive.is_some_and(|at| over(at, SERVICE_STALL_MS)) {
        Some(Storm::Ui)
    } else {
        None
    }
}

/// One entry at `now`: `storm`, then `starved`.
pub fn trip(
    window: &mut StormWindow,
    now: Cycles,
    tick_at: Cycles,
    alive: Option<Cycles>,
    per_ms: Cycles,
) -> Option<Storm> {
    if storm(window, now, per_ms) {
        Some(Storm::Entries)
    } else {
        starved(now, tick_at, alive, per_ms)
    }
}

/// The guard's trips by test, wrapping; `usb_storms` is their sum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StormKinds {
    pub entries: u32,
    pub tick: u32,
    pub ui: u32,
}

impl StormKinds {
    pub const fn new() -> Self {
        StormKinds {
            entries: 0,
            tick: 0,
            ui: 0,
        }
    }

    pub fn add(&mut self, s: Storm) {
        let n = match s {
            Storm::Entries => &mut self.entries,
            Storm::Tick => &mut self.tick,
            Storm::Ui => &mut self.ui,
        };
        *n = n.wrapping_add(1);
    }

    pub fn total(&self) -> u32 {
        self.entries.wrapping_add(self.tick).wrapping_add(self.ui)
    }
}

/// The USB RX FIFO's head entry, its status word peeked (GRXSTSR).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RxHead(pub u32);

/// The head the handler's poll was to read (`before`) is still the head
/// after it: no class took it (a full console), and RXFLVL would re-enter
/// the handler nonstop. Park it: mask RXFLVL until the UI drains the class.
/// Any other head is progress; each packet leaves two entries.
pub fn park(before: Option<RxHead>, after: Option<RxHead>) -> bool {
    before.is_some() && before == after
}

/// Report text in a fixed buffer. The last `TRUNCATED.len()` bytes are kept
/// back, so a report that runs out of room always ends in `# TRUNCATED`.
pub struct Report<const N: usize> {
    buf: [u8; N],
    len: usize,
    cut: bool,
}

impl<const N: usize> Default for Report<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Report<N> {
    pub const TRUNCATED: &'static str = "# TRUNCATED\n";

    pub const fn new() -> Self {
        const { assert!(N > Self::TRUNCATED.len()) };
        Report {
            buf: [0; N],
            len: 0,
            cut: false,
        }
    }

    pub fn heading(&mut self, title: &str) {
        self.put("# ", title);
    }

    /// One screen line; a line that won't fit cuts the report.
    pub fn line(&mut self, text: &str) {
        self.put("", text);
    }

    /// Records `text` and draws it with `draw`: the two can't drift apart.
    pub fn drawn<R>(&mut self, text: &str, draw: impl FnOnce(&str) -> R) -> R {
        self.line(text);
        draw(text)
    }

    fn put(&mut self, prefix: &str, text: &str) {
        if self.cut {
            return;
        }
        let end = self.len + prefix.len() + text.len() + 1;
        if end + Self::TRUNCATED.len() > N {
            let t = Self::TRUNCATED.as_bytes();
            if let Some(room) = self.buf.get_mut(self.len..self.len + t.len()) {
                room.copy_from_slice(t);
            }
            self.cut = true;
            return;
        }
        let mut at = self.len;
        for part in [prefix.as_bytes(), text.as_bytes(), b"\n"] {
            if let Some(room) = self.buf.get_mut(at..at + part.len()) {
                room.copy_from_slice(part);
            }
            at += part.len();
        }
        self.len = end;
    }

    pub fn as_str(&self) -> &str {
        let n = if self.cut {
            self.len + Self::TRUNCATED.len()
        } else {
            self.len
        };
        self.buf
            .get(..n)
            .and_then(|b| core::str::from_utf8(b).ok())
            .unwrap_or("")
    }
}

/// The chip's 96-bit UID as 24 uppercase hex digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialNumber([u8; 24]);

impl SerialNumber {
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.0).unwrap_or("")
    }
}

/// Hex of the UID bytes in order (the HAL's `Uid::read()` gives them so).
pub fn serial_hex(uid: &[u8; 12]) -> SerialNumber {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = [b'0'; 24];
    let nibbles = uid.iter().flat_map(|&b| [b >> 4, b & 15]);
    for (slot, nibble) in out.iter_mut().zip(nibbles) {
        *slot = DIGITS.get(usize::from(nibble)).copied().unwrap_or(b'0');
    }
    SerialNumber(out)
}
