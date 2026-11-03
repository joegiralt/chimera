//! `answer`: one request in, its whole answer out, ending in one `OK` or `ERR` line.

use core::fmt::{self, Write};

use super::{
    CardAnswer, Command, Frame, MAX_LINE, NoArg, Out, PROTOCOL, Refusal, Request, Stalled,
    write_board, write_card, write_shot, write_status, write_sweep,
};
use crate::board::BoardId;
use crate::boot::{BootSeen, UsbRegs};
use crate::hw::MAX_VOICES;
use crate::perf::load::AudioStats;
use crate::ui::UiState;
use chimera_hal::store::{Files, Mark, Store, StoreError, SweepRun};
use chimera_hal::units::{Micros, Millis};

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub audio: AudioStats,
    pub loop_avg_us: u32,
    pub loop_peak_us: u32,
    /// `None`: nothing streams in this build.
    pub streams: Option<StreamTally>,
    /// `None`: no USB interrupt in this build.
    pub usb: Option<UsbTally>,
}

/// The USB interrupt's counters: its longest entry and storms since boot,
/// its share since the last `stats`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbTally {
    /// Its own time's share, the audio that preempted it taken off.
    pub irq_share: PerMille,
    /// Its longest entry, the audio taken off.
    pub irq_us_max: Micros,
    /// The storm guard's trips.
    pub storms: u32,
    /// USB MIDI event packets the host sent that were dropped: on another
    /// cable, a reserved CIN, no sinks, or the door closed.
    pub midi_in_drops: u32,
    /// USB MIDI out's losses: echo messages USB's out ring had no room
    /// for, and IN event packets the endpoint refused.
    pub midi_out_drops: u32,
}

/// The streams' and the card IRQ's counters (slice 1 § 8, § 9), since boot
/// but `irq_share`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamTally {
    pub underruns: u32,
    pub cold: u32,
    /// The cursor's tripwire (`StreamStats::unplanned`).
    pub unplanned: u32,
    /// Card read faults past their retry (`CardStats::faults`).
    pub card_faults: u32,
    pub frame_errors: u32,
    pub unreadable: u32,
    pub held_at_cap: u32,
    pub late_refused: u32,
    pub quiet_ms_max: Millis,
    pub quiets: u32,
    /// The card IRQ's longest entry, wall time.
    pub irq_us_max: Micros,
    /// Its longest less the audio that preempted it.
    pub irq_own_us_max: Micros,
    /// Its own time's share since the last `stats`.
    pub irq_share: PerMille,
}

/// A share in tenths of a percent, printed as one (`0.4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerMille(u16);

impl PerMille {
    /// `part` of `whole`, rounded, at most all of it; nothing of nothing.
    pub fn of(part: u64, whole: u64) -> PerMille {
        let part = part.min(whole);
        let pm = match part.checked_mul(1_000) {
            Some(p) => p.saturating_add(whole / 2).checked_div(whole).unwrap_or(0),
            // `whole` ≥ `part` > `u64::MAX` / 1,000: over 1,000.
            None => part / (whole / 1_000),
        };
        // At most 1,000: `part` ≤ `whole`.
        PerMille(pm as u16)
    }
}

impl fmt::Display for PerMille {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.0 / 10, self.0 % 10)
    }
}

/// `sweep`'s outcome, reached before its answer starts. Nothing sweeps in
/// the running synth: the bitmap needs the arena's gap as it is before a
/// project loads, so the mark asks the next boot (ADR 0094).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepAsk {
    /// The mark asks the next boot; the unit restarts once `OK` is out.
    Requested,
    /// The project has unsaved changes a restart would lose: nothing
    /// written.
    Unsaved,
    /// No `/CHIMERA`: nothing Chimera wrote, nothing to sweep.
    Unused,
    Failed(StoreError),
}

/// `sweep`'s request: refused over unsaved changes, else the card's mark
/// set to `Asked`. A mark it can't rewrite owes a sweep already.
pub fn ask_sweep(_: &crate::errand::Errand, unsaved: bool, s: &mut impl Store) -> SweepAsk {
    if unsaved {
        return SweepAsk::Unsaved;
    }
    match s.mount().and_then(|vol| s.request_sweep(vol)) {
        Ok(Mark::Unused) => SweepAsk::Unused,
        Ok(_) => SweepAsk::Requested,
        Err(e) => SweepAsk::Failed(e),
    }
}

