//! ORBIT's runner behind bank 2's door (ADR 0208): its block, its
//! clock's real-time, a live key into capture, and its view. With `bank2`
//! and `orbit` these entries and what only they reach (the steps, MUTATE,
//! the walks, the clocks' counting) link into `.bank2` (`bank2.x`'s ORBIT
//! globs) and bank 1 keeps the proxies below, the runner's state, its
//! constructor and the default view. Elsewhere (the desktop, the tests,
//! a build without `bank2`) bank 1 calls them directly.
//!
//! Closed, ORBIT is stopped: no block sends a note or moves the clock, the
//! harmony is let go, keys and real-time pass it by, and the view says it
//! isn't running. Its scenes stay the project's, loaded and saved whole.

use super::engine::Events;
use super::runner::{OrbitRunner, OrbitView};
use crate::dsp::engines::EngineCtx;
use crate::instrument::{AudioShared, Instrument};
use crate::note_queue::{NoteEvent, RealTime};
use crate::source_table::GateView;
use crate::stream::port::{CtxBase, NoPorts, Port, Streams, VoicePorts};
use crate::stream::stats::StreamStats;
use crate::voice_alloc::VoiceIdx;
use chimera_hal::card_bus::CardFault;

/// The entries. Private, called only by the proxies below inside the
/// door's `with`, so no pointer into bank 2 leaves it.
#[repr(C)]
pub struct OrbitDoor {
    #[allow(clippy::type_complexity)]
    block: fn(
        &mut OrbitRunner,
        &mut Instrument,
        &AudioShared,
        &GateView<'_>,
        &mut OrbitPortsMut<'_, '_, '_>,
    ) -> Events,
    realtime: fn(&mut OrbitRunner, usize, RealTime, u64),
    live: fn(&mut OrbitRunner, NoteEvent),
    fill_view: fn(&OrbitRunner, &mut OrbitView),
}

impl OrbitDoor {
    pub const DOOR: OrbitDoor = OrbitDoor {
        block,
        realtime,
        live,
        fill_view,
    };
}

/// One block, every voice's stream through `ports`. One entry for both
/// port kinds: a fn pointer can't be generic, and the shipped build only
/// ever streams, so a second entry would sit in bank 2 dead.
#[inline(never)]
fn block(
    r: &mut OrbitRunner,
    inst: &mut Instrument,
    shared: &AudioShared,
    src: &GateView<'_>,
    ports: &mut OrbitPortsMut<'_, '_, '_>,
) -> Events {
    r.block_ported_in(inst, shared, src, ports)
}

/// The ports a block runs ORBIT through, as one type the door's entry
/// can name: none (`NoPorts`) or the voices' streams. The match is per
/// ORBIT note-on's port, never in the render.
pub enum OrbitPortsMut<'a, 'b, 'r> {
    None,
    Streams(&'a mut Streams<'b, 'r>),
}

impl VoicePorts for OrbitPortsMut<'_, '_, '_> {
    #[inline(always)]
    fn ctx<R>(
        &mut self,
        base: CtxBase<'_>,
        v: VoiceIdx,
        port: Port,
        f: impl FnOnce(&mut EngineCtx<'_>) -> R,
    ) -> R {
        match self {
            OrbitPortsMut::None => NoPorts.ctx(base, v, port, f),
            OrbitPortsMut::Streams(s) => s.ctx(base, v, port, f),
        }
    }

    #[inline(always)]
    fn drop_ring(&mut self, v: VoiceIdx) {
        match self {
            OrbitPortsMut::None => NoPorts.drop_ring(v),
            OrbitPortsMut::Streams(s) => s.drop_ring(v),
        }
    }

    fn stats(&self) -> Option<&StreamStats> {
        match self {
            OrbitPortsMut::None => None,
            OrbitPortsMut::Streams(s) => s.stats(),
        }
    }

    fn fault(&self) -> CardFault {
        match self {
            OrbitPortsMut::None => NoPorts.fault(),
            OrbitPortsMut::Streams(s) => s.fault(),
        }
    }
}

