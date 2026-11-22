//! ORBIT's clock types: the internal clock, and a MIDI clock to follow.

use super::euclid::Steps;
use crate::block::DiskCode;
use core::num::NonZeroU32;

/// A position within a bar, in 2³² ths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Phase(u32);

impl Phase {
    pub const ZERO: Phase = Phase(0);

    pub const fn new(p: u32) -> Phase {
        Phase(p)
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// As a fraction of the bar, 0..1.
    pub fn turns(self) -> f32 {
        self.0 as f32 / 4_294_967_296.0
    }
}

/// A tempo in whole BPM, 30..=300.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bpm(u16);

impl Bpm {
    pub const MIN: u16 = 30;
    pub const MAX: u16 = 300;
    pub const DEFAULT: Bpm = Bpm(120);

    pub const fn new(b: u16) -> Option<Bpm> {
        if b >= Self::MIN && b <= Self::MAX {
            Some(Bpm(b))
        } else {
            None
        }
    }

    pub const fn clamped(b: u16) -> Bpm {
        Bpm(if b < Self::MIN {
            Self::MIN
        } else if b > Self::MAX {
            Self::MAX
        } else {
            b
        })
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// A point on the song's timeline: whole bars, then a phase within the bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub bar: u32,
    pub phase: Phase,
}

impl Position {
    pub const ZERO: Position = Position {
        bar: 0,
        phase: Phase::ZERO,
    };

    /// Bars and phase as one count of 2⁻³² bars.
    pub const fn units(self) -> u64 {
        ((self.bar as u64) << 32) | self.phase.0 as u64
    }

    pub const fn from_units(u: u64) -> Position {
        Position {
            bar: (u >> 32) as u32,
            phase: Phase(u as u32),
        }
    }
}

/// Half a block (§ 6.2).
pub const WINDOW: u64 = 32;
/// The fastest external clock the advance will follow.
pub const MAX_BPM_EXT: u32 = 1200;

/// The most a window may advance, in 2⁻³² bars: what `MAX_BPM_EXT` moves in
/// a block at `sample_rate`.
pub const fn advance_cap(sample_rate: NonZeroU32) -> u64 {
    (2 * WINDOW) * ((MAX_BPM_EXT as u64) << 32) / (sample_rate.get() as u64 * 240)
}

// A window at the cap spans less than one step of the longest ring (48 kHz).
const _: () = assert!(
    (2 * WINDOW) * ((MAX_BPM_EXT as u64) << 32) / (48_000 * 240)
        < step_start(1, Steps::MAX).get() as u64
);

const BAR: u64 = 1 << 32;

/// First phase of step `k` of `steps`: ⌈k · 2³² / n⌉.
pub const fn step_start(k: u8, steps: Steps) -> Phase {
    let n = steps.get() as u64;
    Phase((k as u64 * BAR).div_ceil(n) as u32)
}

/// The step holding `phase`: ⌊phase · n / 2³²⌋.
pub const fn step_at(phase: Phase, steps: Steps) -> u8 {
    ((phase.0 as u64 * steps.get() as u64) >> 32) as u8
}

/// Samples to bars, exact rational: a bar is `sample_rate · 240 / bpm`
/// samples. Whole bars and a remainder, all in `u64`.
#[derive(Clone, Copy, Debug)]
pub struct InternalClock {
    at: u64,
    pos: Position,
    bpm: Bpm,
    sample_rate: NonZeroU32,
}

impl InternalClock {
    pub const fn start(at_sample: u64, bpm: Bpm, sample_rate: NonZeroU32) -> Self {
        InternalClock {
            at: at_sample,
            pos: Position::ZERO,
            bpm,
            sample_rate,
        }
    }

    pub fn anchor(&self) -> u64 {
        self.at
    }

    pub fn bpm(&self) -> Bpm {
        self.bpm
    }

    fn per_bar(&self) -> u64 {
        self.sample_rate.get() as u64 * 240
    }

    pub fn bar_len(&self) -> u32 {
        let b = self.bpm.0 as u64;
        ((self.per_bar() + b / 2) / b) as u32
    }

    /// Re-anchor: `pos` falls at sample `at`, at the new tempo.
    pub fn set_bpm(&mut self, at: u64, pos: Position, bpm: Bpm) {
        self.at = at;
        self.pos = pos;
        self.bpm = bpm;
    }