/// What a shell can tell. `None`: not in this build.
pub trait Unit {
    fn ui(&self) -> &UiState;
    /// Reading resets the loop timer.
    fn stats(&mut self) -> Option<Stats>;
    fn bench(&self) -> Option<&str>;
    /// `card`'s outcome, reached before the answer starts. `None`: not in
    /// this build.
    fn card(&mut self) -> Option<CardAnswer>;
    fn frame(&self) -> Frame<'_>;
    /// Arms the restart, made once `OK` is out. `None`: not in this build.
    fn dfu(&mut self) -> Option<()>;
    /// What the chip saw at boot: `status`'s `boot` and `last_usb_regs`
    /// lines, after `at`. `None`: neither.
    fn boot(&self) -> Option<BootSeen>;
    /// The boot's card sweep, run or skipped: `status`'s `sweep` line,
    /// after `boot`. `None`: no line.
    fn sweep(&self) -> Option<SweepRun> {
        None
    }
    /// The main board from its strap: `status`'s `board` line, after
    /// `at`. `None`: no line.
    fn board(&self) -> Option<BoardId> {
        None
    }
    /// `sweep`'s request (`ask_sweep`), reached before the answer starts;
    /// on `Requested` the shell restarts once `OK` is out. `None`: not in
    /// this build.
    fn request_sweep(&mut self) -> Option<SweepAsk> {
        None
    }
    /// MIDI in's pipe: `status`'s `midi` line, after `sweep`. `None`: no
    /// line (no MIDI in).
    fn midi(&self) -> Option<super::MidiStats> {
        None
    }
    /// The OTG and PWR registers now: `status`'s `usb_regs` line, last
    /// before `OK`. `None`: no line.
    fn usb_regs(&self) -> Option<UsbRegs>;
    /// The flash controller's registers. `None`: not in this build.
    fn flash_regs(&self) -> Option<super::FlashRegs> {
        None
    }
    /// `PEEK_AT`'s words, read after `flash_regs`' lines are out.
    fn flash_peek(&self) -> Option<super::FlashPeek> {
        None
    }
    /// This build's bank-2 seal, which a staged file is judged against.
    /// `None`: unsealed, or no bank 2 in this build.
    fn bank2_seal(&self) -> Option<crate::bank2::Seal> {
        None
    }
    /// `bank2 install`'s or `bank2 erase`'s outcome, reached before the
    /// answer starts. `None`: not in this build.
    fn bank2_install(&mut self) -> Option<super::Bank2Answer> {
        None
    }
    /// The console's hands, which `key` and `turn` queue on. `None`: not
    /// in this build.
    fn hands(&mut self) -> Option<&mut super::Hands> {
        None
    }
    /// Provokes the fault, once `OK` is not yet out. `None`: not in this build.
    #[cfg(feature = "fault-inject")]
    fn fault(&mut self, _: super::FaultArg) -> Option<()> {
        None
    }
    /// The three DMA streams' registers and NDTR range. `None`: not in this build.
    #[cfg(feature = "fault-inject")]
    fn fault_dump(&mut self) -> Option<[super::StreamSnap; 3]> {
        None
    }
    /// The card behind `ls` and `put`; `NoCard` where there is none.
    type Files: Files;
    /// The card, for `ls` and `put`. `None`: not in this build.
    fn files(&mut self) -> Option<&mut Self::Files>;
    /// A Sound `put` or `rm` reached the card: the library's page goes,
    /// once the answer is out.
    fn forget_library(&mut self) {}
}

/// Why an answer is a single `ERR` line.
enum Why {
    Refused(Refusal),
    Absent(Command),
    /// Card work is pending in the UI, which a card verb waits for, or
    /// the hands' queue is full.
    Busy,
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Why::Refused(Refusal::Unknown(w)) => write!(f, "unknown command {w}, try help"),
            Why::Refused(Refusal::Arguments(c)) => write!(f, "{} takes {}", c.name(), c.usage()),
            Why::Refused(Refusal::TooLong) => write!(f, "line too long, {MAX_LINE} max"),
            Why::Absent(c) => write!(f, "{} is not in this build", c.name()),
            Why::Busy => f.write_str("busy"),
        }
    }
}

/// Text straight to `out`: no response buffer. The only `fmt::Error` source is `put`,
/// so a `fmt::Error` here always means `Stalled`.
pub(super) struct Text<'a, O: Out>(pub(super) &'a mut O);

impl<O: Out> Write for Text<'_, O> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.put(s.as_bytes()).map_err(|Stalled| fmt::Error)
    }
}