/// The ports a shell's block may run ORBIT through: each becomes an
/// `OrbitPortsMut`. No rings (`NoPorts`) or the voices' streams
/// (`Streams`).
pub trait OrbitPorts: VoicePorts + sealed::Sealed {
    #[doc(hidden)]
    fn as_orbit<R>(&mut self, f: impl FnOnce(&mut OrbitPortsMut<'_, '_, '_>) -> R) -> R;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for crate::stream::port::NoPorts {}
    impl Sealed for crate::stream::port::Streams<'_, '_> {}
}

impl OrbitPorts for NoPorts {
    #[inline(always)]
    fn as_orbit<R>(&mut self, f: impl FnOnce(&mut OrbitPortsMut<'_, '_, '_>) -> R) -> R {
        f(&mut OrbitPortsMut::None)
    }
}

impl OrbitPorts for Streams<'_, '_> {
    #[inline(always)]
    fn as_orbit<R>(&mut self, f: impl FnOnce(&mut OrbitPortsMut<'_, '_, '_>) -> R) -> R {
        f(&mut OrbitPortsMut::Streams(self))
    }
}

#[inline(never)]
fn realtime(r: &mut OrbitRunner, src: usize, rt: RealTime, now: u64) {
    r.realtime_in(src, rt, now)
}

#[inline(never)]
fn live(r: &mut OrbitRunner, ev: NoteEvent) {
    r.live_in(ev)
}

#[inline(never)]
fn fill_view(r: &OrbitRunner, v: &mut OrbitView) {
    r.fill_view_in(v)
}

/// `f` on the entries: through the door with `bank2` and `orbit` (`None`
/// while it is closed), else bank 1's own.
#[inline(always)]
fn through<R>(f: impl FnOnce(&OrbitDoor) -> R) -> Option<R> {
    #[cfg(all(feature = "bank2", feature = "orbit"))]
    {
        crate::bank2::DOOR_CELL.with(|d| f(&d.orbit))
    }
    #[cfg(not(all(feature = "bank2", feature = "orbit")))]
    {
        Some(f(&OrbitDoor::DOOR))
    }
}

impl OrbitRunner {
    /// A MIDI real-time message from source `src`, drained at `now` (§ 6.1).
    /// Only the leading clock's are taken, and none under SYNC INT. Start,
    /// Continue and Stop act at the next block, which sends their offs.
    #[inline(always)]
    pub fn realtime(&mut self, src: usize, rt: RealTime, now: u64) {
        through(|d| (d.realtime)(self, src, rt, now));
    }

    /// A live note, drained before it goes to the `Instrument`: into an
    /// armed capture (spec § 5.3), from any source and channel. It runs
    /// from the first key-down to the last key-up, at most `MAX_POOL`
    /// notes, each once; a STEP ring's takes one a key-down, repeats too,
    /// and ends when its hits are filled.
    #[inline(always)]
    pub fn live(&mut self, ev: NoteEvent) {
        through(|d| (d.live)(self, ev));
    }

    /// `block_with`, a streamed note starting through its voice's port
    /// from `ports`: what the firmware runs. Closed, nothing, and no
    /// harmony.
    #[inline(always)]
    pub fn block_ported<P: OrbitPorts>(
        &mut self,
        inst: &mut Instrument,
        shared: &AudioShared,
        src: &GateView<'_>,
        ports: &mut P,
    ) -> Events {
        through(|d| ports.as_orbit(|p| (d.block)(self, inst, shared, src, p))).unwrap_or_else(
            || {
                inst.set_harmony(None);
                Events::new()
            },
        )
    }

    /// The view, written into `v` in place: every field, every time, as
    /// a triple buffer's slot comes back two publishes stale. Closed, only
    /// that ORBIT isn't running.
    #[inline(always)]
    pub fn fill_view(&self, v: &mut OrbitView) {
        if through(|d| (d.fill_view)(self, v)).is_none() {
            v.running = false;
        }
    }
}