    /// Where the song is at `sample`. A bar line lands on the sample
    /// `sample_of` gives it, phase 0 exactly.
    pub fn position(&self, sample: u64) -> Position {
        let n = self.per_bar();
        let e = sample.saturating_sub(self.at) * self.bpm.0 as u64;
        let phase = self.pos.phase.0 as u64 + (e % n * BAR) / n;
        let mut bar = (self.pos.bar as u64 + e / n + (phase >> 32)).min(u32::MAX as u64) as u32;
        let mut phase = phase as u32;
        let line = |bar: u32| Position {
            bar,
            phase: Phase::ZERO,
        };
        if bar < u32::MAX && self.sample_of(line(bar + 1)) <= sample {
            bar += 1;
            phase = 0;
        } else if bar > self.pos.bar && self.sample_of(line(bar)) == sample {
            phase = 0;
        }
        Position {
            bar,
            phase: Phase(phase),
        }
    }

    /// The sample `pos` falls on, rounded to nearest; saturates at the anchor.
    pub fn sample_of(&self, pos: Position) -> u64 {
        if pos <= self.pos {
            return self.at;
        }
        let mut db = (pos.bar - self.pos.bar) as u64;
        let dp = if pos.phase >= self.pos.phase {
            (pos.phase.0 - self.pos.phase.0) as u64
        } else {
            db -= 1;
            BAR - (self.pos.phase.0 - pos.phase.0) as u64
        };
        let (n, b) = (self.per_bar(), self.bpm.0 as u64);
        let (q, r) = (db * n / b, db * n % b);
        let (num, den) = (r * BAR + dp * n, b * BAR);
        self.at + q + (num + den / 2) / den
    }
}

/// Which clock moves ORBIT.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClockSource {
    Internal,
    Midi,
}

/// SETTINGS › MIDI CONFIG › SYNC: who drives ORBIT's transport (ADR 0080).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SyncMode {
    /// ORBIT's own clock; MIDI clock, Start, Continue and Stop are ignored.
    Int,
    /// The DAW's clock and transport lead; silence falls back (ADR 0072).
    Ext,
}

impl SyncMode {
    pub const ALL: [SyncMode; 2] = [SyncMode::Int, SyncMode::Ext];
    /// A DAW works out of the box; with no clock ORBIT still runs free.
    pub const DEFAULT: SyncMode = SyncMode::Ext;
}

impl DiskCode for SyncMode {
    fn disk_code(self) -> u8 {
        match self {
            SyncMode::Int => 0,
            SyncMode::Ext => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            SyncMode::Int => "INT",
            SyncMode::Ext => "EXT",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(SyncMode::Int),
            1 => Some(SyncMode::Ext),
            _ => None,
        }
    }
}

/// MIDI clocks in a bar: four beats of 24 (§ 6.2).
pub const TICKS_PER_BAR: u64 = 96;
/// Tick intervals the tempo is averaged over: a beat's.
const AVERAGE: usize = 24;
/// A leader this long silent has let go, in ms.
pub const SILENT_MS: u64 = 500;

/// Where the `n`-th Clock after Start falls past the origin: `(n − 1)/96`
/// of a bar, rounded up as `step_start` rounds, so a step on a tick is
/// exactly that tick.
const fn tick_units(n: u32) -> u64 {
    let k = n.saturating_sub(1) as u64;
    ((k / TICKS_PER_BAR) << 32) + ((k % TICKS_PER_BAR) << 32).div_ceil(TICKS_PER_BAR)
}

/// An incoming MIDI clock, as a position (§ 6.1, 6.2). It follows one
/// source, the leader: the first to send a Clock or Start, until it is
/// silent `SILENT_MS`. Each Clock marks its tick; between ticks the phase
/// runs on at the last beat's average rate, never reaching the next tick.
/// Every message is O(1), and `now` is a sample count on the runner's
/// clock, at block resolution.
#[derive(Clone, Copy, Debug)]
pub struct MidiClock {
    leader: Option<u8>,
    /// The leader's last Clock, Start or Continue: silence counts from here.
    heard: u64,
    /// The leader's last Clock, for the next interval.
    last_tick: Option<u64>,
    /// The last `AVERAGE` intervals, a ring, `len` of them filled, and
    /// their sum.
    intervals: [u16; AVERAGE],
    len: u8,
    at: u8,
    sum: u32,
    /// Clocks counted since Start, or since a rebase.
    ticks: u32,
    /// Where tick 1 falls.
    origin: Position,
    /// Between Start or Continue and Stop: only then do Clocks count. True
    /// until a Stop, so a clock with no Start still moves.
    playing: bool,
    silent: u64,
    sample_rate: NonZeroU32,
    /// The beat as the reverb and the delay take it, in samples (0: none
    /// yet): the mean over the run since the tempo was last taken, not one
    /// beat's sum (#1046).
    held: u32,
    /// The run: the tick it starts on, and the intervals since.
    run_from: u64,
    run_ticks: u32,
    /// The last `off` beat sums' distance from `held`, summed: a beat's
    /// worth of them past `HOLD` on average is a tempo change.
    dev: i32,
    off: u8,
}