/// The whole answer, terminal line included. `Unit::stats` is read only for `stats`.
pub fn answer(
    req: Result<Request, Refusal>,
    unit: &mut impl Unit,
    out: &mut impl Out,
) -> Result<(), Stalled> {
    let mut t = Text(out);
    let body = match req {
        Err(r) => Err(Why::Refused(r)),
        Ok(Request::Help(NoArg)) => Ok(help(&mut t)),
        Ok(Request::Status(NoArg)) => Ok(write_status(unit.ui(), &mut t)
            .and_then(|()| unit.board().map_or(Ok(()), |b| write_board(b, &mut t)))
            .and_then(|()| unit.boot().map_or(Ok(()), |b| writeln!(t, "{b}")))
            .and_then(|()| unit.sweep().map_or(Ok(()), |r| write_sweep(&r, &mut t)))
            .and_then(|()| unit.midi().map_or(Ok(()), |m| writeln!(t, "{m}")))
            .and_then(|()| {
                unit.usb_regs()
                    .map_or(Ok(()), |r| writeln!(t, "usb_regs {r}"))
            })),
        Ok(r @ Request::Stats(NoArg)) => unit
            .stats()
            .map(|s| stats(&s, &mut t))
            .ok_or(Why::Absent(r.command())),
        Ok(r @ Request::Bench(NoArg)) => unit
            .bench()
            .map(|b| bench(b, &mut t))
            .ok_or(Why::Absent(r.command())),
        // Its ERR is its own line, not a `Why`: a build that never
        // measures links none of it. A stop keeps what came before.
        Ok(r @ Request::Card(_)) => match unit.card() {
            None => Err(Why::Absent(r.command())),
            Some(CardAnswer::Dma(a)) => {
                return super::write_dma_answer(&a, &mut t).map_err(|fmt::Error| Stalled);
            }
            Some(CardAnswer::Cleaned(Ok(()))) => Ok(Ok(())),
            Some(CardAnswer::Cleaned(Err(e))) => {
                return writeln!(t, "ERR card: {}", e.message()).map_err(|fmt::Error| Stalled);
            }
            Some(CardAnswer::Measured(s, fail)) => {
                let wrote = write_card(&s, &mut t);
                match fail {
                    None => Ok(wrote),
                    Some(e) => {
                        return wrote
                            .and_then(|()| writeln!(t, "ERR card: {}", e.message()))
                            .map_err(|fmt::Error| Stalled);
                    }
                }
            }
        },
        Ok(Request::Shot(c)) => return write_shot(unit.frame(), c, t.0),
        Ok(r @ Request::Dfu(NoArg)) => unit.dfu().map(Ok).ok_or(Why::Absent(r.command())),
        Ok(r @ Request::Sweep(NoArg)) => match unit.request_sweep() {
            None => Err(Why::Absent(r.command())),
            Some(SweepAsk::Requested) => {
                Ok(t.write_str("sweep asked: the next boot sweeps, restarting\n"))
            }
            Some(no) => {
                let why = match no {
                    SweepAsk::Unsaved => "unsaved changes, save first",
                    SweepAsk::Unused => "the card has no /CHIMERA, nothing to sweep",
                    SweepAsk::Failed(e) => e.message(),
                    SweepAsk::Requested => unreachable!(),
                };
                return writeln!(t, "ERR sweep: {why}").map_err(|fmt::Error| Stalled);
            }
        },
        // The registers go out before the peek: a read of bank 2 that
        // faults still leaves them on the host.
        Ok(r @ Request::Flash(NoArg)) => match unit.flash_regs() {
            None => Err(Why::Absent(r.command())),
            Some(regs) => Ok(super::write_flash_regs(&regs, &mut t).and_then(|()| {
                unit.flash_peek()
                    .map_or(Ok(()), |p| super::write_flash_peek(&p, &mut t))
            })),
        },
        Ok(r @ Request::Bank2(super::Bank2Arg::Status)) => match unit.ui().bank2() {
            None => Err(Why::Absent(r.command())),
            Some(b) => Ok(b.line(&mut t).and_then(|()| t.write_char('\n'))),
        },
        // `install` or `erase`: its whole answer, `OK` or `ERR` included,
        // is the outcome's.
        Ok(r @ Request::Bank2(_)) => match unit.bank2_install() {
            None => Err(Why::Absent(r.command())),
            Some(a) => return super::write_bank2(&a, t.0),
        },
        Ok(r @ Request::Key(k)) => queue(unit, r, super::Gesture::Key(k)),
        Ok(r @ Request::Turn(tn)) => queue(unit, r, super::Gesture::Turn(tn)),
        #[cfg(feature = "fault-inject")]
        Ok(r @ Request::Fault(super::FaultArg::Ndtr)) => unit
            .fault_dump()
            .map(|d| super::write_snaps(&d, &mut t))
            .ok_or(Why::Absent(r.command())),
        #[cfg(feature = "fault-inject")]
        Ok(r @ Request::Fault(arg)) => unit.fault(arg).map(Ok).ok_or(Why::Absent(r.command())),
        #[cfg(feature = "card-put")]
        Ok(Request::Ls(_) | Request::Put(_) | Request::Rm(_) | Request::Get(_))
            if unit.ui().card_pending() =>
        {
            Err(Why::Busy)
        }
        #[cfg(feature = "card-put")]
        Ok(r @ Request::Ls(arg)) => match unit.files() {
            None => Err(Why::Absent(r.command())),
            Some(f) => return super::ls(arg, f, t.0),
        },
        #[cfg(feature = "card-put")]
        Ok(r @ Request::Put(arg)) => {
            let open = unit.ui().project().meta().id();
            let seal = unit.bank2_seal();
            match unit.files() {
                None => Err(Why::Absent(r.command())),
                Some(f) => {
                    let said = super::put(arg, open, seal, f, t.0);
                    if matches!(arg.to, super::PutTo::Sound { .. }) {
                        unit.forget_library();
                    }
                    return said;
                }
            }
        }
        #[cfg(feature = "card-put")]
        Ok(r @ Request::Rm(arg)) => match unit.files() {
            None => Err(Why::Absent(r.command())),
            Some(f) => {
                let said = super::rm(arg, f, t.0);
                unit.forget_library();
                return said;
            }
        },
        #[cfg(feature = "card-put")]
        Ok(r @ Request::Get(arg)) => match unit.files() {
            None => Err(Why::Absent(r.command())),
            Some(f) => return super::get(arg, f, t.0),
        },
    };
    match body {
        Ok(wrote) => wrote.and_then(|()| t.write_str("OK\n")),
        Err(why) => writeln!(t, "ERR {why}"),
    }
    .map_err(|fmt::Error| Stalled)
}

