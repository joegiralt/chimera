//! `answer`: one request in, its whole answer out, ending in one `OK` or `ERR` line.

use core::fmt::{self, Write};

use super::{
    Command, Frame, MAX_LINE, NoArg, Out, PROTOCOL, Refusal, Request, Stalled, write_shot,
    write_status,
};
use crate::perf::load::AudioStats;
use crate::ui::UiState;

#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub audio: AudioStats,
    pub loop_avg_us: u32,
    pub loop_peak_us: u32,
}

/// What a shell can tell. `None`: not in this build.
pub trait Unit {
    fn ui(&self) -> &UiState;
    /// Reading resets the loop timer.
    fn stats(&mut self) -> Option<Stats>;
    fn bench(&self) -> Option<&str>;
    fn frame(&self) -> Frame<'_>;
    /// Arms the restart, made once `OK` is out. `None`: not in this build.
    fn dfu(&mut self) -> Option<()>;
}

/// Why an answer is a single `ERR` line.
enum Why {
    Refused(Refusal),
    Absent(Command),
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Why::Refused(Refusal::Unknown(w)) => write!(f, "unknown command {w}, try help"),
            Why::Refused(Refusal::Arguments(c)) => write!(f, "{} takes {}", c.name(), c.usage()),
            Why::Refused(Refusal::TooLong) => write!(f, "line too long, {MAX_LINE} max"),
            Why::Absent(c) => write!(f, "{} is not in this build", c.name()),
        }
    }
}

/// Text straight to `out`: no response buffer. The only `fmt::Error` source is `put`,
/// so a `fmt::Error` here always means `Stalled`.
struct Text<'a, O: Out>(&'a mut O);

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
        Ok(Request::Status(NoArg)) => Ok(write_status(unit.ui(), &mut t)),
        Ok(r @ Request::Stats(NoArg)) => unit
            .stats()
            .map(|s| stats(&s, &mut t))
            .ok_or(Why::Absent(r.command())),
        Ok(r @ Request::Bench(NoArg)) => unit
            .bench()
            .map(|b| bench(b, &mut t))
            .ok_or(Why::Absent(r.command())),
        Ok(Request::Shot(c)) => return write_shot(unit.frame(), c, t.0),
        Ok(r @ Request::Dfu(NoArg)) => unit.dfu().map(Ok).ok_or(Why::Absent(r.command())),
    };
    match body {
        Ok(wrote) => wrote.and_then(|()| t.write_str("OK\n")),
        Err(why) => writeln!(t, "ERR {why}"),
    }
    .map_err(|fmt::Error| Stalled)
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
    writeln!(t, "stack_bytes {}", a.stack_used)?;
    writeln!(t, "loop_avg_us {}", s.loop_avg_us)?;
    writeln!(t, "loop_peak_us {}", s.loop_peak_us)
}

fn bench(b: &str, t: &mut impl Write) -> fmt::Result {
    t.write_str(b)?;
    if b.is_empty() || b.ends_with('\n') {
        Ok(())
    } else {
        t.write_str("\n")
    }
}