/// How far a beat's sum moves, on average over a beat of them, before the
/// held tempo is taken again: tick times are drained a block at a time, so
/// a steady clock's sum wanders ±1 block, a DAW's jitter on top; 1.5 blocks
/// is past their mean.
const HOLD: u32 = 3 * crate::hw::BLOCK_SIZE as u32 / 2;

impl MidiClock {
    pub const fn new(sample_rate: NonZeroU32) -> MidiClock {
        MidiClock {
            leader: None,
            heard: 0,
            last_tick: None,
            intervals: [0; AVERAGE],
            len: 0,
            at: 0,
            sum: 0,
            ticks: 0,
            origin: Position::ZERO,
            playing: true,
            silent: sample_rate.get() as u64 * SILENT_MS / 1000,
            sample_rate,
            held: 0,
            run_from: 0,
            run_ticks: 0,
            dev: 0,
            off: 0,
        }
    }

    fn quiet(&self, now: u64) -> bool {
        now.saturating_sub(self.heard) >= self.silent
    }

    /// `src` may lead: no leader, it is the leader, or the leader is silent.
    /// A new leader, or one back from silence, starts a fresh average and
    /// counts: a clock with no Start still moves ORBIT, whatever an old
    /// one's Stop left, but never starts it (ADR 0072, amended).
    fn take(&mut self, src: usize, now: u64) -> bool {
        let (src, quiet) = (src as u8, self.quiet(now));
        if self.leader.is_some_and(|l| l != src) && !quiet {
            return false;
        }
        if self.leader != Some(src) || quiet {
            self.leader = Some(src);
            self.last_tick = None;
            self.len = 0;
            self.sum = 0;
            self.playing = true;
            (self.held, self.dev, self.off) = (0, 0, 0);
        }
        self.heard = now;
        true
    }

    /// A Clock from `src` at `now`; false: `src` doesn't lead.
    pub fn tick(&mut self, src: usize, now: u64) -> bool {
        if !self.take(src, now) {
            return false;
        }
        if let Some(prev) = self.last_tick {
            let d = now.saturating_sub(prev).min(u16::MAX as u64) as u16;
            let slot = &mut self.intervals[self.at as usize];
            if self.len as usize == AVERAGE {
                self.sum -= *slot as u32;
            }
            *slot = d;
            self.sum += d as u32;
            self.at = (self.at + 1) % AVERAGE as u8;
            self.len = (self.len + 1).min(AVERAGE as u8);
            self.hold(now);
        }
        self.last_tick = Some(now);
        if self.playing {
            self.ticks = self.ticks.saturating_add(1);
        }
        true
    }

    /// The held beat (#1046): first the full average, then the mean over
    /// the whole run since, refined each time the run doubles (2, 4, 8 …
    /// beats), so a tick's jitter and its block's drain shrink with the run
    /// and the long-run tempo is exact; a tempo change, a beat of sums
    /// `HOLD` off on average, takes the tempo again from the newest beat
    /// and starts a new run. A steady clock never hunts: each refinement
    /// moves it less.
    fn hold(&mut self, now: u64) {
        if !self.settled() {
            return;
        }
        if self.held == 0 {
            self.restart(now, self.sum);
            return;
        }
        self.run_ticks += 1;
        self.dev += self.sum as i32 - self.held as i32;
        self.off += 1;
        if self.off as usize == AVERAGE {
            let mean = self.dev / AVERAGE as i32;
            (self.dev, self.off) = (0, 0);
            // The newest beat: the window's mean straddles the change.
            if mean.unsigned_abs() > HOLD {
                self.restart(now, self.sum);
                return;
            }
        }
        let beats = self.run_ticks / AVERAGE as u32;
        if self.run_ticks.is_multiple_of(AVERAGE as u32) && beats.is_power_of_two() {
            let span = now.saturating_sub(self.run_from);
            let n = u64::from(self.run_ticks);
            self.held = ((span * AVERAGE as u64 + n / 2) / n).min(u32::MAX as u64) as u32;
        }
    }