/// `key` or `turn`: `OK` once queued.
fn queue(unit: &mut impl Unit, r: Request, g: super::Gesture) -> Result<fmt::Result, Why> {
    match unit.hands() {
        None => Err(Why::Absent(r.command())),
        Some(h) => h.queue(g).map(Ok).map_err(|super::Busy| Why::Busy),
    }
}

fn help(t: &mut impl Write) -> fmt::Result {
    writeln!(t, "chimera console {PROTOCOL}")?;
    Command::ALL
        .iter()
        .try_for_each(|c| writeln!(t, "{:<8}{}", c.name(), c.about()))
}

fn stats(s: &Stats, t: &mut impl Write) -> fmt::Result {
    let a = &s.audio;
    writeln!(t, "load_pct {}", a.load_avg)?;
    writeln!(t, "peak_pct {}", a.load_peak)?;
    writeln!(t, "overruns {}", a.overruns)?;
    t.write_str("drops")?;
    match a.active_drops() {
        [] => t.write_str(" -")?,
        d => d.iter().try_for_each(|n| write!(t, " {n}"))?,
    }
    writeln!(t, "\ndesyncs {}", a.desyncs)?;
    writeln!(t, "dma_errors {}", a.dma_errors)?;
    writeln!(t, "fifo_errors {}", a.fifo_errors)?;
    writeln!(t, "stack_bytes {}", a.stack_used)?;
    writeln!(t, "voices {} {} of {MAX_VOICES}", a.voices, a.voices_peak)?;
    writeln!(t, "cost_pct {}", a.cost_pct)?;
    writeln!(t, "loop_avg_us {}", s.loop_avg_us)?;
    writeln!(t, "loop_peak_us {}", s.loop_peak_us)?;
    s.streams.map_or(Ok(()), |n| streams(&n, t))?;
    s.usb.map_or(Ok(()), |u| {
        writeln!(t, "usb_irq_pct {}", u.irq_share)?;
        writeln!(t, "usb_irq_us_max {}", u.irq_us_max.get())?;
        writeln!(t, "usb_storms {}", u.storms)?;
        writeln!(t, "usb_midi_in_drops {}", u.midi_in_drops)?;
        writeln!(t, "usb_midi_out_drops {}", u.midi_out_drops)
    })
}

fn streams(n: &StreamTally, t: &mut impl Write) -> fmt::Result {
    writeln!(t, "stream_underruns {}", n.underruns)?;
    writeln!(t, "stream_cold {}", n.cold)?;
    writeln!(t, "stream_unplanned {}", n.unplanned)?;
    writeln!(t, "stream_frame_errors {}", n.frame_errors)?;
    writeln!(t, "stream_unreadable {}", n.unreadable)?;
    writeln!(t, "stream_held_at_cap {}", n.held_at_cap)?;
    writeln!(t, "stream_late_refused {}", n.late_refused)?;
    writeln!(t, "card_faults {}", n.card_faults)?;
    writeln!(t, "card_quiet_ms_max {}", n.quiet_ms_max.get())?;
    writeln!(t, "card_quiets {}", n.quiets)?;
    writeln!(t, "card_irq_us_max {}", n.irq_us_max.get())?;
    writeln!(t, "card_irq_own_us_max {}", n.irq_own_us_max.get())?;
    writeln!(t, "card_irq_pct {}", n.irq_share)
}

fn bench(b: &str, t: &mut impl Write) -> fmt::Result {
    t.write_str(b)?;
    if b.is_empty() || b.ends_with('\n') {
        Ok(())
    } else {
        t.write_str("\n")
    }
}