    /// The tempo taken as `beat`, a new run from the last full beat's
    /// first tick.
    fn restart(&mut self, now: u64, beat: u32) {
        self.held = beat.max(1);
        self.run_from = now.saturating_sub(u64::from(self.sum));
        self.run_ticks = AVERAGE as u32;
        (self.dev, self.off) = (0, 0);
    }

    /// The average spans a whole beat of intervals.
    pub fn settled(&self) -> bool {
        self.len as usize == AVERAGE
    }

    /// A leader heard within `SILENT_MS`, playing or stopped.
    pub fn heard(&self, now: u64) -> bool {
        self.leader.is_some() && !self.quiet(now)
    }

    /// The leader's beat in samples, held (`hold`); `None` until its
    /// average first fills.
    pub fn beat(&self) -> Option<u32> {
        (self.held != 0).then_some(self.held)
    }

    /// Start from `src`: back to bar 0, the phase held there until the
    /// first Clock, which is the downbeat. False: `src` doesn't lead.
    pub fn start(&mut self, src: usize, now: u64) -> bool {
        if !self.take(src, now) {
            return false;
        }
        self.ticks = 0;
        self.origin = Position::ZERO;
        self.playing = true;
        true
    }

    /// Continue from `src`: Clocks count on from where Stop left them.
    pub fn resume(&mut self, src: usize, now: u64) -> bool {
        let ok = self.take(src, now);
        self.playing |= ok;
        ok
    }

    /// Stop from `src`: Clocks no longer count.
    pub fn stop(&mut self, src: usize, now: u64) -> bool {
        let ok = self.take(src, now);
        self.playing &= !ok;
        ok
    }

    /// The tick count so far falls at `at` from now on: a clock taken up
    /// with no Start carries on from where ORBIT is.
    pub fn rebase(&mut self, at: Position) {
        self.origin = at;
        self.ticks = self.ticks.min(1);
    }

    /// The last beat's intervals: their count and their sum in samples;
    /// `None` before the first.
    fn measured(&self) -> Option<(u64, u64)> {
        (self.len > 0 && self.sum > 0).then_some((self.len as u64, self.sum as u64))
    }

    /// Where the clock is at `now`; `None` with no leader, after
    /// `SILENT_MS` of silence, or stopped: a stopped DAW leads nothing, so
    /// B6 plays on the internal clock (ADR 0080). Past a tick it runs on at
    /// the average rate, at least one unit (the tick has come), and stops
    /// short of the next.
    pub fn position(&self, now: u64) -> Option<Position> {
        if !self.leading(now) {
            return None;
        }
        let at = |n| self.origin.units().saturating_add(tick_units(n));
        if self.ticks == 0 {
            return Some(self.origin);
        }
        let tick = at(self.ticks);
        let next = at(self.ticks + 1);
        // A beat's samples past the tick is past the next one already:
        // clamped there, the product fits u64 (sum < 2^21, n ≤ 24).
        let run = match (self.measured(), self.last_tick) {
            (Some((n, sum)), Some(t)) => {
                (now.saturating_sub(t).min(sum) << 32) * n / (TICKS_PER_BAR * sum)
            }
            _ => 0,
        };
        Some(Position::from_units((tick + run.max(1)).min(next - 1)))
    }

    /// A leader, heard within `SILENT_MS` and playing: `position` is
    /// `Some`, without its divides.
    pub fn leading(&self, now: u64) -> bool {
        self.leader.is_some() && !self.quiet(now) && self.playing
    }

    /// A bar's samples at the averaged rate; 120 BPM's before one is known.
    pub fn bar_len(&self) -> u32 {
        match self.measured() {
            Some((n, sum)) => (sum * TICKS_PER_BAR / n).min(u32::MAX as u64) as u32,
            None => self.sample_rate.get() * 240 / Bpm::DEFAULT.0 as u32,
        }
    }

    /// The averaged tempo, rounded and held to `Bpm`'s range.
    pub fn bpm(&self) -> Bpm {
        let bar = self.bar_len().max(1) as u64;
        let b = (self.sample_rate.get() as u64 * 240 + bar / 2) / bar;
        Bpm::clamped(b.min(u16::MAX as u64) as u16)
    }
}
